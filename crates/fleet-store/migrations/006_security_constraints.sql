-- 006_security_constraints.sql — Phase 9.1.7 보안 패치
--
-- M5: sessions.token_hash UNIQUE — 동일한 토큰 해시로 두 세션이 생성되는 것을 방지.
--     세션 고정(session fixation) 및 토큰 충돌 공격 차단.
-- M6: user_roles.granted_by ON DELETE SET NULL — 관리자 계정 삭제 시
--     역할 부여 이력을 보존(NULL로 전환). 기존 NO ACTION은 삭제를 차단했음.

-- ── M5: token_hash UNIQUE ────────────────────────────────────────────
-- 부분 인덱스(idx_sessions_token_hash)가 이미 존재하지만, UNIQUE는 전체 행에 적용.
-- 만료된 세션의 token_hash가 재사용되는 것도 방지.
CREATE UNIQUE INDEX IF NOT EXISTS uq_sessions_token_hash
    ON sessions(token_hash);

-- ── M6: granted_by ON DELETE SET NULL ────────────────────────────────
-- 기존 FK 제약을 NO ACTION(기본값)에서 SET NULL로 교체.
-- 관리자가 삭제되어도 "누가 부여했는지" 기록 자체는 보존(granted_by만 NULL).
ALTER TABLE user_roles DROP CONSTRAINT IF EXISTS user_roles_granted_by_fkey;
ALTER TABLE user_roles ADD CONSTRAINT user_roles_granted_by_fkey
    FOREIGN KEY (granted_by) REFERENCES users(id) ON DELETE SET NULL;
