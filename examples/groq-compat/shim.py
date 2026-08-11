#!/usr/bin/env python3
"""Groq 호환 standalone 리버스 프록시 — docker 없이 쓰는 로컬 경로.

liteLLM 게이트웨이([`litellm_hook.py`](./litellm_hook.py))가 정본 경로지만,
개발자 노트북에서 `grok` CLI 하나만 Groq 에 붙이려고 Postgres + liteLLM
컨테이너를 띄우는 것은 과하다. 이 스크립트는 **표준 라이브러리만으로** 같은
정규화([`sanitizer.sanitize_request`](./sanitizer.py))를 수행하는 최소 프록시다.

## 사용법

```bash
# 1) 프록시 기동 (기본 포트 8899)
python3 examples/groq-compat/shim.py

# 2) ~/.grok/config.toml 에 프록시를 가리키는 모델 추가
#    [model.groq-free-70b]
#    base_url = "http://127.0.0.1:8899/v1"
#    api_key  = "gsk_..."
#    model    = "llama-3.3-70b-versatile"
#    api_backend = "chat_completions"

# 3) 평소처럼 사용
grok -m groq-free-70b -p "..."
```

환경변수:

- `PORT` — 리슨 포트 (기본 `8899`)
- `HOST` — 리슨 주소 (기본 `127.0.0.1`; 로컬 전용이므로 기본값 유지 권장)
- `UPSTREAM_BASE_URL` — 업스트림 (기본 `https://api.groq.com/openai/v1`)

API 키는 저장하지 않는다 — 클라이언트의 `Authorization` 헤더를 그대로 전달한다.
"""

from __future__ import annotations

import json
import os
import ssl
import sys
import urllib.error
import urllib.request
from http.server import BaseHTTPRequestHandler, HTTPServer
from socketserver import ThreadingMixIn

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

from sanitizer import sanitize_request  # noqa: E402

UPSTREAM = os.environ.get("UPSTREAM_BASE_URL", "https://api.groq.com/openai/v1").rstrip("/")
HOST = os.environ.get("HOST", "127.0.0.1")
PORT = int(os.environ.get("PORT", "8899"))

# 일부 macOS python.org 빌드는 시스템 루트 인증서를 보지 못한다.
try:
    import certifi

    SSL_CONTEXT = ssl.create_default_context(cafile=certifi.where())
except ImportError:  # pragma: no cover
    SSL_CONTEXT = ssl.create_default_context()

# 업스트림 CDN 이 기본 urllib User-Agent 를 봇으로 차단(Cloudflare 1010)하므로
# 평범한 SDK UA 로 교체한다.
_USER_AGENT = "OpenAI/Python 1.0"

_HOP_BY_HOP = {
    "connection",
    "keep-alive",
    "proxy-authenticate",
    "proxy-authorization",
    "te",
    "trailers",
    "transfer-encoding",
    "upgrade",
    "content-length",
    "host",
    "user-agent",
    "accept-encoding",
}


def _upstream_url(path: str) -> str:
    """`/v1/chat/completions` → `<UPSTREAM>/chat/completions`."""
    if path.startswith("/v1/"):
        path = path[3:]
    elif path == "/v1":
        path = "/"
    return UPSTREAM + path


class Handler(BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"
    server_version = "groq-compat-shim/1.0"

    def log_message(self, fmt: str, *args: object) -> None:
        sys.stderr.write("[shim] %s - %s\n" % (self.address_string(), fmt % args))

    # --- helpers ---------------------------------------------------------

    def _forward_headers(self) -> dict[str, str]:
        headers = {
            k: v for k, v in self.headers.items() if k.lower() not in _HOP_BY_HOP
        }
        headers["User-Agent"] = _USER_AGENT
        return headers

    def _relay(self, method: str, body: bytes | None) -> None:
        req = urllib.request.Request(
            _upstream_url(self.path), data=body, method=method,
            headers=self._forward_headers(),
        )

        try:
            resp = urllib.request.urlopen(req, context=SSL_CONTEXT)
            status = resp.getcode()
        except urllib.error.HTTPError as exc:
            resp = exc
            status = exc.code
        except urllib.error.URLError as exc:
            self.send_error(502, f"upstream unreachable: {exc.reason}")
            return

        ctype = resp.headers.get("Content-Type", "application/json")
        self.send_response(status)
        self.send_header("Content-Type", ctype)
        # 길이를 미리 알 수 없으므로(SSE 스트리밍 포함) 항상 chunked 로 중계한다.
        self.send_header("Transfer-Encoding", "chunked")
        self.end_headers()

        try:
            while True:
                chunk = resp.read(4096)
                if not chunk:
                    break
                self.wfile.write(b"%X\r\n%s\r\n" % (len(chunk), chunk))
                self.wfile.flush()
            self.wfile.write(b"0\r\n\r\n")
        except BrokenPipeError:  # 클라이언트가 도중에 끊음
            pass
        finally:
            resp.close()

    # --- verbs -----------------------------------------------------------

    def do_POST(self) -> None:
        length = int(self.headers.get("Content-Length") or 0)
        body = self.rfile.read(length) if length else b""

        try:
            payload = json.loads(body) if body else None
        except json.JSONDecodeError:
            payload = None

        if payload is not None:
            patched, stripped = sanitize_request(payload)
            if stripped:
                self.log_message("stripped %s", sorted(stripped))
                body = json.dumps(patched).encode()

        self._relay("POST", body)

    def do_GET(self) -> None:
        self._relay("GET", None)


class ThreadingHTTPServerV11(ThreadingMixIn, HTTPServer):
    daemon_threads = True
    allow_reuse_address = True


def main() -> None:
    server = ThreadingHTTPServerV11((HOST, PORT), Handler)
    sys.stderr.write(f"[shim] listening on http://{HOST}:{PORT}/v1 -> {UPSTREAM}\n")
    try:
        server.serve_forever()
    except KeyboardInterrupt:
        sys.stderr.write("\n[shim] shutting down\n")
        server.shutdown()


if __name__ == "__main__":
    main()
