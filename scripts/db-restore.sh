#!/usr/bin/env bash
# scripts/db-restore.sh
#
# Automates the manual `pg_restore` procedure documented in
# docs/deployment.md section 5.2. Default mode is SAFE: it restores into a
# newly created database (mirrors the doc's `createdb fleet_restored` +
# `pg_restore -d fleet_restored`), never touching whatever DATABASE_URL's
# current database holds, unless --in-place is explicitly passed.
#
# Usage:
#   # Safe default — restore into a fresh DB to inspect/verify a backup:
#   DATABASE_URL=postgres://fleet@localhost/fleet_prod \
#     scripts/db-restore.sh /var/backups/fleet/fleet_20260719T030000Z.dump
#
#   # Destructive — replace the live DB named in DATABASE_URL:
#   scripts/db-restore.sh --in-place --yes /var/backups/fleet/fleet_....dump
#
# Flags:
#   --target-db NAME   Name for the newly created database (default:
#                       fleet_restored_<timestamp>). Ignored with --in-place.
#   --in-place         Restore directly into DATABASE_URL's database instead
#                       of creating a new one. DESTRUCTIVE — drops/replaces
#                       existing objects via `pg_restore --clean`.
#   --yes              Skip the interactive confirmation prompt for
#                       --in-place (for use from other scripts / CI).
#
# This is what scripts/db-migrate-safe.sh points operators at when a
# migration fails (see docs/deployment.md 6.2: "다운그레이드는 지원되지
# 않습니다. 백업에서 복구하세요" — this script is that recovery procedure,
# automated).

set -euo pipefail

log() { printf '%s\n' "$*" >&2; }
die() { log "error: $*"; exit 1; }

usage() {
    cat >&2 <<'EOF'
Usage: db-restore.sh [--target-db NAME] [--in-place [--yes]] <dump-file>
EOF
}

TARGET_DB=""
IN_PLACE=false
ASSUME_YES=false
DUMP_FILE=""

while [[ $# -gt 0 ]]; do
    case "$1" in
        --target-db)
            TARGET_DB="${2:?--target-db requires a value}"
            shift 2
            ;;
        --in-place)
            IN_PLACE=true
            shift
            ;;
        --yes)
            ASSUME_YES=true
            shift
            ;;
        -h|--help)
            usage
            exit 0
            ;;
        -*)
            log "unknown flag: $1"
            usage
            exit 1
            ;;
        *)
            DUMP_FILE="$1"
            shift
            ;;
    esac
done

[[ -n "$DUMP_FILE" ]] || { usage; die "dump file path required"; }
[[ -f "$DUMP_FILE" ]] || die "no such file: $DUMP_FILE"
: "${DATABASE_URL:?DATABASE_URL must be set (postgres://user@host/dbname)}"

command -v pg_restore >/dev/null 2>&1 || die "pg_restore not found on PATH"
command -v psql >/dev/null 2>&1 || die "psql not found on PATH"

# Verify checksum if db-backup.sh produced one alongside the dump.
checksum_file="${DUMP_FILE}.sha256"
if [[ -f "$checksum_file" ]]; then
    log "==> verifying checksum"
    ( cd "$(dirname "$DUMP_FILE")" && sha256sum -c "$(basename "$checksum_file")" >&2 ) \
        || die "checksum verification failed — dump may be corrupt or tampered"
else
    log "==> no .sha256 file found next to dump — skipping integrity check"
fi

# Swap the dbname (and only the dbname) in a postgres:// URI, preserving any
# query string (?sslmode=..., etc). Doesn't handle URIs with slashes inside
# the query string itself — good enough for the connection strings this
# project documents (examples/fleet.env, docs/deployment.md).
derive_url_with_dbname() {
    local url="$1" new_db="$2"
    local no_query="${url%%\?*}"
    local query="${url#"$no_query"}"   # empty, or "?..."
    local base="${no_query%/*}"
    printf '%s/%s%s' "$base" "$new_db" "$query"
}

if $IN_PLACE; then
    log "==> mode: IN-PLACE restore into the database from \$DATABASE_URL"
    if ! $ASSUME_YES; then
        read -r -p "This will DROP and recreate objects in the LIVE database. Type 'yes' to continue: " confirm
        [[ "$confirm" == "yes" ]] || die "aborted by user"
    fi
    log "==> running pg_restore --clean --if-exists"
    pg_restore --clean --if-exists --no-owner --no-privileges -d "$DATABASE_URL" "$DUMP_FILE"
    log "==> in-place restore complete"
else
    TARGET_DB="${TARGET_DB:-fleet_restored_$(date -u +%Y%m%dT%H%M%SZ)}"
    log "==> mode: safe restore into new database '${TARGET_DB}'"

    maintenance_url="$(derive_url_with_dbname "$DATABASE_URL" postgres)"
    target_url="$(derive_url_with_dbname "$DATABASE_URL" "$TARGET_DB")"

    log "==> creating database ${TARGET_DB}"
    # Redirect psql's own stdout (it echoes the "CREATE DATABASE" command tag)
    # to stderr — this script's stdout contract is "only the final URL on
    # success", and callers like scripts/db-migrate-safe.sh rely on that.
    psql "$maintenance_url" -v ON_ERROR_STOP=1 -c "CREATE DATABASE \"${TARGET_DB}\";" >&2 \
        || die "CREATE DATABASE failed — does the connecting role have CREATEDB privilege?"

    log "==> running pg_restore into ${TARGET_DB}"
    pg_restore --no-owner --no-privileges -d "$target_url" "$DUMP_FILE"

    log "==> restore complete. Verify, then point DATABASE_URL at it:"
    log "    ${target_url}"
    printf '%s\n' "$target_url"
fi
