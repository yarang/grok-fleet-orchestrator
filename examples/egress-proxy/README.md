# 이그레스 프록시 — api.z.ai 트래픽 단일 IP 통합

`oci-yarangdev-arm1` / `oci-yarangdev-ec1` / `oci-yarangdev-ec2`, 총 3개 노드가
`api.z.ai`로 보내는 아웃바운드 트래픽을 프록시 2대(HA)를 통해 단일 고정 공인 IP로 통합한다.

> 이 3개는 **각 호스트에 직접 SSH로 접속해 프로세스/리스닝 포트로 확인한 결과**다.
> 같은 `oci-yarangdev-*` 계정 그룹 안에서도 `arm2`는 grok/fleet-worker가 설치되어 있지
> 않았고, `oci-yarang-*` 계정 그룹(4대)은 아예 무관했다. **계정/호스트 이름 패턴만으로
> 유추하지 말고, 반드시 실제 프로세스로 재확인할 것** — 이 저장소의 배포 스크립트나
> 인벤토리 파일이 실제 운영 상태를 항상 반영하는 건 아니다.

## 구조

```
                         api.z.ai (실제 공인 IP, DNS로 조회)
                                 ▲
                                 │  단일 고정 공인 IP로만 도착
                    ┌────────────┴────────────┐
                    │   proxy-01 (MASTER)       │◄──VRRP──►│ proxy-02 (BACKUP) │
                    │   HAProxy (SNI relay)     │          │  HAProxy           │
                    │   Reserved Public IP 보유  │          │  failover 시 인수   │
                    └────────────┬──────────────┘
                                 │  VIP 10.0.1.100 (내부망)
                    ┌─────────────┼─────────────┐
            oci-yarangdev-arm1  oci-yarangdev-ec1  oci-yarangdev-ec2
      (각 노드 /etc/hosts: api.z.ai → 10.0.1.100)
```

grok CLI는 `HTTP_PROXY`를 몰라도 된다 — `api.z.ai`를 조회하면 프록시의 VIP가 나오도록
DNS를 속이고, 프록시는 TLS를 까지 않은 채(ClientHello의 SNI만 보고) 그대로 실제
`api.z.ai`로 중계한다. 클라이언트(grok CLI)는 실제 `api.z.ai`의 인증서를 그대로
검증하므로 TLS 신뢰 체인도 깨지지 않는다.

## VCN 토폴로지 — 확인 완료

3개 노드 모두 같은 사설 서브넷(`10.0.0.0/24`)에 있고, 노드 간 TCP 상호 도달이 실측으로
확인됐다 (`/dev/tcp`로 22번 포트 상호 접속 성공). **프록시 VM 2대를 이 노드들과 같은
VCN(가능하면 같은 서브넷)에 생성하면 사설 VIP(`10.0.1.100` 등) 방식을 그대로 쓸 수 있다.**
별도 VCN에 만들 경우에만 아래 "도달 불가" 시나리오로 전환할 것.

- **같은 VCN(또는 피어링됨)** — 기본 권장: 위 구성대로 사설 VIP를 그대로 쓴다.
- **도달 불가**(별도 VCN으로 만든 경우)면: 3개 노드의 `/etc/hosts`는 프록시의 **공인 IP**를
  가리켜야 한다. 이 경우 HAProxy의 `bind *:443`을 공인 인터페이스에 열고, 보안그룹에서
  **이 3개 노드의 공인 IP만** 443 인바운드 허용하도록 화이트리스트를 걸 것 (오픈 릴레이
  방지 — `be_deny` 백엔드만으로는 SNI 필터링만 되고 출발지 IP는 필터링하지 않는다).

## 설치 순서

1. 프록시 VM 2대 프로비저닝 (기존 워커와 별도 계정/컴파트먼트에 최소 스펙으로).
2. 각 프록시에 `haproxy.cfg`, `keepalived.conf`(MASTER/BACKUP 값 다르게),
   `notify-failover.sh` 배치.
3. OCI Reserved Public IP 하나 생성 → `notify-failover.sh`의 OCID 값 채움.
4. 3개 노드에 DNS 오버라이드 적용 — `apply-hosts-override.sh` 참조.
5. 검증 (아래 "검증 절차" 필수 — 프로덕션 트래픽 전환 전에 반드시 확인).

## 워커 노드 DNS 오버라이드 롤아웃

`~/.ssh/config`에 이미 호스트 별칭이 있으므로 별도 인벤토리 파일 없이 바로 적용 가능:

```bash
./apply-hosts-override.sh 10.0.1.100   # 프록시 VIP (또는 공인 IP)
# 특정 노드 1대만 먼저 검증하려면:
./apply-hosts-override.sh 10.0.1.100 oci-yarangdev-arm1
```

## 검증 절차 (필수, 롤아웃 전)

1. 노드 1대에서만 먼저 적용 후: `curl -v https://api.z.ai` — TLS 핸드셰이크가 정상
   완료되고 인증서 체인이 유효한지 확인 (프록시가 SNI만 보고 그대로 흘려보내므로
   실제 api.z.ai 인증서가 그대로 보여야 정상).
2. `grok agent serve`를 재시작하고 실제 워크로드 1건 실행 — 정상 응답 확인.
3. 프록시 서버에서 `tcpdump -n host <워커IP> and port 443`으로 실제로 relay를
   경유하는지 확인.
4. Z.ai 쪽에서 관측 가능하다면(대시보드 등), 요청 발신 IP가 프록시의 고정 IP로
   찍히는지 확인.
5. 위 4가지가 모두 확인된 후에만 나머지 노드에 순차 롤아웃.
6. keepalived failover 테스트: `systemctl stop haproxy` on proxy-01 → VIP/공인 IP가
   proxy-02로 넘어가는지, 그 사이 요청이 얼마나 끊기는지(수 초 이내여야 정상) 확인.

## 알려진 제약

- `be_deny` 화이트리스트는 SNI 기준이라 `api.z.ai`가 아닌 도메인으로는 절대 릴레이되지
  않는다 — 이후 다른 API를 추가로 호출하게 되면 `haproxy.cfg`에 `use_backend` 룰을
  추가해야 한다.
- 로컬 워크스테이션(Mac Studio/Mini/MacBook)의 IP 변동은 이 구성으로 해결되지 않는다 —
  서버 노드 트래픽만 대상.
