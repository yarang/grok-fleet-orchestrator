-- Project 엔티티 (로드맵 #48, 1단계).
--
-- `tasks.project_id`(001_init.sql)는 이미 존재해 Task가 선택적으로 Project를
-- 참조할 수 있었지만, 참조 대상인 `projects` 테이블 자체가 없어 그 컬럼은
-- 지금까지 순수 미검증 메타데이터였다(어떤 스케줄러 코드도 읽지 않는다 —
-- `docs/architecture/project-feature-design.md` "구현 상태는 부분 구현").
-- 이 migration이 처음으로 실제 Project 엔티티를 만든다.
--
-- 설계 노트:
--   * `status`는 목표 계약의 5-상태(Draft/Active/Draining/ArchiveBlocked/
--     Archived)가 아니라 3-상태(active/draining/archived)다 — 나머지 둘은
--     Agent/AgentTemplate/effect ledger(전부 미구현)가 있어야 의미가 생긴다.
--     `crates/fleet-core/src/project.rs`의 모듈 문서에 이유를 자세히 남겼다.
--   * `tasks.project_id`에 FK를 걸지 않는다. 이 컬럼은 이 migration 이전부터
--     존재했고 지금까지 어떤 검증도 거치지 않았으므로, 실제 배포 DB에
--     `projects` 테이블에 없는 값이 이미 저장돼 있을 가능성을 배제할 수
--     없다 — FK를 강제하면 그런 배포에서 이 migration 자체가 실패한다.
--     참조 무결성은 애플리케이션 계층(Task 제출 시 project_id 존재·상태
--     확인)에서 담당한다.
CREATE TABLE projects (
    id           UUID PRIMARY KEY,
    name         TEXT NOT NULL UNIQUE,
    description  TEXT,
    created_by   TEXT,
    status       TEXT NOT NULL DEFAULT 'active'
                     CHECK (status IN ('active', 'draining', 'archived')),
    created_at   TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at   TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- 목록 화면의 기본 필터(archived 숨기기 등)를 뒷받침.
CREATE INDEX idx_projects_status ON projects(status);
