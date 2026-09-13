-- 실행 중인 ACP 세션의 신원을 Task 행에 내구화한다 (로드맵 #70 게이트 2·7 선행).
--
-- 이 컬럼이 없던 동안 ACP `session_id`는 `fleet-transport`의 인메모리 맵에만
-- 존재했다(`fleet-store`·`fleet-core`에 grep 0건). 그래서 오케스트레이터가
-- 재시작하면 "내가 띄운 세션"을 가리킬 이름 자체가 사라졌고, 그 뒤의 모든
-- cancel이 대상 없이 조용히 성공했으며, 워커에게 무엇을 들고 있느냐고 물어도
-- 그 답을 내 Task에 붙일 수 없었다.
--
-- **별도 `task_attempts` 테이블을 만들지 않는 이유**는 2026-08-26의 Attempt
-- 흡수 판정(로드맵 #97) 때문이다. 무재시도 정책 아래에서 Task와 실행 시도는
-- 1:0..1이고, 그것은 문서가 아니라 코드에서 참이다 —
-- `Task::allowed_predecessors(Pending)`이 빈 배열이라 `Dispatched → Pending`이
-- 불가능하고, 한 번 dispatch된 Task는 영원히 한 번만 dispatch된다. 따라서
-- `task_id`가 곧 실행 신원이며, 세션은 그 행에 1:1로 얹힌다.
--
-- **NULL을 기본값으로 접지 않는다.** 세션이 열리기 전(Pending), 열리지 못한 채
-- 실패한 Task, 그리고 이 마이그레이션 이전의 모든 행은 세션이 **없는** 것이지
-- "빈 세션"을 가진 것이 아니다. 기본값을 주면 없던 사실을 만든다(037·040이
-- 같은 이유로 nullable이다).
ALTER TABLE tasks ADD COLUMN acp_session_id TEXT;

-- 재시작 뒤 "내가 띄웠다고 믿는 세션"을 여는 조회의 축이다. 부분 인덱스인
-- 이유는 그 조회가 항상 비어 있지 않은 값만 찾기 때문이다 — 대부분의 행은
-- NULL이고 그것들을 색인에 넣을 이유가 없다.
CREATE INDEX idx_tasks_acp_session ON tasks (acp_session_id)
    WHERE acp_session_id IS NOT NULL;
