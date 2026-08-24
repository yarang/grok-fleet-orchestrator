-- Issue 추적 (로드맵 #88).
--
-- `docs/architecture/issues.md`가 정본이다. Issue는 Project가 해결해야 할
-- 일감이며 orchestrator의 인프라 장애 추적이 아니다.
--
-- 설계 노트:
--   * **`tasks`에 `issue_id` 컬럼을 추가하지 않는다** (불변식 I1). 넣는
--     순간 Task 상태 머신이 Issue를 읽어야 하는 압력이 생기고, 두 상태
--     머신이 경쟁한다. 연관은 아래 `issue_task_links` join 테이블이
--     소유한다.
--   * `status`에 `in_progress`가 없다. "진행 중"은 비터미널 연관 Task가
--     있다는 사실에서 유도 가능하며, 상태로 승격하면 Task 상태를 복제하게
--     된다. CHECK 제약이 이 부재를 스키마 레벨에서 고정한다.
--   * `close_reason`은 `status='closed'`일 때만, 그리고 그때는 반드시
--     있어야 한다 — CHECK 제약으로 양방향 강제한다. 애플리케이션의
--     `Issue::transition_to`와 같은 규칙을 DB에도 둬, 다른 경로로 들어온
--     쓰기도 이 불변식을 깨지 못하게 한다.
--   * FK 정책(`docs/architecture/issues.md`): `issue_comments`만 CASCADE를
--     쓴다 — 폭발 반경이 정확히 한 스레드이고 mutation 사실 자체는 audit에
--     독립적으로 남는다. `issue_task_links.task_id`는 `SET NULL` +
--     `task_label` 보존이다(`011_audit_log.sql`의 `actor_label`과 같은
--     패턴 — 어떤 Task와 엮여 있었는지는 Issue 이력의 일부다).
--     `issues.project_id`는 CASCADE를 쓰지 않는다(`#78`의 교훈) — Project
--     행 삭제는 archive 보존 기간을 통과한 별도 관리 작업이며, 그때
--     Issue를 어떻게 할지는 retention 정책이 정한다(`#91`).
CREATE TABLE issues (
    id           UUID PRIMARY KEY,
    project_id   UUID NOT NULL REFERENCES projects(id),
    title        TEXT NOT NULL,
    body         TEXT NOT NULL DEFAULT '',
    status       TEXT NOT NULL DEFAULT 'open'
                     CHECK (status IN ('open', 'triaged', 'ready_for_agent', 'resolved', 'closed')),
    close_reason TEXT
                     CHECK (close_reason IS NULL
                            OR close_reason IN ('fixed', 'wont_fix', 'duplicate', 'obsolete')),
    severity     TEXT NOT NULL DEFAULT 'medium'
                     CHECK (severity IN ('critical', 'high', 'medium', 'low')),
    -- 라벨 문자열 배열 (JSONB — labels 검색은 아직 요구사항이 아니라
    -- 별도 테이블로 정규화하지 않는다).
    labels       JSONB NOT NULL DEFAULT '[]'::jsonb,
    assignee     TEXT,
    created_by   TEXT NOT NULL,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at   TIMESTAMPTZ NOT NULL DEFAULT NOW(),

    -- closed면 사유 필수, closed가 아니면 사유 금지.
    CONSTRAINT issues_close_reason_matches_status CHECK (
        (status = 'closed' AND close_reason IS NOT NULL)
        OR (status <> 'closed' AND close_reason IS NULL)
    )
);

-- Project 범위 조회가 기본 접근 패턴(`issue:read`는 Project 범위 조회다).
CREATE INDEX idx_issues_project_status ON issues(project_id, status, created_at DESC);
-- "열린 Issue" 목록 — `#89`의 dedup 부분 유니크 인덱스도 같은 술어를 쓴다.
CREATE INDEX idx_issues_open ON issues(project_id, created_at DESC)
    WHERE status <> 'closed';

-- Issue 스레드의 코멘트 (append-only).
CREATE TABLE issue_comments (
    id         UUID PRIMARY KEY,
    issue_id   UUID NOT NULL REFERENCES issues(id) ON DELETE CASCADE,
    author     TEXT NOT NULL,
    body       TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_issue_comments_issue ON issue_comments(issue_id, created_at);

-- Issue ↔ Task 연관. `tasks`에 `issue_id`를 두지 않는 이유는 위 I1 참고.
CREATE TABLE issue_task_links (
    issue_id   UUID NOT NULL REFERENCES issues(id) ON DELETE CASCADE,
    -- Task가 삭제되면 NULL이 되고 task_label이 남는다.
    task_id    UUID REFERENCES tasks(id) ON DELETE SET NULL,
    task_label TEXT NOT NULL,
    linked_by  TEXT NOT NULL,
    linked_at  TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- 같은 Task를 같은 Issue에 두 번 연결하지 않는다. task_id가 NULL이 된
-- (삭제된 Task) 행은 이 유니크의 대상이 아니다 — Postgres에서 NULL은
-- 서로 같지 않으므로 자연히 제외된다.
CREATE UNIQUE INDEX idx_issue_task_links_unique ON issue_task_links(issue_id, task_id);
CREATE INDEX idx_issue_task_links_task ON issue_task_links(task_id);
