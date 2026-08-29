-- AgentTemplate 정본과 revision immutability (로드맵 #86, 1단계).
--
-- 정본: docs/architecture/agents/agent-template.md
--
-- 두 테이블로 나눈 이유는 참조가 3-tuple
-- `(template_id, content_revision, content_hash)`이기 때문이다. 정체성과 본문을
-- 한 테이블에 합치면 "본문을 고치면 새 revision"을 표현할 수 없다 — 상태 전이
-- 한 번마다 본문이 복제되거나, 본문 UPDATE가 과거 revision을 덮어써 `#65`의
-- 재현성이 깨진다.
--
-- 이 마이그레이션이 만들지 않는 것과 그 이유:
--   * `isolation_class`      — 등급을 표현할 타입이 코드에 없다(`#52`). 검증할
--                              집합 없는 자유 문자열은 값이 아무 뜻도 갖지 않는다.
--   * `projects.default_agent_template_id`
--                            — 쓰는 주체도 읽는 주체도 없다(`#49` 2단계).
--   * `builtin/default@1` 행 — 정본이 요구하는 "tool binding이 `ReadOnly` 등급
--                              한정"을 단정할 타입이 없다. 본문 상수와 그
--                              content_hash는 fleet-core에 두고 시드 삽입만 미룬다.

CREATE TABLE agent_templates (
    id           UUID PRIMARY KEY,
    -- NULL이면 전역 템플릿. FK를 RESTRICT로 두는 것은 `027_agents.sql`과 같은
    -- 이유다 — Project 삭제가 템플릿을 조용히 함께 지우면, 그 템플릿을 pin한
    -- 다른 Project의 Agent가 참조를 잃는다. 이 도메인에 CASCADE를 쓰지 않는
    -- 결정은 `#78`에서 worker 삭제 하나가 CASCADE 둘을 타고 암호화된 LLM
    -- 자격증명을 파괴한 사건 이후의 규칙이다.
    project_id   UUID REFERENCES projects(id) ON DELETE RESTRICT,
    name         TEXT NOT NULL,
    description  TEXT,
    created_by   TEXT,
    status       TEXT NOT NULL DEFAULT 'draft'
                     CHECK (status IN ('draft', 'published', 'deprecated',
                                       'retired', 'discarded')),
    created_at   TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at   TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- 이름 유일성에 UNIQUE 제약을 쓸 수 없다. Postgres의 UNIQUE는 NULL을 서로
-- **다른** 값으로 보므로 `UNIQUE (project_id, name)`은 전역 템플릿
-- (`project_id IS NULL`) 두 개가 같은 이름을 갖는 것을 막지 못한다. 부분 유니크
-- 인덱스 두 장으로 나눠야 두 범위 모두에서 유일성이 성립한다.
CREATE UNIQUE INDEX idx_agent_templates_project_name
    ON agent_templates(project_id, name)
    WHERE project_id IS NOT NULL;
CREATE UNIQUE INDEX idx_agent_templates_global_name
    ON agent_templates(name)
    WHERE project_id IS NULL;

CREATE INDEX idx_agent_templates_project_status
    ON agent_templates(project_id, status);

-- 불변 본문. UPDATE 경로는 애플리케이션이 제공하지 않는다 — Store 트레이트에
-- 본문 컬럼을 고치는 메서드가 아예 없는 것이 immutability의 집행 방법이다.
-- `revoked_at`만 예외이며, 그것은 본문이 아니라 "새 pin을 받는가"를 바꾼다.
CREATE TABLE agent_template_revisions (
    id               UUID PRIMARY KEY,
    template_id      UUID NOT NULL REFERENCES agent_templates(id) ON DELETE RESTRICT,
    -- 템플릿 안에서 1부터 증가. Store가 트랜잭션 안에서 할당한다.
    content_revision INTEGER NOT NULL,
    -- fleet-core `AgentTemplateBody::content_hash`의 값(sha256 hex, 64자).
    -- 저장된 본문에서 재계산해 대조할 수 있어야 하므로, 저장되는 body는 반드시
    -- 정규화(정렬·중복 제거)된 형태여야 한다.
    content_hash     TEXT NOT NULL CHECK (length(content_hash) = 64),
    role_prompt      TEXT NOT NULL,
    -- 정규화된 집합. TEXT[]로 두어 배열 원소 검색이 가능하게 한다.
    tools            TEXT[] NOT NULL DEFAULT '{}',
    skills           TEXT[] NOT NULL DEFAULT '{}',
    revoked_at       TIMESTAMPTZ,
    created_by       TEXT,
    created_at       TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE (template_id, content_revision)
);

-- content_hash에 유니크 제약을 **걸지 않는다.** 같은 내용을 다시 publish하면
-- 새 revision id에 같은 content_hash가 나오는 것이 정본의 요구사항이며, 그것이
-- "hash는 본문의 동일성을 말하고 revision은 발행 사건을 말한다"는 두 계층의
-- 구분이다. 조회용 인덱스만 둔다.
CREATE INDEX idx_agent_template_revisions_hash
    ON agent_template_revisions(template_id, content_hash);

-- `027_agents.sql`은 이 두 컬럼을 만들지 않으면서 그 이유를 "채울 주체
-- (AgentTemplate `#86`)가 없어 항상 NULL인 컬럼이 된다"로 적었다. 그 전제가
-- 이 마이그레이션에서 해소된다. 부수 효과가 아니라 본질이다 — pin이 있어야
-- retire의 의존 집합이 공집합이 아니게 되고, revision revoke가 비로소 막을
-- 대상을 갖는다.
--
-- 둘 다 NULL 허용이다. 기존 Agent 행에 채울 값이 없고, 템플릿 없이 만든
-- Agent도 계속 유효하기 때문이다.
ALTER TABLE agents
    ADD COLUMN agent_template_id UUID
        REFERENCES agent_templates(id) ON DELETE RESTRICT,
    ADD COLUMN agent_template_revision_id UUID
        REFERENCES agent_template_revisions(id) ON DELETE RESTRICT,
    -- 3-tuple 참조의 절반만 남는 상태를 스키마가 막는다. revision만 있고
    -- template이 없으면 어느 정체성의 본문인지 조회할 수 없고, template만 있고
    -- revision이 없으면 "어떤 본문으로 만들어졌나"에 답할 수 없다.
    ADD CONSTRAINT agents_template_pin_complete CHECK (
        (agent_template_id IS NULL AND agent_template_revision_id IS NULL)
        OR (agent_template_id IS NOT NULL AND agent_template_revision_id IS NOT NULL)
    );

-- retire의 의존 집합 조회(`WHERE agent_template_id = $1`)가 이 인덱스를 쓴다.
-- 그 조회는 확인 화면과 트랜잭션 안에서 각각 한 번씩, 즉 retire 한 번에 두 번
-- 돈다.
CREATE INDEX idx_agents_agent_template_id
    ON agents(agent_template_id)
    WHERE agent_template_id IS NOT NULL;
