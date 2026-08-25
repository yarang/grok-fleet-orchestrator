-- 로드맵 #96 — Task 삭제 시 Pending 의존자 차단을 위한 부분 GIN 인덱스.
--
-- 설계 정본: docs/architecture/tasks/management.md "삭제 계약" 절.
--   삭제 전 "이 Task를 dependency_ids에 담은 Pending Task가 있는가"를 확인
--   해야 하고, 이 조회는 매 삭제 요청마다 실행되는 경로다.
--
-- 부분 인덱스인 이유: dependency_ids가 빈 배열인 Task(대다수 - 대부분의
-- Task는 다른 Task에 의존하지 않는다)는 이 조회의 대상이 될 수 없으므로
-- 인덱싱할 이유가 없다. idx_tasks_idempotency(024)와 같은 근거다.
--
-- 주의 — 이 인덱스가 실제로 쓰이려면 쿼리가 predicate를 증명해야 한다.
-- `parent_task_id = $1` 같은 등호 비교는 Postgres가 자동으로 IS NOT NULL을
-- 함의한다고 보고 idx_tasks_parent_task_id(부분, WHERE parent_task_id IS
-- NOT NULL)를 쓰지만, 배열 포함 연산자 `@>`에는 그런 함의 규칙이 없다.
-- 즉 `WHERE dependency_ids @> ARRAY[$1]::uuid[]`만 쓰면 플래너가 이 인덱스의
-- predicate(`dependency_ids <> '{}'`)를 증명하지 못해 seq scan으로 빠진다.
-- 이 인덱스를 쓰는 조회는 반드시 두 조건을 함께 명시해야 한다:
--   WHERE dependency_ids <> '{}' AND dependency_ids @> ARRAY[$1]::uuid[]
-- (docs/log.md 2026-08-26 lint 항목에 이 정정의 근거를 기록해 두었다.)
CREATE INDEX idx_tasks_dependency_ids
    ON tasks USING GIN (dependency_ids)
    WHERE dependency_ids <> '{}';
