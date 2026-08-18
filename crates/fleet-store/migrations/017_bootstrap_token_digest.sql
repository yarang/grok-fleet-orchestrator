-- Bootstrap token 원문을 저장하지 않는다.
-- 기존 토큰은 원문을 SHA-256 digest로 원자적으로 치환하므로, 업그레이드 후에도
-- 이미 배포된 worker join token은 계속 사용할 수 있다.
CREATE EXTENSION IF NOT EXISTS pgcrypto;

ALTER TABLE bootstrap_tokens RENAME COLUMN token TO token_digest;

UPDATE bootstrap_tokens
   SET token_digest = encode(digest(token_digest, 'sha256'), 'hex');
