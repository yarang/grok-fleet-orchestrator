---
name: github-actions-monitor
description: Custom skill to monitor, verify, and follow-up on GitHub Actions CI run outcomes for a given git commit SHA.
---

# GitHub Actions 모니터링 스킬 (GitHub Actions Monitor Skill)

이 스킬은 로컬 기여 코드가 원격 리포지토리에 푸시된 후, GitHub Actions CI의 기동 및 통과 여부를 실시간으로 추적/검증하는 패키지화된 자동화 도구 및 지침입니다.

---

## 1. 개요 및 설계 목적

일부 에이전트는 `git push` 후에 원격 빌드가 완벽히 그린(Green)으로 수렴하는지 검증하지 않고 세션을 종료하는 실수를 범합니다. 이 스킬은 푸시된 커밋 SHA를 기반으로 GitHub Actions API를 직접 추적하여, 빌드가 `completed` 및 `success` 상태가 될 때까지 폴링 모니터링하는 작업을 완전히 내재화합니다.

---

## 2. 패키징된 도구 사용법

이 스킬 폴더에는 자동화용 CLI 파이썬 스크립트인 `monitor_ci.py` 가 포함되어 있습니다.

### 위치
*   스크립트 파일: `scripts/monitor_ci.py`

### CLI 옵션
```bash
# 기본 사용법
python3 scripts/monitor_ci.py --commit <COMMIT_SHA>

# 상세 제어 옵션
python3 scripts/monitor_ci.py \
    --owner yarang \
    --repo grok-fleet-orchestrator \
    --commit <COMMIT_SHA> \
    --poll 15 \
    --max-wait 300
```

### 실행 출력 예시
```text
Starting GitHub Actions CI monitoring for commit: 93faaad in yarang/grok-fleet-orchestrator
[1/20] Run ID: 31136317855 | Status: in_progress | Conclusion: None
[2/20] Run ID: 31136317855 | Status: in_progress | Conclusion: None
[3/20] Run ID: 31136317855 | Status: completed | Conclusion: success
🟢 GitHub Actions CI PASSED successfully!
```

---

## 3. 에이전트 지침서 (CI Follow-up Protocol)

에이전트는 원격 저장소에 커밋을 Push한 후, 다음 절차를 무조건적으로 이행해야 합니다.

1.  **커밋 SHA 획득**:
    *   `git rev-parse HEAD` 명령어를 사용해 푸시된 커밋의 전체 해시값을 취득합니다.
2.  **모니터러 스크립트 백그라운드 구동**:
    *   `run_command` 또는 백그라운드 태스크(Task) 기능을 사용해 `monitor_ci.py` 스크립트를 구동하여 최종 성공(PASS) 판정 시점까지 추적합니다.
3.  **최종 상태 리포트**:
    *   성공 시: `🟢 GitHub Actions CI PASSED successfully!` 메시지와 함께 검증 완결을 선언합니다.
    *   실패 시: API 로그를 기반으로 실패한 Job(예: clippy, test, fmt)의 원인을 상세 분석해 추가 패치 작업을 개시해야 합니다.
