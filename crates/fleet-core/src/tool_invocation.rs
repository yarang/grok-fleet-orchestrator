//! Agent가 Task 실행 중 호출한 도구의 **관측** (로드맵 `#70` 게이트 4 선행).
//!
//! ## 이것은 effect ledger가 아니다
//!
//! [실행 일관성](../../../docs/architecture/tasks/execution-consistency.md)의
//! effect ledger는 `Planned`/`Started`/`Applied`/`NoEffect`/`Compensating`/
//! `Compensated`/`Unknown`/`CompensationFailed` 여덟 상태와 provider
//! idempotency key, external receipt를 요구한다. 이 모듈은 그 중 **아무것도
//! 구현하지 않는다.**
//!
//! 그럼에도 이것이 그 원장의 선행인 이유가 있다. 원장에 무엇을 적으려면 먼저
//! **일어난 일을 알아야 하는데, 오케스트레이터는 그것을 알 방법이 없었다.**
//! 도구는 Worker의 grok 프로세스 안에서 돌고 오케스트레이터는 ACP로
//! `session/prompt`만 보낸다. ACP는 `session/update`로 도구 호출을 알려주지만
//! `acp_transport.rs`가 그 알림을 `_ => None`으로 전부 버리고 있었다 — 즉 Task가
//! 실제로 무엇을 했는지에 대한 유일한 증거가 매번 폐기됐다.
//!
//! 그래서 원장의 상태 기계를 짓기 전에 그 증거를 붙잡는다. **없는 것을 미리
//! 만들지 않되, 있는데 버리고 있던 것은 줍는다.**
//!
//! ## 무엇을 기록하고 무엇을 버리는가
//!
//! ACP의 `ToolCall`은 `title`·`raw_input`·`raw_output`·`content`·`locations`를
//! 함께 준다. 이 구조체는 **그 다섯을 전부 버린다.**
//! [관측성 정본](../../../docs/architecture/observability-and-reconciliation.md)의
//! 금지 목록이 prompt·사용자 입력·repository URL·raw provider payload를 명시하고,
//! 위 다섯은 전부 거기에 해당하거나 해당할 수 있다 — `title`은 "Read
//! /home/user/.ssh/id_rsa"처럼 경로와 인자를 사람이 읽는 문장으로 담는다.
//!
//! 남기는 넷은 그 위험이 없다: [`tool_call_id`](ToolInvocation::tool_call_id)는
//! 세션 범위의 불투명한 식별자, [`kind`](ToolInvocation::kind)와
//! [`status`](ToolInvocation::status)는 **닫힌 어휘**,
//! [`name`](ToolInvocation::name)은 도구의 프로그램적 이름이라 사용자 입력이
//! 아니라 설정이다. 그리고 그 넷이 곧 원장 명세가 요구하는 `tool_id`와
//! side-effect class에 대응한다.

use serde::{Deserialize, Serialize};

/// 도구의 범주. ACP `ToolKind`의 거울.
///
/// **ACP 타입을 그대로 쓰지 않는 이유**: `fleet-core`는
/// `--no-default-features`(=`acp` 꺼짐)에서도 컴파일돼야 하는데 그 세트에는
/// `agent-client-protocol`이 없다. 거울을 두면 이 어휘가 transport 계층의
/// 피처 게이트와 무관해진다.
///
/// 값을 늘리거나 이름을 바꾸는 것은 저장된 이벤트의 표현을 바꾸는 일이므로,
/// ACP가 새 종류를 더하면 [`Other`](Self::Other)로 접는다 — 알 수 없는 종류를
/// 새 이름으로 만들면 옛 이벤트를 읽는 쪽이 깨진다.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolInvocationKind {
    Read,
    Edit,
    Delete,
    Move,
    Search,
    Execute,
    Think,
    Fetch,
    SwitchMode,
    Other,
}

/// 도구 호출의 실행 상태. ACP `ToolCallStatus`의 거울.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolInvocationStatus {
    Pending,
    InProgress,
    Completed,
    Failed,
}

impl ToolInvocationStatus {
    /// 더 이상 전이가 오지 않는 상태인가.
    ///
    /// effect ledger가 언젠가 물을 질문("이 호출은 끝났는가")을 지금 답할 수
    /// 있는 만큼만 답한다. 다만 `Completed`가 **부작용이 적용됐다는 증거는
    /// 아니다** — Agent가 그렇게 보고했을 뿐이고, 정본이 "process output이나
    /// 모델의 '완료' 서술은 effect 증거가 아니다"라고 적은 그것이다.
    pub fn is_terminal(&self) -> bool {
        matches!(self, Self::Completed | Self::Failed)
    }
}

/// 도구 호출 하나에 대해 Agent가 알려 온 것 중 **남겨도 안전한 부분**.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolInvocation {
    /// 세션 범위에서 이 호출을 가리키는 불투명한 식별자.
    ///
    /// 같은 값으로 여러 번 온다 — ACP는 시작을 `ToolCall`로, 이후 변화를
    /// `ToolCallUpdate`로 보낸다. 이벤트를 접거나 합치지 않고 온 대로 남기는
    /// 이유는 그 전이 자체가 원장이 물을 사실이기 때문이다.
    pub tool_call_id: String,
    /// 도구의 프로그램적 이름. ACP에서 선택 필드라 없을 수 있다.
    ///
    /// effect ledger 명세의 `tool_id`에 대응하는 유일한 필드다.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    pub kind: ToolInvocationKind,
    pub status: ToolInvocationStatus,
}
