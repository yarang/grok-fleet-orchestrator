---
name: fleet-api-consumers
description: fleet-api /v1/* has no browser consumers — dashboard is a separate same-origin server; this drives CORS/CSP decisions
metadata:
  type: project
---

fleet-api (`--http-bind`, `/v1/*`) is consumed **only by non-browser clients**:
fleet-cli, fleet-worker (registration.rs), fleet-mcp. The web dashboard is a
*separate* server (fleet-dashboard) whose frontend only calls its own same-origin
`/api/*` routes — it never talks to fleet-api over HTTP.

**Why:** this was verified by grepping every `fetch(` in
`crates/fleet-dashboard/assets/*` (all relative `/api/...` paths) and by checking
which crates depend on fleet-api (only fleet-cli, and it embeds it in-process).

**How to apply:** CORS on fleet-api should stay disabled by default (empty
allow-list) and security headers can be maximally strict (`default-src 'none'`),
because no browser is a legitimate client. If someone proposes relaxing CORS,
ask which browser app needs it — as of 2026-07-31 there is none. Opt-in
allow-list lives in `FLEET_API_CORS_ORIGINS`.

Related: [[cf-access-jwt-unverified]]
