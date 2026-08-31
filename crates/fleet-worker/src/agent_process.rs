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
};

use crate::config::WorkerConfig;
use crate::error::WorkerError;
use crate::grok_process::{apply_llm_proxy_envs, host_of, terminate_child};

/// 실행 중인 Agent 프로세스 하나.
struct AgentProc {
    child: Child,
    port: u16,
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
    /// 목록은 **desired가 `running`인 Agent만** 담는다. 정리한 Agent를 담지 않는
    /// 이유는 관측 어휘에 "없음"에 해당하는 값이 없어서이고, 그것으로 충분하다 —
    /// 오케스트레이터는 목록에 없는 것의 관측을 지운다.
    pub async fn reconcile(
        &self,
        commands: Option<&[AgentCommand]>,
    ) -> Option<Vec<AgentObservation>> {
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
        let doomed: Vec<(AgentId, AgentProc)> = to_stop
            .into_iter()
            .filter_map(|id| procs.remove(&id).map(|p| (id, p)))
            .collect();
        terminate_all(doomed).await;

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

        Some(observations)
    }

    /// 모든 Agent 프로세스를 종료한다. Worker 종료 경로에서 호출한다.
    ///
    /// `kill_on_drop`이 있어도 명시적으로 부르는 이유: drop은 SIGKILL이고,
    /// 여기서는 singleton과 같은 5초 유예를 준다.
    pub async fn shutdown_all(&self) {
        let mut procs = self.procs.lock().await;
        let drained: Vec<(AgentId, AgentProc)> = procs.drain().collect();
        terminate_all(drained).await;
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
        let workspace = self.workspace_root.join(agent_id.to_string());
        tokio::fs::create_dir_all(&workspace).await?;

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
        cmd.spawn()
    }
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
        assert_eq!(
            m.reconcile(Some(&[])).await,
            Some(Vec::new()),
            "정말로 없는 것은 빈 목록으로 말한다"
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
}
