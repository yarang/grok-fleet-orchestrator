//! `examples/fleet.env`가 실제 파서로 검증되는지 확인 (로드맵 #77).
//!
//! `FLEET_API_TOKENS`는 한때 평면 쉼표 문자열이었으나 principal·capability를
//! 표현하지 못해 `{principal_id, token, capabilities}` JSON 배열로 전환됐다.
//! 그 뒤로도 예시 파일이 옛 형식을 안내하고 있어, 저장소 예시를 그대로 따르면
//! `fleet serve`가 기동조차 하지 못하는 상태가 유지됐다. 이 테스트는 예시가
//! 파서와 다시 갈라지는 것을 막는다.

use std::path::PathBuf;

use fleet_api::ApiTokenCredential;

fn example_env() -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("examples")
        .join("fleet.env");
    assert!(
        path.exists(),
        "examples/fleet.env not found at {}",
        path.display()
    );
    std::fs::read_to_string(&path).expect("examples/fleet.env must be readable")
}

/// 주석이 아닌 `KEY=value` 라인에서 값을 뽑는다.
fn env_value(contents: &str, key: &str) -> Option<String> {
    contents
        .lines()
        .map(str::trim)
        .filter(|line| !line.starts_with('#'))
        .find_map(|line| line.strip_prefix(&format!("{key}=")))
        .map(str::to_string)
}

#[test]
fn examples_fleet_env_api_tokens_parse_as_manifest() {
    let contents = example_env();
    let raw = env_value(&contents, "FLEET_API_TOKENS")
        .expect("examples/fleet.env must define FLEET_API_TOKENS");

    let tokens: Vec<ApiTokenCredential> = serde_json::from_str(&raw).unwrap_or_else(|e| {
        panic!(
            "examples/fleet.env FLEET_API_TOKENS must be a JSON array of \
             {{principal_id, token, capabilities}} — `fleet serve` rejects anything else \
             and will not start. parse error: {e}\nvalue: {raw}"
        )
    });

    // `parse_scoped_api_tokens`가 강제하는 것과 동일한 조건.
    assert!(
        !tokens.is_empty(),
        "manifest must contain at least one credential"
    );
    for token in &tokens {
        assert!(
            !token.principal_id.trim().is_empty(),
            "principal_id must not be empty"
        );
        assert!(!token.token.trim().is_empty(), "token must not be empty");
        assert!(
            !token.capabilities.is_empty(),
            "capabilities must not be empty — a credential with no capability can do nothing"
        );
    }
}

/// 예시가 최소 권한을 보이는 형상이어야 한다. 특히 LLM credential 원문 export는
/// 모든 워커의 프로바이더 API 키를 열람하므로 기본 예시에 넣지 않는다.
#[test]
fn examples_fleet_env_does_not_grant_credential_export_by_default() {
    let contents = example_env();
    let raw = env_value(&contents, "FLEET_API_TOKENS").expect("FLEET_API_TOKENS must be defined");
    assert!(
        !raw.contains("worker:llm_credential:export"),
        "the default example must not grant worker:llm_credential:export"
    );
}
