//! Control plane 권한 lease 관리자 (로드맵 #63, 1·3단계).
//!
//! 백그라운드 태스크로 이 프로세스가 `control_plane_lease`(`fleet-store`)를
//! 계속 쥐고 있는지 획득·갱신한다. Fleet는 유효한 dispatch lease를 가진
//! Orchestrator가 최대 하나여야 한다
//! (`docs/architecture/control-plane-authority-and-failover.md`).
//!
//! ## 이 모듈이 담당하는 범위
//!
//! 여기서는 lease 획득·갱신·상실·반납이 여러 인스턴스 사이에서 정확히
//! 동작하는 primitive를 세운다. `status()`가 반환하는 상태를 실제 제어
//! 경로가 검사해 "lease 없는 인스턴스는 신규 제어 동작을 수행하지 않는다"
//! (불변식 2)를 강제하는 배선은 2단계에서 [`LeaseObserver`]를 통해
//! `Dispatcher`/`Reconciler`에 들어갔다.
//!
//! ## 상태 전이
//!
//! ```text
//! Stopped --(acquire 성공)--> Active
//! Stopped --(acquire 거절/실패)--> Stopped (poll_interval 뒤 재시도)
//! Active --(renew 실패)--> Fenced --(poll_interval 뒤)--> 다시 acquire 시도
//! Active --(정상 종료 → release)--> Stopped (루프 종료)
//! ```
//!
//! `Fenced`에서 자동으로 재시도하는 것이 표준 leader-election 패턴이다.
//!
//! ## 알려진 한계 — epoch는 아직 쓰기를 거르지 않는다
//!
//! 표준 패턴에서 이 자동 재시도의 안전성은 "재시도를 막는 것"이 아니라
//! epoch 기반으로 오래된 쓰기를 거절하는 것이 담당한다(불변식 4·5). **이
//! 저장소에는 그 거절이 아직 없다.** `LeaseStatus::Active { epoch }`는
//! 이 모듈 밖으로 나가지 않으며([`LeaseObserver::allows_control`]이 bool로
//! 축약한다), dispatch 기록에도 task 상태 쓰기에도 epoch가 남지 않는다.
//!
//! 오늘 fenced 인스턴스를 막는 것은 epoch가 아니라 `allows_control()`의
//! bool 검사다 — 갱신 실패를 관측한 **뒤에** 시도되는 제어 동작은 거절되지만,
//! 관측 직전에 이미 DB로 떠난 쓰기는 막지 못한다. 그 창을 닫으려면 task
//! 상태 쓰기가 epoch 술어를 함께 갖는 CAS여야 하고, Worker 이벤트도 자신이
//! 어느 epoch에서 dispatch됐는지 함께 실어 와야 한다(로드맵 `#67`의
//! fencing token). 근거와 판단은
//! `docs/architecture/control-plane-authority-and-failover.md`의
//! "구현 상태와 유예" 표에 있다.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use tracing::{debug, error, info, warn};

use fleet_store::{ControlLease, Store, StoreError};
use tokio_util::sync::CancellationToken;

/// 이 프로세스가 관측한 현재 lease 상태.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LeaseStatus {
    /// lease를 쥐고 있지 않다 — 아직 시도 전이거나, 거절됐거나, fenced됐다.
    Stopped,
    /// lease를 쥐고 있고 최근 갱신에 성공했다.
    Active { epoch: i64 },
    /// lease를 쥐고 있었으나 갱신에 실패했다 — 다른 인스턴스가 가로챘거나
    /// DB에 도달할 수 없다. `Active`였을 때 한 신규 제어 동작이 있다면
    /// 이제 신뢰할 수 없다.
    Fenced,
}

impl LeaseStatus {
    /// 지금 신규 control-plane 동작(dispatch/cancel/Agent command/breaker
    /// 변경)을 수행해도 되는지. `Fenced`도 `false`다 — 방금 fence됐다는 걸
    /// 알아도, 그 순간까지의 판단은 이미 무효였을 수 있다.
    pub fn is_active(self) -> bool {
        matches!(self, LeaseStatus::Active { .. })
    }

    pub fn epoch(self) -> Option<i64> {
        match self {
            LeaseStatus::Active { epoch } => Some(epoch),
            _ => None,
        }
    }
}

/// [`LeaseManager`] 설정.
#[derive(Debug, Clone)]
pub struct LeaseManagerConfig {
    /// lease가 갱신 없이 유효한 최대 시간. 이 시간이 지나면 다른 인스턴스가
    /// 가로챌 수 있다.
    pub ttl: Duration,
    /// lease를 갱신하는 주기. `ttl`보다 충분히 짧아야 한다 — 그렇지 않으면
    /// 정상적인 네트워크 지연·GC pause만으로도 갱신 전에 만료될 수 있다.
    pub renew_interval: Duration,
    /// lease 획득 실패(거절 또는 fenced) 후 재시도까지 대기하는 주기.
    pub poll_interval: Duration,
    /// 정상 종료([`LeaseManagerHandle::shutdown`])에서 루프가 lease를
    /// 반납하고 끝나기를 기다리는 최대 시간. DB가 응답하지 않을 때 프로세스
    /// 종료가 무한정 매달리지 않도록 상한을 둔다 — 이 시간을 넘기면 반납을
    /// 포기하고 abort하며, lease는 `ttl`로 자연 만료된다.
    pub shutdown_grace: Duration,
}

impl Default for LeaseManagerConfig {
    fn default() -> Self {
        Self {
            ttl: Duration::from_secs(15),
            renew_interval: Duration::from_secs(5),
            poll_interval: Duration::from_secs(3),
            shutdown_grace: Duration::from_secs(5),
        }
    }
}

/// Control plane lease 관리자.
pub struct LeaseManager {
    store: Arc<dyn Store>,
    cluster_id: String,
    instance_id: String,
    config: LeaseManagerConfig,
    status: Arc<Mutex<LeaseStatus>>,
    shutdown: CancellationToken,
}

/// 백그라운드 루프 핸들. 정상 종료는 [`LeaseManagerHandle::shutdown`],
/// 비정상 종료 흉내는 [`LeaseManagerHandle::abort`]다 — 둘의 차이는 lease를
/// 반납하느냐이고, 그 차이가 그대로 대기 인스턴스의 승격 지연으로 나타난다
/// (정상 종료는 즉시, abort는 최대 `ttl`).
pub struct LeaseManagerHandle {
    inner: tokio::task::JoinHandle<()>,
    status: Arc<Mutex<LeaseStatus>>,
    shutdown: CancellationToken,
    shutdown_grace: Duration,
}

impl LeaseManagerHandle {
    /// 현재 관측된 lease 상태.
    pub fn status(&self) -> LeaseStatus {
        *self.status.lock().unwrap()
    }

    /// 읽기 전용 관측 handle을 복제한다. `FleetState`처럼 lease 획득·갱신
    /// 책임 없이 "지금 신규 control-plane 동작을 해도 되는가"만 확인하고
    /// 싶은 소비자에게 넘긴다(로드맵 #63 2단계 — dispatch/cancel 경로).
    pub fn observer(&self) -> LeaseObserver {
        LeaseObserver(self.status.clone())
    }

    /// 정상 종료. 루프에 종료를 알리고, 루프가 **자기가 쥔 epoch로** lease를
    /// 반납한 뒤 끝나기를 기다린다(로드맵 #63 3단계).
    ///
    /// 반납을 이 핸들에서 직접 하지 않는 이유: `release()`는 현재 `Active`
    /// epoch를 읽어 그 값으로 CAS한다. 핸들 쪽에서 호출하면 갱신 루프와 같은
    /// 상태를 두 태스크가 동시에 읽게 되고, 읽은 직후 루프가 재획득해 epoch가
    /// 바뀌면 이미 유효하지 않은 epoch로 반납을 시도하게 된다. 루프 안에서
    /// 반납하면 epoch를 소유한 유일한 주체가 반납하므로 그 경쟁 자체가
    /// 성립하지 않는다.
    ///
    /// `shutdown_grace` 안에 끝나지 않으면(DB 무응답 등) 반납을 포기하고
    /// abort한다 — 종료가 매달리는 것보다 TTL 자연 만료가 낫다.
    ///
    /// timeout 분기에서 **명시적으로** abort하는 이유: `timeout(grace, handle)`이
    /// 만료되면 `JoinHandle`은 drop되는데, tokio에서 `JoinHandle`의 drop은
    /// 취소가 아니라 detach다. 그대로 두면 종료 중인 프로세스의 갱신 루프가
    /// 계속 살아서 lease를 갱신하고, 결국 "반납도 못 했고 TTL 만료도 오지
    /// 않는" 상태가 된다 — 주석이 약속한 fallback 자체가 성립하지 않는다.
    pub async fn shutdown(self) {
        let aborter = self.inner.abort_handle();
        self.shutdown.cancel();
        if tokio::time::timeout(self.shutdown_grace, self.inner)
            .await
            .is_err()
        {
            aborter.abort();
            warn!(
                grace = ?self.shutdown_grace,
                "control plane lease release did not finish in time — giving up \
                 and letting the lease expire by TTL"
            );
        }
    }

    /// 백그라운드 루프를 취소하고 종료 대기. lease는 명시적으로 release하지
    /// 않는다 — abort는 비정상 종료를 흉내내는 데도 쓰이므로, 여기서
    /// release까지 시도하면 "프로세스가 죽었을 때"와 "정상 종료"를 같은
    /// 코드 경로로 뭉개게 된다. 정상 종료는 별도로 `release`를 호출한다.
    pub async fn abort(self) {
        self.inner.abort();
        let _ = self.inner.await;
    }
}

/// [`LeaseManager`]/[`LeaseManagerHandle`]의 상태만 읽는 가벼운 handle.
///
/// dispatch/cancel처럼 "지금 신규 control-plane 동작을 해도 되는가"만 알면
/// 되는 소비자가 lease 획득·갱신 책임(store 접근, cluster_id/instance_id)
/// 전체를 짊어지지 않도록 분리했다. `Clone`이라 `FleetState`에 값으로
/// 담아 자유롭게 공유할 수 있다.
#[derive(Clone)]
pub struct LeaseObserver(Arc<Mutex<LeaseStatus>>);

impl LeaseObserver {
    pub fn status(&self) -> LeaseStatus {
        *self.0.lock().unwrap()
    }

    /// 지금 신규 control-plane 동작(dispatch/cancel/breaker 변경)을
    /// 수행해도 되는지 (로드맵 #63 불변식 2).
    pub fn allows_control(&self) -> bool {
        self.status().is_active()
    }

    /// 임의 상태의 observer를 직접 만든다. 정상 production 경로는 항상
    /// [`LeaseManagerHandle::observer`]를 거친다 — 이 생성자는 실제
    /// `LeaseManager` 없이 "지금 lease가 Active/Fenced/Stopped라면
    /// dispatch/cancel/reconcile이 어떻게 반응하는가"를 검증해야 하는
    /// 상위 크레이트(fleet-scheduler 자신을 포함) 테스트를 위한 것이다.
    pub fn with_status(status: LeaseStatus) -> Self {
        Self(Arc::new(Mutex::new(status)))
    }
}

impl LeaseManager {
    pub fn new(
        store: Arc<dyn Store>,
        cluster_id: impl Into<String>,
        instance_id: impl Into<String>,
        config: LeaseManagerConfig,
    ) -> Self {
        Self {
            store,
            cluster_id: cluster_id.into(),
            instance_id: instance_id.into(),
            config,
            status: Arc::new(Mutex::new(LeaseStatus::Stopped)),
            shutdown: CancellationToken::new(),
        }
    }

    /// 이 프로세스의 instance_id.
    pub fn instance_id(&self) -> &str {
        &self.instance_id
    }

    /// 현재 관측된 lease 상태.
    pub fn status(&self) -> LeaseStatus {
        *self.status.lock().unwrap()
    }

    fn set_status(&self, status: LeaseStatus) {
        *self.status.lock().unwrap() = status;
    }

    /// lease 획득을 한 번 시도한다. 성공하면 상태를 `Active`로 갱신한다.
    /// `scan_once`(HealthChecker) 관례와 동일하게 테스트에서 루프 없이
    /// 직접 호출할 수 있게 공개한다.
    pub async fn try_acquire(&self) -> Result<ControlLease, StoreError> {
        let result = self
            .store
            .acquire_control_lease(&self.cluster_id, &self.instance_id, self.config.ttl)
            .await;
        match &result {
            Ok(lease) => {
                self.set_status(LeaseStatus::Active { epoch: lease.epoch });
                info!(
                    cluster_id = %self.cluster_id,
                    instance_id = %self.instance_id,
                    epoch = lease.epoch,
                    "control plane lease acquired"
                );
            }
            Err(StoreError::Conflict(_)) => {
                self.set_status(LeaseStatus::Stopped);
            }
            Err(e) => {
                self.set_status(LeaseStatus::Stopped);
                warn!(
                    cluster_id = %self.cluster_id,
                    error = %e,
                    "control plane lease acquire failed (store error, not a refusal)"
                );
            }
        }
        result
    }

    /// 현재 lease를 한 번 갱신한다. 실패하면 상태를 `Fenced`로 갱신한다 —
    /// 호출자는 이 반환값을 보고 신규 제어 동작을 멈춰야 한다.
    pub async fn try_renew(&self, epoch: i64) -> Result<ControlLease, StoreError> {
        let result = self
            .store
            .renew_control_lease(&self.cluster_id, &self.instance_id, epoch, self.config.ttl)
            .await;
        match &result {
            Ok(lease) => {
                self.set_status(LeaseStatus::Active { epoch: lease.epoch });
                debug!(
                    cluster_id = %self.cluster_id,
                    epoch = lease.epoch,
                    expires_at = %lease.expires_at,
                    "control plane lease renewed"
                );
            }
            Err(e) => {
                self.set_status(LeaseStatus::Fenced);
                error!(
                    cluster_id = %self.cluster_id,
                    instance_id = %self.instance_id,
                    epoch,
                    error = %e,
                    "control plane lease renewal failed — fencing this instance"
                );
            }
        }
        result
    }

    /// lease를 명시적으로 반납한다(정상 종료). 지금 `Active` epoch와 일치할
    /// 때만 반납하며, 결과와 무관하게 상태를 `Stopped`로 만든다 — 반납이
    /// 거절돼도(이미 다른 instance가 가로챈 경우) 이 인스턴스가 계속
    /// `Active`라고 믿을 이유는 없다.
    pub async fn release(&self) -> Result<bool, StoreError> {
        let epoch = self.status().epoch();
        self.set_status(LeaseStatus::Stopped);
        let Some(epoch) = epoch else {
            return Ok(false);
        };
        self.store
            .release_control_lease(&self.cluster_id, &self.instance_id, epoch)
            .await
    }

    /// 백그라운드 획득·갱신 루프 시작.
    pub fn spawn(self) -> LeaseManagerHandle {
        let status = self.status.clone();
        let shutdown = self.shutdown.clone();
        let shutdown_grace = self.config.shutdown_grace;
        let inner = tokio::spawn(async move {
            self.run().await;
        });
        LeaseManagerHandle {
            inner,
            status,
            shutdown,
            shutdown_grace,
        }
    }

    async fn run(&self) {
        info!(
            cluster_id = %self.cluster_id,
            instance_id = %self.instance_id,
            ttl = ?self.config.ttl,
            "lease manager started"
        );
        // 종료 신호는 **대기 지점에서만** 받는다. `try_acquire`/`try_renew`의
        // DB 호출을 `select!`로 감싸면 취소될 때 future가 drop되는데, 서버
        // 쪽 statement는 이미 커밋됐을 수 있어 "이 프로세스가 존재를 모르는
        // lease"가 남는다. 대기만 취소하면 진행 중인 DB 호출은 언제나 끝까지
        // 관측되므로 그 창이 열리지 않는다.
        while !self.shutdown.is_cancelled() {
            match self.try_acquire().await {
                Ok(lease) => {
                    self.renew_loop(lease.epoch).await;
                }
                Err(_) => {
                    // Conflict(다른 instance가 유효)와 store error 둘 다
                    // 같은 처리 — 잠시 기다렸다가 다시 시도한다.
                }
            }
            if self.shutdown.is_cancelled() {
                break;
            }
            tokio::select! {
                _ = self.shutdown.cancelled() => break,
                _ = tokio::time::sleep(self.config.poll_interval) => {}
            }
        }

        // 정상 종료 경로에서만 여기 도달한다 — `abort()`는 이 지점에 닿기
        // 전에 태스크 자체를 없앤다. 지금 lease를 쥐고 있으면 즉시 반납해
        // 대기 인스턴스가 TTL을 기다리지 않고 승격할 수 있게 한다.
        match self.release().await {
            Ok(true) => info!(
                cluster_id = %self.cluster_id,
                instance_id = %self.instance_id,
                "control plane lease released on graceful shutdown"
            ),
            Ok(false) => {}
            Err(e) => warn!(
                cluster_id = %self.cluster_id,
                instance_id = %self.instance_id,
                error = %e,
                "control plane lease release failed on shutdown — the lease will \
                 expire by TTL instead"
            ),
        }
        info!(instance_id = %self.instance_id, "lease manager stopped");
    }

    /// `Active`가 된 뒤 갱신에 실패하거나 종료 신호를 받을 때까지 반복한다.
    async fn renew_loop(&self, epoch: i64) {
        let mut interval = tokio::time::interval(self.config.renew_interval);
        interval.tick().await; // 첫 tick은 즉시 발생 — acquire 직후라 소비만.
        loop {
            tokio::select! {
                _ = self.shutdown.cancelled() => return,
                _ = interval.tick() => {}
            }
            if self.try_renew(epoch).await.is_err() {
                return;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fleet_store::mem::MemStore;

    fn manager(
        store: Arc<dyn Store>,
        instance_id: &str,
        config: LeaseManagerConfig,
    ) -> LeaseManager {
        LeaseManager::new(store, "test-cluster", instance_id, config)
    }

    /// 백그라운드 루프가 첫 acquire를 끝낼 때까지 기다린다.
    async fn wait_active(handle: &LeaseManagerHandle) {
        let mut waited = Duration::ZERO;
        while !handle.status().is_active() && waited < Duration::from_secs(2) {
            tokio::time::sleep(Duration::from_millis(10)).await;
            waited += Duration::from_millis(10);
        }
        assert!(handle.status().is_active(), "loop must reach Active");
    }

    #[tokio::test]
    async fn starts_stopped() {
        let store = MemStore::new_arc();
        let m = manager(store, "a", LeaseManagerConfig::default());
        assert_eq!(m.status(), LeaseStatus::Stopped);
        assert!(!m.status().is_active());
    }

    #[tokio::test]
    async fn try_acquire_transitions_to_active() {
        let store = MemStore::new_arc();
        let m = manager(store, "a", LeaseManagerConfig::default());

        let lease = m.try_acquire().await.unwrap();
        assert_eq!(lease.epoch, 1);
        assert_eq!(m.status(), LeaseStatus::Active { epoch: 1 });
        assert!(m.status().is_active());
        assert_eq!(m.status().epoch(), Some(1));
    }

    #[tokio::test]
    async fn second_manager_is_refused_and_stays_stopped() {
        let store = MemStore::new_arc();
        let a = manager(store.clone(), "a", LeaseManagerConfig::default());
        let b = manager(store, "b", LeaseManagerConfig::default());

        a.try_acquire().await.unwrap();
        let err = b.try_acquire().await.unwrap_err();
        assert!(matches!(err, StoreError::Conflict(_)));
        assert_eq!(b.status(), LeaseStatus::Stopped);
        // a는 여전히 Active — b의 거절이 a에게 영향을 주지 않는다.
        assert!(a.status().is_active());
    }

    #[tokio::test]
    async fn try_renew_keeps_active_with_same_epoch() {
        let store = MemStore::new_arc();
        let m = manager(store, "a", LeaseManagerConfig::default());
        let lease = m.try_acquire().await.unwrap();

        let renewed = m.try_renew(lease.epoch).await.unwrap();
        assert_eq!(renewed.epoch, lease.epoch);
        assert_eq!(m.status(), LeaseStatus::Active { epoch: lease.epoch });
    }

    #[tokio::test]
    async fn try_renew_with_wrong_epoch_fences() {
        let store = MemStore::new_arc();
        let m = manager(store, "a", LeaseManagerConfig::default());
        m.try_acquire().await.unwrap();

        let err = m.try_renew(999).await.unwrap_err();
        assert!(matches!(err, StoreError::NotFound));
        assert_eq!(
            m.status(),
            LeaseStatus::Fenced,
            "a failed renewal must fence, not silently stay Active"
        );
        assert!(!m.status().is_active());
    }

    #[tokio::test]
    async fn release_lets_another_manager_acquire_and_reports_stopped() {
        let store = MemStore::new_arc();
        let a = manager(store.clone(), "a", LeaseManagerConfig::default());
        let b = manager(store, "b", LeaseManagerConfig::default());

        a.try_acquire().await.unwrap();
        let released = a.release().await.unwrap();
        assert!(released);
        assert_eq!(a.status(), LeaseStatus::Stopped);

        let lease = b.try_acquire().await.unwrap();
        assert_eq!(lease.active_instance_id, "b");
        assert_eq!(lease.epoch, 2);
    }

    #[tokio::test]
    async fn release_without_ever_acquiring_is_a_noop() {
        let store = MemStore::new_arc();
        let m = manager(store, "a", LeaseManagerConfig::default());
        let released = m.release().await.unwrap();
        assert!(!released);
        assert_eq!(m.status(), LeaseStatus::Stopped);
    }

    #[tokio::test]
    async fn spawned_loop_acquires_and_becomes_observable_active() {
        let store = MemStore::new_arc();
        let config = LeaseManagerConfig {
            ttl: Duration::from_millis(200),
            renew_interval: Duration::from_millis(30),
            poll_interval: Duration::from_millis(30),
            ..LeaseManagerConfig::default()
        };
        let m = manager(store, "a", config);
        let handle = m.spawn();

        // 백그라운드 루프의 첫 acquire를 기다린다.
        let mut waited = Duration::ZERO;
        while !handle.status().is_active() && waited < Duration::from_secs(2) {
            tokio::time::sleep(Duration::from_millis(10)).await;
            waited += Duration::from_millis(10);
        }
        assert!(handle.status().is_active(), "loop must reach Active");

        handle.abort().await;
    }

    #[tokio::test]
    async fn spawned_loop_survives_a_missed_renewal_window_by_keeping_lease_before_ttl() {
        // renew_interval이 ttl보다 충분히 짧으면, 정상적인 스케줄링 지연
        // 정도로는 fence되지 않아야 한다.
        let store = MemStore::new_arc();
        let config = LeaseManagerConfig {
            ttl: Duration::from_millis(500),
            renew_interval: Duration::from_millis(50),
            poll_interval: Duration::from_millis(50),
            ..LeaseManagerConfig::default()
        };
        let m = manager(store, "a", config);
        let handle = m.spawn();

        let mut waited = Duration::ZERO;
        while !handle.status().is_active() && waited < Duration::from_secs(2) {
            tokio::time::sleep(Duration::from_millis(10)).await;
            waited += Duration::from_millis(10);
        }
        assert!(handle.status().is_active());

        // 여러 renew 주기가 지나도록 기다린다 — fence되지 않고 계속 Active.
        tokio::time::sleep(Duration::from_millis(400)).await;
        assert!(
            handle.status().is_active(),
            "lease must survive multiple renewal cycles without fencing"
        );

        handle.abort().await;
    }

    #[tokio::test]
    async fn graceful_shutdown_releases_the_lease_so_a_standby_acquires_at_once() {
        // 로드맵 #63 3단계의 핵심 동작: 정상 종료는 TTL을 기다리지 않는다.
        let store = MemStore::new_arc();
        let config = LeaseManagerConfig {
            // ttl을 길게 둬야 "TTL 만료 덕분에 획득됐다"는 대안 설명이
            // 배제된다 — 아래 acquire가 성공한다면 반납 때문일 수밖에 없다.
            ttl: Duration::from_secs(60),
            renew_interval: Duration::from_millis(30),
            poll_interval: Duration::from_millis(30),
            ..LeaseManagerConfig::default()
        };
        let primary = manager(store.clone(), "primary", config.clone());
        let handle = primary.spawn();
        wait_active(&handle).await;

        handle.shutdown().await;

        let standby = manager(store, "standby", config);
        let lease = standby
            .try_acquire()
            .await
            .expect("a released lease must be acquirable before its TTL elapses");
        assert_eq!(lease.active_instance_id, "standby");
        assert!(
            lease.epoch > 1,
            "승격은 반드시 더 큰 epoch를 받아야 한다 (불변식 4의 전제)"
        );
    }

    #[tokio::test]
    async fn abort_keeps_the_lease_held_so_a_standby_must_wait_for_the_ttl() {
        // `shutdown()`과 `abort()`의 차이를 명시적으로 고정한다. abort는
        // 프로세스 급사를 흉내내므로 반납하지 않아야 하고, 그래서 대기
        // 인스턴스는 TTL 만료까지 기다려야 한다. 이 대칭 테스트가 없으면
        // 위 테스트는 "release가 어디선가 불렸다"만 말할 뿐, 두 경로가
        // 실제로 갈라지는지는 말해주지 않는다.
        let store = MemStore::new_arc();
        let config = LeaseManagerConfig {
            ttl: Duration::from_secs(60),
            renew_interval: Duration::from_millis(30),
            poll_interval: Duration::from_millis(30),
            ..LeaseManagerConfig::default()
        };
        let primary = manager(store.clone(), "primary", config.clone());
        let handle = primary.spawn();
        wait_active(&handle).await;

        handle.abort().await;

        let standby = manager(store, "standby", config);
        let err = standby
            .try_acquire()
            .await
            .expect_err("an un-released lease must stay held until its TTL elapses");
        assert!(matches!(err, StoreError::Conflict(_)), "got {err:?}");
    }
}
