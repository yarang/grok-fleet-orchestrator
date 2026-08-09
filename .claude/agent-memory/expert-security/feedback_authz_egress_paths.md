---
name: authz-egress-paths
description: When fixing a data-exposure bug, enumerate every egress path for that data and centralize the filter — validated approach on this repo
metadata:
  type: feedback
---

When fixing an authorization/data-exposure bug, do not patch the call site you
happened to find. First enumerate **every** path that emits the same data, then put
the filter in one shared module that all paths must traverse.

**Why:** on this repo I fixed a `task:output` bypass in the SSE stream
(`/api/events/stream`) while writing, in the same review, that "exposing the same
data through two paths and gating only one is the same as no gate." I then failed to
check the polling twin (`GET /api/events`), which had no filter at all and was a real
vulnerability. The team lead caught the pattern as valuable once corrected and
explicitly endorsed the consolidation ("공통 모듈로 묶어서 재발을 구조적으로 막은
접근도 좋습니다"), as well as the earlier decision to gate *all* admin pages rather
than only the three that were reported.

**How to apply:** for any sensitive field, grep for every handler that serializes the
containing type (polling, streaming, export, webhook, audit) before declaring a fix
complete. Prefer one `*_view`/filter module documenting *why* it exists over
per-handler redaction. In fleet-dashboard this is `src/event_view.rs` — route any new
event egress through it. Scope generously when the same class of defect obviously
spans siblings; the lead prefers the comprehensive sweep over the minimal patch.

Related: [[login-lockout-amplification]]
