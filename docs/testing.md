# Testing and manual verification

The project has three test paths:

1. An automated suite that runs the Rust unit tests and a fake LSP client over
   stdio against the real `tour-lsp` binary.
2. A headless end-to-end run that loads the built extension into a real Zed
   window under Xvfb, drives the extension through Zed, asserts the LSP
   transcript, and saves screenshots.
3. A manual verification matrix for behavior that still needs a person at a
   live editor: painting details, scrolling, soft wrap, and thread links.

The automated suite covers protocol payloads and layout anchors. The headless
run covers the extension-to-Zed wiring and records what was painted. The manual
matrix is the release sign-off for the remaining visual and interactive cases.

## Automated suite

Run everything from the repository root:

```sh
./scripts/test.sh
```

The script runs, in order:

```sh
./scripts/check-packaging.sh
./scripts/check-skill.sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo build --workspace --locked
cargo test --workspace --locked
```

For the tour language server alone:

```sh
cargo test -p tour-lsp --locked
```

For only the fake LSP harness:

```sh
cargo test -p tour-lsp --test harness --locked
```

### Fake LSP harness

`crates/tour-lsp/tests/support/mod.rs` is a small LSP client for integration
tests. It starts the compiled `tour-lsp` binary, performs the initialize
handshake, sends document notifications and requests, reads server-initiated
requests, and fails with a timeout instead of hanging when a payload is missing.

`crates/tour-lsp/tests/harness.rs` drives the server over stdio and asserts:

- initialize settings variants,
- tour `didOpen`, code lens, and inlay hint payloads,
- below, above, and last-line layout anchors,
- leading padding space, wrapping, padding, right bias, and the trailing
  newline,
- `workspace/executeCommand` transitions and no-op boundaries (`tour.next` at
  the last step, `tour.previous` at the first step, and out-of-range `goto`),
- `window/showDocument` URIs, selections, `takeFocus`, and `external`,
- `workspace/inlayHint/refresh` and `workspace/codeLens/refresh` requests,
- server behavior when overlay requests are omitted (the Zed settings gates
  themselves are manual; see M06-M09),
- multiple tours and multiple source files, replacement, and stop cleanup,
- character columns, tabs, multibyte text, CRLF, and the last-line fallback.

`crates/tour-lsp/tests/lifecycle.rs` covers process lifecycle and start/prompt
flows. `crates/tour-lsp/tests/sample_tour.rs` keeps the checked-in sample tour
parseable and verifies that its relative targets exist.

Unit tests beside the implementation cover the parser, state, settings, position
conversion, hint formatting, control row anchors, and server state transitions.
See `crates/tour-lsp/src/*.rs`.

### CI readiness

`./scripts/test.sh` is the CI entry point. The fake LSP harness is headless and
uses only stdio, so it needs no Zed binary or display. The GitHub Actions
workflow templates live in `ci/ci.yml` and `ci/release.yml`; see
[ci.md](ci.md) for the workflow-scope blocker and the one-step move into
`.github/workflows/`. The CI job runs `./scripts/test.sh` and
`./scripts/build-extension.sh`.

### Headless Zed end-to-end run

```sh
./scripts/e2e-zed.sh
```

The run downloads a pinned Zed release, installs the built extension into an
isolated profile, and exercises the settings gates, placement modes, last-line
fallback, relative paths, Unicode ranges, and auto-start modes. It asserts the
LSP transcript and the extension index, and saves one screenshot per case under
`target/e2e/runs/`. See [e2e.md](e2e.md) for the case table, dependencies, and
artifacts.

The headless cases also drive the UI: OCR plus `xdotool` click the real Next,
Previous, Overview, Back to thread, and Stop lenses; answer the auto-start
prompt; switch tabs; open a second tour; resize the window; and scroll the
editor. That covers M01-M17 and M19-M22 at the protocol and interaction level.
Only M18's agent-output `ResourceLink` still needs an agent.

The run is separate from `./scripts/test.sh` because it needs Xvfb, a software
Vulkan driver, `tesseract`, and a Zed download. Use it before the manual pass.
The manual pass remains the sign-off for painting details.

## Manual verification

### Prerequisites

- Zed Stable, Preview, or a newer development build. The extension uses
  released `zed_extension_api` 0.7.0, the newest extension API supported by
  Stable and Preview. See the Development section in `README.md`.
- The extension installed as a dev extension, and `tour-lsp` on `PATH`:

  ```sh
  cargo install --path crates/tour-lsp --locked
  ```

  `./scripts/build-extension.sh` builds the prebuilt release WASM, but Zed's dev
  install recompiles the extension from the repository root.

- A scratch Zed project. The examples below use a temporary directory, but any
  workspace works. Tour paths resolve relative to the tour file.

### Baseline Zed settings

Start each case from this settings block and change only the fields named by the
case:

```json
{
  "code_lens": "on",
  "inlay_hints": {
    "enabled": true,
    "show_background": false
  },
  "lsp": {
    "tour": {
      "settings": {
        "placement": "below",
        "auto_start": "never"
      }
    }
  }
}
```

With `auto_start: "never"`, open the tour file and run the start lens to begin.
For prompt and always cases, reopen the tour file after changing settings.

### Manual fixtures

Create a scratch project and two source files:

```sh
scratch="$(mktemp -d)"
printf 'line0\nline1 long enough\nline2\nline3 long enough\n' \
  > "$scratch/basic.rs"
printf 'line0\nline1 long enough\nline2\nline3 long enough' \
  > "$scratch/last-no-newline.rs"
printf 'line0\nline1 long enough\nline2\nline3 long enough\n' \
  > "$scratch/last-with-newline.rs"
printf 'let\t😀 = 1;\r\nlet café = 2;\r\nlast\t値' \
  > "$scratch/unicode.rs"
```

Create `basic.tour.md` in the same directory:

```markdown
# Manual tour

[Back to the thread](zed:///agent/thread/manual-session?name=Manual%20tour)

## 1. Entry
[basic.rs:2:3-2:9](basic.rs#L2:3-L2:9)

Entry body line one.
Entry body line two.

## 2. Second
[basic.rs:4:1-4:4](basic.rs#L4:1-L4:4)

Second body.
```

Open `basic.tour.md` in Zed as a worktree file. For the last-line and Unicode
cases, create a one-step tour whose target points at the relevant file and line,
or edit the target link in `basic.tour.md`.

### Headless e2e cases by matrix row

| Matrix row | E2E case |
| --- | --- |
| M01 | `below` |
| M02 | `above` |
| M03 | `below`, `navigate` |
| M04 | `last-line`, `last-line-newline` |
| M05 | `below` (background), `background-off` |
| M06 | `below` |
| M07 | `inlay-off`, `inlay-off-navigate` |
| M08 | `below`, `never`, `navigate` |
| M09 | `code-lens-off`, `prompt-code-lens-off` |
| M10 | `below` |
| M11 | `prompt`, `prompt-accept`, `prompt-decline` |
| M12 | `never` |
| M13 | `never`, `prompt` (Tour tab and language id) |
| M14 | `long-body`, `narrow` |
| M15 | `scroll` |
| M16 | `multiple-tours`, `navigate` (stop cleanup) |
| M17 | `relative-dotdot`, `absolute-path`, `unicode`, `unicode-cafe`, `unicode-last` |
| M18 | `thread-link` (fallback); agent output remains manual |
| M19 | `last-line-newline` |
| M20 | `last-line`, `unicode-last` |
| M21 | `unicode`, `unicode-cafe`, `unicode-last` |
| M22 | `plain-text`, `markdown-source` |

### Matrix

Record the Zed version, OS, date, relevant settings, and a note or screenshot
for each case. Put `x` in `Pass` or `Fail`. A failure gets a follow-up issue; do
not change product behavior as part of this test pass.

| ID | Area | Steps | Expected result | Automated cover | Pass | Fail |
| --- | --- | --- | --- | --- | --- | --- |
| M01 | Below placement | Below settings, open a tour, start it. | Selected code is visible, the body hint is below the selection, and the control row is between the selected code and the body. Source buffer text is unchanged. | `harness.rs::layout_below_*`, `hint.rs`, `controls.rs` | [ ] | [ ] |
| M02 | Above placement | Set `placement: "above"`, restart the tour. | Body hint is directly above the selection. Control row is above the body hint, not between the body and the selection. | `harness.rs::layout_above_*`, `hint.rs`, `controls.rs` | [ ] | [ ] |
| M03 | Control row below | Below settings, start `basic.tour.md`, inspect the row. | At the first step the row reads `1/2 \| Next ▶ \| Overview \| Stop` and sits between the selected code and the body hint. On the next step it reads `◀ Prev \| 2/2 \| Overview \| Stop`. | `harness.rs::layout_below_*`, `controls.rs`, `server.rs` | [ ] | [ ] |
| M04 | Last-line fallback | Target the last line of `last-no-newline.rs`, start below. | Body hint follows the selected line. Control row is above the selected line because no line follows it. The row and body do not overlap. | `harness.rs::layout_last_line_without_*`, `hint.rs`, `controls.rs` | [ ] | [ ] |
| M05 | One hint plus lens | Enable `inlay_hints.show_background`, start a step with a two-line body. | Exactly one body hint and one separate code lens row render. There is no second inlay hint, no per-hint color, weight, or italic styling, and the background is rectangular. | `harness.rs`, `hint.rs` | [ ] | [ ] |
| M06 | Inlay hints on | `inlay_hints.enabled: true`, code lens on, start a tour. | Body hint renders with one leading padding space and uniform padding, and no left rule. | `harness.rs`, `hint.rs` | [ ] | [ ] |
| M07 | Inlay hints off | `inlay_hints.enabled: false`, code lens on, start a tour. | Body hint is absent. Control row still renders and Previous/Next still move the selection. | `harness.rs::inlay_hints_off_*` (protocol proxy), `server.rs` | [ ] | [ ] |
| M08 | Code lens on | `code_lens: "on"`, open a tour. | Tour file has `▶ Start tour (N steps)` at the top. A running tour has `■ Stop tour \| ⟳ Restart \| n/N \| ▶ Resume` and the source control row in its boundary shape: `1/N \| Next ▶ \| Overview \| Stop` at the first step, `◀ Prev \| N/N \| Overview \| Stop` at the last. After Stop on a later step the tour file also offers `▶ Resume \| n/N`, which returns to that step instead of restarting at step 0. | `harness.rs`, `server.rs`, `controls.rs` | [ ] | [ ] |
| M09 | Code lens off | `code_lens: "off"`, reopen the tour. | No start lens and no source control row. `auto_start: "always"` still starts and `prompt` still asks. The missing step controls are expected in v1. | `harness.rs::prompt_*`, `both_overlays_off_*` (protocol proxy) | [ ] | [ ] |
| M10 | `auto_start: always` | Set `auto_start: "always"`, reopen the tour file. | Tour starts immediately at step 0 and selects its range. No prompt appears. | `lifecycle.rs::auto_start_always_*`, `server.rs` | [ ] | [ ] |
| M11 | `auto_start: prompt` | Set `auto_start: "prompt"`, reopen the tour file. | A prompt offers `Start tour` and `Not now`. Start begins at step 0; Not now leaves the tour inactive. | `lifecycle.rs::auto_start_prompt_*`, `harness.rs::prompt_*` | [ ] | [ ] |
| M12 | `auto_start: never` | Set `auto_start: "never"`, reopen the tour file. | Nothing starts automatically. With code lens on, `▶ Start tour (N steps)` starts it. With code lens off there is no visible start control. | `harness.rs::layout_below_*`, `server.rs` | [ ] | [ ] |
| M13 | Tour file in the editor | Open `basic.tour.md` as a normal editor tab. | It stays readable Markdown source in the raw editor. Markdown preview, highlighting, and Markdown editor features are unavailable until the Tour language gets the Markdown grammar or the server attaches to Markdown. The LSP still parses the file. | `lifecycle.rs`, `parser.rs` | [ ] | [ ] |
| M14 | Long body, narrow window, soft wrap | Use a step with more than six body lines and lines longer than 80 characters. Narrow the window and enable soft wrap. | Long lines wrap at word boundaries and the body is capped at six wrapped lines. `…` appears only when the line budget overflows, on the last retained line. Soft wrap keeps the hint readable and does not edit the source buffer. | `hint.rs::wraps_long_lines_at_word_boundaries_without_truncating`, `hint.rs::hard_wraps_words_longer_than_the_budget`, `hint.rs::clamps_long_bodies_to_the_line_budget` | [ ] | [ ] |
| M15 | Scrolling while active | Start a tour, scroll away from the selection, navigate Next and Previous, and scroll back. | The selection follows the step. Hints and lenses refresh correctly. There is no unacceptable scroll jump when the hint is inserted, and the overlay stays attached to the right lines. | Manual only | [ ] | [ ] |
| M16 | Multiple tours and source files | Start `basic.tour.md`, then open a second tour with a different target. Navigate across files, then Stop. | The last opened tour wins. Overlays for the old tour and old source disappear. Only the current source has a hint and control row. Stop removes the current overlays, leaves the selection alone, and returns the tour-file lens to `▶ Start tour`, plus `▶ Resume \| n/N` when a later step is still open. | `harness.rs::multiple_tours_*`, `state.rs` | [ ] | [ ] |
| M17 | Relative and absolute paths, character columns | Put a tour in a subdirectory with relative targets that use `.` and `..`, then try an absolute target. Use a character-level and cross-line range. | Relative paths resolve against the tour directory and normalize `.`/`..` lexically. Absolute paths work. `window/showDocument` and the source overlays use the same normalized file URI. `window/showDocument` selects the exact inclusive-start, exclusive-end character range. | `harness.rs::character_*`, `parser.rs::normalizes_dot_segments_*`, `state.rs::parent_targets_*`, `server.rs::overlay_handlers_accept_normalized_*`, `sample_tour.rs`, `navigation.rs` | [ ] | [ ] |
| M18 | Thread link | Open a tour with a `zed:///agent/thread/...` link from agent output, then try the link in the tour file and Markdown preview. | The link works from agent output. The tour file stores and displays the URI. Preview and code lens routing may not open the agent panel in this version; that gap is documented in `HANDOFF.md` and issue #10. | `parser.rs` thread link tests, `sample_tour.rs` | [ ] | [ ] |
| M19 | Last line with trailing newline | Target the last content line of `last-with-newline.rs`, start below. | The empty final LSP line is used. Body hint and control row anchor there instead of using the end-of-line fallback. | `harness.rs::layout_last_line_with_*`, `hint.rs`, `controls.rs` | [ ] | [ ] |
| M20 | Last line without trailing newline | Target the last line of `last-no-newline.rs`, start below. | End-of-line fallback is used: the hint label starts with a newline and the control row anchors at the selection start line. | `harness.rs::layout_last_line_without_*` | [ ] | [ ] |
| M21 | Tabs, multibyte, CRLF | Target the tab, emoji, accented text, and final line in `unicode.rs`. | Tab counts as one column. Emoji selects exact UTF-16 code units. CRLF is not counted as part of a line. The final-line hint anchors after the last character. | `harness.rs::character_*`, `navigation.rs` | [ ] | [ ] |
| M22 | Text formats and unlisted languages | Target a `.txt` file and a Markdown file. Then try a file with an unknown extension. | `.txt` attaches through `Plain Text`; Markdown attaches directly. A file with an unknown extension has no language; add the language to `extension.toml` or set the buffer language to `Plain Text`. | `e2e: plain-text, markdown-source` | [ ] | [ ] |

### Cleanup

Stop any running tour, close the scratch project, and remove temporary files.
Do not commit manual fixtures unless they are stable sample tours with tests.
