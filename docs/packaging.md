# Packaging and distribution

This page defines what the project builds, what ships, and how the extension
finds the native `tour-lsp` binary. Install steps are in [install.md](install.md).
The release checklist is in [release.md](release.md).

## Release metadata

The Zed manifest at the repository root is the packaging source of truth.
`scripts/check-packaging.sh` verifies these values in CI:

| Field | Value |
| --- | --- |
| `id` | `zed-code-tour` |
| `name` | `Code Tour` |
| `version` | `0.1.0` |
| `schema_version` | `1` |
| `repository` | `https://github.com/matej-douro/zed-code-tour` |

The workspace version in `Cargo.toml` must match the version in `extension.toml`.
`scripts/bump-version.sh` updates both files and refreshes `Cargo.lock`.

## Build outputs

`./scripts/build-extension.sh` builds the root `zed-code-tour-extension` package
for `wasm32-wasip2` and copies the release result to `extension.wasm` at the
repository root. The repository root is both the Cargo package manifest and the
directory Zed compiles for a dev-extension install; the package name makes Zed
expect `target/wasm32-wasip2/debug/zed_code_tour_extension.wasm`.

A prebuilt extension directory needs:

- `extension.toml`,
- `extension.wasm`,
- `languages/tour/config.toml`.

`cargo build --locked --release -p tour-lsp` builds the native language server at
`target/release/tour-lsp` (`tour-lsp.exe` on Windows).

## What ships

The extension and the language server have separate release artifacts.

- The extension bundle contains `extension.toml`, `extension.wasm`, the Tour
  language directory, and the README. This is enough for a local dev install
  from a prebuilt directory.
- `tour-lsp` is a native binary and cannot run inside the extension WASM. It is
  distributed separately as a per-platform archive and is never bundled into the
  extension WASM.
- For a future Zed extension-registry entry, Zed builds the extension WASM from
  the tagged source. The GitHub release still carries the prebuilt WASM and the
  native server archives so local and non-registry installs use the same
  artifacts.

## How the extension finds `tour-lsp`

### Local development, version 0.1.0

`language_server_command` calls `Worktree::which("tour-lsp")` and starts the
first executable found on the shell `PATH`. There is no download, no bundled
native binary, and no path relative to the user's project.

The supported reproducible local path is:

```sh
cargo install --path crates/tour-lsp --locked --root "$HOME/.local"
# binary: $HOME/.local/bin/tour-lsp
```

The default Cargo install path, `$HOME/.cargo/bin/tour-lsp`, is also supported
because it is normally on `PATH`. This keeps local development explicit and
reproducible: the same command on every platform installs the same server from
the checked-out source.

If `tour-lsp` is not on `PATH`, the extension fails with a clear install hint.
It does not silently fall back to a download.

### Future Zed extension-registry release

The registry contract is:

1. Prefer a `tour-lsp` already on `PATH`, so local source builds keep working.
2. Otherwise read the latest GitHub release for
   `matej-douro/zed-code-tour`, select the archive for
   `zed::current_platform()`, and download it with `zed::download_file`.
3. Unpack into Zed's extension work directory under
   `tour-lsp-<version>/`, call `zed::make_file_executable`, and cache the path
   for later `language_server_command` calls.
4. Remove older cached versions after a successful download.

This download path is not implemented in version 0.1.0. The repository is
private and `License: TBD` blocks a public registry release. The asset naming
and layout below are fixed now so the release script and the future extension
download code can be implemented together without changing published artifact
names.

| Zed platform | Asset |
| --- | --- |
| Linux x86_64 | `tour-lsp-<version>-x86_64-unknown-linux-gnu.tar.gz` |
| Linux aarch64 | `tour-lsp-<version>-aarch64-unknown-linux-gnu.tar.gz` |
| macOS Intel | `tour-lsp-<version>-x86_64-apple-darwin.tar.gz` |
| macOS Apple Silicon | `tour-lsp-<version>-aarch64-apple-darwin.tar.gz` |
| Windows x86_64 | `tour-lsp-<version>-x86_64-pc-windows-msvc.tar.gz` |

Each archive contains the binary at its root: `tour-lsp`, or `tour-lsp.exe` on
Windows. The archive uses the Rust target triple so a release matrix can build
each platform with the standard toolchain.

## Local release artifacts

`./scripts/package-release.sh` builds and packages the artifacts for the host
platform:

```text
dist/
  extension-<version>.wasm
  zed-code-tour-<version>-extension.tar.gz
  tour-lsp-<version>-<host-target>.tar.gz
  SHA256SUMS
```

The extension archive expands to a directory containing `extension.toml`,
`extension.wasm`, `README.md`, and `languages/tour/config.toml`. It can be
loaded with `Install Dev Extension` after extracting it.

The server archive expands to a directory containing the `tour-lsp` binary. Put
that binary on `PATH` to use it with the extension.

The script packages only the host target. A full release for the registry needs
one archive per row in the table above, built on the matching platform or with a
cross-compilation matrix.

## License gate

`README.md` currently says `License: TBD`. That is not a usable public license.
Until a license is chosen and the README is updated:

- `scripts/package-release.sh` refuses to create release artifacts. Set
  `ALLOW_UNLICENSED_RELEASE=1` only to verify the packaging locally.
- Do not publish a GitHub release, attach artifacts to a public release, or
  submit the extension to the Zed registry.
- The release workflow template uses the same gate, so a tag build fails while
  the license is unresolved.

Resolving the license is a prerequisite for the registry release, not part of
version 0.1.0 packaging.
