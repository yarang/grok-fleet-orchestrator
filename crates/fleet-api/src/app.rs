//! axum 앱 조립 + 서버 실행.
//!
//! `AppState`는 모든 핸들러가 공유하는 의존성(Store, 인증 설정 등)을 캡슐화.
//! `build_app`는 라우터를 조립하고, `run_http_server`는 바인딩 후 serve.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;

use axum::{
    extract::Request,
    http::{header, HeaderName, HeaderValue, Method, StatusCode},
    middleware::{self, Next},
    response::Response,
    routing::{get, post},
    Router,
};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use subtle::{Choice, ConstantTimeEq};
use tower_http::cors::CorsLayer;
use tower_http::set_header::SetResponseHeaderLayer;
use tower_http::trace::TraceLayer;
use tracing::info;

use fleet_core::PermissionKind;
use fleet_credentials::MasterKey;
use fleet_store::Store;
use fleet_transport::WorkerTransport;

use crate::handlers;

/// 인증 미들웨어가 요청에 주입하는 최소 권한 컨텍스트.
///
/// handler는 raw header를 다시 해석하지 않고 이 context를 기준으로 endpoint capability를 확인한다.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthorizationContext {
    pub principal_id: String,
    pub authentication_method: AuthenticationMethod,
    pub capabilities: Vec<PermissionKind>,
    /// 요청자가 worker operational credential로 인증된 경우 해당 worker의 신원.
    ///
    /// `Some(id)`일 때만 self-binding 검사(register/heartbeat/deregister가
    /// 조작하려는 worker_id와 일치하는지)가 적용된다. `None`(admin bearer,
    /// development no-auth, CF Access)이면 기존처럼 제한 없이 통과한다.
    pub worker_id: Option<fleet_core::WorkerId>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthenticationMethod {
    ScopedBearer,
    DevelopmentNoAuth,
    /// join에서 발급된 worker operational credential(`fwo_...`)로 인증됨.
    WorkerOperational,
    /// Cloudflare Access가 서명 검증한 세션(`Cf-Access-Jwt-Assertion`)으로 인증됨.
    ///
    /// principal은 JWT의 `email` 클레임이다.
    CloudflareAccess,
}

/// 환경 manifest에 정의하는 bearer credential. token은 로그나 API 응답에 기록하지 않는다.
#[derive(Debug, Clone, Deserialize)]
pub struct ApiTokenCredential {
    pub principal_id: String,
    pub token: String,
    pub capabilities: Vec<PermissionKind>,
}

/// HTTP API 서버의 공유 상태.
pub struct AppState {
    /// Store trait 구현체 (보통 `Arc<PgStore>`).
    pub store: Arc<dyn Store>,
    /// 워커 통신 transport. `None`이면 register/deregister가 Store만 갱신
    /// (MockTransport 사용 시나 테스트에서 transport 연동이 불필요한 경우).
    pub transport: Option<Arc<dyn WorkerTransport>>,
    /// 워커에게 권장할 하트비트 주기 (초).
    pub heartbeat_interval_secs: u32,
    /// 인증 생략 여부 (개발 모드).
    pub allow_no_auth: bool,
    /// principal과 capability가 결합된 bearer credential 목록.
    pub valid_tokens: Option<Arc<Vec<ApiTokenCredential>>>,
    /// Cloudflare Access Application AUD (Phase 4).
    /// 설정된 경우 CF-Access-Jwt-Assertion 헤더의 aud 클레임과 비교.
    pub cf_audience: Option<String>,
    /// CF Access principal(JWT `email` 클레임) → capability 매핑.
    ///
    /// 키는 소문자 이메일. `None`이면 CF Access를 통과한 모든 세션이 아무
    /// capability도 갖지 못한다(fail-closed, 로드맵 `#74`) — 아래
    /// [`cf_access_capabilities`] 참조.
    pub cf_principal_capabilities: Option<Arc<HashMap<String, Vec<PermissionKind>>>>,
    /// Credentials 암호화용 마스터 키 (Phase 8.6).
    /// `None`이면 credentials API 엔드포인트가 503 반환.
    pub master_key: Option<Arc<MasterKey>>,
    /// CORS 허용 출처 목록 (명시적 allow-list).
    ///
    /// 비어 있으면 CORS 응답 헤더를 전혀 내보내지 않음(브라우저 교차 출처 차단).
    /// fleet-api의 정상 소비자는 fleet-cli / fleet-worker / fleet-mcp 같은
    /// 비브라우저 클라이언트이므로 기본값(빈 목록)이 올바른 배포 형태.
    /// 브라우저 기반 외부 콘솔을 붙이는 경우에만 출처를 명시적으로 추가.
    pub cors_allowed_origins: Vec<String>,
    /// HTTP 요청 지연 히스토그램 (인프로세스 누산).
    ///
    /// 다른 메트릭과 달리 스토어에서 계산할 수 없어 미들웨어가 요청마다
    /// 기록한다. 프로세스 재시작 시 0으로 돌아간다.
    pub http_metrics: Arc<crate::metrics::HttpMetrics>,
    /// 이 orchestrator를 외부에서 접근하는 공개 base URL(예:
    /// `https://fleet.agentthread.dev`). `FLEET_BASE_URL` env로 설정.
    ///
    /// join 응답이 렌더링하는 `worker.toml`의 `orchestrator_url` 값으로 쓰인다
    /// — 설정하지 않으면 그 필드는 플레이스홀더(`<set-to-your-orchestrator-url>`)로
    /// 남아 운영자가 수동으로 채워야 한다.
    pub public_base_url: Option<String>,
}

impl AppState {
    pub fn new(store: Arc<dyn Store>) -> Self {
        Self {
            store,
            transport: None,
            heartbeat_interval_secs: 15,
            allow_no_auth: true,
            valid_tokens: None,
            cf_audience: None,
            cf_principal_capabilities: None,
            master_key: None,
            cors_allowed_origins: Vec::new(),
            http_metrics: Arc::new(crate::metrics::HttpMetrics::new()),
            public_base_url: None,
        }
    }

    pub fn with_heartbeat_interval(mut self, secs: u32) -> Self {
        self.heartbeat_interval_secs = secs;
        self
    }

    /// Worker transport 연결. 설정 시 register/deregister가 transport도 갱신.
    pub fn with_transport(mut self, transport: Arc<dyn WorkerTransport>) -> Self {
        self.transport = Some(transport);
        self
    }

    pub fn with_tokens(mut self, tokens: Vec<ApiTokenCredential>) -> Self {
        self.valid_tokens = Some(Arc::new(tokens));
        self.allow_no_auth = false;
        self
    }

    /// Cloudflare Access AUD 설정. 이후 모든 보호된 엔드포인트는
    /// 유효한 CF-Access-Jwt-Assertion 헤더를 요구.
    pub fn with_cf_audience(mut self, aud: impl Into<String>) -> Self {
        self.cf_audience = Some(aud.into());
        self.allow_no_auth = false;
        self
    }

    /// CF Access principal별 capability allow-list 설정 (로드맵 `#58`, `#74`).
    ///
    /// 이메일은 대소문자를 구분하지 않도록 소문자로 정규화해서 보관한다.
    /// 매핑을 설정하면 **열거되지 않은 이메일은 capability가 없다**
    /// (fail-closed). 이 빌더를 아예 호출하지 않은 배포도 마찬가지로
    /// fail-closed다(전체 capability를 받던 과거 동작은 로드맵 `#74`로
    /// 제거됨) — [`cf_access_capabilities`] 참조.
    pub fn with_cf_principal_capabilities(
        mut self,
        capabilities: impl IntoIterator<Item = (String, Vec<PermissionKind>)>,
    ) -> Self {
        let map: HashMap<String, Vec<PermissionKind>> = capabilities
            .into_iter()
            .map(|(email, caps)| (email.trim().to_ascii_lowercase(), caps))
            .collect();
        self.cf_principal_capabilities = Some(Arc::new(map));
        self
    }

    /// Credentials 암호화 마스터 키 설정. 설정하지 않으면 credentials API
    /// 엔드포인트가 503 Service Unavailable 반환.
    pub fn with_master_key(mut self, key: MasterKey) -> Self {
        self.master_key = Some(Arc::new(key));
        self
    }

    /// CORS 허용 출처 allow-list 설정 (예: `https://console.example.com`).
    ///
    /// 스킴+호스트(+포트)까지 정확히 일치해야 함. 빈 목록이면 CORS 비활성.
    /// 와일드카드(`*`)는 의도적으로 지원하지 않음 — 인증된 API에서
    /// 모든 출처를 허용하면 CSRF/데이터 유출 경로가 열림.
    pub fn with_cors_origins(mut self, origins: Vec<String>) -> Self {
        self.cors_allowed_origins = origins;
        self
    }

    /// 이 orchestrator의 공개 base URL 설정 (`FLEET_BASE_URL`). join 응답의
    /// `worker.toml`에 `orchestrator_url`로 그대로 채워진다.
    pub fn with_public_base_url(mut self, url: impl Into<String>) -> Self {
        self.public_base_url = Some(url.into());
        self
    }
}

/// 전체 라우터를 조립. 라우트 구조:
///
/// ```text
/// /
/// ├── health                      → GET /v1/health
/// └── v1
///     └── workers
///         ├── register            → POST
///         ├── heartbeat           → POST
///         ├──                     → GET (list)
///         └── :id                 → GET / DELETE
/// ```
pub fn build_app(state: Arc<AppState>) -> Router {
    let api_routes = Router::new()
        .route("/register", post(handlers::register_worker))
        .route("/join", post(handlers::join_worker))
        .route("/heartbeat", post(handlers::heartbeat))
        .route("/", get(handlers::list_workers))
        .route(
            "/:id",
            get(handlers::get_worker).delete(handlers::deregister_worker),
        )
        // Worker operational credential rotate/revoke (로드맵 #60 6단계). LLM
        // model credential 하위 자원(`/:name/credentials`, 복수형)과는 별개의
        // 단수형 경로 — worker 자신을 인증하는 데 쓰는 단일 credential이다.
        .route(
            "/:id/credential/rotate",
            post(handlers::rotate_worker_credential),
        )
        .route(
            "/:id/credential",
            axum::routing::delete(handlers::revoke_worker_credential),
        );

    // Phase 8.6: worker credentials (sub-resource under /v1/workers/:name/credentials).
    // PUT  /v1/workers/:name/credentials                       — set/rotate
    // GET  /v1/workers/:name/credentials                       — list (metadata only)
    // GET  /v1/workers/:name/credentials/:model_id/export      — decrypted (admin)
    // DELETE /v1/workers/:name/credentials/:model_id           — remove
    let cred_routes = Router::new()
        .route(
            "/",
            axum::routing::put(handlers::put_worker_credential)
                .get(handlers::list_worker_credentials),
        )
        .route("/:model_id/export", get(handlers::export_worker_credential))
        .route(
            "/:model_id",
            axum::routing::delete(handlers::delete_worker_credential),
        );

    // /v1/workers/:name/credentials/* 라우팅을 위해 api_routes에 nest.
    // axum에서는 /:name 하위에 또다른 /:model_id를 두려면 별도 nest가 필요.
    let api_routes = api_routes.nest("/:name/credentials", cred_routes);

    let token_routes = Router::new()
        .route(
            "/",
            post(handlers::create_bootstrap_token).get(handlers::list_bootstrap_tokens),
        )
        .route(
            "/:token",
            axum::routing::delete(handlers::revoke_bootstrap_token),
        );

    // Admin API bearer token rotate/revoke (로드맵 #72). bootstrap token
    // (worker join 전용)과는 별개 자원 — `admin_token:manage`/`admin_token:list`
    // capability로 통제한다.
    let admin_token_routes = Router::new()
        .route(
            "/",
            post(handlers::create_admin_token).get(handlers::list_admin_tokens),
        )
        .route("/:principal_id/rotate", post(handlers::rotate_admin_token))
        .route(
            "/:principal_id",
            axum::routing::delete(handlers::revoke_admin_token),
        );

    let v1 = Router::new()
        .route("/health", get(handlers::health))
        .route("/hosts/register", post(handlers::register_host))
        .nest("/workers", api_routes)
        .nest("/bootstrap-tokens", token_routes)
        .nest("/admin/tokens", admin_token_routes);

    // 인증 미들웨어 순서 (로드맵 #58).
    //
    // tower/axum에서 나중에 붙인 `layer`가 더 바깥이라 **먼저** 실행된다.
    // 따라서 아래 순서는 실행 순서로 "CF Access → auth_middleware"가 된다.
    //
    // 이 순서는 필수다: CF Access 미들웨어가 JWT 서명을 검증한 뒤
    // `VerifiedUser`를 request extension에 넣어야, `auth_middleware`가 그
    // principal로 `AuthorizationContext`를 구성하고 capability를 강제할 수
    // 있다. 반대 순서였을 때는 CF Access 전용 배포에서 auth_middleware가
    // principal을 전혀 볼 수 없어 `authorize_http_endpoint`가 한 번도
    // 호출되지 않았다.
    let state_for_auth = state.clone();
    let v1 = v1.layer(middleware::from_fn(move |req, next| {
        let state = state_for_auth.clone();
        async move { auth_middleware(state, req, next).await }
    }));

    // Cloudflare Access 미들웨어 (가장 바깥 = 가장 먼저 실행).
    // 설정된 경우 모든 요청이 CF-Access-Jwt-Assertion 검증을 받음.
    let state_for_cf = state.clone();
    let v1 = if state.cf_audience.is_some() {
        v1.layer(middleware::from_fn(move |req, next| {
            let state = state_for_cf.clone();
            async move { crate::cloudflare::cloudflare_access_middleware(state, req, next).await }
        }))
    } else {
        v1
    };

    Router::new()
        .nest("/v1", v1)
        // /metrics는 인증 미들웨어 바깥에 위치 (Prometheus 스크랩 표준).
        // 단, CF Access가 켜져 있다면 외부망에서는 여전히 CF 토큰 검증을 받음.
        .route(
            "/metrics",
            get({
                let state = state.clone();
                move || {
                    let state = state.clone();
                    async move { crate::metrics::metrics_handler(state).await }
                }
            }),
        )
        // OpenAPI 스펙 (로드맵 #21) — /metrics와 동일한 이유로 인증 미들웨어
        // 바깥: API 스펙 자체는 비밀이 아니고, 클라이언트 도구(Swagger UI 등)가
        // 토큰 없이 바로 불러올 수 있어야 발견성이 생긴다.
        .route("/openapi.yaml", get(openapi_spec))
        // HTTP 지연 기록 — 모든 라우트를 감싼다. 인증 미들웨어보다 바깥이라
        // 인증 실패로 거부된 요청의 지연도 함께 관측된다(부하 판단에 필요).
        .layer({
            let metrics = state.http_metrics.clone();
            middleware::from_fn(move |req, next: middleware::Next| {
                let metrics = metrics.clone();
                async move {
                    let started = std::time::Instant::now();
                    let response = next.run(req).await;
                    metrics.observe(started.elapsed());
                    response
                }
            })
        })
        // 보안 헤더 — 모든 응답에 적용 (이미 설정되지 않은 경우만).
        // fleet-api는 순수 JSON API이므로 CSP는 `default-src 'none'`으로 최소화.
        .layer(SetResponseHeaderLayer::if_not_present(
            HeaderName::from_static("content-security-policy"),
            HeaderValue::from_static(API_CSP),
        ))
        .layer(SetResponseHeaderLayer::if_not_present(
            HeaderName::from_static("x-frame-options"),
            HeaderValue::from_static("DENY"),
        ))
        .layer(SetResponseHeaderLayer::if_not_present(
            HeaderName::from_static("x-content-type-options"),
            HeaderValue::from_static("nosniff"),
        ))
        // HSTS: 평문 HTTP 응답에서는 브라우저가 무시하므로 로컬 개발에 무해.
        .layer(SetResponseHeaderLayer::if_not_present(
            HeaderName::from_static("strict-transport-security"),
            HeaderValue::from_static(API_HSTS),
        ))
        .layer(SetResponseHeaderLayer::if_not_present(
            HeaderName::from_static("referrer-policy"),
            HeaderValue::from_static("no-referrer"),
        ))
        .layer(TraceLayer::new_for_http())
        // CORS — 명시적 allow-list. 기본값(빈 목록)은 CORS 헤더를 내보내지 않아
        // 브라우저 교차 출처 요청이 차단됨 (permissive 제거).
        .layer(cors_layer(&state.cors_allowed_origins))
        .with_state(state)
}

/// `/v1` HTTP API의 OpenAPI 3.0.3 스펙 (로드맵 #21). `schema.rs`/`handlers.rs`
/// 기준으로 수기 작성 — 코드가 바뀌면 이 파일도 함께 갱신해야 한다(자동
/// 생성 아님). 대시보드 API(`/api/*`, `fleet-dashboard` 크레이트)는 별도
/// 크레이트이자 훨씬 큰 표면(~30개 라우트)이라 이 스펙의 범위 밖이다.
const OPENAPI_YAML: &str = include_str!("openapi.yaml");

/// `GET /openapi.yaml` 핸들러. `/metrics`와 동일하게 인증 미들웨어 바깥에
/// 등록되어 토큰 없이도 조회 가능.
async fn openapi_spec() -> impl axum::response::IntoResponse {
    (
        [(header::CONTENT_TYPE, "application/yaml; charset=utf-8")],
        OPENAPI_YAML,
    )
}

/// JSON 전용 API용 CSP — 스크립트/이미지/프레임 등 모든 서브리소스 로드 금지.
const API_CSP: &str =
    "default-src 'none'; frame-ancestors 'none'; base-uri 'none'; form-action 'none'";

/// HSTS 2년 + 서브도메인 + preload.
const API_HSTS: &str = "max-age=63072000; includeSubDomains; preload";

/// 설정된 allow-list로 `CorsLayer` 구성.
///
/// - 빈 목록 → `CorsLayer::new()` (Access-Control-* 헤더 없음 = 교차 출처 차단)
/// - 와일드카드(`*`)와 파싱 불가한 출처는 경고 로그와 함께 무시 (fail-closed)
/// - `allow_credentials`는 켜지 않음 — fleet-api는 쿠키가 아닌
///   Authorization 헤더 기반 인증이므로 자격 증명 포함 요청이 필요 없음
fn cors_layer(origins: &[String]) -> CorsLayer {
    let mut parsed: Vec<HeaderValue> = Vec::new();
    for origin in origins {
        let trimmed = origin.trim();
        if trimmed.is_empty() {
            continue;
        }
        if trimmed == "*" {
            tracing::warn!("CORS wildcard '*' rejected for an authenticated API — ignored");
            continue;
        }
        // Origin 헤더는 정확히 `scheme://host[:port]` 형태. 경로/트레일링 슬래시가
        // 붙으면 어떤 요청과도 매칭되지 않아 조용한 오설정이 되므로 미리 거부.
        let looks_like_origin = (trimmed.starts_with("https://") || trimmed.starts_with("http://"))
            && !trimmed.ends_with('/')
            && trimmed.matches('/').count() == 2;
        if !looks_like_origin {
            tracing::warn!(
                origin = %trimmed,
                "CORS origin must be exactly 'scheme://host[:port]' — ignored"
            );
            continue;
        }
        match HeaderValue::from_str(trimmed) {
            Ok(value) => parsed.push(value),
            Err(e) => {
                tracing::warn!(origin = %trimmed, error = %e, "invalid CORS origin — ignored");
            }
        }
    }

    if parsed.is_empty() {
        return CorsLayer::new();
    }

    info!(
        count = parsed.len(),
        "CORS enabled with explicit origin allow-list"
    );
    CorsLayer::new()
        .allow_origin(parsed)
        .allow_methods([Method::GET, Method::POST, Method::PUT, Method::DELETE])
        .allow_headers([header::AUTHORIZATION, header::CONTENT_TYPE])
}

/// Bearer token 인증 미들웨어.
///
/// - `allow_no_auth == true`면 통과
/// - Cloudflare Access가 설정된 경우 bearer 없이 CF middleware의 인증에 맡김
/// - 그 외 `valid_tokens == None`이면 fail-closed
/// - 그 외에는 `Authorization: Bearer <token>` 헤더가 `valid_tokens`에 있어야 함
async fn auth_middleware(
    state: Arc<AppState>,
    req: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    if state.allow_no_auth {
        let mut req = req;
        req.extensions_mut().insert(AuthorizationContext {
            principal_id: "development".into(),
            authentication_method: AuthenticationMethod::DevelopmentNoAuth,
            capabilities: PermissionKind::all().to_vec(),
            worker_id: None,
        });
        authorize_http_endpoint(&req)?;
        return Ok(next.run(req).await);
    }

    // health 엔드포인트는 인증 provider 구성 여부와 관계없이 LB 프로브에 허용한다.
    if normalized_v1_path(req.uri().path()) == "/health" {
        return Ok(next.run(req).await);
    }

    // join은 자체적으로 body의 bootstrap token을 검증한다(join_worker 핸들러,
    // enroll_worker 경유) — Authorization 헤더에 의존하지 않는다. `fleet-worker
    // join` CLI는 애초에 이 헤더를 보내지 않으므로(worker-enrollment.md 참고),
    // valid_tokens/cf_audience가 설정된 배포에서 아래 일반 분기를 그대로
    // 통과시키면 join이 핸들러에 도달하기도 전에 401로 막힌다. join 요청 본문의
    // token 자체가 인증 수단이므로 여기서 우회해도 보안 경계가 약해지지 않는다 —
    // join_worker는 AuthorizationContext를 요구하지 않는다.
    if req.method() == axum::http::Method::POST
        && normalized_v1_path(req.uri().path()) == "/workers/join"
    {
        return Ok(next.run(req).await);
    }

    // Worker operational credential 조회 — join에서 발급된 `fwo_...` 토큰인지
    // 먼저 확인한다. `valid_tokens`/`cf_audience` 설정 여부와 무관하게 동작해야
    // 하므로(워커는 admin bearer allow-list에 없다) 아래 관리자 bearer 분기보다
    // 앞서 검사한다. 매치되지 않으면 조용히 기존 로직으로 폴백한다.
    if let Some(header) = req
        .headers()
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
    {
        if let Some(token) = header
            .strip_prefix("Bearer ")
            .or_else(|| header.strip_prefix("bearer "))
        {
            let digest = fleet_core::BootstrapToken::digest_for(token);
            if let Ok(Some(credential)) = state
                .store
                .find_active_worker_operational_credential(&digest)
                .await
            {
                let mut req = req;
                req.extensions_mut().insert(AuthorizationContext {
                    principal_id: format!("worker:{}", credential.worker_id),
                    authentication_method: AuthenticationMethod::WorkerOperational,
                    capabilities: vec![
                        PermissionKind::WorkerRegister,
                        PermissionKind::WorkerDelete,
                    ],
                    worker_id: Some(credential.worker_id),
                });
                authorize_http_endpoint(&req)?;
                return Ok(next.run(req).await);
            }
        }
    }

    // Admin API bearer token 조회 — DB(`admin_api_tokens`)에 발급된 토큰인지
    // 확인한다 (로드맵 #72). env `valid_tokens`(정적 목록)를 보완하는 두 번째
    // 소스이며, `valid_tokens`/`cf_audience` 설정 여부와 무관하게 동작해야
    // 하므로(DB 전용 배포도 지원) 아래 env allow-list 분기보다 앞서 검사한다.
    // 매치되지 않으면 조용히 기존 로직으로 폴백한다.
    if let Some(header) = req
        .headers()
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
    {
        if let Some(token) = header
            .strip_prefix("Bearer ")
            .or_else(|| header.strip_prefix("bearer "))
        {
            let presented_digest = fleet_core::BootstrapToken::digest_for(token);
            if let Ok(Some(admin_token)) = state
                .store
                .find_active_admin_token_by_digest(&presented_digest)
                .await
            {
                // WHERE token_digest = $1로 이미 정확히 일치한 row지만,
                // 상수시간 비교 관례(env allow-list의 `token_matches`와 동일한
                // 방어 심도)를 유지하기 위해 반환된 digest를 다시 한 번 ct_eq로
                // 확정한다. 두 값 모두 SHA-256 hex(고정 64자)라 길이 누출도 없다.
                let matches = admin_token
                    .token_digest
                    .as_bytes()
                    .ct_eq(presented_digest.as_bytes());
                if bool::from(matches) {
                    let mut req = req;
                    req.extensions_mut().insert(AuthorizationContext {
                        principal_id: admin_token.principal_id.clone(),
                        authentication_method: AuthenticationMethod::ScopedBearer,
                        capabilities: admin_token.capabilities.clone(),
                        worker_id: None,
                    });
                    authorize_http_endpoint(&req)?;
                    return Ok(next.run(req).await);
                }
            }
        }
    }

    let Some(tokens) = &state.valid_tokens else {
        // CF Access 전용 배포. 서명 검증 자체는 바깥의 CF middleware가 이미
        // 끝냈고, 여기서는 그 결과(VerifiedUser)를 principal로 승격시켜
        // capability를 강제한다 (로드맵 #58).
        if state
            .cf_audience
            .as_deref()
            .is_some_and(|audience| !audience.trim().is_empty())
        {
            let Some(user) = req
                .extensions()
                .get::<crate::cloudflare::VerifiedUser>()
                .cloned()
            else {
                // CF middleware가 통과시켰는데 principal이 없다면 미들웨어
                // 구성이 깨진 것이다. 무인증 통과 대신 fail-closed.
                tracing::error!(
                    path = %req.uri().path(),
                    "cf_audience is configured but no verified CF Access principal was attached"
                );
                return Err(StatusCode::UNAUTHORIZED);
            };
            let capabilities = cf_access_capabilities(&state, &user.email);
            let mut req = req;
            req.extensions_mut().insert(AuthorizationContext {
                principal_id: cf_access_principal_id(&user),
                authentication_method: AuthenticationMethod::CloudflareAccess,
                capabilities,
                worker_id: None,
            });
            authorize_http_endpoint(&req)?;
            return Ok(next.run(req).await);
        }
        tracing::error!(path = %req.uri().path(), "protected API has no authentication provider");
        return Err(StatusCode::UNAUTHORIZED);
    };

    let auth_header = req
        .headers()
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok());

    let Some(header) = auth_header else {
        tracing::warn!(path = %req.uri().path(), "missing Authorization header");
        return Err(StatusCode::UNAUTHORIZED);
    };

    let token = header
        .strip_prefix("Bearer ")
        .or_else(|| header.strip_prefix("bearer "));

    let Some(token) = token else {
        tracing::warn!(path = %req.uri().path(), "malformed Authorization header");
        return Err(StatusCode::UNAUTHORIZED);
    };

    if let Some(credential) = token_matches(tokens, token) {
        let mut req = req;
        req.extensions_mut().insert(AuthorizationContext {
            principal_id: credential.principal_id.clone(),
            authentication_method: AuthenticationMethod::ScopedBearer,
            capabilities: credential.capabilities.clone(),
            worker_id: None,
        });
        authorize_http_endpoint(&req)?;
        Ok(next.run(req).await)
    } else {
        tracing::warn!(path = %req.uri().path(), "invalid bearer token");
        Err(StatusCode::UNAUTHORIZED)
    }
}

/// CF Access 세션의 principal 식별자.
///
/// `email` 클레임이 있으면 그대로 쓴다(감사 로그와 향후 매핑의 기준값).
/// service token처럼 email이 없는 세션은 개별 주체를 특정할 수 없으므로
/// audience 기반 식별자를 부여해 최소한 사람 계정과는 구분되게 한다.
fn cf_access_principal_id(user: &crate::cloudflare::VerifiedUser) -> String {
    if user.email.trim().is_empty() {
        format!("cf-access:aud:{}", user.audience)
    } else {
        user.email.clone()
    }
}

/// CF Access principal에게 부여할 capability.
///
/// **Fail-closed** (로드맵 `#74`) — 매핑이 아예 설정되지 않은 배포에서도
/// 빈 capability를 반환한다. `cf_principal_capabilities`가 `None`이면
/// CF Access를 통과한 어떤 세션도 아무 capability를 갖지 못한다(`/health`
/// 같은 인증 예외 경로만 여전히 접근 가능). 매핑이 설정된 경우에는
/// 열거되지 않은 이메일에 capability를 주지 않는다 — 매핑을 명시한
/// 운영자의 의도는 "여기 적힌 principal만 권한을 갖는다"이지, "적지 않으면
/// 전권"이 아니다.
///
/// 과거에는 매핑이 없는 배포에서 [`PermissionKind::all`]을 부여했다 — CF
/// Access application 정책이 곧 유일한 접근 통제라 그 정책이 넓게 열려
/// 있으면 통과한 누구나 토큰 발급·워커 삭제까지 수행할 수 있었다(감사에서
/// 발견). `fleet-cli::runtime::run_serve`가 이제 `FLEET_CF_AUDIENCE`가
/// 설정됐는데 매핑이 비어 있으면 기동 자체를 거부하므로(비로그백 무인증
/// bind 거부와 같은 원칙), 이 함수의 `None` 분기는 방어적 이중 안전장치다
/// — 정상적으로 기동한 배포라면 이 분기에 도달하지 않는다.
fn cf_access_capabilities(state: &AppState, email: &str) -> Vec<PermissionKind> {
    match state.cf_principal_capabilities.as_ref() {
        None => Vec::new(),
        Some(map) => map
            .get(email.trim().to_ascii_lowercase().as_str())
            .cloned()
            .unwrap_or_default(),
    }
}

/// `/v1` 하위 경로를 mount 지점과 무관한 형태로 정규화한다.
///
/// axum `nest`는 하위 라우터로 요청을 넘기기 전에 요청 URI에서 prefix를
/// 제거한다. `/v1` 라우터에 붙인 미들웨어는 따라서 `/v1/workers`가 아니라
/// `/workers`를 본다. 이 사실을 놓치면 capability 행렬이 어떤 경로와도
/// 매칭되지 않아 **모든 요청이 검사 없이 통과**한다(로드맵 #58에서 발견).
///
/// mount 지점이 바뀌어도 안전하도록 두 형태를 모두 같은 값으로 접는다.
fn normalized_v1_path(path: &str) -> &str {
    match path.strip_prefix("/v1") {
        Some(rest) if rest.starts_with('/') => rest,
        _ => path,
    }
}

/// LLM credential 하위 자원(`/workers/{name}/credentials…`)의 route 종류.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LlmCredentialRoute {
    /// `/workers/{name}/credentials` — 목록 조회 / 저장.
    Collection,
    /// `/workers/{name}/credentials/{model_id}` — 개별 credential.
    Item,
    /// `/workers/{name}/credentials/{model_id}/export` — **평문 API 키 반환**.
    Export,
}

/// 경로가 LLM credential 하위 자원인지 분류한다.
///
/// worker operational credential의 단수형 경로(`/workers/{id}/credential`,
/// `/workers/{id}/credential/rotate`)와 반드시 구분돼야 한다. 두 자원은 이름만
/// 비슷할 뿐 완전히 다른 비밀이다 — 전자는 워커가 자신을 인증하는 토큰이고,
/// 후자는 LLM 프로바이더 API 키다. `strip_prefix("credentials")`는 단수형에
/// 매칭되지 않으므로 이 구분이 문자열 수준에서 보장된다.
fn llm_credential_route(path: &str) -> Option<LlmCredentialRoute> {
    let rest = path.strip_prefix("/workers/")?;
    let (name, tail) = rest.split_once('/')?;
    if name.is_empty() {
        return None;
    }
    let tail = tail.strip_prefix("credentials")?;
    match tail.trim_end_matches('/') {
        "" => Some(LlmCredentialRoute::Collection),
        item if item.starts_with('/') && item.ends_with("/export") => {
            Some(LlmCredentialRoute::Export)
        }
        item if item.starts_with('/') => Some(LlmCredentialRoute::Item),
        // `/credentialsomething` 처럼 route 표면에 없는 형태. capability를
        // 요구하는 쪽(fail-closed)으로 분류해 둔다.
        _ => Some(LlmCredentialRoute::Item),
    }
}

/// `/workers/{id}` 단일 세그먼트 경로인지 판정한다 (`/workers/{id}/credential`,
/// `/workers/{name}/credentials/...`는 제외). `GET /workers/{id}`의 capability를
/// LLM credential 하위 자원과 혼동 없이 매칭하기 위한 헬퍼다.
fn is_worker_by_id_route(path: &str) -> bool {
    match path.strip_prefix("/workers/") {
        Some(rest) => !rest.is_empty() && !rest.contains('/'),
        None => false,
    }
}

/// route별 최소 capability 행렬. 경로는 [`normalized_v1_path`] 기준
/// (`/workers`, `/bootstrap-tokens` …)이다.
///
/// `None`이면 이 route에 대한 판정이 없다는 뜻이며, **`authorize_http_endpoint`는
/// 이를 허용이 아니라 거부로 취급한다**(로드맵 `#73`). `/health`와
/// `POST /workers/join`만 이 함수 밖에서 명시적으로 허용된다 — 전자는 LB
/// 프로브용, 후자는 bootstrap token 자체가 인증 수단이라 `auth_middleware`가
/// capability 검사 이전에 우회시킨다.
///
/// **행렬에서 빠진 route는 이제 인증만 통과해도 거부된다.** 새 route를 추가할
/// 때는 반드시 여기에 함께 등록하고, 아래 `capability_matrix_covers_router`
/// 테스트 목록을 늘린다 — 그러지 않으면 그 route는 항상 403을 반환한다(과거
/// `#58`/`#66`처럼 조용히 열려 있는 대신, 조용히 막혀 있는 쪽으로 안전하게
/// 실패한다).
fn required_capability(method: &Method, path: &str) -> Option<PermissionKind> {
    let capability = match (method, path) {
        (&Method::GET, "/workers") => PermissionKind::WorkerList,
        (&Method::POST, "/workers/register") | (&Method::POST, "/workers/heartbeat") => {
            PermissionKind::WorkerRegister
        }
        (&Method::POST, path) if path.ends_with("/credential/rotate") => {
            PermissionKind::WorkerCredentialManage
        }
        (&Method::DELETE, path)
            if path.starts_with("/workers/") && path.ends_with("/credential") =>
        {
            PermissionKind::WorkerCredentialManage
        }
        // LLM 프로바이더 credential 하위 자원 (로드맵 #66). worker 삭제
        // (`WorkerDelete`) 매칭보다 **먼저** 판정해야 한다 — 그렇지 않으면
        // `DELETE /workers/{name}/credentials/{model}`이 worker 삭제 권한으로
        // 잘못 흡수된다.
        (&Method::GET, path) if llm_credential_route(path) == Some(LlmCredentialRoute::Export) => {
            PermissionKind::WorkerLlmCredentialExport
        }
        (&Method::GET, path) if llm_credential_route(path).is_some() => {
            PermissionKind::WorkerLlmCredentialRead
        }
        (&Method::PUT, path) | (&Method::DELETE, path) if llm_credential_route(path).is_some() => {
            PermissionKind::WorkerLlmCredentialManage
        }
        // `GET /workers/{id}` (로드맵 `#73`). 이전에는 행렬에 없어 인증만
        // 통과하면 누구나 조회할 수 있었고, 응답 `endpoint` 필드에 워커의
        // ACP `server-key`가 실려 있어 워커 간 시크릿 노출 경로였다
        // (근본 해결은 `#75`가 그 필드 자체를 없앤다). LLM credential 하위
        // 경로와 겹치지 않도록 `is_worker_by_id_route`로 단일 세그먼트만
        // 매칭한다.
        (&Method::GET, path) if is_worker_by_id_route(path) => PermissionKind::WorkerList,
        (&Method::DELETE, path) if path.starts_with("/workers/") => PermissionKind::WorkerDelete,
        (&Method::POST, "/bootstrap-tokens") => PermissionKind::TokenIssue,
        (&Method::GET, "/bootstrap-tokens") => PermissionKind::TokenList,
        (&Method::DELETE, path) if path.starts_with("/bootstrap-tokens/") => {
            PermissionKind::TokenRevoke
        }
        // Admin API bearer token rotate/revoke (로드맵 #72). bootstrap token
        // 전용인 `Token*`과 재사용하지 않는다 — `#66`에서 겪은 capability
        // 이름 충돌 재발 방지.
        (&Method::GET, "/admin/tokens") => PermissionKind::AdminTokenList,
        (&Method::POST, "/admin/tokens") => PermissionKind::AdminTokenManage,
        (&Method::POST, path)
            if path.starts_with("/admin/tokens/") && path.ends_with("/rotate") =>
        {
            PermissionKind::AdminTokenManage
        }
        (&Method::DELETE, path) if path.starts_with("/admin/tokens/") => {
            PermissionKind::AdminTokenManage
        }
        // `POST /hosts/register` (로드맵 `#73`). 이전에는 행렬에 없어 인증만
        // 통과하면(워커 자신의 operational credential 포함) 누구나 호출할 수
        // 있었고, `upsert_host`의 `ON CONFLICT DO UPDATE`가 기존 Host의
        // `ssh_host`/`ssh_user`/`status`/`worker_id`를 무조건 덮어썼다.
        (&Method::POST, "/hosts/register") => PermissionKind::HostProvision,
        _ => return None,
    };
    Some(capability)
}

/// 현재 `/v1` route 표면의 최소 capability 행렬을 강제한다. Worker self
/// identity와 Project scope는 후속 identity 단계에서 추가한다.
///
/// **기본값은 deny다** (로드맵 `#73`). `required_capability`가 `None`을
/// 반환하는 route — 즉 행렬에 등록되지 않은 route — 는 더 이상 통과시키지
/// 않는다. `/health`와 `POST /workers/join`만 이 함수 안에서 명시적으로
/// 허용한다.
///
/// join을 여기서도 예외 처리하는 이유: `auth_middleware`는 `allow_no_auth`가
/// 아닐 때만 join을 이 함수 호출 전에 미리 우회시킨다(join_worker가 body의
/// bootstrap token으로 자체 인증하므로). `allow_no_auth == true`(개발 기본값)
/// 경로는 그 우회를 거치지 않고 이 함수를 그대로 호출하므로, 여기서도 명시
/// 허용하지 않으면 join이 개발 모드에서만 항상 403이 된다.
fn authorize_http_endpoint(req: &Request) -> Result<(), StatusCode> {
    let path = normalized_v1_path(req.uri().path());
    if path == "/health" {
        return Ok(());
    }
    if req.method() == axum::http::Method::POST && path == "/workers/join" {
        return Ok(());
    }
    let Some(required) = required_capability(req.method(), path) else {
        tracing::warn!(
            path,
            method = %req.method(),
            "HTTP route has no capability-matrix entry — denying by default (#73)"
        );
        return Err(StatusCode::FORBIDDEN);
    };
    let authorized = req
        .extensions()
        .get::<AuthorizationContext>()
        .is_some_and(|context| context.capabilities.contains(&required));
    if authorized {
        Ok(())
    } else {
        tracing::warn!(
            path,
            capability = required.as_str(),
            "HTTP capability denied"
        );
        Err(StatusCode::FORBIDDEN)
    }
}

/// Bearer 토큰을 allow-list와 상수시간으로 비교.
///
/// 타이밍 공격 방어를 위한 두 가지 속성:
/// 1. 두 값을 SHA-256으로 축약한 뒤 고정 길이(32바이트) 상수시간 비교 —
///    바이트 단위 조기 종료도, 토큰 길이 누출도 발생하지 않음.
/// 2. 일치하는 토큰을 찾아도 조기 반환하지 않고 목록 전체를 순회 —
///    allow-list 내 위치(몇 번째 토큰인지)가 응답 시간으로 누출되지 않음.
fn token_matches<'a>(
    candidates: &'a [ApiTokenCredential],
    presented: &str,
) -> Option<&'a ApiTokenCredential> {
    let presented_digest = Sha256::digest(presented.as_bytes());
    let mut matched = Choice::from(0u8);
    let mut match_index = None;
    for (index, candidate) in candidates.iter().enumerate() {
        let candidate_digest = Sha256::digest(candidate.token.as_bytes());
        let is_match = presented_digest.ct_eq(candidate_digest.as_slice());
        if bool::from(is_match) {
            match_index = Some(index);
        }
        matched |= is_match;
    }
    bool::from(matched).then(|| &candidates[match_index.expect("matched token has index")])
}

/// env `valid_tokens`(`FLEET_API_TOKENS`)에 있는 각 토큰을 DB(`admin_api_tokens`)로
/// 1회 자동 upsert한다 (로드맵 #72 무중단 전환).
///
/// 원문 토큰을 재노출하지 않고 digest만 계산해 넣는다 — `#59`의 017
/// bootstrap token digest 마이그레이션과 같은 정신이다. 이미 해당
/// `principal_id`의 행이 DB에 있으면(활성/회수 여부 무관) 건드리지 않으므로,
/// 서버가 뜰 때마다(멱등하게) 호출해도 안전하다 — 두 번째 인스턴스가 동시에
/// 같은 principal을 생성하려는 race는 `Conflict`로 조용히 무시한다.
///
/// `store`가 admin token 저장을 지원하지 않는 백엔드(mock 등)면
/// `StoreError::Unsupported`를 그대로 반환한다 — 호출자는 이를 치명적 에러로
/// 취급하지 않고 로그만 남기는 것을 권장한다(서버 기동을 막지 않기 위함).
pub async fn sync_env_admin_tokens_to_store(
    store: &dyn fleet_store::Store,
    tokens: &[ApiTokenCredential],
) -> Result<(), fleet_store::StoreError> {
    let existing = store.list_admin_tokens().await?;
    let existing_principals: std::collections::HashSet<&str> =
        existing.iter().map(|t| t.principal_id.as_str()).collect();

    for token in tokens {
        if existing_principals.contains(token.principal_id.as_str()) {
            continue;
        }
        let record = fleet_store::AdminApiToken {
            principal_id: token.principal_id.clone(),
            token_digest: fleet_core::BootstrapToken::digest_for(&token.token),
            capabilities: token.capabilities.clone(),
            created_at: chrono::Utc::now(),
            rotated_at: None,
            revoked_at: None,
            rotation_generation: 1,
        };
        match store.create_admin_token(&record).await {
            Ok(()) => {
                info!(
                    principal_id = %token.principal_id,
                    "env admin API token auto-imported into DB (로드맵 #72)"
                );
            }
            // 동시 기동한 다른 인스턴스가 이미 만들었다 — 조용히 넘어간다.
            Err(fleet_store::StoreError::Conflict(_)) => {}
            Err(e) => return Err(e),
        }
    }
    Ok(())
}

/// DB에 admin API 토큰이 하나도 없으면 전체 capability를 가진 1개를
/// 발급한다 (로드맵 `#80`).
///
/// `fleet admin-tokens create`/`fleet token issue` 둘 다 기존 admin bearer를
/// 요구하므로, 이 함수가 없으면 최초 admin 토큰을 API로 만들 방법이 없었다
/// — 운영자가 `FLEET_API_TOKENS` JSON manifest를 손으로 작성하는 것뿐이었다.
///
/// **`sync_env_admin_tokens_to_store` 이후에 호출해야 한다.** env
/// `FLEET_API_TOKENS`가 설정된 배포는 그 호출이 먼저 DB를 채우므로 여기서는
/// 아무 것도 하지 않는다 — env 토큰이 있는데 별도 bootstrap 토큰까지 발급하면
/// 최소 권한 원칙에 어긋난다.
///
/// dashboard OTP(`fleet_store::seed_rbac_and_maybe_issue_bootstrap`)를
/// 재사용하지 않는다 — 그 함수는 `purpose`를 구분하지 않고 모든 사용 가능한
/// bootstrap token을 세므로(`rbac.rs`), worker join 토큰이 하나라도 살아
/// 있으면 admin OTP가 영구 미발급된다. 여기서는 그 함수와 별개로
/// `admin_api_tokens` 테이블만 본다.
///
/// 반환값은 원문 토큰이다. **이 함수는 어디에도 파일을 쓰지 않는다** — 값을
/// 저널/표준출력에 남기지 않고 저장하는 것은 호출자(`fleet-cli`)의 책임이다.
pub async fn issue_admin_bootstrap_token_if_needed(
    store: &dyn fleet_store::Store,
) -> Result<Option<String>, fleet_store::StoreError> {
    let existing = store.list_admin_tokens().await?;
    if !existing.is_empty() {
        return Ok(None);
    }

    let raw = handlers::generate_random_bytes(32)
        .map_err(|e| fleet_store::StoreError::Decode(format!("CSPRNG failure: {e}")))?;
    let token = format!("fat_{}", handlers::base64url(&raw));
    let record = fleet_store::AdminApiToken {
        principal_id: "bootstrap".to_string(),
        token_digest: fleet_core::BootstrapToken::digest_for(&token),
        capabilities: PermissionKind::all().to_vec(),
        created_at: chrono::Utc::now(),
        rotated_at: None,
        revoked_at: None,
        rotation_generation: 1,
    };
    match store.create_admin_token(&record).await {
        Ok(()) => {
            info!(
                principal_id = "bootstrap",
                "issued first-run admin bootstrap token (full capability) — 로드맵 #80"
            );
            Ok(Some(token))
        }
        // 동시 기동한 다른 인스턴스가 이미 발급했다 — 조용히 넘어간다.
        Err(fleet_store::StoreError::Conflict(_)) => Ok(None),
        Err(e) => Err(e),
    }
}

/// 서버 바인딩 + serve. shutdown 시그널은 호출자가 처리.
pub async fn run_http_server(state: Arc<AppState>, bind: SocketAddr) -> std::io::Result<()> {
    let app = build_app(state);
    let listener = tokio::net::TcpListener::bind(bind).await?;
    info!(%bind, "HTTP API server listening");
    axum::serve(listener, app).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use fleet_core::CircuitState;
    use fleet_store::mem::MemStore;
    use std::sync::Arc;

    fn test_token(token: &str) -> ApiTokenCredential {
        ApiTokenCredential {
            principal_id: "test".into(),
            token: token.into(),
            capabilities: PermissionKind::all().to_vec(),
        }
    }

    #[tokio::test]
    async fn app_state_defaults_to_no_auth() {
        let store = MemStore::new_arc();
        let state = AppState::new(store);
        assert!(state.allow_no_auth);
        assert!(state.valid_tokens.is_none());
        assert_eq!(state.heartbeat_interval_secs, 15);
    }

    #[tokio::test]
    async fn app_state_with_tokens_disables_no_auth() {
        let store = MemStore::new_arc();
        let state = AppState::new(store).with_tokens(vec![test_token("secret")]);
        assert!(!state.allow_no_auth);
        assert_eq!(state.valid_tokens.as_ref().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn protected_state_without_auth_provider_is_rejected() {
        let mut state = AppState::new(MemStore::new_arc());
        state.allow_no_auth = false;
        let app = build_app(Arc::new(state));
        let response = tower::ServiceExt::oneshot(
            app,
            axum::http::Request::builder()
                .uri("/v1/workers")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    /// `POST /v1/workers/join`은 `fleet-worker join` CLI가 Authorization
    /// header를 보내지 않으므로, admin bearer 보호가 켜진 배포(`with_tokens`)에서도
    /// 미들웨어 단계에서 401로 막히면 안 된다 — bootstrap token은 request body로
    /// 전달되고 join_worker 핸들러 자체가 검증한다. Authorization header 없이도
    /// 핸들러까지 도달해 (빈 token이므로) 400 BadRequest를 받는지 확인한다 —
    /// 401이 아니라는 게 핵심.
    #[tokio::test]
    async fn join_bypasses_admin_bearer_requirement() {
        let state = AppState::new(MemStore::new_arc()).with_tokens(vec![ApiTokenCredential {
            principal_id: "root".into(),
            token: "admin-secret".into(),
            capabilities: PermissionKind::all().to_vec(),
        }]);
        let app = build_app(Arc::new(state));
        let response = tower::ServiceExt::oneshot(
            app,
            axum::http::Request::builder()
                .method("POST")
                .uri("/v1/workers/join")
                .header("content-type", "application/json")
                .body(axum::body::Body::from("{}"))
                .unwrap(),
        )
        .await
        .unwrap();
        assert_ne!(
            response.status(),
            StatusCode::UNAUTHORIZED,
            "join must not be blocked by the admin bearer middleware — bootstrap token auth happens in the handler"
        );
    }

    #[tokio::test]
    async fn build_app_smoke() {
        let store = MemStore::new_arc();
        let state = Arc::new(AppState::new(store));
        let _router = build_app(state);
        // 빌드가 성공하면 OK — 라우터 구성 검증.
    }

    #[test]
    fn token_matches_exact_token() {
        let tokens = vec![test_token("alpha"), test_token("bravo")];
        assert!(token_matches(&tokens, "alpha").is_some());
        assert!(token_matches(&tokens, "bravo").is_some());
    }

    #[test]
    fn token_matches_rejects_unknown_and_prefixes() {
        let tokens = vec![test_token("secret-token")];
        assert!(token_matches(&tokens, "secret").is_none());
        assert!(token_matches(&tokens, "secret-token-extra").is_none());
        assert!(token_matches(&tokens, "Secret-Token").is_none());
        assert!(token_matches(&tokens, "").is_none());
    }

    #[test]
    fn token_matches_empty_allow_list_rejects_everything() {
        assert!(token_matches(&[], "anything").is_none());
        assert!(token_matches(&[], "").is_none());
    }

    #[tokio::test]
    async fn app_state_cors_origins_default_empty() {
        let store = MemStore::new_arc();
        let state = AppState::new(store);
        assert!(state.cors_allowed_origins.is_empty());
    }

    /// `GET /v1/health` 요청을 라우터에 흘려보내고 응답을 반환.
    async fn health_response(state: AppState, origin: Option<&str>) -> Response {
        use tower::ServiceExt;
        let app = build_app(Arc::new(state));
        let mut builder = axum::http::Request::builder()
            .method(axum::http::Method::GET)
            .uri("/v1/health");
        if let Some(o) = origin {
            builder = builder.header("origin", o);
        }
        let req = builder.body(axum::body::Body::empty()).unwrap();
        app.oneshot(req).await.unwrap()
    }

    #[tokio::test]
    async fn responses_carry_security_headers() {
        let state = AppState::new(MemStore::new_arc());
        let resp = health_response(state, None).await;
        let headers = resp.headers();

        assert_eq!(headers.get("content-security-policy").unwrap(), API_CSP);
        assert_eq!(headers.get("x-frame-options").unwrap(), "DENY");
        assert_eq!(headers.get("x-content-type-options").unwrap(), "nosniff");
        assert_eq!(headers.get("strict-transport-security").unwrap(), API_HSTS);
        assert_eq!(headers.get("referrer-policy").unwrap(), "no-referrer");
    }

    #[tokio::test]
    async fn cors_disabled_by_default_for_cross_origin_request() {
        let state = AppState::new(MemStore::new_arc());
        let resp = health_response(state, Some("https://evil.example.com")).await;
        // permissive였다면 Access-Control-Allow-Origin이 존재했을 것.
        assert!(resp.headers().get("access-control-allow-origin").is_none());
    }

    #[tokio::test]
    async fn cors_allow_list_echoes_only_listed_origin() {
        let allowed = "https://console.example.com";
        let state = AppState::new(MemStore::new_arc()).with_cors_origins(vec![allowed.to_string()]);
        let resp = health_response(state, Some(allowed)).await;
        assert_eq!(
            resp.headers().get("access-control-allow-origin").unwrap(),
            allowed
        );

        let state = AppState::new(MemStore::new_arc()).with_cors_origins(vec![allowed.to_string()]);
        let resp = health_response(state, Some("https://evil.example.com")).await;
        assert!(resp.headers().get("access-control-allow-origin").is_none());
    }

    #[tokio::test]
    async fn cors_wildcard_entry_is_ignored() {
        let state = AppState::new(MemStore::new_arc()).with_cors_origins(vec!["*".to_string()]);
        let resp = health_response(state, Some("https://evil.example.com")).await;
        assert!(resp.headers().get("access-control-allow-origin").is_none());
    }

    #[tokio::test]
    async fn cors_malformed_origin_entries_are_ignored() {
        // 경로 포함 / 트레일링 슬래시 / 스킴 누락은 조용한 오설정이 되므로 거부.
        for bad in [
            "https://console.example.com/",
            "https://console.example.com/app",
            "console.example.com",
        ] {
            let state = AppState::new(MemStore::new_arc()).with_cors_origins(vec![bad.to_string()]);
            let resp = health_response(state, Some("https://console.example.com")).await;
            assert!(
                resp.headers().get("access-control-allow-origin").is_none(),
                "origin entry {bad} should have been rejected"
            );
        }
    }

    #[tokio::test]
    async fn bearer_auth_accepts_valid_and_rejects_invalid_token() {
        use tower::ServiceExt;

        async fn call(token: Option<&str>) -> StatusCode {
            let state = Arc::new(
                AppState::new(MemStore::new_arc()).with_tokens(vec![test_token("good-token")]),
            );
            let app = build_app(state);
            let mut builder = axum::http::Request::builder()
                .method(axum::http::Method::GET)
                .uri("/v1/workers");
            if let Some(t) = token {
                builder = builder.header("authorization", format!("Bearer {t}"));
            }
            let req = builder.body(axum::body::Body::empty()).unwrap();
            app.oneshot(req).await.unwrap().status()
        }

        assert_ne!(call(Some("good-token")).await, StatusCode::UNAUTHORIZED);
        assert_eq!(call(Some("good-toke")).await, StatusCode::UNAUTHORIZED);
        assert_eq!(call(Some("good-token ")).await, StatusCode::UNAUTHORIZED);
        assert_eq!(call(Some("")).await, StatusCode::UNAUTHORIZED);
        assert_eq!(call(None).await, StatusCode::UNAUTHORIZED);
    }

    // ── capability 행렬 경로 정규화 (로드맵 #58) ─────────────────────────

    #[test]
    fn normalized_path_folds_mounted_and_stripped_forms() {
        // axum nest가 prefix를 제거한 형태(미들웨어가 실제로 보는 값).
        assert_eq!(normalized_v1_path("/workers"), "/workers");
        // 마운트 지점이 남아 있는 형태.
        assert_eq!(normalized_v1_path("/v1/workers"), "/workers");
        assert_eq!(normalized_v1_path("/v1/health"), "/health");
        // `/v1`으로 시작하지만 경계가 아닌 경로는 건드리지 않는다.
        assert_eq!(normalized_v1_path("/v1beta/workers"), "/v1beta/workers");
    }

    #[test]
    fn capability_matrix_matches_both_path_forms() {
        for (mounted, stripped) in [
            ("/v1/workers", "/workers"),
            ("/v1/bootstrap-tokens", "/bootstrap-tokens"),
        ] {
            assert_eq!(
                required_capability(&Method::GET, normalized_v1_path(mounted)),
                required_capability(&Method::GET, stripped),
                "path form must not change the required capability"
            );
        }
    }

    #[test]
    fn capability_matrix_covers_protected_routes() {
        assert_eq!(
            required_capability(&Method::GET, "/workers"),
            Some(PermissionKind::WorkerList)
        );
        assert_eq!(
            required_capability(&Method::POST, "/workers/register"),
            Some(PermissionKind::WorkerRegister)
        );
        assert_eq!(
            required_capability(&Method::POST, "/workers/heartbeat"),
            Some(PermissionKind::WorkerRegister)
        );
        assert_eq!(
            required_capability(&Method::POST, "/workers/abc/credential/rotate"),
            Some(PermissionKind::WorkerCredentialManage)
        );
        assert_eq!(
            required_capability(&Method::DELETE, "/workers/abc/credential"),
            Some(PermissionKind::WorkerCredentialManage)
        );
        assert_eq!(
            required_capability(&Method::DELETE, "/workers/abc"),
            Some(PermissionKind::WorkerDelete)
        );
        assert_eq!(
            required_capability(&Method::POST, "/bootstrap-tokens"),
            Some(PermissionKind::TokenIssue)
        );
        assert_eq!(
            required_capability(&Method::GET, "/bootstrap-tokens"),
            Some(PermissionKind::TokenList)
        );
        assert_eq!(
            required_capability(&Method::DELETE, "/bootstrap-tokens/bt_x"),
            Some(PermissionKind::TokenRevoke)
        );
        // join은 bootstrap token 자체가 인증 수단.
        assert_eq!(required_capability(&Method::POST, "/workers/join"), None);
        assert_eq!(required_capability(&Method::GET, "/health"), None);
    }

    // ── 기본값 deny 전환 (로드맵 #73) ────────────────────────────────────

    #[test]
    fn get_worker_by_id_and_host_register_now_require_capability() {
        // 이전에는 행렬에 없어 `authorize_http_endpoint`가 통과시켰다 — 전자는
        // 워커의 ACP `server-key`를 담은 `endpoint` 필드를 무권한 노출했고,
        // 후자는 기존 Host 레코드를 무권한으로 덮어썼다.
        assert_eq!(
            required_capability(&Method::GET, "/workers/abc-123"),
            Some(PermissionKind::WorkerList)
        );
        assert_eq!(
            required_capability(&Method::POST, "/hosts/register"),
            Some(PermissionKind::HostProvision)
        );
        // LLM credential 하위 경로와 혼동되지 않는다.
        assert_eq!(
            required_capability(&Method::GET, "/workers/abc-123/credentials"),
            Some(PermissionKind::WorkerLlmCredentialRead)
        );
        assert_ne!(
            required_capability(&Method::GET, "/workers/abc-123"),
            required_capability(&Method::GET, "/workers/abc-123/credentials")
        );
    }

    #[test]
    fn is_worker_by_id_route_matches_single_segment_only() {
        assert!(is_worker_by_id_route("/workers/abc-123"));
        assert!(!is_worker_by_id_route("/workers/"));
        assert!(!is_worker_by_id_route("/workers"));
        assert!(!is_worker_by_id_route("/workers/abc/credential"));
        assert!(!is_worker_by_id_route("/workers/abc/credentials"));
        assert!(!is_worker_by_id_route("/workers/abc/credentials/model/export"));
    }

    /// 함수 수준의 기본값 회귀 가드. `#58`/`#66`이 두 번 반복한 "행렬 미등록 =
    /// 허용"이 세 번째로 되살아나지 않았음을, 실제 axum 라우팅과 무관하게
    /// 고정한다 — `authorize_http_endpoint`는 `required_capability`가 `None`을
    /// 반환하는 어떤 (method, path) 조합에 대해서도 `Err(FORBIDDEN)`을 반환해야
    /// 한다. `/health`와 `POST /workers/join`만 예외다.
    #[test]
    fn authorize_http_endpoint_denies_by_default_for_any_unmatched_route() {
        for (method, path) in [
            (Method::GET, "/this-route-does-not-exist"),
            (Method::POST, "/this-route-does-not-exist"),
            (Method::PUT, "/workers"),
            (Method::PATCH, "/workers/abc-123"),
            // 등록됐지만 이 메서드로는 존재하지 않는 조합.
            (Method::GET, "/bootstrap-tokens/bt_x"),
        ] {
            assert_eq!(
                required_capability(&method, path),
                None,
                "test setup assumption broken: {method} {path} unexpectedly has a capability"
            );
            let req = axum::http::Request::builder()
                .method(method.clone())
                .uri(format!("/v1{path}"))
                .body(axum::body::Body::empty())
                .unwrap();
            assert_eq!(
                authorize_http_endpoint(&req),
                Err(StatusCode::FORBIDDEN),
                "{method} {path} must deny by default when unregistered"
            );
        }

        // 명시적 예외 둘은 허용된다.
        let health = axum::http::Request::builder()
            .method(Method::GET)
            .uri("/v1/health")
            .body(axum::body::Body::empty())
            .unwrap();
        assert_eq!(authorize_http_endpoint(&health), Ok(()));

        let join = axum::http::Request::builder()
            .method(Method::POST)
            .uri("/v1/workers/join")
            .body(axum::body::Body::empty())
            .unwrap();
        assert_eq!(authorize_http_endpoint(&join), Ok(()));
    }

    /// 커버리지 목록. `build_app`의 route 표면과 병행 유지한다(로드맵 #21의
    /// `openapi_yaml_is_valid_and_covers_known_paths`와 같은 관례) — 새 route를
    /// `build_app`에 추가할 때 이 목록도 함께 늘린다. 여기 없는 (method, path)
    /// 조합은 기본값 deny 시험(위)이 잡아내므로, 이 목록의 실질적 목적은
    /// "router에 실제로 있는 모든 route가 의도적으로 capability를 갖고
    /// 있다"는 긍정 확인이다.
    #[test]
    fn capability_matrix_covers_router_routes() {
        const ALLOW_LISTED_WITHOUT_CAPABILITY: &[(Method, &str)] =
            &[(Method::GET, "/health"), (Method::POST, "/workers/join")];

        let routes: &[(Method, &str)] = &[
            (Method::POST, "/workers/register"),
            (Method::POST, "/workers/heartbeat"),
            (Method::GET, "/workers"),
            (Method::GET, "/workers/w1"),
            (Method::DELETE, "/workers/w1"),
            (Method::POST, "/workers/w1/credential/rotate"),
            (Method::DELETE, "/workers/w1/credential"),
            (Method::PUT, "/workers/w1/credentials"),
            (Method::GET, "/workers/w1/credentials"),
            (Method::GET, "/workers/w1/credentials/model-a/export"),
            (Method::DELETE, "/workers/w1/credentials/model-a"),
            (Method::POST, "/bootstrap-tokens"),
            (Method::GET, "/bootstrap-tokens"),
            (Method::DELETE, "/bootstrap-tokens/bt_x"),
            (Method::POST, "/admin/tokens"),
            (Method::GET, "/admin/tokens"),
            (Method::POST, "/admin/tokens/svc-a/rotate"),
            (Method::DELETE, "/admin/tokens/svc-a"),
            (Method::POST, "/hosts/register"),
        ];

        for (method, path) in routes {
            assert!(
                required_capability(method, path).is_some(),
                "{method} {path} is a registered route with no capability-matrix entry"
            );
        }

        for (method, path) in ALLOW_LISTED_WITHOUT_CAPABILITY {
            assert_eq!(
                required_capability(method, path),
                None,
                "{method} {path} was expected to be capability-free (allow-listed),                  but the matrix now has an entry — update ALLOW_LISTED_WITHOUT_CAPABILITY                  or this test if that's intentional"
            );
        }
    }

    // ── Admin API token route capability (로드맵 #72) ────────────────────

    #[test]
    fn capability_matrix_covers_admin_token_routes() {
        assert_eq!(
            required_capability(&Method::POST, "/admin/tokens"),
            Some(PermissionKind::AdminTokenManage)
        );
        assert_eq!(
            required_capability(&Method::GET, "/admin/tokens"),
            Some(PermissionKind::AdminTokenList)
        );
        assert_eq!(
            required_capability(&Method::POST, "/admin/tokens/svc-a/rotate"),
            Some(PermissionKind::AdminTokenManage)
        );
        assert_eq!(
            required_capability(&Method::DELETE, "/admin/tokens/svc-a"),
            Some(PermissionKind::AdminTokenManage)
        );
        // bootstrap token 전용 capability(`Token*`)와 재사용되지 않는다.
        assert_ne!(
            required_capability(&Method::POST, "/admin/tokens"),
            Some(PermissionKind::TokenIssue)
        );
        // mount 지점이 남아 있는 형태도 같은 결론이어야 한다.
        assert_eq!(
            required_capability(&Method::GET, normalized_v1_path("/v1/admin/tokens")),
            Some(PermissionKind::AdminTokenList)
        );
    }

    // ── LLM credential route capability (로드맵 #66) ─────────────────────

    #[test]
    fn capability_matrix_covers_llm_credential_routes() {
        // 이 네 route가 행렬에서 빠져 있던 동안, 인증만 통과하면 누구나
        // 모든 워커의 LLM 프로바이더 API 키를 평문으로 가져갈 수 있었다.
        assert_eq!(
            required_capability(&Method::GET, "/workers/w1/credentials"),
            Some(PermissionKind::WorkerLlmCredentialRead)
        );
        assert_eq!(
            required_capability(&Method::GET, "/workers/w1/credentials/grok-4/export"),
            Some(PermissionKind::WorkerLlmCredentialExport)
        );
        assert_eq!(
            required_capability(&Method::PUT, "/workers/w1/credentials"),
            Some(PermissionKind::WorkerLlmCredentialManage)
        );
        assert_eq!(
            required_capability(&Method::DELETE, "/workers/w1/credentials/grok-4"),
            Some(PermissionKind::WorkerLlmCredentialManage)
        );
        // mount 지점이 남아 있는 형태도 같은 결론이어야 한다.
        assert_eq!(
            required_capability(
                &Method::GET,
                normalized_v1_path("/v1/workers/w1/credentials/grok-4/export")
            ),
            Some(PermissionKind::WorkerLlmCredentialExport)
        );
    }

    #[test]
    fn llm_credential_routes_do_not_collide_with_operational_credential() {
        // 단수형 `/credential`(worker 자신을 인증하는 `fwo_` 토큰)과
        // 복수형 `/credentials`(LLM 프로바이더 API 키)는 서로 다른 비밀이다.
        assert_eq!(llm_credential_route("/workers/w1/credential"), None);
        assert_eq!(llm_credential_route("/workers/w1/credential/rotate"), None);
        assert_eq!(llm_credential_route("/workers"), None);
        assert_eq!(llm_credential_route("/bootstrap-tokens"), None);
        assert_eq!(
            llm_credential_route("/workers/w1/credentials"),
            Some(LlmCredentialRoute::Collection)
        );
        assert_eq!(
            llm_credential_route("/workers/w1/credentials/"),
            Some(LlmCredentialRoute::Collection)
        );
        assert_eq!(
            llm_credential_route("/workers/w1/credentials/grok-4"),
            Some(LlmCredentialRoute::Item)
        );
        assert_eq!(
            llm_credential_route("/workers/w1/credentials/grok-4/export"),
            Some(LlmCredentialRoute::Export)
        );

        // 단수형은 기존 capability를 그대로 유지한다.
        assert_eq!(
            required_capability(&Method::DELETE, "/workers/w1/credential"),
            Some(PermissionKind::WorkerCredentialManage)
        );
        // 복수형 DELETE가 worker 삭제 권한으로 흡수되면 안 된다.
        assert_ne!(
            required_capability(&Method::DELETE, "/workers/w1/credentials/grok-4"),
            Some(PermissionKind::WorkerDelete)
        );
    }

    #[test]
    fn llm_credential_export_is_not_covered_by_manage() {
        // 평문 export는 저장/삭제 권한과 분리돼 있어야 한다 — 프로비저너
        // 토큰이 credential을 덮어쓰거나 지울 수 있으면 안 되고, 반대로
        // 저장 권한만 있는 주체가 키 원문을 읽을 수 있어도 안 된다.
        assert_ne!(
            required_capability(&Method::GET, "/workers/w1/credentials/grok-4/export"),
            required_capability(&Method::PUT, "/workers/w1/credentials")
        );
    }

    // ── CF Access principal/capability (로드맵 #58) ──────────────────────

    fn verified_user(email: &str) -> crate::cloudflare::VerifiedUser {
        crate::cloudflare::VerifiedUser {
            email: email.to_string(),
            audience: "aud-123".to_string(),
            expires_at: 0,
        }
    }

    #[test]
    fn cf_principal_id_is_the_jwt_email() {
        assert_eq!(
            cf_access_principal_id(&verified_user("ops@example.com")),
            "ops@example.com"
        );
    }

    #[test]
    fn cf_principal_id_falls_back_to_audience_when_email_missing() {
        // service token 등 email 클레임이 없는 세션.
        assert_eq!(
            cf_access_principal_id(&verified_user("")),
            "cf-access:aud:aud-123"
        );
    }

    #[test]
    fn cf_capabilities_default_to_empty_when_unmapped_deployment() {
        // 로드맵 #74 — 매핑을 아예 설정하지 않은 배포는 fail-closed다.
        // 전체 capability를 부여하던 과거 동작(fail-open)은 제거됐다.
        let state = AppState::new(MemStore::new_arc()).with_cf_audience("aud-123");
        assert_eq!(cf_access_capabilities(&state, "ops@example.com"), Vec::new());
    }

    #[test]
    fn cf_capabilities_use_mapping_case_insensitively() {
        let state = AppState::new(MemStore::new_arc())
            .with_cf_audience("aud-123")
            .with_cf_principal_capabilities([(
                "Ops@Example.com".to_string(),
                vec![PermissionKind::WorkerList],
            )]);
        assert_eq!(
            cf_access_capabilities(&state, "OPS@example.com "),
            vec![PermissionKind::WorkerList]
        );
    }

    #[test]
    fn cf_capabilities_are_empty_for_principal_missing_from_mapping() {
        // 매핑이 설정되면 열거되지 않은 principal은 fail-closed.
        let state = AppState::new(MemStore::new_arc())
            .with_cf_audience("aud-123")
            .with_cf_principal_capabilities([(
                "ops@example.com".to_string(),
                vec![PermissionKind::WorkerList],
            )]);
        assert!(cf_access_capabilities(&state, "stranger@example.com").is_empty());
    }

    #[tokio::test]
    async fn cf_only_deployment_without_verified_principal_is_rejected() {
        use tower::ServiceExt;
        // CF middleware가 붙지 않은(=VerifiedUser가 없는) 상태를 강제로 만든 뒤
        // auth_middleware가 무인증 통과 대신 401을 내는지 확인.
        let mut state = AppState::new(MemStore::new_arc());
        state.allow_no_auth = false;
        state.cf_audience = Some("aud-123".into());
        // build_app이 CF 미들웨어를 붙이지 못하도록 라우터를 직접 구성하는 대신,
        // 여기서는 CF JWT 없이 요청한다. CF 미들웨어가 401로 막아야 하고,
        // 설령 통과하더라도 auth_middleware가 principal 부재로 401을 낸다.
        let app = build_app(Arc::new(state));
        let req = axum::http::Request::builder()
            .uri("/v1/workers")
            .body(axum::body::Body::empty())
            .unwrap();
        let response = app.oneshot(req).await.unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[test]
    fn circuit_state_unused_marker() {
        // CircuitState가 이 모듈에서 미사용이더라도 다른 곳에서 쓰이므로 re-export
        let _ = CircuitState::Closed;
    }

    // ── OpenAPI 스펙 (로드맵 #21) ────────────────────────────────────────

    #[test]
    fn openapi_yaml_is_valid_and_covers_known_paths() {
        let doc: serde_yaml::Value =
            serde_yaml::from_str(OPENAPI_YAML).expect("openapi.yaml must be valid YAML");
        assert_eq!(
            doc["openapi"].as_str(),
            Some("3.0.3"),
            "unexpected or missing openapi version field"
        );
        let paths = doc["paths"].as_mapping().expect("paths must be a mapping");
        for expected in [
            "/health",
            "/workers/register",
            "/workers/join",
            "/workers/heartbeat",
            "/workers",
            "/workers/{id}",
            "/workers/{name}/credentials",
            "/bootstrap-tokens",
            "/bootstrap-tokens/{token_id}",
            "/hosts/register",
            "/metrics",
        ] {
            assert!(
                paths.contains_key(serde_yaml::Value::String(expected.to_string())),
                "openapi.yaml is missing path: {expected}"
            );
        }
    }

    #[tokio::test]
    async fn openapi_yaml_route_is_reachable_without_auth() {
        use tower::ServiceExt;
        // 토큰이 설정돼 있어도 /openapi.yaml은 인증 없이 조회 가능해야 한다
        // (/metrics와 동일한 이유 — 발견성).
        let state = AppState::new(MemStore::new_arc()).with_tokens(vec![test_token("secret")]);
        let app = build_app(Arc::new(state));
        let req = axum::http::Request::builder()
            .method(axum::http::Method::GET)
            .uri("/openapi.yaml")
            .body(axum::body::Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let content_type = resp
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or_default()
            .to_string();
        assert!(
            content_type.contains("yaml"),
            "unexpected content-type: {content_type}"
        );

        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        assert!(String::from_utf8_lossy(&body).starts_with("openapi: 3.0.3"));
    }
}
