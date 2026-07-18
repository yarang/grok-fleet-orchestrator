-- 002_indexes.sql — 쿼리 패턴별 인덱스

-- tasks: 상태별 목록 조회 (가장 빈번한 쿼리)
CREATE INDEX idx_tasks_status_phase_created ON tasks (status_phase, created_at DESC);

-- tasks: 생성자별 조회
CREATE INDEX idx_tasks_created_by ON tasks (created_by, created_at DESC);

-- tasks: status_phase만으로 필터링 (커버링 인덱스)
CREATE INDEX idx_tasks_phase ON tasks (status_phase);

-- workers: 상태별 조회
CREATE INDEX idx_workers_status ON workers (status);

-- workers: 라벨 기반 필터링 (JSONB GIN)
CREATE INDEX idx_workers_labels_gin ON workers USING GIN (labels jsonb_path_ops);

-- task_outputs: 특정 작업의 청크 시퀀스순 조회
-- (task_id, seq)는 이미 PRIMARY KEY이므로 추가 인덱스 불필요.

-- events: 시퀀스 기반 페이지네이션 + 타입별 필터
CREATE INDEX idx_events_type_created ON events (event_type, created_at DESC);
