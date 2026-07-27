-- 007_hosts.sql — 호스트 인벤토리 (Phase P1.5)
--
-- workers 테이블은 "현재 등록된 워커"만 추적한다.
-- 이 마이그레이션은 등록 여부와 무관하게 인프라에 존재하는 모든 호스트를
-- 추적하기 위한 hosts + host_events 테이블을 도입한다.
--
-- 설계:
--   * hosts — 물리/가상 호스트 1건. hostname UNIQUE. workers 테이블과 1:1 관계(optional FK).
--   * host_events — 호스트 단위 타임라인 (프로비저닝/하트비트/장애/복구).
--   * heartbeat로 수집된 grok_version, os_info 등을 hosts에 upsert.

-- ── hosts ───────────────────────────────────────────────────────────
CREATE TABLE hosts (
    id                  UUID PRIMARY KEY,
    hostname            TEXT UNIQUE NOT NULL,
    -- 연결된 워커 (등록된 경우). 미등록 호스트는 NULL.
    worker_id           UUID REFERENCES workers(id) ON DELETE SET NULL,
    -- 호스트 상태: provisioned | online | offline | failed
    status              TEXT NOT NULL DEFAULT 'provisioned',
    -- SSH 접속 정보 (프로비저닝 시 기록)
    ssh_host            TEXT,
    ssh_port            INTEGER NOT NULL DEFAULT 22,
    ssh_user            TEXT,
    -- 런타임 정보 (heartbeat로 갱신)
    grok_version        TEXT,
    fleet_worker_version TEXT,
    os_info             JSONB NOT NULL DEFAULT '{}'::jsonb,
    -- 시스템 메트릭 스냅샷 (heartbeat로 갱신)
    load_avg            JSONB,
    mem_available_mb    BIGINT,
    disk_free_mb        BIGINT,
    -- 타임스탬프
    last_heartbeat_at   TIMESTAMPTZ,
    provisioned_at      TIMESTAMPTZ,
    created_at          TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at          TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- 상태별 조회용 인덱스.
CREATE INDEX idx_hosts_status ON hosts(status);
-- worker_id 역참조 (워커 → 호스트).
CREATE INDEX idx_hosts_worker_id ON hosts(worker_id) WHERE worker_id IS NOT NULL;

-- updated_at 자동 갱신 트리거.
CREATE OR REPLACE FUNCTION update_hosts_updated_at() RETURNS TRIGGER AS $$
BEGIN
    NEW.updated_at = NOW();
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER hosts_updated_at BEFORE UPDATE ON hosts
    FOR EACH ROW EXECUTE FUNCTION update_hosts_updated_at();

-- ── host_events ─────────────────────────────────────────────────────
-- 호스트 단위 타임라인. 프로비저닝 단계, heartbeat 이상, 장애/복구 등.
CREATE TABLE host_events (
    id          UUID PRIMARY KEY,
    host_id     UUID NOT NULL REFERENCES hosts(id) ON DELETE CASCADE,
    event_type  TEXT NOT NULL,           -- provision_start, provision_ok, provision_fail,
                                         -- heartbeat_ok, heartbeat_miss, grok_installed,
                                         -- grok_uninstalled, status_change
    severity    TEXT NOT NULL DEFAULT 'info',  -- info | warn | error
    message     TEXT,
    payload     JSONB NOT NULL DEFAULT '{}'::jsonb,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- 호스트별 타임라인 조회 (최신순).
CREATE INDEX idx_host_events_host_created ON host_events(host_id, created_at DESC);
-- 심각도별 필터링.
CREATE INDEX idx_host_events_severity ON host_events(severity, created_at DESC)
    WHERE severity IN ('warn', 'error');
