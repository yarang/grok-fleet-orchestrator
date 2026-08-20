-- 019_worker_liveness_mode.sql — 로드맵 #61 1단계.
--
-- Worker별 liveness 보고 방식을 저장한다. 기본값 'periodic'은 기존 배포와
-- 완전히 동일하게 동작한다 (heartbeat_interval_secs 기반 주기적 보고).
-- 'on_demand'는 이 마이그레이션에서 값을 저장/조회할 수 있게만 하며, 실제
-- on-demand dispatch(사전 ACP probe 등)는 별도 control-stream 인프라
-- (로드맵 #67) 없이는 구현하지 않는다 — docs/architecture/worker-liveness-policy.md 참고.
ALTER TABLE workers
    ADD COLUMN liveness_mode TEXT NOT NULL DEFAULT 'periodic'
        CHECK (liveness_mode IN ('periodic', 'on_demand'));
