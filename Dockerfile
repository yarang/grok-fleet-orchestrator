# syntax=docker/dockerfile:1
#
# Multi-stage build for the Grok Fleet Orchestrator workspace.
#
# This single Dockerfile builds both binaries the workspace produces
# (`fleet` — the orchestrator/CLI, and `fleet-worker` — the worker daemon)
# and exposes them as two separate final targets so each service gets a
# minimal, single-purpose runtime image:
#
#   docker build --target orchestrator -t fleet-orchestrator .
#   docker build --target worker       -t fleet-worker .
#
# docker-compose.yml builds both targets from this one file.
#
# Rust version pinned to match `.tool-versions` (rust 1.93.1) /
# `rust-toolchain.toml` (stable) — bump together if either changes.

########################################
# 1. Builder — compiles all workspace binaries
########################################
FROM rust:1.93-slim-bookworm AS builder

# `build-essential` is required because `rustls`'s `ring` backend compiles
# C/asm glue code at build time. `pkg-config` is pulled in transitively by a
# few crates for library discovery. No OpenSSL/native-tls dependency exists
# in this workspace (sqlx/reqwest/lettre all use `rustls` + `webpki-roots`),
# so no `libssl-dev` is needed.
RUN apt-get update && apt-get install -y --no-install-recommends \
        build-essential \
        pkg-config \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /build

# Team note (disk space incident): the host has repeatedly run near-full on
# the build volume from accumulated `target/debug/incremental` caches.
# Release profile already has `incremental` off by default via the workspace
# Cargo.toml's `[profile.release]`, but we set this explicitly as a second
# safety net so a Docker build never grows an incremental cache regardless of
# profile. If you see "No space left on device" during `docker build`, this
# is very likely the same host-disk issue, not a code problem — free space
# (e.g. `docker builder prune`) and retry.
ENV CARGO_INCREMENTAL=0

# The workspace has 11 members; there is no cheap "copy manifests only, build
# deps, then copy source" caching trick that stays correct for a workspace
# this size, so we copy the full source tree in one layer.
COPY . .

# Release profile (lto=thin, codegen-units=1, strip=true) is already defined
# in the workspace Cargo.toml. Builds the whole workspace with `--features
# "acp mtls"`, matching the exact command documented in docs/deployment.md
# section (C) — this also produces `fleet-worker` since it's a workspace
# member, so both binaries come out of one build.
#
# IMPORTANT — no cache mount on `target/` here, and this is deliberate, not
# an oversight. `fleet-store` embeds SQL migrations at compile time via
# `sqlx::migrate!("./migrations")` (crates/fleet-store/src/postgres.rs),
# which registers each *currently existing* migration file as a tracked
# compiler input. Verified directly against this repo's own build output
# (target/debug/libfleet_store.d): a build compiled against migrations
# 001-006 still lists only 001-006 as tracked paths after migrations
# 007-011 were added — Cargo/rustc has no mechanism to notice that a
# *directory* gained new files it never tracked, so it will happily reuse a
# stale cached `fleet-store`/`fleet-cli` artifact that is missing the new
# migrations. This is exactly the staleness class reported by the team for
# `cargo test -p fleet-mcp` reusing a stale `target/debug/fleet`. A
# `--mount=type=cache,target=/build/target` here would reintroduce that same
# bug inside Docker across separate `docker build` runs. Only the cargo
# registry (crates.io downloads, keyed off Cargo.lock content — not subject
# to this problem) is cache-mounted; `target/` starts fresh every build, so
# each image always reflects exactly the migrations present in this COPY.
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    cargo build --release --features "acp mtls" \
    && cp target/release/fleet /build/fleet \
    && cp target/release/fleet-worker /build/fleet-worker

########################################
# 2. Runtime — orchestrator (`fleet serve`)
########################################
FROM debian:bookworm-slim AS orchestrator

# `ca-certificates` backs rustls' webpki trust store for outbound HTTPS
# (SMTP via lettre, Cloudflare/OIDC calls, etc.). `curl` powers the
# HEALTHCHECK below.
RUN apt-get update && apt-get install -y --no-install-recommends \
        ca-certificates \
        curl \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --system --uid 10001 --create-home --shell /usr/sbin/nologin fleet

COPY --from=builder /build/fleet /usr/local/bin/fleet

USER fleet
WORKDIR /home/fleet

# 8081 = --http-bind (fleet-api: worker registration/heartbeat, /v1/health,
#        /metrics). 8082 = --dashboard-bind (fleet-dashboard web UI).
EXPOSE 8081 8082

HEALTHCHECK --interval=30s --timeout=5s --start-period=20s --retries=3 \
    CMD curl -fsS http://127.0.0.1:8081/v1/health || exit 1

ENTRYPOINT ["fleet"]
# All `serve` options are configurable via env vars (FLEET_HTTP_BIND,
# FLEET_DASHBOARD_BIND, FLEET_API_TOKENS, FLEET_TRANSPORT, ...) — see
# examples/fleet.env and docs/deployment.md.
CMD ["serve"]

########################################
# 3. Runtime — worker (`fleet-worker`)
########################################
FROM debian:bookworm-slim AS worker

RUN apt-get update && apt-get install -y --no-install-recommends \
        ca-certificates \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --system --uid 10002 --create-home --shell /usr/sbin/nologin fleet-worker

COPY --from=builder /build/fleet-worker /usr/local/bin/fleet-worker

# NOTE: fleet-worker manages a `grok agent serve` subprocess
# (crates/fleet-worker/src/grok_process.rs). The `grok` CLI is an external
# tool and not part of this repository, so it is intentionally NOT bundled
# here. Without it, the daemon still registers with the orchestrator and
# runs the heartbeat loop (useful for exercising the registration/heartbeat
# plumbing in local integration tests) — it will simply fail to spawn the
# grok subprocess and give up after its retry budget. To exercise real task
# dispatch, bind-mount a real `grok` binary into the container and point
# `[grok].bin` in worker.toml at it.
USER fleet-worker
WORKDIR /home/fleet-worker

ENTRYPOINT ["fleet-worker"]
CMD ["--config", "/etc/fleet/worker.toml"]
