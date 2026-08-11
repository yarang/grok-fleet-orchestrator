"""liteLLM 게이트웨이용 pre-call 훅 — Groq 호환 정규화 (정본 경로).

[`sanitizer.sanitize_request`](./sanitizer.py) 를 liteLLM 프록시의
`async_pre_call_hook` 에 연결한다. 업스트림 공급자로 나가기 직전에 비표준
메시지 프로퍼티를 제거하므로, Groq 처럼 요청 본문을 엄격 검증하는 공급자도
`grok` CLI 같은 클라이언트를 그대로 받아들일 수 있다.

## 설정

`examples/litellm-config.yaml` 에 다음을 추가한다:

```yaml
litellm_settings:
  callbacks: ["groq_compat.litellm_hook.proxy_handler_instance"]
```

liteLLM 은 이 문자열을 **config.yaml 이 있는 디렉토리 기준**으로 해석해
`<config_dir>/groq_compat/litellm_hook.py` 를 로드한다
(`litellm/proxy/types_utils/utils.py::get_instance_fn`). 따라서
`docker-compose.yml` 에서 이 디렉토리를 config.yaml 옆에 마운트해야 한다:

```yaml
volumes:
  - ./examples/litellm-config.yaml:/app/config.yaml:ro
  - ./examples/groq-compat:/app/groq_compat:ro
```

## 주의

liteLLM 은 이 모듈을 `spec_from_file_location` 으로 **패키지가 아닌 단독
모듈**로 로드한다. 따라서 `from .sanitizer import ...` 같은 상대 임포트는
동작하지 않는다 — 아래처럼 모듈 자신의 디렉토리를 `sys.path` 에 넣고 절대
임포트해야 한다.
"""

from __future__ import annotations

import os
import sys
from typing import Any

# 상대 임포트가 불가능한 로딩 방식이므로 자기 디렉토리를 경로에 추가한다.
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

from sanitizer import sanitize_request  # noqa: E402

try:  # liteLLM 이 설치된 환경에서만 존재
    from litellm.integrations.custom_logger import CustomLogger
except ImportError:  # pragma: no cover - 단위 테스트/로컬 개발 편의
    class CustomLogger:  # type: ignore[no-redef]
        """liteLLM 미설치 환경에서 임포트만 통과시키기 위한 스텁."""


try:
    from litellm._logging import verbose_proxy_logger as _logger
except ImportError:  # pragma: no cover
    import logging

    _logger = logging.getLogger(__name__)


class OpenAICompatSanitizer(CustomLogger):
    """업스트림 전송 직전에 비표준 메시지 프로퍼티를 제거하는 훅."""

    async def async_pre_call_hook(
        self,
        user_api_key_dict: Any,
        cache: Any,
        data: dict,
        call_type: str,
    ) -> dict | None:
        patched, stripped = sanitize_request(data)
        if stripped:
            _logger.info(
                "groq-compat: stripped non-standard message properties %s (call_type=%s)",
                sorted(stripped),
                call_type,
            )
        return patched


# liteLLM config 의 `callbacks:` 가 참조하는 인스턴스 이름.
proxy_handler_instance = OpenAICompatSanitizer()
