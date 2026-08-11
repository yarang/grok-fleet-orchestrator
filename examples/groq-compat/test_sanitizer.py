"""sanitizer 단위 테스트. 의존성 없이 `python3 test_sanitizer.py` 로 실행된다."""

from __future__ import annotations

import os
import sys
import unittest

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

from sanitizer import sanitize_messages, sanitize_request  # noqa: E402


class SanitizeMessagesTest(unittest.TestCase):
    def test_strips_grok_build_fields(self):
        """실측된 실패 원인 — grok-build 가 붙이는 두 필드가 제거되어야 한다."""
        messages = [
            {"role": "user", "content": "hi"},
            {
                "role": "assistant",
                "content": "ok",
                "model_id": "llama-3.3-70b-versatile",
                "model_fingerprint": "fp_abc",
            },
        ]
        result, stripped = sanitize_messages(messages)

        self.assertEqual(stripped, {"model_id", "model_fingerprint"})
        self.assertEqual(result[1], {"role": "assistant", "content": "ok"})

    def test_preserves_standard_fields(self):
        messages = [
            {"role": "system", "content": "sys"},
            {"role": "user", "content": "q", "name": "alice"},
            {
                "role": "assistant",
                "content": None,
                "tool_calls": [
                    {
                        "id": "call_1",
                        "type": "function",
                        "function": {"name": "read_file", "arguments": "{}"},
                    }
                ],
            },
            {"role": "tool", "tool_call_id": "call_1", "content": "file body"},
        ]
        result, stripped = sanitize_messages(messages)

        self.assertEqual(stripped, set())
        self.assertEqual(result, messages)

    def test_strips_extra_keys_inside_tool_calls(self):
        messages = [
            {
                "role": "assistant",
                "content": None,
                "tool_calls": [
                    {
                        "id": "call_1",
                        "type": "function",
                        "function": {"name": "f", "arguments": "{}"},
                        "provider_meta": {"latency_ms": 12},
                    }
                ],
            }
        ]
        result, _ = sanitize_messages(messages)

        self.assertNotIn("provider_meta", result[0]["tool_calls"][0])
        self.assertEqual(result[0]["tool_calls"][0]["id"], "call_1")

    def test_null_assistant_content_without_tool_calls_becomes_empty_string(self):
        """Groq 은 content=null + tool_calls 없음 조합을 거부한다."""
        result, _ = sanitize_messages([{"role": "assistant", "content": None}])

        self.assertEqual(result[0]["content"], "")

    def test_null_assistant_content_with_tool_calls_is_left_alone(self):
        messages = [
            {
                "role": "assistant",
                "content": None,
                "tool_calls": [{"id": "c", "type": "function", "function": {}}],
            }
        ]
        result, _ = sanitize_messages(messages)

        self.assertIsNone(result[0]["content"])

    def test_does_not_mutate_input(self):
        messages = [{"role": "assistant", "content": "ok", "model_id": "x"}]
        sanitize_messages(messages)

        self.assertIn("model_id", messages[0])

    def test_non_dict_entries_pass_through(self):
        result, stripped = sanitize_messages(["not-a-dict", None])

        self.assertEqual(result, ["not-a-dict", None])
        self.assertEqual(stripped, set())


class SanitizeRequestTest(unittest.TestCase):
    def test_returns_original_object_when_nothing_stripped(self):
        data = {"model": "m", "messages": [{"role": "user", "content": "hi"}]}

        result, stripped = sanitize_request(data)

        self.assertIs(result, data)
        self.assertEqual(stripped, set())

    def test_patches_messages_and_keeps_other_top_level_keys(self):
        data = {
            "model": "llama-3.3-70b-versatile",
            "stream": True,
            "tools": [{"type": "function", "function": {"name": "f"}}],
            "messages": [{"role": "assistant", "content": "ok", "model_id": "x"}],
        }

        result, stripped = sanitize_request(data)

        self.assertEqual(stripped, {"model_id"})
        self.assertEqual(result["model"], "llama-3.3-70b-versatile")
        self.assertTrue(result["stream"])
        self.assertEqual(result["tools"], data["tools"])
        self.assertNotIn("model_id", result["messages"][0])

    def test_payload_without_messages_is_untouched(self):
        data = {"model": "text-embedding-3-small", "input": "hello"}

        result, stripped = sanitize_request(data)

        self.assertIs(result, data)
        self.assertEqual(stripped, set())


if __name__ == "__main__":
    unittest.main(verbosity=2)
