-- 031: Agent 수렴 프로토콜 — desired state와 명령 세대 (로드맵 #67 4b).
--
-- 030이 명령이 갈 **방향**(`agents.worker_id`)을 만들었고, 이 마이그레이션이
-- 명령 **자체**를 만든다. 별도의 명령 큐 테이블을 두지 않는 이유는 설계
-- 정본(`docs/architecture/agents/provisioning.md` §"상태와 명령")에 있다:
-- 봉투의 필드가 대부분 `agents` 행의 상태이고, 수렴 모델에서는 "지연된 ACK가
-- 새 상태를 덮어쓰지 못한다"가 `WHERE command_generation = $ack` CAS로 공짜로
-- 성립한다.
--
-- **`last_acked_generation = command_generation`은 전달·수락이지 수렴이
-- 아니다.** 4b에는 프로세스 매니저가 없으므로 Worker가 정직하게 말할 수 있는
-- 최대치가 "이 세대의 명령을 받았고 받아들였다"이며, 바로 그 점이 이 컬럼들을
-- 030이 만들기를 거부한 항상-기본값 컬럼과 구분한다(030은 채울 주체가 없는
-- 컬럼을 미뤘고, 여기서는 주체가 생겼다). 4c가 이 등식을 "돌고 있다"로 읽으면
-- 어떤 테스트도 잡지 못하는 조용한 오탐이 된다 — 관측은 4c가 별도 필드로
-- 얹어야 한다.
--
-- **만들지 않은 것**: `agents.status`의 CHECK는 넓히지 않는다. 4b는 새 관측
-- 상태를 만들지 않기 때문이다 — `Starting`은 `(status, desired_status)`의
-- 순수 함수라 컬럼이 필요 없고, `Running`/`Failed`는 프로세스를 볼 수 있는
-- 4c만 관측할 수 있다. `worker_incarnation`·`fencing_token`도 만들지 않는다
-- (전자는 경합할 두 번째 writer가, 후자는 `worker_execution_lease` 테이블
-- 자체가 없다 — 구현 게이트 ①).

ALTER TABLE agents
    ADD COLUMN desired_status TEXT NOT NULL DEFAULT 'stopped'
        CHECK (desired_status IN ('running', 'stopped')),
    ADD COLUMN command_generation BIGINT NOT NULL DEFAULT 0,
    ADD COLUMN last_acked_generation BIGINT NOT NULL DEFAULT 0,
    -- ACK는 이미 낸 명령에 대해서만 올 수 있다. 이 CHECK가 없으면 CAS의
    -- 버그가 "아직 내지 않은 세대를 확인받은" 행을 조용히 남긴다.
    ADD CONSTRAINT agents_ack_not_ahead CHECK (last_acked_generation <= command_generation);

-- 백필은 DEFAULT가 한다: 기존 행 전부 `stopped`/0/0이다. `ready` 행을
-- `running`으로 올리지 않는 이유는 `AgentStatus::Ready`의 정의가 "아직 시작
-- 명령을 받지 않았다"이기 때문이다 — 배포만으로 명령이 나가면 운영자가 내린
-- 적 없는 결정이 된다. 0/0은 "명령한 적 없고 확인받은 적 없다"로 정합하다.

-- `030`의 트리거는 **건드리지 않는다.** 그 트리거는 `BEFORE UPDATE OF
-- worker_id`라 `desired_status` 변경을 덮지 못하므로, 여기에 세대 증가를
-- 얹으면 증가 주체가 트리거와 애플리케이션 둘로 갈린다. 배정 변경에서의 세대
-- 증가는 애플리케이션(`assign_agent_worker`)이 하고, FK가 유발하는
-- `ON DELETE SET NULL`은 증가시킬 필요가 없다 — Worker가 없으면 전달될 곳도
-- 없고, 다음 배정이 그때 올린다. 030이 `assigned_at`을 트리거에 둔 근거
-- ("Worker 삭제는 애플리케이션을 거치지 않고도 일어난다")는 세대로 확장되지
-- 않는다.

-- 새 인덱스를 만들지 않는다. 명령 목록 조회(`list_agent_commands`)의 술어는
--
--     WHERE worker_id = $1
--       AND (status <> 'stopped' OR last_acked_generation < command_generation)
--
-- 인데, 030의 부분 인덱스 `idx_agents_worker_status(worker_id, status) WHERE
-- worker_id IS NOT NULL`이 선두 컬럼으로 이 조회를 그대로 받는다. 뒤의 OR는
-- 인덱스가 아니라 필터로 처리되지만, 한 Worker에 배정된 Agent 수는 배정
-- 원장이 제한하는 작은 수이므로 여기서 인덱스를 새로 다는 것은 이득 없이
-- 쓰기 비용만 늘린다.
--
-- **OR의 오른쪽이 있는 이유**를 여기 남겨 둔다. `status <> 'stopped'`만으로
-- 거르면, 회수가 올린 세대가 목록에 실리지 못해 **영원히 전달되지 않는다** —
-- 회수된 모든 Agent에서 `last_acked_generation < command_generation`이 고정되고
-- `command_delivered()`가 항상 false가 된다. 위 CHECK(`ack_not_ahead`)는 이
-- 상태를 위반으로 보지 않으므로 DB가 잡아 주지도 않는다. 목록이 무한히 자라지
-- 않는 것은 확인되는 순간 오른쪽 조건이 거짓이 되어 행이 조용해지기 때문이다.
