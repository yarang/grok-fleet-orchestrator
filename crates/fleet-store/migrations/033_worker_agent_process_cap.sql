-- 033: Worker의 Agent 프로세스 상한 (로드맵 #67 구현 게이트 ①-A).
--
-- 설계 정본은 `docs/architecture/agents/provisioning.md` §"배정 슬롯 상한"이다.
--
-- 이 값은 이미 Worker쪽에 있었다(`fleet-worker/src/config.rs`의
-- `grok.max_agent_processes`). 4c의 프로세스 매니저가 그것을 집행하고 초과 시
-- `observed_reason = 'cap_reached'`로 보고한다. 없던 것은 **오케스트레이터가
-- 그 숫자를 아는 것**뿐이고, 이 컬럼이 그 하나를 만든다.
--
-- **NULL을 허용하고 DEFAULT를 두지 않는다.** `workers.max_concurrent`의
-- `NOT NULL DEFAULT 4`를 베끼지 않았다 — 그 기본값은 구버전 Worker에 대해
-- 날조이기 때문이다. 실제 상한이 2인 Worker도 4로 기록되고, 배정은 그 4를
-- 근거로 초과한다. NULL은 "이 Worker의 상한을 모른다"를 뜻하며, 모르는 상한은
-- 배정 필터를 걸지 않는다(§"모르는 상한은 필터하지 않는다"). 그래도 되는
-- 이유는 오케스트레이터의 상한이 **유일한 방어선이 아니라 두 번째**이기
-- 때문이다: 최종 집행자는 Worker이고, 이 숫자가 없을 때의 최악은 초과 spawn이
-- 아니라 `cap_reached`로 거절된 관측이다.
--
-- 032의 관측 컬럼과 같은 NULL 의미를 쓰지만 **같은 축은 아니다**. 저쪽 NULL은
-- "아직 관측하지 않았다"(시간이 지나면 채워진다)이고, 이쪽 NULL은 "이 Worker는
-- 보고하지 않는다"(그 Worker가 업그레이드될 때까지 영구)다.

ALTER TABLE workers
    -- 0이나 음수는 "상한 없음"이 아니라 설정 오류다. NULL이 이미 "모른다"를
    -- 차지하고 있으므로 0에 별도 의미를 주면 두 개의 무한대가 생긴다.
    ADD COLUMN max_agent_processes INTEGER
        CHECK (max_agent_processes IS NULL OR max_agent_processes > 0);

-- 백필하지 않는다. 이미 등록된 Worker는 이 값을 보고한 적이 없고, 다음
-- 재시작의 register가 채운다 — 그 전까지 NULL이 사실이다.

-- 인덱스를 만들지 않는다. 이 컬럼은 `choose_worker`가 이미 메모리로 가져온
-- 후보 목록을 거르는 데만 쓰이고(`list_workers`는 status/labels로 조회한다),
-- 슬롯 선점의 조회는 `WHERE id = $1`의 단일 행 잠금이라 PK가 받는다.
