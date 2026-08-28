-- 028_worker_incarnation.sql — 워커 프로세스 incarnation 시작 시각
--
-- 정본([권한과 장애 전환](../../../docs/architecture/control-plane-authority-and-failover.md))의
-- `worker_incarnation`을 오케스트레이터 쪽 절반만 먼저 채운다. 정본과 다이어그램은
-- 이 값을 하트비트가 실어 보내는 `process_incarnation` 카운터로 그렸지만, 그러면
-- 판정의 입력이 다시 **피통제자의 자기 신고**가 된다. 여기서는 오케스트레이터가
-- 관측한 사실 — "이미 존재하는 워커 row에 register 요청이 다시 왔다" — 만으로
-- 값을 만든다. fleet-worker는 기동 시 register를 정확히 1회 호출하고
-- (`runner.rs`의 `register_with_retry`), `#78` 이후 종료 시 deregister를 하지
-- 않으므로, 기존 row에 대한 register는 모호함 없이 "그 프로세스가 재시작했다"이다.
--
-- 카운터가 아니라 timestamptz인 이유: 회수 판정은 이 값을 `tasks.dispatched_at`과
-- 비교해야 하는데 그 컬럼은 이미 존재하고 `NOW()`로 찍힌다(012). 카운터로 두면
-- 비교를 위해 incarnation을 태스크에도 실어야 해서 `tasks` 스키마까지 바뀐다.
-- 갱신도 `NOW()`로 하므로 술어의 양변이 같은 Postgres 시계에서 나온다 —
-- 오케스트레이터가 여러 대인 배포에서도 호스트 간 시계 오차가 판정에 들어오지 않는다.
ALTER TABLE workers ADD COLUMN incarnation_started_at TIMESTAMPTZ;

-- 백필을 `NOW()`가 아니라 `registered_at`으로 하는 것이 핵심이다. `NOW()`로 채우면
-- 마이그레이션 시점 이전에 디스패치된 정상 진행 중 태스크가 전부 "이전 incarnation의
-- 고아"로 오판되어 업그레이드 즉시 대량 회수된다. `registered_at`은 **현재 row가
-- 만들어진 시각**이므로, 그 row가 살아 있는 동안 이 워커로 간 어떤 디스패치보다도
-- 반드시 앞선다 — 진행 중 태스크가 오판되지 않는다. (그보다 앞선 디스패치가 있다면
-- 그 사이에 row가 지워졌다는 뜻이고, 그 태스크는 실제로 고아다.)
UPDATE workers SET incarnation_started_at = registered_at WHERE incarnation_started_at IS NULL;

ALTER TABLE workers ALTER COLUMN incarnation_started_at SET NOT NULL;
ALTER TABLE workers ALTER COLUMN incarnation_started_at SET DEFAULT NOW();
