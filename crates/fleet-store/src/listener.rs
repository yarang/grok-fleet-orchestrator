//! LISTEN/NOTIFY 기반 실시간 이벤트 스트림.
//!
//! PostgreSQL의 `LISTEN` 채널을 구독하여, 새 이벤트가 `events` 테이블에
//! INSERT될 때(트리거 → `pg_notify`) 즉시 수신합니다. 다중 admin 인스턴스와
//! 웹 대시보드(SSE)가 동일한 이벤트 스트림을 공유합니다.
//!
//! ## 흐름
//!
//! ```text
//! [Admin A] INSERT event → events 테이블
//!                          ↓ trigger
//!                   pg_notify('fleet_events', seq)
//!                          ↓ LISTEN
//! [Admin B PgListener]  [Dashboard SSE]  ← 즉시 수신
//! ```

use std::time::Duration;

use futures::Stream;
use sqlx::postgres::PgListener;
use tracing::warn;

use crate::Store;

/// LISTEN/NOTIFY 구독이 사용하는 Postgres 채널명.
pub const EVENT_CHANNEL: &str = "fleet_events";

/// 폴백 폴링 간격 (LISTEN 연결이 끊겼을 때).
const FALLBACK_POLL_INTERVAL: Duration = Duration::from_secs(5);

/// `fleet_events` 채널에 대한 리스너를 시작하고, 새 이벤트를 스트림으로 반환.
///
/// 이 스트림은 무한히 실행되며, 각 항목은 `list_events(after_seq, ..)`로
/// 페치한 이벤트 목록입니다. LISTEN 알림의 payload는 seq 번호이며,
/// 실제 이벤트 페이로드는 Store에서 페치합니다.
///
/// ## 재연결
///
/// 연결이 끊기면 자동으로 재연결을 시도합니다 (sqlx PgListener 내장 동작).
pub async fn listen_events<'a, S>(
    store: &'a S,
    pool: &sqlx::PgPool,
) -> Result<impl Stream<Item = Vec<fleet_core::EventEntry>> + 'a, crate::StoreError>
where
    S: Store + 'a,
{
    let mut listener = PgListener::connect_with(pool).await?;
    listener.listen(EVENT_CHANNEL).await?;

    let stream = async_stream::stream! {
        let mut last_seq: u64 = 0;

        loop {
            // NOTIFY 대기 (타임아웃으로 폴백 폴링 보장)
            match tokio::time::timeout(FALLBACK_POLL_INTERVAL, listener.recv()).await {
                Ok(Ok(notification)) => {
                    if let Ok(seq) = notification.payload().parse::<u64>() {
                        if seq > last_seq {
                            match store.list_events(last_seq, 100).await {
                                Ok(events) => {
                                    if let Some(last) = events.last() {
                                        last_seq = last.seq;
                                    }
                                    if !events.is_empty() {
                                        yield events;
                                    }
                                }
                                Err(e) => {
                                    warn!("failed to fetch events after notify: {e}");
                                }
                            }
                        }
                    }
                }
                Ok(Err(e)) => {
                    warn!("pg_listener error, will retry: {e}");
                    tokio::time::sleep(Duration::from_secs(1)).await;
                }
                Err(_) => {
                    // 타임아웃 — 폴백으로 한 번 페치 (NOTIFY 누락 대비)
                    if let Ok(events) = store.list_events(last_seq, 100).await {
                        if let Some(last) = events.last() {
                            last_seq = last.seq;
                        }
                        if !events.is_empty() {
                            yield events;
                        }
                    }
                }
            }
        }
    };

    Ok(stream)
}
