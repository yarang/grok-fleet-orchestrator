-- Agent 배정 (로드맵 #67 4a).
--
-- 4단계를 4a/4b/4c로 나눈 근거는 docs/architecture/agents/provisioning.md에
-- 있다. 요약하면: 명령 봉투는 명령을 받을 *프로세스*를 만들지만 명령이 갈
-- *방향*은 만들지 못한다. 그 방향의 저장 자리가 `worker_id`다 — 이 컬럼이
-- 없으면 heartbeat 응답에 desired state를 실을 때 "어느 Worker의 응답에
-- 싣는가"에 답할 수 없다. 4b가 만들 수렴 프로토콜의 선행이다.
--
-- FK를 거는 근거는 `027_agents.sql`이 `agents.project_id`에 쓴 것과 같다:
-- 이 migration이 **만드는** 컬럼이므로 배포 DB에 검증되지 않은 선행 값이
-- 존재할 수 없다. `022`가 `tasks.project_id`에 FK를 걸지 않게 했던 조건
-- (선행 데이터)이 여기서는 성립하지 않는다.
--
-- `ON DELETE SET NULL`이 `027`의 `RESTRICT`와 다른 이유: 저쪽은 Project가
-- 사라질 때 Agent가 조용히 함께 사라지지 않게 막는 것이고, 이쪽은 Worker
-- 등록이 해제될 때 그 배정이 **더는 참이 아님**을 기록하는 것이다. 배정은
-- Agent의 정체성이 아니라 운영 상태이므로 사라져도 Agent는 그대로 유효하다.
-- 4a에는 정리할 프로세스가 아직 없어 배정 회수는 컬럼을 비우는 것이 전부이며,
-- 그 전부를 DB가 대신 해 준다. 4c에서 프로세스가 생기면 "Worker가 사라졌다"는
-- cleanup 증거를 요구하는 사건이 되고, 그때는 이 자동 회수만으로 부족해진다.
--
-- **만들지 않은 것**: `desired_status`/`command_generation`/
-- `last_acked_generation`. 이것들을 채우는 주체는 4b(수렴 프로토콜)이며,
-- 지금 만들면 아무도 쓰지 않는 항상-기본값 컬럼이 된다. Worker가 신고하는
-- `max_agent_processes`도 만들지 않는다 — 프로세스 매니저(4c)가 없는 동안
-- Worker는 자기 상한을 집행할 수 없으므로, 집행되지 않는 수를 저장하는 것은
-- 항상-NULL 컬럼의 뒤집힌 형태다.
ALTER TABLE agents
    ADD COLUMN worker_id UUID REFERENCES workers(id) ON DELETE SET NULL,
    ADD COLUMN assigned_at TIMESTAMPTZ,
    -- 배정의 절반만 남는 상태를 스키마가 막는다(`029`의
    -- `agents_template_pin_complete`와 같은 이유). `assigned_at`만 있고
    -- `worker_id`가 없으면 어디에 배정됐는지 답할 수 없고, `worker_id`만
    -- 있고 `assigned_at`이 없으면 언제부터 그랬는지 답할 수 없다.
    -- `ON DELETE SET NULL`이 `worker_id`만 비우면 이 CHECK가 깨지므로
    -- 애플리케이션이 아니라 여기서 막는다 — 아래 트리거 참고.
    ADD CONSTRAINT agents_placement_complete CHECK (
        (worker_id IS NULL AND assigned_at IS NULL)
        OR (worker_id IS NOT NULL AND assigned_at IS NOT NULL)
    );

-- `ON DELETE SET NULL`은 `worker_id`만 NULL로 만들어 위 CHECK를 위반시킨다.
-- Postgres는 FK의 SET NULL 대상을 컬럼 단위로만 지정할 수 있으므로(그리고
-- `assigned_at`은 FK의 일부가 아니므로) 짝을 맞추는 일은 트리거가 한다.
-- 이것을 애플리케이션에 두지 않는 이유: Worker 삭제는 애플리케이션을 거치지
-- 않고도(운영자의 직접 DELETE, 다른 인스턴스) 일어나며, 그때 CHECK 위반으로
-- 삭제가 실패하면 원인이 전혀 다른 곳에 나타난다.
CREATE FUNCTION agents_clear_assigned_at() RETURNS TRIGGER AS $$
BEGIN
    IF NEW.worker_id IS NULL THEN
        NEW.assigned_at := NULL;
    END IF;
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER agents_clear_assigned_at_trg
    BEFORE UPDATE OF worker_id ON agents
    FOR EACH ROW EXECUTE FUNCTION agents_clear_assigned_at();

-- 배정 원장 조회 — "이 Worker에 배정된, 아직 회수되지 않은 Agent가 몇인가".
-- least-loaded 정렬의 부하 출처이며 Worker 자기보고를 쓰지 않는다
-- (`crates/fleet-scheduler/src/selector.rs`가 #67 3단계에서 같은 이유로
-- `Worker::active_tasks`를 버렸다). `status`를 인덱스에 포함하는 이유는 그
-- 조회가 `status <> 'stopped'`로 거르기 때문이다 — 회수된 Agent는 슬롯을
-- 잡지 않으므로, 회수만으로 슬롯이 풀리고 배정 회수 경로가 따로 필요 없다.
-- 부분 인덱스인 이유는 배정되지 않은 Agent가 이 조회의 대상이 아니어서다.
CREATE INDEX idx_agents_worker_status
    ON agents(worker_id, status)
    WHERE worker_id IS NOT NULL;
