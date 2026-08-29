//! 대시보드 정적 자산의 XSS 방어 불변식 (#98 / S13).
//!
//! 이 파일이 지키는 것은 **구조적 불변식 두 개**뿐이다. "모든 보간이
//! 이스케이프되었는가"는 값마다 출처 판단이 필요해 리뷰의 몫으로 남긴다 —
//! 변수 이름으로 allow-list를 만들면 `fooLabel`이라 이름 붙인 원본 데이터가
//! 통과하고 안전한 변수를 개명하면 실패하는, 시간이 지날수록 틀려지는
//! 테스트가 된다. 여기서는 이름에 의존하지 않는 두 가지만 잠근다.
//!
//! 1. `escapeHtml`은 `app.js` 하나만 정의한다.
//!    예전에는 11개 페이지 스크립트가 각자
//!    `d.textContent = s; return d.innerHTML` 형태의 지역 정의로 전역을
//!    **가렸다**. 텍스트 노드 직렬화는 `&`·`<`·`>`만 바꾸고 `"`·`'`는
//!    그대로 두므로 속성 문맥에서 무력하다. 호출부는 이스케이프하는 것처럼
//!    보이는데 실제로는 약한 함수가 불리던 상태였고, 이 결함은 호출부만
//!    읽어서는 보이지 않는다.
//!
//! 2. `.js` 자산은 `onclick="…"` 속성 문자열을 만들지 않는다.
//!    HTML 파서는 속성값의 문자 참조를 JS 파싱보다 **먼저** 디코드하므로,
//!    속성 안의 JS 문자열 리터럴에 값을 넣는 자리는 HTML 이스케이프로
//!    원리적으로 막을 수 없다(`&#39;`가 `'`로 되돌아간다). 데이터는
//!    `data-*`에 두고 값은 코드에서 읽는다.

use fleet_dashboard::assets::Asset;

/// `//`부터 줄 끝까지를 제거한다.
///
/// 두 단정 모두 **코드**를 보려는 것이지 설명을 보려는 것이 아니다. 이
/// 저장소의 자산에는 위험한 패턴을 인용해 둔 한국어 주석이 있고(예:
/// `admin-ssh-keys.js`가 왜 인라인 `onclick`을 쓰면 안 되는지 설명하며
/// 그 형태를 그대로 적어 둔다), 그 설명은 이 변경에서 가장 값어치 있는
/// 산문이므로 테스트를 통과시키려고 문구를 비틀지 않는다.
///
/// 앞이 `:`인 `//`는 건너뛴다 — 지금은 자산에 URL이 없지만, 나중에 누가
/// `https://…`를 넣었을 때 뒷부분을 통째로 먹어 **위반을 감추는** 쪽으로
/// 고장 나면 안 된다.
fn strip_line_comments(src: &str) -> String {
    src.lines()
        .map(|line| {
            let bytes = line.as_bytes();
            let mut i = 0;
            while i + 1 < bytes.len() {
                if bytes[i] == b'/' && bytes[i + 1] == b'/' && (i == 0 || bytes[i - 1] != b':') {
                    return line[..i].to_string();
                }
                i += 1;
            }
            line.to_string()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// 임베드된 `.js` 자산을 (경로, 주석 제거된 본문)으로 모은다.
///
/// **바닥 단정을 함께 둔다.** `assets.rs`는 폴더가 없으면 빈 디렉토리로
/// 간주하도록 되어 있어서, 개수를 확인하지 않으면 자산이 0개일 때 두 테스트가
/// 조용히 통과한다 — `agent.md` §4.3 (3)이 기록한 "실행되지 않았는데 초록"과
/// 같은 모양이다.
fn js_assets() -> Vec<(String, String)> {
    let files: Vec<(String, String)> = Asset::iter()
        .filter(|p| p.ends_with(".js"))
        .map(|p| {
            let raw = Asset::get(&p).expect("iter가 준 경로는 반드시 읽힌다");
            let text = String::from_utf8(raw.data.into_owned()).expect("자산은 UTF-8");
            (p.to_string(), strip_line_comments(&text))
        })
        .collect();

    assert!(
        files.iter().any(|(p, _)| p == "app.js"),
        "app.js가 임베드되지 않았다 — 자산 스캔이 빈 집합을 보고 있으므로 \
         아래 단정들은 아무것도 검증하지 못한다"
    );
    assert!(
        files.len() >= 15,
        "`.js` 자산이 {}개뿐이다(최소 15). 자산이 사라졌거나 임베드 경로가 \
         바뀐 것이며, 이 테스트는 그 상태에서 무의미하게 통과한다",
        files.len()
    );
    files
}

#[test]
fn only_app_js_defines_escape_html() {
    let offenders: Vec<String> = js_assets()
        .into_iter()
        .filter(|(path, body)| path != "app.js" && body.contains("function escapeHtml"))
        .map(|(path, _)| path)
        .collect();

    assert!(
        offenders.is_empty(),
        "{offenders:?}가 `escapeHtml`을 다시 정의한다. 나중에 실행되는 정의가 \
         전역을 가리므로, 호출부는 그대로인데 이스케이프 강도만 조용히 \
         내려간다. 이스케이프 규칙을 바꿔야 한다면 app.js의 정의를 고친다"
    );
}

#[test]
fn no_js_asset_builds_an_inline_onclick_attribute() {
    // 속성 문자열은 `onclick="` 처럼 공백 없이 붙고, DOM 프로퍼티 대입은
    // 이 저장소의 관례상 `row.onclick = () => …` 처럼 공백을 끼고 쓴다.
    // 프로퍼티 대입은 HTML 파싱을 거치지 않으므로 위험하지 않다.
    let offenders: Vec<String> = js_assets()
        .into_iter()
        .filter(|(_, body)| body.contains("onclick="))
        .map(|(path, _)| path)
        .collect();

    assert!(
        offenders.is_empty(),
        "{offenders:?}가 `onclick=` 속성을 문자열로 만든다. 속성 안의 JS \
         문자열 리터럴은 HTML 이스케이프로 막을 수 없다 — 값은 `data-*`에 \
         두고 `addEventListener`에서 읽는다(admin-ssh-keys.js 참고). \
         DOM 프로퍼티 대입이라면 `el.onclick = ` 처럼 띄어 쓴다"
    );
}
