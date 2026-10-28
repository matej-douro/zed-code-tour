#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

if command -v rustup >/dev/null 2>&1; then
    if ! rustup target list --installed | grep -qx wasm32-wasip2; then
        echo "Installing the wasm32-wasip2 target..."
        rustup target add wasm32-wasip2
    fi
else
    echo "rustup not found; make sure the wasm32-wasip2 target is installed." >&2
fi

cargo build --locked --release --target wasm32-wasip2 -p zed-code-tour-extension

cp \
    target/wasm32-wasip2/release/zed_code_tour_extension.wasm \
    extension.wasm

echo "Built extension.wasm"
