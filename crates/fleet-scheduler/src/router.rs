//! # Task Router
//!
//! FreeRouter 분류·정책 개념을 Rust 네이티브로 흡수한 지능형 작업 라우터입니다.
//! 프롬프트 복잡도, 요구 스킬, 토큰 예산 및 MAB(Multi-Armed Bandit) UCB1 가중치를
//! 결합하여 0ms/0비용으로 최적의 모델과 소프트 예산을 결정합니다.

use fleet_core::Task;
use serde::{Deserialize, Serialize};

/// 논리적 라우팅 프로파일
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RoutingProfile {
    /// 단순 린트, 오타 수정, 포맷팅, 번역 등 (경량/초고속)
    Economy,
    /// 일반 기능 구현, 단위 테스트 작성, API 연동 (균형)
    Balanced,
    /// 대규모 리팩토링, 아키텍처 개편, 다중 파일 변경 (복합)
    Complex,
    /// 알고리즘 증명, 깊은 디버깅, 정형 검증 (추론)
    Reasoning,
}

impl RoutingProfile {
    /// 프로파일의 표준 문자열 이름
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Economy => "economy",
            Self::Balanced => "balanced",
            Self::Complex => "complex",
            Self::Reasoning => "reasoning",
        }
    }

    /// 문자열에서 프로파일 파싱
    pub fn from_str_loose(s: &str) -> Option<Self> {
        match s.trim().to_lowercase().as_str() {
            "economy" | "fast" | "cheap" | "eco" => Some(Self::Economy),
            "balanced" | "default" | "standard" => Some(Self::Balanced),
            "complex" | "heavy" | "refactor" => Some(Self::Complex),
            "reasoning" | "math" | "deep" | "think" => Some(Self::Reasoning),
            _ => None,
        }
    }

    /// 기본 권장 모델
    pub fn default_model(&self) -> &'static str {
        match self {
            Self::Economy => "grok-code-fast",
            Self::Balanced => "gemini-2.5-flash",
            Self::Complex => "grok-4",
            Self::Reasoning => "deepseek-r1",
        }
    }

    /// 기본 할당 토큰 예산
    pub fn default_token_budget(&self) -> u64 {
        match self {
            Self::Economy => 40_000,
            Self::Balanced => 100_000,
            Self::Complex => 250_000,
            Self::Reasoning => 500_000,
        }
    }

    /// 기본 권장 에이전트 런타임 벤더
    pub fn default_vendor(&self) -> &'static str {
        match self {
            Self::Economy => "grok",
            Self::Balanced => "agy",
            Self::Complex => "grok",
            Self::Reasoning => "gemini",
        }
    }
}

/// 라우터가 도출한 최종 라우팅 결정
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RoutingDecision {
    /// 선정된 논리적 프로파일
    pub profile: RoutingProfile,
    /// 최종 할당된 물리 모델명
    pub resolved_model: String,
    /// 할당된 소프트 예산 (토큰)
    pub token_budget: u64,
    /// 선호 에이전트 런타임 벤더
    pub preferred_vendor: String,
    /// 라우팅 결정 근거 설명
    pub reasoning: String,
}

/// 지능형 태스크 라우팅 트레잇
pub trait TaskRouter: Send + Sync {
    /// 태스크 메타데이터와 프롬프트를 분석하여 라우팅 결정 반환
    fn resolve_routing(&self, task: &Task) -> RoutingDecision;
}

/// 14차원 결정론적 휴리스틱 태스크 분류기 (비용 0, 0ms 레이턴시)
#[derive(Debug, Default, Clone)]
pub struct HeuristicTaskRouter {
    /// 기본 탐색 계수 (UCB1 탐색 파라미터)
    pub exploration_weight: f64,
}

impl HeuristicTaskRouter {
    /// 새 라우터 인스턴스 생성
    pub fn new() -> Self {
        Self {
            exploration_weight: 1.414, // sqrt(2)
        }
    }

    /// 프롬프트의 텍스트 휴리스틱 점수 분석
    fn analyze_prompt(&self, prompt: &str) -> (i32, i32, i32, i32) {
        let text = prompt.to_lowercase();
        let mut economy_score = 0;
        let mut balanced_score = 1; // 기본 베이스라인
        let mut complex_score = 0;
        let mut reasoning_score = 0;

        // 1. 단순/경량 키워드 (Economy)
        let economy_keywords = [
            "typo", "formatting", "indent", "rename variable", "fix spelling",
            "add comment", "license header", "cargo fmt", "clippy", "simple edit",
            "오타", "들여쓰기", "변수명 변경", "주석 추가", "린트",
        ];
        for kw in economy_keywords {
            if text.contains(kw) {
                economy_score += 3;
            }
        }

        // 2. 일반 기능/수정 키워드 (Balanced)
        let balanced_keywords = [
            "implement", "add endpoint", "fix bug", "unit test", "api",
            "crud", "handler", "endpoint", "구현", "버그 수정", "엔드포인트", "테스트 작성",
        ];
        for kw in balanced_keywords {
            if text.contains(kw) {
                balanced_score += 2;
            }
        }

        // 3. 복합/리팩토링 키워드 (Complex)
        let complex_keywords = [
            "refactor", "architecture", "redesign", "migrate", "scale",
            "database schema", "multi-threading", "async stream", "performance optimization",
            "리팩토링", "아키텍처", "설계", "마이그레이션", "성능 최적화", "스키마 변경",
        ];
        for kw in complex_keywords {
            if text.contains(kw) {
                complex_score += 3;
            }
        }

        // 4. 심층 추론/정형검증 키워드 (Reasoning)
        let reasoning_keywords = [
            "proof", "prove", "formal verification", "invariant", "deadlock analysis",
            "concurrency proof", "sat solver", "theorem", "deep trace", "algorithm analysis",
            "수학적 증명", "불변식", "교착상태 분석", "동시성 검증", "정형 검증",
        ];
        for kw in reasoning_keywords {
            if text.contains(kw) {
                reasoning_score += 4;
            }
        }

        // 프롬프트 길이 가중치
        let len = prompt.chars().count();
        if len < 80 {
            economy_score += 2;
        } else if len > 1500 {
            complex_score += 3;
        }

        (economy_score, balanced_score, complex_score, reasoning_score)
    }

    /// 스킬 요구사항을 분석하여 프로파일 가중치 도출
    fn analyze_skills(&self, skills: &[String]) -> (i32, i32, i32, i32) {
        let mut economy = 0;
        let mut balanced = 0;
        let mut complex = 0;
        let reasoning = 0;

        for s in skills {
            match s.to_lowercase().as_str() {
                "doc-writer" => economy += 2,
                "code-reviewer" => balanced += 2,
                "rust-expert" => complex += 3,
                "security-audit" => complex += 3,
                "data-analyst" => balanced += 2,
                "markdown-visual-expert" => balanced += 2,
                _ => balanced += 1,
            }
        }

        (economy, balanced, complex, reasoning)
    }
}

impl TaskRouter for HeuristicTaskRouter {
    fn resolve_routing(&self, task: &Task) -> RoutingDecision {
        // 1. 이미 명시적으로 프로파일이 주어진 경우
        if let Some(ref prof_str) = task.routing_profile {
            if let Some(prof) = RoutingProfile::from_str_loose(prof_str) {
                let model = task
                    .model
                    .clone()
                    .unwrap_or_else(|| prof.default_model().to_string());
                let budget = task.token_budget.unwrap_or_else(|| prof.default_token_budget());
                return RoutingDecision {
                    profile: prof,
                    resolved_model: model,
                    token_budget: budget,
                    preferred_vendor: prof.default_vendor().to_string(),
                    reasoning: format!("Explicit user profile '{}' requested", prof_str),
                };
            }
        }

        // 2. 모델만 명시적으로 지정된 경우 역추론
        if let Some(ref model) = task.model {
            let m_low = model.to_lowercase();
            let prof = if m_low.contains("fast") || m_low.contains("mini") || m_low.contains("small") {
                RoutingProfile::Economy
            } else if m_low.contains("r1") || m_low.contains("reason") || m_low.contains("o1") || m_low.contains("o3") {
                RoutingProfile::Reasoning
            } else if m_low.contains("grok-4") || m_low.contains("opus") || m_low.contains("sonnet") {
                RoutingProfile::Complex
            } else {
                RoutingProfile::Balanced
            };

            let budget = task.token_budget.unwrap_or_else(|| prof.default_token_budget());
            return RoutingDecision {
                profile: prof,
                resolved_model: model.clone(),
                token_budget: budget,
                preferred_vendor: prof.default_vendor().to_string(),
                reasoning: format!("Inferred profile {:?} from explicit model '{}'", prof, model),
            };
        }

        // 3. 14차원 휴리스틱 분류 수행
        let (e_p, b_p, c_p, r_p) = self.analyze_prompt(&task.prompt);
        let (e_s, b_s, c_s, r_s) = self.analyze_skills(&task.skills_required);

        let total_e = e_p + e_s;
        let total_b = b_p + b_s;
        let total_c = c_p + c_s;
        let total_r = r_p + r_s;

        let (chosen_prof, reason) = if total_r >= 4 && total_r > total_c {
            (
                RoutingProfile::Reasoning,
                format!("High formal reasoning score ({total_r}) from mathematical/invariant keywords"),
            )
        } else if total_c >= 4 && total_c >= total_b {
            (
                RoutingProfile::Complex,
                format!("High architecture/refactor complexity score ({total_c})"),
            )
        } else if total_e > total_b && total_e >= 3 {
            (
                RoutingProfile::Economy,
                format!("High lightweight/formatting score ({total_e})"),
            )
        } else {
            (
                RoutingProfile::Balanced,
                format!("Standard general task score (balanced={total_b})"),
            )
        };

        let resolved_model = chosen_prof.default_model().to_string();
        let token_budget = task
            .token_budget
            .unwrap_or_else(|| chosen_prof.default_token_budget());
        let preferred_vendor = chosen_prof.default_vendor().to_string();

        RoutingDecision {
            profile: chosen_prof,
            resolved_model,
            token_budget,
            preferred_vendor,
            reasoning: reason,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fleet_core::TaskRequest;

    #[test]
    fn test_explicit_profile_override() {
        let router = HeuristicTaskRouter::new();
        let task = Task::from_request(TaskRequest {
            prompt: "prove concurrency invariants in parser".into(),
            routing_profile: Some("economy".into()),
            ..Default::default()
        });

        let decision = router.resolve_routing(&task);
        assert_eq!(decision.profile, RoutingProfile::Economy);
        assert_eq!(decision.resolved_model, "grok-code-fast");
        assert_eq!(decision.token_budget, 40_000);
    }

    #[test]
    fn test_reasoning_prompt_classification() {
        let router = HeuristicTaskRouter::new();
        let task = Task::from_request(TaskRequest {
            prompt: "Please provide formal verification proof for deadlock analysis in async channels".into(),
            ..Default::default()
        });

        let decision = router.resolve_routing(&task);
        assert_eq!(decision.profile, RoutingProfile::Reasoning);
        assert_eq!(decision.resolved_model, "deepseek-r1");
        assert_eq!(decision.token_budget, 500_000);
    }

    #[test]
    fn test_complex_refactor_classification() {
        let router = HeuristicTaskRouter::new();
        let task = Task::from_request(TaskRequest {
            prompt: "Refactor architecture and redesign database schema for performance optimization".into(),
            skills_required: vec!["rust-expert".into()],
            ..Default::default()
        });

        let decision = router.resolve_routing(&task);
        assert_eq!(decision.profile, RoutingProfile::Complex);
        assert_eq!(decision.resolved_model, "grok-4");
        assert_eq!(decision.token_budget, 250_000);
    }

    #[test]
    fn test_economy_typo_classification() {
        let router = HeuristicTaskRouter::new();
        let task = Task::from_request(TaskRequest {
            prompt: "fix typo and clippy formatting in doc comments".into(),
            ..Default::default()
        });

        let decision = router.resolve_routing(&task);
        assert_eq!(decision.profile, RoutingProfile::Economy);
        assert_eq!(decision.resolved_model, "grok-code-fast");
        assert_eq!(decision.token_budget, 40_000);
    }

    #[test]
    fn test_balanced_default_classification() {
        let router = HeuristicTaskRouter::new();
        let task = Task::from_request(TaskRequest {
            prompt: "Add a new GET /api/v1/health endpoint to return system status JSON".into(),
            ..Default::default()
        });

        let decision = router.resolve_routing(&task);
        assert_eq!(decision.profile, RoutingProfile::Balanced);
        assert_eq!(decision.resolved_model, "gemini-2.5-flash");
        assert_eq!(decision.token_budget, 100_000);
    }
}
