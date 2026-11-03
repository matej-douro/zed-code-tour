#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

fail() {
    echo "packaging check failed: $*" >&2
    exit 1
}

read_manifest_version() {
    awk -F'"' '/^version = / { print $2; exit }' "$1"
}

extension_version="$(read_manifest_version extension.toml)"
workspace_version="$(read_manifest_version Cargo.toml)"

[[ "$extension_version" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] ||
    fail "extension.toml version is not semver: $extension_version"
[[ "$workspace_version" == "$extension_version" ]] ||
    fail "extension.toml version $extension_version does not match Cargo.toml version $workspace_version"

grep -qx 'id = "zed-code-tour"' extension.toml ||
    fail "extension.toml id must be zed-code-tour"
grep -qx 'name = "Code Tour"' extension.toml ||
    fail "extension.toml name must be Code Tour"
grep -qx 'schema_version = 1' extension.toml ||
    fail "extension.toml schema_version must be 1"
grep -qx 'repository = "https://github.com/matej-douro/zed-code-tour"' extension.toml ||
    fail "extension.toml repository must point at matej-douro/zed-code-tour"

[[ -f languages/tour/config.toml ]] ||
    fail "languages/tour/config.toml is required by the extension bundle"

language_count="$(
    awk '
        /^languages = \[/ { in_languages = 1; next }
        in_languages && /^\]/ { in_languages = 0 }
        in_languages && /^[[:space:]]*"/ { count++ }
        END { print count + 0 }
    ' extension.toml
)"
[[ "$language_count" -ge 50 ]] ||
    fail "extension.toml should list at least 50 source languages, found $language_count"
grep -qx '    "Plain Text",' extension.toml ||
    fail "extension.toml must include the Plain Text language"
grep -qx '    "Markdown",' extension.toml ||
    fail "extension.toml must include the Markdown language"
grep -qx 'name = "zed-code-tour-extension"' Cargo.toml ||
    fail "the root Cargo package must be named zed-code-tour-extension"
[[ -f src/lib.rs ]] ||
    fail "the extension source is missing"
[[ ! -e crates/zed-code-tour-extension/Cargo.toml ]] ||
    fail "stale crates/zed-code-tour-extension/Cargo.toml remains; the extension package lives at the repository root"
[[ -f scripts/build-extension.sh ]] ||
    fail "scripts/build-extension.sh is missing"

echo "Packaging metadata OK (version $extension_version)"
