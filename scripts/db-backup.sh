#!/usr/bin/env bash
# scripts/db-backup.sh
#
# Automates the manual `pg_dump` procedure documented in
# docs/deployment.md section 5.1 ("백업 및 복구"): a full custom-format dump,
# a checksum alongside it, and retention-based cleanup of old dumps.
#
# Usage:
#   DATABASE_URL=postgres://fleet@localhost/fleet_prod scripts/db-backup.sh
#   scripts/db-backup.sh --out-dir /var/backups/fleet --retention-days 14
#
# Env vars (all optional except DATABASE_URL):
#   DATABASE_URL              Postgres connection URI (required — same var
#                              `fleet`/`fleet-cli` already reads).
#   FLEET_BACKUP_DIR           Output directory (default: /var/backups/fleet).
#   FLEET_BACKUP_RETENTION_DAYS Days of dumps to keep (default: 14; set 0 to
#                              disable pruning).
#
# Output: on success, prints the absolute path of the new dump file as the
# LAST line of stdout (all other progress goes to stderr) — this makes the
# script composable, e.g. `snapshot=$(scripts/db-backup.sh)`. It's what
# scripts/db-migrate-safe.sh relies on to capture the pre-migration snapshot.
#
# Scope note: this is periodic full-dump backup, not continuous WAL-based
# point-in-time recovery (PITR). Restoring lands you on the timestamp of the
# nearest prior dump, not an arbitrary instant. True continuous PITR needs
# WAL archiving configured on the Postgres server itself (e.g. pgBackRest,
# WAL-G, or `archive_mode=on` + `archive_command`) — that's a Postgres-server
# operational concern outside this application's scope; docs/deployment.md
# now points this out next to the automated-backup instructions.

set -euo pipefail

log() { printf '%s\n' "$*" >&2; }
die() { log "error: $*"; exit 1; }

OUT_DIR="${FLEET_BACKUP_DIR:-/var/backups/fleet}"
RETENTION_DAYS="${FLEET_BACKUP_RETENTION_DAYS:-14}"

usage() {
    cat >&2 <<'EOF'
Usage: db-backup.sh [--out-dir DIR] [--retention-days N]

  --out-dir DIR          Where to write the .dump (+ .sha256) file.
                          Default: $FLEET_BACKUP_DIR or /var/backups/fleet
  --retention-days N     Delete dumps older than N days after a successful
                          backup. 0 disables pruning.
                          Default: $FLEET_BACKUP_RETENTION_DAYS or 14
EOF
}

while [[ $# -gt 0 ]]; do
    case "$1" in
        --out-dir)
            OUT_DIR="${2:?--out-dir requires a value}"
            shift 2
            ;;
        --retention-days)
            RETENTION_DAYS="${2:?--retention-days requires a value}"
            shift 2
            ;;
        -h|--help)
            usage
            exit 0
            ;;
        *)
            log "unknown argument: $1"
            usage
            exit 1
            ;;
    esac
done

: "${DATABASE_URL:?DATABASE_URL must be set (postgres://user@host/dbname)}"
command -v pg_dump >/dev/null 2>&1 || die "pg_dump not found on PATH — install postgresql-client (matching the server's major version where possible)"
command -v sha256sum >/dev/null 2>&1 || die "sha256sum not found on PATH"

mkdir -p "$OUT_DIR"

timestamp="$(date -u +%Y%m%dT%H%M%SZ)"
dump_file="${OUT_DIR}/fleet_${timestamp}.dump"
tmp_file="${dump_file}.tmp"

log "==> dumping $(printf '%s' "$DATABASE_URL" | sed -E 's#(://[^:]+):[^@]+@#\1:****@#') -> ${dump_file}"

# --format=custom: compressed, supports selective/parallel restore via
# pg_restore (matches docs/deployment.md 5.1). --no-owner/--no-privileges:
# restoring into a differently-named/owned DB (scripts/db-restore.sh's
# default "restore into a new DB" mode) shouldn't fail on role mismatches.
if ! pg_dump --format=custom --no-owner --no-privileges --file="$tmp_file" "$DATABASE_URL"; then
    rm -f "$tmp_file"
    die "pg_dump failed — no dump file written"
fi

# Atomic rename: a reader (or a crashed run) never sees a half-written dump
# under the final name.
mv "$tmp_file" "$dump_file"

( cd "$OUT_DIR" && sha256sum "$(basename "$dump_file")" > "$(basename "$dump_file").sha256" )

size_human="$(du -h "$dump_file" | cut -f1)"
log "==> backup complete: ${dump_file} (${size_human})"

if [[ "$RETENTION_DAYS" -gt 0 ]]; then
    log "==> pruning dumps older than ${RETENTION_DAYS} day(s) in ${OUT_DIR}"
    find "$OUT_DIR" -maxdepth 1 -name 'fleet_*.dump' -type f -mtime "+${RETENTION_DAYS}" -print -delete | while read -r pruned; do
        log "    pruned: ${pruned}"
        rm -f "${pruned}.sha256"
    done
fi

# Composability contract: exactly one line, the dump path, on stdout.
printf '%s\n' "$dump_file"
