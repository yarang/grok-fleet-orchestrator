# Memory Index

- [Rust toolchain location](reference_rust_toolchain.md) — cargo lives in ~/.asdf/shims, not on PATH; export it before building
- [fleet-api consumers](project_fleet_api_consumers.md) — no browser talks to fleet-api; drives CORS/CSP strictness
- [CF Access JWT unverified](project_cf_access_jwt_unverified.md) — open HIGH finding: signature never checked in fleet-api
- [Rate limit IP provenance](project_rate_limit_ip_provenance.md) — peer IP is always 127.0.0.1 behind cloudflared; IP-keyed limits collapse to one global bucket
- [Login lockout amplification](project_login_lockout_amplification.md) — open HIGH in mainline: /login records inside the blocked branch → permanent account lockout
- [Authz egress paths](feedback_authz_egress_paths.md) — enumerate every path emitting sensitive data; centralize the filter, don't patch one call site
