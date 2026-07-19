-- 003_bootstrap_tokens.sql — Phase 8.3 부트스트랩 토큰 저장소
--
-- 목적: fleet-worker join 흐름에서 사용되는 일회성 (또는 제한적 다회용)
-- 부트스트랩 토큰을 영속화. 발급/추적/회수가 가능하며, 사용 시 atomic하게
-- use_count를 증가시켜 race condition을 방지.
--
-- 설계:
--   * token TEXT PK — 호출자가 생성한 난수 (base64url, prefix 포함 가능)
--   * max_uses / use_count — 다회용 토큰 지원 (기본 1)
--   * expires_at — 선택적 만료 (NULL = 무기한)
--   * last_used_by / last_used_at — 감사 추적용

CREATE TABLE bootstrap_tokens (
    token           TEXT PRIMARY KEY,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    created_by      TEXT,
    expires_at      TIMESTAMPTZ,
    max_uses        INTEGER NOT NULL DEFAULT 1 CHECK (max_uses >= 1),
    use_count       INTEGER NOT NULL DEFAULT 0,
    notes           TEXT,
    last_used_by    TEXT,
    last_used_at    TIMESTAMPTZ
);

-- 만료된 토큰 조회용 부분 인덱스.
CREATE INDEX idx_bootstrap_tokens_expires ON bootstrap_tokens(expires_at)
    WHERE expires_at IS NOT NULL;

-- 사용 통계 조회용.
CREATE INDEX idx_bootstrap_tokens_created ON bootstrap_tokens(created_at DESC);
