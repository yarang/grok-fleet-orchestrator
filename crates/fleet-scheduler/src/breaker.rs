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
}

impl BreakerInner {
    fn new() -> Self {
        Self {
            state: BreakerState::Closed,
            samples: VecDeque::new(),
            opened_at: None,
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
        Self {
            config,
            inner: Mutex::new(BreakerInner::new()),
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
    /// 1회 프로브를 허용합니다.
    pub fn check(&self) -> Result<(), BreakerOpen> {
        let mut inner = self.inner.lock().unwrap();

        match inner.state {
            BreakerState::Closed => Ok(()),
            BreakerState::Open => {
                // 쿨다운 경과 확인
                let elapsed = inner
                    .opened_at
                    .map(|t| t.elapsed())
                    .unwrap_or(Duration::ZERO);

                if elapsed >= Duration::from_secs(self.config.open_duration_secs) {
                    // HalfOpen으로 전이 — 프로브 1회 허용
                    inner.state = BreakerState::HalfOpen;
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
                // 프로브는 이미 check()에서 소비됨 — 다음 check는 Closed 또는 Open
                // 단순화: HalfOpen에서는 항상 허용 (record에서 판단)
                Ok(())
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
                if is_failure {
                    inner.state = BreakerState::Open;
                    inner.opened_at = Some(now);
                    tracing::warn!(target: "fleet::breaker", "half-open probe failed, reopening");
                } else {
                    inner.state = BreakerState::Closed;
                    inner.samples.clear();
                    inner.opened_at = None;
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
    }
}

/// 회로가 열려 있을 때 반환되는 에러.
#[derive(Debug, Clone, thiserror::Error)]
#[error("circuit breaker is open (remaining cool-down: {:?})", remaining)]
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
    pub fn get(&self, worker_id: WorkerId) -> std::sync::Arc<CircuitBreaker> {
        let mut breakers = self.breakers.lock().unwrap();
        breakers
            .entry(worker_id)
            .or_insert_with(|| std::sync::Arc::new(CircuitBreaker::new(self.config.clone())))
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
    fn registry_isolates_workers() {
        let reg = BreakerRegistry::new(strict_config());
        let w1 = WorkerId::new();
        let w2 = WorkerId::new();

        let cb1 = reg.get(w1);
        for _ in 0..3 {
            cb1.record(Outcome::Failure);
        }

        assert_eq!(reg.state_of(w1), BreakerState::Open);
        assert_eq!(reg.state_of(w2), BreakerState::Closed); // w2는 영향 없음
    }
}
