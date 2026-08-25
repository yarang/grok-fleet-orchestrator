-- 로드맵 #62 2단계 — 클라이언트 제출 멱등성 (실행 일관성 검증 게이트 3번).
--
-- 설계 정본: docs/architecture/tasks/execution-consistency.md
--   "MCP와 HTTP task submit은 idempotency_key와 payload hash를 받는다.
--    동일 principal, 동일 key, 동일 hash의 재요청은 기존 Task를 반환한다.
--    같은 key에 다른 payload가 오면 409 Conflict로 거부한다."
--
-- 유일성 스코프를 `created_by`로 잡은 이유:
-- 정본이 말하는 "principal"에 해당하는 값이 오늘 코드에 존재하지 않는다.
-- MCP stdio 서버(crates/fleet-mcp/src/server.rs)는 FLEET_MCP_CAPABILITIES로
-- capability 집합만 받을 뿐 호출자 신원을 받지 않고, handle_dispatch_task는
-- created_by를 'mcp' 리터럴로 채운다. 지금 principal 컬럼을 만들면 아무도
-- 채우지 않는 열이 된다. 그래서 실제로 채워지는 created_by를 스코프로 쓴다.
-- 한계는 로드맵 #62 행에 표로 남겼다: MCP 제출은 전부 'mcp' 버킷을 공유하므로
-- 키 네임스페이스가 MCP 클라이언트 단위가 아니라 오케스트레이터 단위다.
ALTER TABLE tasks
    ADD COLUMN idempotency_key TEXT,
    ADD COLUMN idempotency_payload_hash TEXT;

-- 부분 인덱스인 이유: 키 없는 제출(대다수)은 유일성 대상이 아니다. Postgres는
-- NULL을 서로 다른 값으로 보므로 전체 UNIQUE로도 동작은 하지만, 부분 인덱스가
-- 의도를 드러내고 인덱스 크기도 실제 사용량에 비례한다.
CREATE UNIQUE INDEX idx_tasks_idempotency
    ON tasks (created_by, idempotency_key)
    WHERE idempotency_key IS NOT NULL;

-- 키와 해시는 항상 함께 있거나 함께 없다. 해시만 있는 행은 비교 상대가 없고,
-- 키만 있는 행은 재요청이 왔을 때 같은 요청인지 판정할 수 없다 — 둘 다
-- 도달 불가능한 상태여야 하므로 DB가 직접 막는다.
ALTER TABLE tasks
    ADD CONSTRAINT tasks_idempotency_pair_complete
    CHECK ((idempotency_key IS NULL) = (idempotency_payload_hash IS NULL));
