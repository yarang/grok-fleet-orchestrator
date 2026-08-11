"""OpenAI Chat Completions 요청 본문을 스펙에 맞게 정규화하는 순수 로직.

## 배경

Groq 의 OpenAI 호환 엔드포인트는 요청 본문을 **엄격하게(strict) 검증**한다.
스펙에 없는 프로퍼티가 메시지에 하나라도 붙어 있으면 400 으로 거부한다:

    'messages.6' : for 'role:assistant' the following must be satisfied
    [('messages.6' : property 'model_id' is unsupported)]

`grok` CLI(grok-build)는 assistant 메시지에 `model_id` / `model_fingerprint`
를 붙여 대화 이력에 저장하고, 이를 그대로 chat/completions 페이로드에 실어
보낸다. xAI / OpenAI / OpenRouter 는 모르는 필드를 무시하므로 문제가 없지만
Groq 은 거부한다. 그 결과 **툴 호출이 한 번이라도 발생한 턴은 두 번째 요청부터
전부 실패**한다 — 즉 에이전틱 작업(파일 읽기/편집)이 Groq 에서 원천적으로
불가능해진다. liteLLM 게이트웨이도 이 필드를 걸러주지 않고 그대로 통과시킨다
(2026-08-11 실측).

## 화이트리스트 방식을 쓰는 이유

허용 목록은 정확히 "OpenAI Chat Completions 스펙이 정의한 프로퍼티"이다.
Groq 은 그 밖의 필드를 **어차피 전부 거부**하므로, 화이트리스트 방식이
Groq 이 받아줬을 정보를 잃게 만드는 일은 원리적으로 없다. 반대로 블랙리스트
방식은 클라이언트가 새 비표준 필드를 추가할 때마다 다시 깨진다.

## 사용처

- [`litellm_hook.py`](./litellm_hook.py) — liteLLM 게이트웨이 pre-call 훅 (정본 경로)
- [`shim.py`](./shim.py) — docker 없이 쓰는 standalone 로컬 프록시

두 경로 모두 이 모듈의 [`sanitize_request`] 하나만 호출한다.
"""

from __future__ import annotations

from typing import Any

__all__ = [
    "ALLOWED_MESSAGE_KEYS",
    "ALLOWED_TOOL_CALL_KEYS",
    "sanitize_messages",
    "sanitize_request",
]

# OpenAI Chat Completions 스펙이 정의한 메시지 프로퍼티.
# 참고: https://platform.openai.com/docs/api-reference/chat/create
ALLOWED_MESSAGE_KEYS: frozenset[str] = frozenset(
    {
        "role",
        "content",
        "name",
        "tool_calls",
        "tool_call_id",
        "refusal",
        "audio",
        # deprecated 이지만 스펙에 남아 있고 Groq 도 받는다.
        "function_call",
    }
)

# tool_calls[] 원소가 가질 수 있는 프로퍼티.
ALLOWED_TOOL_CALL_KEYS: frozenset[str] = frozenset({"id", "type", "function", "index"})


def _sanitize_tool_call(tool_call: Any) -> Any:
    if not isinstance(tool_call, dict):
        return tool_call
    return {k: v for k, v in tool_call.items() if k in ALLOWED_TOOL_CALL_KEYS}


def sanitize_messages(messages: Any) -> tuple[list[Any], set[str]]:
    """`messages` 배열에서 비표준 프로퍼티를 제거한다.

    반환값은 `(정규화된 메시지 리스트, 제거된 키 이름 집합)` 이다. 제거된 키
    집합은 호출자가 로그로 남겨 "무엇이 왜 사라졌는지" 추적할 수 있게 한다.

    입력을 변경하지 않고(non-mutating) 새 리스트를 만든다.
    """
    if not isinstance(messages, list):
        return messages, set()

    stripped: set[str] = set()
    result: list[Any] = []

    for message in messages:
        if not isinstance(message, dict):
            result.append(message)
            continue

        stripped.update(set(message) - ALLOWED_MESSAGE_KEYS)
        clean = {k: v for k, v in message.items() if k in ALLOWED_MESSAGE_KEYS}

        if isinstance(clean.get("tool_calls"), list):
            clean["tool_calls"] = [_sanitize_tool_call(tc) for tc in clean["tool_calls"]]

        # Groq 은 assistant 메시지의 content 가 null 이면서 tool_calls 도 없는
        # 조합을 거부한다. 빈 문자열로 정규화한다.
        if (
            clean.get("role") == "assistant"
            and clean.get("content") is None
            and not clean.get("tool_calls")
        ):
            clean["content"] = ""

        result.append(clean)

    return result, stripped


def sanitize_request(data: Any) -> tuple[Any, set[str]]:
    """chat/completions 요청 본문 전체를 정규화한다.

    `messages` 키가 없으면 원본을 그대로 돌려준다(embeddings 등 다른 호출
    타입에서도 안전하게 부를 수 있도록).
    """
    if not isinstance(data, dict) or "messages" not in data:
        return data, set()

    messages, stripped = sanitize_messages(data["messages"])
    if not stripped:
        return data, stripped

    patched = dict(data)
    patched["messages"] = messages
    return patched, stripped
