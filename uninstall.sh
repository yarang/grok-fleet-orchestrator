#!/usr/bin/env bash
#
# uninstall.sh — fleet + fleet-worker 제거.
#
# install.sh --uninstall 와 동일하지만 별도 파일로 제공.
#
# 사용법:
#   ./uninstall.sh
#   ./uninstall.sh --bin-dir ~/.local/bin
#   ./uninstall.sh --purge   # worker.toml 도 함께 삭제 (주의)

set -euo pipefail

BIN_DIR="${FLEET_BIN_DIR:-/usr/local/bin}"
PURGE=false

while [[ $# -gt 0 ]]; do
    case "$1" in
        --bin-dir) BIN_DIR="$2"; shift 2 ;;
        --purge)   PURGE=true; shift ;;
        -h|--help)
            grep -E '^#(\s|$)' "$0" | sed 's/^# \{0,1\}//'
            exit 0
            ;;
        *) echo "unknown arg: $1" >&2; exit 2 ;;
    esac
done

removed=false
for bin in fleet fleet-worker; do
    target="${BIN_DIR}/${bin}"
    if [[ -f "${target}" ]]; then
        if [[ -w "${BIN_DIR}" ]]; then
            rm -f "${target}"
        elif command -v sudo >/dev/null 2>&1; then
            sudo rm -f "${target}"
        else
            echo "permission denied: ${target}" >&2
            exit 1
        fi
        echo "removed ${target}"
        removed=true
    fi
done

$removed || echo "no fleet binaries found in ${BIN_DIR}"

if $PURGE; then
    # worker.toml + fleet worker 상태 디렉토리 정리.
    for path in /etc/fleet/worker.toml /etc/fleet "${HOME}/.config/fleet"; do
        if [[ -e "${path}" ]]; then
            echo "removing ${path}"
            if [[ -w "$(dirname "${path}")" ]]; then
                rm -rf "${path}"
            else
                sudo rm -rf "${path}"
            fi
        fi
    done
else
    cat <<'EOF' >&2

note: /etc/fleet/worker.toml and worker state files are kept.
      run with --purge to remove them as well.
EOF
fi

echo
echo "shell PATH entries are not removed; edit your ~/.zshrc / ~/.bashrc manually."
