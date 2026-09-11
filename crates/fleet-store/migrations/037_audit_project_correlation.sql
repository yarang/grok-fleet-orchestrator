-- 037: audit_log에 project_id 상관관계 컬럼 (로드맵 #95 1단계)
--
-- `#76`이 감사 범위를 넓히면서 `project_id`를 상관관계 필드로 남겨 뒀고, 그때의
-- 대기 사유는 "Project 엔티티가 아직 없다"였다. 그 사유는 `#48` 1·2·3단계
-- (2026-08-24, `022_projects.sql`)로 해소됐다.
--
-- 왜 `detail` JSONB로 부족한가. 값은 이미 일부 지점이 `detail.project_id`에 넣고
-- 있지만 **저장돼 있을 뿐 색인되지 않는다** — `AuditFilter`에 술어를 만들 자리가
-- 없어 "이 Project에서 무슨 일이 있었는가"를 질의할 수 없다. 게다가 자유 형식이라
-- Project 범위 감사 지점 11곳 중 5곳만 값을 싣고 있었고, 키 오타가 나도 컴파일이
-- 통과한 뒤 질의만 조용히 빈 결과를 낸다.
--
-- **FK를 걸지 않는다.** `projects`에 hard-delete 경로가 없다는 것은 근거가 아니다.
-- 근거는 감사가 "시도"의 사실을 기록한다는 데 있다 — 존재하지 않는 Project를 지목한
-- 거절된 요청의 실패 감사는 FK를 위반하고, 그러면 **감사가 가장 필요한 순간에 감사
-- 쓰기가 실패한다.** `011`이 `actor_user_id`에 `ON DELETE SET NULL`을 고른 것과 같은
-- 계열이되 근거가 다르다: 거기서는 대상이 사라져도 기록이 남아야 해서, 여기서는
-- 대상이 애초에 없었어도 기록이 남아야 해서다.
--
-- NULL은 "빠뜨렸다"가 아니라 "이 이벤트는 어떤 Project에도 속하지 않는다"는 단정이다
-- (`auth.*`, `user.*`, `worker.*` 등). 글로벌 AgentTemplate(`project_id IS NULL`)의
-- 이벤트도 같은 이유로 NULL이다.

ALTER TABLE audit_log ADD COLUMN project_id UUID;

-- 기존 행 backfill. 이것이 없으면 `?project_id=X` 질의가 컬럼 도입 이전의 이벤트를
-- 한 건도 돌려주지 않는다 — 감사 표면에서 조용한 누락은 "그 Project에서 아무 일도
-- 없었다"로 읽히므로, 부분 구현보다 나쁘다.
--
-- 두 절이 필요한 이유: `agent.*`/`issue.*`/`agent_template.create`는 값을 `detail`에
-- 넣었지만, `project.*` 계열은 Project 자체가 대상이라 `target_id`에 들어 있다.
-- `->>`는 키 부재와 JSON null을 모두 SQL NULL로 주므로 별도 가드가 필요 없고,
-- `::uuid` 캐스트는 형식이 깨진 값에서 시끄럽게 실패한다 — 여기서는 그것이 원하는
-- 동작이다(감사 데이터를 조용히 버리지 않는다).
UPDATE audit_log
   SET project_id = (detail->>'project_id')::uuid
 WHERE project_id IS NULL
   AND detail->>'project_id' IS NOT NULL;

UPDATE audit_log
   SET project_id = target_id::uuid
 WHERE project_id IS NULL
   AND target_type = 'project'
   AND target_id IS NOT NULL;

-- `011`이 세운 관행과 같은 모양 — 상관 축 + 최신순. 이것이 없으면 새 술어가
-- seq scan이 된다.
CREATE INDEX idx_audit_log_project ON audit_log(project_id, created_at DESC);
