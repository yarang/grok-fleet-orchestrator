-- 040_audit_control_epoch.sql — 감사 기록에 제어면 세대를 싣는다
-- (로드맵 `#70` 게이트 6 선행).
--
-- ## 왜 필요한가
--
-- 관측성 정본의 alert 표는 "control lease renewal 실패 또는 둘 이상의 owner
-- 관측"에 대해 운영자의 첫 조치를 **"fencing/epoch 증거 확인"**으로 규정한다.
-- 그런데 그 증거를 담을 자리가 감사에 없었다 — `audit_log`는 actor·outcome
-- 계열만 갖는다.
--
-- ## 왜 지금까지 못 넣었는가 (2026-09-06 실측)
--
-- 게이트 ⑥의 차단 사유는 오랫동안 "`control_epoch`가 필드로 존재하지 않는다"
-- 였는데 그것은 부정확했다. `control_epoch`는 이미 셋(026·031·035)에 있다.
-- 진짜 막힌 자리는 다른 데였다: 감사 기록을 내는 35군데가 전부
-- `fleet-api`·`fleet-dashboard`·`fleet-core`이고 **`fleet-scheduler`는 0건**이라,
-- dispatch·펜싱 같은 제어면 결정이 감사에 아무것도 남기지 않았다. 컬럼만
-- 먼저 만들었다면 모든 emitter에서 항상 NULL이었을 것이다.
--
-- 그래서 이 마이그레이션은 **제어면 감사 경로와 같은 증분에서** 들어온다.
--
-- ## NULL을 허용하는 이유
--
-- 두 가지가 모두 NULL이며 **둘 다 정상이다.**
--   1. 기존 35개 emitter(운영자 행위) — 그 결정에는 제어면 세대가 없다.
--   2. 리스를 갖지 못한 채 내린 거절 — `LeaseStatus::Fenced`/`Stopped`에는
--      epoch 자체가 없다(`lease.rs`의 `epoch()`가 `None`).
-- 2번에서 "epoch가 없었다"는 것 자체가 증거이므로, 그 사실은 `detail`의
-- `lease_status`가 나른다. NOT NULL로 두면 이 두 경우를 표현할 수 없다.
ALTER TABLE audit_log ADD COLUMN IF NOT EXISTS control_epoch BIGINT;

-- 세대별 조회 — "epoch 7에서 무슨 일이 있었나"가 위 alert의 첫 질문이다.
-- 부분 인덱스인 이유: 압도적 다수의 행이 NULL이고(운영자 행위), 그것들은
-- 이 질문의 대상이 아니다.
CREATE INDEX IF NOT EXISTS idx_audit_log_control_epoch
    ON audit_log(control_epoch, created_at DESC)
    WHERE control_epoch IS NOT NULL;
