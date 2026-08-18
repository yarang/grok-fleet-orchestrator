//! axum 앱 조립 + 서버 실행.
//!
//! `AppState`는 모든 핸들러가 공유하는 의존성(Store, 인증 설정 등)을 캡슐화.
//! `build_app`는 라우터를 조립하고, `run_http_server`는 바인딩 후 serve.

use std::net::SocketAddr;
use std::sync::Arc;

use axum::{
    extract::Request,
    http::{header, HeaderName, HeaderValue, Method, StatusCode},
    middleware::{self, Next},
    response::Response,
    routing::{get, post},
    Router,
};
use sha2::{Digest, Sha256};
use serde::Deserialize;
use subtle::{Choice, ConstantTimeEq};
use tower_http::cors::CorsLayer;
use tower_http::set_header::SetResponseHeaderLayer;
use tower_http::trace::TraceLayer;
use tracing::info;

use fleet_credentials::MasterKey;
use fleet_core::PermissionKind;
use fleet_store::Store;
use fleet_transport::WorkerTransport;

use crate::handlers;

/// 인증 미들웨어가 요청에 주입하는 최소 권한 컨텍스트.
///
/// handler는 raw header를 다시 해석하지 않고 이 context를 기준으로 endpoint capability를 확인한다.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthorizationContext {
    pub principal_id: String,
    pub authentication_method: AuthenticationMethod,
    pub capabilities: Vec<PermissionKind>,
    /// 요청자가 worker operational credential로 인증된 경우 해당 worker의 신원.
    ///
    /// `Some(id)`일 때만 self-binding 검사(register/heartbeat/deregister가
    /// 조작하려는 worker_id와 일치하는지)가 적용된다. `None`(admin bearer,
    /// development no-auth, CF Access)이면 기존처럼 제한 없이 통과한다.
    pub worker_id: Option<fleet_core::WorkerId>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthenticationMethod {
    ScopedBearer,
    DevelopmentNoAuth,
    /// join에서 발급된 worker operational credential(`fwo_...`)로 인증됨.
    WorkerOperational,
}

/// 환경 manifest에 정의하는 bearer credential. token은 로그나 API 응답에 기록하지 않는다.
#[derive(Debug, Clone, Deserialize)]
pub struct ApiTokenCredential {
    pub principal_id: String,
    pub token: String,
    pub capabilities: Vec<PermissionKind>,
}

/// HTTP API 서버의 공유 상태.
pub struct AppState {
    /// Store trait 구현체 (보통 `Arc<PgStore>`).
    pub store: Arc<dyn Store>,
    /// 워커 통신 transport. `None`이면 register/deregister가 Store만 갱신
    /// (MockTransport 사용 시나 테스트에서 transport 연동이 불필요한 경우).
    pub transport: Option<Arc<dyn WorkerTransport>>,
    /// 워커에게 권장할 하트비트 주기 (초).
    pub heartbeat_interval_secs: u32,
    /// 인증 생략 여부 (개발 모드).
    pub allow_no_auth: bool,
    /// principal과 capability가 결합된 bearer credential 목록.
    pub valid_tokens: Option<Arc<Vec<ApiTokenCredential>>>,
    /// Cloudflare Access Application AUD (Phase 4).
    /// 설정된 경우 CF-Access-Jwt-Assertion 헤더의 aud 클레임과 비교.
    pub cf_audience: Option<String>,
    /// Credentials 암호화용 마스터 키 (Phase 8.6).
    /// `None`이면 credentials API 엔드포인트가 503 반환.
    pub master_key: Option<Arc<MasterKey>>,
    /// CORS 허용 출처 목록 (명시적 allow-list).
    ///
    /// 비어 있으면 CORS 응답 헤더를 전혀 내보내지 않음(브라우저 교차 출처 차단).
    /// fleet-api의 정상 소비자는 fleet-cli / fleet-worker / fleet-mcp 같은
    /// 비브라우저 클라이언트이므로 기본값(빈 목록)이 올바른 배포 형태.
    /// 브라우저 기반 외부 콘솔을 붙이는 경우에만 출처를 명시적으로 추가.
    pub cors_allowed_origins: Vec<String>,
    /// HTTP 요청 지연 히스토그램 (인프로세스 누산).
    ///
    /// 다른 메트릭과 달리 스토어에서 계산할 수 없어 미들웨어가 요청마다
    /// 기록한다. 프로세스 재시작 시 0으로 돌아간다.
    pub http_metrics: Arc<crate::metrics::HttpMetrics>,
}

impl AppState {
    pub fn new(store: Arc<dyn Store>) -> Self {
        Self {
            store,
            transport: None,
            heartbeat_interval_secs: 15,
            allow_no_auth: true,
            valid_tokens: None,
            cf_audience: None,
            master_key: None,
            cors_allowed_origins: Vec::new(),
            http_metrics: Arc::new(crate::metrics::HttpMetrics::new()),
        }
    }

    pub fn with_heartbeat_interval(mut self, secs: u32) -> Self {
        self.heartbeat_interval_secs = secs;
        self
    }

    /// Worker transport 연결. 설정 시 register/deregister가 transport도 갱신.
    pub fn with_transport(mut self, transport: Arc<dyn WorkerTransport>) -> Self {
        self.transport = Some(transport);
        self
    }

    pub fn with_tokens(mut self, tokens: Vec<ApiTokenCredential>) -> Self {
        self.valid_tokens = Some(Arc::new(tokens));
        self.allow_no_auth = false;
        self
    }

    /// Cloudflare Access AUD 설정. 이후 모든 보호된 엔드포인트는
    /// 유효한 CF-Access-Jwt-Assertion 헤더를 요구.
    pub fn with_cf_audience(mut self, aud: impl Into<String>) -> Self {
        self.cf_audience = Some(aud.into());
        self.allow_no_auth = false;
        self
    }

    /// Credentials 암호화 마스터 키 설정. 설정하지 않으면 credentials API
    /// 엔드포인트가 503 Service Unavailable 반환.
    pub fn with_master_key(mut self, key: MasterKey) -> Self {
        self.master_key = Some(Arc::new(key));
        self
    }

    /// CORS 허용 출처 allow-list 설정 (예: `https://console.example.com`).
    ///
    /// 스킴+호스트(+포트)까지 정확히 일치해야 함. 빈 목록이면 CORS 비활성.
    /// 와일드카드(`*`)는 의도적으로 지원하지 않음 — 인증된 API에서
    /// 모든 출처를 허용하면 CSRF/데이터 유출 경로가 열림.
    pub fn with_cors_origins(mut self, origins: Vec<String>) -> Self {
        self.cors_allowed_origins = origins;
        self
    }
}

/// 전체 라우터를 조립. 라우트 구조:
///
/// ```text
/// /
/// ├── health                      → GET /v1/health
/// └── v1
///     └── workers
///         ├── register            → POST
///         ├── heartbeat           → POST
///         ├──                     → GET (list)
///         └── :id                 → GET / DELETE
/// ```
pub fn build_app(state: Arc<AppState>) -> Router {
    let api_routes = Router::new()
        .route("/register", post(handlers::register_worker))
        .route("/join", post(handlers::join_worker))
        .route("/heartbeat", post(handlers::heartbeat))
        .route("/", get(handlers::list_workers))
        .route(
            "/:id",
            get(handlers::get_worker).delete(handlers::deregister_worker),
        );

    // Phase 8.6: worker credentials (sub-resource under /v1/workers/:name/credentials).
    // PUT  /v1/workers/:name/credentials                       — set/rotate
    // GET  /v1/workers/:name/credentials                       — list (metadata only)
    // GET  /v1/workers/:name/credentials/:model_id/export      — decrypted (admin)
    // DELETE /v1/workers/:name/credentials/:model_id           — remove
    let cred_routes = Router::new()
        .route(
            "/",
            axum::routing::put(handlers::put_worker_credential)
                .get(handlers::list_worker_credentials),
        )
        .route("/:model_id/export", get(handlers::export_worker_credential))
        .route(
            "/:model_id",
            axum::routing::delete(handlers::delete_worker_credential),
        );

    // /v1/workers/:name/credentials/* 라우팅을 위해 api_routes에 nest.
    // axum에서는 /:name 하위에 또다른 /:model_id를 두려면 별도 nest가 필요.
    let api_routes = api_routes.nest("/:name/credentials", cred_routes);

    let token_routes = Router::new()
        .route(
            "/",
            post(handlers::create_bootstrap_token).get(handlers::list_bootstrap_tokens),
        )
        .route(
            "/:token",
            axum::routing::delete(handlers::revoke_bootstrap_token),
        );

    let v1 = Router::new()
        .route("/health", get(handlers::health))
        .route("/hosts/register", post(handlers::register_host))
        .nest("/workers", api_routes)
        .nest("/bootstrap-tokens", token_routes);

    // Cloudflare Access 미들웨어 (가장 바깥).
    // 설정된 경우 모든 요청이 CF-Access-Jwt-Assertion 검증을 받음.
    let state_for_cf = state.clone();
    let v1 = if state.cf_audience.is_some() {
        v1.layer(middleware::from_fn(move |req, next| {
            let state = state_for_cf.clone();
            async move { crate::cloudflare::cloudflare_access_middleware(state, req, next).await }
        }))
    } else {
        v1
    };

    // Bearer token 인증 미들웨어 (CF Access 뒤).
    let state_for_auth = state.clone();
    let v1 = v1.layer(middleware::from_fn(move |req, next| {
        let state = state_for_auth.clone();
        async move { auth_middleware(state, req, next).await }
    }));

    Router::new()
        .nest("/v1", v1)
        // /metrics는 인증 미들웨어 바깥에 위치 (Prometheus 스크랩 표준).
        // 단, CF Access가 켜져 있다면 외부망에서는 여전히 CF 토큰 검증을 받음.
        .route(
            "/metrics",
            get({
                let state = state.clone();
                move || {
                    let state = state.clone();
                    async move { crate::metrics::metrics_handler(state).await }
                }
            }),
        )
        // OpenAPI 스펙 (로드맵 #21) — /metrics와 동일한 이유로 인증 미들웨어
        // 바깥: API 스펙 자체는 비밀이 아니고, 클라이언트 도구(Swagger UI 등)가
        // 토큰 없이 바로 불러올 수 있어야 발견성이 생긴다.
        .route("/openapi.yaml", get(openapi_spec))
        // HTTP 지연 기록 — 모든 라우트를 감싼다. 인증 미들웨어보다 바깥이라
        // 인증 실패로 거부된 요청의 지연도 함께 관측된다(부하 판단에 필요).
        .layer({
            let metrics = state.http_metrics.clone();
            middleware::from_fn(move |req, next: middleware::Next| {
                let metrics = metrics.clone();
                async move {
                    let started = std::time::Instant::now();
                    let response = next.run(req).await;
                    metrics.observe(started.elapsed());
                    response
                }
            })
        })
        // 보안 헤더 — 모든 응답에 적용 (이미 설정되지 않은 경우만).
        // fleet-api는 순수 JSON API이므로 CSP는 `default-src 'none'`으로 최소화.
        .layer(SetResponseHeaderLayer::if_not_present(
            HeaderName::from_static("content-security-policy"),
            HeaderValue::from_static(API_CSP),
        ))
        .layer(SetResponseHeaderLayer::if_not_present(
            HeaderName::from_static("x-frame-options"),
            HeaderValue::from_static("DENY"),
        ))
        .layer(SetResponseHeaderLayer::if_not_present(
            HeaderName::from_static("x-content-type-options"),
            HeaderValue::from_static("nosniff"),
        ))
        // HSTS: 평문 HTTP 응답에서는 브라우저가 무시하므로 로컬 개발에 무해.
        .layer(SetResponseHeaderLayer::if_not_present(
            HeaderName::from_static("strict-transport-security"),
            HeaderValue::from_static(API_HSTS),
        ))
        .layer(SetResponseHeaderLayer::if_not_present(
            HeaderName::from_static("referrer-policy"),
            HeaderValue::from_static("no-referrer"),
        ))
        .layer(TraceLayer::new_for_http())
        // CORS — 명시적 allow-list. 기본값(빈 목록)은 CORS 헤더를 내보내지 않아
        // 브라우저 교차 출처 요청이 차단됨 (permissive 제거).
        .layer(cors_layer(&state.cors_allowed_origins))
        .with_state(state)
}

/// `/v1` HTTP API의 OpenAPI 3.0.3 스펙 (로드맵 #21). `schema.rs`/`handlers.rs`
/// 기준으로 수기 작성 — 코드가 바뀌면 이 파일도 함께 갱신해야 한다(자동
/// 생성 아님). 대시보드 API(`/api/*`, `fleet-dashboard` 크레이트)는 별도
/// 크레이트이자 훨씬 큰 표면(~30개 라우트)이라 이 스펙의 범위 밖이다.
const OPENAPI_YAML: &str = include_str!("openapi.yaml");

/// `GET /openapi.yaml` 핸들러. `/metrics`와 동일하게 인증 미들웨어 바깥에
/// 등록되어 토큰 없이도 조회 가능.
async fn openapi_spec() -> impl axum::response::IntoResponse {
    (
        [(header::CONTENT_TYPE, "application/yaml; charset=utf-8")],
        OPENAPI_YAML,
    )
}

/// JSON 전용 API용 CSP — 스크립트/이미지/프레임 등 모든 서브리소스 로드 금지.
const API_CSP: &str =
    "default-src 'none'; frame-ancestors 'none'; base-uri 'none'; form-action 'none'";

/// HSTS 2년 + 서브도메인 + preload.
const API_HSTS: &str = "max-age=63072000; includeSubDomains; preload";

/// 설정된 allow-list로 `CorsLayer` 구성.
///
/// - 빈 목록 → `CorsLayer::new()` (Access-Control-* 헤더 없음 = 교차 출처 차단)
/// - 와일드카드(`*`)와 파싱 불가한 출처는 경고 로그와 함께 무시 (fail-closed)
/// - `allow_credentials`는 켜지 않음 — fleet-api는 쿠키가 아닌
///   Authorization 헤더 기반 인증이므로 자격 증명 포함 요청이 필요 없음
fn cors_layer(origins: &[String]) -> CorsLayer {
    let mut parsed: Vec<HeaderValue> = Vec::new();
    for origin in origins {
        let trimmed = origin.trim();
        if trimmed.is_empty() {
            continue;
        }
        if trimmed == "*" {
            tracing::warn!("CORS wildcard '*' rejected for an authenticated API — ignored");
            continue;
        }
        // Origin 헤더는 정확히 `scheme://host[:port]` 형태. 경로/트레일링 슬래시가
        // 붙으면 어떤 요청과도 매칭되지 않아 조용한 오설정이 되므로 미리 거부.
        let looks_like_origin = (trimmed.starts_with("https://") || trimmed.starts_with("http://"))
            && !trimmed.ends_with('/')
            && trimmed.matches('/').count() == 2;
        if !looks_like_origin {
            tracing::warn!(
                origin = %trimmed,
                "CORS origin must be exactly 'scheme://host[:port]' — ignored"
            );
            continue;
        }
        match HeaderValue::from_str(trimmed) {
            Ok(value) => parsed.push(value),
            Err(e) => {
                tracing::warn!(origin = %trimmed, error = %e, "invalid CORS origin — ignored");
            }
        }
    }

    if parsed.is_empty() {
        return CorsLayer::new();
    }

    info!(
        count = parsed.len(),
        "CORS enabled with explicit origin allow-list"
    );
    CorsLayer::new()
        .allow_origin(parsed)
        .allow_methods([Method::GET, Method::POST, Method::PUT, Method::DELETE])
        .allow_headers([header::AUTHORIZATION, header::CONTENT_TYPE])
}

/// Bearer token 인증 미들웨어.
///
/// - `allow_no_auth == true`면 통과
/// - Cloudflare Access가 설정된 경우 bearer 없이 CF middleware의 인증에 맡김
/// - 그 외 `valid_tokens == None`이면 fail-closed
/// - 그 외에는 `Authorization: Bearer <token>` 헤더가 `valid_tokens`에 있어야 함
async fn auth_middleware(
    state: Arc<AppState>,
    req: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    if state.allow_no_auth {
        let mut req = req;
        req.extensions_mut().insert(AuthorizationContext {
            principal_id: "development".into(),
            authentication_method: AuthenticationMethod::DevelopmentNoAuth,
            capabilities: PermissionKind::all().to_vec(),
            worker_id: None,
        });
        authorize_http_endpoint(&req)?;
        return Ok(next.run(req).await);
    }

    // health 엔드포인트는 인증 provider 구성 여부와 관계없이 LB 프로브에 허용한다.
    if req.uri().path() == "/v1/health" {
        return Ok(next.run(req).await);
    }

    // Worker operational credential 조회 — join에서 발급된 `fwo_...` 토큰인지
    // 먼저 확인한다. `valid_tokens`/`cf_audience` 설정 여부와 무관하게 동작해야
    // 하므로(워커는 admin bearer allow-list에 없다) 아래 관리자 bearer 분기보다
    // 앞서 검사한다. 매치되지 않으면 조용히 기존 로직으로 폴백한다.
    if let Some(header) = req
        .headers()
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
    {
        if let Some(token) = header
            .strip_prefix("Bearer ")
            .or_else(|| header.strip_prefix("bearer "))
        {
            let digest = fleet_core::BootstrapToken::digest_for(token);
            if let Ok(Some(credential)) =
                state.store.find_active_worker_operational_credential(&digest).await
            {
                let mut req = req;
                req.extensions_mut().insert(AuthorizationContext {
                    principal_id: format!("worker:{}", credential.worker_id),
                    authentication_method: AuthenticationMethod::WorkerOperational,
                    capabilities: vec![PermissionKind::WorkerRegister, PermissionKind::WorkerDelete],
                    worker_id: Some(credential.worker_id),
                });
                authorize_http_endpoint(&req)?;
                return Ok(next.run(req).await);
            }
        }
    }

    let Some(tokens) = &state.valid_tokens else {
        // CF Access 전용 배포는 별도 middleware가 인증을 담당한다. 이 경로의
        // principal extraction은 CF claims → AuthorizationContext 단계에서 추가한다.
        if state
            .cf_audience
            .as_deref()
            .is_some_and(|audience| !audience.trim().is_empty())
        {
            return Ok(next.run(req).await);
        }
        tracing::error!(path = %req.uri().path(), "protected API has no authentication provider");
        return Err(StatusCode::UNAUTHORIZED);
    };

    let auth_header = req
        .headers()
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok());

    let Some(header) = auth_header else {
        tracing::warn!(path = %req.uri().path(), "missing Authorization header");
        return Err(StatusCode::UNAUTHORIZED);
    };

    let token = header
        .strip_prefix("Bearer ")
        .or_else(|| header.strip_prefix("bearer "));

    let Some(token) = token else {
        tracing::warn!(path = %req.uri().path(), "malformed Authorization header");
        return Err(StatusCode::UNAUTHORIZED);
    };

    if let Some(credential) = token_matches(tokens, token) {
        let mut req = req;
        req.extensions_mut().insert(AuthorizationContext {
            principal_id: credential.principal_id.clone(),
            authentication_method: AuthenticationMethod::ScopedBearer,
            capabilities: credential.capabilities.clone(),
            worker_id: None,
        });
        authorize_http_endpoint(&req)?;
        Ok(next.run(req).await)
    } else {
        tracing::warn!(path = %req.uri().path(), "invalid bearer token");
        Err(StatusCode::UNAUTHORIZED)
    }
}

/// 현재 `/v1` route 표면의 최소 capability 행렬. 등록되지 않은 보호 route는
/// fail-closed 한다. Worker self identity와 Project scope는 후속 identity 단계에서 추가한다.
fn authorize_http_endpoint(req: &Request) -> Result<(), StatusCode> {
    let path = req.uri().path();
    if path == "/v1/health" {
        return Ok(());
    }
    let required = match (req.method(), path) {
        (&Method::GET, "/v1/workers") => PermissionKind::WorkerList,
        (&Method::POST, "/v1/workers/register") | (&Method::POST, "/v1/workers/heartbeat") => {
            PermissionKind::WorkerRegister
        }
        (&Method::DELETE, path) if path.starts_with("/v1/workers/") => PermissionKind::WorkerDelete,
        (&Method::POST, "/v1/bootstrap-tokens") => PermissionKind::TokenIssue,
        (&Method::GET, "/v1/bootstrap-tokens") => PermissionKind::TokenList,
        (&Method::DELETE, path) if path.starts_with("/v1/bootstrap-tokens/") => {
            PermissionKind::TokenRevoke
        }
        // Worker join은 bootstrap token을 자체 인증 수단으로 사용한다. 이 route의 bearer
        // bypass/worker identity 전환은 #60에서 별도로 분리한다.
        _ => return Ok(()),
    };
    let authorized = req
        .extensions()
        .get::<AuthorizationContext>()
        .is_some_and(|context| context.capabilities.contains(&required));
    if authorized {
        Ok(())
    } else {
        tracing::warn!(path, capability = required.as_str(), "HTTP capability denied");
        Err(StatusCode::FORBIDDEN)
    }
}

/// Bearer 토큰을 allow-list와 상수시간으로 비교.
///
/// 타이밍 공격 방어를 위한 두 가지 속성:
/// 1. 두 값을 SHA-256으로 축약한 뒤 고정 길이(32바이트) 상수시간 비교 —
///    바이트 단위 조기 종료도, 토큰 길이 누출도 발생하지 않음.
/// 2. 일치하는 토큰을 찾아도 조기 반환하지 않고 목록 전체를 순회 —
///    allow-list 내 위치(몇 번째 토큰인지)가 응답 시간으로 누출되지 않음.
fn token_matches<'a>(
    candidates: &'a [ApiTokenCredential],
    presented: &str,
) -> Option<&'a ApiTokenCredential> {
    let presented_digest = Sha256::digest(presented.as_bytes());
    let mut matched = Choice::from(0u8);
    let mut match_index = None;
    for (index, candidate) in candidates.iter().enumerate() {
        let candidate_digest = Sha256::digest(candidate.token.as_bytes());
        let is_match = presented_digest.ct_eq(candidate_digest.as_slice());
        if bool::from(is_match) {
            match_index = Some(index);
        }
        matched |= is_match;
    }
    bool::from(matched).then(|| &candidates[match_index.expect("matched token has index")])
}

/// 서버 바인딩 + serve. shutdown 시그널은 호출자가 처리.
pub async fn run_http_server(state: Arc<AppState>, bind: SocketAddr) -> std::io::Result<()> {
    let app = build_app(state);
    let listener = tokio::net::TcpListener::bind(bind).await?;
    info!(%bind, "HTTP API server listening");
    axum::serve(listener, app).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use fleet_store::mem::MemStore;
    use fleet_core::CircuitState;
    use std::sync::Arc;

    fn test_token(token: &str) -> ApiTokenCredential {
        ApiTokenCredential {
            principal_id: "test".into(),
            token: token.into(),
            capabilities: PermissionKind::all().to_vec(),
        }
    }

    #[tokio::test]
    async fn app_state_defaults_to_no_auth() {
        let store = MemStore::new_arc();
        let state = AppState::new(store);
        assert!(state.allow_no_auth);
        assert!(state.valid_tokens.is_none());
        assert_eq!(state.heartbeat_interval_secs, 15);
    }

    #[tokio::test]
    async fn app_state_with_tokens_disables_no_auth() {
        let store = MemStore::new_arc();
        let state = AppState::new(store).with_tokens(vec![test_token("secret")]);
        assert!(!state.allow_no_auth);
        assert_eq!(state.valid_tokens.as_ref().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn protected_state_without_auth_provider_is_rejected() {
        let mut state = AppState::new(MemStore::new_arc());
        state.allow_no_auth = false;
        let app = build_app(Arc::new(state));
        let response = tower::ServiceExt::oneshot(
            app,
            axum::http::Request::builder()
                .uri("/v1/workers")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn build_app_smoke() {
        let store = MemStore::new_arc();
        let state = Arc::new(AppState::new(store));
        let _router = build_app(state);
        // 빌드가 성공하면 OK — 라우터 구성 검증.
    }

    #[test]
    fn token_matches_exact_token() {
        let tokens = vec![test_token("alpha"), test_token("bravo")];
        assert!(token_matches(&tokens, "alpha").is_some());
        assert!(token_matches(&tokens, "bravo").is_some());
    }

    #[test]
    fn token_matches_rejects_unknown_and_prefixes() {
        let tokens = vec![test_token("secret-token")];
        assert!(token_matches(&tokens, "secret").is_none());
        assert!(token_matches(&tokens, "secret-token-extra").is_none());
        assert!(token_matches(&tokens, "Secret-Token").is_none());
        assert!(token_matches(&tokens, "").is_none());
    }

    #[test]
    fn token_matches_empty_allow_list_rejects_everything() {
        assert!(token_matches(&[], "anything").is_none());
        assert!(token_matches(&[], "").is_none());
    }

    #[tokio::test]
    async fn app_state_cors_origins_default_empty() {
        let store = MemStore::new_arc();
        let state = AppState::new(store);
        assert!(state.cors_allowed_origins.is_empty());
    }

    /// `GET /v1/health` 요청을 라우터에 흘려보내고 응답을 반환.
    async fn health_response(state: AppState, origin: Option<&str>) -> Response {
        use tower::ServiceExt;
        let app = build_app(Arc::new(state));
        let mut builder = axum::http::Request::builder()
            .method(axum::http::Method::GET)
            .uri("/v1/health");
        if let Some(o) = origin {
            builder = builder.header("origin", o);
        }
        let req = builder.body(axum::body::Body::empty()).unwrap();
        app.oneshot(req).await.unwrap()
    }

    #[tokio::test]
    async fn responses_carry_security_headers() {
        let state = AppState::new(MemStore::new_arc());
        let resp = health_response(state, None).await;
        let headers = resp.headers();

        assert_eq!(headers.get("content-security-policy").unwrap(), API_CSP);
        assert_eq!(headers.get("x-frame-options").unwrap(), "DENY");
        assert_eq!(headers.get("x-content-type-options").unwrap(), "nosniff");
        assert_eq!(headers.get("strict-transport-security").unwrap(), API_HSTS);
        assert_eq!(headers.get("referrer-policy").unwrap(), "no-referrer");
    }

    #[tokio::test]
    async fn cors_disabled_by_default_for_cross_origin_request() {
        let state = AppState::new(MemStore::new_arc());
        let resp = health_response(state, Some("https://evil.example.com")).await;
        // permissive였다면 Access-Control-Allow-Origin이 존재했을 것.
        assert!(resp.headers().get("access-control-allow-origin").is_none());
    }

    #[tokio::test]
    async fn cors_allow_list_echoes_only_listed_origin() {
        let allowed = "https://console.example.com";
        let state = AppState::new(MemStore::new_arc()).with_cors_origins(vec![allowed.to_string()]);
        let resp = health_response(state, Some(allowed)).await;
        assert_eq!(
            resp.headers().get("access-control-allow-origin").unwrap(),
            allowed
        );

        let state = AppState::new(MemStore::new_arc()).with_cors_origins(vec![allowed.to_string()]);
        let resp = health_response(state, Some("https://evil.example.com")).await;
        assert!(resp.headers().get("access-control-allow-origin").is_none());
    }

    #[tokio::test]
    async fn cors_wildcard_entry_is_ignored() {
        let state = AppState::new(MemStore::new_arc()).with_cors_origins(vec!["*".to_string()]);
        let resp = health_response(state, Some("https://evil.example.com")).await;
        assert!(resp.headers().get("access-control-allow-origin").is_none());
    }

    #[tokio::test]
    async fn cors_malformed_origin_entries_are_ignored() {
        // 경로 포함 / 트레일링 슬래시 / 스킴 누락은 조용한 오설정이 되므로 거부.
        for bad in [
            "https://console.example.com/",
            "https://console.example.com/app",
            "console.example.com",
        ] {
            let state = AppState::new(MemStore::new_arc()).with_cors_origins(vec![bad.to_string()]);
            let resp = health_response(state, Some("https://console.example.com")).await;
            assert!(
                resp.headers().get("access-control-allow-origin").is_none(),
                "origin entry {bad} should have been rejected"
            );
        }
    }

    #[tokio::test]
    async fn bearer_auth_accepts_valid_and_rejects_invalid_token() {
        use tower::ServiceExt;

        async fn call(token: Option<&str>) -> StatusCode {
            let state = Arc::new(
                AppState::new(MemStore::new_arc()).with_tokens(vec![test_token("good-token")]),
            );
            let app = build_app(state);
            let mut builder = axum::http::Request::builder()
                .method(axum::http::Method::GET)
                .uri("/v1/workers");
            if let Some(t) = token {
                builder = builder.header("authorization", format!("Bearer {t}"));
            }
            let req = builder.body(axum::body::Body::empty()).unwrap();
            app.oneshot(req).await.unwrap().status()
        }

        assert_ne!(call(Some("good-token")).await, StatusCode::UNAUTHORIZED);
        assert_eq!(call(Some("good-toke")).await, StatusCode::UNAUTHORIZED);
        assert_eq!(call(Some("good-token ")).await, StatusCode::UNAUTHORIZED);
        assert_eq!(call(Some("")).await, StatusCode::UNAUTHORIZED);
        assert_eq!(call(None).await, StatusCode::UNAUTHORIZED);
    }

    #[test]
    fn circuit_state_unused_marker() {
        // CircuitState가 이 모듈에서 미사용이더라도 다른 곳에서 쓰이므로 re-export
        let _ = CircuitState::Closed;
    }

    // ── OpenAPI 스펙 (로드맵 #21) ────────────────────────────────────────

    #[test]
    fn openapi_yaml_is_valid_and_covers_known_paths() {
        let doc: serde_yaml::Value =
            serde_yaml::from_str(OPENAPI_YAML).expect("openapi.yaml must be valid YAML");
        assert_eq!(
            doc["openapi"].as_str(),
            Some("3.0.3"),
            "unexpected or missing openapi version field"
        );
        let paths = doc["paths"]
            .as_mapping()
            .expect("paths must be a mapping");
        for expected in [
            "/health",
            "/workers/register",
            "/workers/join",
            "/workers/heartbeat",
            "/workers",
            "/workers/{id}",
            "/workers/{name}/credentials",
            "/bootstrap-tokens",
            "/bootstrap-tokens/{token_id}",
            "/hosts/register",
            "/metrics",
        ] {
            assert!(
                paths.contains_key(serde_yaml::Value::String(expected.to_string())),
                "openapi.yaml is missing path: {expected}"
            );
        }
    }

    #[tokio::test]
    async fn openapi_yaml_route_is_reachable_without_auth() {
        use tower::ServiceExt;
        // 토큰이 설정돼 있어도 /openapi.yaml은 인증 없이 조회 가능해야 한다
        // (/metrics와 동일한 이유 — 발견성).
        let state = AppState::new(MemStore::new_arc()).with_tokens(vec![test_token("secret")]);
        let app = build_app(Arc::new(state));
        let req = axum::http::Request::builder()
            .method(axum::http::Method::GET)
            .uri("/openapi.yaml")
            .body(axum::body::Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let content_type = resp
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or_default()
            .to_string();
        assert!(
            content_type.contains("yaml"),
            "unexpected content-type: {content_type}"
        );

        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        assert!(String::from_utf8_lossy(&body).starts_with("openapi: 3.0.3"));
    }
}
