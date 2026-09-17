-- 명령이 **언제 나갔는지**를 기록한다 (로드맵 `#70` 게이트 3 — ACK 유실).
--
-- `031`이 `command_generation`/`last_acked_generation`을 만들어 "확인됐는가"는
-- 알 수 있게 됐지만, **"얼마나 오래 확인되지 않았는가"는 알 수 없었다.**
-- 그래서 Worker가 명령을 영영 집어가지 않아도 아무 신호가 나지 않았고,
-- `command_delivered()`/`start_pending()`은 MCP 응답에 값으로만 실릴 뿐
-- 아무도 그것으로 판정하지 않았다(grep: 그 둘의 호출부는 `handlers.rs`의
-- JSON 조립 두 줄뿐이었다).
--
-- **`updated_at`으로 대신할 수 없다.** 그 컬럼은 관측 보고(`#67` 4c-B)를
-- 포함해 행을 건드리는 **모든** 쓰기에서 움직인다. 명령을 집어가지 않는
-- Worker라도 heartbeat은 계속 올 수 있으므로, `updated_at`을 기준으로 재면
-- 미확인 시간이 영원히 0에 가깝게 유지된다 — 정확히 탐지하려는 상황에서
-- 탐지가 되지 않는다.
ALTER TABLE agents ADD COLUMN command_issued_at TIMESTAMPTZ;

-- 기존 행 중 **미확인 명령을 들고 있는 것**만 `updated_at`으로 채운다.
--
-- 이것은 `041`이 "기본값을 주면 없던 사실을 만든다"며 NULL을 고수한 것과
-- 모순되지 않는다. 저기서 채울 뻔한 값(세션 id)은 **추측**이었지만, 여기의
-- `updated_at`은 증명 가능한 **상한**이다 — 명령이 이 행을 마지막으로 손댄
-- 시각보다 나중에 나갔을 수는 없다. 상한으로 재면 미확인 시간이 실제보다
-- 짧게 나오므로 판정은 **늦어질 뿐 틀리지 않는다**(거짓 양성이 생기지
-- 않는다). 확인된 명령만 가진 행은 잴 대상이 없으므로 NULL로 둔다.
UPDATE agents
   SET command_issued_at = updated_at
 WHERE last_acked_generation < command_generation;
