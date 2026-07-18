//! Server-Sent Events 스트리밍.
//!
//! Postgres LISTEN/NOTIFY에서 발생하는 이벤트를 브라우저에 스트리밍합니다.
//! Phase 3의 `fleet_store::listen_events`가 제공하는 Stream을 SSE Event로 변환.

use std::convert::Infallible;
use std::time::Duration;

use axum::extract::State;
use axum::response::sse::{Event, KeepAlive, Sse};
use std::sync::Arc;
use tokio_stream::StreamExt;
use tracing::debug;

use crate::app::DashboardState;

/// `/api/events/stream` — SSE 스트리밍.
///
/// 연결이 열리면 LISTEN/NOTIFY 스트림을 구독하고, 새 이벤트가 도착할 때마다
/// SSE Event로 브라우저에 푸시합니다.
pub async fn events_stream(
    State(state): State<Arc<DashboardState>>,
) -> Sse<impl tokio_stream::Stream<Item = Result<Event, Infallible>>> {
    debug!("SSE client connected");

    let pool = state.pool.clone();
    let store = state.store.clone();

    // LISTEN/NOTIFY 스트림 시작.
    let event_stream = async_stream::stream! {
        loop {
            match fleet_store::listen_events(store.as_ref(), &pool).await {
                Ok(stream) => {
                    tokio::pin!(stream);
                    while let Some(events) = stream.next().await {
                        for entry in events {
                            let payload = serde_json::to_string(&entry).unwrap_or_else(|_| "{}".into());
                            yield Ok(Event::default()
                                .event("fleet_event")
                                .data(payload));
                        }
                    }
                    // 스트림 종료 시 재연결 대기 후 재시도.
                    tokio::time::sleep(Duration::from_secs(1)).await;
                }
                Err(e) => {
                    tracing::warn!(error = %e, "SSE listener error, retrying in 1s");
                    tokio::time::sleep(Duration::from_secs(1)).await;
                }
            }
        }
    };

    Sse::new(event_stream).keep_alive(
        KeepAlive::new()
            .interval(Duration::from_secs(15))
            .text("keep-alive"),
    )
}
