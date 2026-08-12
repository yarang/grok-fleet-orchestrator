//! 워커별 CircuitBreaker 레지스트리.
//!
//! grok-build의 `xai-circuit-breaker`와 동일한 3상태 머신 (Closed/Open/HalfOpen)과
//! 슬라이딩 윈도우 알고리즘을 사용하되, 의존성 없이 자체 구현합니다.
//! 향후 grok-build의 것으로 교체 가능하도록 동일한 인터페이스 유지.

use std::collections::{HashMap, VecDeque};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use fleet_core::{CircuitBreakerConfig, CircuitState, WorkerId};

/// CircuitBreaker의 3상태 (도메인 `CircuitState`와 동일).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BreakerState {
    Closed,
    Open,
    HalfOpen,
}

impl From<BreakerState> for CircuitState {
    fn from(s: BreakerState) -> Self {
        match s {
            BreakerState::Closed => CircuitState::Closed,
            BreakerState::Open => CircuitState::Open,
            BreakerState::HalfOpen => CircuitState::HalfOpen,
        }
    }
}

/// 작업 결과 분류.
#[derive(Debug, Clone, Copy)]
pub enum Outcome {
    Success,
    Failure,
}

struct BreakerInner {
    state: BreakerState,
    /// 슬라이딩 윈도우 내 (is_failure, timestamp) 쌍.
    samples: VecDeque<(bool, Instant)>,
    /// Open 상태가 된 시각 (HalfOpen 전이 판단용).
    opened_at: Option<Instant>,
    /// HalfOpen 상태에서 발급된 프로브 슬롯의 발급 시각들.
    /// 길이가 `half_open_max_probes`에 도달하면 추가 `check()`를 거부합니다.
    /// `record()`가 호출되면 슬롯을 해제합니다 — 단, 프로브 호출자가 크래시하거나
    /// `record()`를 영영 호출하지 않는 "lost probe" 상황을 대비해, `open_duration_secs`
    /// 이상 해제되지 않은 슬롯은 `check()`가 스스로 회수합니다.
    half_open_probes: VecDeque<Instant>,
}

impl BreakerInner {
    fn new() -> Self {
        Self {
            state: BreakerState::Closed,
            samples: VecDeque::new(),
            opened_at: None,
            half_open_probes: VecDeque::new(),
        }
    }
}

/// 단일 워커에 대한 CircuitBreaker.
pub struct CircuitBreaker {
    config: CircuitBreakerConfig,
    inner: Mutex<BreakerInner>,
}

impl CircuitBreaker {
    pub fn new(config: CircuitBreakerConfig) -> Self {
        Self::new_with_state(config, CircuitState::Closed)
    }

    pub fn new_with_state(config: CircuitBreakerConfig, state: CircuitState) -> Self {
        let mut inner = BreakerInner::new();
        inner.state = match state {
            CircuitState::Closed => BreakerState::Closed,
            CircuitState::Open => BreakerState::Open,
            CircuitState::HalfOpen => BreakerState::HalfOpen,
        };
        if state == CircuitState::Open {
            // opened_at을 초기화하여 쿨다운 경과시간 계산이 유효하도록 함
            inner.opened_at = Some(std::time::Instant::now());
        }
        Self {
            config,
            inner: Mutex::new(inner),
        }
    }

    /// 현재 상태 조회.
    pub fn state(&self) -> BreakerState {
        self.inner.lock().unwrap().state
    }

    /// 회로가 열려 있는지 (빠른 경로).
    pub fn is_open(&self) -> bool {
        matches!(self.state(), BreakerState::Open)
    }

    /// 작업 허용 여부 확인. Open이면 에러 반환.
    ///
    /// `Open` 상태에서 `open_duration` 경과 시 `HalfOpen`으로 자동 전이되어
    /// `half_open_max_probes`개까지 동시 프로브를 허용합니다. 그 이상은 거부됩니다.
    pub fn check(&self) -> Result<(), BreakerOpen> {
        let mut inner = self.inner.lock().unwrap();
        let now = Instant::now();

        match inner.state {
            BreakerState::Closed => Ok(()),
            BreakerState::Open => {
                // 쿨다운 경과 확인
                let elapsed = inner
                    .opened_at
                    .map(|t| t.elapsed())
                    .unwrap_or(Duration::ZERO);

                if elapsed >= Duration::from_secs(self.config.open_duration_secs) {
                    // HalfOpen으로 전이 — 이 check() 호출 자체가 첫 프로브가 됩니다.
                    inner.state = BreakerState::HalfOpen;
                    inner.half_open_probes.clear();
                    inner.half_open_probes.push_back(now);
                    tracing::info!(
                        target: "fleet::breaker",
                        "circuit half-open after {:?} cool-down",
                        elapsed
                    );
                    Ok(())
                } else {
                    Err(BreakerOpen {
                        remaining: Duration::from_secs(self.config.open_duration_secs) - elapsed,
                    })
                }
            }
            BreakerState::HalfOpen => {
                // Lost-probe 회수: record()가 끝내 호출되지 않은 채 open_duration_secs
                // 이상 지난 슬롯은 죽은 프로브로 간주해 회수합니다(호출자 크래시 등 대비).
                let stale_after = Duration::from_secs(self.config.open_duration_secs.max(1));
                inner
                    .half_open_probes
                    .retain(|&issued_at| now.duration_since(issued_at) < stale_after);

                let max_probes = self.config.half_open_max_probes.max(1) as usize;
                if inner.half_open_probes.len() < max_probes {
                    inner.half_open_probes.push_back(now);
                    Ok(())
                } else {
                    Err(BreakerOpen {
                        remaining: Duration::ZERO,
                    })
                }
            }
        }
    }

    /// 작업 결과 기록. 상태 전이 수행.
    pub fn record(&self, outcome: Outcome) -> BreakerState {
        let mut inner = self.inner.lock().unwrap();
        let now = Instant::now();
        let window = Duration::from_secs(self.config.window_duration_secs);

        let is_failure = matches!(outcome, Outcome::Failure);

        match inner.state {
            BreakerState::Closed => {
                // 윈도우에서 샘플 추가 + 오래된 것 제거
                inner.samples.push_back((is_failure, now));
                while let Some(&(_, t)) = inner.samples.front() {
                    if now.duration_since(t) > window {
                        inner.samples.pop_front();
                    } else {
                        break;
                    }
                }

                // trip 조건 확인
                let total = inner.samples.len() as u32;
                if total >= self.config.min_samples {
                    let failures = inner.samples.iter().filter(|(f, _)| *f).count() as f64;
                    let error_rate = failures / total as f64;
                    if error_rate >= self.config.error_rate_threshold {
                        inner.state = BreakerState::Open;
                        inner.opened_at = Some(now);
                        inner.half_open_probes.clear();
                        tracing::warn!(
                            target: "fleet::breaker",
                            "circuit opened: {} failures / {} samples (rate {:.2})",
                            failures,
                            total,
                            error_rate
                        );
                    }
                }
            }
            BreakerState::HalfOpen => {
                // 이 프로브가 쓴 슬롯 하나를 해제합니다. 어떤 슬롯이 정확히 이
                // 결과에 대응하는지는 추적하지 않지만(1:1 매칭 불필요), 슬롯 "개수"만
                // 정확하면 동시 프로브 상한을 강제하는 목적은 충분히 달성됩니다.
                inner.half_open_probes.pop_front();

                if is_failure {
                    inner.state = BreakerState::Open;
                    inner.opened_at = Some(now);
                    inner.half_open_probes.clear();
                    tracing::warn!(target: "fleet::breaker", "half-open probe failed, reopening");
                } else {
                    inner.state = BreakerState::Closed;
                    inner.samples.clear();
                    inner.opened_at = None;
                    inner.half_open_probes.clear();
                    tracing::info!(target: "fleet::breaker", "half-open probe succeeded, closing");
                }
            }
            BreakerState::Open => {
                // Open 상태에서의 결과는 무시 (이미 차단됨)
            }
        }

        inner.state
    }

    /// 수동 리셋 (admin/대시보드용).
    pub fn reset(&self) {
        let mut inner = self.inner.lock().unwrap();
        inner.state = BreakerState::Closed;
        inner.samples.clear();
        inner.opened_at = None;
        inner.half_open_probes.clear();
    }

    /// 강제 Open (다중 admin 동기화용). 다른 admin이 Open시킨 상태를 반영.
    pub fn force_open(&self) {
        let mut inner = self.inner.lock().unwrap();
        inner.state = BreakerState::Open;
        inner.opened_at = Some(Instant::now());
        inner.half_open_probes.clear();
    }
}

/// 회로가 요청을 거부할 때 반환되는 에러 — `Open` 쿨다운 대기 중이거나,
/// `HalfOpen`에서 동시 프로브 상한(`half_open_max_probes`)에 도달한 경우입니다.
/// 후자는 `remaining`이 `Duration::ZERO`로 설정됩니다(고정된 쿨다운이 아니라
/// "지금 당장은 프로브 슬롯이 없다"는 의미).
#[derive(Debug, Clone, thiserror::Error)]
#[error("circuit breaker is not accepting requests (remaining: {:?})", remaining)]
pub struct BreakerOpen {
    pub remaining: Duration,
}

/// 워커 ID를 키로 하는 CircuitBreaker 레지스트리.
///
/// grok-build의 `CircuitBreakerRegistry`와 동일한 인터페이스.
/// `get(worker_id)`로 워커별 브레이커를 조회/지연 생성합니다.
pub struct BreakerRegistry {
    config: CircuitBreakerConfig,
    breakers: Mutex<HashMap<WorkerId, std::sync::Arc<CircuitBreaker>>>,
}

impl BreakerRegistry {
    pub fn new(config: CircuitBreakerConfig) -> Self {
        Self {
            config,
            breakers: Mutex::new(HashMap::new()),
        }
    }

    /// 워커의 브레이커를 조회. 없으면 생성.
    pub fn get(
        &self,
        worker_id: WorkerId,
        initial_state: CircuitState,
    ) -> std::sync::Arc<CircuitBreaker> {
        let mut breakers = self.breakers.lock().unwrap();
        breakers
            .entry(worker_id)
            .or_insert_with(|| {
                std::sync::Arc::new(CircuitBreaker::new_with_state(
                    self.config.clone(),
                    initial_state,
                ))
            })
            .clone()
    }

    /// 워커의 브레이커 상태 조회 (없으면 Closed).
    pub fn state_of(&self, worker_id: WorkerId) -> BreakerState {
        let breakers = self.breakers.lock().unwrap();
        breakers
            .get(&worker_id)
            .map(|cb| cb.state())
            .unwrap_or(BreakerState::Closed)
    }

    /// 특정 워커의 브레이커를 리셋 (워커 등록 해제 시).
    /// 존재하지 않아도 에러 아님.
    pub fn reset(&self, worker_id: WorkerId) {
        let breakers = self.breakers.lock().unwrap();
        if let Some(cb) = breakers.get(&worker_id) {
            cb.reset();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strict_config() -> CircuitBreakerConfig {
        CircuitBreakerConfig {
            enabled: true,
            window_duration_secs: 60,
            min_samples: 3,
            error_rate_threshold: 0.5,
            open_duration_secs: 1, // 짧게 — 테스트용
            half_open_max_probes: 1,
            failure_codes: vec![],
        }
    }

    #[test]
    fn breaker_opens_after_threshold() {
        let cb = CircuitBreaker::new(strict_config());

        // 3번 실패 (min_samples=3, error_rate=1.0 >= 0.5)
        cb.record(Outcome::Failure);
        cb.record(Outcome::Failure);
        assert_eq!(cb.state(), BreakerState::Closed); // 아직 샘플 부족

        cb.record(Outcome::Failure);
        assert_eq!(cb.state(), BreakerState::Open);

        // check는 에러 반환
        assert!(cb.check().is_err());
    }

    #[test]
    fn breaker_half_open_after_cooldown() {
        let cb = CircuitBreaker::new(strict_config());
        for _ in 0..3 {
            cb.record(Outcome::Failure);
        }
        assert_eq!(cb.state(), BreakerState::Open);

        // 쿨다운 대기
        std::thread::sleep(Duration::from_millis(1100));

        // check가 HalfOpen 전이 후 Ok 반환
        assert!(cb.check().is_ok());
        assert_eq!(cb.state(), BreakerState::HalfOpen);

        // 프로브 성공 → Closed
        cb.record(Outcome::Success);
        assert_eq!(cb.state(), BreakerState::Closed);
    }

    #[test]
    fn breaker_success_keeps_closed() {
        let cb = CircuitBreaker::new(strict_config());
        for _ in 0..10 {
            cb.record(Outcome::Success);
        }
        assert_eq!(cb.state(), BreakerState::Closed);
    }

    #[test]
    fn half_open_limits_concurrent_probes_to_configured_max() {
        let cb = CircuitBreaker::new(strict_config()); // half_open_max_probes = 1
        for _ in 0..3 {
            cb.record(Outcome::Failure);
        }
        assert_eq!(cb.state(), BreakerState::Open);
        std::thread::sleep(Duration::from_millis(1100)); // 쿨다운(1s) 경과

        // 첫 프로브는 허용되고 HalfOpen으로 전이.
        assert!(cb.check().is_ok());
        assert_eq!(cb.state(), BreakerState::HalfOpen);

        // 첫 프로브가 아직 record()로 해소되지 않은 상태에서 두 번째 check()는
        // half_open_max_probes=1을 초과하므로 거부되어야 한다 (수정 전에는 항상 Ok).
        assert!(cb.check().is_err());

        // 첫 프로브를 성공으로 기록하면 슬롯이 해제되고 Closed로 전이.
        cb.record(Outcome::Success);
        assert_eq!(cb.state(), BreakerState::Closed);

        // Closed 상태에서는 슬롯 제한과 무관하게 항상 허용.
        assert!(cb.check().is_ok());
    }

    #[test]
    fn half_open_allows_configured_max_probes_concurrently() {
        let mut config = strict_config();
        config.half_open_max_probes = 2;
        let cb = CircuitBreaker::new(config);
        for _ in 0..3 {
            cb.record(Outcome::Failure);
        }
        std::thread::sleep(Duration::from_millis(1100));

        assert!(cb.check().is_ok()); // 프로브 1
        assert!(cb.check().is_ok()); // 프로브 2 (max=2라 허용)
        assert!(cb.check().is_err()); // 프로브 3 — 상한 초과, 거부
    }

    #[test]
    fn half_open_probe_failure_reopens_and_clears_slots() {
        let cb = CircuitBreaker::new(strict_config());
        for _ in 0..3 {
            cb.record(Outcome::Failure);
        }
        std::thread::sleep(Duration::from_millis(1100));

        assert!(cb.check().is_ok());
        assert_eq!(cb.state(), BreakerState::HalfOpen);

        cb.record(Outcome::Failure); // 프로브 실패 → 다시 Open
        assert_eq!(cb.state(), BreakerState::Open);
        assert!(cb.check().is_err()); // 새 쿨다운 시작, 즉시 재개방 안 됨
    }

    #[test]
    fn half_open_lost_probe_is_reclaimed_after_cooldown() {
        let cb = CircuitBreaker::new(strict_config()); // open_duration_secs = 1
        for _ in 0..3 {
            cb.record(Outcome::Failure);
        }
        std::thread::sleep(Duration::from_millis(1100));

        // 프로브 발급 후 record()를 영영 호출하지 않는 "lost probe" 시나리오.
        assert!(cb.check().is_ok());
        assert_eq!(cb.state(), BreakerState::HalfOpen);
        assert!(cb.check().is_err()); // 아직 stale 판정 전이라 거부됨

        // open_duration_secs(1s) 이상 경과하면 죽은 프로브로 간주해 회수되어야 한다.
        std::thread::sleep(Duration::from_millis(1100));
        assert!(cb.check().is_ok()); // 회수된 슬롯 자리에 새 프로브 발급 성공
    }

    #[test]
    fn registry_isolates_workers() {
        let reg = BreakerRegistry::new(strict_config());
        let w1 = WorkerId::new();
        let w2 = WorkerId::new();

        let cb1 = reg.get(w1, CircuitState::Closed);
        for _ in 0..3 {
            cb1.record(Outcome::Failure);
        }

        assert_eq!(reg.state_of(w1), BreakerState::Open);
        assert_eq!(reg.state_of(w2), BreakerState::Closed); // w2는 영향 없음
    }
}
