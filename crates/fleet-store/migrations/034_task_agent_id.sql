-- 로드맵 #49 2단계: Task가 어느 Agent의 것인지 기록한다.
--
-- NULL은 정상이다 — "이 Task는 특정 Agent에 묶이지 않았다"는 뜻이며, 기존
-- Task 전부와 Agent를 지목하지 않은 새 제출이 여기 해당한다. dispatch는 이
-- 값이 있을 때만 그것을 지키고, 없으면 기존 Worker 선택 그대로 동작한다.
--
-- FK를 거는 이유: agents 행에는 hard delete 경로가 없다(코드베이스 전체에
-- `DELETE FROM agents`가 없음). 따라서 기본 RESTRICT로 두어도 정상 운영을
-- 막지 않으면서, 참조 무결성이 깨진 채 남는 것을 DB가 거절해 준다.
ALTER TABLE tasks ADD COLUMN agent_id UUID REFERENCES agents(id);

-- Agent별 Task 조회(#67 게이트 ①-B의 lease 레코드가 이 방향으로 읽는다)를
-- 위한 부분 인덱스. NULL 행이 대다수이므로 전체 인덱스는 낭비다.
CREATE INDEX idx_tasks_agent_id ON tasks (agent_id) WHERE agent_id IS NOT NULL;
