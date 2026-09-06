//! OS 스레드 워치독 — heartbeat 루프가 **돌지 않게 된** 경우의 상한.
//!
//! ## 이 모듈이 메우는 구멍
//!
//! 감독 없는 Agent 실행에는 이미 두 겹의 상한이 있다.
//!
//! 1. **자기 펜싱** (로드맵 `#67` 게이트 ⑥). 제어면과 `agent_fence_after_secs`
//!    이상 끊기면 heartbeat 루프가 스스로
//!    [`fence_all`](crate::agent_process::AgentProcessManager::fence_all)을 부른다.
//! 2. **루프 감시** (`runner::await_shutdown`). heartbeat 루프 **태스크**가
//!    패닉·취소·조기 반환으로 끝나면 Worker가 실패 종료하고, 감독자가 다시
//!    띄운 incarnation의 sweep이 살아남은 자식을 회수한다.
//!
//! 둘 다 **루프가 돌고 있다**는 것을 전제한다. 1은 루프 안의 코드고, 2는
//! `JoinHandle`이 완료되는 것을 신호로 쓴다. 그래서 다음 부류는 어느 쪽에도
//! 걸리지 않는다:
//!
//! * 런타임 굶주림 — 워커 스레드 전부가 블로킹 syscall에 잡혀 있다.
//! * 데드락 — 락을 쥔 태스크가 그 락을 필요로 하는 것을 await한다.
//!
//! 이때 프로세스는 살아 있고 `JoinHandle`은 영원히 완료되지 않는다. 오케스트
//! 레이터는 heartbeat 타임아웃으로 이 Worker를 `Offline`으로 판정하지만, 이
//! Worker의 Agent 프로세스는 **아무도 보지 않는 채로 계속 돈다** — 그리고
//! 그것을 멈출 유일한 코드(자기 펜싱)가 바로 멈춰 선 그 루프 안에 있다.
//!
//! **이 부류는 가설이 아니다.** 2026-09-05에 `detect_grok_version`이
//! `std::process::Command::output()`으로 heartbeat 루프 안에서 300초를
//! 블로킹했다(agent.md §3의 기록과 같은 계열). 그동안 펜싱 기한은 한 번도
//! 평가되지 않았다.
//!
//! ## 왜 OS 스레드인가
//!
//! [`tokio::spawn`]으로 띄우면 감시자가 **감시 대상과 같은 런타임에서**
//! 스케줄된다. 굶은 런타임은 감시자도 굶기므로, 잡아야 할 바로 그 경우에
//! 잡지 못한다. [`std::thread`]는 커널이 스케줄하므로 그 결합이 없다.
//!
//! ## 프로세스 전체의 정지와 런타임 굶주림을 구분한다
//!
//! `SIGSTOP`, 디버거 attach, VM 정지는 워치독 스레드까지 함께 멈춘다. 그때
//! 비콘이 멈춰 있는 것은 **정상**이고, 재개 직후 그것을 굶주림으로 읽으면
//! 워치독이 멀쩡한 Worker를 죽인다.
//!
//! 두 경우는 **워치독 자신의 잠을 재서** 갈린다. 굶주림이면 워치독은 제때
//! 깨어나므로 자기 `sleep`은 정확하고 비콘만 멈춰 있다. 프로세스 전체가
//! 멈췄으면 워치독의 `sleep`도 함께 크게 늘어난다 —
//! [`Verdict::Frozen`]은 그 초과를 보고 기준점을 다시 잡는다.
//!
//! 판정이 애매하면 **죽이지 않는 쪽**으로 접는다. 놓친 굶주림은 오케스트
//! 레이터의 `Offline` 판정이 여전히 덮지만, 잘못 죽인 Worker를 되돌리는
//! 것은 아무도 하지 않는다.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use tracing::{error, info, warn};

/// 워치독이 프로세스를 끝낼 때 쓰는 종료 코드.
///
/// `ExitCode::FAILURE`(1)와 **구분한다.** 1은 "이 Worker가 정상적으로 시작
/// 하거나 계속 돌 수 없었다"는 넓은 실패이고, 이 코드는 "런타임이 멈춰서
/// 바깥에서 끊었다"는 한 가지 사실만 뜻한다. 감독자 로그에서 재기동의 원인을
/// 되짚을 때 이 구분이 유일한 단서다 — 스택도, graceful shutdown 로그도
/// 남지 않는 경로이기 때문이다.
pub const EXIT_WATCHDOG: i32 = 70;

/// heartbeat 루프가 살아 있음을 알리는 카운터.
///
/// 시각이 아니라 **횟수**를 담는다. 시각을 담으려면 [`Instant`]를 원자적으로
/// 저장할 표현이 필요하고, 그 인코딩은 워치독이 자기 잠을 재는 방식과 두
/// 개의 서로 다른 시간 원천을 만든다. 횟수는 인코딩이 필요 없고, 워치독은
/// "직전 관측 이후 값이 변했는가"만 물으면 된다.
#[derive(Debug, Default)]
pub struct ProgressBeacon {
    beats: AtomicU64,
    disarmed: AtomicBool,
}

impl ProgressBeacon {
    /// heartbeat 루프가 한 바퀴의 **시작**에 도달했음을 기록한다.
    pub fn beat(&self) {
        self.beats.fetch_add(1, Ordering::Relaxed);
    }

    /// 지금까지의 beat 수.
    pub fn beats(&self) -> u64 {
        self.beats.load(Ordering::Relaxed)
    }

    /// 워치독을 영구히 해제한다.
    ///
    /// **정상 종료 경로에서 반드시 불러야 한다.** shutdown이 시작되면 루프는
    /// 곧 beat을 멈추는데, 그 정지는 굶주림과 카운터만으로는 구분되지
    /// 않는다. 해제하지 않으면 느린 graceful shutdown이 워치독의 기한을 넘겨
    /// **정상 종료 중인 프로세스를 죽인다**.
    pub fn disarm(&self) {
        self.disarmed.store(true, Ordering::Relaxed);
    }

    /// 해제되었는가.
    pub fn is_disarmed(&self) -> bool {
        self.disarmed.load(Ordering::Relaxed)
    }
}

/// 관측 한 번의 판정.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Verdict {
    /// 비콘이 움직였다 — 루프는 돌고 있다.
    Progressed,
    /// 워치독 자신의 잠이 크게 초과됐다 — 프로세스 전체가 멈춰 있었다.
    Frozen,
    /// 비콘은 멈춰 있지만 아직 기한 안이다.
    Waiting,
    /// 비콘이 기한을 넘겨 멈춰 있다.
    Stalled,
}

/// 워치독이 자기 잠의 초과를 "프로세스 정지"로 읽는 배수.
///
/// 스케줄링 지터는 poll 간격의 몇 퍼센트지 배수가 아니다. 반대로 디버거
/// attach나 `SIGSTOP`은 사람의 시간 척도라 poll을 여러 배 넘긴다. 2배는 그
/// 둘 사이에 크게 빈 자리다.
const FREEZE_OVERRUN_FACTOR: u32 = 2;

fn judge(
    advanced: bool,
    slept: Duration,
    poll: Duration,
    since_baseline: Duration,
    stall_after: Duration,
) -> Verdict {
    if advanced {
        Verdict::Progressed
    } else if slept > poll * FREEZE_OVERRUN_FACTOR {
        Verdict::Frozen
    } else if since_baseline >= stall_after {
        Verdict::Stalled
    } else {
        Verdict::Waiting
    }
}

/// 워치독의 본체. 잠과 행동을 주입받아 결정 로직만 남긴다.
///
/// `sleep`은 **실제로 잔 시간을 돌려준다.** 요청한 시간이 아니라 잰 시간을
/// 쓰는 이유는 [`Verdict::Frozen`] 판정 자체가 그 차이로 성립하기 때문이고,
/// 경과 누적을 별도의 [`Instant`]가 아니라 이 반환값으로 하는 이유는 두
/// 시간 원천이 어긋날 자리를 만들지 않기 위해서다.
fn watch<S, A>(
    beacon: &ProgressBeacon,
    poll: Duration,
    stall_after: Duration,
    mut sleep: S,
    on_stall: A,
) where
    S: FnMut(Duration) -> Duration,
    A: FnOnce(u64, Duration),
{
    let mut baseline = beacon.beats();
    let mut since = Duration::ZERO;

    loop {
        let slept = sleep(poll);
        if beacon.is_disarmed() {
            return;
        }
        let now = beacon.beats();
        since += slept;

        match judge(now != baseline, slept, poll, since, stall_after) {
            Verdict::Progressed | Verdict::Frozen => {
                baseline = now;
                since = Duration::ZERO;
            }
            Verdict::Waiting => {}
            Verdict::Stalled => {
                on_stall(baseline, since);
                return;
            }
        }
    }
}

/// 기한에서 관측 간격을 정한다.
///
/// 기한을 여러 조각으로 나눠야 판정의 해상도가 기한 자체에 묶이지 않는다.
/// 위쪽 상한이 있는 이유는 반대다 — 기한이 아주 길 때 관측이 그만큼 성겨지면
/// [`Verdict::Frozen`]의 창(`poll`의 2배)도 함께 커져 짧은 정지가 굶주림으로
/// 오독될 여지가 생긴다.
fn poll_interval(stall_after: Duration) -> Duration {
    (stall_after / 6).clamp(Duration::from_secs(1), Duration::from_secs(30))
}

/// 워치독 스레드를 띄운다. `stall_after`가 0이면 띄우지 않는다.
///
/// 반환된 핸들을 join할 필요는 없다 — 이 스레드는 해제되거나 프로세스를
/// 끝내는 두 가지로만 끝난다.
pub fn spawn(
    beacon: Arc<ProgressBeacon>,
    workspace_root: PathBuf,
    stall_after: Duration,
) -> Option<std::thread::JoinHandle<()>> {
    if stall_after.is_zero() {
        info!("worker watchdog is disabled");
        return None;
    }
    let poll = poll_interval(stall_after);
    info!(
        stall_after_secs = stall_after.as_secs(),
        poll_secs = poll.as_secs(),
        "starting the worker watchdog on a dedicated OS thread"
    );
    let started = std::thread::Builder::new()
        .name("fleet-watchdog".to_string())
        .spawn(move || {
            watch(
                &beacon,
                poll,
                stall_after,
                |d| {
                    let t = Instant::now();
                    std::thread::sleep(d);
                    t.elapsed()
                },
                |beats, stalled_for| terminate(&workspace_root, beats, stalled_for),
            );
        });
    match started {
        Ok(handle) => Some(handle),
        Err(e) => {
            // 스레드를 못 만드는 것은 자원 고갈이고, 그 상태에서 Worker를
            // 세우면 워치독이 막으려던 것보다 확실한 손해가 난다.
            warn!(error = %e, "could not start the worker watchdog thread — continuing without it");
            None
        }
    }
}

/// 굶주림을 확정했을 때의 행동.
///
/// ## 왜 먼저 죽이고 나가는가
///
/// [`std::process::exit`]는 소멸자를 돌리지 않으므로 `kill_on_drop`이 자식을
/// 거두지 않는다. 그래서 그냥 나가면 Agent 프로세스는 그대로 남고, 그것을
/// 회수하는 것은 **감독자가 이 Worker를 다시 띄워 줄 때뿐이다** — 감독자가
/// 없는 배포에서는 영영 회수되지 않는다. 감독 없는 실행의 상한이 감독자의
/// 존재에 기대면 그것은 상한이 아니다.
///
/// ## 왜 `fence_all`이 아니라 pid인가
///
/// [`fence_all`](crate::agent_process::AgentProcessManager::fence_all)은
/// `async`다. 지금 확정한 사실이 "그 런타임이 진행하지 않는다"이므로, 그것을
/// 부르는 것은 정의상 걸린다. 그래서 근거를 메모리가 아니라
/// **디스크의 spawn 기록**에서 가져와 동기적으로 처리한다 — 그 기록은
/// 재기동 뒤의 sweep이 쓰는 것과 같은 것이고, pid 재사용 방어도 같은 것을
/// 쓴다.
fn terminate(workspace_root: &Path, beats: u64, stalled_for: Duration) -> ! {
    error!(
        beats,
        stalled_secs = stalled_for.as_secs(),
        "the heartbeat loop has made no progress past the watchdog deadline — \
         the runtime is stalled and this worker's agents are running unsupervised"
    );
    let killed = crate::agent_process::terminate_recorded_agents_blocking(workspace_root);
    for k in &killed {
        error!(
            agent_id = %k.agent_id,
            pid = k.pid,
            killed = k.killed,
            "watchdog is terminating an agent process before killing this worker"
        );
    }
    error!(
        agents = killed.len(),
        exit_code = EXIT_WATCHDOG,
        "worker watchdog is terminating this process"
    );
    std::process::exit(EXIT_WATCHDOG)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 대본 한 걸음 — 이번 관측이 "잤다고 보고할" 시간과, 그 관측 직전에
    /// 비콘에 할 일.
    type Step = (Duration, Box<dyn Fn(&ProgressBeacon)>);

    /// 잠을 대신하는 대본. 각 관측 직전에 `before`를 실행하고, 그 관측이
    /// "잤다고 보고할" 시간을 준다.
    fn drive(
        beacon: &ProgressBeacon,
        poll: Duration,
        stall_after: Duration,
        script: Vec<Step>,
    ) -> Option<(u64, Duration)> {
        let mut it = script.into_iter();
        let fired = std::cell::RefCell::new(None);
        watch(
            beacon,
            poll,
            stall_after,
            |requested| match it.next() {
                Some((slept, before)) => {
                    before(beacon);
                    slept
                }
                // 대본이 끝나면 해제해 루프를 끝낸다.
                None => {
                    beacon.disarm();
                    requested
                }
            },
            |beats, since| *fired.borrow_mut() = Some((beats, since)),
        );
        fired.into_inner()
    }

    fn on_time(n: usize, poll: Duration) -> Vec<Step> {
        (0..n).map(|_| (poll, noop())).collect()
    }

    fn noop() -> Box<dyn Fn(&ProgressBeacon)> {
        Box::new(|_| {})
    }

    fn beating() -> Box<dyn Fn(&ProgressBeacon)> {
        Box::new(|b| b.beat())
    }

    #[test]
    fn a_beating_loop_never_trips_the_watchdog() {
        let poll = Duration::from_secs(5);
        let b = ProgressBeacon::default();
        let script = (0..100).map(|_| (poll, beating())).collect();
        assert_eq!(
            drive(&b, poll, Duration::from_secs(30), script),
            None,
            "a loop that keeps beating must never be judged stalled"
        );
    }

    #[test]
    fn a_frozen_beacon_trips_once_the_deadline_passes() {
        let poll = Duration::from_secs(5);
        let b = ProgressBeacon::default();
        b.beat();
        b.beat();
        // 6번째 관측에서 누적 30초가 되어야 발동한다.
        let fired = drive(&b, poll, Duration::from_secs(30), on_time(10, poll))
            .expect("a beacon frozen past the deadline must fire");
        assert_eq!(fired.0, 2, "the report names the beat count it froze at");
        assert_eq!(
            fired.1,
            Duration::from_secs(30),
            "it must fire at the deadline, not before or after"
        );
    }

    #[test]
    fn the_deadline_is_not_reached_one_observation_early() {
        let poll = Duration::from_secs(5);
        let b = ProgressBeacon::default();
        assert_eq!(
            drive(&b, poll, Duration::from_secs(30), on_time(5, poll)),
            None,
            "25s of no progress is inside a 30s deadline"
        );
    }

    #[test]
    fn a_whole_process_freeze_is_not_a_runtime_stall() {
        let poll = Duration::from_secs(5);
        let b = ProgressBeacon::default();
        // 워치독 자신의 잠이 매번 크게 초과된다 = 프로세스 전체가 멈췄다.
        // 비콘은 한 번도 움직이지 않지만 발동해서는 안 된다.
        let script = (0..20).map(|_| (Duration::from_secs(60), noop())).collect();
        assert_eq!(
            drive(&b, poll, Duration::from_secs(30), script),
            None,
            "a frozen process must not be mistaken for a starved runtime"
        );
    }

    #[test]
    fn a_freeze_does_not_hide_a_stall_that_follows_it() {
        let poll = Duration::from_secs(5);
        let b = ProgressBeacon::default();
        let mut script: Vec<Step> = vec![(Duration::from_secs(60), noop())];
        script.extend(on_time(10, poll));
        let fired = drive(&b, poll, Duration::from_secs(30), script)
            .expect("the reset must be to the freeze only, not permanent");
        assert_eq!(
            fired.1,
            Duration::from_secs(30),
            "the deadline is measured from the freeze, not from the start"
        );
    }

    #[test]
    fn disarming_stops_the_watchdog_before_it_can_fire() {
        let poll = Duration::from_secs(5);
        let b = ProgressBeacon::default();
        let mut script: Vec<Step> = on_time(4, poll);
        // 기한을 넘길 그 관측에서 잠에 들기 전에 해제된다.
        script.push((poll, Box::new(|b| b.disarm())));
        script.extend(on_time(10, poll));
        assert_eq!(
            drive(&b, poll, Duration::from_secs(25), script),
            None,
            "a disarmed beacon must not be judged at all"
        );
    }

    #[test]
    fn the_poll_interval_divides_the_deadline_but_stays_bounded() {
        assert_eq!(
            poll_interval(Duration::from_secs(600)),
            Duration::from_secs(30),
            "a long deadline is capped so the freeze window stays small"
        );
        assert_eq!(
            poll_interval(Duration::from_secs(60)),
            Duration::from_secs(10),
            "an ordinary deadline is split six ways"
        );
        assert_eq!(
            poll_interval(Duration::from_secs(3)),
            Duration::from_secs(1),
            "a short deadline never polls faster than once a second"
        );
    }

    #[test]
    fn a_disabled_watchdog_starts_no_thread() {
        assert!(
            spawn(
                Arc::new(ProgressBeacon::default()),
                std::path::PathBuf::from("/nonexistent"),
                Duration::ZERO,
            )
            .is_none(),
            "a zero deadline must not start a watchdog"
        );
    }
}
