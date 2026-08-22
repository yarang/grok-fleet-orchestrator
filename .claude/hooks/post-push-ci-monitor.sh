#!/bin/bash
# PostToolUse hook — after a `git push` Bash command completes, verify the
# pushed commit's GitHub Actions run goes green using the github-actions-monitor
# skill from the grok-fleet-custom-plugins Claude Code plugin (user-global,
# installed via ~/.claude/local-marketplaces/grok-fleet-custom-plugins — see
# https://github.com/yarang/grok-fleet-orchestrator commit history for the
# migration from this repo's now-removed .agents/skills/github-actions-monitor).
# Deliberately global rather than project-local: this hook lives under
# .claude/, and pointing it at a project-local skill directory (.agents/)
# created a silent cross-directory dependency — a refactor of one broke the
# other with no error, since this script no-ops quietly when the script is
# missing. Depending on the global plugin instead removes that coupling.
#
# Activated per user request: "이 기능을 활성화하여 모든 github push에 적용하자"
# (2026-08-21). Runs synchronously inside the hook (PostToolUse's own timeout
# below bounds this) so the poll/wait loop already implemented in
# monitor_ci.py can run to completion and its final PASS/FAIL line reaches
# Claude's context directly, instead of a fire-and-forget background process
# whose result nothing would ever collect.
set -euo pipefail

INPUT="$(cat)"

# Only proceed for a Bash git push, and only if that push actually succeeded —
# running CI monitoring after a failed push is meaningless. Field names are
# defensive (tool_output vs tool_response) since exact stdin shape can vary
# across Claude Code versions.
COMMAND=$(printf '%s' "$INPUT" | python3 -c "
import json, sys
try:
    d = json.load(sys.stdin)
except Exception:
    sys.exit(0)
print(d.get('tool_input', {}).get('command', ''))
" 2>/dev/null || true)

case "$COMMAND" in
  *"git push"*) ;;
  *) exit 0 ;;
esac

EXIT_CODE=$(printf '%s' "$INPUT" | python3 -c "
import json, sys
try:
    d = json.load(sys.stdin)
except Exception:
    print(0)
    sys.exit(0)
for key in ('tool_output', 'tool_response'):
    v = d.get(key)
    if isinstance(v, dict) and 'exit_code' in v:
        print(v['exit_code'])
        sys.exit(0)
print(0)
" 2>/dev/null || echo 0)

if [ "$EXIT_CODE" != "0" ]; then
  # push itself failed — nothing to monitor.
  exit 0
fi

PROJECT_DIR="${CLAUDE_PROJECT_DIR:-$(pwd)}"
cd "$PROJECT_DIR" 2>/dev/null || exit 0

# Global plugin location, not project-local — see header comment above.
SKILL_SCRIPT="$HOME/.claude/local-marketplaces/grok-fleet-custom-plugins/plugins/grok-fleet-custom-plugins/skills/github-actions-monitor/scripts/monitor_ci.py"
[ -f "$SKILL_SCRIPT" ] || exit 0

SHA=$(git rev-parse HEAD 2>/dev/null || true)
[ -z "$SHA" ] && exit 0

# Only meaningful against the GitHub remote (gitea is treated as down this
# session — see docs/credentials memory) — but harmless either way, the
# script just times out quietly if no matching run ever appears.
OWNER_REPO=$(git remote get-url origin 2>/dev/null | sed -E 's#.*github\.com[:/]+([^/]+)/([^/.]+)(\.git)?#\1 \2#')
OWNER=$(echo "$OWNER_REPO" | awk '{print $1}')
REPO=$(echo "$OWNER_REPO" | awk '{print $2}')
if [ -z "$OWNER" ] || [ -z "$REPO" ]; then
  exit 0
fi

# monitor_ci.py exits 1 on CI failure, 2 on timeout — that's its own signal
# carried in stdout (🟢/🔴/⚠️ lines), not something the hook should propagate
# as its own exit code (PostToolUse can't block/undo the push regardless, and
# a non-zero hook exit here has an ambiguous contract other than 2).
python3 "$SKILL_SCRIPT" --owner "$OWNER" --repo "$REPO" --commit "$SHA" --poll 15 --max-wait 480 || true
exit 0
