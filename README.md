# zed-code-tour

A Zed extension plus language server for LSP-driven code tours written as
`.tour.md` files. A tour file selects exact character ranges in source files and
renders a virtual annotation block below the selection, with Previous / Next /
Overview / Stop controls as a code lens row that hides the boundary controls.
Source buffers are never edited.

Status: early implementation. The extension and language server start and
speak LSP, the tour file parser and model are implemented, the language server
tracks one active tour, and the tour file has a start/stop code lens with the
`always`, `prompt`, and `never` auto-start modes. Character-range navigation
selects exact UTF-16 ranges with `window/showDocument`. Agent-facing usage docs
and a parse-tested sample tour are available. The active step is rendered as a
body inlay hint in the source file, and the active source file has a step control
code lens row. A tour thread link is stored and exposed, and the control row can
focus it and explain the current routing gap. Full thread routing needs a Zed
core follow-up. A headless end-to-end harness loads the built extension into a
real Zed window under Xvfb, asserts the LSP transcript, drives real code lens and
prompt interactions, and saves screenshots.
The full design, constraints, and open questions live in
[HANDOFF.md](HANDOFF.md).

## Roadmap

- [ ] Spike: verify code lens and inlay hint ordering and placement
- [x] Scaffold Zed extension and tour LSP workspace
- [x] Tour file parser and tour model
- [x] LSP initialization, settings, and active tour state
- [x] Start/stop code lens and start flow
- [x] Body inlay hint renderer
- [x] Step control code lens row
- [x] Character-level range navigation with `window/showDocument`
- [x] Agent integration, sample tour, and usage docs
- [x] Thread link support and routing gap
- [x] Test harness and manual verification matrix
- [x] Headless Zed end-to-end harness
- [x] Packaging, install docs, and CI/release
- [x] Ready-to-install Zed agent skill

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
- `skills/SKILL.md`: ready-to-install Zed agent skill for writing and starting
  tours. See [skills/README.md](skills/README.md).
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
- `docs/ci.md`: CI/release workflow status, the workflow-scope blocker, and
  the one-step enable procedure tracked in #25.
- `docs/testing.md`: automated suite entry points, the fake LSP harness,
  the headless Zed run, and the manual verification matrix.
- `docs/e2e.md`: headless Zed end-to-end case table, dependencies, and
  artifacts.

Zed has no wildcard language registration, so the `tour` server is registered
explicitly for the Tour language and for each supported source language in
`extension.toml`. The initial list covers the built-in languages targeted by the
scaffold; add source languages there as tour overlay support grows.

The Tour language takes precedence over Markdown for `*.tour.md` files. In this
scaffold it has no grammar, so tour files do not get Markdown preview, syntax
highlighting, or Markdown-specific editing features. The language server still
reads tour files as Markdown. A follow-up can attach the Markdown grammar and
queries to the Tour language, or attach the server to Markdown and drop the Tour
language.

## Settings

Tour settings live under the language server settings category:

```json
{
  "lsp": {
    "tour": {
      "settings": {
        "placement": "below",
        "auto_start": "prompt"
      }
    }
  }
}
```

`placement` is `below` (default) or `above`. `auto_start` is `always`, `prompt`
(default), or `never`. Absent or invalid values fall back to the defaults. The
extension reads these settings and passes them to the server as initialization
options. See [docs/lsp-state.md](docs/lsp-state.md) for the schema, document
filter, multiple-tour policy, and refresh behavior.

## Usage

Tour files are Markdown with a `.tour.md` suffix. Put the file anywhere in the
worktree. Target paths resolve against the directory that contains the tour
file, so a tour in `docs/` refers to `../src/main.rs`. The sample
[examples/zed-code-tour.tour.md](examples/zed-code-tour.tour.md) is a complete
tour over this repository and is covered by a parser test.

### Write a tour

```markdown
# Auth flow

[Back to the creating thread](zed:///agent/thread/<session_id>?name=Auth%20flow)

## 1. Entry point
[../src/main.rs:12:4-12:20](../src/main.rs#L12:4-L12:20)

The program starts here, then calls the router at line 14.

## 2. Router setup
[../src/router.rs:40:8-40:58](../src/router.rs#L40:8-L40:58)

Paths are registered here.
```

- `# Title` names the tour.
- `## <number>. <title>` starts a step. Steps run in document order.
- The first link in a step is its target. The link text is human-readable; the
  destination is a local file path plus a range fragment. Relative paths resolve
  against the tour file directory.
- Range fragments use `#L<line>:<column>-L<line>:<column>` with 1-based line and
  column numbers. The start position is inclusive and the end position is
  exclusive, matching LSP ranges. Ranges may span lines.
- A `zed:///agent/thread/<session_id>?name=...` link records the creating
  thread. A tour with this link gets a `Back to thread` control that focuses the
  link line and shows the URI. Zed does not route thread URIs from code lens or
  preview yet; see [docs/thread-link.md](docs/thread-link.md) and the core
  follow-up in #22.
- The first paragraph of the step body is the annotation hint. Keep it short.
  A hidden `<!-- tour-step: {"hint": "Short text"} -->` comment overrides it.
- Do not put placement or auto-start in the tour file. Those are user settings.

### Agent flow

The end-to-end flow is:

1. The agent writes a `*.tour.md` file and leaves source files unchanged.
2. The agent emits a `ResourceLink` named `Start tour` that points at the tour
   file.
3. The user clicks the link. Zed opens the file with the Tour language, and the
   language server receives `didOpen`.
4. The `auto_start` setting decides whether to start immediately, ask first, or
   wait for the start lens.
5. Once started, the tour selects step 0 and the tour-file lens shows stop and
   restart controls.

The ResourceLink is an ACP content block in agent output. Its `uri` must be an
absolute `file://` URI:

```json
{
  "type": "resource_link",
  "name": "Start tour",
  "uri": "file:///workspace/acme/docs/auth.tour.md",
  "mimeType": "text/markdown"
}
```

Point the link at the tour file, not at a source file. The name is the clickable
label.

### Agent skill

`skills/SKILL.md` is a ready-to-install Zed skill that teaches an agent the tour
format, range rules, start flow, and verification checklist. Zed loads skills
from `~/.agents/skills/<name>/SKILL.md` or `<project>/.agents/skills/<name>/SKILL.md`;
extensions cannot register skills. Copy the file into a `zed-code-tour/`
directory under one of those roots. See [skills/README.md](skills/README.md) for
the commands and the one-level discovery rule.

### `auto_start` modes

`auto_start` defaults to `prompt`.

| Mode | Behavior | Tradeoff |
| --- | --- | --- |
| `prompt` (default) | On `didOpen`, asks `Start tour` or `Not now`. | One extra click, but opening a tour to read or edit it does not start a tour unexpectedly. |
| `always` | Starts at step 0 as soon as the tour file opens. | The shortest agent flow, but any open starts the tour, including reopening the file later. Use it when opening a tour always means starting one. |
| `never` | Does not prompt or start. Waits for the start/stop code lens. | Fully manual, but it needs code lens. With code lens off there is no visible start control in v1. |

The agent cannot override `auto_start` per tour. It is the user's setting in
`lsp.tour.settings`.

### Code lens caveat

The tour-file start/stop lens and the source step control row are code lenses.
Code lens is off by default. With code lens off:

- `auto_start: "always"` still starts the tour.
- `auto_start: "prompt"` still shows the start prompt.
- The start/stop lens is not rendered, and the step controls are not available.
- `auto_start: "never"` leaves no visible way to start the tour.

Enable code lens in Zed (`"code_lens": "on"`) when you want the manual start lens
or step controls.

### Out of scope for v1

- There is no MCP `start_tour` tool. Agents start a tour by writing a
  `.tour.md` file and emitting a `Start tour` ResourceLink.
- There is no inlay-part control fallback. Step controls require code lens; with
  code lens off, the tour can start but has no step controls.
- Thread routing from code lens or preview needs a Zed core patch. The v1
  fallback focuses the link and shows the URI; it does not silently fail. See
  [docs/thread-link.md](docs/thread-link.md) and #22.
- Tours do not edit source buffers. Selections and annotations are virtual.

### Copy-paste agent instruction

Use this as custom instructions for an agent that should produce tours:

```text
When the user asks for a code walkthrough, write a tour file and link it.

1. Write a Markdown file named <topic>.tour.md in the worktree. Do not modify
   source files.
2. Use this format, with steps in visit order:

   # <tour title>

   [Back to the creating thread](zed:///agent/thread/<session_id>?name=<thread name>)

   ## 1. <step title>
   [<relative path>:<start line>:<start column>-<end line>:<end column>](<relative path>#L<start line>:<start column>-L<end line>:<end column>)

   <short explanation for this step>

   ## 2. <step title>
   [<relative path>:<start line>:<start column>-<end line>:<end column>](<relative path>#L<start line>:<start column>-L<end line>:<end column>)

   <short explanation for this step>

3. Paths are relative to the tour file. Lines and columns are 1-based. The
   range start is inclusive and the range end is exclusive, matching LSP. The
   first link in each step is the target. Ranges may span lines.
4. After writing the file, emit an ACP ResourceLink named "Start tour" with the
   absolute file URI, for example:
   {"type":"resource_link","name":"Start tour","uri":"file:///workspace/acme/docs/auth.tour.md","mimeType":"text/markdown"}
   Do not call an MCP start_tour tool; it does not exist in v1.
5. If you cannot obtain the agent thread id, omit the thread link.
```

## Development

Requirements:

- Rust with the `wasm32-wasip2` target. The pinned toolchain in
  `rust-toolchain.toml` lists the target; `scripts/build-extension.sh` also
  installs it through rustup when needed.
- Network access to crates.io on the first build. The extension uses released
  `zed_extension_api` 0.7.0, the newest extension API supported by Zed Stable
  and Preview, so its wasm declares `since_v0_6_0` and loads on those channels.
  The 0.8.0 schema hooks are deferred to a draft PR; they provide cosmetic
  settings-editor validation and are not required for runtime settings.
- Zed Stable or Preview for loading the extension as a dev extension.

Build and test the workspace:

```sh
./scripts/test.sh
```

This runs the packaging metadata check, `cargo fmt --check`, `cargo clippy`,
`cargo build`, and `cargo test` for the workspace. The equivalent manual
commands are:

```sh
cargo build --workspace --locked
cargo test --workspace --locked
```

The integration tests under `crates/tour-lsp/tests/` start the real server
binary over stdio. Run the headless end-to-end checks against a pinned Zed
release with:

```sh
./scripts/e2e-zed.sh
```

To inspect the payload assertions, the headless cases, and the manual checks
that need a live Zed editor, see [docs/testing.md](docs/testing.md) and
[docs/e2e.md](docs/e2e.md).

Build the WASM extension:

```sh
./scripts/build-extension.sh
```

The script builds the root `zed-code-tour-extension` package in release mode
and writes `extension.wasm` at the repository root. That file is build output
and is not committed. A dev-extension install does not require this prebuild:
Zed recompiles the selected directory and writes
`target/wasm32-wasip2/debug/zed_code_tour_extension.wasm` itself.

Install the language server so the extension can find it in `PATH`:

```sh
cargo install --path crates/tour-lsp --locked --root "$HOME/.local"
```

Load the extension in Zed:

1. Open the extensions view with `zed: extensions`.
2. Click `Install Dev Extension` and choose the repository root, the directory
   that contains `Cargo.toml`, `extension.toml`, and `languages/`.
3. Zed compiles `zed-code-tour-extension` from the selected directory and loads
   the resulting `zed_code_tour_extension.wasm`.

Enable `code_lens: "on"`, `inlay_hints.enabled: true`, and the
`lsp.tour.settings` block shown in [docs/install.md](docs/install.md). Both
overlays are off by default.

Opening a `*.tour.md` file uses the Tour language and starts the `tour`
language server. The server handles `initialize`, `initialized`, `didOpen`,
`didChange`, `didClose`, `workspace/didChangeConfiguration`,
`textDocument/codeLens`, `workspace/executeCommand`, `shutdown`, and `exit`;
advertises the tour caps and commands; tracks the active tour; returns the
tour-file start/stop lens, the source-file step control row, and the body inlay
hint for the active step.

## Packaging, install, and release

- [Install and local setup](docs/install.md) covers the dev extension, the
  local `tour-lsp` binary, required Zed settings, and verification.
- [Packaging and distribution](docs/packaging.md) defines what ships, how the
  extension finds `tour-lsp`, and the future registry asset names.
- [Release checklist](docs/release.md) covers version bumps, tags, artifacts,
  and notes. [CI workflow status](docs/ci.md) records the workflow-scope
  blocker and the one-step enable procedure tracked in #25.

License: TBD. This repository is not licensed for redistribution. A public
GitHub release and a Zed extension-registry submission are blocked until a
license is chosen, `LICENSE` is added, and this line is updated. The release
script enforces this gate.
