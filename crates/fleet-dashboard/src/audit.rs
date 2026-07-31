//! 감사 로그 기록 헬퍼.
//!
//! 핸들러가 `fleet_core::AuditEvent`를 조립해 이 모듈의 [`record`]로 넘기면,
//! 스토어 기록 실패를 삼키고 로깅만 한다.
//!
//! ## 왜 실패를 삼키는가
//!
//! 감사 기록 실패가 원래 작업을 되돌리면, 감사 테이블 장애가 곧 로그인 불가로
//! 이어진다. 가용성 관점에서 이 트레이드오프는 받아들이되, 실패는 `error`
//! 레벨로 남겨 모니터링에서 잡히게 한다.
//!
//! ## 기록 원칙
//!
//! 비밀번호·세션 토큰·재설정 토큰 원문은 `detail`에 절대 넣지 않는다.
//! 감사 로그는 보관 기간이 길고 열람 범위가 넓다.

use fleet_core::AuditEvent;

use crate::DashboardState;

/// 감사 이벤트를 기록한다. 실패해도 호출 흐름을 막지 않는다.
pub async fn record(state: &DashboardState, event: AuditEvent) {
    if let Err(e) = state.store.record_audit_event(&event).await {
        tracing::error!(
            error = %e,
            action = %event.action,
            actor = %event.actor_label,
            "failed to record audit event"
        );
    }
}
