//! Agent별 grok 프로세스 매니저 (로드맵 `#67` 4c-A).
//!
//! `GrokRunner`(`grok_process.rs`)의 1:1 singleton을 **대체하지 않고 옆에 선다.**
//! singleton은 `workers.endpoint`로 등록된 Worker 자신의 ACP 종단이고 모든 Task
//! dispatch가 그리로 가므로, Agent별로 쪼개는 순간 dispatch는 `tasks.agent_id`
//! 라우팅(`#49` 2단계)이 생기기 전까지 갈 곳을 잃는다. 설계 근거는
//! [Agent 프로비저닝](../../../docs/architecture/agents/provisioning.md)의
//! §"설계 결정 (`#67` 4c)"에 있다.
//!
//! ## 수렴
//!
//! heartbeat 응답의 `agents` 목록을 **권위 있는 전체 집합**으로 읽는다:
//!
//! - 목록이 `None`이면 아무것도 하지 않는다. store 조회가 실패한 beat이 그 Worker의
//!   Agent를 전부 죽이면 안 된다.
//! - `Some([])`은 "정말로 없다"이므로 전부 정리한다.
//! - 목록에 있고 `desired_status = running`인데 프로세스가 없으면 띄운다.
//! - 목록에 없거나 `stopped`이면 정리한다.
//!
//! 매 beat마다 전체 목록이 다시 오므로 이 함수는 **멱등**이며, 명령을 놓쳤는지
//! 추적할 필요가 없다.
//!
//! ## 관측 (4c-B)
//!
//! [`reconcile`](AgentProcessManager::reconcile)이 이번 beat에 본 것을
//! [`AgentObservation`] 목록으로 돌려주고, `registration.rs`가 그것을 다음
//! heartbeat 요청에 싣는다. 4c-A에서 워커 로그 한 줄이 전부였던 거절이 여기서
//! 오케스트레이터에 도달한다.
//!
//! **`Starting`은 만들지 않는다.** 정본의 이름표는 그것을 "자식을 띄웠고 아직
//! health check 전"으로 정의했는데 이 매니저에는 health check가 없다 —
//! `try_wait()`는 "죽지 않았다"만 말한다. 어휘는 `Running`과 `Failed` 둘이다.
//!
//! **크래시 루프는 보이지 않는다.** 0단계가 죽은 자식을 걷어내고 3단계가 같은
//! beat에 재기동하므로, 매 beat 죽었다 살아나는 Agent도 관측은 `Running`이다.
//! 그것을 드러내려면 상태가 아니라 **사건**을 실어야 하는데(재기동 횟수 또는
//! 전이 이벤트), 그 채널은 지금 없고 소비자도 없다. 4c-B는 상태만 다룬다.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;

use tokio::process::{Child, Command};
use tokio::sync::Mutex;
use tracing::{debug, info, warn};

use fleet_core::{
    AgentCommand, AgentDesiredStatus, AgentId, AgentObservation, AgentObservationReason,
    AgentOrphan, AgentOrphanReason,
};
use serde::{Deserialize, Serialize};

use crate::config::WorkerConfig;
use crate::error::WorkerError;
use crate::grok_process::{apply_llm_proxy_envs, host_of, terminate_child};

/// 실행 중인 Agent 프로세스 하나.
struct AgentProc {
    child: Child,
    port: u16,
}

/// 자식 하나가 자기 workspace에 남기는 디스크 기록의 파일 이름
/// (로드맵 `#70` 게이트 ③).
const SPAWN_RECORD: &str = ".fleet-agent.json";

/// [`SPAWN_RECORD`]의 내용.
///
/// **이 파일이 존재하는 이유는 [`AgentProcessManager::procs`]가 메모리이기
/// 때문이다.** Worker가 SIGKILL·전원 차단·패닉으로 죽으면 `kill_on_drop`의
/// Drop이 돌지 않아 자식이 살아남는데, 새 incarnation의 `procs`는 비어 있어
/// 그 자식을 **알 수 있는 방법이 아무것도 없다**. 재조정 루프는 자기가 띄운
/// 것만 보므로 원리적으로 그것을 발견하지 못한다.
///
/// `port`를 함께 적는 이유는 진단이다 — 이 자식은 `agent_port_range`의 포트
/// 하나를 계속 쥐고 있고, 그래서 새 incarnation의 `free_port`가 그 포트에서
/// bind에 실패해 `NoFreePort`로 거절한다. 원인(우리가 남긴 고아)에서 아주 먼
/// 자리에 증상이 나타나는 셈이라, 그 연결을 기록이 직접 이어 준다.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct SpawnRecord {
    agent_id: AgentId,
    pid: u32,
    port: u16,
    /// 우리가 이 자식을 띄운 벽시계 시각 (epoch 초).
    ///
    /// **pid 재사용을 거르는 수단이다.** 기록을 남긴 뒤 Worker가 죽고 한참
    /// 지나면 그 pid는 전혀 다른 프로그램의 것일 수 있다 — Linux의 기본
    /// `pid_max`는 32768이라 바쁜 호스트에서 순환은 드문 일이 아니다. 대조가
    /// 없으면 이 sweep은 **관계 없는 프로세스를 죽이는 코드**가 된다.
    ///
    /// 대조 대상으로 **프로세스 이름이 아니라 시작 시각을 고른 이유**가 있다.
    /// 이름은 `exec`에 의해 바뀐다 — 셸 스크립트로 감싼 진입점이나 래퍼는
    /// 기동 직후와 정착 후의 이름이 다르고, 그래서 이름 대조는 그 경계에서
    /// 경합한다. 시작 시각은 `exec`이 프로세스를 **새로 만들지 않으므로**
    /// 보존되며, 그 경합 자체가 없다.
    started_at_unix: u64,
}

/// [`SpawnRecord::started_at_unix`]와 관측된 `start_time()`이 이만큼까지
/// 어긋나는 것은 같은 프로세스로 본다.
///
/// 둘은 서로 다른 시계에서 온다 — 하나는 우리가 `spawn()` 직후에 읽은
/// 벽시계이고 다른 하나는 커널이 프로세스 생성 시점에 기록한 값이라, 초
/// 단위로 반올림되는 자리에서 1~2초는 정상적으로 벌어진다. 반대로 pid가
/// 재사용될 만큼 시간이 흐른 경우는 이 창을 한참 벗어난다.
const START_TIME_SLACK_SECS: u64 = 5;

/// [`AgentProcessManager::reconcile`]이 이번 beat에 만든 것 (로드맵 `#70` 게이트 ③).
///
/// 두 목록을 **한 구조체로 함께** 돌려주는 이유는 둘 다 같은 한 번의 순회에서
/// 나오기 때문이다. 따로 돌면 그 사이에 프로세스 표가 바뀔 수 있고, 그러면
/// "관측에도 없고 고아에도 없는" 프로세스가 생긴다.
///
/// 두 목록의 **의미는 대칭이 아니다**. `observations`는 권위 있는 전체 집합이라
/// 빈 목록이 "돌고 있는 것이 하나도 없다"는 주장이고, `orphans`는 사건 목록이라
/// 빈 목록이 "이번 beat에 그런 일이 없었다"일 뿐이다. 이 차이가
/// `HeartbeatRequest`에서 `Option<Vec<_>>`와 `Vec<_>`의 차이로 그대로 이어진다.
#[derive(Debug, Default)]
pub struct ReconcileOutcome {
    pub observations: Vec<AgentObservation>,
    pub orphans: Vec<AgentOrphan>,
}

/// Agent를 이번 beat에 띄우지 못한 이유.
///
/// **거절 경로는 하나다.** 원인이 둘이어도 결과("이번 beat에 뜨지 않았다")는
/// 같고, 4c-B에서 둘 다 같은 관측 상태로 접힌다. 이름을 둘로 나눠 보고하면
/// 운영자는 서로 다른 두 오류를 보고 같은 처방을 찾게 된다. 원인은 이름이
/// 아니라 **로그 필드**로 구분한다.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RejectReason {
    /// `grok.max_agent_processes`에 도달했다.
    CapReached,
    /// `grok.agent_port_range`에 쓸 수 있는 포트가 없다.
    NoFreePort,
}

impl RejectReason {
    fn as_str(self) -> &'static str {
        match self {
            RejectReason::CapReached => "process cap reached",
            RejectReason::NoFreePort => "no free port in range",
        }
    }
}

/// 워커 안의 거절 이유를 오케스트레이터가 아는 어휘로 옮긴다 (로드맵 `#67` 4c-B).
///
/// 변환을 두고 두 번째 문자열 어휘를 만들지 않는 이유: `as_str`의 값
/// (`"process cap reached"`)은 **사람이 읽는 로그 문구**이고 코어의
/// `as_str`(`"cap_reached"`)은 **DB CHECK에 적힌 값**이다. 같은 문자열로
/// 합치면 로그 문구를 다듬는 순간 스키마가 깨진다.
impl From<RejectReason> for AgentObservationReason {
    fn from(r: RejectReason) -> Self {
        match r {
            RejectReason::CapReached => AgentObservationReason::CapReached,
            RejectReason::NoFreePort => AgentObservationReason::NoFreePort,
        }
    }
}

/// Agent별 프로세스 매니저.
pub struct AgentProcessManager {
    config: Arc<WorkerConfig>,
    /// singleton과 같은 host에 붙인다 — 포트만 Agent마다 다르다.
    host: String,
    port_range: (u16, u16),
    max_processes: usize,
    workspace_root: PathBuf,
    procs: Mutex<HashMap<AgentId, AgentProc>>,
}

impl AgentProcessManager {
    /// 설정에서 매니저를 만든다. 포트 범위와 workspace 루트를 **여기서** 확정한다.
    ///
    /// 파싱 실패를 기동 시점으로 당기는 이유: 첫 Agent 명령이 도착하는 것은 몇
    /// 시간 뒤일 수 있고, 그때 설정 오류를 처음 발견하면 그 beat의 명령이 조용히
    /// 버려진다.
    pub fn new(config: Arc<WorkerConfig>) -> Result<Self, WorkerError> {
        let raw = &config.grok.agent_port_range;
        let (lo, hi) = raw
            .split_once('-')
            .ok_or_else(|| {
                WorkerError::Config(format!(
                    "grok.agent_port_range must be \"start-end\", got {raw:?}"
                ))
            })
            .and_then(|(a, b)| {
                let lo: u16 = a.trim().parse().map_err(|_| {
                    WorkerError::Config(format!("grok.agent_port_range start is not a port: {a:?}"))
                })?;
                let hi: u16 = b.trim().parse().map_err(|_| {
                    WorkerError::Config(format!("grok.agent_port_range end is not a port: {b:?}"))
                })?;
                Ok((lo, hi))
            })?;
        if lo == 0 || lo > hi {
            return Err(WorkerError::Config(format!(
                "grok.agent_port_range is empty or inverted: {raw:?}"
            )));
        }

        let workspace_root = match &config.grok.agent_workspace_root {
            Some(p) => PathBuf::from(p),
            None => {
                let base = match &config.grok.cwd {
                    Some(c) => PathBuf::from(c),
                    None => std::env::current_dir()?,
                };
                base.join("fleet-agents")
            }
        };

        Ok(Self {
            host: host_of(&config.grok.bind_addr).to_string(),
            port_range: (lo, hi),
            max_processes: config.grok.max_agent_processes as usize,
            workspace_root,
            config,
            procs: Mutex::new(HashMap::new()),
        })
    }

    /// 지금 살아 있다고 매니저가 믿는 Agent들. 순서는 정하지 않는다.
    pub async fn running_agents(&self) -> Vec<AgentId> {
        self.procs.lock().await.keys().copied().collect()
    }

    /// 이전 incarnation이 남긴 Agent 프로세스를 찾아 종료하고, 그것을 사건으로
    /// 돌려준다 (로드맵 `#70` 게이트 ③).
    ///
    /// **재조정 루프로는 원리적으로 찾을 수 없는 고아를 다룬다.**
    /// [`reconcile`](Self::reconcile)은 자기 [`procs`](Self::procs)에 있는 것만
    /// 보는데, Worker가 SIGKILL·전원 차단·패닉으로 죽으면 그 표는 사라지고
    /// 자식은 남는다(`kill_on_drop`은 Drop이 돌 때만 유효하다). 새
    /// incarnation에게 그 자식은 존재하지 않는 것과 같고, 유일한 흔적은
    /// **엉뚱한 자리에 나타나는 증상** 하나다 — 그 자식이 계속 쥐고 있는 포트
    /// 때문에 [`free_port`](Self::free_port)의 bind가 실패해 새 Agent가
    /// `NoFreePort`로 거절된다. 원인과 증상이 이만큼 떨어져 있으면 운영자는
    /// 포트 범위를 넓히는 잘못된 처방을 찾게 된다.
    ///
    /// 그래서 근거를 메모리가 아니라 **디스크**에서 가져온다. 자식마다
    /// [`SPAWN_RECORD`]가 workspace에 남아 있고, 이 함수는 그것을 읽어 pid의
    /// 생사를 확인한다.
    ///
    /// ## 죽이는 것 말고 다른 선택지가 없다
    ///
    /// 살아 있는 자식을 **입양할 수는 없다.** [`Child`] 핸들은 그것을 만든
    /// 프로세스에만 있고 새 incarnation에는 pid밖에 없어, `try_wait()`으로
    /// 생사를 볼 수단이 없다 — 0단계가 성립하지 않는다. 그대로 두면
    /// 오케스트레이터가 같은 Agent를 다시 배정할 때 `reconcile`이 **두 번째**
    /// 프로세스를 띄우고, 포트가 다르니 아무것도 그것을 막지 않는다. 그러므로
    /// 종료가 유일하게 일관된 처리다. 대가는 정직하게 적는다 — 멀쩡히 돌던
    /// Agent가 Worker 재기동 뒤 한 번 죽는다.
    ///
    /// ## 언제 부르는가
    ///
    /// **기동 시점에 한 번만** 부른다. 근거가 "이전 incarnation이 남긴 것"이라
    /// 매 beat 다시 물어볼 대상이 아니고, 프로세스 표를 새로고침하는 비용을
    /// beat 타이머 안에 넣을 이유도 없다. 그 비용은
    /// [`ProcessesToUpdate::Some`](sysinfo::ProcessesToUpdate::Some)으로
    /// 기록에 적힌 pid만 새로고침해 한 번 더 줄인다 — 전체 열거를 부르지 않는
    /// 것은 agent.md §3.3 기록 2가 `sysinfo`의 다른 API에서 겪은 종류의 비용을
    /// 이 경로에 들이지 않기 위해서다.
    pub async fn sweep_stale_incarnation(&self) -> Vec<AgentOrphan> {
        let records = self.read_spawn_records().await;
        if records.is_empty() {
            return Vec::new();
        }

        let pids: Vec<sysinfo::Pid> = records
            .iter()
            .map(|r| sysinfo::Pid::from_u32(r.pid))
            .collect();
        let mut sys = sysinfo::System::new();
        sys.refresh_processes(sysinfo::ProcessesToUpdate::Some(&pids), true);

        let mut orphans = Vec::new();
        for record in records {
            if let Some(proc) = sys.process(sysinfo::Pid::from_u32(record.pid)) {
                let observed = proc.start_time();
                if observed.abs_diff(record.started_at_unix) <= START_TIME_SLACK_SECS {
                    // 죽이는 것과 신고하는 것을 **묶지 않는다.** kill이 실패해도
                    // (권한 부족 등) 고아는 거기 있고 포트를 쥐고 있으므로,
                    // 운영자가 알아야 할 사실은 달라지지 않는다.
                    let killed = proc.kill();
                    warn!(
                        agent_id = %record.agent_id,
                        pid = record.pid,
                        port = record.port,
                        killed,
                        "terminating an agent process left by a previous worker incarnation"
                    );
                    orphans.push(AgentOrphan {
                        agent_id: record.agent_id,
                        reason: AgentOrphanReason::StaleIncarnation,
                    });
                } else {
                    // pid가 재사용됐다. **아무것도 하지 않는다** — 이 자리에서
                    // 틀리면 남의 프로세스를 죽인다.
                    debug!(
                        agent_id = %record.agent_id,
                        pid = record.pid,
                        recorded = record.started_at_unix,
                        observed,
                        "pid was reused by an unrelated process — leaving it alone"
                    );
                }
            }
            // 판정이 어느 쪽이든 기록은 지운다. 남기면 다음 기동이 같은 판정을
            // 다시 하는데, 그때는 시간이 더 흘러 pid 재사용 가능성만 커진다.
            self.remove_spawn_record(record.agent_id).await;
        }
        orphans
    }

    /// workspace 루트 아래의 모든 [`SPAWN_RECORD`]를 읽는다.
    ///
    /// 읽을 수 없는 파일은 **지우고 넘어간다.** 이 파일을 쓰는 것은 우리뿐이라
    /// 깨진 내용은 부분 쓰기의 흔적이고, 남겨 두면 매 기동마다 같은 경고가
    /// 나오면서 아무 판정도 만들지 못한다.
    async fn read_spawn_records(&self) -> Vec<SpawnRecord> {
        let mut out = Vec::new();
        let mut dir = match tokio::fs::read_dir(&self.workspace_root).await {
            Ok(d) => d,
            // 아직 Agent를 한 번도 띄우지 않은 Worker의 정상 경로다.
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return out,
            Err(e) => {
                warn!(
                    root = %self.workspace_root.display(),
                    error = %e,
                    "cannot scan the agent workspace root for spawn records"
                );
                return out;
            }
        };
        while let Ok(Some(entry)) = dir.next_entry().await {
            let path = entry.path().join(SPAWN_RECORD);
            let Ok(body) = tokio::fs::read(&path).await else {
                continue;
            };
            match serde_json::from_slice::<SpawnRecord>(&body) {
                Ok(r) => out.push(r),
                Err(e) => {
                    warn!(
                        path = %path.display(),
                        error = %e,
                        "discarding an unreadable agent spawn record"
                    );
                    let _ = tokio::fs::remove_file(&path).await;
                }
            }
        }
        out
    }

    /// heartbeat 응답의 명령 목록에 프로세스 집합을 수렴시키고, **본 것**을
    /// 돌려준다 (관측은 로드맵 `#67` 4c-B).
    ///
    /// 실패는 **전파하지 않는다** — 한 Agent를 못 띄운 것이 heartbeat 루프를
    /// 멈추면 나머지 Agent의 명령도 함께 끊긴다. 대신 그 실패가 반환값에
    /// [`AgentObservation::Failed`]로 실려 오케스트레이터에 도달한다. 4c-A에서
    /// 이 자리는 워커 로그 한 줄이 전부인 **조용한 실패 모드**였다.
    ///
    /// 반환값의 `None`은 명령 목록이 `None`이었다는 뜻, 즉 "이번 beat에는 할 말이
    /// 없다"다. `Some(vec![])`은 "이 Worker에 관측할 것이 하나도 없다"이며 그
    /// 구분은 명령 목록의 그것과 정확히 대칭이다.
    ///
    /// 관측 목록은 **desired가 `running`인 Agent만** 담는다. 정리한 Agent를 담지
    /// 않는 이유는 관측 어휘에 "없음"에 해당하는 값이 없어서이고, 그것으로
    /// 충분하다 — 오케스트레이터는 목록에 없는 것의 관측을 지운다.
    ///
    /// 그런데 그 "지운다"가 닿지 못하는 경우가 하나 있고, 그것이
    /// [`ReconcileOutcome::orphans`]가 있는 이유다 (로드맵 `#70` 게이트 ③).
    /// 명령 목록에서 **사라진** Agent의 프로세스를 아래 2단계가 종료하는데,
    /// 그 Agent는 이미 다른 Worker에 배정됐을 수 있어 관측을 적을 자리가
    /// 이 Worker에는 없다(`036`의 CHECK). 그래서 종료 사실이 지금까지
    /// **워커 로그 한 줄로만** 남았다 — 4c-A의 거절이 그랬던 것과 같은 모양의
    /// 조용한 실패 모드이며, 여기서 그것을 사건으로 만들어 올려 보낸다.
    pub async fn reconcile(&self, commands: Option<&[AgentCommand]>) -> Option<ReconcileOutcome> {
        let Some(commands) = commands else {
            debug!("no authoritative agent list this beat — leaving processes untouched");
            return None;
        };

        let mut procs = self.procs.lock().await;

        // 0. 죽은 자식을 먼저 걷어낸다. 이 단계가 앞에 있어야 아래 3단계의
        //    "desired=running인데 없으면 띄운다"가 **재기동을 겸한다** — 별도
        //    재시작 경로를 만들면 상한 검사가 두 곳으로 갈라진다.
        let mut dead = Vec::new();
        for (id, proc) in procs.iter_mut() {
            match proc.child.try_wait() {
                Ok(None) => {}
                Ok(Some(status)) => {
                    warn!(agent_id = %id, ?status, "agent process exited");
                    dead.push(*id);
                }
                Err(e) => {
                    // 상태를 물어볼 수 없으면 살아 있다고 볼 근거가 없다.
                    warn!(agent_id = %id, error = %e, "cannot poll agent process — treating as dead");
                    dead.push(*id);
                }
            }
        }
        for id in dead {
            procs.remove(&id);
            // 죽은 자식의 기록을 남겨 두면 다음 재기동의 sweep이 그것을 고아로
            // 신고한다. 그때 pid는 이미 남의 것일 수 있고, 이름 대조가 그것을
            // 걸러 주더라도 없는 사건을 매번 다시 판정하는 비용이 남는다.
            self.remove_spawn_record(id).await;
        }

        // 1. 살려 둘 집합 = 목록에 있고 desired가 running인 것.
        let keep: HashSet<AgentId> = commands
            .iter()
            .filter(|c| c.desired_status == AgentDesiredStatus::Running)
            .map(|c| c.agent_id)
            .collect();

        // 2. 그 밖은 정리한다 — `stopped` 명령을 받은 것과 **목록에서 사라진
        //    것**이 같은 취급을 받는다. 후자는 다른 Worker로 재배정됐거나 이미
        //    회수가 확인된 경우이며, 어느 쪽이든 이 Worker가 계속 들고 있을
        //    이유가 없다.
        let to_stop: Vec<AgentId> = procs
            .keys()
            .copied()
            .filter(|id| !keep.contains(id))
            .collect();

        // 2-a. 부재와 명시적 `stopped`를 **여기서 처음으로 구분한다.** 종료
        //      자체는 같지만 보고가 다르다 — 명시적 회수는 오케스트레이터가
        //      시킨 일이라 새로 알려 줄 것이 없고, 부재는 그쪽이 모르는
        //      사건이다. `mentioned`가 `keep`이 아니라 **전체 명령 목록**에서
        //      나오는 것이 이 구분의 전부다.
        let mentioned: HashSet<AgentId> = commands.iter().map(|c| c.agent_id).collect();
        let orphans: Vec<AgentOrphan> = to_stop
            .iter()
            .filter(|id| !mentioned.contains(id))
            .map(|id| {
                warn!(
                    agent_id = %id,
                    reason = AgentOrphanReason::Unplaced.as_str(),
                    "terminating an agent process the orchestrator no longer places here"
                );
                AgentOrphan {
                    agent_id: *id,
                    reason: AgentOrphanReason::Unplaced,
                }
            })
            .collect();

        let doomed: Vec<(AgentId, AgentProc)> = to_stop
            .into_iter()
            .filter_map(|id| procs.remove(&id).map(|p| (id, p)))
            .collect();
        for (id, _) in &doomed {
            self.remove_spawn_record(*id).await;
        }
        terminate_all(doomed).await;

        // 2.5. 회수가 **명시된** Agent의 디렉터리를 지운다. 위 2단계가 부재와
        //      `stopped`를 같이 취급한 것과 달리 여기서는 **명시적 `stopped`만**
        //      본다 — 종료는 잘못해도 다시 띄우면 되지만 삭제는 되돌릴 수 없다.
        //      근거는 `remove_workspace`의 독스트링에 있다.
        //
        //      최악의 경우 창은 한 beat이다: `registration.rs`가 응답을 파싱하는
        //      시점에 ack를 버퍼에 넣고 그것이 다음 beat의 요청에 실리는데,
        //      오케스트레이터의 heartbeat 핸들러는 `ack_agent_commands`를
        //      `list_agent_commands`보다 **먼저** 부르므로 그 응답에는 이미 이
        //      명령이 없다. (회수가 `status`까지 내리지 않은 경우에는 첫 disjunct
        //      가 살아 있어 더 오래 실려 오지만, 짧은 쪽에 맞춰 설계한다.)
        //
        //      그래서 실패는 전파하지 않고 경고만 남긴다 — 남은 디렉터리는
        //      누수이고, 되돌릴 수 없는 쪽으로 틀리는 것보다 낫다.
        for cmd in commands
            .iter()
            .filter(|c| c.desired_status == AgentDesiredStatus::Stopped)
        {
            match self.remove_workspace(cmd.agent_id).await {
                Ok(true) => info!(agent_id = %cmd.agent_id, "agent workspace removed"),
                Ok(false) => {}
                Err(e) => warn!(
                    agent_id = %cmd.agent_id,
                    error = %e,
                    "failed to remove agent workspace — it will leak"
                ),
            }
        }

        // 3. 있어야 하는데 없는 것을 띄우고, 그 결과를 그대로 관측으로 적는다.
        //    관측을 여기서 만드는 이유는 이 루프가 desired=running인 Agent를
        //    **정확히 한 번씩** 지나가기 때문이다 — 뒤에서 다시 훑으면 0단계가
        //    걷어낸 자식과 방금 띄운 자식을 구분할 근거가 사라진다.
        let mut observations = Vec::with_capacity(commands.len());
        for cmd in commands
            .iter()
            .filter(|c| c.desired_status == AgentDesiredStatus::Running)
        {
            if procs.contains_key(&cmd.agent_id) {
                observations.push(AgentObservation::Running {
                    agent_id: cmd.agent_id,
                });
                continue;
            }

            // 자리 확보. 상한과 포트가 **하나의 Result**로 합쳐지므로 아래
            // 거절 로그는 정확히 한 곳이다.
            let slot = if procs.len() >= self.max_processes {
                Err(RejectReason::CapReached)
            } else {
                self.free_port(&procs).ok_or(RejectReason::NoFreePort)
            };

            let port = match slot {
                Ok(p) => p,
                Err(reason) => {
                    warn!(
                        agent_id = %cmd.agent_id,
                        generation = cmd.generation,
                        reason = reason.as_str(),
                        running = procs.len(),
                        max = self.max_processes,
                        port_range = %self.config.grok.agent_port_range,
                        "agent process not started this beat"
                    );
                    observations.push(AgentObservation::Failed {
                        agent_id: cmd.agent_id,
                        reason: reason.into(),
                    });
                    continue;
                }
            };

            match self.spawn(cmd.agent_id, port).await {
                Ok(child) => {
                    info!(
                        agent_id = %cmd.agent_id,
                        generation = cmd.generation,
                        port,
                        "agent process started"
                    );
                    procs.insert(cmd.agent_id, AgentProc { child, port });
                    observations.push(AgentObservation::Running {
                        agent_id: cmd.agent_id,
                    });
                }
                Err(e) => {
                    warn!(
                        agent_id = %cmd.agent_id,
                        generation = cmd.generation,
                        port,
                        error = %e,
                        "failed to spawn agent process"
                    );
                    observations.push(AgentObservation::Failed {
                        agent_id: cmd.agent_id,
                        reason: AgentObservationReason::SpawnFailed,
                    });
                }
            }
        }

        Some(ReconcileOutcome {
            observations,
            orphans,
        })
    }

    /// 모든 Agent 프로세스를 종료한다. Worker 종료 경로에서 호출한다.
    ///
    /// `kill_on_drop`이 있어도 명시적으로 부르는 이유: drop은 SIGKILL이고,
    /// 여기서는 singleton과 같은 5초 유예를 준다.
    pub async fn shutdown_all(&self) {
        let mut procs = self.procs.lock().await;
        let drained: Vec<(AgentId, AgentProc)> = procs.drain().collect();
        // 깨끗한 종료에서는 기록을 남기지 않는다. 남기면 다음 기동의 sweep이
        // **정상 종료를 고아로** 신고한다 — 이 경로와 SIGKILL 경로의 차이가
        // 바로 그 파일의 유무다.
        for (id, _) in &drained {
            self.remove_spawn_record(*id).await;
        }
        terminate_all(drained).await;
    }

    /// 제어면을 잃었을 때 이 Worker의 Agent 프로세스를 **전부** 멈추고, 멈춘
    /// 것들의 id를 돌려준다 (로드맵 `#67` 게이트 ⑥).
    ///
    /// [`Self::shutdown_all`]과 프로세스를 다루는 방식은 같고 두 가지가 다르다.
    ///
    /// 첫째, **workspace를 지우지 않는다.** 배정은 여전히 유효하고 연결이
    /// 돌아오면 같은 Agent가 같은 자리에서 다시 뜬다 — 여기서 지우면 펜싱이
    /// 네트워크 단절을 작업 손실로 바꾼다. workspace를 지우는 것은
    /// `remove_workspace`가 다루는 **미배치** 경로뿐이다.
    ///
    /// 둘째, 멈춘 id를 돌려준다. 상태는 다음 beat의 관측 목록이 저절로
    /// 바로잡지만(멈춘 Agent가 목록에서 빠지면 오케스트레이터가 그 관측을
    /// 지운다) 그 경로는 **왜** 멈췄는지를 나르지 못한다.
    ///
    /// spawn 기록은 `shutdown_all`과 같은 이유로 지운다 — 이것은 의도된 정지라
    /// 기록을 남기면 다음 기동의 sweep이 **정상 종료를 고아로** 신고한다.
    ///
    /// 두 번 불러도 안전하다. 첫 호출이 표를 비우므로 두 번째는 빈 목록을
    /// 돌려주고, 그래서 호출자가 "이미 펜싱했다"는 상태를 따로 들 필요가 없다.
    pub async fn fence_all(&self) -> Vec<AgentId> {
        let mut procs = self.procs.lock().await;
        let drained: Vec<(AgentId, AgentProc)> = procs.drain().collect();
        let fenced: Vec<AgentId> = drained.iter().map(|(id, _)| *id).collect();
        for id in &fenced {
            self.remove_spawn_record(*id).await;
        }
        terminate_all(drained).await;
        fenced
    }

    /// 범위에서 아직 쓰지 않는 포트 하나.
    ///
    /// 우리가 이미 잡은 포트를 제외한 뒤, 남은 후보를 **실제로 bind해 본다**.
    /// 이 프로세스 밖의 점유(다른 데몬)를 걸러내기 위한 것이며, bind와 자식의
    /// bind 사이에는 창이 남는다 — 그 창을 닫으려면 자식에게 소켓을 물려줘야
    /// 하는데 `grok agent serve`가 그 인터페이스를 갖고 있지 않다.
    fn free_port(&self, procs: &HashMap<AgentId, AgentProc>) -> Option<u16> {
        let taken: HashSet<u16> = procs.values().map(|p| p.port).collect();
        let (lo, hi) = self.port_range;
        (lo..=hi).find(|port| {
            !taken.contains(port)
                && std::net::TcpListener::bind((self.host.as_str(), *port)).is_ok()
        })
    }

    /// Agent 하나의 grok 프로세스를 띄운다.
    ///
    /// **secret은 여기서 만들고 보관하지 않는다.** 4c-A에서 소비자는 자식
    /// 프로세스 자신뿐이므로 매니저가 들고 있으면 읽는 사람이 없는 필드가 된다.
    /// 자식이 죽으면 다음 beat에 **새 secret으로** 다시 뜬다.
    ///
    /// `grok.secret`을 재사용하지 않는 이유는 그 값이 이미 Worker의 ACP 종단을
    /// 여는 열쇠이기 때문이다 — 같은 값을 주면 Agent 하나가 샜을 때 Worker
    /// 종단까지 열린다.
    async fn spawn(&self, agent_id: AgentId, port: u16) -> Result<Child, std::io::Error> {
        let workspace = self.ensure_workspace(agent_id).await?;

        let secret = hex::encode(rand::random::<[u8; 32]>());
        let bind = format!("{}:{}", self.host, port);

        let mut cmd = Command::new(&self.config.grok.bin);
        cmd.arg("agent")
            .arg("serve")
            .arg("--bind")
            .arg(&bind)
            .arg("--secret")
            .arg(&secret)
            .current_dir(&workspace)
            .kill_on_drop(true);
        apply_llm_proxy_envs(&mut cmd, &self.config.llm_proxy);
        let child = cmd.spawn()?;

        // 기록은 spawn **직후**에 쓴다 (로드맵 `#70` 게이트 ③). 순서가 뒤집혀
        // 프로세스가 먼저 뜨고 기록이 나중이면, 그 사이에 Worker가 죽었을 때
        // 아무 흔적도 남기지 않은 고아가 생긴다 — 이 기록이 막으려는 바로 그
        // 상태다.
        match child.id() {
            Some(pid) => {
                self.write_spawn_record(agent_id, pid, port, &workspace)
                    .await
            }
            // 아직 살아 있는데 pid를 물어볼 수 없는 경우는 없다. `None`은 이미
            // 죽어 거둬진 자식이라는 뜻이고, 그런 자식에 대한 기록은 다음
            // 재기동의 sweep이 판정할 대상이 아니다.
            None => warn!(
                agent_id = %agent_id,
                "spawned agent process has no pid — it will not be sweepable"
            ),
        }
        Ok(child)
    }

    /// [`SPAWN_RECORD`]를 쓴다.
    ///
    /// 실패를 **전파하지 않는다.** 기록이 없어도 이번 incarnation의 동작은
    /// 온전하고, 잃는 것은 다음 재기동의 sweep이 이 자식을 볼 수 있는 능력뿐이다.
    /// 그것 때문에 방금 정상적으로 뜬 Agent를 죽이는 것은 손해가 더 크다.
    async fn write_spawn_record(
        &self,
        agent_id: AgentId,
        pid: u32,
        port: u16,
        workspace: &std::path::Path,
    ) {
        let record = SpawnRecord {
            agent_id,
            pid,
            port,
            started_at_unix: now_unix(),
        };
        let body = match serde_json::to_vec(&record) {
            Ok(b) => b,
            Err(e) => {
                warn!(agent_id = %agent_id, error = %e, "cannot serialise the agent spawn record");
                return;
            }
        };
        let path = workspace.join(SPAWN_RECORD);
        if let Err(e) = tokio::fs::write(&path, body).await {
            warn!(
                agent_id = %agent_id,
                path = %path.display(),
                error = %e,
                "failed to record the spawned agent process — it will not be sweepable"
            );
        }
    }

    /// 기록을 지운다. 없으면 조용히 성공한다.
    ///
    /// **매니저가 그 프로세스를 더 이상 믿지 않게 되는 모든 자리에서 부른다.**
    /// 기록이 남으면 다음 재기동의 sweep이 이미 끝난 일을 고아로 신고한다.
    async fn remove_spawn_record(&self, agent_id: AgentId) {
        let path = self
            .workspace_root
            .join(agent_id.to_string())
            .join(SPAWN_RECORD);
        match tokio::fs::remove_file(&path).await {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => warn!(
                agent_id = %agent_id,
                error = %e,
                "failed to remove the agent spawn record"
            ),
        }
    }

    /// Agent 디렉터리를 만들고 **root 아래로 해석되는지 확인한** 경로를 준다
    /// (로드맵 `#69` 1단계).
    ///
    /// 여기서 값어치를 하는 검사는 `..` 거절이 아니다 — `agent_id`는 UUID라
    /// 경로 조각에 구분자도 `..`도 섞일 수 없다. 실제로 막는 것은 **symlink**다:
    /// `workspace_root/<agent_id>`가 이미 링크로 존재하면 `create_dir_all`은
    /// 조용히 성공하고, 그 뒤의 모든 쓰기는 링크가 가리키는 곳으로 간다.
    ///
    /// root **자신이** 링크인 경우는 위반이 아니다. 양쪽을 다 정규화하므로
    /// root가 가리키는 실제 경로 아래에 자식이 놓이고 `starts_with`가 성립한다 —
    /// 운영자가 workspace를 다른 볼륨에 두는 것은 정상 구성이다. 한쪽만
    /// 정규화하면 이 정상 구성이 위반으로 보고된다.
    ///
    /// 이 검사가 [`Task.cwd`]의 containment와 다른 이유는 **경로를 정한 쪽과
    /// 확인하는 쪽이 같은 호스트**이기 때문이다. 거기서 막힌 것은 오케스트레이터가
    /// 남의 파일시스템을 `canonicalize`할 수 없다는 사실이었지 검사 자체가 아니다.
    ///
    /// [`Task.cwd`]: fleet_core::validate_workspace_cwd
    async fn ensure_workspace(&self, agent_id: AgentId) -> Result<PathBuf, std::io::Error> {
        tokio::fs::create_dir_all(&self.workspace_root).await?;
        let path = self.workspace_root.join(agent_id.to_string());
        tokio::fs::create_dir_all(&path).await?;
        self.contained(&path).await
    }

    /// `path`를 정규화하고 workspace root 아래인지 확인한다. 존재해야 한다.
    async fn contained(&self, path: &std::path::Path) -> Result<PathBuf, std::io::Error> {
        let root = tokio::fs::canonicalize(&self.workspace_root).await?;
        let canon = tokio::fs::canonicalize(path).await?;
        if !canon.starts_with(&root) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!(
                    "agent workspace {} resolves outside {}",
                    canon.display(),
                    root.display()
                ),
            ));
        }
        Ok(canon)
    }

    /// 회수된 Agent의 디렉터리를 지운다. 지울 것이 없었으면 `Ok(false)`
    /// (로드맵 `#69` 1단계).
    ///
    /// **호출 조건이 이 함수의 안전성 전부다.** 부르는 쪽은 명령 목록에
    /// `desired_status = stopped`가 **명시적으로 실려 있을 때만** 부른다. 목록에서
    /// 사라진 것을 근거로 부르면 안 된다 — `list_agent_commands`의 술어상 다른
    /// Worker로 이동, 미배치(`worker_id = NULL`), 회수 확인 완료가 모두 "부재"로
    /// 뭉쳐지고, 그중 앞의 둘에서 지우면 아직 살아 있는 Agent의 작업물이
    /// 사라진다. checkpoint push가 없는 지금 그 소실에는 복구 경로가 없다.
    ///
    /// 삭제 전에 [`contained`](Self::contained)를 다시 통과시킨다. 만든 시점과
    /// 지우는 시점 사이에 링크가 끼어들 수 있고, 재귀 삭제는 되돌릴 수 없으므로
    /// 여기서의 한 번 더가 생성 시의 검사보다 값이 크다.
    async fn remove_workspace(&self, agent_id: AgentId) -> Result<bool, std::io::Error> {
        let path = self.workspace_root.join(agent_id.to_string());
        let canon = match self.contained(&path).await {
            Ok(c) => c,
            // 애초에 없으면 지울 것도 없다 — 매 beat 반복되는 정상 경로다.
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(false),
            Err(e) => return Err(e),
        };
        tokio::fs::remove_dir_all(&canon).await?;
        Ok(true)
    }
}

/// 지금의 벽시계 (epoch 초).
///
/// 시계가 epoch 이전으로 설정된 호스트에서는 0을 준다. 그 값은
/// [`START_TIME_SLACK_SECS`] 대조를 통과하지 못하므로, 고장난 시계는 고아를
/// **놓치는** 쪽으로 틀린다 — 남의 프로세스를 죽이는 쪽으로 틀리지 않는다.
fn now_unix() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// 여러 Agent 프로세스를 **동시에** 종료한다.
///
/// 직렬로 돌면 안 되는 이유가 구체적이다: `terminate_child`는 SIGTERM을 보내지
/// 않고 자식이 스스로 끝나기를 5초 기다린 뒤 SIGKILL한다. `grok agent serve`는
/// 그 신호를 받지 못하므로 **항상** 5초를 다 쓴다. 직렬이면 상한 4개 기준
/// 20초가 되고, 그동안 heartbeat 루프가 lock을 잡은 채 멈춰 beat을 건너뛴다.
/// 동시에 돌리면 배치 전체가 약 5초다.
async fn terminate_all(procs: Vec<(AgentId, AgentProc)>) {
    let handles: Vec<_> = procs
        .into_iter()
        .map(|(id, mut proc)| {
            tokio::spawn(async move {
                terminate_child(&mut proc.child, &format!("agent {id}")).await;
                info!(agent_id = %id, port = proc.port, "agent process stopped");
            })
        })
        .collect();
    for h in handles {
        let _ = h.await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    // 구현부는 `AgentObservation`만 만들고 그 안을 들여다보지 않는다.
    // 상태를 **읽는** 쪽은 테스트뿐이라 여기서만 가져온다.
    use fleet_core::AgentObservedStatus;
    use std::io::Write;

    /// 인자를 무시하고 오래 자는 가짜 grok. 실제 바이너리 없이 프로세스
    /// 수명만 시험하기 위한 것이다.
    fn fake_grok(dir: &std::path::Path) -> String {
        let path = dir.join("fake-grok.sh");
        let mut f = std::fs::File::create(&path).unwrap();
        writeln!(f, "#!/bin/sh\nexec sleep 300").unwrap();
        drop(f);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        path.to_string_lossy().into_owned()
    }

    /// 즉시 종료하는 가짜 grok.
    fn dying_grok(dir: &std::path::Path) -> String {
        let path = dir.join("dying-grok.sh");
        let mut f = std::fs::File::create(&path).unwrap();
        writeln!(f, "#!/bin/sh\nexit 3").unwrap();
        drop(f);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        path.to_string_lossy().into_owned()
    }

    fn manager(dir: &tempfile::TempDir, bin: String, range: &str, max: u32) -> AgentProcessManager {
        let config = WorkerConfig::for_test()
            .grok_bin(bin)
            .bind_addr("127.0.0.1:2419")
            .agent_port_range(range)
            .agent_workspace_root(dir.path().to_string_lossy().into_owned())
            .max_agent_processes(max)
            .build();
        AgentProcessManager::new(Arc::new(config)).unwrap()
    }

    fn cmd(id: AgentId, desired: AgentDesiredStatus) -> AgentCommand {
        AgentCommand {
            agent_id: id,
            desired_status: desired,
            generation: 1,
        }
    }

    /// 살아 있는 자식 하나를 만들고 그것을 가리키는 [`SPAWN_RECORD`]를 심는다.
    ///
    /// 이전 incarnation이 죽은 상황을 재현하는 방법이다 — 매니저의 `procs`는
    /// 비어 있는데 디스크에는 기록이 있고 프로세스는 살아 있다. 실제 워커를
    /// SIGKILL하는 대신 이렇게 만드는 이유는, 재현해야 하는 것이 "워커가
    /// 죽는 방식"이 아니라 **그 결과로 남는 상태**이기 때문이다.
    fn plant_stale_record(
        root: &std::path::Path,
        agent_id: AgentId,
        port: u16,
        started_at_unix: u64,
    ) -> std::process::Child {
        let child = std::process::Command::new("sleep")
            .arg("300")
            .spawn()
            .expect("sleep은 어느 유닉스에나 있다");
        let dir = root.join(agent_id.to_string());
        std::fs::create_dir_all(&dir).unwrap();
        let record = SpawnRecord {
            agent_id,
            pid: child.id(),
            port,
            started_at_unix,
        };
        std::fs::write(dir.join(SPAWN_RECORD), serde_json::to_vec(&record).unwrap()).unwrap();
        child
    }

    /// `sleep`이 아직 살아 있는지 본다. `try_wait`은 거둬 가므로 판정 뒤에도
    /// 핸들이 유효하다.
    fn still_alive(child: &mut std::process::Child) -> bool {
        matches!(child.try_wait(), Ok(None))
    }

    /// **이 테스트가 이 변경의 핵심 단정 하나다.** 목록에서 사라진 것을
    /// 종료하는 동작 자체는 이미 있었고(`absence_from_the_command_list_...`),
    /// 없던 것은 그 종료가 오케스트레이터에 **도달하는 경로**다. 그때까지
    /// 워커 로그 한 줄이 전부였다.
    #[tokio::test]
    async fn absence_from_the_list_is_reported_as_an_orphan() {
        let dir = tempfile::tempdir().unwrap();
        let m = manager(&dir, fake_grok(dir.path()), "39400-39419", 4);
        let a = AgentId::new();

        let _ = m
            .reconcile(Some(&[cmd(a, AgentDesiredStatus::Running)]))
            .await;

        // 다음 beat: 권위 있는 목록이 왔는데 이 Agent가 없다.
        let outcome = m.reconcile(Some(&[])).await.expect("목록을 줬으므로 온다");

        assert_eq!(
            outcome.orphans,
            vec![AgentOrphan {
                agent_id: a,
                reason: AgentOrphanReason::Unplaced,
            }],
            "부재로 종료한 프로세스는 사건으로 보고된다"
        );
        assert!(
            outcome.observations.is_empty(),
            "관측 목록에 섞이면 안 된다 — 저쪽은 권위 있는 전체 집합이라 \
             orphan의 id가 들어가면 지워야 할 관측이 살아남는다"
        );
        m.shutdown_all().await;
    }

    /// 앞 테스트의 짝. **명시적 회수는 고아가 아니다** — 오케스트레이터가
    /// 시킨 일이라 새로 알려 줄 것이 없다. 이 단정이 없으면 부재와 회수를
    /// 구분하지 않는 구현(`!keep.contains`만 보는 것)도 앞 테스트를 통과한다.
    #[tokio::test]
    async fn an_explicit_stop_is_not_an_orphan() {
        let dir = tempfile::tempdir().unwrap();
        let m = manager(&dir, fake_grok(dir.path()), "39420-39439", 4);
        let a = AgentId::new();

        let _ = m
            .reconcile(Some(&[cmd(a, AgentDesiredStatus::Running)]))
            .await;
        let outcome = m
            .reconcile(Some(&[cmd(a, AgentDesiredStatus::Stopped)]))
            .await
            .expect("목록을 줬으므로 온다");

        assert!(m.running_agents().await.is_empty(), "회수는 종료시킨다");
        assert!(
            outcome.orphans.is_empty(),
            "시킨 대로 한 것은 사건이 아니다"
        );
        m.shutdown_all().await;
    }

    /// 이전 incarnation이 남긴 프로세스를 재기동 sweep이 찾아 죽이고 신고한다.
    ///
    /// 이 경로가 없으면 그 자식은 **영원히 보이지 않는다** — 새 매니저의
    /// `procs`는 비어 있어 `reconcile`이 원리적으로 도달하지 못한다.
    #[cfg(unix)]
    #[tokio::test]
    async fn a_process_left_by_a_previous_incarnation_is_swept() {
        let dir = tempfile::tempdir().unwrap();
        let m = manager(&dir, fake_grok(dir.path()), "39440-39459", 4);
        let a = AgentId::new();
        let mut child = plant_stale_record(dir.path(), a, 39440, now_unix());

        let orphans = m.sweep_stale_incarnation().await;

        assert_eq!(
            orphans,
            vec![AgentOrphan {
                agent_id: a,
                reason: AgentOrphanReason::StaleIncarnation,
            }]
        );
        // SIGKILL이 실제로 닿았는지 본다. 신고만 하고 죽이지 않으면 그 자식은
        // 포트를 계속 쥐고, 오케스트레이터가 같은 Agent를 다시 배정할 때
        // 두 번째 프로세스가 뜬다.
        for _ in 0..50 {
            if !still_alive(&mut child) {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
        assert!(!still_alive(&mut child), "sweep은 신고만 하지 않고 죽인다");
        assert!(
            !dir.path().join(a.to_string()).join(SPAWN_RECORD).exists(),
            "판정이 끝난 기록은 지운다"
        );
        // 작업물은 남긴다 — 종료는 잘못해도 다시 띄우면 되지만 삭제는
        // 되돌릴 수 없다는 `remove_workspace`의 판단이 여기서도 그대로다.
        assert!(dir.path().join(a.to_string()).is_dir());
        let _ = child.kill();
    }

    /// **pid 재사용 방어.** 기록의 시작 시각이 실제 프로세스의 것과 어긋나면
    /// 아무것도 하지 않는다.
    ///
    /// 이 단정이 없으면 sweep은 "기록에 적힌 pid를 죽이는 코드"가 되고,
    /// Worker가 죽은 뒤 pid가 순환한 호스트에서 **남의 프로세스를 죽인다**.
    /// Linux의 기본 `pid_max`가 32768이라 그 순환은 드문 일이 아니다.
    #[cfg(unix)]
    #[tokio::test]
    async fn a_reused_pid_is_left_alone() {
        let dir = tempfile::tempdir().unwrap();
        let m = manager(&dir, fake_grok(dir.path()), "39460-39479", 4);
        let a = AgentId::new();
        // 우리가 적었다고 주장하는 시각이 실제 프로세스의 시작보다 한참 전이다.
        let mut child = plant_stale_record(dir.path(), a, 39460, now_unix() - 3600);

        let orphans = m.sweep_stale_incarnation().await;

        assert!(orphans.is_empty(), "우리 것이라고 말할 근거가 없다");
        assert!(
            still_alive(&mut child),
            "근거가 없으면 죽이지 않는다 — 이 자리에서 틀리면 남의 프로세스를 죽인다"
        );
        assert!(
            !dir.path().join(a.to_string()).join(SPAWN_RECORD).exists(),
            "판정이 끝났으므로 기록은 지운다 — 남기면 다음 기동에 시간이 더 \
             흘러 재사용 가능성만 커진 채로 같은 판정을 다시 한다"
        );
        let _ = child.kill();
    }

    /// 깨끗한 종료는 기록을 남기지 않는다. 남기면 다음 기동의 sweep이
    /// **정상 종료를 고아로** 신고한다 — 이 경로와 SIGKILL 경로의 차이가
    /// 바로 그 파일의 유무이므로, 이 단정이 sweep 전체의 거짓 양성률을 정한다.
    #[tokio::test]
    async fn a_clean_shutdown_leaves_nothing_to_sweep() {
        let dir = tempfile::tempdir().unwrap();
        let m = manager(&dir, fake_grok(dir.path()), "39480-39499", 4);
        let a = AgentId::new();

        let _ = m
            .reconcile(Some(&[cmd(a, AgentDesiredStatus::Running)]))
            .await;
        assert!(
            dir.path().join(a.to_string()).join(SPAWN_RECORD).exists(),
            "떠 있는 동안에는 기록이 있어야 한다"
        );
        m.shutdown_all().await;

        assert!(
            !dir.path().join(a.to_string()).join(SPAWN_RECORD).exists(),
            "깨끗한 종료는 기록을 지운다"
        );
        let m2 = manager(&dir, fake_grok(dir.path()), "39480-39499", 4);
        assert!(
            m2.sweep_stale_incarnation().await.is_empty(),
            "그러므로 다음 기동은 신고할 것이 없다"
        );
    }

    /// 펜싱은 돌던 것을 전부 멈추고 **무엇을 멈췄는지** 돌려준다. 그 목록이
    /// 없으면 오케스트레이터는 Agent가 왜 멈췄는지 영영 알 수 없다 — 상태는
    /// 다음 beat의 관측이 바로잡지만 이유를 나르는 경로는 이것뿐이다.
    #[tokio::test]
    async fn fencing_stops_every_process_and_names_them() {
        let dir = tempfile::tempdir().unwrap();
        let m = manager(&dir, fake_grok(dir.path()), "39520-39539", 4);
        let (a, b) = (AgentId::new(), AgentId::new());

        let _ = m
            .reconcile(Some(&[
                cmd(a, AgentDesiredStatus::Running),
                cmd(b, AgentDesiredStatus::Running),
            ]))
            .await;
        assert_eq!(m.running_agents().await.len(), 2, "둘 다 떠 있어야 한다");

        let mut fenced = m.fence_all().await;
        fenced.sort();
        let mut expected = vec![a, b];
        expected.sort();

        assert_eq!(fenced, expected, "멈춘 것을 전부 이름으로 돌려준다");
        assert!(
            m.running_agents().await.is_empty(),
            "펜싱 뒤에 남아 있는 프로세스가 없어야 한다"
        );
    }

    /// **workspace는 지우지 않는다.** 배정은 여전히 유효하고 연결이 돌아오면
    /// 같은 Agent가 같은 자리에서 다시 뜬다 — 여기서 지우면 펜싱이 네트워크
    /// 단절을 작업 손실로 바꾼다. 미배치 경로(`remove_workspace`)와 갈리는
    /// 자리가 정확히 여기다.
    #[tokio::test]
    async fn fencing_keeps_the_workspace() {
        let dir = tempfile::tempdir().unwrap();
        let m = manager(&dir, fake_grok(dir.path()), "39540-39559", 4);
        let a = AgentId::new();

        let _ = m
            .reconcile(Some(&[cmd(a, AgentDesiredStatus::Running)]))
            .await;
        let ws = dir.path().join(a.to_string());
        assert!(ws.exists(), "떠 있는 동안 workspace가 있어야 한다");

        m.fence_all().await;

        assert!(
            ws.exists(),
            "펜싱은 프로세스만 멈춘다 — 배정이 살아 있으므로 작업물도 살아야 한다"
        );
    }

    /// 펜싱은 **의도된** 정지이므로 spawn 기록을 남기지 않는다. 남기면 다음
    /// 기동의 sweep이 이 정지를 `stale_incarnation` 고아로 신고하고, 운영자는
    /// 네트워크를 봐야 할 자리에서 SIGKILL 흔적을 쫓게 된다.
    #[tokio::test]
    async fn fencing_leaves_nothing_for_the_next_sweep_to_report() {
        let dir = tempfile::tempdir().unwrap();
        let m = manager(&dir, fake_grok(dir.path()), "39560-39579", 4);
        let a = AgentId::new();

        let _ = m
            .reconcile(Some(&[cmd(a, AgentDesiredStatus::Running)]))
            .await;
        m.fence_all().await;

        assert!(
            !dir.path().join(a.to_string()).join(SPAWN_RECORD).exists(),
            "의도된 정지는 기록을 지운다"
        );
        let m2 = manager(&dir, fake_grok(dir.path()), "39560-39579", 4);
        assert!(
            m2.sweep_stale_incarnation().await.is_empty(),
            "그러므로 다음 기동은 이 정지를 고아로 신고하지 않는다"
        );
    }

    /// 두 번째 호출은 빈 목록이다. 이것이 호출자가 "이미 펜싱했다"는 상태를
    /// 따로 들지 않아도 되는 근거이며 — 단절이 이어지는 동안 heartbeat 루프는
    /// 매 beat 이 함수를 부른다 — 같은 사건이 감사에 반복해서 쌓이지 않는
    /// 이유이기도 하다.
    #[tokio::test]
    async fn fencing_twice_reports_the_event_once() {
        let dir = tempfile::tempdir().unwrap();
        let m = manager(&dir, fake_grok(dir.path()), "39580-39599", 4);
        let a = AgentId::new();

        let _ = m
            .reconcile(Some(&[cmd(a, AgentDesiredStatus::Running)]))
            .await;

        assert_eq!(m.fence_all().await, vec![a], "첫 호출이 사건을 만든다");
        assert!(
            m.fence_all().await.is_empty(),
            "두 번째 호출은 멈출 것이 없으므로 사건도 만들지 않는다"
        );
    }

    /// 읽을 수 없는 기록은 판정을 만들지 않고 사라진다. 남겨 두면 매 기동마다
    /// 같은 경고만 나오고 아무 결론도 나지 않는다.
    #[tokio::test]
    async fn an_unreadable_record_is_discarded() {
        let dir = tempfile::tempdir().unwrap();
        let m = manager(&dir, fake_grok(dir.path()), "39500-39519", 4);
        let a = AgentId::new();
        let agent_dir = dir.path().join(a.to_string());
        std::fs::create_dir_all(&agent_dir).unwrap();
        std::fs::write(agent_dir.join(SPAWN_RECORD), b"{ truncated").unwrap();

        assert!(m.sweep_stale_incarnation().await.is_empty());
        assert!(!agent_dir.join(SPAWN_RECORD).exists());
    }

    #[tokio::test]
    async fn running_command_starts_a_process() {
        let dir = tempfile::tempdir().unwrap();
        let m = manager(&dir, fake_grok(dir.path()), "39140-39159", 4);
        let a = AgentId::new();

        let _ = m
            .reconcile(Some(&[cmd(a, AgentDesiredStatus::Running)]))
            .await;

        assert_eq!(m.running_agents().await, vec![a]);
        m.shutdown_all().await;
    }

    #[tokio::test]
    async fn reconcile_is_idempotent_across_beats() {
        let dir = tempfile::tempdir().unwrap();
        let m = manager(&dir, fake_grok(dir.path()), "39160-39179", 4);
        let a = AgentId::new();
        let list = [cmd(a, AgentDesiredStatus::Running)];

        let _ = m.reconcile(Some(&list)).await;
        let _ = m.reconcile(Some(&list)).await;
        let _ = m.reconcile(Some(&list)).await;

        // 매 beat마다 전체 목록이 다시 오지만 프로세스는 하나뿐이어야 한다.
        assert_eq!(m.running_agents().await.len(), 1);
        m.shutdown_all().await;
    }

    #[tokio::test]
    async fn stopped_command_terminates_the_process() {
        let dir = tempfile::tempdir().unwrap();
        let m = manager(&dir, fake_grok(dir.path()), "39180-39199", 4);
        let a = AgentId::new();

        let _ = m
            .reconcile(Some(&[cmd(a, AgentDesiredStatus::Running)]))
            .await;
        assert_eq!(m.running_agents().await.len(), 1);

        let _ = m
            .reconcile(Some(&[cmd(a, AgentDesiredStatus::Stopped)]))
            .await;
        assert!(m.running_agents().await.is_empty());
    }

    /// 목록에서 **사라진** Agent도 정리된다 — 재배정되었거나 회수가 확인된
    /// 경우이며, 명시적 `stopped` 명령과 같은 취급을 받는다.
    #[tokio::test]
    async fn an_agent_missing_from_the_list_is_cleaned_up() {
        let dir = tempfile::tempdir().unwrap();
        let m = manager(&dir, fake_grok(dir.path()), "39200-39219", 4);
        let (a, b) = (AgentId::new(), AgentId::new());

        let _ = m
            .reconcile(Some(&[
                cmd(a, AgentDesiredStatus::Running),
                cmd(b, AgentDesiredStatus::Running),
            ]))
            .await;
        assert_eq!(m.running_agents().await.len(), 2);

        let _ = m
            .reconcile(Some(&[cmd(a, AgentDesiredStatus::Running)]))
            .await;
        assert_eq!(m.running_agents().await, vec![a]);
        m.shutdown_all().await;
    }

    /// **`None`과 `Some([])`의 구분.** 이 두 테스트가 함께 있어야 의미가 있다 —
    /// 하나만으로는 구분이 무너졌는지 알 수 없다. 구분이 무너질 때의 대가는
    /// store 조회 실패 한 번이 그 Worker의 Agent를 전부 죽이는 것이다.
    #[tokio::test]
    async fn none_means_no_authoritative_list_and_changes_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let m = manager(&dir, fake_grok(dir.path()), "39220-39239", 4);
        let a = AgentId::new();

        let _ = m
            .reconcile(Some(&[cmd(a, AgentDesiredStatus::Running)]))
            .await;
        assert_eq!(m.running_agents().await.len(), 1);

        let _ = m.reconcile(None).await;

        assert_eq!(
            m.running_agents().await,
            vec![a],
            "None은 '목록 없음'이므로 아무것도 정리하지 않아야 한다"
        );
        m.shutdown_all().await;
    }

    #[tokio::test]
    async fn empty_list_means_genuinely_none_and_cleans_everything() {
        let dir = tempfile::tempdir().unwrap();
        let m = manager(&dir, fake_grok(dir.path()), "39240-39259", 4);
        let a = AgentId::new();

        let _ = m
            .reconcile(Some(&[cmd(a, AgentDesiredStatus::Running)]))
            .await;
        assert_eq!(m.running_agents().await.len(), 1);

        let _ = m.reconcile(Some(&[])).await;

        assert!(
            m.running_agents().await.is_empty(),
            "Some([])는 '정말로 없다'이므로 전부 정리해야 한다"
        );
    }

    #[tokio::test]
    async fn process_cap_rejects_the_excess() {
        let dir = tempfile::tempdir().unwrap();
        let m = manager(&dir, fake_grok(dir.path()), "39260-39279", 1);
        let (a, b) = (AgentId::new(), AgentId::new());

        let obs = m
            .reconcile(Some(&[
                cmd(a, AgentDesiredStatus::Running),
                cmd(b, AgentDesiredStatus::Running),
            ]))
            .await
            .expect("권위 있는 목록을 줬으므로 관측도 온다");

        assert_eq!(
            m.running_agents().await.len(),
            1,
            "상한이 1이면 하나만 뜬다"
        );
        // 4c-A에서 거절은 워커 로그 한 줄이 전부였다. 4c-B의 핵심은 그것이
        // 오케스트레이터에 **이유와 함께** 도달한다는 것이므로, 뜬 개수만
        // 세면 이 단계가 실제로 무엇을 더했는지 증명하지 못한다.
        let obs = obs.observations;
        assert_eq!(obs.len(), 2, "desired=running인 둘 다에 대해 말한다");
        assert_eq!(
            obs.iter()
                .filter(|o| o.status() == AgentObservedStatus::Running)
                .count(),
            1
        );
        let failed = obs
            .iter()
            .find(|o| o.status() == AgentObservedStatus::Failed)
            .expect("거절된 하나가 실패로 보고된다");
        assert_eq!(failed.reason(), Some(AgentObservationReason::CapReached));
        m.shutdown_all().await;
    }

    /// 포트 소진은 상한 초과와 **같은 거절 경로**를 탄다 — 결과가 같기 때문이다.
    #[tokio::test]
    async fn port_exhaustion_rejects_the_excess() {
        let dir = tempfile::tempdir().unwrap();
        let m = manager(&dir, fake_grok(dir.path()), "39280-39280", 4);
        let (a, b) = (AgentId::new(), AgentId::new());

        let obs = m
            .reconcile(Some(&[
                cmd(a, AgentDesiredStatus::Running),
                cmd(b, AgentDesiredStatus::Running),
            ]))
            .await
            .expect("권위 있는 목록을 줬으므로 관측도 온다");

        assert_eq!(
            m.running_agents().await.len(),
            1,
            "포트가 하나뿐이면 하나만 뜬다"
        );
        // 거절 **경로**는 하나지만 관측의 이유는 갈린다 — 그 구분이 실제로
        // 살아 있는지 여기서 본다.
        let failed = obs
            .observations
            .iter()
            .find(|o| o.status() == AgentObservedStatus::Failed)
            .expect("거절된 하나가 실패로 보고된다");
        assert_eq!(failed.reason(), Some(AgentObservationReason::NoFreePort));
        m.shutdown_all().await;
    }

    /// 관측의 `None`/`Some([])` 구분은 명령 목록의 그것과 대칭이다.
    ///
    /// 이 단정이 없으면 `reconcile`이 언제나 `Some`을 돌려주도록 바뀌어도
    /// 아무 테스트도 깨지지 않는데, 그 변경은 store 조회가 실패한 beat에
    /// "관측할 것이 하나도 없다"를 보내 살아 있는 Agent들의 관측을 전부
    /// 지우게 만든다.
    #[tokio::test]
    async fn no_authoritative_list_means_nothing_observed() {
        let dir = tempfile::tempdir().unwrap();
        let m = manager(&dir, fake_grok(dir.path()), "39320-39339", 4);

        assert!(
            m.reconcile(None).await.is_none(),
            "말해 줄 것이 없는 beat은 관측도 없다"
        );
        let outcome = m
            .reconcile(Some(&[]))
            .await
            .expect("정말로 없는 것은 빈 목록으로 말한다");
        assert!(
            outcome.observations.is_empty(),
            "관측할 것이 없으면 빈 목록이다"
        );
        assert!(
            outcome.orphans.is_empty(),
            "띄운 적이 없으면 정리할 고아도 없다"
        );
        m.shutdown_all().await;
    }

    /// 죽은 자식은 다음 beat에 재기동된다 — 별도 재시작 경로 없이 3단계의
    /// "없으면 띄운다"가 그 일을 한다.
    #[tokio::test]
    async fn a_dead_child_is_respawned_on_the_next_beat() {
        let dir = tempfile::tempdir().unwrap();
        let m = manager(&dir, dying_grok(dir.path()), "39300-39319", 4);
        let a = AgentId::new();
        let list = [cmd(a, AgentDesiredStatus::Running)];

        let _ = m.reconcile(Some(&list)).await;
        // 자식이 종료할 시간을 준다.
        tokio::time::sleep(std::time::Duration::from_millis(300)).await;

        // 죽은 것을 걷어내고 다시 띄운다 — 목록에 여전히 running이 있으므로.
        let _ = m.reconcile(Some(&list)).await;
        assert_eq!(m.running_agents().await.len(), 1);
        m.shutdown_all().await;
    }

    #[test]
    fn an_inverted_port_range_is_rejected_at_construction() {
        let dir = tempfile::tempdir().unwrap();
        let config = WorkerConfig::for_test()
            .agent_port_range("39400-39300")
            .agent_workspace_root(dir.path().to_string_lossy().into_owned())
            .build();
        // `unwrap_err()`을 쓰지 않는 이유: Ok 타입인 매니저가 `Child`를 들고
        // 있어 `Debug`를 만족시킬 수 없다.
        let err = match AgentProcessManager::new(Arc::new(config)) {
            Ok(_) => panic!("an inverted range must be rejected"),
            Err(e) => e,
        };
        assert!(err.to_string().contains("inverted"), "got: {err}");
    }

    #[test]
    fn a_malformed_port_range_is_rejected_at_construction() {
        let dir = tempfile::tempdir().unwrap();
        let config = WorkerConfig::for_test()
            .agent_port_range("2420")
            .agent_workspace_root(dir.path().to_string_lossy().into_owned())
            .build();
        assert!(AgentProcessManager::new(Arc::new(config)).is_err());
    }

    #[test]
    fn workspace_root_defaults_under_grok_cwd_when_unset() {
        let config = WorkerConfig::for_test().build();
        let m = AgentProcessManager::new(Arc::new(config)).unwrap();
        assert!(
            m.workspace_root.ends_with("fleet-agents"),
            "got: {:?}",
            m.workspace_root
        );
    }
    // ── Agent 디렉터리의 경계와 생명주기 (로드맵 `#69` 1단계) ─────────────

    #[tokio::test]
    async fn spawning_creates_the_agent_directory_under_the_workspace_root() {
        let dir = tempfile::tempdir().unwrap();
        let m = manager(&dir, fake_grok(dir.path()), "39220-39239", 4);
        let a = AgentId::new();

        let _ = m
            .reconcile(Some(&[cmd(a, AgentDesiredStatus::Running)]))
            .await;

        assert!(dir.path().join(a.to_string()).is_dir());
        m.shutdown_all().await;
    }

    /// 이 테스트가 이 변경의 핵심 단정이다. 목록에서 사라진 것은 다른 Worker로
    /// 이동했을 수도, 미배치가 됐을 수도, 회수 확인이 끝났을 수도 있다 —
    /// `list_agent_commands`의 술어가 셋을 하나로 뭉친다. 그중 앞의 둘에서
    /// 지우면 살아 있는 Agent의 작업물이 복구 경로 없이 사라진다.
    #[tokio::test]
    async fn absence_from_the_command_list_stops_the_process_but_keeps_the_directory() {
        let dir = tempfile::tempdir().unwrap();
        let m = manager(&dir, fake_grok(dir.path()), "39240-39259", 4);
        let a = AgentId::new();
        let workspace = dir.path().join(a.to_string());

        let _ = m
            .reconcile(Some(&[cmd(a, AgentDesiredStatus::Running)]))
            .await;
        std::fs::write(workspace.join("work.txt"), b"unpushed").unwrap();

        // 다음 beat: 권위 있는 목록이 왔는데 이 Agent가 없다.
        let _ = m.reconcile(Some(&[])).await;

        assert!(
            m.running_agents().await.is_empty(),
            "부재는 프로세스를 정리하는 근거는 된다"
        );
        assert!(
            workspace.join("work.txt").is_file(),
            "부재는 디렉터리를 지우는 근거가 되지 않는다"
        );
        m.shutdown_all().await;
    }

    #[tokio::test]
    async fn an_explicit_stopped_command_removes_the_agent_directory() {
        let dir = tempfile::tempdir().unwrap();
        let m = manager(&dir, fake_grok(dir.path()), "39260-39279", 4);
        let a = AgentId::new();
        let workspace = dir.path().join(a.to_string());

        let _ = m
            .reconcile(Some(&[cmd(a, AgentDesiredStatus::Running)]))
            .await;
        assert!(workspace.is_dir());

        let _ = m
            .reconcile(Some(&[cmd(a, AgentDesiredStatus::Stopped)]))
            .await;

        assert!(!workspace.exists(), "명시적 회수는 지우는 근거가 된다");
        m.shutdown_all().await;
    }

    /// 회수 명령은 확인이 올 때까지 매 beat 다시 온다. 이미 지운 뒤의 반복이
    /// 오류가 되면 그 beat의 로그가 매번 경고로 더럽혀진다.
    #[tokio::test]
    async fn removing_an_already_removed_directory_is_not_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let m = manager(&dir, fake_grok(dir.path()), "39280-39299", 4);
        let a = AgentId::new();

        assert!(!m.remove_workspace(a).await.unwrap());
        let _ = m
            .reconcile(Some(&[cmd(a, AgentDesiredStatus::Running)]))
            .await;
        assert!(m.remove_workspace(a).await.unwrap());
        assert!(!m.remove_workspace(a).await.unwrap());
        m.shutdown_all().await;
    }

    /// `agent_id`는 UUID라 `..`가 섞일 수 없으므로, 이 경계에서 실제로 막는
    /// 것은 symlink다. 검사가 없으면 `create_dir_all`이 조용히 성공하고 이후의
    /// 모든 쓰기가 링크 대상으로 나간다.
    #[cfg(unix)]
    #[tokio::test]
    async fn an_agent_directory_that_is_a_symlink_out_of_the_root_is_rejected() {
        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let m = manager(&root, fake_grok(root.path()), "39300-39319", 4);
        let a = AgentId::new();

        std::os::unix::fs::symlink(outside.path(), root.path().join(a.to_string())).unwrap();

        let err = m.ensure_workspace(a).await.unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::InvalidData, "got: {err}");

        // 거절이 관측으로 도달하고, 링크 대상은 건드려지지 않는다.
        let observed = m
            .reconcile(Some(&[cmd(a, AgentDesiredStatus::Running)]))
            .await
            .unwrap();
        assert!(matches!(
            observed.observations.as_slice(),
            [AgentObservation::Failed { agent_id, .. }] if *agent_id == a
        ));
        assert_eq!(std::fs::read_dir(outside.path()).unwrap().count(), 0);
        m.shutdown_all().await;
    }

    /// 반대 방향의 단정 — root **자신이** 링크인 것은 정상 구성이다. 한쪽만
    /// 정규화하면 workspace를 다른 볼륨에 둔 배치가 위반으로 보고된다.
    #[cfg(unix)]
    #[tokio::test]
    async fn a_symlinked_workspace_root_is_not_a_violation() {
        let real = tempfile::tempdir().unwrap();
        let holder = tempfile::tempdir().unwrap();
        let link = holder.path().join("agents-link");
        std::os::unix::fs::symlink(real.path(), &link).unwrap();

        let config = WorkerConfig::for_test()
            .grok_bin(fake_grok(holder.path()))
            .bind_addr("127.0.0.1:2419")
            .agent_port_range("39320-39339")
            .agent_workspace_root(link.to_string_lossy().into_owned())
            .max_agent_processes(4)
            .build();
        let m = AgentProcessManager::new(Arc::new(config)).unwrap();
        let a = AgentId::new();

        let ws = m.ensure_workspace(a).await.unwrap();
        assert!(
            ws.starts_with(real.path().canonicalize().unwrap()),
            "got: {ws:?}"
        );
        assert!(real.path().join(a.to_string()).is_dir());
    }
}
