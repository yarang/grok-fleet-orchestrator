//! Server-Sent Events 스트리밍.
//!
//! Postgres LISTEN/NOTIFY에서 발생하는 이벤트를 브라우저에 스트리밍합니다.
//! Phase 3의 `fleet_store::listen_events`가 제공하는 Stream을 SSE Event로 변환.

use std::convert::Infallible;
use std::time::Duration;

use axum::extract::{Extension, State};
use axum::response::sse::{Event, KeepAlive, Sse};
use fleet_core::PermissionKind;
use std::sync::Arc;
use tokio_stream::StreamExt;
use tracing::debug;

use crate::app::DashboardState;
use crate::auth::{require_permission, AuthPrincipal};
use crate::error::ApiError;
use crate::event_view::{filter_event, may_see_task_output};

/// `/api/events/stream` — SSE 스트리밍.
///
/// 연결이 열리면 LISTEN/NOTIFY 스트림을 구독하고, 새 이벤트가 도착할 때마다
/// SSE Event로 브라우저에 푸시합니다.
///
/// **인가**: 폴링 방식인 `GET /api/events`와 동일하게 `events:list` 권한을
/// 요구한다. 같은 데이터를 두 경로로 노출하면서 한쪽에만 게이트를 두면
/// 게이트가 없는 것과 같다.
pub async fn events_stream(
    State(state): State<Arc<DashboardState>>,
    Extension(principal): Extension<AuthPrincipal>,
) -> Result<Sse<impl tokio_stream::Stream<Item = Result<Event, Infallible>>>, ApiError> {
    require_permission(&principal, PermissionKind::EventsList)?;

    // 출력 열람 권한은 연결 시점에 한 번만 평가한다.
    let may_see_output = may_see_task_output(&principal);
    debug!(
        user = %principal.user.username,
        may_see_output,
        "SSE client connected"
    );

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
                            let entry = filter_event(entry, may_see_output);
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

    Ok(Sse::new(event_stream).keep_alive(
        KeepAlive::new()
            .interval(Duration::from_secs(15))
            .text("keep-alive"),
    ))
}
