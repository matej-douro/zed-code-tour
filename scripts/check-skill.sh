#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

fail() {
    echo "skill check failed: $*" >&2
    exit 1
}

skill="skills/zed-code-tour/SKILL.md"
[[ -f "$skill" ]] || fail "$skill is missing"
[[ -f skills/README.md ]] || fail "skills/README.md is missing"

[[ "$(head -n 1 "$skill")" == "---" ]] ||
    fail "$skill must start with YAML frontmatter"
[[ "$(grep -c '^---$' "$skill")" -ge 2 ]] ||
    fail "$skill must close the YAML frontmatter with ---"

name="$(sed -n 's/^name: //p' "$skill" | head -n 1)"
[[ "$name" == "zed-code-tour" ]] ||
    fail "skill name must be zed-code-tour"
[[ ${#name} -le 64 ]] ||
    fail "skill name must be at most 64 characters"
[[ "$name" =~ ^[a-z0-9]([a-z0-9-]*[a-z0-9])?$ ]] ||
    fail "skill name must contain only lowercase letters, numbers, and hyphens"
[[ "$(basename "$(dirname "$skill")")" == "$name" ]] ||
    fail "skill directory must match the frontmatter name ($name)"

description="$(sed -n 's/^description: //p' "$skill" | head -n 1)"
[[ -n "$description" ]] || fail "skill description is empty"
[[ ${#description} -le 1024 ]] ||
    fail "skill description must be at most 1024 characters"

[[ "$(wc -c < "$skill")" -le 102400 ]] ||
    fail "$skill must be at most 100KB"

echo "Skill frontmatter OK ($name)"
