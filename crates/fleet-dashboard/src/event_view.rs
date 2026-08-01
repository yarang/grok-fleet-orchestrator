//! 이벤트 egress 공통 인가 필터.
//!
//! **이 모듈이 존재하는 이유**: 동일한 fleet 이벤트가 두 경로로 나간다 —
//! 폴링(`GET /api/events`)과 스트리밍(`GET /api/events/stream`). 두 경로 모두
//! `events:list` 권한만 요구하지만, 이벤트 페이로드에는 작업 stdout/stderr가
//! 실려 있고 그 데이터는 별도 권한(`task:output`)으로 보호되는 자산이다
//! (`get_task_detail_api`는 `task:output`이 없으면 출력을 `None`으로 내린다).
//!
//! 한쪽 경로에만 필터를 걸면 다른 쪽으로 그대로 새어나간다. 실제로 SSE에만
//! 먼저 필터를 넣었다가 폴링 경로가 그대로 노출된 적이 있다. 그래서 필터를
//! 이 한 곳에 모으고, **모든 이벤트 egress는 반드시 이 함수를 통과**시킨다.

use fleet_core::{EventEntry, FleetEvent, PermissionKind};

use crate::auth::AuthPrincipal;

/// 출력 열람 권한이 없는 사용자에게 보여줄 대체 문자열.
pub const REDACTED: &str = "[redacted: task:output permission required]";

/// principal이 작업 출력을 볼 수 있는지 여부.
///
/// 스트리밍 경로에서는 연결 시점에 한 번만 평가해 재사용한다.
pub fn may_see_task_output(principal: &AuthPrincipal) -> bool {
    principal.has(PermissionKind::TaskOutput)
}

/// 이벤트 1건에서 작업 stdout/stderr를 제거한다.
///
/// `may_see_output == true`면 원본을 그대로 돌려준다.
pub fn filter_event(entry: EventEntry, may_see_output: bool) -> EventEntry {
    if may_see_output {
        return entry;
    }
    let mut entry = entry;
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

/// 이벤트 목록 전체에 [`filter_event`]를 적용한다 (폴링 경로용).
pub fn filter_events(entries: Vec<EventEntry>, may_see_output: bool) -> Vec<EventEntry> {
    if may_see_output {
        return entries;
    }
    entries
        .into_iter()
        .map(|e| filter_event(e, false))
        .collect()
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
        let entry = filter_event(progress_entry("SECRET_API_KEY=xyz"), false);
        match entry.event {
            FleetEvent::TaskProgress { chunk, .. } => assert_eq!(chunk, REDACTED),
            other => panic!("unexpected variant: {other:?}"),
        }
    }

    #[test]
    fn redacts_task_completed_output() {
        let entry = filter_event(completed_entry("SECRET_API_KEY=xyz"), false);
        match entry.event {
            FleetEvent::TaskCompleted { result, .. } => assert_eq!(result.output, REDACTED),
            other => panic!("unexpected variant: {other:?}"),
        }
    }

    #[test]
    fn permitted_user_sees_original_output() {
        let entry = filter_event(completed_entry("visible-output"), true);
        match entry.event {
            FleetEvent::TaskCompleted { result, .. } => assert_eq!(result.output, "visible-output"),
            other => panic!("unexpected variant: {other:?}"),
        }
    }

    #[test]
    fn redacted_payload_never_serializes_secret() {
        // 직렬화 결과에도 원문이 남지 않아야 한다 (실제 전송 경로 검증).
        for entry in [progress_entry("TOPSECRET"), completed_entry("TOPSECRET")] {
            let payload = serde_json::to_string(&filter_event(entry, false)).unwrap();
            assert!(
                !payload.contains("TOPSECRET"),
                "redacted payload leaked output: {payload}"
            );
        }
    }

    #[test]
    fn list_filter_redacts_every_entry() {
        // 폴링 경로 회귀: 목록 안에 섞인 출력 이벤트가 하나도 새면 안 된다.
        let entries = vec![
            progress_entry("LEAK1"),
            completed_entry("LEAK2"),
            progress_entry("LEAK3"),
        ];
        let payload = serde_json::to_string(&filter_events(entries, false)).unwrap();
        for needle in ["LEAK1", "LEAK2", "LEAK3"] {
            assert!(!payload.contains(needle), "polling path leaked {needle}");
        }
    }

    #[test]
    fn leaves_non_output_events_untouched() {
        let entry = EventEntry {
            seq: 3,
            event: FleetEvent::WorkerJoined {
                worker_id: WorkerId::new(),
                name: "build-1".into(),
                endpoint: "wss://build-1/ws".into(),
                at: Utc::now(),
            },
        };
        match filter_event(entry, false).event {
            FleetEvent::WorkerJoined { name, .. } => assert_eq!(name, "build-1"),
            other => panic!("unexpected variant: {other:?}"),
        }
    }
}
