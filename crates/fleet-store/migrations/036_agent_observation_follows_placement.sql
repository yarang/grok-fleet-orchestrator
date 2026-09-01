-- 036: 배치가 풀리면 관측도 함께 지운다 (로드맵 #67 게이트 ②).
--
-- 게이트 ②는 `assign_agent_worker`의 UPDATE에 술어를 하나 얹는다:
--
--     AND (worker_id = $2 OR observed_status IS NULL OR observed_status = 'failed')
--
-- 즉 **다른 Worker가 실제로 돌리고 있다고 보고한 Agent는 옮기지 않는다.**
-- 분할된 Worker는 명령을 받지 못하므로(명령은 heartbeat 응답으로만 간다)
-- 그동안 새 프로세스가 생길 수 없고, 그래서 이 관측은 분할 중에도 단조롭다 —
-- 옮기면 같은 Agent가 두 곳에서 도는 것을 막을 방법이 없다.
--
-- ## 그 술어가 만드는 사각지대와, 이 마이그레이션이 그것을 없애는 방식
--
-- 술어의 대가는 **관측이 stale하면 영구히 막힌다**는 것이다. Worker가
-- 제어평면과 영영 끊기면 `observed_status = 'running'`을 지울 주체가 없고,
-- 그 Worker에 얹힌 Agent는 어느 Worker로도 옮길 수 없다. 운영자가 그
-- Worker를 지워도 마찬가지였다 — 030의 FK가 `worker_id`만 NULL로 만들고
-- 관측은 남기기 때문이다.
--
-- 결정: **Worker 삭제가 그 Worker의 Agent 관측도 함께 지운다.** 삭제는 이미
-- 자격증명을 CASCADE로 파괴하는 명시적 파괴 행위이므로 "이 Worker는 없다"는
-- 운영자의 선언으로 취급한다. 대가는 정직하게 적는다 — 운영자가 틀렸다면
-- (실은 살아 있는데 제어평면과만 단절) 중복 실행이 그대로 발생한다. 판단의
-- 주체를 사람으로 옮긴 것이지 위험을 없앤 것이 아니다.
--
-- ## 왜 애플리케이션이 아니라 트리거인가
--
-- 030이 `assigned_at`에 대해 적은 근거가 여기서 더 강하게 성립한다. Worker
-- 삭제는 애플리케이션을 거치지 않고도 일어난다(운영자의 직접 DELETE, 다른
-- 인스턴스). `PgStore::delete_worker`에만 관측 삭제를 두면, 직접 DELETE는
-- stale한 `running`을 그대로 남기고 — 그 Agent는 **영구히 회수 불가능해진다**.
-- 즉 애플리케이션에 두는 선택은 이 마이그레이션이 없애려는 바로 그 실패를
-- 남긴다.
--
-- ## 이름을 바꾸는 이유
--
-- 030의 `agents_clear_assigned_at`은 이제 `assigned_at`만 지우지 않는다.
-- 이름을 그대로 두면 다음 사람이 함수 이름을 읽고 관측이 남아 있다고 믿는다.
-- 030 자체는 적용이 끝난 파일이라 고칠 수 없으므로, 여기서 정확한 이름의
-- 함수/트리거를 만들고 옛 짝을 떨어뜨린다.

-- (1) 백필. 트리거는 **앞으로의** 전이만 고친다. 이미
--     `worker_id IS NULL`인 채 관측이 남아 있는 행은 손대지 않으면 영구히
--     막힌 상태로 남는다 — 이 마이그레이션이 없애려는 바로 그 상태다.
--     아래 CHECK보다 **먼저** 와야 한다. 순서가 바뀌면 마이그레이션 자체가
--     적용되지 않는다.
UPDATE agents
   SET observed_status = NULL,
       observed_at = NULL,
       observed_reason = NULL
 WHERE worker_id IS NULL
   AND observed_status IS NOT NULL;

-- (2) 트리거 교체. 세 관측 컬럼을 **함께** 비우는 이유는 032의
--     `agents_observation_complete`가 "셋 다 NULL"만 허용하기 때문이다.
--     하나만 지우면 절반만 채워진 관측이 되어 CHECK가 깨진다.
--
--     조건이 두 개이고, **일부러 다르다.**
--
--     관측은 `worker_id`가 **바뀌면** 지운다. 관측은 "어떤 Worker가 이
--     프로세스에 대해 한 말"이지 Agent의 속성이 아니므로, 배치가 다른
--     Worker로 옮겨가는 순간 그 말은 새 자리에 대해 거짓이 된다. NULL이 될
--     때만 지우면 옛 Worker의 `failed`가 새 Worker를 따라가고, 새 Worker는
--     아직 시도조차 하지 않았는데 실패한 것으로 보인다.
--
--     `assigned_at`은 NULL이 될 때만 지운다. 030의 조건 그대로다.
--     `assign_agent_worker`가 같은 UPDATE에서 `assigned_at = now()`를 쓰므로,
--     이쪽까지 "바뀌면"으로 옮기면 방금 찍은 배치 시각을 트리거가 도로
--     지운다.
--
--     **같은 Worker로의 재배정에서는 관측이 살아남아야 한다.** `IS DISTINCT
--     FROM`이 거짓이 되어 자동으로 그렇게 되지만, 이것은 우연히 맞는 것이
--     아니라 게이트 ②가 성립하기 위한 조건이다 — 여기서 지우면 "같은
--     Worker로 한 번 재배정해 관측을 없앤 뒤 아무 데로나 옮긴다"는 2단계
--     우회가 열린다. 술어의 `worker_id = $2` 갈래가 열어 둔 문이 그대로
--     게이트를 통과하는 문이 되는 것이다.
--
--     술어와 트리거는 서로를 방해하지 않는다. UPDATE의 WHERE는 **갱신 전
--     행**에 대해 평가되고 BEFORE ROW 트리거는 그 행이 선택된 **뒤**에
--     발화하므로, 트리거가 `observed_status`를 NULL로 만드는 것이 그 행을
--     통과시킨 술어의 판정을 되돌리지 않는다.
CREATE FUNCTION agents_clear_placement_and_observation() RETURNS TRIGGER AS $$
BEGIN
    IF NEW.worker_id IS DISTINCT FROM OLD.worker_id THEN
        NEW.observed_status := NULL;
        NEW.observed_at := NULL;
        NEW.observed_reason := NULL;
    END IF;
    IF NEW.worker_id IS NULL THEN
        NEW.assigned_at := NULL;
    END IF;
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER agents_clear_placement_and_observation_trg
    BEFORE UPDATE OF worker_id ON agents
    FOR EACH ROW EXECUTE FUNCTION agents_clear_placement_and_observation();

DROP TRIGGER agents_clear_assigned_at_trg ON agents;
DROP FUNCTION agents_clear_assigned_at();

-- (3) 불변식을 DB가 강제한다. 030이 `worker_id`/`assigned_at` 짝에 대해 쓴
--     것과 같은 모양이다 — CHECK가 불변식을 선언하고 트리거가 유지한다.
--     행 단위 BEFORE 트리거는 CHECK 검증보다 먼저 발화하므로 둘은 충돌하지
--     않는다. 이 줄이 있어야 "트리거가 옳다"가 가정이 아니라 DB가 지키는
--     사실이 된다.
--
--     이 불변식은 게이트 ②의 술어 자체를 떠받치기도 한다. `worker_id = $2`
--     갈래가 미배치 Agent(`worker_id IS NULL`)에 대해 건전한 것은 그런 행에
--     관측이 남아 있을 수 없기 때문이고, 그것을 보장하는 것이 이 CHECK다.
ALTER TABLE agents
    ADD CONSTRAINT agents_observation_requires_placement CHECK (
        worker_id IS NOT NULL OR observed_status IS NULL
    );

-- 인덱스를 만들지 않는다. 032가 적은 이유가 그대로다 — 술어가 얹히는
-- UPDATE는 `id`(PK)로 행을 찾고, 관측은 그 행을 손에 쥔 뒤에 보는 값이다.
-- `observed_status`로 **거르는** 조회는 여전히 아무도 하지 않는다.
