-- 004_rbac.sql — Phase 9.1 RBAC + 세션 기반 인증
--
-- 대시보드 웹 인증을 위한 사용자/역할/권한/세션 테이블.
-- 기존 bearer token 인증(fleet-api)과 병행하며, 대시보드 경로에만 적용.
--
-- 설계:
--   * users — username 기반 로그인, argon2id PHC password_hash
--   * roles / permissions / 매핑 테이블 — RBAC 카탈로그
--   * sessions — 쿠키 토큰(stateful), DB에는 SHA-256 hash만 저장
--   * login_attempts — rate limiting (IP 기반 5회 실패 시 60초 잠금)
--   * bootstrap_tokens 확장 — purpose 컬럼 추가 (worker_join | admin_bootstrap)
--
-- 권한 카탈로그와 builtin 역할 매핑은 SQL이 아닌 Rust `seed_rbac_if_empty()`로
-- 삽입. 이유: Permission enum과 단일 진실 공급원 유지, idempotent.

-- ── users ────────────────────────────────────────────────────────────
CREATE TABLE users (
    id              UUID PRIMARY KEY,
    username        TEXT UNIQUE NOT NULL,
    email           TEXT,
    password_hash   TEXT NOT NULL,            -- argon2id PHC
    enabled         BOOLEAN NOT NULL DEFAULT TRUE,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    last_login_at   TIMESTAMPTZ
);

-- username 검증은 Rust(`User::validate_username`)에서 담당하지만,
-- 최소 길이 constraint를 DB 수준에서도 방어적으로 적용.
ALTER TABLE users ADD CONSTRAINT chk_username_len CHECK (char_length(username) BETWEEN 3 AND 64);

-- 활성 사용자 빠른 조회용.
CREATE INDEX idx_users_enabled ON users(enabled) WHERE enabled = TRUE;

-- ── roles ────────────────────────────────────────────────────────────
CREATE TABLE roles (
    id              UUID PRIMARY KEY,
    name            TEXT UNIQUE NOT NULL,
    description     TEXT,
    builtin         BOOLEAN NOT NULL DEFAULT FALSE,  -- admin/operator/viewer = TRUE
    created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- ── permissions ──────────────────────────────────────────────────────
CREATE TABLE permissions (
    id              UUID PRIMARY KEY,
    name            TEXT UNIQUE NOT NULL,     -- "task:create" 등
    description     TEXT
);

-- ── user_roles ───────────────────────────────────────────────────────
CREATE TABLE user_roles (
    user_id         UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    role_id         UUID NOT NULL REFERENCES roles(id) ON DELETE CASCADE,
    granted_at      TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    granted_by      UUID REFERENCES users(id),
    PRIMARY KEY (user_id, role_id)
);

CREATE INDEX idx_user_roles_user ON user_roles(user_id);
CREATE INDEX idx_user_roles_role ON user_roles(role_id);

-- ── role_permissions ─────────────────────────────────────────────────
CREATE TABLE role_permissions (
    role_id         UUID NOT NULL REFERENCES roles(id) ON DELETE CASCADE,
    permission_id   UUID NOT NULL REFERENCES permissions(id) ON DELETE CASCADE,
    PRIMARY KEY (role_id, permission_id)
);

CREATE INDEX idx_role_permissions_role ON role_permissions(role_id);
CREATE INDEX idx_role_permissions_perm ON role_permissions(permission_id);

-- ── sessions ─────────────────────────────────────────────────────────
-- stateful cookie session. DB에는 SHA-256(token) hex만 저장.
CREATE TABLE sessions (
    id              UUID PRIMARY KEY,
    user_id         UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    token_hash      TEXT NOT NULL,            -- SHA-256 hex (64 chars)
    created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    expires_at      TIMESTAMPTZ NOT NULL,
    ip_address      INET,
    user_agent      TEXT
);

-- 활성 세션 빠른 조회 (토큰 검증 경로).
CREATE INDEX idx_sessions_token_hash ON sessions(token_hash) WHERE expires_at > NOW();
-- 사용자별 세션 목록/폐기.
CREATE INDEX idx_sessions_user_expires ON sessions(user_id, expires_at);

-- ── login_attempts ───────────────────────────────────────────────────
-- rate limiting + 감사. (identifier, ip) 기준 5회 실패 시 60초 잠금.
CREATE TABLE login_attempts (
    id              UUID PRIMARY KEY,
    identifier      TEXT NOT NULL,            -- username (또는 IP, user 미확인 시)
    ip_address      TEXT,
    success         BOOLEAN NOT NULL,
    failure_reason  TEXT,
    attempted_at    TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- 최근 실패 카운트용 (잠금 판정 쿼리에서 사용).
CREATE INDEX idx_login_attempts_id_time ON login_attempts(identifier, attempted_at DESC);
CREATE INDEX idx_login_attempts_ip_time ON login_attempts(ip_address, attempted_at DESC)
    WHERE success = FALSE;

-- ── bootstrap_tokens 확장 ────────────────────────────────────────────
-- 기존 worker_join 토큰 외에 admin 등록용 admin_bootstrap 토큰 추가.
-- 기존 행은 DEFAULT 'worker_join'으로 채워짐 (하위 호환성).
ALTER TABLE bootstrap_tokens
    ADD COLUMN IF NOT EXISTS purpose TEXT NOT NULL DEFAULT 'worker_join'
    CHECK (purpose IN ('worker_join', 'admin_bootstrap'));

CREATE INDEX idx_bootstrap_tokens_purpose ON bootstrap_tokens(purpose)
    WHERE purpose = 'admin_bootstrap';
