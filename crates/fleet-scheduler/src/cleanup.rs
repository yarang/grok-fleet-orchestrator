//! 만료 데이터 정리 백그라운드 루프 (로드맵 P1 #18).
//!
//! `Store::delete_expired_sessions` / `Store::delete_old_login_attempts`는
//! 이미 구현되어 있었지만, 실제로 이를 주기적으로 호출하는 코드가 없어서
//! `sessions` 테이블은 만료된 뒤에도 영구히 쌓이기만 했다 (`login_attempts`는
//! `fleet-dashboard`의 로그인 성공 경로에서 기회적으로만 정리됨 — 아무도
//! 로그인하지 않으면 그마저도 동작하지 않는다).
//!
//! [`HealthChecker`](crate::health::HealthChecker)와 동일한 "설정 + spawn +
//! JoinHandle 기반 abort" 패턴을 그대로 따른다.

use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use tokio::task::JoinHandle;
use tracing::{info, warn};

use fleet_store::Store;

/// 정리 루프 설정.
#[derive(Debug, Clone)]
pub struct CleanupConfig {
    /// 정리 사이클 폴링 주기.
    pub interval: Duration,
    /// 이보다 오래된 로그인 시도 기록(`login_attempts`)을 삭제.
    /// `fleet-dashboard`의 기회적 정리(로그인 성공 시)와 동일 기준(7일)을
    /// 기본값으로 사용해 두 경로가 서로 다른 보존 정책을 갖지 않도록 한다.
    pub login_attempt_retention: chrono::Duration,
}

impl Default for CleanupConfig {
    fn default() -> Self {
        Self {
            interval: Duration::from_secs(3600), // 1시간마다
            login_attempt_retention: chrono::Duration::days(7),
        }
    }
}

/// 단일 정리 사이클 결과. 로깅/테스트에서 사용.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CleanupSummary {
    /// 삭제된 만료 세션 수.
    pub expired_sessions: u64,
    /// 삭제된 오래된 로그인 시도 기록 수.
    pub old_login_attempts: u64,
}

/// 정리 루프. spawn하면 백그라운드 태스크를 반환.
pub struct SessionCleanup {
    store: Arc<dyn Store>,
    config: CleanupConfig,
}

/// 백그라운드 정리 루프 핸들. `abort()`로 종료.
pub struct SessionCleanupHandle {
    inner: JoinHandle<()>,
}

impl SessionCleanupHandle {
    /// 백그라운드 루프를 취소하고 종료 대기.
    pub async fn abort(self) {
        self.inner.abort();
        let _ = self.inner.await;
    }
}

impl SessionCleanup {
    pub fn new(store: Arc<dyn Store>, config: CleanupConfig) -> Self {
        Self { store, config }
    }

    /// 백그라운드 루프 시작. 첫 정리는 기동 직후 즉시 수행 — 재시작 사이
    /// 쌓였을 수 있는 만료 데이터를 바로 청소한다 (HealthChecker와 달리
    /// 첫 틱을 기다리지 않음. 세션 정리는 지연돼도 위험하지 않지만, 빠르게
    /// 한 번 청소해두는 편이 재시작 직후 테이블 크기를 더 정확히 관측하기
    /// 좋다).
    pub fn spawn(self) -> SessionCleanupHandle {
        let handle = tokio::spawn(async move {
            self.run().await;
        });
        SessionCleanupHandle { inner: handle }
    }

    async fn run(&self) {
        info!(
            interval = ?self.config.interval,
            retention_days = self.config.login_attempt_retention.num_days(),
            "session/login-attempt cleanup task started"
        );

        let mut interval = tokio::time::interval(self.config.interval);
        loop {
            self.sweep_once().await;
            interval.tick().await;
        }
    }

    /// 단일 정리 사이클. 테스트에서 직접 호출 가능.
    ///
    /// 두 정리 작업은 서로 독립적 — 한쪽이 실패해도 다른 쪽은 계속 시도한다
    /// (예: `login_attempts` 삭제가 실패해도 만료 세션은 정리되어야 함).
    pub async fn sweep_once(&self) -> CleanupSummary {
        let expired_sessions = match self.store.delete_expired_sessions().await {
            Ok(n) => n,
            Err(e) => {
                warn!(error = %e, "delete_expired_sessions failed");
                0
            }
        };

        let cutoff = Utc::now() - self.config.login_attempt_retention;
        let old_login_attempts = match self.store.delete_old_login_attempts(cutoff).await {
            Ok(n) => n,
            Err(e) => {
                warn!(error = %e, "delete_old_login_attempts failed");
                0
            }
        };

        if expired_sessions > 0 || old_login_attempts > 0 {
            info!(
                expired_sessions,
                old_login_attempts, "cleanup sweep removed expired data"
            );
        }

        CleanupSummary {
            expired_sessions,
            old_login_attempts,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fleet_core::{LoginAttempt, Session, SessionId, UserId};
    use fleet_store::mem::MemStore;
    use uuid::Uuid;

    /// 만료된 세션 N개를 실제로 저장소에 삽입 (`create_session` 경유).
    async fn seed_expired_sessions(store: &MemStore, n: usize) {
        for _ in 0..n {
            let token = Uuid::new_v4().to_string();
            store
                .create_session(&Session {
                    id: SessionId::new(),
                    user_id: UserId::new(),
                    token_hash: token,
                    created_at: Utc::now() - chrono::Duration::days(1),
                    // 이미 만료됨 — delete_expired_sessions의 대상.
                    expires_at: Utc::now() - chrono::Duration::minutes(1),
                    ip_address: None,
                    user_agent: None,
                })
                .await
                .unwrap();
        }
    }

    /// `retention`보다 오래된 로그인 시도 기록 N개를 실제로 삽입.
    async fn seed_old_login_attempts(store: &MemStore, n: usize, retention: chrono::Duration) {
        for _ in 0..n {
            store
                .record_login_attempt(&LoginAttempt {
                    id: Uuid::new_v4(),
                    identifier: "old-attempt".to_string(),
                    ip_address: None,
                    success: false,
                    failure_reason: Some("bad_password".to_string()),
                    attempted_at: Utc::now() - retention - chrono::Duration::days(1),
                })
                .await
                .unwrap();
        }
    }

    #[tokio::test]
    async fn sweep_once_reports_deleted_counts() {
        let store = MemStore::new();
        seed_expired_sessions(&store, 3).await;
        seed_old_login_attempts(&store, 5, chrono::Duration::days(7)).await;
        let cleanup = SessionCleanup::new(Arc::new(store), CleanupConfig::default());

        let summary = cleanup.sweep_once().await;
        assert_eq!(summary.expired_sessions, 3);
        assert_eq!(summary.old_login_attempts, 5);
    }

    #[tokio::test]
    async fn sweep_once_passes_correct_retention_cutoff() {
        let store = Arc::new(MemStore::new());
        let config = CleanupConfig {
            interval: Duration::from_secs(1),
            login_attempt_retention: chrono::Duration::days(7),
        };
        let cleanup = SessionCleanup::new(store.clone(), config);

        let before = Utc::now() - chrono::Duration::days(7);
        cleanup.sweep_once().await;
        let after = Utc::now() - chrono::Duration::days(7);

        let recorded = store
            .last_delete_old_login_attempts_cutoff()
            .expect("cutoff recorded");
        assert!(
            recorded >= before && recorded <= after,
            "cutoff should be ~7 days before now (recorded={recorded}, expected between {before} and {after})"
        );
    }

    #[tokio::test]
    async fn sweep_once_is_zero_when_nothing_to_delete() {
        let store = Arc::new(MemStore::new());
        let cleanup = SessionCleanup::new(store, CleanupConfig::default());

        let summary = cleanup.sweep_once().await;
        assert_eq!(summary, CleanupSummary::default());
    }

    #[tokio::test]
    async fn sweep_once_is_resilient_to_store_errors() {
        // 하나(또는 둘 다) 실패해도 panic하지 않고 0으로 보고해야 한다 — 백그라운드
        // 루프가 죽지 않고 다음 사이클에 재시도할 수 있어야 하므로.
        let store =
            MemStore::new().with_failing(&["delete_expired_sessions", "delete_old_login_attempts"]);
        // 실패 주입이 정말 걸러지는지(사이드이펙트가 아니라) 확인하기 위해
        // 지울 데이터도 함께 넣어둔다 — 그래도 0이 나와야 한다.
        seed_expired_sessions(&store, 2).await;
        seed_old_login_attempts(&store, 2, chrono::Duration::days(7)).await;
        let cleanup = SessionCleanup::new(Arc::new(store), CleanupConfig::default());

        let summary = cleanup.sweep_once().await;
        assert_eq!(summary, CleanupSummary::default());
    }

    #[tokio::test]
    async fn spawn_runs_sweep_immediately_without_waiting_for_first_tick() {
        // HealthChecker와 달리 첫 사이클을 interval 대기 없이 즉시 실행한다 —
        // 재시작 직후 쌓여있던 만료 데이터를 바로 청소하기 위함.
        let store = Arc::new(MemStore::new());
        seed_expired_sessions(&store, 1).await;
        seed_old_login_attempts(&store, 1, chrono::Duration::days(7)).await;
        let config = CleanupConfig {
            // interval을 아주 길게 잡아서, "즉시 실행"이 아니라면 이 테스트
            // 타임아웃 내에 세션이 절대 지워지지 않는다.
            interval: Duration::from_secs(3600),
            login_attempt_retention: chrono::Duration::days(7),
        };
        let cleanup = SessionCleanup::new(store.clone(), config);
        let handle = cleanup.spawn();

        // 즉시 실행되는지 폴링 (최대 1초) — CI 환경 스케줄링 지연 감안.
        let mut swept = false;
        for _ in 0..50 {
            if store.last_delete_old_login_attempts_cutoff().is_some() {
                swept = true;
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        handle.abort().await;

        assert!(swept, "first sweep should run immediately on spawn");
    }
}
