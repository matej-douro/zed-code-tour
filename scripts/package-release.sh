#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

if grep -qx 'License: TBD' README.md && [[ "${ALLOW_UNLICENSED_RELEASE:-0}" != "1" ]]; then
    echo "License: TBD blocks public release." >&2
    echo "Choose a license, update README.md, and remove this gate before packaging a release." >&2
    echo "Set ALLOW_UNLICENSED_RELEASE=1 only to verify artifacts locally; do not publish them." >&2
    exit 1
fi

./scripts/check-packaging.sh
version="$(awk -F'"' '/^version = / { print $2; exit }' extension.toml)"
host_target="$(rustc -vV | awk '/^host: / { print $2 }')"
case "$host_target" in
    x86_64-unknown-linux-gnu | aarch64-unknown-linux-gnu | x86_64-apple-darwin | aarch64-apple-darwin | x86_64-pc-windows-msvc) ;;
    *)
        echo "Unsupported release host target: $host_target" >&2
        exit 1
        ;;
esac

exe_suffix=""
if [[ "$host_target" == *windows* ]]; then
    exe_suffix=".exe"
fi

./scripts/build-extension.sh
cargo build --locked --release -p tour-lsp

dist_dir="$repo_root/dist"
extension_bundle="zed-code-tour-${version}-extension"
lsp_bundle="tour-lsp-${version}-${host_target}"

rm -rf "$dist_dir"
mkdir -p "$dist_dir/$extension_bundle/languages/tour" "$dist_dir/$lsp_bundle"

cp extension.toml README.md "$dist_dir/$extension_bundle/"
cp extension.wasm "$dist_dir/$extension_bundle/"
cp languages/tour/config.toml "$dist_dir/$extension_bundle/languages/tour/"
cp "target/release/tour-lsp${exe_suffix}" "$dist_dir/$lsp_bundle/"

tar -czf "$dist_dir/${extension_bundle}.tar.gz" -C "$dist_dir" "$extension_bundle"
tar -czf "$dist_dir/${lsp_bundle}.tar.gz" -C "$dist_dir" "$lsp_bundle"
cp extension.wasm "$dist_dir/extension-${version}.wasm"
rm -rf "$dist_dir/$extension_bundle" "$dist_dir/$lsp_bundle"

checksum_cmd=(sha256sum)
if ! command -v sha256sum >/dev/null 2>&1; then
    checksum_cmd=(shasum -a 256)
fi
(
    cd "$dist_dir"
    "${checksum_cmd[@]}" ./*.tar.gz ./*.wasm > SHA256SUMS
)

echo "Release artifacts:"
ls -1 "$dist_dir"
if grep -qx 'License: TBD' README.md; then
    echo "WARNING: License: TBD. These artifacts are for local verification only and must not be published." >&2
fi
