-- 008_email_auth.sql — 이메일 기반 인증 전환
--
-- username → email을 로그인 식별자로 변경.
-- email_verified 컬럼 추가 + 이메일 인증 토큰 테이블.

-- 1. email_verified 컬럼 추가.
ALTER TABLE users ADD COLUMN IF NOT EXISTS email_verified BOOLEAN NOT NULL DEFAULT false;

-- 2. 기존 사용자의 email이 있으면 verified로 설정 (마이그레이션 호환).
UPDATE users SET email_verified = true WHERE email IS NOT NULL AND email != '';

-- 3. username이 NULL이거나 email에서 파생된 값이면 email prefix로 채우기.
UPDATE users
SET username = split_part(email, '@', 1)
WHERE (username IS NULL OR username = '') AND email IS NOT NULL AND email != '';

-- 4. email UNIQUE 제약 추가 (중복 email이 있으면 실패 — 사전 정리 필요).
-- 기존 데이터에 email이 없는 경우 username을 email로 복사.
UPDATE users SET email = username WHERE email IS NULL OR email = '';

-- 5. email NOT NULL + UNIQUE.
ALTER TABLE users ALTER COLUMN email SET NOT NULL;
ALTER TABLE users ADD CONSTRAINT uq_users_email UNIQUE (email);

-- 6. 이메일 인증 토큰 테이블.
CREATE TABLE IF NOT EXISTS email_verification_tokens (
    id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id     UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    token_hash  TEXT NOT NULL UNIQUE,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    expires_at  TIMESTAMPTZ NOT NULL,
    consumed_at TIMESTAMPTZ
);

CREATE INDEX IF NOT EXISTS idx_email_ver_tokens_user ON email_verification_tokens(user_id);
CREATE INDEX IF NOT EXISTS idx_email_ver_tokens_hash ON email_verification_tokens(token_hash);
