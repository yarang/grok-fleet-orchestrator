-- 004_worker_credentials.sql — Phase 8.6 워커 API 키 중앙 관리
--
-- 목적: 각 워커의 grok API 키(model config 섹션 전체)를 암호화하여 저장.
-- 프로비저닝/회전 시 fleet-credentials 크레이트로 복호화 후 워커에 배포.
--
-- 설계:
--   * worker_name + model_id 복합 PK — 한 워커가 여러 모델을 사용 가능.
--   * encrypted_blob TEXT — base64(nonce || ciphertext || tag). AES-256-GCM.
--   * base_url / api_backend / context_window / model_name — 암호화 대상 아님.
--     (이들은 비밀이 아니고, 프로비저닝 실패 시 디버깅을 위해 평문 필요)
--   * rotated_at — 마지막 회전 일시 (감사 로그).
--
-- 마스터 키는 환경변수 FLEET_MASTER_KEY 또는 /etc/fleet/master.key에서 로드.
-- 키를 잃어버리면 모든 blob 복호화 불가 → 백업 필수.

CREATE TABLE worker_credentials (
    worker_name     TEXT NOT NULL REFERENCES workers(name) ON DELETE CASCADE,
    model_id        TEXT NOT NULL,
    -- base64url(nonce(12B) || ciphertext || tag(16B)). api_key만 암호화.
    encrypted_blob  TEXT NOT NULL,
    -- 아래는 비밀이 아닌 메타데이터 (평문 저장, 디버깅/프로비저닝용).
    base_url        TEXT NOT NULL,
    api_backend     TEXT NOT NULL DEFAULT 'chat_completions',
    context_window  INTEGER NOT NULL DEFAULT 200000,
    model_name      TEXT,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    rotated_at      TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (worker_name, model_id)
);

-- 워커별 조회 인덱스.
CREATE INDEX idx_worker_credentials_worker ON worker_credentials(worker_name);
