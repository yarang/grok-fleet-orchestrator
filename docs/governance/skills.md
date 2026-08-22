---
type: contributor-guide
authority: canonical
implementation: partial
verification: code-checked
source: "docs/governance/skills.md"
last_verified: "2026-08-16"
---

# Skills System Guide

> **현재 CLI 범위:** 이 문서는 현재 `fleet tasks submit --skill`의 로컬 파일 주입 동작을
> 설명한다. Project·Agent 기반 Skill catalog, Project grant, required/optional binding, revision/hash
> snapshot은 아직 구현 전 목표 계약이며 [Agent 하네스](../architecture/agents/harness-composition.md)와
> [배치·맥락 계약](../architecture/entity-placement-and-context.md)이 우선한다.

Skills are plain-text prompt files that inject role-specific expertise into a fleet task agent at submission time. By selecting the right skill you get a specialist — not a generalist — focused on your problem.

---

## What Is a Skill?

A skill is a Markdown file containing a system-level persona prompt. When you pass `--skill <name>` to `fleet tasks submit`, the loader reads the corresponding `.md` file and prepends it to the agent's context before task execution begins.

```
fleet tasks submit --skill rust-expert "Refactor src/main.rs for idiomatic error handling"
#                  ^^^^^^^^^^^^^^^^^^^
#                  Loads ~/.config/grok-fleet/skills/rust-expert.md
```

Skills define:
- **Role** — who the agent is pretending to be
- **Responsibilities** — what it focuses on
- **Core principles** — how it reasons and prioritises
- **Working method** — the step-by-step approach it takes
- **Output format** — how it structures its response

---

## Skill File Location

The loader resolves skill files in the following priority order:

| Priority | Location |
|----------|----------|
| 1 (highest) | `$FLEET_SKILLS_DIR/<name>.md` |
| 2 | `~/.config/grok-fleet/skills/<name>.md` |
| 3 | Built-in skills bundled with the `fleet` binary |

### Override with `FLEET_SKILLS_DIR`

```bash
# Point to a project-local skills directory
export FLEET_SKILLS_DIR=/path/to/my-project/.fleet/skills

fleet tasks submit --skill custom-skill "..."
```

This lets teams version-control project-specific skills alongside their codebase.

---

## Skill File Format

Each skill file is a freeform Markdown document. The recommended structure is:

```markdown
You are a [role] expert specializing in [domain].

## Role and Responsibilities
[What the agent does]

## Core Principles
- [Principle 1]
- [Principle 2]

## Working Method
[Step-by-step analysis approach]

## Output Format
[How the agent structures its response]
```

> **📝 Note:** The file is injected verbatim. Write it as a system prompt addressed directly to the LLM — not as documentation addressed to a human reader.

---

## Built-In Skills

The following skills ship in `~/.config/grok-fleet/skills/`:

| Skill name | File | Specialty |
|------------|------|-----------|
| `rust-expert` | `rust-expert.md` | Rust refactoring, performance, Clippy, error handling |
| `security-audit` | `security-audit.md` | OWASP Top 10, SQL injection, XSS, auth/authz review |
| `code-reviewer` | `code-reviewer.md` | Readability, maintainability, test coverage, PR feedback |
| `doc-writer` | `doc-writer.md` | API docs, README, architecture docs, English + Korean, Mermaid/SVG guidelines |
| `markdown-visual-expert` | `markdown-visual-expert.md` | Markdown technical specs, Mermaid diagrams, vector SVGs, `docs/assets/diagrams/` asset management |
| `data-analyst` | `data-analyst.md` | SQL optimisation, data pipelines, statistical analysis |

---

## CLI Usage

### Basic submission

```bash
fleet tasks submit --skill <skill-name> "<task description>"
```

### Examples

```bash
# Rust refactoring
fleet tasks submit --skill rust-expert \
  "Refactor src/engine/parser.rs — eliminate all Clippy warnings and replace manual error strings with thiserror"

# Security audit of a PR diff
fleet tasks submit --skill security-audit \
  "Audit the authentication middleware in src/middleware/auth.rs for privilege escalation and token leakage"

# Code review
fleet tasks submit --skill code-reviewer \
  "Review PR #142: adds Redis caching layer to the user service"

# Write documentation
fleet tasks submit --skill doc-writer \
  "Write a README for the grok-fleet-orchestrator project with setup, usage, and contribution sections in English and Korean"

# Write architecture specification with Mermaid/SVG visual diagrams
fleet tasks submit --skill markdown-visual-expert \
  "Write an architecture spec for the task scheduler lifecycle. Include a Mermaid sequence diagram and save any complex assets in docs/assets/diagrams/architecture/"

# SQL optimisation
fleet tasks submit --skill data-analyst \
  "Optimise the daily_active_users query in analytics/queries/dau.sql — it currently takes 4 minutes on prod"
```

### List available skills

```bash
fleet skills list
```

### Inspect a skill

```bash
fleet skills show rust-expert
```

---

## Combining Skills

You can chain skills by submitting sequential tasks where the output of one feeds the next:

```bash
# Step 1: Audit for vulnerabilities
fleet tasks submit --skill security-audit \
  "Audit src/api/handlers.rs" --output audit-report.md

# Step 2: Fix the findings and review the fix
fleet tasks submit --skill rust-expert \
  "Apply the remediations from audit-report.md to src/api/handlers.rs"

# Step 3: Review the final result
fleet tasks submit --skill code-reviewer \
  "Review the changes in src/api/handlers.rs after the security remediation"
```

> **📝 Note:** Multi-skill orchestration via `--skill skill-a,skill-b` is planned. Until then, use sequential task submissions.

---

## Creating a Custom Skill

1. **Create the file**

   ```bash
   mkdir -p ~/.config/grok-fleet/skills
   cat > ~/.config/grok-fleet/skills/my-skill.md << 'EOF'
   You are a [role] expert specializing in [domain].

   ## Role and Responsibilities
   ...

   ## Core Principles
   - ...

   ## Working Method
   ...

   ## Output Format
   ...
   EOF
   ```

2. **Test it**

   ```bash
   fleet tasks submit --skill my-skill "Hello, introduce yourself and describe what you can help with"
   ```

3. **Share it with your team**

   Commit the file to your repository and set `FLEET_SKILLS_DIR` in the project's `.envrc` (or equivalent):

   ```bash
   # .envrc
   export FLEET_SKILLS_DIR="$(pwd)/.fleet/skills"
   ```

---

## Tips

- **Keep skills focused.** A skill covering Rust, security, and documentation simultaneously will be mediocre at all three.
- **Version-control project skills.** Team-specific skills (e.g., your internal API conventions) belong in the repo under `.fleet/skills/`.
- **Iterate on output format.** If the agent's output structure doesn't match your workflow, edit the `## Output Format` section of the skill file.
- **Use `FLEET_SKILLS_DIR` in CI.** Point it to a `ci/skills/` directory for reproducible, pipeline-specific agent behaviour.
