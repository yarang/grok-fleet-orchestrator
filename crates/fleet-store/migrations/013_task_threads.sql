-- 013_task_threads.sql — 작업 스레드(연속 대화) 지원 스키마
--
-- 설계 배경: LLM이 태스크를 질문으로 마쳐 사용자 응답이 필요한 경우, 새 태스크를
-- "이어가기(Reply)"로 제출할 수 있어야 한다. 이를 위해 두 개의 독립적인 축을 둔다:
--
--   parent_task_id — 바로 앞 태스크가 무엇이었는지 (체인의 한 칸).
--   thread_id      — 체인 전체를 한 번에 조회하기 위한 평평한(flat) 키.
--                    루트 태스크는 자기 자신의 id를 thread_id로 갖고, 모든 자식은
--                    생성 시점에 부모의 thread_id를 그대로 물려받는다. 이렇게 하면
--                    "이 스레드의 모든 태스크"를 재귀 쿼리 없이
--                    `WHERE thread_id = $1` 한 번으로 구할 수 있다.
--
-- project_id는 아직 project 테이블/기능이 없지만, 나중에 도입할 때 기존 스레드를
-- 재귀적으로 훑어 backfill할 필요가 없도록 지금부터 nullable 컬럼으로 예약해둔다
-- (thread_id에 project_id를 채우는 단순 UPDATE 한 번으로 끝나도록).
ALTER TABLE tasks ADD COLUMN IF NOT EXISTS thread_id UUID;
ALTER TABLE tasks ADD COLUMN IF NOT EXISTS parent_task_id UUID REFERENCES tasks(id) ON DELETE SET NULL;
-- 예약 컬럼 — 아직 project 테이블이 없으므로 FK 없이 nullable UUID로만 둔다.
-- project 기능 도입 시 `ALTER TABLE tasks ADD CONSTRAINT ... REFERENCES projects(id)`로
-- FK만 추가하면 되고, 기존 행은 project_id가 NULL인 채로 계속 유효하다.
ALTER TABLE tasks ADD COLUMN IF NOT EXISTS project_id UUID;

-- 기존 행은 스레드가 없었으므로 각자 자기 자신이 루트다.
UPDATE tasks SET thread_id = id WHERE thread_id IS NULL;

ALTER TABLE tasks ALTER COLUMN thread_id SET NOT NULL;

CREATE INDEX IF NOT EXISTS idx_tasks_thread_id ON tasks (thread_id, created_at);
CREATE INDEX IF NOT EXISTS idx_tasks_parent_task_id ON tasks (parent_task_id) WHERE parent_task_id IS NOT NULL;
