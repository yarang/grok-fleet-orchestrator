-- 012_task_dispatch_latency.sql — 태스크 디스패치 지연 시간 수집용 스키마 갱신
--
-- 작업이 큐에 머무른 지연 시간을 정밀 측정하기 위해 dispatched_at 컬럼을 추가합니다.
ALTER TABLE tasks ADD COLUMN IF NOT EXISTS dispatched_at TIMESTAMPTZ;
