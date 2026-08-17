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
//! <SKILL: security-audit>
//! ...스킬 본문...
//! </SKILL>
//!
//! <TASK>
//! ...원래 프롬프트...
//! </TASK>
//! ```

use std::path::PathBuf;

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
    PathBuf::from(home).join(".config").join("grok-fleet").join("skills")
}

/// 스킬 파일 본문을 반환합니다. YAML frontmatter(`---`)가 있으면 제거합니다.
fn strip_frontmatter(content: &str) -> &str {
    if content.starts_with("---") {
        // 두 번째 `---` 이후부터 반환
        if let Some(end) = content[3..].find("\n---") {
            return content[3 + end + 4..].trim_start();
        }
    }
    content
}

/// 단일 스킬 파일을 로드합니다.
/// 파일이 없거나 읽기 오류 시 `None`을 반환합니다 (soft-fail).
fn load_skill(skills_dir: &PathBuf, name: &str) -> Option<String> {
    // `name`에 경로 구분자가 들어오면 무시 (보안)
    if name.contains('/') || name.contains('\\') || name.contains("..") {
        warn!(skill = name, "skill name contains path traversal characters; skipping");
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

/// `skills_required` 목록에서 유효한 스킬을 로드하여 원래 프롬프트 앞에
/// 인젝션한 새 프롬프트를 반환합니다 (디렉토리 명시 버전, 테스트 / 고급 사용).
///
/// 로드할 스킬이 없거나 스킬 파일이 존재하지 않으면 원래 `prompt`를 그대로 반환합니다.
pub fn inject_skills_from_dir(
    prompt: &str,
    skills_required: &[String],
    skills_dir: &PathBuf,
) -> String {
    if skills_required.is_empty() {
        return prompt.to_string();
    }
    let mut blocks = Vec::new();
    for skill in skills_required {
        if let Some(body) = load_skill(skills_dir, skill) {
            blocks.push(format!("<SKILL: {skill}>\n{body}\n</SKILL>"));
        }
    }
    if blocks.is_empty() {
        return prompt.to_string();
    }
    format!("{}\n\n<TASK>\n{}\n</TASK>", blocks.join("\n\n"), prompt)
}

/// `skills_required` 목록에서 유효한 스킬을 로드하여 원래 프롬프트 앞에
/// 인젝션한 새 프롬프트를 반환합니다.
///
/// 스킬 디렉토리는 `FLEET_SKILLS_DIR` 환경 변수 또는 `~/.config/grok-fleet/skills/`
/// 기본 경로를 사용합니다.
///
/// 로드할 스킬이 없거나 스킬 파일이 존재하지 않으면 원래 `prompt`를 그대로 반환합니다.
pub fn inject_skills(prompt: &str, skills_required: &[String]) -> String {
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
        let result = inject_skills_from_dir(prompt, &[], &tmp.path().to_path_buf());
        assert_eq!(result, prompt);
    }

    #[test]
    fn missing_skill_file_returns_original_prompt() {
        let tmp = TempDir::new().unwrap();
        let prompt = "audit the code";
        let result = inject_skills_from_dir(
            prompt,
            &["nonexistent-skill".to_string()],
            &tmp.path().to_path_buf(),
        );
        assert_eq!(result, prompt);
    }

    #[test]
    fn skill_is_injected_before_prompt() {
        let tmp = TempDir::new().unwrap();
        setup_skill(&tmp, "rust-expert", "You are a Rust expert.");

        let prompt = "refactor this code";
        let result = inject_skills_from_dir(
            prompt,
            &["rust-expert".to_string()],
            &tmp.path().to_path_buf(),
        );

        assert!(result.contains("<SKILL: rust-expert>"));
        assert!(result.contains("You are a Rust expert."));
        assert!(result.contains("<TASK>"));
        assert!(result.contains("refactor this code"));
        // 스킬이 프롬프트 앞에 위치해야 한다
        assert!(result.find("<SKILL").unwrap() < result.find("<TASK>").unwrap());
    }

    #[test]
    fn frontmatter_is_stripped() {
        let tmp = TempDir::new().unwrap();
        setup_skill(&tmp, "sec-audit", "---\nname: sec-audit\n---\nYou are a security auditor.");

        let prompt = "check vulnerabilities";
        let result = inject_skills_from_dir(
            prompt,
            &["sec-audit".to_string()],
            &tmp.path().to_path_buf(),
        );

        assert!(!result.contains("name: sec-audit"), "frontmatter should be stripped");
        assert!(result.contains("You are a security auditor."));
    }

    #[test]
    fn path_traversal_is_rejected() {
        let tmp = TempDir::new().unwrap();
        let prompt = "do something";
        let result = inject_skills_from_dir(
            prompt,
            &["../etc/passwd".to_string()],
            &tmp.path().to_path_buf(),
        );
        // 경로 우회 시도는 soft-fail → 원래 프롬프트 그대로
        assert_eq!(result, prompt);
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
            &tmp.path().to_path_buf(),
        );

        assert!(result.contains("<SKILL: skill-a>"));
        assert!(result.contains("<SKILL: skill-b>"));
        assert!(result.contains("Skill A content."));
        assert!(result.contains("Skill B content."));
    }
}
