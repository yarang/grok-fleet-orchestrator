#!/usr/bin/env bash
# scripts/db-migrate-safe.sh
#
# Addresses roadmap item "마이그레이션 롤백 스크립트 부재" (missing migration
# rollback script).
#
# Why this shape and not per-migration down.sql files: `fleet-store` embeds
# migrations at compile time via `sqlx::migrate!("./migrations")`
# (crates/fleet-store/src/postgres.rs), applied through simple (non-reversible)
# filenames (001_init.sql, 002_indexes.sql, ...) rather than sqlx's
# `<ver>.up.sql`/`<ver>.down.sql` convention. docs/deployment.md section 6.2
# already documents the project's chosen policy explicitly: "다운그레이드는
# 지원되지 않습니다. 백업에서 복구하세요" (downgrades are not supported,
# restore from backup). Retrofitting per-step down-migrations for the
# existing RBAC/hosts/credentials schema would be a large, invasive, and
# separately-risky change to migrations/*.sql that's out of scope here.
#
# This script instead operationalizes the documented restore-from-backup
# policy: it takes an automatic snapshot immediately before running
# migrations, and on failure prints the exact command to roll back to that
# snapshot via scripts/db-restore.sh.
#
# Usage:
#   DATABASE_URL=postgres://fleet@localhost/fleet_prod scripts/db-migrate-safe.sh
#
# Env vars:
#   DATABASE_URL         Required — same var `fleet migrate` itself reads.
#   FLEET_BIN            Path/name of the orchestrator binary (default: fleet).
#   FLEET_BACKUP_DIR      Passed through to scripts/db-backup.sh.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

log() { printf '%s\n' "$*" >&2; }
die() { log "error: $*"; exit 1; }

FLEET_BIN="${FLEET_BIN:-fleet}"

: "${DATABASE_URL:?DATABASE_URL must be set (postgres://user@host/dbname)}"
command -v "$FLEET_BIN" >/dev/null 2>&1 || die "'${FLEET_BIN}' not found on PATH (set FLEET_BIN=/path/to/fleet)"
[[ -x "${SCRIPT_DIR}/db-backup.sh" ]] || die "${SCRIPT_DIR}/db-backup.sh missing or not executable"

log "==> pre-migration snapshot"
snapshot="$("${SCRIPT_DIR}/db-backup.sh")"
[[ -n "$snapshot" ]] || die "db-backup.sh did not report a snapshot path — aborting before touching the schema"
log "==> snapshot ready: ${snapshot}"

log "==> applying migrations (${FLEET_BIN} migrate)"
if "$FLEET_BIN" migrate; then
    log "==> migrations applied successfully"
    log "==> pre-migration snapshot kept at: ${snapshot}"
    log "    (prune manually once you're confident you won't need it — retention"
    log "     rules in db-backup.sh only prune on its OWN next run)"
    exit 0
fi

status=$?
cat >&2 <<EOF

===============================================================
MIGRATION FAILED (exit ${status}).

This schema does not support automatic downgrade (see
docs/deployment.md section 6.2). To restore the exact pre-migration
state captured above:

    scripts/db-restore.sh --in-place --yes '${snapshot}'

Or, to inspect the pre-migration data without touching the live DB
first:

    scripts/db-restore.sh '${snapshot}'
===============================================================
EOF
exit "$status"
