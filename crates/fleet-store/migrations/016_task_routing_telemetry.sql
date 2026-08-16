-- 016_task_routing_telemetry.sql: 지능형 태스크 라우팅, 실시간 예산 통제 및 텔레메트리 스키마

-- 1. tasks 테이블 컬럼 확장
ALTER TABLE tasks
    ADD COLUMN IF NOT EXISTS requested_profile TEXT,
    ADD COLUMN IF NOT EXISTS resolved_model TEXT,
    ADD COLUMN IF NOT EXISTS token_budget BIGINT,
    ADD COLUMN IF NOT EXISTS partial_output TEXT;

-- 2. task_telemetry 테이블 신설 (결정론적 평가 메타데이터 적재)
CREATE TABLE IF NOT EXISTS task_telemetry (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    task_id UUID NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
    routing_profile TEXT NOT NULL,
    resolved_model TEXT NOT NULL,
    runtime_vendor TEXT NOT NULL DEFAULT 'grok', -- 'grok' | 'agy' | 'gemini'
    
    -- 결정론적 지표 (토큰 비용 0 평가)
    exit_code INT,
    tool_error_count INT DEFAULT 0,
    duration_secs DOUBLE PRECISION NOT NULL DEFAULT 0.0,
    input_tokens BIGINT DEFAULT 0,
    output_tokens BIGINT DEFAULT 0,
    total_tokens BIGINT DEFAULT 0,
    estimated_cost_usd NUMERIC(10, 6) DEFAULT 0,
    
    -- 압축 및 제어 통계
    compact_count INT DEFAULT 0,
    tokens_saved_by_compact BIGINT DEFAULT 0,
    budget_exhausted_pct NUMERIC(5, 2),
    is_grace_wrapped BOOLEAN DEFAULT FALSE,
    has_user_retry BOOLEAN DEFAULT FALSE,
    
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_task_telemetry_profile_model ON task_telemetry(routing_profile, resolved_model);
CREATE INDEX IF NOT EXISTS idx_task_telemetry_task_id ON task_telemetry(task_id);
