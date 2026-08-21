#!/bin/bash
# fleet-mcp-launch.sh — canonical stdio launcher for Fleet's MCP server.
#
# Deployed on the orchestrator host at /usr/local/bin/fleet-mcp-launch.sh and
# invoked over SSH by MCP clients (Claude Code, Gemini CLI, ...). See
# docs/deployment/mcp-clients.md for the client-side config that calls this.
#
# Loads /etc/fleet/fleet.env the same way systemd's EnvironmentFile= does —
# each line's value is taken literally as the remainder of the line, with no
# shell word-splitting or re-interpretation. A naive `bash -c 'source
# fleet.env'` breaks on any value containing spaces or shell-special
# characters (FLEET_API_TOKENS is a JSON array with commas/braces;
# FLEET_GMAIL_APP_PASS is a Google App Password with embedded spaces) —
# bash would treat those characters as command separators instead of literal
# value content.
#
# Unsets FLEET_HTTP_BIND/FLEET_DASHBOARD_BIND so this ad-hoc `fleet serve`
# invocation skips the HTTP API and dashboard servers entirely (see
# runtime.rs: both are Option<String> binds, gated on being Some) — avoids
# port conflicts with the orchestrator's already-running systemd-managed
# instance, which owns those ports for real traffic.
#
# --transport acp is required — `fleet serve`'s default transport is `mock`
# (no real workers contacted at all), not the systemd service's `acp`. Without
# this flag every fleet_dispatch_task call would silently no-op against a
# fake in-memory worker instead of reaching worker-ajou-ec1 or any other real
# worker (caught while first testing this launcher against MCP tools).
#
# --no-health-check --no-cleanup --no-reconcile disable this ad-hoc process's
# own copies of the background scheduler loops. Its only job is to answer MCP
# calls for the duration of one client session — the real systemd-managed
# `fleet.service` already runs these loops continuously, and running a second
# set concurrently against the same DB risks duplicate/contending actions
# (e.g. both processes reconciling the same stale task). --no-circuit-sync is
# deliberately NOT passed — CircuitBreaker sync re-applies its own
# self-published events idempotently, so it's harmless even duplicated (see
# `fleet serve --help`).
set -euo pipefail

FLEET_ENV="${FLEET_ENV_PATH:-/etc/fleet/fleet.env}"

while IFS='=' read -r key value; do
  case "$key" in
    ''|'#'*) continue ;;
  esac
  export "$key=$value"
done < "$FLEET_ENV"

unset FLEET_HTTP_BIND FLEET_DASHBOARD_BIND
exec /usr/local/bin/fleet serve --transport acp --no-health-check --no-cleanup --no-reconcile
