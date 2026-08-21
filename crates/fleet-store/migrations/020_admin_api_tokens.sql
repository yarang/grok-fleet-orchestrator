-- Admin API bearer token을 DB에 영속화 (로드맵 #72).
--
-- 기존 FLEET_API_TOKENS env var는 부팅 시 1회만 파싱되는 정적 목록이라
-- 회전에 파일 편집 + 서비스 재기동이 필요했고, 값이 세션 로그 등에 노출되면
-- 재발급 전까지 무효화할 방법이 없었다. `018_worker_operational_credentials.sql`
-- (로드맵 #60)와 동일한 패턴을 admin bearer 토큰에 적용한다 — 원문은 저장하지
-- 않고 SHA-256 digest만 보관하며, 생성/rotate 응답에만 원문을 1회 노출한다.
--
-- principal_id를 PK로 사용해 "1 principal = 1 활성 토큰"을 스키마 레벨에서
-- 강제한다 — worker_operational_credentials가 worker_id를 PK로 쓴 것과 동일한
-- 이유(같은 principal이 여러 활성 토큰을 가질 수 있는지는 이번 설계 범위 밖).
CREATE TABLE admin_api_tokens (
    principal_id         TEXT PRIMARY KEY,
    token_digest         TEXT NOT NULL UNIQUE,
    -- PermissionKind::as_str() 값의 JSON 배열 (예: ["worker:list", "worker:register"]).
    capabilities         JSONB NOT NULL,
    created_at           TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    rotated_at           TIMESTAMPTZ,
    revoked_at           TIMESTAMPTZ,
    rotation_generation  BIGINT NOT NULL DEFAULT 1 CHECK (rotation_generation >= 1)
);

-- 인증 경로(digest 조회)와 자동 가져오기(principal 존재 확인) 모두
-- revoked_at IS NULL 필터를 타므로 부분 인덱스로 최적화.
CREATE INDEX idx_admin_api_tokens_active
    ON admin_api_tokens(principal_id)
    WHERE revoked_at IS NULL;
