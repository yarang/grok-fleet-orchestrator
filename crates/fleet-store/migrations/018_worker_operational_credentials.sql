-- Worker operational credential은 bootstrap token과 별개이며 원문을 저장하지 않는다.
CREATE TABLE worker_operational_credentials (
    worker_id UUID PRIMARY KEY REFERENCES workers(id) ON DELETE CASCADE,
    credential_digest TEXT NOT NULL UNIQUE,
    issued_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    expires_at TIMESTAMPTZ,
    revoked_at TIMESTAMPTZ,
    rotation_generation BIGINT NOT NULL DEFAULT 1 CHECK (rotation_generation >= 1)
);

CREATE INDEX idx_worker_operational_credentials_active
    ON worker_operational_credentials(worker_id)
    WHERE revoked_at IS NULL;
