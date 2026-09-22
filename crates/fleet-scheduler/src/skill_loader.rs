//! 에이전트 스킬 동적 로더.
//!
//! `skills_required` 배열에 지정된 스킬 이름을 읽어,
//! `~/.config/grok-fleet/skills/<name>.md` (또는 `FLEET_SKILLS_DIR` 환경 변수)에서
//! 스킬 지시 마크다운을 로드하고, 프롬프트 앞에 인젝션합니다.
//!
//! # 스킬 파일 포맷
//! 스킬 파일은 단순 마크다운 또는 텍스트 파일입니다. YAML frontmatter (`---`)가
//! 존재하면 파싱을 건너뛰고 `---` 이후 본문만 사용합니다.
//!
//! # 인젝션 방식
//! ```text
//! <SKILL: rust-expert>
//! ...스킬 본문...
//! </SKILL>
//!
//! <TASK>
//! ...원래 프롬프트...
//! </TASK>
//! ```

use std::path::{Path, PathBuf};

use fleet_core::SkillSnapshotEntry;
use sha2::{Digest, Sha256};
use tracing::{debug, warn};

/// 스킬 디렉토리 기본 경로를 반환합니다.
///
/// 우선순위:
/// 1. `FLEET_SKILLS_DIR` 환경 변수
/// 2. `~/.config/grok-fleet/skills/`
fn default_skills_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("FLEET_SKILLS_DIR") {
        return PathBuf::from(dir);
    }
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".into());
    PathBuf::from(home)
        .join(".config")
        .join("grok-fleet")
        .join("skills")
}

/// 스킬 파일 본문을 반환합니다. YAML frontmatter(`---`)가 있으면 제거합니다.
fn strip_frontmatter(content: &str) -> &str {
    if let Some(rest) = content.strip_prefix("---") {
        if let Some(end) = rest.find("\n---") {
            return rest[end + 4..].trim_start();
        }
    }
    content
}

/// 단일 스킬 파일을 로드합니다.
/// 파일이 없거나 읽기 오류 시 `None`을 반환합니다 (soft-fail).
fn load_skill(skills_dir: &Path, name: &str) -> Option<String> {
    // `name`에 경로 구분자가 들어오면 무시 (path-traversal 방어)
    if name.contains('/') || name.contains('\\') || name.contains("..") {
        warn!(
            skill = name,
            "skill name contains path traversal characters; skipping"
        );
        return None;
    }
    let path = skills_dir.join(format!("{name}.md"));
    match std::fs::read_to_string(&path) {
        Ok(content) => {
            debug!(skill = name, path = %path.display(), "loaded skill");
            Some(strip_frontmatter(&content).to_string())
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            // .txt도 시도
            let txt_path = skills_dir.join(format!("{name}.txt"));
            match std::fs::read_to_string(&txt_path) {
                Ok(content) => {
                    debug!(skill = name, path = %txt_path.display(), "loaded skill (txt)");
                    Some(strip_frontmatter(&content).to_string())
                }
                Err(_) => {
                    warn!(skill = name, path = %path.display(), "skill file not found; skipping");
                    None
                }
            }
        }
        Err(e) => {
            warn!(skill = name, error = %e, "failed to read skill file; skipping");
            None
        }
    }
}

/// 스킬 조립 결과 (로드맵 `#65`).
///
/// **누락을 값으로 돌려주는 것이 이 타입의 존재 이유다.** 예전 시그니처는
/// `-> String`이었고 누락된 스킬은 `warn!` 한 줄로 흘려보냈다. 그래서 호출부에는
/// 거절할 방법이 **원리적으로 없었고**, `skills_required`라는 이름이 강제되지
/// 않는 장식이었다.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillInjection {
    /// 스킬 블록이 앞에 붙은 프롬프트. 누락이 있어도 조립은 해 둔다 —
    /// 거절 여부는 호출부의 판단이고, 이 함수는 사실만 돌려준다.
    pub prompt: String,
    /// 요청됐지만 **로드하지 못한** 스킬 이름. 파일이 없거나, 읽을 수 없거나,
    /// 이름에 경로 구분자가 들어 있어 거부된 경우다.
    pub missing: Vec<String>,
    /// 실제로 주입된 Skill의 신원 (로드맵 `#65` 게이트 2).
    ///
    /// **이름이 아니라 내용의 해시**다. 이름은 그때 무엇이 실행됐는지를 말해
    /// 주지 않는다 — 파일이 오케스트레이터 디스크에 있어 언제든 바뀐다.
    /// `skills_required`가 비어 있으면 빈 벡터이며, 그것은 "조립했고 Skill이
    /// 없었다"라는 **사실**이다(기록 없음과 다르다).
    pub loaded: Vec<SkillSnapshotEntry>,
}

impl SkillInjection {
    /// 요청된 스킬이 전부 로드됐는지.
    pub fn is_complete(&self) -> bool {
        self.missing.is_empty()
    }
}

/// `skills_required` 목록을 로드해 프롬프트 앞에 인젝션한다
/// (디렉토리 명시 버전, 테스트 / 고급 사용).
///
/// 누락된 스킬은 [`SkillInjection::missing`]에 담아 돌려준다. **여기서
/// 거절하지 않는 이유**는 이 함수가 파일시스템만 아는 순수 조립기이기
/// 때문이다 — 거절은 Task 상태 전이를 동반하므로 `Dispatcher`의 일이다.
pub fn inject_skills_from_dir(
    prompt: &str,
    skills_required: &[String],
    skills_dir: &Path,
) -> SkillInjection {
    if skills_required.is_empty() {
        return SkillInjection {
            prompt: prompt.to_string(),
            missing: Vec::new(),
            loaded: Vec::new(),
        };
    }
    let mut blocks = Vec::new();
    let mut missing = Vec::new();
    let mut loaded = Vec::new();
    for skill in skills_required {
        match load_skill(skills_dir, skill) {
            Some(body) => {
                // **frontmatter를 뺀 뒤를 잰다.** 그것이 실제로 프롬프트에
                // 들어간 바이트이기 때문이다 — 파일 전체를 재면 메타데이터만
                // 바뀌어도 다른 실행으로 보이고, "같은 입력이었는가"에
                // 거짓으로 답한다.
                loaded.push(SkillSnapshotEntry {
                    name: skill.clone(),
                    sha256: format!("{:x}", Sha256::digest(body.as_bytes())),
                    bytes: body.len(),
                });
                blocks.push(format!("<SKILL: {skill}>\n{body}\n</SKILL>"));
            }
            None => missing.push(skill.clone()),
        }
    }
    let prompt = if blocks.is_empty() {
        prompt.to_string()
    } else {
        format!("{}\n\n<TASK>\n{}\n</TASK>", blocks.join("\n\n"), prompt)
    };
    SkillInjection {
        prompt,
        missing,
        loaded,
    }
}

/// `skills_required` 목록을 로드해 프롬프트 앞에 인젝션한다.
///
/// 스킬 디렉토리는 `FLEET_SKILLS_DIR` 환경 변수 또는
/// `~/.config/grok-fleet/skills/` 기본 경로를 사용합니다.
pub fn inject_skills(prompt: &str, skills_required: &[String]) -> SkillInjection {
    inject_skills_from_dir(prompt, skills_required, &default_skills_dir())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn setup_skill(dir: &TempDir, name: &str, content: &str) {
        let path = dir.path().join(format!("{name}.md"));
        fs::write(path, content).unwrap();
    }

    /// 테스트들은 env var 경쟁을 피하기 위해 inject_skills_from_dir를 직접 사용합니다.

    #[test]
    fn no_skills_returns_original_prompt() {
        let tmp = TempDir::new().unwrap();
        let prompt = "build the project";
        let result = inject_skills_from_dir(prompt, &[], tmp.path());
        assert_eq!(result.prompt, prompt);
        assert!(result.is_complete(), "요청한 스킬이 없으면 누락도 없다");
    }

    /// **누락이 값으로 나온다** (로드맵 `#65`).
    ///
    /// 예전 이름은 `missing_skill_file_returns_original_prompt`였고 단정도
    /// "원래 프롬프트를 그대로 돌려준다" 하나뿐이었다. 그 단정은 지금도
    /// 참이지만 **그것만으로는 부족하다** — 호출부가 거절하려면 누락이
    /// 반환값에 있어야 하고, 예전 시그니처에는 그것을 실을 자리가 없었다.
    #[test]
    fn a_missing_skill_is_reported_not_swallowed() {
        let tmp = TempDir::new().unwrap();
        let prompt = "audit the code";
        let result = inject_skills_from_dir(prompt, &["nonexistent-skill".to_string()], tmp.path());
        assert_eq!(result.prompt, prompt);
        assert!(!result.is_complete());
        assert_eq!(result.missing, vec!["nonexistent-skill".to_string()]);
    }

    /// 일부만 있는 경우에도 **있는 것은 붙이고 없는 것은 보고한다.**
    /// 조립기는 사실만 돌려주고 거절은 `Dispatcher`가 한다.
    #[test]
    fn a_partial_load_reports_only_what_is_missing() {
        let tmp = TempDir::new().unwrap();
        setup_skill(&tmp, "present", "I am here.");
        let result = inject_skills_from_dir(
            "work",
            &["present".to_string(), "absent".to_string()],
            tmp.path(),
        );
        assert!(result.prompt.contains("<SKILL: present>"));
        assert_eq!(result.missing, vec!["absent".to_string()]);
    }

    /// **같은 이름이라도 본문이 다르면 다른 해시다** (로드맵 `#65` 게이트 2).
    ///
    /// 이것이 이 스냅샷의 존재 이유다 — `skills_required`는 이름 목록이라
    /// "그때 무엇이 실행됐는가"에 답하지 못한다. 파일은 오케스트레이터
    /// 디스크에 있어 언제든 바뀐다.
    #[test]
    fn the_snapshot_identifies_content_not_the_name() {
        let tmp = TempDir::new().unwrap();
        setup_skill(&tmp, "audit", "version one");
        let first = inject_skills_from_dir("work", &["audit".to_string()], tmp.path());

        setup_skill(&tmp, "audit", "version two");
        let second = inject_skills_from_dir("work", &["audit".to_string()], tmp.path());

        assert_eq!(first.loaded.len(), 1);
        assert_eq!(first.loaded[0].name, second.loaded[0].name);
        assert_ne!(
            first.loaded[0].sha256, second.loaded[0].sha256,
            "이름만 같고 본문이 다른 두 실행이 같은 신원을 가지면 재현이 성립하지 않는다"
        );
        assert_eq!(first.loaded[0].bytes, "version one".len());
    }

    /// **frontmatter를 뺀 뒤를 잰다.** 파일 전체를 재면 메타데이터만 바뀌어도
    /// 다른 실행으로 보이고, "같은 입력이었는가"에 거짓으로 답한다.
    #[test]
    fn the_hash_covers_only_the_injected_body() {
        let tmp = TempDir::new().unwrap();
        setup_skill(&tmp, "a", "---\nname: a\n---\nBODY");
        let with_fm = inject_skills_from_dir("w", &["a".to_string()], tmp.path());

        setup_skill(&tmp, "a", "---\nname: a\nnote: changed metadata\n---\nBODY");
        let other_fm = inject_skills_from_dir("w", &["a".to_string()], tmp.path());

        assert_eq!(
            with_fm.loaded[0].sha256, other_fm.loaded[0].sha256,
            "frontmatter만 달라진 것은 같은 실행이다 — 프롬프트에 들어간 바이트가 같다"
        );
    }

    /// 누락된 스킬은 스냅샷에 **없다.** 없는 것을 신원과 함께 적으면 그
    /// 기록이 "실행됐다"고 거짓말한다.
    #[test]
    fn a_missing_skill_is_absent_from_the_snapshot() {
        let tmp = TempDir::new().unwrap();
        setup_skill(&tmp, "present", "here");
        let r = inject_skills_from_dir(
            "w",
            &["present".to_string(), "absent".to_string()],
            tmp.path(),
        );
        assert_eq!(r.loaded.len(), 1);
        assert_eq!(r.loaded[0].name, "present");
        assert_eq!(r.missing, vec!["absent".to_string()]);
    }

    /// 경로 우회 시도는 **누락으로 보고된다** — 조용히 건너뛰면 그 Task가
    /// 스킬 없이 실행되고, 이름이 수상했다는 사실조차 남지 않는다.
    #[test]
    fn a_path_traversal_name_is_reported_as_missing() {
        let tmp = TempDir::new().unwrap();
        let result = inject_skills_from_dir("work", &["../../etc/passwd".to_string()], tmp.path());
        assert_eq!(result.prompt, "work");
        assert_eq!(result.missing, vec!["../../etc/passwd".to_string()]);
    }

    #[test]
    fn skill_is_injected_before_prompt() {
        let tmp = TempDir::new().unwrap();
        setup_skill(&tmp, "rust-expert", "You are a Rust expert.");

        let prompt = "refactor this code";
        let result = inject_skills_from_dir(prompt, &["rust-expert".to_string()], tmp.path());

        assert!(result.prompt.contains("<SKILL: rust-expert>"));
        assert!(result.prompt.contains("You are a Rust expert."));
        assert!(result.prompt.contains("<TASK>"));
        assert!(result.prompt.contains("refactor this code"));
        assert!(result.prompt.find("<SKILL").unwrap() < result.prompt.find("<TASK>").unwrap());
    }

    #[test]
    fn frontmatter_is_stripped() {
        let tmp = TempDir::new().unwrap();
        setup_skill(
            &tmp,
            "sec-audit",
            "---\nname: sec-audit\n---\nYou are a security auditor.",
        );

        let prompt = "check vulnerabilities";
        let result = inject_skills_from_dir(prompt, &["sec-audit".to_string()], tmp.path());

        assert!(
            !result.prompt.contains("name: sec-audit"),
            "frontmatter should be stripped"
        );
        assert!(result.prompt.contains("You are a security auditor."));
    }

    #[test]
    fn path_traversal_is_rejected() {
        let tmp = TempDir::new().unwrap();
        let prompt = "do something";
        let result = inject_skills_from_dir(prompt, &["../etc/passwd".to_string()], tmp.path());
        assert_eq!(result.prompt, prompt);
        assert!(
            !result.is_complete(),
            "거부한 이름을 '로드 성공'으로 접으면 그 Task가 스킬 없이 나간다"
        );
    }

    #[test]
    fn multiple_skills_are_injected() {
        let tmp = TempDir::new().unwrap();
        setup_skill(&tmp, "skill-a", "Skill A content.");
        setup_skill(&tmp, "skill-b", "Skill B content.");

        let prompt = "do the task";
        let result = inject_skills_from_dir(
            prompt,
            &["skill-a".to_string(), "skill-b".to_string()],
            tmp.path(),
        );

        assert!(result.prompt.contains("<SKILL: skill-a>"));
        assert!(result.prompt.contains("<SKILL: skill-b>"));
        assert!(result.prompt.contains("Skill A content."));
        assert!(result.prompt.contains("Skill B content."));
    }
}
