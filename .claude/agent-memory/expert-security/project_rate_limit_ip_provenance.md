---
name: rate-limit-ip-provenance
description: Dashboard rate limiting keys on ConnectInfo peer IP, which is always 127.0.0.1 in the documented cloudflared deployment
metadata:
  type: project
---

Every dashboard auth handler derives `ip` from `ConnectInfo<SocketAddr>`
(`addr.ip().to_string()`), i.e. the real TCP peer. Good news: it is unspoofable
(no `X-Forwarded-For` trust). Bad news: `docs/deployment.md` §3 documents the
production topology as `cloudflared` → `http://127.0.0.1:8082`, so in production
**the peer IP is 127.0.0.1 for every request on earth**.

**Why this matters:** any rate-limit or lockout limb keyed on that IP degrades
into a single global bucket. With `MAX_IP_FAILED_ATTEMPTS = 20` /
`FAILED_ATTEMPT_WINDOW_SECS = 60`, one attacker sending 20 bad requests a minute
locks out *every* user of the deployment. Conversely it provides zero per-attacker
isolation.

**How to apply:** whenever reviewing rate limiting, lockouts, audit IPs, or
`login_attempts` rows, do not accept "it's keyed by IP" as sufficient — ask
whether the deployment is behind cloudflared. The real fix is to parse
`CF-Connecting-IP` (or `X-Forwarded-For` right-most-untrusted) *only* when the
peer is in a configured trusted-proxy allow-list, and to make per-identifier
limbs carry the real weight.

Related: [[fleet-api-consumers]]
