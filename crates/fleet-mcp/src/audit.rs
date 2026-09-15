//! MCP 표면의 감사 기록 (로드맵 `#95` 2단계).
//!
//! 정본([인가·감사](../../../docs/security/authorization-and-audit.md))이 "모든
//! mutation은 append-only audit event를 남긴다"고 요구하는데, 이 표면만 **한 건도
//! 남기지 않고 있었다.** 막고 있던 것은 `ToolContext`에 호출자 신원이 없다는
//! 것이었고(로드맵 `#95`가 두 번 그렇게 적었다), 로드맵 `#58`이 런처 주장
//! 신원(`FLEET_MCP_PRINCIPAL`)을 넣으면서 해소됐다.
//!
//! **actor는 `ToolContext::created_by`와 같은 값이다.** 즉 런처가 신원을 주면
//! `mcp:<principal>`이고 주지 않으면 `"mcp"`다. 리소스의 `created_by`와 감사의
//! actor가 같은 문자열이라는 것이 중요하다 — 둘이 갈라지면 "누가 만들었나"와
//! "누가 그 행위를 했나"를 나중에 맞대 볼 수 없다.
//!
//! `actor_user_id`는 항상 `None`이다. 이 표면에는 `users` 행에 대응하는 주체가
//! 없고, 그것이 사람 행위와 이 표면을 DB에서 가르는 필드다(문자열이 아니라).

use fleet_core::AuditEvent;
use fleet_scheduler::FleetState;

/// 감사 이벤트를 기록한다. 실패해도 제어 흐름을 바꾸지 않되 조용히 넘기지도
/// 않는다 — Dashboard의 `audit::record`와 같은 처분이다.
pub async fn record(state: &FleetState, event: AuditEvent) {
    if let Err(e) = state.store.record_audit_event(&event).await {
        tracing::error!(
            error = %e,
            action = %event.action,
            actor = %event.actor_label,
            "failed to record an MCP audit event"
        );
    }
}
