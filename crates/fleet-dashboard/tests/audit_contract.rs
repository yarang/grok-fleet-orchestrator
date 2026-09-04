//! 대시보드 mutation route의 감사 계약 (로드맵 #95 3단계).
//!
//! `#95`가 고치려는 결함은 "감사 코드가 없다"가 아니라 **감사가 계약이
//! 아니라 관습이었다**는 것이다. 31개 mutation route는 전부
//! `require_permission`을 부른다 — 그건 함수 시그니처가 강제하는 계약이라
//! 빠뜨릴 수가 없다. 반면 `crate::audit::record`는 아무도 강제하지 않아서
//! 20개만 부르고 11개는 부르지 않았고, 그 사실을 아무도 몰랐다. 관습은
//! 사람의 기억에 비례해 낡는다.
//!
//! 그래서 이 파일은 개별 핸들러가 아니라 **route 집합**을 잠근다. 아래 표에
//! 없는 mutation route가 `app.rs`에 생기면 실패하고, 표에 있는데 `app.rs`에
//! 없어도 실패한다. 새 mutation route를 추가하는 사람은 그 route가 무엇을
//! 감사하는지 **적어야만** 컴파일 게이트를 통과한다.
//!
//! # "mutation"의 정의
//!
//! 스캐너는 **non-GET route**를 mutation으로 센다. 그 정의는 하나를
//! 빗나가는데, 상태를 바꾸면서 GET인 route가 실재하기 때문이다
//! (`GET /verify-email`). 그 예외는 `STATE_CHANGING_GET_ROUTES`가 따로
//! 담는다. 여기서 배울 것은 정의를 넓히면 되는 문제가 아니라는 점이다 —
//! "본문이 상태를 바꾸는가"는 소스 스캔이 판정할 수 없고, 판정하려 들면
//! 아래 *이 테스트가 확인하지 않는 것*이 말하는 본문 해석의 함정으로 곧장
//! 들어간다. 그래서 값싼 정의를 쓰고 **예외를 눈에 보이게** 적었다.
//!
//! # 이 테스트가 확인하지 않는 것
//!
//! 이 파일은 **route가 분류됐는지**만 본다. 그 핸들러가 실제로 감사 행을
//! 남기는지, 남긴다면 모든 경로에서 남기는지는 보지 않는다 — 그건
//! `dashboard_api.rs`의 런타임 테스트가 HTTP로 요청을 보내고
//! `list_audit_events`로 확인하는 몫이다. 두 겹을 나눈 이유는 소스 스캔으로
//! "모든 실행 경로가 기록하는가"를 판정하려면 함수 본문을 해석해야 하는데,
//! 이 저장소에는 그 해석이 곧바로 틀리는 자리가 이미 두 곳 있기 때문이다:
//! `provision_host_api`는 헬퍼 `run_provisioning` 안에서 기록하고,
//! `delete_project_api`는 action을 `match`로 계산해 한 요청에 행을 **두 개**
//! 남길 수 있다. 본문 스캔은 이 둘을 예외로 파야 하고, 예외가 쌓이는
//! 테스트는 시간이 지날수록 틀려진다.
//!
//! # 감사하지 않기로 한 route가 생긴다면
//!
//! 지금은 31개 전부가 감사한다. 그래서 표에 "면제" 칸을 두지 않았다 —
//! 아무도 만들지 않는 상태를 미리 만들면 그 칸은 검증되지 않은 채로 남는다.
//! 면제가 필요해지는 날 표의 **형태를 바꿔야 하고**, 그 순간이 곧 왜
//! 면제인지를 기록할 자리다.

use std::path::PathBuf;

/// `(HTTP 메서드, 경로, 그 route가 남기는 감사 action 상수 이름들)`.
///
/// action이 여럿인 자리는 요청 하나가 행을 여러 개 남길 수 있다는 뜻이다
/// (`DELETE /api/projects/:id`가 Draining과 Archived를 각각 기록한다).
const MUTATION_ROUTES: &[(&str, &str, &[&str])] = &[
    ("POST", "/api/agent-templates", &["AGENT_TEMPLATE_CREATE"]),
    (
        "POST",
        "/api/agent-templates/:id/revisions",
        &["AGENT_TEMPLATE_REVISION_CREATE"],
    ),
    (
        "POST",
        "/api/agent-templates/:id/revisions/:revision_id/revoke",
        &["AGENT_TEMPLATE_REVISION_REVOKE"],
    ),
    (
        "POST",
        "/api/agent-templates/:id/status",
        &["AGENT_TEMPLATE_STATUS_CHANGE"],
    ),
    ("POST", "/api/agents", &["AGENT_CREATE"]),
    ("DELETE", "/api/agents/:id", &["AGENT_STOP"]),
    ("POST", "/api/agents/:id/place", &["AGENT_ASSIGN"]),
    ("POST", "/api/agents/:id/start", &["AGENT_START"]),
    ("POST", "/api/hosts/provision", &["HOST_PROVISION"]),
    ("POST", "/api/issues", &["ISSUE_CREATE"]),
    ("PATCH", "/api/issues/:id", &["ISSUE_UPDATE"]),
    ("POST", "/api/issues/:id/comments", &["ISSUE_COMMENT"]),
    ("POST", "/api/issues/:id/links", &["ISSUE_LINK"]),
    (
        "DELETE",
        "/api/issues/:id/links/:task_id",
        &["ISSUE_UNLINK"],
    ),
    ("POST", "/api/issues/:id/transition", &["ISSUE_TRANSITION"]),
    ("POST", "/api/projects", &["PROJECT_CREATE"]),
    (
        "DELETE",
        "/api/projects/:id",
        &["PROJECT_ARCHIVE_REQUESTED", "PROJECT_ARCHIVED"],
    ),
    ("POST", "/api/ssh-keys", &["SSH_KEY_CREATE"]),
    ("DELETE", "/api/ssh-keys/:name", &["SSH_KEY_DELETE"]),
    ("POST", "/api/tasks", &["TASK_SUBMIT"]),
    ("DELETE", "/api/tasks/:id", &["TASK_DELETE"]),
    ("POST", "/api/users", &["USER_CREATE"]),
    ("POST", "/api/users/:id/delete", &["USER_DELETE"]),
    ("POST", "/api/users/:id/toggle", &["USER_TOGGLE"]),
    (
        "POST",
        "/api/users/resend-verification",
        &["AUTH_VERIFICATION_RESENT"],
    ),
    ("POST", "/bootstrap", &["AUTH_BOOTSTRAP"]),
    (
        "POST",
        "/forgot-password",
        &["AUTH_PASSWORD_RESET_REQUESTED"],
    ),
    ("POST", "/login", &["AUTH_LOGIN"]),
    ("POST", "/logout", &["AUTH_LOGOUT"]),
    (
        "POST",
        "/resend-verification",
        &["AUTH_VERIFICATION_RESENT"],
    ),
    ("POST", "/reset-password", &["AUTH_PASSWORD_RESET"]),
];

/// 상태를 바꾸는데 **GET인** route.
///
/// 위 표의 스캐너는 "non-GET이면 mutation"이라는 정의로 돌아간다. 그 정의는
/// 값싸고 대부분 맞지만, 하나를 놓친다 — 메일로 보낸 링크를 클릭해서
/// 도달하는 확인 페이지다. 링크는 GET일 수밖에 없고(메일 클라이언트가
/// POST를 만들 수 없다), 그런데도 `verify_email_page`는 토큰을 소비하고
/// `users.email_verified`를 세운다. 즉 **HTTP 메서드는 상태 변경 여부의
/// 근사값이지 정의가 아니다.**
///
/// 이 표를 따로 둔 이유는 `MUTATION_ROUTES`에 `("GET", ...)` 행을 넣을 수
/// 없기 때문이다. 그쪽의 `stale` 검사는 스캔 결과에서 GET을 **걸러낸 뒤**
/// 비교하므로, GET 행을 넣으면 즉시 "app.rs에 없는 route"로 실패한다.
/// 정의가 다른 두 집합은 표도 달라야 한다.
///
/// 여기 있는 route는 자동으로 발견되지 않는다 — 사람이 적어야 한다. 그래서
/// 이 표는 `MUTATION_ROUTES`보다 약하다: 새로 생긴 상태 변경 GET을 잡아내지
/// 못한다. 잡아내는 것은 경로가 사라지거나 그 action이 지워지는 쪽이다.
const STATE_CHANGING_GET_ROUTES: &[(&str, &str, &[&str])] =
    &[("GET", "/verify-email", &["AUTH_EMAIL_VERIFIED"])];

/// axum `MethodRouter` 생성자 이름. GET만 mutation이 아니다.
const VERBS: &[&str] = &[
    "get", "post", "put", "patch", "delete", "head", "options", "trace",
];

fn manifest(rel: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(rel)
}

fn read(rel: &str) -> String {
    let path = manifest(rel);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{} 읽기 실패: {e}", path.display()))
}

/// `//`부터 줄 끝까지를 지운다.
///
/// `app.rs`의 주석에는 route 경로를 인용한 한국어 설명이 있다(예: `/api/audit`이
/// `/api/events`와 어떻게 다른지). 그 설명을 route로 세면 표에 없는 유령
/// 경로가 생겨 이 테스트가 **거짓으로 실패**한다.
///
/// 앞이 `:`인 `//`는 건너뛴다 — `https://`의 뒷부분을 먹어서 진짜 위반을
/// **감추는** 쪽으로 고장 나면 안 된다.
fn strip_line_comments(src: &str) -> String {
    src.lines()
        .map(|line| {
            let bytes = line.as_bytes();
            for i in 0..bytes.len().saturating_sub(1) {
                if bytes[i] == b'/' && bytes[i + 1] == b'/' && (i == 0 || bytes[i - 1] != b':') {
                    return &line[..i];
                }
            }
            line
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// `open` 위치의 `(`와 짝을 이루는 `)`의 오프셋.
fn match_paren(src: &str, open: usize) -> usize {
    let bytes = src.as_bytes();
    let mut depth = 0usize;
    for (i, b) in bytes.iter().enumerate().skip(open) {
        match *b {
            b'(' => depth += 1,
            b')' => {
                depth -= 1;
                if depth == 0 {
                    return i;
                }
            }
            _ => {}
        }
    }
    panic!("`.route(`의 괄호가 닫히지 않았다 (offset {open})");
}

/// 인자 목록에서 axum 메서드 생성자 이름을 뽑는다.
///
/// 식별자 경계를 직접 본다. `delete_ssh_key_api(`는 `delete` 뒤가 `_`라
/// 걸리지 않고, `axum::routing::delete(`는 앞이 `:`라 걸린다 — 이 저장소는
/// 두 표기를 **섞어 쓰므로** 앞이 `.`인 경우만 보면 안 된다.
fn scan_verbs(args: &str) -> Vec<String> {
    let bytes = args.as_bytes();
    let is_ident = |b: u8| b.is_ascii_alphanumeric() || b == b'_';
    let mut found = Vec::new();
    for verb in VERBS {
        let mut from = 0usize;
        while let Some(hit) = args[from..].find(verb) {
            let start = from + hit;
            let end = start + verb.len();
            from = end;
            let boundary_before = start == 0 || !is_ident(bytes[start - 1]);
            let call_after = args[end..].trim_start().starts_with('(');
            if boundary_before && call_after {
                found.push((start, (*verb).to_string()));
            }
        }
    }
    found.sort();
    found.into_iter().map(|(_, v)| v).collect()
}

/// `app.rs`에 등록된 `(메서드, 경로)`를 전부 뽑는다.
///
/// **모르는 표기를 만나면 건너뛰지 않고 실패한다.** 파서가 route를 인식하지
/// 못하면 이 테스트는 실패하는 게 아니라 조용히 통과하고, 그 route는 계약
/// 밖으로 사라진다. 실제로 이 저장소의 `axum::routing::delete(...)` 두 자리가
/// 그렇게 빠질 뻔했다. 모든 `.route()`는 최소 한 개의 method handler를
/// 가지므로, 0개는 route가 없다는 뜻이 아니라 파서가 낡았다는 뜻이다.
fn scan_routes(src: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut idx = 0usize;
    while let Some(hit) = src[idx..].find(".route(") {
        let open = idx + hit + ".route(".len() - 1;
        let close = match_paren(src, open);
        let args = &src[open + 1..close];
        let after_quote = args
            .trim_start()
            .strip_prefix('"')
            .unwrap_or_else(|| panic!("`.route(`의 첫 인자가 문자열 리터럴이 아니다: {args}"));
        let end = after_quote
            .find('"')
            .unwrap_or_else(|| panic!("경로 리터럴이 닫히지 않았다: {args}"));
        let path = after_quote[..end].to_string();
        let rest = &after_quote[end + 1..];
        let verbs = scan_verbs(rest);
        assert!(
            !verbs.is_empty(),
            "`.route(\"{path}\", …)`에서 메서드를 하나도 인식하지 못했다. \
             0개는 route가 없다는 뜻이 아니라 이 파서가 새 표기를 못 읽는다는 \
             뜻이다 — VERBS나 scan_verbs를 고쳐야 한다. 인자 원문:{rest}"
        );
        for verb in verbs {
            out.push((verb.to_uppercase(), path.clone()));
        }
        idx = close;
    }
    out
}

/// `app.rs`의 mutation route 집합과 위 표가 정확히 일치해야 한다.
#[test]
fn every_mutation_route_is_classified() {
    let src = strip_line_comments(&read("src/app.rs"));
    let mut found: Vec<(String, String)> = scan_routes(&src)
        .into_iter()
        .filter(|(verb, _)| verb != "GET")
        .collect();
    found.sort();

    for pair in found.windows(2) {
        assert_ne!(
            pair[0], pair[1],
            "같은 route가 두 번 등록됐다 — axum이 런타임에 panic한다"
        );
    }

    let mut expected: Vec<(String, String)> = MUTATION_ROUTES
        .iter()
        .map(|(m, p, _)| ((*m).to_string(), (*p).to_string()))
        .collect();
    expected.sort();

    let unclassified: Vec<_> = found.iter().filter(|r| !expected.contains(r)).collect();
    let stale: Vec<_> = expected.iter().filter(|r| !found.contains(r)).collect();

    assert!(
        unclassified.is_empty(),
        "`app.rs`에 있으나 MUTATION_ROUTES에 없는 route: {unclassified:?}\n\
         새 mutation route는 무엇을 감사하는지 표에 적어야 한다. 감사하지 \
         않기로 했다면 이 파일 상단 주석의 '면제' 항목을 읽어라."
    );
    assert!(
        stale.is_empty(),
        "MUTATION_ROUTES에 있으나 `app.rs`에 없는 route: {stale:?}\n\
         route가 사라졌거나 이름이 바뀌었다 — 표를 따라가야 한다."
    );
}

/// 상태를 바꾸는 GET route가 여전히 GET으로 등록돼 있어야 한다.
///
/// `MUTATION_ROUTES` 쪽의 `stale` 검사에 대응하는 짝이다. 경로가 사라지거나
/// 이름이 바뀌면 여기서 깨진다 — 그때가 이 route를 계속 예외로 둘지 다시
/// 판단할 자리다. 메서드가 POST로 바뀌면 여기서 깨지고, 동시에
/// `every_mutation_route_is_classified`가 "표에 없는 mutation route"로도
/// 깨진다. 두 번 깨지는 것은 중복이 아니라 어느 표로 옮겨야 하는지를
/// 말해 주는 신호다.
#[test]
fn state_changing_get_routes_are_still_registered_as_get() {
    let src = strip_line_comments(&read("src/app.rs"));
    let found = scan_routes(&src);
    for (method, path, _) in STATE_CHANGING_GET_ROUTES {
        assert_eq!(*method, "GET", "{path}: 이 표는 GET만 담는다");
        assert!(
            found.contains(&("GET".to_string(), (*path).to_string())),
            "`app.rs`에 `GET {path}`가 없다 — 경로가 바뀌었거나 사라졌다. \
             사라졌다면 이 표에서도 지워라."
        );
    }
}

/// 표가 이름 붙인 action이 실재하고, 대시보드 코드가 실제로 그 이름을 쓴다.
///
/// 오타와 유물을 잡는다. "핸들러가 그 action을 남기는가"는 보지 않는다 —
/// 이 파일 상단의 *이 테스트가 확인하지 않는 것*을 보라.
#[test]
fn every_declared_action_exists_and_is_referenced() {
    let core = read("../fleet-core/src/audit.rs");
    let dir = manifest("src");
    let mut dashboard = String::new();
    for entry in std::fs::read_dir(&dir).expect("src 디렉터리를 읽을 수 없다") {
        let path = entry.expect("디렉터리 항목").path();
        if path.extension().is_some_and(|e| e == "rs") {
            dashboard.push_str(&std::fs::read_to_string(&path).expect("소스 읽기"));
        }
    }

    for (method, path, actions) in MUTATION_ROUTES.iter().chain(STATE_CHANGING_GET_ROUTES) {
        assert!(
            !actions.is_empty(),
            "{method} {path}: 감사 action이 비어 있다"
        );
        for action in *actions {
            assert!(
                core.contains(&format!("pub const {action}:")),
                "{method} {path}: `fleet_core::audit::action::{action}`이 선언되어 있지 않다"
            );
            assert!(
                dashboard.contains(&format!("action::{action}")),
                "{method} {path}: 대시보드 코드 어디에서도 `action::{action}`을 쓰지 않는다"
            );
        }
    }
}
