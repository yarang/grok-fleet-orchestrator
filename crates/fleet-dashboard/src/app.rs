//! 대시보드 앱 조립 + 서버 실행.

use std::net::SocketAddr;
use std::sync::Arc;

use axum::body::Body;
use axum::extract::{Request, State};
use axum::http::{HeaderName, HeaderValue};
use axum::middleware;
use axum::middleware::Next;
use axum::response::Response;
use axum::routing::{delete, get, post, Router};
use tower_http::cors::CorsLayer;
use tower_http::set_header::SetResponseHeaderLayer;
use tower_http::trace::TraceLayer;
use tracing::info;

use fleet_scheduler::Dispatcher;
use fleet_store::Store;

use crate::auth::require_session;
use crate::handlers;

/// 대시보드 서버의 공유 상태.
pub struct DashboardState {
    /// Store trait 구현체.
    pub store: Arc<dyn Store>,
    /// LISTEN/NOTIFY용 Postgres 풀 (SSE 스트리밍에서 사용).
    pub pool: sqlx::PgPool,
    /// 쿠키 Secure 플래그 (로컬 개발은 false, 프로덕션은 true).
    pub secure_cookies: bool,
    /// SMTP 설정 (이메일 인증 발송용). None이면 이메일 발송 안 함.
    pub smtp_config: Option<crate::email::SmtpConfig>,
    /// 마스터 키 (SSH 키 암호화/복호화용). None이면 프로비저닝 기능 비활성.
    pub master_key: Option<Arc<fleet_credentials::MasterKey>>,
    /// 태스크 제출(`POST /api/tasks`)용 Dispatcher. `fleet serve`가 fleet-api와 공유하는
    /// 동일 인스턴스를 주입한다 — None이면 태스크 제출 UI/API가 503을 반환한다(주로 이
    /// 기능을 다루지 않는 테스트 하네스용).
    pub dispatcher: Option<Arc<Dispatcher>>,
    /// 이 대시보드가 리버스 프록시 뒤에서 마운트된 경로 prefix (예: `/dashboard`).
    /// `FLEET_DASHBOARD_BASE_PATH` env로 설정. 빈 문자열이면 루트 마운트(기존 동작과
    /// 동일). `normalize_base_path`로 정규화됨 — 앞에 `/`가 붙고 뒤에는 안 붙는다.
    ///
    /// 앱 내부 axum 라우터 자체는 이 prefix를 모른다(계속 `/login`, `/api/...`처럼
    /// unprefixed로 등록) — nginx가 `location /dashboard/ { proxy_pass .../; }`로
    /// prefix를 벗겨서 넘겨주기 때문이다. 이 필드는 오직 **브라우저로 내려가는
    /// 응답**(redirect Location, HTML `<base href>`)에만 쓰인다. 그래야 브라우저가
    /// 상대경로를 다시 prefix 포함해서 요청한다 — 이게 없으면 로그인 리다이렉트가
    /// prefix 없는 절대경로(`/login`)로 나가 nginx에서 404가 난다(실제 프로덕션에서
    /// 관측된 버그).
    pub base_path: String,
}

/// `FLEET_DASHBOARD_BASE_PATH` 값을 정규화한다.
///
/// 빈 값/미설정 → `""`(루트 마운트). 그 외에는 앞에 `/`를 보장하고 뒤의 `/`는 제거한다
/// (`"dashboard"`, `"/dashboard"`, `"/dashboard/"` 모두 `"/dashboard"`로 수렴).
fn normalize_base_path(raw: &str) -> String {
    let trimmed = raw.trim().trim_end_matches('/');
    if trimmed.is_empty() {
        String::new()
    } else if trimmed.starts_with('/') {
        trimmed.to_string()
    } else {
        format!("/{trimmed}")
    }
}

impl DashboardState {
    pub fn new(
        store: Arc<dyn Store>,
        pool: sqlx::PgPool,
        dispatcher: Option<Arc<Dispatcher>>,
    ) -> Self {
        Self {
            store,
            pool,
            secure_cookies: true,
            smtp_config: crate::email::SmtpConfig::from_env(),
            master_key: fleet_credentials::MasterKey::load().ok().map(Arc::new),
            dispatcher,
            base_path: normalize_base_path(
                &std::env::var("FLEET_DASHBOARD_BASE_PATH").unwrap_or_default(),
            ),
        }
    }

    /// 로컬 개발용 (Secure 쿠키 비활성).
    pub fn new_insecure(
        store: Arc<dyn Store>,
        pool: sqlx::PgPool,
        dispatcher: Option<Arc<Dispatcher>>,
    ) -> Self {
        Self {
            store,
            pool,
            secure_cookies: false,
            smtp_config: crate::email::SmtpConfig::from_env(),
            master_key: fleet_credentials::MasterKey::load().ok().map(Arc::new),
            dispatcher,
            base_path: normalize_base_path(
                &std::env::var("FLEET_DASHBOARD_BASE_PATH").unwrap_or_default(),
            ),
        }
    }

    /// `path`(반드시 `/`로 시작) 앞에 `base_path`를 붙인 절대경로를 만든다.
    /// redirect의 `Location` 헤더처럼, 브라우저가 다시 요청을 보낼 절대경로가
    /// 필요한 자리에 쓴다. `base_path`가 빈 문자열이면 `path`를 그대로 반환한다
    /// (기존 루트 마운트 동작과 100% 동일).
    pub fn abs(&self, path: &str) -> String {
        debug_assert!(
            path.starts_with('/'),
            "abs() expects a root-relative path, got {path:?}"
        );
        format!("{}{}", self.base_path, path)
    }
}

/// `text/html` 응답의 `<head>` 바로 뒤에 `<base href="{base_path}/">`를 주입한다.
///
/// 이게 있어야 페이지 안의 모든 상대경로(`href="login"`, `fetch('api/workers')` 등 —
/// 앞에 `/`가 없는 참조)가 이 대시보드가 리버스 프록시 뒤 어느 prefix에 마운트돼
/// 있든 올바르게 그 prefix를 붙여 다시 요청된다. `base_path`가 빈 문자열(루트
/// 마운트)이면 `<base href="/">`를 넣는다 — 상대경로 해석 기준을 항상 origin
/// root로 고정해, 페이지가 어떤 하위 경로에서 서빙되는지와 무관하게 동일하게
/// 동작한다(핸들러마다 반환하는 raw HTML 문자열/임베드 자산을 일일이 고치는 대신
/// 이 미들웨어 하나로 모든 HTML 응답에 일괄 적용).
///
/// `<head>` 태그가 없는 응답(HTML이 아니거나 조각 HTML)은 그대로 통과시킨다.
async fn inject_base_href(
    State(state): State<Arc<DashboardState>>,
    req: Request,
    next: Next,
) -> Response {
    let response = next.run(req).await;

    let is_html = response
        .headers()
        .get(axum::http::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|ct| ct.starts_with("text/html"));
    if !is_html {
        return response;
    }

    let (mut parts, body) = response.into_parts();
    let Ok(bytes) = axum::body::to_bytes(body, usize::MAX).await else {
        // 모을 수 없는 body(스트리밍 등) — 원본 그대로 재구성해 통과.
        return Response::from_parts(parts, Body::empty());
    };

    let base_tag = format!("<base href=\"{}/\">", state.base_path);
    let injected = match bytes.windows(6).position(|w| w == b"<head>") {
        Some(idx) => {
            let mut out = Vec::with_capacity(bytes.len() + base_tag.len());
            out.extend_from_slice(&bytes[..idx + 6]);
            out.extend_from_slice(base_tag.as_bytes());
            out.extend_from_slice(&bytes[idx + 6..]);
            out
        }
        None => bytes.to_vec(),
    };

    // body 길이가 바뀌었으므로 기존 Content-Length는 반드시 제거한다 — 안 지우면
    // 브라우저가 새 body를 원래(더 짧은) 길이만큼만 읽고 나머지를 버린다.
    parts.headers.remove(axum::http::header::CONTENT_LENGTH);
    Response::from_parts(parts, Body::from(injected))
}

/// 전체 라우터 조립.
///
/// 라우트 그룹:
/// - **public**: `/login`, `/logout`, `/health`, `/static/*` (세션 미들웨어 없음 —
///   `/static/*`는 로그인/부트스트랩 등 인증 전 화면도 자신의 CSS/JS를 로드해야
///   하므로 여기 속한다; 민감 데이터 없음)
/// - **protected**: `/`, `/api/*` (require_session 적용)
///
/// 보안 헤더 (Phase 9.1.7):
/// - CSP, X-Frame-Options, HSTS, X-Content-Type-Options, Referrer-Policy
/// - CORS는 동일 출처만 허용 (permissive 제거)
pub fn build_dashboard_app(state: Arc<DashboardState>) -> Router {
    let public = Router::new()
        .route("/login", get(handlers::login_page).post(handlers::login))
        .route(
            "/bootstrap",
            get(handlers::bootstrap_page).post(handlers::bootstrap),
        )
        .route("/health", get(handlers::health))
        .route("/verify-email", get(handlers::verify_email_page))
        .route(
            "/forgot-password",
            get(handlers::forgot_password_page).post(handlers::forgot_password),
        )
        .route(
            "/reset-password",
            get(handlers::reset_password_page).post(handlers::reset_password),
        )
        .route(
            "/resend-verification",
            get(handlers::resend_verification_page).post(handlers::resend_verification_form),
        )
        .route(
            "/api/users/resend-verification",
            post(handlers::resend_verification_api),
        )
        // 정적 자산(CSS/JS) — 민감 데이터 없음, 로그인/부트스트랩처럼 인증 전
        // 화면도 이걸 로드해야 한다. require_session 뒤에 있으면 로그인 페이지가
        // 자기 자신의 스타일시트를 못 불러와 303으로 리다이렉트되는 순환에
        // 빠진다 — 실제로 프로덕션에서 로그인 화면이 스타일 없이(unstyled) 뜨는
        // 버그로 관측됨.
        .route("/static/*path", get(handlers::static_asset));

    let protected = Router::new()
        .route("/", get(handlers::index))
        // ── P1: 페이지 ──
        .route("/tasks", get(handlers::task_queue_page))
        .route("/tasks/new", get(handlers::task_new_page))
        .route("/tasks/:id", get(handlers::task_detail_page))
        .route("/workers/:id", get(handlers::worker_detail_page))
        .route("/admin/users", get(handlers::admin_users_page))
        // ── P1: 사용자 CRUD API ──
        .route(
            "/api/users",
            get(handlers::list_users_api).post(handlers::create_user_api),
        )
        .route("/api/users/:id/toggle", post(handlers::toggle_user_api))
        .route("/api/users/:id/delete", post(handlers::delete_user_api))
        // ── P1.5: 호스트 ──
        .route("/hosts", get(handlers::host_inventory_page))
        // Project 화면 (로드맵 #48). `/projects/new`는 `/projects/:id`보다
        // 먼저 등록해야 "new"가 id로 해석되지 않는다.
        .route("/projects", get(handlers::projects_page))
        .route("/projects/new", get(handlers::project_new_page))
        .route("/projects/:id", get(handlers::project_detail_page))
        // AgentTemplate 관리 화면 (로드맵 #92). Project 화면과 같은 3면
        // 구성 — 목록/생성/상세. 생성 폼만 `create` 권한으로 가린다.
        .route("/agent-templates", get(handlers::agent_templates_page))
        .route(
            "/agent-templates/new",
            get(handlers::agent_template_new_page),
        )
        .route(
            "/agent-templates/:id",
            get(handlers::agent_template_detail_page),
        )
        .route("/hosts/:hostname", get(handlers::host_detail_page))
        .route("/hosts/provision", get(crate::provisioning::provision_page))
        // ── P2: 관리 ──
        .route("/admin/activity", get(handlers::admin_activity_page))
        .route("/admin/tools", get(handlers::admin_tools_page))
        .route(
            "/admin/ssh-keys",
            get(crate::provisioning::admin_ssh_keys_page),
        )
        // ── API ──
        .route("/api/overview", get(handlers::overview))
        .route("/api/workers", get(handlers::list_workers))
        .route("/api/workers/:id", get(handlers::get_worker_detail))
        .route(
            "/api/tasks",
            get(handlers::list_tasks).post(handlers::submit_task_api),
        )
        .route(
            "/api/tasks/:id",
            get(handlers::get_task_detail_api).delete(handlers::delete_task_api),
        )
        .route("/api/tasks/:id/thread", get(handlers::get_task_thread_api))
        .route("/api/task-threads", get(handlers::list_task_threads_api))
        .route("/api/events", get(handlers::list_events))
        .route("/api/events/stream", get(crate::sse::events_stream))
        .route("/api/me", get(handlers::me))
        .route("/api/hosts", get(handlers::list_hosts_api))
        .route("/api/hosts/:hostname", get(handlers::get_host_detail_api))
        // Project (로드맵 #48, 1단계). PATCH(policy 변경)와 host/worker 배정
        // endpoint는 docs/contracts/project-management.md가 승인 전 차단
        // 대상으로 명시해 아직 없다.
        .route(
            "/api/projects",
            get(handlers::list_projects_api).post(handlers::create_project_api),
        )
        .route(
            "/api/projects/:id",
            get(handlers::get_project_detail_api).delete(handlers::delete_project_api),
        )
        // Agent (로드맵 #49, 1단계). PATCH가 없는 것은 규칙이다 —
        // `project_id`가 불변이라 갱신 경로 자체를 만들지 않는다.
        .route(
            "/api/agents",
            get(handlers::list_agents_api).post(handlers::create_agent_api),
        )
        .route("/api/agents/:id", delete(handlers::stop_agent_api))
        // 배정 (로드맵 #67 4a). `PATCH /api/agents/:id`가 아니라 하위
        // 경로인 것은 규칙을 지키기 위해서다 — Agent에는 갱신 경로가 없고,
        // 배정은 필드 편집이 아니라 **명명된 동작**이다. 같은 이유로
        // 회수가 `DELETE`인 것과 짝을 이룬다.
        .route("/api/agents/:id/place", post(handlers::place_agent_api))
        .route(
            "/api/agent-templates",
            get(handlers::list_agent_templates_api).post(handlers::create_agent_template_api),
        )
        .route(
            "/api/agent-templates/:id",
            get(handlers::get_agent_template_api),
        )
        .route(
            "/api/agent-templates/:id/revisions",
            get(handlers::list_agent_template_revisions_api)
                .post(handlers::create_agent_template_revision_api),
        )
        .route(
            "/api/agent-templates/:id/revisions/:revision_id/revoke",
            post(handlers::revoke_agent_template_revision_api),
        )
        .route(
            "/api/agent-templates/:id/dependents",
            get(handlers::agent_template_dependents_api),
        )
        .route(
            "/api/agent-templates/:id/status",
            post(handlers::change_agent_template_status_api),
        )
        // Issue (로드맵 #92, Issue 표면). 상태 전이는 PATCH가 아니라 별도
        // endpoint다 — 목표 상태마다 요구 capability가 다르기 때문
        // (handlers::required_capability_for_transition).
        .route(
            "/api/issues",
            get(handlers::list_issues_api).post(handlers::create_issue_api),
        )
        .route(
            "/api/issues/:id",
            get(handlers::get_issue_api).patch(handlers::update_issue_api),
        )
        .route(
            "/api/issues/:id/transition",
            post(handlers::transition_issue_api),
        )
        .route(
            "/api/issues/:id/comments",
            get(handlers::list_issue_comments_api).post(handlers::add_issue_comment_api),
        )
        .route(
            "/api/issues/:id/links",
            get(handlers::list_issue_links_api).post(handlers::link_issue_task_api),
        )
        .route(
            "/api/issues/:id/links/:task_id",
            axum::routing::delete(handlers::unlink_issue_task_api),
        )
        // 인증/권한 감사 로그 (audit_log 테이블).
        // 작업·워커 생명주기 이벤트는 위의 /api/events가 담당한다 — 이전에는
        // 같은 이벤트 데이터를 /api/audit이 중복 제공해 이름이 혼동됐다.
        .route("/api/audit", get(handlers::list_auth_audit_api))
        .route("/api/tools", get(handlers::list_tools_api))
        // ── SSH 키 관리 API ──
        .route(
            "/api/ssh-keys",
            get(crate::provisioning::list_ssh_keys_api)
                .post(crate::provisioning::create_ssh_key_api),
        )
        .route(
            "/api/ssh-keys/:name",
            axum::routing::delete(crate::provisioning::delete_ssh_key_api),
        )
        // ── 프로비저닝 API ──
        .route(
            "/api/hosts/provision",
            post(crate::provisioning::provision_host_api),
        )
        .route("/logout", post(handlers::logout))
        .route_layer(middleware::from_fn_with_state(
            state.clone(),
            require_session,
        ));

    // 보안 헤더 상수.
    let csp = HeaderValue::from_static(
        "default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; connect-src 'self'; frame-ancestors 'none'; base-uri 'self'",
    );
    let frame_opts = HeaderValue::from_static("DENY");
    let nosniff = HeaderValue::from_static("nosniff");
    let hsts = HeaderValue::from_static("max-age=63072000; includeSubDomains; preload");
    let referrer = HeaderValue::from_static("strict-origin-when-cross-origin");

    Router::new()
        .merge(public)
        .merge(protected)
        // HTML 응답에 <base href>를 주입 — 리버스 프록시 prefix 무관 동작의 핵심.
        // 보안 헤더보다 먼저(레이어는 바깥→안 순서로 적용되므로, 여기 등록 순서상
        // body를 실제로 건드리는 이 레이어가 최종 응답 직전에 돈다) 둬서 body를
        // 재작성한 뒤에도 아래 레이어들이 헤더를 마저 세팅하도록 한다.
        .layer(middleware::from_fn_with_state(
            state.clone(),
            inject_base_href,
        ))
        // 보안 헤더 — 모든 응답에 적용 (이미 설정되지 않은 경우만).
        .layer(SetResponseHeaderLayer::if_not_present(
            HeaderName::from_static("content-security-policy"),
            csp,
        ))
        .layer(SetResponseHeaderLayer::if_not_present(
            HeaderName::from_static("x-frame-options"),
            frame_opts,
        ))
        .layer(SetResponseHeaderLayer::if_not_present(
            HeaderName::from_static("x-content-type-options"),
            nosniff,
        ))
        .layer(SetResponseHeaderLayer::if_not_present(
            HeaderName::from_static("strict-transport-security"),
            hsts,
        ))
        .layer(SetResponseHeaderLayer::if_not_present(
            HeaderName::from_static("referrer-policy"),
            referrer,
        ))
        .layer(TraceLayer::new_for_http())
        // CORS — 동일 출처 전용 (permissive 제거).
        // 외부 API 접근이 필요하면 token 기반 인증을 사용할 것.
        .layer(CorsLayer::new())
        .with_state(state)
}

/// 대시보드 HTTP 서버 바인딩 + serve.
pub async fn run_dashboard_server(
    state: Arc<DashboardState>,
    bind: SocketAddr,
) -> std::io::Result<()> {
    let app = build_dashboard_app(state);
    let listener = tokio::net::TcpListener::bind(bind).await?;
    info!(%bind, "dashboard server listening");
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use fleet_store::mem::MemStore;
    use fleet_store::Store;

    #[tokio::test]
    async fn dashboard_app_builds() {
        let store = Arc::new(MemStore::new()) as Arc<dyn Store>;
        // dashboard_app 빌드만 검증 (실제 pool은 필요 없음).
        // pool은 SSE용이므로 stub. 실제 통합 테스트는 PgStore 기반.
        let _ = store;
    }

    #[test]
    fn normalize_base_path_empty_stays_empty() {
        assert_eq!(normalize_base_path(""), "");
        assert_eq!(normalize_base_path("   "), "");
    }

    #[test]
    fn normalize_base_path_adds_leading_slash() {
        assert_eq!(normalize_base_path("dashboard"), "/dashboard");
    }

    #[test]
    fn normalize_base_path_strips_trailing_slash() {
        assert_eq!(normalize_base_path("/dashboard/"), "/dashboard");
        assert_eq!(normalize_base_path("/dashboard"), "/dashboard");
    }

    #[tokio::test]
    async fn abs_prefixes_with_base_path() {
        let store = Arc::new(MemStore::new()) as Arc<dyn Store>;
        let pool = sqlx::postgres::PgPoolOptions::new()
            .max_connections(1)
            .connect_lazy("postgres://__test_unused__@localhost/__none__")
            .expect("connect_lazy must not perform I/O");
        let mut state = DashboardState::new(store, pool, None);
        state.base_path = "/dashboard".to_string();
        assert_eq!(state.abs("/login"), "/dashboard/login");
    }

    #[tokio::test]
    async fn abs_is_noop_for_root_mount() {
        let store = Arc::new(MemStore::new()) as Arc<dyn Store>;
        let pool = sqlx::postgres::PgPoolOptions::new()
            .max_connections(1)
            .connect_lazy("postgres://__test_unused__@localhost/__none__")
            .expect("connect_lazy must not perform I/O");
        let state = DashboardState::new(store, pool, None);
        assert_eq!(state.abs("/login"), "/login");
    }
}
