-- Control plane 권한 lease (로드맵 #63, 1단계).
--
-- Fleet는 하나의 논리적 제어 기관만 허용한다(docs/architecture/
-- control-plane-authority-and-failover.md) — 유효한 dispatch lease를 가진
-- Orchestrator는 최대 하나다. 지금까지는 이걸 강제하는 저장소 레벨 primitive가
-- 전혀 없어서, 같은 DB를 가리키는 두 프로세스를 실수로 동시에 띄우면
-- Active-Active로 동작해 dispatch·cancel·breaker 변경이 충돌할 수 있었다.
--
-- 이 테이블은 그 primitive만 담는다 — "누가 지금 제어권을 쥐고 있는가"를
-- CAS(조건부 UPDATE)로 관리한다. dispatch/cancel/Agent command 핸들러가 이
-- lease를 실제로 검사하도록 배선하는 것과, Worker 안의 Agent process를 막는
-- `worker_execution_lease`(fencing token)는 별도 단계(로드맵 #67)다 — 지금은
-- lease 자체의 획득·갱신·상실이 여러 인스턴스 사이에서 정확히 동작하는지만
-- 보장한다.
--
-- 설계 노트:
--   * `cluster_id`를 PK로 둬 하나의 DB가 이론상 여러 독립 클러스터의 lease를
--     같은 테이블에서 관리할 수 있게 했다 — 실제 배포는 고정값
--     하나("default")만 쓰지만, 테스트 격리(클러스터 ID를 테스트마다 다르게)에도
--     그대로 쓸모가 있다.
--   * `epoch`는 획득할 때마다(최초 획득 포함) 1씩 증가하는 단조 값이다.
--     이전 epoch에서 시작된 in-flight 요청이 새 epoch 획득 이후에도 살아남아
--     상태를 바꾸는 것을 막는 근거가 된다(불변식 4·5). NOT NULL DEFAULT 0으로
--     시작해 최초 acquire가 1로 만든다.
--   * 시각 비교는 전부 `NOW()`(DB 서버 시각) 기준이다 — 애플리케이션 서버 시각
--     을 신뢰하면 클럭 스큐만으로 두 인스턴스가 동시에 "내가 유효하다"고
--     믿을 수 있다.
CREATE TABLE control_plane_lease (
    cluster_id        TEXT PRIMARY KEY,
    active_instance_id TEXT NOT NULL,
    epoch             BIGINT NOT NULL DEFAULT 0 CHECK (epoch >= 0),
    acquired_at       TIMESTAMPTZ NOT NULL,
    expires_at        TIMESTAMPTZ NOT NULL,
    last_renewed_at   TIMESTAMPTZ NOT NULL
);

-- lease 만료 여부를 자주 확인하므로(획득 시도마다) 인덱스로 뒷받침한다.
CREATE INDEX idx_control_plane_lease_expires ON control_plane_lease(expires_at);
