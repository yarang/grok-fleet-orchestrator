-- 011_audit_log.sql — 구조화된 감사 로그
--
-- 지금까지 인증/권한 이벤트는 `tracing::info!`로만 남아서, 로그 수집기가
-- 없으면 사후 추적이 불가능하고 질의도 할 수 없었다. 이 테이블은 "누가,
-- 무엇을, 언제, 어디서, 성공/실패" 를 질의 가능한 형태로 보존한다.
--
-- 설계 노트:
--   * actor_user_id는 ON DELETE SET NULL — 사용자를 지워도 감사 기록은
--     남아야 한다. 감사 로그가 FK CASCADE로 함께 삭제되면 "계정을 지워
--     흔적을 지우는" 행위를 막을 수 없다.
--   * actor_label은 그래서 별도로 보존한다. 사용자 삭제 후에도 누구였는지
--     알 수 있어야 하고, 로그인 실패처럼 아직 사용자가 특정되지 않은
--     이벤트는 입력된 이메일/IP를 그대로 남긴다.
--   * detail은 JSONB 자유 형식 — 액션마다 필요한 맥락이 다르다.
--     **비밀번호/토큰 원문은 절대 넣지 않는다.**

CREATE TABLE IF NOT EXISTS audit_log (
    id              UUID PRIMARY KEY,
    -- 행위자. 미인증 이벤트(로그인 실패 등)는 NULL.
    actor_user_id   UUID REFERENCES users(id) ON DELETE SET NULL,
    -- 행위자 표시용 문자열 (username / email / "system"). 사용자 삭제 후에도 보존.
    actor_label     TEXT NOT NULL,
    -- "auth.login", "user.create" 등 점 표기 액션명.
    action          TEXT NOT NULL,
    -- 대상 종류/식별자 (선택). 예: ("user", "<uuid>"), ("worker", "build-1").
    target_type     TEXT,
    target_id       TEXT,
    outcome         TEXT NOT NULL CHECK (outcome IN ('success', 'failure')),
    ip_address      TEXT,
    detail          JSONB NOT NULL DEFAULT '{}'::jsonb,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- 최신순 조회 (감사 화면 기본 정렬).
CREATE INDEX IF NOT EXISTS idx_audit_log_created ON audit_log(created_at DESC);
-- 특정 사용자의 활동 추적.
CREATE INDEX IF NOT EXISTS idx_audit_log_actor ON audit_log(actor_user_id, created_at DESC);
-- 액션별 필터 (예: 로그인 실패만 모아 보기).
CREATE INDEX IF NOT EXISTS idx_audit_log_action ON audit_log(action, created_at DESC);
