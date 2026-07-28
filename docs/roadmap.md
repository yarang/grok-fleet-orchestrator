# Roadmap & Gap Analysis

> Generated 2026-07-28. Priority levels: **P0** critical, **P1** high, **P2** medium, **P3** low.

## P0 — Production Blockers

1. **Dockerfile** — 컨테이너화 불가. 프로덕션 배포 필수.
2. **docker-compose** — 로컬 개발/통합 테스트용 orchestrator + Postgres + worker 환경 부재.

## P1 — Security & Data Risks

3. **패스워드 재설정 엔드포인트 레이트 리미팅** — `/forgot-password`, `/reset-password`, `/resend-verification`에 rate limiting 없음. 이메일 폭탄 / 토큰 열거 공격에 취약.
4. **PgStore auth 통합 테스트 부재** — User, Session, LoginAttempt, PasswordReset, EmailVerification 작업이 실제 Postgres 대비 미검증.
5. **지연 시간/처리량 메트릭 부재** — `task_duration_seconds`, `dispatch_latency`, `http_request_duration` 히스토그램 없음.
6. **DB 백업 스크립트 부재** — `pg_dump` 자동화, 시점 복구 설정 없음.

## P2 — Scale Prep

7. `CorsLayer::permissive()` on fleet-api — 모든 출처 허용.
8. fleet-api 보안 헤더 부재 — CSP/HSTS/X-Frame-Options 미적용.
9. 구조화된 감사 로그 테이블 부재 — auth 이벤트가 `tracing::info!`로만 출력.
10. 세션 토큰 로테이션 부재 — 8시간 고정 토큰.
11. 페이지네이션 불일치 — `WorkerFilter`에 offset 없음, fleet-api workers 엔드포인트에 페이지네이션 없음.
12. API 오류 응답 포맷 불일치 — fleet-api는 `{error:{code,message}}`, dashboard는 ad-hoc.
13. OpenTelemetry / 분산 추적 부재 — `#[instrument]` 스팬 없음.
14. 다크 모드, 컬럼 정렬, 고급 필터링 부재.
15. 시작 시 설정 검증 부재 — `DATABASE_URL` 등 필수 항목 미검증.
16. 커넥션 풀 튜닝 부재 — `acquire_timeout`, `max_lifetime`, `idle_timeout` 미설정.
17. `fleet_list_tasks` MCP 도구 부재.
18. 예약된 DB 정리 작업 부재 — 만료된 세션/토큰 미청소.
19. 마이그레이션 롤백 스크립트 부재.

## P3 — Nice-to-have

20. Bearer 토큰 비교가 상수시간이 아님.
21. OpenAPI/Swagger 스펙 부재.
22. Dashboard API에 `/v1` 버전 부재.
23. 프론트엔드 페이지네이션 UI 부재.
24. 모바일 반응형 감사 미수행.
25. 서킷 브레이커 상태가 인메모리 전용 (다중 인스턴스 불가).
26. 시크릿 매니저 통합 부재 (Vault/AWS SM).
27. 예시 설정 파일 부재 (`orchestrator.example.toml`).
28. 추가 MCP 도구 (토큰 관리, 호스트 인벤토리, 브레이커 리셋).
29. Dashboard API의 하드코딩된 도구 목록.
30. CI 커버리지 리포팅 부재.

---

## 현재 진행 상황 (2026-07-28 기준)

### 완료된 기능
- ✅ RBAC 권한 강제 (10개 API 핸들러)
- ✅ 쿠키 세션 인증 (Phase 9.1)
- ✅ 이메일 기반 로그인 + 인증 플로우
- ✅ Gmail SMTP 통합
- ✅ 비밀번호 재설정 플로우 (T4)
- ✅ 인증 이메일 재발송 UI (T5)
- ✅ 태스크 API 페이지네이션 offset 지원 (T6)
- ✅ 워커 상세 태스크 쿼리 SQL 최적화 (T7)
- ✅ 태스크/워커 상세 페이지
- ✅ 사용자 관리 CRUD UI
- ✅ SSE 실시간 업데이트
- ✅ mTLS 지원
- ✅ 서킷 브레이커
- ✅ ACP 자동 재연결
- ✅ 헬스체커 + 하트비트 모니터링
- ✅ 프로비저너 (Ansible 플레이북)
- ✅ Prometheus 메트릭 (7개 메트릭 패밀리)

### 테스트 현황
~375+ 테스트. 11개 크레이트 전체에 걸쳐 단위/통합/E2E 테스트 보유.
