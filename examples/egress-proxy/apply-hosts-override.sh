#!/usr/bin/env bash
# examples/egress-proxy/apply-hosts-override.sh
#
# 실제로 grok agent serve 워크로드가 도는 3개 노드(oci-yarangdev-arm1/ec1/ec2)의
# /etc/hosts에 "api.z.ai -> <프록시 IP>" 항목을 멱등하게(이미 있으면 갱신, 없으면 추가) 적용한다.
# ~/.ssh/config 에 이미 정의된 Host 별칭을 그대로 사용하므로 별도 인벤토리 불필요.
#
# 주의: oci-yarang-* (4대)와 oci-yarangdev-arm2는 grok/fleet-worker가 설치되어
# 있지 않음을 각 호스트에서 직접 확인했으므로(프로세스/리스닝 포트 없음) 대상에서
# 제외했다. 계정 그룹명만 보고 유추하지 말 것 — 반드시 실제 프로세스로 재확인.
#
# 사용:
#   ./apply-hosts-override.sh <프록시 VIP 또는 공인 IP>
#   ./apply-hosts-override.sh 10.0.1.100 oci-yarangdev-arm1   # 특정 노드만 먼저 검증할 때

set -euo pipefail

PROXY_IP="${1:?사용법: $0 <프록시 IP> [호스트별칭...]}"
shift || true

DEFAULT_HOSTS=(
    oci-yarangdev-arm1
    oci-yarangdev-ec1
    oci-yarangdev-ec2
)

TARGETS=("$@")
if [ "${#TARGETS[@]}" -eq 0 ]; then
    TARGETS=("${DEFAULT_HOSTS[@]}")
fi

MARKER="# egress-proxy-override: api.z.ai"

for host in "${TARGETS[@]}"; do
    echo "==> ${host}"
    ssh "$host" bash -s -- "$PROXY_IP" "$MARKER" <<'REMOTE'
set -euo pipefail
PROXY_IP="$1"
MARKER="$2"
LINE="${PROXY_IP} api.z.ai ${MARKER}"

if grep -qF "$MARKER" /etc/hosts 2>/dev/null; then
    sudo sed -i.bak "/${MARKER//\//\\/}/c\\${LINE}" /etc/hosts
    echo "updated existing entry"
else
    echo "$LINE" | sudo tee -a /etc/hosts > /dev/null
    echo "added new entry"
fi

getent hosts api.z.ai || true
REMOTE
done

echo
echo "완료. 각 노드에서 'curl -v https://api.z.ai' 로 TLS 핸드셰이크 확인 필요."
