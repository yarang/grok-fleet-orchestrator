//! # fleet-cli
//!
//! Grok Fleet Orchestrator의 명령줄 인터페이스.
//!
//! ## 명령
//!
//! - `fleet serve` — MCP stdio + HTTP API + (옵션) 대시보드 실행
//! - `fleet migrate` — 데이터베이스 마이그레이션만 실행
//! - `fleet workers list` / `workers show <name>` — 워커 조회
//! - `fleet tasks list` / `tasks show <id>` / `tasks cancel <id>` — 작업 관리
//! - `fleet token new` — 부트스트랩 토큰 생성
//! - `fleet doctor` — 인프라 진단 (DB 연결, 마이그레이션, 워커 상태)
//! - `fleet provision` — SSH 자동 프로비저닝
//! - `fleet scan-host-keys` — SSH 호스트 공개키 사전 수집 (`ssh-keyscan`과 동일한
//!   목적 — `--host-key-policy strict` 대규모 배포 전 known_hosts를 채운다)
//!
//! ## 환경변수
//!
//! - `DATABASE_URL` — PostgreSQL 연결 문자열 (필수)
//! - `RUST_LOG` — 로깅 레벨 (예: `info,fleet=debug`)

#![forbid(unsafe_code)]
#![allow(missing_docs)]

mod credentials;
mod doctor;
mod logging;
#[cfg(feature = "mtls")]
mod mtls;
mod runtime;
mod token;
mod users;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use std::path::PathBuf;

/// Grok Fleet Orchestrator CLI.
#[derive(Debug, Parser)]
#[command(name = "fleet", version, about, propagate_version = true)]
struct Cli {
    /// 로깅 레벨 (`RUST_LOG` 형식). 예: `info`, `debug,fleet=trace`.
    #[arg(long, env = "FLEET_LOG", default_value = "info")]
    log_level: String,

    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
#[allow(clippy::large_enum_variant)] // CLI dispatch enum — 한 번만 평가되므로 크기 영향 무시.
enum Command {
    /// MCP stdio 서버 실행. AI 클라이언트(grok build, Claude Code 등)에
    /// Fleet 도구를 노출합니다.
    Serve {
        /// `mock` (개발/테스트, 가상 워커) 또는 `acp` (실제 grok agent serve와 WebSocket 통신).
        /// `acp`는 빌드 시 `--features acp` 필요 (기본 활성화).
        #[arg(long, env = "FLEET_TRANSPORT", default_value = "mock")]
        transport: String,

        /// Postgres 최대 연결 수.
        #[arg(long, env = "FLEET_DB_MAX_CONN", default_value_t = 10)]
        db_max_conn: u32,

        /// Postgres 연결 획득 타임아웃 (초). 풀이 고갈된 채 이 시간을 넘게
        /// 대기하면 에러를 반환 (로드맵 P2 #16).
        #[arg(long, env = "FLEET_DB_ACQUIRE_TIMEOUT_SECS", default_value_t = 30)]
        db_acquire_timeout_secs: u64,

        /// Postgres 연결 최대 수명 (초). 이보다 오래된 연결은 반납 시 재사용하지
        /// 않고 닫는다 — 로드밸런서/방화벽의 장기 커넥션 강제 종료 예방.
        /// `0`이면 수명 제한 없음.
        #[arg(long, env = "FLEET_DB_MAX_LIFETIME_SECS", default_value_t = 1800)]
        db_max_lifetime_secs: u64,

        /// Postgres 유휴 연결 타임아웃 (초). 이 시간 이상 미사용 연결은 닫는다.
        /// `0`이면 유휴 타임아웃 없음.
        #[arg(long, env = "FLEET_DB_IDLE_TIMEOUT_SECS", default_value_t = 600)]
        db_idle_timeout_secs: u64,

        /// 헬스체크 비활성화 (기본값: 활성).
        #[arg(long, default_value_t = false)]
        no_health_check: bool,

        /// 헬스체크 폴링 주기 (초).
        #[arg(long, env = "FLEET_HEALTH_INTERVAL", default_value_t = 15)]
        health_interval_secs: u64,

        /// 하트비트 누락 허용 횟수. 이 횟수 × 주기를 초과하면 offline 처리.
        #[arg(long, env = "FLEET_HEALTH_MISSED", default_value_t = 3)]
        health_missed: u32,

        /// 만료 세션/오래된 로그인 시도 정리 루프 비활성화 (기본값: 활성).
        /// 로드맵 P1 #18 — 비활성화 시 `sessions`/`login_attempts` 테이블이
        /// 무한정 쌓인다.
        #[arg(long, default_value_t = false)]
        no_cleanup: bool,

        /// 정리 루프 폴링 주기 (초).
        #[arg(long, env = "FLEET_CLEANUP_INTERVAL_SECS", default_value_t = 3600)]
        cleanup_interval_secs: u64,

        /// 로그인 시도 기록(`login_attempts`) 보존 기간 (일). 이보다 오래된
        /// 기록은 정리 루프가 삭제한다.
        #[arg(long, env = "FLEET_CLEANUP_RETENTION_DAYS", default_value_t = 7)]
        cleanup_retention_days: i64,

        /// stale `Pending` 작업 재조정(reconciliation) 루프 비활성화 (기본값: 활성).
        /// 비활성화 시, `submit()` 도중 프로세스가 죽는 등의 이유로 `Pending`에
        /// 고아로 남은 작업을 아무도 재시도하지 않는다.
        #[arg(long, default_value_t = false)]
        no_reconcile: bool,

        /// 재조정 루프 폴링 주기 (초).
        #[arg(long, env = "FLEET_RECONCILE_INTERVAL_SECS", default_value_t = 30)]
        reconcile_interval_secs: u64,

        /// 이 시간(초)보다 오래 `Pending` 상태로 머문 작업만 재조정 대상으로
        /// 삼는다. 정상적으로 진행 중인 `submit()` 호출(보통 수십~수백ms)과
        /// 경합하지 않도록 dispatch 왕복 시간보다 충분히 크게 잡아야 한다.
        #[arg(long, env = "FLEET_RECONCILE_STALE_SECS", default_value_t = 60)]
        reconcile_stale_secs: u64,

        /// 이 시간(초)보다 오래 `Dispatched` 상태로 머문 작업 중 담당 워커가
        /// store에서 완전히 사라진 것만 `Failed`로 전이한다(워커 재시작으로
        /// worker_id가 바뀌어 고아가 된 작업 회수). "워커 존재 여부"라는 강한
        /// 신호에 대한 최소 유예 시간이므로 `reconcile-stale-secs`보다 짧게
        /// 잡아도 안전하다.
        #[arg(
            long,
            env = "FLEET_RECONCILE_DISPATCHED_CHECK_SECS",
            default_value_t = 30
        )]
        reconcile_dispatched_check_secs: u64,

        /// 담당 워커가 `workers` 테이블에는 남아있지만 `Offline` 상태로 이
        /// 시간(초) 이상 남아있는 `Dispatched` 작업을 `Failed`로 전이한다
        /// (2026-08-13 추가 — HealthChecker가 워커를 Offline 처리해도 Task는
        /// 건드리지 않던 빈틈을 메운다). `Offline`은 되돌릴 수 있는 상태이므로
        /// `reconcile-dispatched-check-secs`보다 훨씬 길게 잡는다.
        #[arg(
            long,
            env = "FLEET_RECONCILE_OFFLINE_WORKER_GRACE_SECS",
            default_value_t = 300
        )]
        reconcile_offline_worker_grace_secs: u64,

        /// HTTP API 바인드 주소 (예: `127.0.0.1:8081`).
        /// 생략하면 HTTP API를 실행하지 않고 MCP stdio만 서비스.
        /// 지정하면 워커 등록/하트비트 엔드포인트가 병렬로 serve됩니다.
        #[arg(long, env = "FLEET_HTTP_BIND")]
        http_bind: Option<String>,

        /// HTTP API 인증용 bearer 토큰 (쉼표 구분).
        /// 생략하면 no-auth 모드 (개발용). Phase 4에서 OIDC로 대체.
        #[arg(long, env = "FLEET_API_TOKENS")]
        api_tokens: Option<String>,

        /// Cloudflare Access Application AUD. 설정된 경우
        /// CF-Access-Jwt-Assertion 헤더 검증 활성화.
        #[arg(long, env = "FLEET_CF_AUDIENCE")]
        cf_audience: Option<String>,

        /// 웹 대시보드 바인드 주소 (예: `127.0.0.1:8082`).
        /// 생략하면 대시보드 서버를 실행하지 않습니다.
        /// 지정하면 `/api/overview`, `/api/workers`, `/api/tasks`,
        /// `/api/events/stream` (SSE) 엔드포인트가 제공됩니다.
        /// 인증은 Phase 9.1 쿠키 세션(RBAC)을 사용합니다.
        #[arg(long, env = "FLEET_DASHBOARD_BIND")]
        dashboard_bind: Option<String>,

        /// 다중 오케스트레이터 인스턴스 간 CircuitBreaker 상태 동기화
        /// 비활성화 (기본값: 활성). 로드맵 #25 — `MultiAdminSync`가
        /// 구현·테스트(`scaleout_sync.rs`)는 오래전에 끝났지만 실제 `fleet
        /// serve` 기동 경로에는 연결된 적이 없어, 스케일아웃 배포에서 한
        /// 인스턴스가 워커를 CircuitOpen 시켜도 다른 인스턴스는 자신이
        /// 별도로 실패를 겪기 전까지 이를 모르는 상태였다. 단일 인스턴스
        /// 배포에서는 자기 자신이 발행한 이벤트를 다시 받아 멱등하게
        /// 재적용할 뿐이라 켜둬도 무해하다.
        #[arg(long, default_value_t = false)]
        no_circuit_sync: bool,

        /// mTLS: 사설 CA PEM 파일 경로. orchestrator↔worker ACP 트래픽을
        /// TLS로 보호 (`--features mtls` 필요). `--mtls-cert`, `--mtls-key` 도 함께 필요.
        #[arg(
            long,
            env = "FLEET_MTLS_CA",
            requires = "mtls_cert",
            requires = "mtls_key"
        )]
        mtls_ca: Option<String>,

        /// mTLS: orchestrator 클라이언트 인증서 PEM.
        #[arg(
            long,
            env = "FLEET_MTLS_CERT",
            requires = "mtls_ca",
            requires = "mtls_key"
        )]
        mtls_cert: Option<String>,

        /// mTLS: orchestrator 클라이언트 비밀키 PEM.
        #[arg(
            long,
            env = "FLEET_MTLS_KEY",
            requires = "mtls_ca",
            requires = "mtls_cert"
        )]
        mtls_key: Option<String>,
    },

    /// 데이터베이스 마이그레이션만 실행하고 종료.
    Migrate,

    /// 워커 관련 조회 명령 그룹.
    Workers {
        #[command(subcommand)]
        action: WorkersAction,
    },

    /// 작업 관련 조회/제어 명령 그룹.
    Tasks {
        #[command(subcommand)]
        action: TasksAction,
    },

    /// 부트스트랩 토큰 관리 (워커 등록용).
    Token {
        #[command(subcommand)]
        action: TokenAction,
    },

    /// 워커 자격 증명(API 키) 중앙 관리.
    /// 마스터 키로 AES-256-GCM 암호화하여 Postgres에 저장.
    Credentials {
        #[command(subcommand)]
        action: CredentialsAction,
    },

    /// 대시보드 사용자 관리 (RBAC). 서버와 동일한 `DATABASE_URL`로
    /// Postgres에 직접 접근하여 사용자/역할/권한을 관리합니다.
    Users {
        #[command(subcommand)]
        action: UsersAction,
    },

    /// 감사 로그 (이벤트 히스토리) 조회.
    /// 모든 상태 변화는 `fleet_events` 테이블에 append-only로 기록됩니다.
    Events {
        #[command(subcommand)]
        action: EventsAction,
    },

    /// 인프라 진단. DB 연결, 마이그레이션 상태, 워커 가용성을 점검하고
    /// 보고서를 출력합니다.
    Doctor {
        /// HTTP API URL (선택). 지정된 경우 /v1/health 를 호출해 응답을 점검.
        #[arg(long, env = "FLEET_API_URL")]
        api_url: Option<String>,

        /// 대시보드 URL (선택). 지정된 경우 /health 호출.
        #[arg(long, env = "FLEET_DASHBOARD_URL")]
        dashboard_url: Option<String>,

        /// Postgres 최대 연결 수 (진단용).
        #[arg(long, default_value_t = 2)]
        db_max_conn: u32,
    },

    /// 원격 서버에 SSH로 접속해 워커 스택을 자동 프로비저닝.
    ///
    /// 단일 호스트 또는 inventory YAML 파일로 일괄 처리.
    Provision {
        /// 단일 호스트 (IP 또는 호스트명). --inventory와 배타.
        #[arg(long, conflicts_with = "inventory")]
        host: Option<String>,

        /// SSH 사용자. --host 모드에서 사용.
        #[arg(long, default_value = "ubuntu")]
        user: String,

        /// SSH 포트.
        #[arg(long, default_value_t = 22)]
        ssh_port: u16,

        /// SSH 개인키 경로.
        #[arg(long)]
        ssh_key: Option<String>,

        /// 워커 이름 (오케스트레이터에 등록될 식별자).
        #[arg(long)]
        name: Option<String>,

        /// 라벨 (key=value 반복). 예: --labels arch=arm64,gpu=false
        #[arg(long, value_delimiter = ',')]
        labels: Vec<String>,

        /// Cloudflare 토큰 (터널 생성용).
        #[arg(long, env = "FLEET_CF_TOKEN")]
        cf_token: Option<String>,

        /// 오케스트레이터 URL.
        #[arg(long, env = "FLEET_ORCHESTRATOR_URL")]
        orchestrator_url: Option<String>,

        /// 로컬 빌드한 fleet-worker 바이너리 경로.
        #[arg(long)]
        fleet_worker_bin: Option<String>,

        /// grok 서브프로세스 시크릿 (worker.toml `[grok] secret`).
        /// 인벤토리 모드에서는 per-worker로 YAML에 지정 가능.
        #[arg(long, env = "FLEET_GROK_SECRET")]
        grok_secret: Option<String>,

        /// 오케스트레이터 등록용 bootstrap bearer 토큰.
        #[arg(long, env = "FLEET_BOOTSTRAP_TOKEN")]
        bootstrap_token: Option<String>,

        /// 오케스트레이터 관리 API 용 bearer 토큰.
        /// PushCredentials 스텝이 credentials 엔드포인트 호출 시 사용.
        /// bootstrap_token 과 별개 — `/v1/workers/:name/credentials` 권한 필요.
        #[arg(long, env = "FLEET_API_TOKEN")]
        api_token: Option<String>,

        /// 인벤토리 YAML 파일 경로. --host 대신 사용.
        #[arg(long, conflicts_with = "host")]
        inventory: Option<String>,

        /// 병렬 처리 수 (인벤토리 모드).
        #[arg(long, default_value_t = 1)]
        parallel: usize,

        /// 특정 태그만 실행 (예: tunnel, setup).
        #[arg(long, value_delimiter = ',')]
        tags: Vec<String>,

        /// 인벤토리 내에서 특정 워커만 실행 (쉼표 구분 이름).
        #[arg(long, value_delimiter = ',')]
        only: Vec<String>,

        /// Dry-run — 실제 변경 없이 무엇을 할지 로깅.
        #[arg(long, default_value_t = false)]
        dry_run: bool,

        /// SSH 서버 호스트 키 검증 정책.
        ///
        /// - `accept-all`: 검증 없이 수용 (위험, 테스트 전용)
        /// - `tofu`: 첫 연결 시 known_hosts에 키 추가 후 일치 검사 (기본값)
        /// - `strict`: known_hosts에 반드시 있어야 함
        ///
        /// 인벤토리 defaults 의 `host_key_policy` 보다 우선하며,
        /// 미지정 시 인벤토리 값 → 기본값(tofu) 순으로 적용.
        #[arg(long, env = "FLEET_HOST_KEY_POLICY", value_name = "POLICY")]
        host_key_policy: Option<String>,

        /// known_hosts 파일 경로. 미지정 시 `~/.ssh/known_hosts`.
        /// 인벤토리 defaults 의 `known_hosts` 보다 우선.
        #[arg(long, env = "FLEET_KNOWN_HOSTS", value_name = "PATH")]
        known_hosts: Option<String>,

        /// mTLS: 프로비저닝하는 워커의 worker.toml 에 `[mtls]` 섹션을 포함.
        /// `--mtls-server-cert`/`--mtls-server-key`/`--mtls-client-ca` 도 함께 필요.
        #[arg(long, default_value_t = false)]
        mtls_enabled: bool,

        /// mTLS 종단 proxy 가 listen할 주소. 기본값 `0.0.0.0:2420`.
        #[arg(long)]
        mtls_listen_addr: Option<String>,

        /// 워커 측 서버 인증서 PEM 의 원격 절대경로 (예: `/etc/fleet/worker-1.pem`).
        /// 프로비저너는 파일 자체를 업로드하지 않고 worker.toml 의 경로만 채운다 —
        /// 인증서 발급은 `fleet mtls issue-server` 로 사전에 수행해 두어야 함.
        #[arg(long)]
        mtls_server_cert: Option<String>,

        /// 워커 측 서버 비밀키 PEM 의 원격 절대경로.
        #[arg(long)]
        mtls_server_key: Option<String>,

        /// 워커가 orchestrator 클라이언트 인증서 검증에 사용할 CA PEM 원격 경로.
        #[arg(long)]
        mtls_client_ca: Option<String>,

        /// orchestrator 에게 광고할 워커 호스트명. 미지정 시 `--name` 사용.
        #[arg(long)]
        mtls_advertised_host: Option<String>,

        /// orchestrator 에게 광고할 포트. 미지정 시 `--mtls-listen-addr` 포트 사용.
        #[arg(long)]
        mtls_advertised_port: Option<u16>,
    },

    /// SSH 호스트 공개키를 사전 수집 (`ssh-keyscan`과 동일한 목적, 로드맵 #39).
    ///
    /// `fleet provision --host-key-policy strict`는 known_hosts에 없는 호스트의
    /// 첫 연결을 전부 거부한다 — 대규모 배포 전 이 명령으로 각 호스트의 키를
    /// 미리 수집해, 지문(fingerprint)을 대역 밖(클라우드 콘솔, 프로비저닝 로그
    /// 등) 채널로 검증한 뒤 `--write`로 known_hosts에 반영하는 흐름을 지원한다.
    ///
    /// **주의**: `--write` 없이 실행하면 지문만 출력하고 파일에는 쓰지 않는다
    /// (기본값). 지문을 검증하지 않고 바로 `--write`하는 것은 TOFU와 동일한
    /// 신뢰 모델이라 MITM 방어 효과가 없다.
    ScanHostKeys {
        /// 단일 호스트 (IP 또는 호스트명). --inventory와 배타.
        #[arg(long, conflicts_with = "inventory")]
        host: Option<String>,

        /// SSH 포트. --host 모드에서 사용.
        #[arg(long, default_value_t = 22)]
        ssh_port: u16,

        /// 인벤토리 YAML 파일 경로 — 전체 워커 호스트를 일괄 스캔. --host 대신 사용.
        #[arg(long, conflicts_with = "host")]
        inventory: Option<String>,

        /// known_hosts 파일 경로. 미지정 시 `~/.ssh/known_hosts`.
        #[arg(long, env = "FLEET_KNOWN_HOSTS")]
        known_hosts: Option<String>,

        /// 스캔한 키를 known_hosts 파일에 실제로 추가. 기본값(false)이면
        /// 지문만 출력한다.
        #[arg(long, default_value_t = false)]
        write: bool,
    },

    /// mTLS 인증서 발급 도구 (Phase 8.5). 사설 CA + 워커 서버 인증서 +
    /// orchestrator 클라이언트 인증서를 로컬에서 생성.
    #[cfg(feature = "mtls")]
    Mtls {
        #[command(subcommand)]
        action: MtlsAction,
    },
}

#[derive(Debug, Subcommand)]
enum WorkersAction {
    /// 등록된 워커 목록을 테이블 형태로 출력.
    List {
        /// 상태 필터 (`online`, `offline`, `degraded`, `circuit_open`).
        #[arg(long)]
        status: Option<String>,

        /// JSON 형식 출력 (스크립트용).
        #[arg(long, default_value_t = false)]
        json: bool,
    },

    /// 이름으로 단일 워커 상세 조회.
    Show {
        /// 워커 이름.
        name: String,
    },
}

#[derive(Debug, Subcommand)]
enum TasksAction {
    /// 작업 목록을 최신순으로 출력.
    List {
        /// 위상 필터 (`pending`, `dispatched`, `completed`, `failed`, `cancelled`,
        /// `terminal`, `active`).
        #[arg(long)]
        status: Option<String>,

        /// 최대 출력 수.
        #[arg(long, default_value_t = 50)]
        limit: usize,

        /// JSON 형식 출력.
        #[arg(long, default_value_t = false)]
        json: bool,
    },

    /// 작업 ID로 단일 작업 상세 조회.
    Show {
        /// 작업 ID (UUID).
        id: String,
    },

    /// 실행 중인 작업을 취소 요청.
    Cancel {
        /// 작업 ID (UUID).
        id: String,

        /// 취소 사유 (기본값: "manual cancel").
        #[arg(long)]
        reason: Option<String>,
    },
}

#[derive(Debug, Subcommand)]
enum TokenAction {
    /// 무작위 부트스트랩 토큰을 생성해 stdout에 출력 (로컬 전용, DB 저장 안 함).
    /// 생성된 토큰은 `--api-tokens` (또는 `FLEET_API_TOKENS`)에 추가하여
    /// 워커 등록 인증에 사용합니다.
    ///
    /// **참고**: 영속 추적/회수가 필요하면 `token issue` 를 사용하세요.
    New {
        /// 토큰 접두어.
        #[arg(long, default_value = "fleet")]
        prefix: String,

        /// 무작위 바이트 길이 (16~64 권장).
        #[arg(long, default_value_t = 32)]
        bytes: usize,
    },

    /// 부트스트랩 토큰을 발급하여 orchestrator의 DB에 저장 (Phase 8.3).
    /// `token new`와 달리 상태를 추적할 수 있고, 사용 후 자동 소진/회수 가능.
    Issue {
        /// Orchestrator HTTP API URL.
        #[arg(long, env = "FLEET_API_URL")]
        api_url: String,

        /// Bearer 토큰 (orchestrator `--api-tokens`에 포함된 값).
        #[arg(long, env = "FLEET_API_TOKEN")]
        api_token: String,

        /// 토큰 접두어.
        #[arg(long, default_value = "fleet")]
        prefix: String,

        /// 무작위 바이트 길이.
        #[arg(long, default_value_t = 32)]
        bytes: usize,

        /// 최대 사용 횟수 (기본 1 = 일회성).
        #[arg(long, default_value_t = 1)]
        max_uses: u32,

        /// 만료까지 초. 생략하면 무기한.
        #[arg(long)]
        expires_in_secs: Option<u64>,

        /// 발급자 식별자 (감사 로그용).
        #[arg(long)]
        created_by: Option<String>,

        /// 자유 메모.
        #[arg(long)]
        notes: Option<String>,
    },

    /// 발급된 부트스트랩 토큰 목록 조회.
    List {
        /// Orchestrator HTTP API URL.
        #[arg(long, env = "FLEET_API_URL")]
        api_url: String,

        /// Bearer 토큰.
        #[arg(long, env = "FLEET_API_TOKEN")]
        api_token: String,

        /// JSON 형식 출력.
        #[arg(long, default_value_t = false)]
        json: bool,
    },

    /// 부트스트랩 토큰을 회수 (즉시 사용 불가능하게 만듦).
    Revoke {
        /// Orchestrator HTTP API URL.
        #[arg(long, env = "FLEET_API_URL")]
        api_url: String,

        /// Bearer 토큰.
        #[arg(long, env = "FLEET_API_TOKEN")]
        api_token: String,

        /// 회수할 토큰 문자열.
        token: String,
    },
}

#[derive(Debug, Subcommand)]
enum UsersAction {
    /// 등록된 사용자 목록을 테이블 형태로 출력.
    List {
        /// JSON 형식 출력 (스크립트용).
        #[arg(long, default_value_t = false)]
        json: bool,
    },

    /// 사용자 상세 조회 (역할, 권한 포함).
    Show {
        /// 사용자 이름.
        username: String,
    },

    /// 신규 사용자 생성. 비밀번호는 안전하게 프롬프트로 입력받습니다.
    Create {
        /// 사용자 이름 (`^[a-zA-Z][a-zA-Z0-9_-]{2,63}$`).
        username: String,

        /// 이메일 (선택).
        #[arg(long)]
        email: Option<String>,

        /// 역할 (기본값: viewer). 여러 번 지정 가능.
        /// builtin: admin, operator, viewer.
        #[arg(long, value_delimiter = ',')]
        roles: Option<Vec<String>>,

        /// 비밀번호를 프롬프트 대신 직접 지정 (비권장 — 셸 히스토리에 남음).
        #[arg(long)]
        password: Option<String>,
    },

    /// 비밀번호 변경. 프롬프트로 새 비밀번호를 두 번 입력받습니다.
    Passwd {
        /// 사용자 이름.
        username: String,

        /// 비밀번호를 프롬프트 대신 직접 지정 (비권장).
        #[arg(long)]
        password: Option<String>,
    },

    /// 사용자 계정 활성화.
    Enable {
        /// 사용자 이름.
        username: String,
    },

    /// 사용자 계정 비활성화 (로그인 차단, 데이터는 유지).
    Disable {
        /// 사용자 이름.
        username: String,
    },

    /// 사용자 삭제 (복구 불가).
    Delete {
        /// 사용자 이름.
        username: String,

        /// 확인 프롬프트 생략.
        #[arg(long, default_value_t = false)]
        yes: bool,
    },

    /// 역할 관리 하위 명령.
    Role {
        #[command(subcommand)]
        action: UserRoleAction,
    },

    /// 최초 관리자 등록용 OTP 부트스트랩 토큰 수동 발급.
    /// (자동 발급 외에 추가/재발급이 필요한 경우.)
    BootstrapToken {
        /// 토큰 만료까지 시간 (시간 단위, 기본 24시간).
        #[arg(long, default_value_t = 24)]
        expires_in_hours: i64,

        /// 현재 활성 토큰이 있어도 강제 재발급.
        #[arg(long, default_value_t = false)]
        force: bool,
    },
}

#[derive(Debug, Subcommand)]
enum UserRoleAction {
    /// 사용자에게 역할 부여.
    Assign {
        /// 사용자 이름.
        username: String,

        /// 역할 이름 (admin / operator / viewer 또는 custom).
        role: String,
    },

    /// 사용자의 역할 회수.
    Revoke {
        /// 사용자 이름.
        username: String,

        /// 역할 이름.
        role: String,
    },
}

#[derive(Debug, Subcommand)]
enum EventsAction {
    /// 최근 이벤트를 시간 역순으로 출력.
    List {
        /// 이 seq 이후의 이벤트만 조회 (기본값: 0 = 처음부터).
        #[arg(long, default_value_t = 0)]
        after_seq: u64,

        /// 최대 출력 수.
        #[arg(long, default_value_t = 50)]
        limit: u32,

        /// JSON 형식 출력.
        #[arg(long, default_value_t = false)]
        json: bool,
    },
}

#[derive(Debug, Subcommand)]
enum CredentialsAction {
    /// 마스터 키 초기화. 새 무작위 키를 생성해 stdout 또는 파일로 출력.
    /// 이 키를 FLEET_MASTER_KEY 환경변수 또는 /etc/fleet/master.key 로 배포.
    InitKey {
        /// 파일로 저장 (지정하지 않으면 hex 문자열을 stdout에 출력).
        #[arg(long)]
        out: Option<std::path::PathBuf>,
    },

    /// 워커에 API 키 자격 증명을 저장 (신규 또는 회전).
    Set {
        /// Orchestrator HTTP API URL.
        #[arg(long, env = "FLEET_API_URL")]
        api_url: String,

        /// Orchestrator API bearer 토큰.
        #[arg(long, env = "FLEET_API_TOKEN")]
        api_token: String,

        /// 워커 이름 (DB에 등록된 name).
        #[arg(long)]
        worker: String,

        /// grok config의 `[model.<id>]` 키.
        #[arg(long, default_value = "grok-build")]
        model_id: String,

        /// API 엔드포인트 base URL.
        #[arg(long)]
        base_url: String,

        /// 평문 API 키. 명령행 인자로 주지 않으면 stdin이나 환경변수에서 읽음.
        #[arg(long, env = "FLEET_CRED_API_KEY")]
        api_key: Option<String>,

        /// `chat_completions` 또는 `responses`.
        #[arg(long, default_value = "chat_completions")]
        api_backend: String,

        /// 컨텍스트 윈도우.
        #[arg(long, default_value_t = 200_000)]
        context_window: u32,

        /// 모델 이름 (예: `GLM-5.1`).
        #[arg(long)]
        model_name: Option<String>,
    },

    /// 워커의 자격 증명 목록을 출력 (api_key는 절대 출력하지 않음).
    List {
        /// Orchestrator HTTP API URL.
        #[arg(long, env = "FLEET_API_URL")]
        api_url: String,

        /// Orchestrator API bearer 토큰.
        #[arg(long, env = "FLEET_API_TOKEN")]
        api_token: String,

        /// 워커 이름.
        #[arg(long)]
        worker: String,

        /// JSON 형식 출력.
        #[arg(long, default_value_t = false)]
        json: bool,
    },

    /// 워커의 자격 증명을 복호화하여 내보내기.
    /// **경고**: API 키가 평문으로 출력됨. 프로비저닝 스크립트에서만 사용.
    Export {
        /// Orchestrator HTTP API URL.
        #[arg(long, env = "FLEET_API_URL")]
        api_url: String,

        /// Orchestrator API bearer 토큰.
        #[arg(long, env = "FLEET_API_TOKEN")]
        api_token: String,

        /// 워커 이름.
        #[arg(long)]
        worker: String,

        /// 모델 ID (기본값: `grok-build`).
        #[arg(long, default_value = "grok-build")]
        model_id: String,

        /// JSON 형식 출력 (기본: TOML 섹션만 stdout).
        #[arg(long, default_value_t = false)]
        json: bool,
    },

    /// 워커의 자격 증명을 제거.
    Delete {
        /// Orchestrator HTTP API URL.
        #[arg(long, env = "FLEET_API_URL")]
        api_url: String,

        /// Orchestrator API bearer 토큰.
        #[arg(long, env = "FLEET_API_TOKEN")]
        api_token: String,

        /// 워커 이름.
        #[arg(long)]
        worker: String,

        /// 모델 ID (기본값: `grok-build`).
        #[arg(long, default_value = "grok-build")]
        model_id: String,
    },
}

#[cfg(feature = "mtls")]
#[derive(Debug, Subcommand)]
pub enum MtlsAction {
    /// 사설 CA 발급 (self-signed). ca.pem + ca.key 가 out 디렉토리에 생성됨.
    InitCa {
        /// 출력 디렉토리 (존재하지 않으면 생성).
        #[arg(long)]
        out: PathBuf,

        /// Common Name (기본값: "Fleet Internal CA").
        #[arg(long, default_value = "Fleet Internal CA")]
        common_name: String,

        /// 유효기간 (일). 기본 10년.
        #[arg(long, default_value_t = 365 * 10)]
        validity_days: u64,
    },

    /// 워커 서버 인증서 발급 (CA로 서명).
    IssueServer {
        /// CA 디렉토리 (init-ca 로 만든 경로. ca.pem, ca.key 포함).
        #[arg(long)]
        ca: PathBuf,

        /// 출력 디렉토리. server.pem + server.key 생성.
        #[arg(long)]
        out: PathBuf,

        /// Common Name. 기본값 "worker".
        #[arg(long, default_value = "worker")]
        common_name: String,

        /// DNS Subject Alternative Names (쉼표 구분). 최소 1개 권장.
        /// orchestrator가 wss://<host>:<port>/... 로 접속할 때 이 값과 일치해야 함.
        #[arg(long, value_delimiter = ',')]
        dns: Vec<String>,

        /// 유효기간 (일). 기본 1년.
        #[arg(long, default_value_t = 365)]
        validity_days: u64,
    },

    /// orchestrator 클라이언트 인증서 발급 (CA로 서명).
    IssueClient {
        /// CA 디렉토리.
        #[arg(long)]
        ca: PathBuf,

        /// 출력 디렉토리. client.pem + client.key 생성.
        #[arg(long)]
        out: PathBuf,

        /// Common Name. 기본값 "orchestrator".
        #[arg(long, default_value = "orchestrator")]
        common_name: String,

        /// 유효기간 (일). 기본 1년.
        #[arg(long, default_value_t = 365)]
        validity_days: u64,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    // rustls 0.23+는 crypto backend(ring/aws-lc-rs)를 프로세스 시작 시 명시적으로
    // 설치해야 한다 — 워크스페이스 의존성 그래프에 두 백엔드가 동시에 컴파일되면
    // (예: tokio-tungstenite의 rustls-tls-webpki-roots가 끌어오는 rustls 설정이
    // 워크스페이스 루트의 `features = ["ring", ...]`와 겹치는 경우) 자동 감지가
    // 모호해져서 첫 TLS 핸드셰이크(예: AcpTransport의 wss:// 워커 접속)에서
    // "Could not automatically determine the process-level CryptoProvider" 패닉이
    // 난다. 지금까지 프로덕션의 워커 endpoint가 전부 ws://(평문)였던 탓에 이
    // 경로가 한 번도 실행되지 않아 발견되지 못했었다 — wss:// 스킴 수정 후
    // 처음으로 드러난 버그. main() 최상단에서 한 번 명시적으로 설치해 모호성을
    // 없앤다.
    #[cfg(feature = "acp")]
    rustls::crypto::ring::default_provider()
        .install_default()
        .expect(
            "failed to install rustls ring CryptoProvider — should only happen if called twice",
        );

    let cli = Cli::parse();
    logging::init(&cli.log_level);

    tracing::info!(
        version = env!("CARGO_PKG_VERSION"),
        command = ?cli.command,
        "fleet CLI starting"
    );

    match cli.command {
        Command::Serve {
            transport,
            db_max_conn,
            db_acquire_timeout_secs,
            db_max_lifetime_secs,
            db_idle_timeout_secs,
            no_health_check,
            health_interval_secs,
            health_missed,
            no_cleanup,
            cleanup_interval_secs,
            cleanup_retention_days,
            no_reconcile,
            reconcile_interval_secs,
            reconcile_stale_secs,
            reconcile_dispatched_check_secs,
            reconcile_offline_worker_grace_secs,
            http_bind,
            api_tokens,
            cf_audience,
            dashboard_bind,
            no_circuit_sync,
            mtls_ca,
            mtls_cert,
            mtls_key,
        } => {
            runtime::run_serve(
                &transport,
                db_max_conn,
                db_acquire_timeout_secs,
                db_max_lifetime_secs,
                db_idle_timeout_secs,
                no_health_check,
                health_interval_secs,
                health_missed,
                no_cleanup,
                cleanup_interval_secs,
                cleanup_retention_days,
                no_reconcile,
                reconcile_interval_secs,
                reconcile_stale_secs,
                reconcile_dispatched_check_secs,
                reconcile_offline_worker_grace_secs,
                http_bind.as_deref(),
                api_tokens.as_deref(),
                cf_audience.as_deref(),
                dashboard_bind.as_deref(),
                no_circuit_sync,
                runtime::MtlsFlags {
                    ca: mtls_ca.as_deref(),
                    cert: mtls_cert.as_deref(),
                    key: mtls_key.as_deref(),
                },
            )
            .await
        }
        Command::Migrate => runtime::run_migrate().await,
        Command::Workers { action } => runtime::run_workers(action).await,
        Command::Tasks { action } => runtime::run_tasks(action).await,
        Command::Token { action } => token::run_token(action).await,
        Command::Credentials { action } => credentials::run_credentials(action).await,
        Command::Users { action } => users::run(action).await,
        Command::Events { action } => runtime::run_events(action).await,
        #[cfg(feature = "mtls")]
        Command::Mtls { action } => mtls::run_mtls(action).await,
        Command::Doctor {
            api_url,
            dashboard_url,
            db_max_conn,
        } => doctor::run_doctor(api_url, dashboard_url, db_max_conn).await,
        Command::Provision {
            host,
            user,
            ssh_port,
            ssh_key,
            name,
            labels,
            cf_token,
            orchestrator_url,
            fleet_worker_bin,
            grok_secret,
            bootstrap_token,
            api_token,
            inventory,
            parallel,
            tags,
            only,
            dry_run,
            host_key_policy,
            known_hosts,
            mtls_enabled,
            mtls_listen_addr,
            mtls_server_cert,
            mtls_server_key,
            mtls_client_ca,
            mtls_advertised_host,
            mtls_advertised_port,
        } => {
            runtime::run_provision(runtime::ProvisionArgs {
                host,
                user,
                ssh_port,
                ssh_key,
                name,
                labels,
                cf_token,
                orchestrator_url,
                fleet_worker_bin,
                grok_secret,
                bootstrap_token,
                api_token,
                inventory,
                parallel,
                tags,
                only,
                dry_run,
                host_key_policy: host_key_policy
                    .as_deref()
                    .map(fleet_provisioner::HostKeyPolicy::parse)
                    .transpose()
                    .map_err(|e| anyhow::anyhow!(e))?,
                known_hosts: known_hosts.map(PathBuf::from),
                mtls_enabled,
                mtls_listen_addr,
                mtls_server_cert_path: mtls_server_cert,
                mtls_server_key_path: mtls_server_key,
                mtls_client_ca_path: mtls_client_ca,
                mtls_advertised_host,
                mtls_advertised_port,
            })
            .await
        }
        Command::ScanHostKeys {
            host,
            ssh_port,
            inventory,
            known_hosts,
            write,
        } => {
            runtime::run_scan_host_keys(runtime::ScanHostKeysArgs {
                host,
                ssh_port,
                inventory,
                known_hosts: known_hosts.map(PathBuf::from),
                write,
            })
            .await
        }
    }
    .context("fleet command failed")?;

    logging::shutdown();
    Ok(())
}
