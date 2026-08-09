#!/usr/bin/env bash
# examples/egress-proxy/notify-failover.sh
#
# keepalived notify_master/notify_backup/notify_fault 훅.
# MASTER로 승격될 때, OCI Reserved Public IP를 이 노드의 VNIC로 재할당해
# Z.ai가 보는 공인 이그레스 IP가 failover 전후로 동일하게 유지되도록 한다.
#
# 사전 준비:
#   - OCI CLI 설치 및 인증 (`oci setup config`), 이 인스턴스에 해당 리소스에
#     대한 IAM 정책 부여 (public-ip 조회/업데이트 권한).
#   - Reserved Public IP를 미리 생성해 OCID를 아래 변수에 채운다.

set -euo pipefail

STATE="${1:-}"
RESERVED_PUBLIC_IP_OCID="ocid1.publicip.oc1..CHANGE_ME"
THIS_PRIVATE_IP_OCID="ocid1.privateip.oc1..CHANGE_ME"   # 이 노드의 primary VNIC private IP OCID
LOG_TAG="egress-proxy-failover"

logger -t "$LOG_TAG" "state=$STATE"

case "$STATE" in
    MASTER)
        logger -t "$LOG_TAG" "promoting: reassigning reserved public IP to this node"
        oci network public-ip update \
            --public-ip-id "$RESERVED_PUBLIC_IP_OCID" \
            --private-ip-id "$THIS_PRIVATE_IP_OCID" \
            --force \
            >> /var/log/egress-proxy-failover.log 2>&1
        ;;
    BACKUP|FAULT)
        # 아무 것도 하지 않음 — public IP 재할당은 승격되는 쪽에서만 수행.
        logger -t "$LOG_TAG" "demoted or faulted, no action taken"
        ;;
    *)
        logger -t "$LOG_TAG" "unknown state: $STATE"
        ;;
esac
