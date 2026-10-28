# zed-code-tour

A Zed extension plus language server for LSP-driven code tours written as
`.tour.md` files. A tour file selects exact character ranges in source files and
renders a virtual annotation block below the selection, with Previous / Next /
Overview / Stop controls as a code lens row. Source buffers are never edited.

Status: scaffolding. The extension and language server start and speak LSP, but
tour behavior is not implemented yet. The full design, constraints, and open
questions live in [HANDOFF.md](HANDOFF.md).

## Roadmap

- [ ] Spike: verify code lens and inlay hint ordering and placement
- [x] Scaffold Zed extension and tour LSP workspace
- [ ] Tour file parser and tour model
- [ ] LSP initialization, settings, and active tour state
- [ ] Start/stop code lens and start flow
- [ ] Body inlay hint renderer
- [ ] Step control code lens row
- [ ] Character-level range navigation with `window/showDocument`
- [ ] Agent integration, sample tour, and usage docs
- [ ] Thread link support and routing gap
- [ ] Test harness and manual verification matrix
- [ ] Packaging, install docs, and CI/release

## Layout

- `HANDOFF.md`: design handoff and source of truth.
- `extension.toml`: Zed extension manifest. Declares the `tour` language server
  and the source languages it attaches to.
- `languages/tour/config.toml`: Tour language registration for `*.tour.md`.
  Zed requires language metadata in this directory; `extension.toml` only
  references language servers.
- `crates/zed-code-tour-extension`: extension crate, built as a `cdylib` for
  `wasm32-wasip2`.
- `crates/tour-lsp`: native language server binary speaking LSP over stdio.
- `scripts/build-extension.sh`: builds the WASM extension to `extension.wasm`.
- `scripts/test.sh`: formatting, lint, build, and test checks.

Zed has no wildcard language registration, so the `tour` server is registered
explicitly for the Tour language and for each supported source language in
`extension.toml`. Source languages can be added to that list as tour overlay
support grows.

## Development

Requirements:

- Rust with the `wasm32-wasip2` target. The pinned toolchain in
  `rust-toolchain.toml` lists the target; `scripts/build-extension.sh` also
  installs it through rustup when needed.
- Zed for loading the extension as a dev extension.

Build and test the workspace:

```sh
./scripts/test.sh
```

This runs `cargo fmt --check`, `cargo clippy`, `cargo build`, and
`cargo test` for the workspace. The equivalent manual commands are:

```sh
cargo build --workspace --locked
cargo test --workspace --locked
```

Build the WASM extension:

```sh
./scripts/build-extension.sh
```

The script writes `extension.wasm` at the repository root. That file is build
output and is not committed.

Install the language server so the extension can find it in `PATH`:

```sh
cargo install --path crates/tour-lsp --locked
```

Load the extension in Zed:

1. Run `./scripts/build-extension.sh`.
2. Open the extensions view with `zed: extensions`.
3. Click `Install Dev Extension` and choose the repository root.

Opening a `*.tour.md` file uses the Tour language and starts the `tour`
language server. The server currently handles `initialize`, `initialized`,
`shutdown`, and `exit`, and returns no capabilities.

License: TBD
