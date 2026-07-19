#!/usr/bin/env bash
#
# install.sh — Grok Fleet Orchestrator 단일 바이너리 설치 스크립트.
#
# 두 개의 바이너리를 설치합니다:
#   - fleet         (orchestrator + MCP 서버 + HTTP API + 대시보드)
#   - fleet-worker  (워커 데몬)
#
# 사용법:
#   curl -fsSL https://raw.githubusercontent.com/yarang/grok-fleet-orchestrator/main/install.sh | bash
#
# 옵션:
#   --version <tag>   설치할 버전 (기본: latest). 예: v0.1.0
#   --user            $INSTALL_DIR 대신 $HOME/.local/bin 에 설치 (sudo 불필요)
#   --build           GitHub Release 대신 cargo build --release 로 로컬 빌드
#   --bin-dir <path>  바이너리 설치 경로 (기본: /usr/local/bin)
#   --no-modify-path  PATH 자동 추가 안 함
#   --uninstall       설치 제거
#   --dry-run         실제 설치 없이 무엇을 할지 출력
#   -h, --help        이 도움말
#
# 환경변수:
#   FLEET_VERSION  --version 과 동일
#   FLEET_BIN_DIR  --bin-dir 과 동일

set -euo pipefail

# ─── 기본 설정 ────────────────────────────────────────────────────────
REPO_OWNER="yarang"
REPO_NAME="grok-fleet-orchestrator"
GITHUB_API="https://api.github.com/repos/${REPO_OWNER}/${REPO_NAME}"
VERSION="${FLEET_VERSION:-}"
BIN_DIR="${FLEET_BIN_DIR:-/usr/local/bin}"
USER_INSTALL=false
BUILD_MODE=false
MODIFY_PATH=true
DRY_RUN=false
UNINSTALL=false

# 색상 (TTY 가 아니면 자동 비활성화).
if [[ -t 1 ]] && [[ -z "${NO_COLOR:-}" ]]; then
    BLUE='\033[0;34m'
    GREEN='\033[0;32m'
    YELLOW='\033[0;33m'
    RED='\033[0;31m'
    BOLD='\033[1m'
    RESET='\033[0m'
else
    BLUE='' GREEN='' YELLOW='' RED='' BOLD='' RESET=''
fi

info()  { printf "${BLUE}ℹ${RESET}  %s\n" "$*" >&2; }
warn()  { printf "${YELLOW}⚠${RESET}  %s\n" "$*" >&2; }
error() { printf "${RED}✗${RESET} %s\n" "$*" >&2; }
step()  { printf "${GREEN}✓${RESET}  %s\n" "$*" >&2; }
bold()  { printf "${BOLD}%s${RESET}" "$*" >&2; }

# ─── 인자 파싱 ────────────────────────────────────────────────────────
print_help() {
    # 파일 선두의 연속된 '# ' 주석 블록만 출력 (shebang 과 빈 줄 이후는 무시).
    awk 'NR == 1 && /^#!/ { next }        # shebang 스킵
         /^#/ { print substr($0, 3); next } # 주석 줄 출력
         { exit }' "$0" >&2
    exit 0
}

while [[ $# -gt 0 ]]; do
    case "$1" in
        --version)      VERSION="$2"; shift 2 ;;
        --user)         USER_INSTALL=true; shift ;;
        --build)        BUILD_MODE=true; shift ;;
        --bin-dir)      BIN_DIR="$2"; shift 2 ;;
        --no-modify-path) MODIFY_PATH=false; shift ;;
        --dry-run)      DRY_RUN=true; shift ;;
        --uninstall)    UNINSTALL=true; shift ;;
        -h|--help)      print_help ;;
        *)              error "unknown argument: $1"; exit 2 ;;
    esac
done

# --user 가 지정된 경우 BIN_DIR 을 덮어쓰기 (사용자가 --bin-dir 을 같이 안 준 경우).
if $USER_INSTALL && [[ "${BIN_DIR}" == "/usr/local/bin" ]]; then
    BIN_DIR="${HOME}/.local/bin"
fi

# ─── 명령어 존재 여부 헬퍼 ────────────────────────────────────────────
have() { command -v "$1" >/dev/null 2>&1; }

# ─── 언인스톨 모드 ────────────────────────────────────────────────────
do_uninstall() {
    info "uninstalling fleet + fleet-worker from ${BIN_DIR}"
    local removed=false
    for bin in fleet fleet-worker; do
        local target="${BIN_DIR}/${bin}"
        if [[ -f "${target}" ]]; then
            if $DRY_RUN; then
                info "[dry-run] rm ${target}"
            else
                rm -f "${target}"
                step "removed ${target}"
            fi
            removed=true
        fi
    done
    if ! $removed; then
        warn "no fleet binaries found in ${BIN_DIR}"
        exit 0
    fi
    # shellcheck disable=SC2088
    warn "note: PATH entries and worker.toml at /etc/fleet/ are not removed; remove manually if needed"
    exit 0
}

$UNINSTALL && do_uninstall

# ─── 플랫폼 감지 ──────────────────────────────────────────────────────
OS="$(uname -s)"
ARCH="$(uname -m)"
case "${OS}" in
    Linux*)  PLATFORM="unknown-linux-gnu" ;;
    Darwin*) PLATFORM="apple-darwin" ;;
    *)       error "unsupported OS: ${OS} (expected Linux or Darwin)"; exit 1 ;;
esac
case "${ARCH}" in
    x86_64|amd64) ARCH_NORM="x86_64" ;;
    aarch64|arm64) ARCH_NORM="aarch64" ;;
    *)             error "unsupported architecture: ${ARCH} (expected x86_64 or aarch64)"; exit 1 ;;
esac
TARGET="${ARCH_NORM}-${PLATFORM}"
info "detected target: ${TARGET}"

# ─── 버전 결정 ────────────────────────────────────────────────────────
resolve_version() {
    # --version 이 명시된 경우 그대로 사용. v 접두사 보정.
    if [[ -n "${VERSION}" ]]; then
        # set -e + `[[ ]] && cmd` 패턴 회피: if 블록 사용.
        if [[ "${VERSION}" != v* ]]; then
            VERSION="v${VERSION}"
        fi
        return 0
    fi
    # GitHub API 로 latest tag 조회. (rate limit 고려: 인증 없으면 60/h 충분.)
    local api_response
    if have curl; then
        api_response="$(curl -fsSL "${GITHUB_API}/releases/latest" 2>/dev/null || true)"
    elif have wget; then
        api_response="$(wget -qO- "${GITHUB_API}/releases/latest" 2>/dev/null || true)"
    else
        error "neither curl nor wget is installed; cannot fetch latest version"
        exit 1
    fi
    local tag_line
    tag_line="$(printf '%s' "${api_response}" | grep -m 1 '"tag_name"' || true)"
    if [[ -n "${tag_line}" ]]; then
        VERSION="$(printf '%s' "${tag_line}" \
            | sed -E 's/.*"tag_name":\s*"([^"]+)".*/\1/')"
    fi
    if [[ -z "${VERSION}" ]]; then
        error "no GitHub Releases found (or API rate limit hit)."
        warn  "hint: re-run with --build to install from source, or --version <tag>"
        exit 1
    fi
}

# ─── 다운로드 헬퍼 ────────────────────────────────────────────────────
download_to() {
    # download_to <url> <output_path>
    local url="$1" out="$2"
    if have curl; then
        curl -fsSL "${url}" -o "${out}"
    elif have wget; then
        wget -qO "${out}" "${url}"
    else
        error "neither curl nor wget is installed"
        exit 1
    fi
}

# ─── cargo build fallback ────────────────────────────────────────────
install_from_build() {
    info "building from source (cargo build --release --features 'acp mtls')"
    have cargo || { error "cargo not found; install Rust via https://rustup.rs"; exit 1; }
    if $DRY_RUN; then
        info "[dry-run] cargo build --release --features 'acp mtls'"
        info "[dry-run] cp target/release/fleet target/release/fleet-worker ${BIN_DIR}/"
        exit 0
    fi
    cargo build --release --features "acp mtls"
    local repo_root
    repo_root="$(cd "$(dirname "$0")" && pwd)"
    if [[ ! -f "${repo_root}/target/release/fleet" ]]; then
        # cargo build 를 이 스크립트 자체가 호출한 경우 repo_root 는 cwd 일 수 있음.
        repo_root="$(pwd)"
    fi
    install_binaries_from "${repo_root}/target/release"
}

# ─── GitHub Release tarball 다운로드 ──────────────────────────────────
install_from_release() {
    resolve_version
    info "installing ${VERSION} from GitHub Releases"

    local tarball_name="fleet-${VERSION}-${TARGET}.tar.gz"
    local checksum_name="fleet-${VERSION}-checksums.txt"
    local base_url="https://github.com/${REPO_OWNER}/${REPO_NAME}/releases/download/${VERSION}"
    local download_dir
    download_dir="$(mktemp -d)"
    # trap 발생 시점에 set -u 가 빈 변수를 잡지 않도록 기본값 사용.
    trap 'rm -rf "${download_dir:-}"' EXIT

    local tarball_path="${download_dir}/${tarball_name}"
    info "downloading ${tarball_name}"
    if $DRY_RUN; then
        info "[dry-run] would download ${base_url}/${tarball_name} -> ${tarball_path}"
        exit 0
    fi
    if ! download_to "${base_url}/${tarball_name}" "${tarball_path}"; then
        error "download failed for ${tarball_name}"
        warn  "the release may not include target ${TARGET}; try --build"
        exit 1
    fi

    # checksum 검증 (선택 — 검증 파일이 있을 때만).
    local checksum_path="${download_dir}/${checksum_name}"
    if download_to "${base_url}/${checksum_name}" "${checksum_path}" 2>/dev/null; then
        if have sha256sum; then
            info "verifying SHA256 checksum"
            (cd "${download_dir}" && sha256sum -c <(grep "${tarball_name}" "${checksum_path}")) \
                || { error "checksum mismatch"; exit 1; }
        elif have shasum; then
            info "verifying SHA256 checksum (shasum)"
            (cd "${download_dir}" && shasum -a 256 -c <(awk '{print $1"  "$2}' "${checksum_path}" | grep "${tarball_name}")) \
                || { error "checksum mismatch"; exit 1; }
        else
            warn "sha256sum/shasum not available; skipping checksum verification"
        fi
    else
        warn "no checksums file at release; skipping verification"
    fi

    info "extracting"
    tar -xzf "${tarball_path}" -C "${download_dir}"
    install_binaries_from "${download_dir}"
}

install_binaries_from() {
    # install_binaries_from <src_dir>  — $src_dir/fleet, $src_dir/fleet-worker 를 $BIN_DIR 로 복사.
    local src_dir="$1"
    local needs_sudo=false
    if [[ ! -d "${BIN_DIR}" ]]; then
        if [[ -w "$(dirname "${BIN_DIR}")" ]]; then
            $DRY_RUN || mkdir -p "${BIN_DIR}"
        elif have sudo; then
            needs_sudo=true
            $DRY_RUN || sudo mkdir -p "${BIN_DIR}"
        else
            error "cannot create ${BIN_DIR} (try --user to install into ~/.local/bin)"
            exit 1
        fi
    elif [[ ! -w "${BIN_DIR}" ]] && have sudo; then
        needs_sudo=true
    elif [[ ! -w "${BIN_DIR}" ]]; then
        error "${BIN_DIR} is not writable (try --user to install into ~/.local/bin)"
        exit 1
    fi

    for bin in fleet fleet-worker; do
        local src="${src_dir}/${bin}"
        local dst="${BIN_DIR}/${bin}"
        if [[ ! -f "${src}" ]]; then
            error "expected binary not found: ${src}"
            exit 1
        fi
        if $DRY_RUN; then
            info "[dry-run] cp ${src} -> ${dst}"
            continue
        fi
        if $needs_sudo; then
            sudo install -m 0755 "${src}" "${dst}"
        else
            install -m 0755 "${src}" "${dst}"
        fi
        step "installed ${dst}"
    done
}

# ─── PATH 자동 추가 ───────────────────────────────────────────────────
modify_path() {
    $MODIFY_PATH || return 0
    # set -e + `[[ ]] && cmd` 회피: if 블록 사용.
    if [[ "${BIN_DIR}" == "/usr/local/bin" ]]; then
        return 0  # 이미 표준 PATH.
    fi

    case "$(basename "${SHELL:-bash}")" in
        zsh)  rc_file="${HOME}/.zshrc" ;;
        bash) rc_file="${HOME}/.bashrc" ;;
        fish) rc_file="${HOME}/.config/fish/config.fish" ;;
        *)    rc_file="${HOME}/.profile" ;;
    esac

    local entry
    # shellcheck disable=SC2016  # ${PATH} 는 rc 파일에 literal 로 기록되어야 함.
    entry='export PATH="${PATH}:${BIN_DIR}"'
    # fish 는 다른 문법.
    if [[ "$(basename "${SHELL:-bash}")" == "fish" ]]; then
        # shellcheck disable=SC2016
        entry='set -gx PATH ${PATH} ${BIN_DIR}'
    fi

    if $DRY_RUN; then
        info "[dry-run] would add '${entry}' to ${rc_file}"
        return 0
    fi

    if ! grep -qF "${BIN_DIR}" "${rc_file}" 2>/dev/null; then
        printf '\n# Added by fleet install.sh\n%s\n' "${entry}" >> "${rc_file}"
        step "added PATH entry to ${rc_file}"
        warn "run \`source ${rc_file}\` or open a new shell to pick up PATH"
    fi
}

# ─── 메인 ─────────────────────────────────────────────────────────────
main() {
    if $BUILD_MODE; then
        install_from_build
    else
        install_from_release
    fi
    modify_path

    if ! $DRY_RUN; then
        echo ""
        step "fleet installed to ${BIN_DIR}"
        printf "%bnext steps:%b\n" "${BOLD}" "${RESET}" >&2
        cat <<'EOF' >&2

  1) Prepare Postgres + run migration:
       createdb fleet_dev
       export DATABASE_URL=postgres://$(whoami)@localhost/fleet_dev
       fleet migrate

  2) Start orchestrator:
       fleet serve --http-bind 127.0.0.1:8081 --dashboard-bind 127.0.0.1:8082

  3) Diagnose:
       fleet doctor --api-url http://127.0.0.1:8081

  Docs: https://github.com/yarang/grok-fleet-orchestrator/blob/main/docs/deployment.md
EOF
    fi
}

main
