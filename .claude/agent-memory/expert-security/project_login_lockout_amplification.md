---
name: login-lockout-amplification
description: Open HIGH finding in mainline — /login records failures inside the already-blocked branch, enabling permanent targeted account lockout
metadata:
  type: project
---

`/login` (and the bootstrap handler) call `record_login_failure(..., "rate_limited")`
*inside* the `if !allowed` branch — i.e. they write a new failure row for requests
that were already refused. The three password-reset endpoints deliberately do NOT
do this ("차단된 요청은 기록하지 않는다 — 락아웃 증폭 방지").

**Why it is exploitable:** the rate-limit window is a trailing 60s count
(`MAX_FAILED_ATTEMPTS = 5`). Because every blocked request adds another row, an
attacker sending ~5 requests/minute for a known email keeps the count at or above
the threshold forever → **permanent targeted account lockout**, unauthenticated
(CSRF is no barrier — `GET /login` yields a usable token pair). Rows are written
with `ip = 127.0.0.1` in the cloudflared deployment, so they also renew the global
IP-limb block, extending it to a full auth-surface DoS.

This was *latent* until commit `b501ca5` fixed the store-layer SQL
(`count_recent_failed_attempts` with `ip=None` used to always return 0, so the
identifier limb was dead). Fixing that bug **activated** this one.

**How to apply:** the fix is to stop writing to `login_attempts` in the blocked
branch while KEEPING the `AuditEvent::failure` audit record — the two paths must be
separated, not both removed. Reported to the team repeatedly (2026-07-31 →
2026-08-01); as of `db614ec` it is still present and **not registered on the
roadmap**. Re-verify with `grep -n '"rate_limited"' crates/fleet-dashboard/src/handlers.rs`
before acting — line numbers move constantly.

Related: [[rate-limit-ip-provenance]], [[cf-access-jwt-unverified]]
