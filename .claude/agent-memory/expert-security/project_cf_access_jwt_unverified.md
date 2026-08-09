---
name: cf-access-jwt-unverified
description: Open HIGH finding — fleet-api Cloudflare Access middleware parses the JWT but never verifies its signature
metadata:
  type: project
---

`crates/fleet-api/src/cloudflare.rs` verifies only `exp` and `aud` from the
Cf-Access-Jwt-Assertion JWT. The function is literally named `parse_jwt_unsafe`
and the signature is never checked against Cloudflare's public keys
(`/cdn-cgi/access/certs`). Anyone who can reach the origin directly can forge
`{"exp": <future>, "aud": "<known aud>"}` with an arbitrary signature and be
treated as an authenticated CF Access user.

**Why it still matters:** the design assumes the origin is only reachable through
the Cloudflare Tunnel, so this is defense-in-depth *today* — but it is the entire
auth boundary if the origin is ever exposed directly, misconfigured, or reachable
from inside the network. `aud` is not a secret (it appears in CF dashboard config
and logs), so it provides no protection.

**How to apply:** flag this whenever CF Access auth, fleet-api exposure, or
"Phase 5" JWT work comes up. Fix = `jsonwebtoken` crate with JWKS fetched and
cached from `https://<team>.cloudflareaccess.com/cdn-cgi/access/certs`, verifying
RS256 signature + iss + aud + exp. Reported to team 2026-07-31; not yet on the
roadmap as its own item.

Related: [[fleet-api-consumers]]
