# Agent notes

User-facing setup and usage docs live in [README.md](README.md). Build and
test commands are in [README.md#development](README.md#development).

## Layout

- `HANDOFF.md`: design handoff and source of truth.
- `extension.toml`: Zed extension manifest. Declares the `tour` language server
  and the source languages it attaches to.
- `languages/tour/config.toml`: Tour language registration for `*.tour.md`.
  Zed requires language metadata in this directory; `extension.toml` only
  references language servers.
- `Cargo.toml` and `src/lib.rs`: the repository root is the
  `zed-code-tour-extension` Cargo package and the workspace root. The extension
  is built as a `cdylib` for `wasm32-wasip2`. Zed's dev-extension builder
  compiles the selected directory, so the package manifest, extension manifest,
  and language assets share the repository root.
- `crates/tour-lsp`: native language server binary speaking LSP over stdio.
  `crates/tour-lsp/src/tour` contains the `.tour.md` parser and tour model.
  `crates/tour-lsp/src/settings.rs` and `state.rs` contain the validated
  settings and active-tour state machine. Character-range conversion and
  navigation commands live in `navigation.rs`. `src/hint.rs` builds the body
  hint and `src/controls.rs` builds the source step control row. See
  [docs/lsp-state.md](docs/lsp-state.md),
  [docs/navigation.md](docs/navigation.md),
  [docs/body-hint.md](docs/body-hint.md),
  [docs/step-controls.md](docs/step-controls.md), and
  [docs/thread-link.md](docs/thread-link.md).
- `examples/zed-code-tour.tour.md`: sample tour over this repository. A test
  keeps it parseable and verifies that its relative targets exist.
- `skills/zed-code-tour/SKILL.md`: ready-to-install Zed agent skill for
  writing and starting tours. See [skills/README.md](skills/README.md).
- `skills/README.md`: skill install paths and why Zed extensions cannot
  register skills automatically.
- `scripts/build-extension.sh`: builds the WASM extension to `extension.wasm`.
- `scripts/check-packaging.sh`: verifies manifest metadata and version sync.
- `scripts/check-skill.sh`: validates the Zed skill frontmatter.
- `scripts/test.sh`: packaging check, formatting, lint, build, and tests.
- `scripts/e2e-zed.sh` and `scripts/e2e_zed.py`: headless end-to-end run
  against a pinned Zed release under Xvfb, including OCR-driven UI
  interactions. See [docs/e2e.md](docs/e2e.md).
- `scripts/package-release.sh`: builds the host release artifacts under `dist/`.
- `scripts/bump-version.sh`: updates the extension and workspace versions.
- `docs/install.md`: dev extension install, local `tour-lsp` setup, and the
  required Zed settings.
- `docs/packaging.md`: what ships, binary discovery, and release artifact
  naming.
- `docs/release.md` and `docs/release-notes-template.md`: release checklist and
  notes template.
- `docs/testing.md`: automated suite entry points, the fake LSP harness,
  the headless Zed run, and the manual verification matrix.
- `docs/e2e.md`: headless Zed end-to-end case table, dependencies, and
  artifacts.

