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
set -euo pipefail

FLEET_ENV="${FLEET_ENV_PATH:-/etc/fleet/fleet.env}"

while IFS='=' read -r key value; do
  case "$key" in
    ''|'#'*) continue ;;
  esac
  export "$key=$value"
done < "$FLEET_ENV"

unset FLEET_HTTP_BIND FLEET_DASHBOARD_BIND
exec /usr/local/bin/fleet serve
