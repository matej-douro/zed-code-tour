#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

if [[ $# -ne 1 ]]; then
    echo "usage: scripts/bump-version.sh <major.minor.patch>" >&2
    exit 2
fi

version="$1"
if [[ ! "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
    echo "version must be semver, got: $version" >&2
    exit 2
fi

replace_first_version() {
    local file="$1"
    local tmp
    tmp="$(mktemp)"
    awk -v version="$version" '
        !done && /^version = "/ { print "version = \"" version "\""; done = 1; next }
        { print }
    ' "$file" > "$tmp"
    mv "$tmp" "$file"
}

replace_first_version extension.toml
replace_first_version Cargo.toml

# Refresh the workspace package versions in Cargo.lock without upgrading
# unrelated dependencies.
cargo check --workspace

./scripts/check-packaging.sh
echo "Bumped to $version. Review Cargo.lock, then run ./scripts/test.sh."
