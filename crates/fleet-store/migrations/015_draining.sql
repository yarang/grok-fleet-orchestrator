-- 015_draining.sql — 드레인 상태 및 DAG 체이닝 지원 스키마 추가.
-- tasks 테이블에 DAG 체이닝용 의존성 및 체크포인트 브랜치 필드를 추가합니다.

ALTER TABLE tasks
    ADD COLUMN dependency_ids UUID[] DEFAULT '{}',
    ADD COLUMN checkpoint_branch VARCHAR(255),
    ADD COLUMN skills_required VARCHAR[] DEFAULT '{}';

ALTER TABLE hosts
    ADD COLUMN cpu_usage REAL,
    ADD COLUMN ram_usage REAL;
