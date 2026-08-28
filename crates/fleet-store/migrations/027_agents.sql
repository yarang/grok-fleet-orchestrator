-- Agent 엔티티 (로드맵 #49, 1단계).
--
-- `022_projects.sql`가 `tasks.project_id`에 FK를 걸지 않은 이유는 그 컬럼이
-- migration보다 먼저 존재해 검증되지 않은 값이 이미 들어 있을 수 있어서였다.
-- 여기서는 그 조건이 성립하지 않는다 — `agents.project_id`는 이 migration이
-- 처음 만드는 컬럼이므로 배포 DB에 선행 데이터가 존재할 수 없고, 따라서
-- **실제 FK를 건다**. 같은 판단 기준(선행 데이터 유무)이 반대 결론을 낸다.
--
-- `ON DELETE RESTRICT`: Project는 `#48`에서 영구 삭제가 아니라 archive
-- 전이만 하므로 이 FK가 실제로 발동할 경로는 오늘 없다. 그럼에도 CASCADE가
-- 아니라 RESTRICT인 이유는, 나중에 물리 삭제 경로가 생기더라도 Agent가
-- 조용히 함께 사라지지 않고 명시적 회수를 강제하기 위해서다.
--
-- 설계 노트:
--   * `status`는 목표 8-상태(Ready/Starting/Running/WarmIdle/Hibernated/
--     Draining/Stopped/Failed)가 아니라 2-상태(ready/stopped)다 — 나머지
--     여섯은 Worker control stream(#89)과 execution lease(#67 2단계)가
--     있어야 도달한다. `crates/fleet-core/src/agent.rs`의 모듈 문서에 상태별
--     차단 사유를 남겼다.
--   * 이름 유일성은 전역이 아니라 `(project_id, name)`이다. Agent 이름은
--     소속 Project 안에서만 뜻이 통하고, Agent는 Project 사이를 이동할 수
--     없으므로(불변 `project_id`) 이 범위가 영구히 안정적이다.
--   * `agent_template_id`, `runtime`, `isolation`, workspace, egress 정책
--     컬럼은 만들지 않는다. 채울 주체(AgentTemplate `#86`, harness 구성
--     `#51`, isolation `#52`)가 전부 없어 항상 NULL인 컬럼이 된다.
CREATE TABLE agents (
    id           UUID PRIMARY KEY,
    -- 불변. 갱신 경로를 애플리케이션에도 두지 않는다 —
    -- docs/architecture/entity-placement-and-context.md 참고.
    project_id   UUID NOT NULL REFERENCES projects(id) ON DELETE RESTRICT,
    name         TEXT NOT NULL,
    description  TEXT,
    created_by   TEXT,
    status       TEXT NOT NULL DEFAULT 'ready'
                     CHECK (status IN ('ready', 'stopped')),
    created_at   TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at   TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE (project_id, name)
);

-- Project archive 게이트가 "이 Project에 `stopped`가 아닌 Agent가 있는가"를
-- 묻는다(fleet_store::project_rules::advance_project_archive). 목록 화면의
-- Project별 필터도 같은 인덱스를 쓴다.
CREATE INDEX idx_agents_project_status ON agents(project_id, status);
