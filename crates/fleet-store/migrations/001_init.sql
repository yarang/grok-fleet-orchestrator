-- 001_init.sql — 초기 스키마
-- fleet-core 도메인 모델과 1:1 대응.
--
-- 설계 원칙:
--   * status / labels / result 등 가변 데이터는 JSONB (유연성 + 인덱싱)
--   * status_phase는 JSONB에서 추출한 생성 칼럼 (빠른 필터링)
--   * events는 append-only (다중 admin 동기화 + 감사 로그)
--   * task_outputs는 스트리밍용 (장기 실행 작업의 stdout 청크)

-- ── workers ────────────────────────────────────────────────────────

CREATE TABLE workers (
    id              UUID PRIMARY KEY,
    name            TEXT UNIQUE NOT NULL,
    endpoint        TEXT NOT NULL,
    labels          JSONB NOT NULL DEFAULT '{}'::jsonb,
    status          TEXT NOT NULL DEFAULT 'offline',
    circuit_state   TEXT NOT NULL DEFAULT 'closed',
    last_seen       TIMESTAMPTZ,
    active_tasks    INTEGER NOT NULL DEFAULT 0,
    max_concurrent  INTEGER NOT NULL DEFAULT 4,
    worker_version  TEXT,
    registered_at   TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- ── tasks ──────────────────────────────────────────────────────────

CREATE TABLE tasks (
    id              UUID PRIMARY KEY,
    prompt          TEXT NOT NULL,
    cwd             TEXT,
    model           TEXT,
    server_hint     TEXT,
    required_labels JSONB NOT NULL DEFAULT '[]'::jsonb,
    max_turns       INTEGER,
    timeout_secs    BIGINT,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    created_by      TEXT NOT NULL,
    priority        TEXT NOT NULL DEFAULT 'normal',
    -- TaskStatus enum을 JSONB로 통째로 저장 (phase, worker_id, started_at 등 포함)
    status          JSONB NOT NULL,
    -- status JSONB의 'phase' 필드를 추출한 생성 칼럼 (인덱싱용)
    status_phase    TEXT GENERATED ALWAYS AS (status->>'phase') STORED
);

-- ── task_outputs (스트리밍 stdout 청크) ─────────────────────────────

CREATE TABLE task_outputs (
    task_id     UUID NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
    -- 글로벌 시퀀스 (BIGSERIAL). per-task 청크 순서는 (task_id, seq)로 보장.
    seq         BIGSERIAL,
    chunk       TEXT NOT NULL,
    written_at  TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (task_id, seq)
);

-- ── events (append-only 이벤트 로그) ────────────────────────────────

CREATE TABLE events (
    seq         BIGSERIAL PRIMARY KEY,
    task_id     UUID REFERENCES tasks(id) ON DELETE SET NULL,
    worker_id   UUID REFERENCES workers(id) ON DELETE SET NULL,
    event_type  TEXT NOT NULL,
    -- FleetEvent enum을 JSONB로 통째로 저장
    payload     JSONB NOT NULL,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- ── LISTEN/NOTIFY: 다중 admin 실시간 동기화 ──────────────────────────
-- 이벤트 INSERT 시 모든 리스너에게 seq 번호 통지.

CREATE OR REPLACE FUNCTION notify_fleet_event() RETURNS TRIGGER AS $$
BEGIN
    PERFORM pg_notify('fleet_events', NEW.seq::text);
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER events_notify AFTER INSERT ON events
    FOR EACH ROW EXECUTE FUNCTION notify_fleet_event();
