-- SSH 키 금고: 대시보드 프로비저닝용 암호화된 SSH 비밀키 저장.
-- encrypted_blob: AES-256-GCM(nonce(12B) || ciphertext || tag(16B)) base64url.
-- fingerprint: 공개키 fingerprint (SHA-256 base64), 표시용.
-- key_type: "ed25519" | "rsa" | "ecdsa"

CREATE TABLE IF NOT EXISTS ssh_keys (
    id              UUID PRIMARY KEY,
    name            TEXT UNIQUE NOT NULL,
    encrypted_blob  TEXT NOT NULL,
    fingerprint     TEXT NOT NULL,
    key_type        TEXT NOT NULL,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
