//! Server-Sent Events 스트리밍.
//!
//! Postgres LISTEN/NOTIFY에서 발생하는 이벤트를 브라우저에 스트리밍합니다.
//! Phase 3의 `fleet_store::listen_events`가 제공하는 Stream을 SSE Event로 변환.

use std::convert::Infallible;
use std::time::Duration;

use axum::extract::{Extension, State};
use axum::response::sse::{Event, KeepAlive, Sse};
use fleet_core::{EventEntry, FleetEvent, PermissionKind};
use std::sync::Arc;
use tokio_stream::StreamExt;
use tracing::debug;

use crate::app::DashboardState;
use crate::auth::{require_permission, AuthPrincipal};
use crate::error::ApiError;

/// 출력 열람 권한이 없는 사용자에게 보여줄 대체 문자열.
const REDACTED: &str = "[redacted: task:output permission required]";

/// `events:list` 권한만 있고 `task:output`은 없는 사용자를 위해
/// 이벤트에서 작업 stdout/stderr를 제거한다.
///
/// **왜 필요한가**: `/api/tasks/:id`(handlers.rs)는 `task:output` 권한이 없으면
/// 출력을 `None`으로 내린다. 그런데 동일한 stdout/stderr가 `TaskProgress.chunk`와
/// `TaskCompleted.result.output`을 통해 이벤트 스트림에도 흐른다. 여기서 걸러내지
/// 않으면 REST에서 막은 데이터를 SSE로 그대로 받아갈 수 있다 (권한 우회).
/// 내장 `Viewer` 역할이 정확히 이 조건(events:list 있음 / task:output 없음)이다.
fn redact_output(mut entry: EventEntry) -> EventEntry {
    match &mut entry.event {
        FleetEvent::TaskProgress { chunk, .. } => {
            *chunk = REDACTED.to_string();
        }
        FleetEvent::TaskCompleted { result, .. } => {
            result.output = REDACTED.to_string();
        }
        _ => {}
    }
    entry
}

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
    let may_see_output = principal.has(PermissionKind::TaskOutput);
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
                            let entry = if may_see_output { entry } else { redact_output(entry) };
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

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use fleet_core::{TaskId, TaskResult, WorkerId};

    fn progress_entry(chunk: &str) -> EventEntry {
        EventEntry {
            seq: 1,
            event: FleetEvent::TaskProgress {
                task_id: TaskId::new(),
                worker_id: WorkerId::new(),
                seq: 1,
                chunk: chunk.to_string(),
                at: Utc::now(),
            },
        }
    }

    fn completed_entry(output: &str) -> EventEntry {
        EventEntry {
            seq: 2,
            event: FleetEvent::TaskCompleted {
                task_id: TaskId::new(),
                worker_id: WorkerId::new(),
                result: TaskResult {
                    output: output.to_string(),
                    exit_code: 0,
                    duration_secs: 1.0,
                    token_usage: None,
                    worker_id: WorkerId::new(),
                    finished_at: Utc::now(),
                },
                at: Utc::now(),
            },
        }
    }

    #[test]
    fn redacts_task_progress_chunk() {
        let entry = redact_output(progress_entry("SECRET_API_KEY=xyz"));
        match entry.event {
            FleetEvent::TaskProgress { chunk, .. } => {
                assert_eq!(chunk, REDACTED);
                assert!(!chunk.contains("SECRET"));
            }
            other => panic!("unexpected variant: {other:?}"),
        }
    }

    #[test]
    fn redacts_task_completed_output() {
        let entry = redact_output(completed_entry("SECRET_API_KEY=xyz"));
        match entry.event {
            FleetEvent::TaskCompleted { result, .. } => {
                assert_eq!(result.output, REDACTED);
                assert!(!result.output.contains("SECRET"));
            }
            other => panic!("unexpected variant: {other:?}"),
        }
    }

    #[test]
    fn redacted_payload_never_serializes_secret() {
        // 직렬화 결과에도 원문이 남지 않아야 한다 (실제 전송 경로 검증).
        for entry in [progress_entry("TOPSECRET"), completed_entry("TOPSECRET")] {
            let payload = serde_json::to_string(&redact_output(entry)).unwrap();
            assert!(
                !payload.contains("TOPSECRET"),
                "redacted payload leaked output: {payload}"
            );
        }
    }

    #[test]
    fn leaves_non_output_events_untouched() {
        // 워커 이벤트는 출력이 없으므로 그대로 통과해야 한다.
        let entry = EventEntry {
            seq: 3,
            event: FleetEvent::WorkerJoined {
                worker_id: WorkerId::new(),
                name: "build-1".into(),
                endpoint: "wss://build-1/ws".into(),
                at: Utc::now(),
            },
        };
        let redacted = redact_output(entry);
        match redacted.event {
            FleetEvent::WorkerJoined { name, .. } => assert_eq!(name, "build-1"),
            other => panic!("unexpected variant: {other:?}"),
        }
    }
}
