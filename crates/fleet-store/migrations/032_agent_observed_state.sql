-- 032: Agent 관측 상태 — Worker가 본 것을 오케스트레이터로 되돌린다
-- (로드맵 #67 4c-B).
--
-- 4c-A가 워커 안에 프로세스 매니저를 만들었고, 이 마이그레이션이 그 매니저가
-- 본 것을 저장할 자리를 만든다. 031이 "관측은 4c가 **별도 필드로** 얹어야
-- 한다"고 예고한 그 필드다.
--
-- ## `agents.status`의 CHECK를 넓히지 않는다
--
-- 정본의 4c-B 범위표는 `status IN ('ready','stopped')`를 넓혀 `starting`/
-- `running`/`failed`를 넣으라고 적혀 있었다. 그렇게 하지 않는 이유가 둘이다.
--
-- 첫째, **한 컬럼에 writer가 둘이 된다.** `status`는 지금 운영자의 회수
-- (`handle_stop_agent`)가 쓰는 컬럼이다. 여기에 워커의 관측이 함께 쓰면,
-- 회수가 `stopped`를 적은 직후 도착한 (그 회수를 보기 전에 만들어진) beat이
-- `running`으로 덮는다. 회수가 조용히 되돌려지고, `status`를 읽는
-- `AgentStatus::blocks_project_archive`가 회수된 Agent를 다시 archive 차단으로
-- 세게 된다. 두 축을 나누면 이 경합 자체가 없다 — 회수는 `status`만, 관측은
-- `observed_status`만 쓴다.
--
-- 둘째, `blocks_project_archive`가 **고치지 않아도 맞는 채로 남는다.** 돌고
-- 있는 Agent는 여전히 `status='ready'`라 archive를 막고, 회수된 것은
-- `'stopped'`라 막지 않는다. 축이 정말로 다르다는 가장 강한 증거다.
--
-- 031이 "두 번째 진실 원천"을 이유로 컬럼화를 거부한 것은 `Starting`처럼
-- **다른 컬럼에서 파생되는** 값에 대한 것이고, 여기 세 컬럼은 파생되지
-- 않는다 — 워커만 가진 정보다.
--
-- ## `starting`을 만들지 않는다
--
-- 정본의 이름표는 관측 `Starting`을 "Worker가 자식을 띄웠고 아직 health check
-- 전"으로 정의했는데, 4c-A에는 **health check가 없다**(`try_wait()`만 있다).
-- 계산할 방법이 없는 값을 저장하면 `#70`이 제거해야 했던 죽은 variant가 된다.
-- 관측 어휘는 `running`과 `failed` 둘이고, 관측하지 못한 것은 NULL이다.
--
-- ## 이유는 상태가 아니라 필드다
--
-- 4c-A가 거절 경로를 하나로 모으고 원인을 로그 필드로 구분한 것과 같다
-- (`agent_process.rs`의 `RejectReason`). `failed` 하나에 `observed_reason`이
-- 붙는다. 세 값 전부 4c-A에 실제 생산자가 있다: `cap_reached`와
-- `no_free_port`는 `RejectReason`의 두 갈래, `spawn_failed`는 `cmd.spawn()`이
-- `Err`를 준 경로다. 죽었다가 **되살아난** 자식에 `exited`를 두지 않는 이유는
-- 생산자가 없어서다 — 0단계가 걷어낸 자식을 3단계가 곧바로 재기동하므로,
-- 그 beat의 정직한 관측은 `running`이거나 재기동이 실패한 이유다.

ALTER TABLE agents
    ADD COLUMN observed_status TEXT
        CHECK (observed_status IN ('running', 'failed')),
    ADD COLUMN observed_at TIMESTAMPTZ,
    ADD COLUMN observed_reason TEXT
        CHECK (observed_reason IN ('cap_reached', 'no_free_port', 'spawn_failed')),
    -- 030의 `agents_placement_complete`와 같은 모양의 짝 맞춤이다. 절반만
    -- 채워진 관측("무엇을 봤는지는 아는데 언제인지 모른다")은 신선도를 판정할
    -- 수 없어 읽는 쪽에서 쓸 수 없고, 반대쪽("언제 봤는지는 아는데 무엇을
    -- 봤는지 모른다")은 아무 뜻도 없다.
    --
    -- `observed_reason`은 `failed`일 때만 채운다. `running`에 이유가 붙으면
    -- 그 이유는 지금 참이 아닌 과거의 것이고, 그것을 지울 주체가 없다.
    ADD CONSTRAINT agents_observation_complete CHECK (
        (observed_status IS NULL AND observed_at IS NULL AND observed_reason IS NULL)
        OR (observed_status = 'running' AND observed_at IS NOT NULL AND observed_reason IS NULL)
        OR (observed_status = 'failed' AND observed_at IS NOT NULL AND observed_reason IS NOT NULL)
    );

-- 백필하지 않는다. 기존 행은 전부 NULL — "아직 아무도 보지 않았다"이며, 그것이
-- 사실이다. 배포 시점에 돌고 있는 Agent 프로세스는 없다(4c-A가 방금 그 기능을
-- 만들었다). 관측이 실제로 도착하면 그때 채워진다.
--
-- 새 인덱스를 만들지 않는다. 관측을 반영하는 UPDATE는 `id`와 `worker_id`로
-- 찾고, 관측을 **읽는** 쪽은 이미 Agent 행을 손에 쥐고 있다. `observed_status`로
-- 거르는 조회는 아직 아무도 하지 않으므로, 지금 인덱스를 다는 것은 030이
-- 거부한 "채울 주체 없는 컬럼"의 인덱스판이다.
