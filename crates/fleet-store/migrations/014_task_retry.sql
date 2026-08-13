-- 로드맵 #38 — dispatch 재시도 상한(max_dispatch_retries) 판단을 위한 카운터.
-- submit()의 최초 시도 또는 Reconciler의 stale-Pending 재시도가 WorkerUnavailable/
-- CircuitOpen으로 실패할 때마다 1씩 증가한다. 이 값이 max_dispatch_retries에
-- 도달하면 Reconciler가 더 이상 재시도하지 않고 Failed(dead-letter)로 전이시킨다.

ALTER TABLE tasks
    ADD COLUMN retry_count INTEGER NOT NULL DEFAULT 0;
