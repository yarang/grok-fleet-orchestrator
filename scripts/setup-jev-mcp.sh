#!/usr/bin/env bash
# setup-jev-mcp.sh — register the `jev` decision-record MCP server with this
# machine's Claude Code, at **local** scope.
#
# Why a script instead of a committed `.mcp.json`:
#   `claude mcp add --scope project` writes `.mcp.json`, and this repository
#   excludes that file from version control on purpose (.gitignore) — the same
#   file also carries this machine's SSH access path to the production
#   orchestrator hosts. Committing the jev entry would mean un-ignoring it and
#   inviting host paths back into tracked config. So the repository ships the
#   *procedure*; each machine runs it once and keeps its own client config.
#
# What jev is:
#   An MCP server that records architecture decisions (ADRs) into a local
#   SQLite file and, when TYPESAFE_API_KEY is set, submits the decision to the
#   TypeSafe `jev` System One evaluator for a fit/reversibility judgement.
#
# PRIVACY — read before enabling evaluation:
#   With TYPESAFE_API_KEY set, the decision's title, context, options,
#   rationale and consequences are sent to api.typesafe.ai. Do not submit
#   decisions containing secrets, customer data, or internal hostnames.
#   Without the key the server still records locally; only the evaluation is
#   skipped.
#
# Usage:
#   scripts/setup-jev-mcp.sh                 # register using the defaults below
#   JEV_MCP_DIR=/path/to/jev-mcp scripts/setup-jev-mcp.sh
#
# Environment:
#   JEV_MCP_DIR       checkout of git.agentthread.dev/yarang/jev-mcp
#                     (default: $HOME/working/tools/jev-mcp)
#   JEV_DB_PATH       SQLite decision store (default: $HOME/.jev/decisions.db)
#   GITEA_TOKEN       only needed if this script has to clone the repo
#   TYPESAFE_API_KEY  read from the ambient environment at server start; this
#                     script never writes it into any config file
set -euo pipefail

JEV_MCP_DIR="${JEV_MCP_DIR:-$HOME/working/tools/jev-mcp}"
JEV_DB_PATH="${JEV_DB_PATH:-$HOME/.jev/decisions.db}"
JEV_MCP_REPO="${JEV_MCP_REPO:-https://git.agentthread.dev/yarang/jev-mcp}"

die() { printf 'error: %s\n' "$*" >&2; exit 1; }

command -v claude >/dev/null || die "claude CLI not found in PATH"
command -v uv >/dev/null || die "uv not found in PATH — see https://docs.astral.sh/uv/"

if [ ! -d "$JEV_MCP_DIR" ]; then
    printf 'jev-mcp not found at %s — cloning\n' "$JEV_MCP_DIR"
    [ -n "${GITEA_TOKEN:-}" ] || die "GITEA_TOKEN is required to clone $JEV_MCP_REPO (private)"
    mkdir -p "$(dirname "$JEV_MCP_DIR")"
    # The token goes on the command line of this one call and is never written
    # to .git/config: the remote is rewritten to the token-free URL right after.
    git clone "https://oauth2:${GITEA_TOKEN}@${JEV_MCP_REPO#https://}" "$JEV_MCP_DIR" >/dev/null 2>&1 \
        || die "clone failed (check GITEA_TOKEN and network access to ${JEV_MCP_REPO#https://})"
    git -C "$JEV_MCP_DIR" remote set-url origin "$JEV_MCP_REPO"
fi

[ -f "$JEV_MCP_DIR/pyproject.toml" ] || die "$JEV_MCP_DIR does not look like a jev-mcp checkout"

mkdir -p "$(dirname "$JEV_DB_PATH")"

# Idempotent: a re-run replaces the existing registration rather than failing.
claude mcp remove jev --scope local >/dev/null 2>&1 || true
claude mcp add jev --scope local \
    -e "JEV_DB_PATH=${JEV_DB_PATH}" \
    -- uv run --directory "$JEV_MCP_DIR" jev-mcp

printf '\nregistered: jev (local scope)\n'
printf '  checkout : %s\n' "$JEV_MCP_DIR"
printf '  store    : %s\n' "$JEV_DB_PATH"
if [ -n "${TYPESAFE_API_KEY:-}" ]; then
    printf '  evaluation: ENABLED — decision text is sent to api.typesafe.ai\n'
else
    printf '  evaluation: disabled (TYPESAFE_API_KEY unset) — decisions are recorded locally only\n'
fi
printf '\nverify with: claude mcp list\n'
