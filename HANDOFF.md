# Handoff: LSP-driven code tours for Zed

Status: design handoff, not implemented.

Source research is against Zed HEAD
`37faef81f74fc1bb08be9939c4cffc8ede6ced2f` in `/workspace/zed-industries/zed`.

## Task

Build a Zed extension and language server that turns a static `.tour.md` file
into a guided code tour.

The extension should:

- let an agent write a human-readable Markdown tour file,
- expose a start control at the top of the tour file,
- select exact character ranges in source files,
- show a virtual annotation block below the selected code,
- provide Previous / Next / step / Overview / Stop controls as a code lens row,
- optionally link back to the creating agent thread.

Do not edit source buffers. The tour is virtual text and selection only.

## Final design

- Tour file is Markdown with a `.tour.md` suffix.
- Steps are `## <number>. <title>` headings.
- The first link in a step is the target. Example:
  `[src/main.rs:12:4-12:20](src/main.rs#L12:4-L12:20)`.
- Character-level ranges come from the link fragment.
- The body is one multi-line inlay hint using the stock `hint` style.
- Controls are a single-line code lens row using the stock code lens style.
- No per-hint or per-lens styling is requested. No italic, bold, color, or
  weight configuration.
- Padding spaces and a left rule (`▎`) make the body block readable. If the
  user enables `inlay_hints.show_background`, the padding gives it a
  rectangular background.
- Body placement is below the selection by default.
- For below placement, the control code lens is anchored at the line after the
  selection, so it appears between the selected code and the body hint.
- Placement and auto-start are extension settings, not tour-file fields.
- The tour file has a start/stop code lens at the top.
- The agent ends by writing the tour file and emitting a `ResourceLink` named
  "Start tour". Opening the file starts the LSP.
- The tour file may contain a `zed:///agent/thread/<session_id>?name=...` link
  for the creating thread.

## Constraints

- Extension and LSP only. No custom panels, webviews, tab toolbars, or arbitrary
  GPUI views.
- Do not modify source buffers to render overlays.
- Use stock inlay hint and code lens styles. Do not attempt per-item styling.
- Do not split the body and controls into two inlay hints for styling. All LSP
  hints share one style.
- Code lens is required for step controls in v1. There is no inlay-part control
  fallback in the first version.
- Both inlay hints and code lens are off by default. The extension cannot
  enable either setting.
- The tour file must remain valid, human-readable Markdown.
- Keep the body hint short. Put the full explanation in the tour file.
- No MCP `start_tour` tool in v1. The agent opens the tour file through a
  resource link.

## Tour file format

Example:

```markdown
# Auth flow

[Back to the thread](zed:///agent/thread/<session_id>?name=...)

## 1. Entry point
[src/main.rs:12:4-12:20](src/main.rs#L12:4-L12:20)

The program starts here, then calls the router at line 14.

## 2. Router setup
[src/router.rs:40:8-40:58](src/router.rs#L40:8-L40:58)

This is where paths are registered.
```

Rules:

- `# Title` is the tour title.
- `## <number>. <title>` starts a step.
- The first link in a step is the target. Resolve relative paths against the
  tour file.
- The fragment is `#L<line>:<column>-L<line>:<column>`. Lines and columns are
  1-based in the file; convert to 0-based LSP positions.
- The link text is human-readable and may include line and column numbers.
- The step body is Markdown. Use the first paragraph or a hidden `hint` field
  as the plain-text body hint.
- A `zed:///agent/thread/...` link is the creating thread link.
- Hidden metadata uses HTML comments. Markdown tooling skips comments, so the
  tour file stays valid Markdown. The current Tour language has no grammar, so
  Markdown preview and highlighting are unavailable on `.tour.md` files until a
  Markdown grammar is attached or the file is opened as Markdown (see
  Architecture).
- Do not put placement or auto-start in the tour file.

Optional hidden metadata:

```markdown
<!-- tour-step: {"hint": "Short plain-text block text"} -->
```

Prefer the visible link for path and range. Use comments only for data that
cannot be expressed in the link.

## Architecture

Extension:

- Defines a language server named, for example, `tour`.
- Registers it for the Tour language and for the source languages where
  overlays should appear. There is no wildcard language registration.
- The Tour language owns `*.tour.md` through `languages/tour/config.toml`. It
  takes precedence over Markdown, so `.tour.md` files do not get Markdown
  preview, highlighting, or other Markdown editor features until the Tour
  language is given the Markdown grammar and queries. Attaching the server to
  Markdown and dropping the Tour language is the alternative.
- The server filters Tour and Markdown buffers by the `.tour.md` suffix and
  ignores other buffers.

Language server:

- Parses `.tour.md` files.
- Maintains active tour state: tour id, step index, source file, range.
- Returns code lenses for the tour file and for the active source file.
- Returns one body inlay hint for the active step.
- Handles `workspace/executeCommand` from code lenses.
- Sends `window/showDocument` to select the step range.
- Sends `workspace/inlayHint/refresh` and `workspace/codeLens/refresh` after
  state changes.
- Stops returning the hint and refreshes when the tour stops.

Suggested capabilities:

- `inlayHintProvider: true`
- `codeLensProvider: { resolveProvider: false }`
- `executeCommandProvider: { commands: [...] }`

Suggested commands:

- `tour.start`
- `tour.stop`
- `tour.next`
- `tour.previous`
- `tour.goto` with step index
- `tour.overview`
- `tour.backToThread`

## LSP behavior

On `didOpen` of a `*.tour.md`:

- Parse the tour.
- Replace any active tour.
- Apply the auto-start setting.
- Select step 0 if starting.
- Refresh lenses and hints.

On `textDocument/codeLens` for a tour file:

- Return a lens at the first line.
- Inactive: `▶ Start tour (N steps)`.
- Active: `■ Stop tour | ⟳ Restart | 2/N`.

On `textDocument/codeLens` for an active source file:

- For `placement: "below"`, return one control row at the line after the
  selection end line.
- For `placement: "above"`, return the control row at the selection start line.
- Suggested middle-step row: `◀ Prev | 2/N | Next ▶ | Overview | Stop`.
  The first step hides `◀ Prev` and the last step hides `Next ▶`, so no dead
  boundary control is clickable. The counter stays visible in every row.
- The control row uses the stock code lens style.
- The block map is layered above the inlay map, so a lens at the line after the
  selection should render above the body hint and below the selected code.
  Verify visually in the spike.
- Last line fallback: if there is no line after the selection, anchor the
  control row at the selection start line. Controls and hint cannot be adjacent
  in that case.

On `textDocument/inlayHint` for an active source file:

- Return exactly one body hint for the active step.
- For below placement, position it at the start of the line after the selection
  end line.
- For above placement, position it at the start of the selection start line.
- For the last line with below placement, anchor at the end of the selection
  line and prefix the hint text with `\n`.
- The hint text is plain text lines. Start each line with the left rule `▎`.
- Pad lines with spaces to a consistent width for the background rectangle.
- The hint contains the body only. No controls.
- End with `\n` so the code line starts on its own display row.

On `workspace/executeCommand`:

- Update the step or stop the tour.
- Send `window/showDocument` with the next range and `takeFocus: true`.
- Send `workspace/inlayHint/refresh`.
- Send `workspace/codeLens/refresh` when lens labels change.

On stop:

- Stop returning the body hint.
- Send `workspace/inlayHint/refresh` so Zed removes it.
- Leave the user's selection alone.

## Layout and visual rules

Below placement, the intended order is:

1. selected code
2. control code lens
3. body inlay hint
4. next code line

The control lens is anchored at the line after the selection. The body hint is
anchored at the same line. The block map is layered above the inlay map, so the
lens should render above the body hint. This ordering must be verified.

Above placement:

- Body hint is above the selection.
- Control lens is anchored at the selection start line.
- The lens is not between the selection and the hint.

Visual rules:

- Use the stock `hint` style for the body.
- Use the stock code lens style for controls.
- Do not configure italic, weight, color, or per-item styles.
- Keep the left rule and padding.
- Background appears only when the user enables
  `inlay_hints.show_background`.
- No two-hint styling split. All LSP hints share one style.

## Settings

Use the language server settings category. The extension API cannot read
arbitrary top-level extension settings.

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

Values:

- `placement`: `"below"` or `"above"`. Default `"below"`.
- `auto_start`: `"always"`, `"prompt"`, or `"never"`. Default `"prompt"`.

Implementation:

- Read `LspSettings::for_worktree("tour", worktree)`.
- Pass the settings JSON to the LSP as initialization options, or have the LSP
  request configuration.
- Provide `language_server_initialization_options_schema` for validation and
  completion under `lsp.tour.settings`. The released `zed_extension_api` 0.7.0
  does not expose this hook, so the current extension passes settings through
  `language_server_initialization_options` and
  `language_server_workspace_configuration` only. The schema hooks are deferred
  to a draft PR pinned to extension API 0.8.0 and are cosmetic settings-editor
  validation, not runtime plumbing.
- If a first-class `tour` settings section is required, that is a core or
  context-server-settings change.

## Start flow

1. Agent writes `tour.tour.md`.
2. Agent emits a `ResourceLink` named "Start tour" pointing to the file.
3. User clicks the link. Zed opens the file.
4. The LSP receives `didOpen`.
5. `auto_start: "always"` starts the tour immediately.
6. `auto_start: "prompt"` sends `window/showMessageRequest` with `Start tour`
   and `Not now` actions.
7. `auto_start: "never"` waits for the code lens at the top of the tour file.

The start lens at the top of the tour file is the primary manual control. It
shows `▶ Start tour (N steps)` when inactive and
`■ Stop tour | ⟳ Restart | 2/N` when active.

Code lens is off by default, so the prompt and auto-start paths are required for
the agent-open flow to work. Step controls also require code lens in v1.

## Thread link

Thread to tour:

- Agent emits a `ResourceLink` or Markdown link to the tour file.
- The tour file can be opened from the agent panel.

Tour to thread:

- The thread URI is `zed:///agent/thread/<session_id>?name=<name>`.
- The agent panel's link handler opens it with `AgentPanel::open_thread`.
- A `ResourceLink` with that URI works in agent output today.
- The Markdown preview and code lens command do not route that URI to the agent
  panel. The preview uses its own link handler, and the app's open listener does
  not parse thread paths.
- To make "Back to thread" work from the control row or preview, add routing for
  `zed:///agent/thread/<id>` to the app's open-request path and call
  `AgentPanel::open_thread`. Then the LSP command can send
  `window/showDocument` with `external: true` and the thread URI.
- Without that patch, show the thread URI as text or link to a static export.

## Acceptance criteria

- [ ] A sample `.tour.md` parses into title, thread link, and ordered steps.
- [ ] Step ranges include line and column and resolve relative to the tour file.
- [ ] `window/showDocument` selects the exact character range.
- [ ] One multi-line body hint appears below the selection by default.
- [ ] `placement: "above"` moves the body hint above the selection.
- [ ] The body hint uses the left rule and padded lines.
- [ ] The body hint contains no controls.
- [ ] The control code lens executes tour commands.
- [ ] For below placement, the control lens renders between the selected code
  and the body hint.
- [ ] No per-hint or per-lens style configuration is attempted.
- [ ] Previous, Next, Goto, Overview, Stop, and Back to thread are wired.
- [ ] The tour file has a start/stop code lens at the top.
- [ ] `auto_start: always`, `prompt`, and `never` behave as specified.
- [ ] With code lens off, auto-start/prompt still starts the tour, and the
  missing step controls are documented.
- [ ] The overlay never edits the source buffer.
- [ ] The body hint refreshes and disappears correctly on stop.
- [ ] The thread link works in agent output; the routing gap is documented.
- [ ] The extension behaves gracefully when inlay hints or code lens are off.
- [ ] There is no unacceptable scroll jump when the body hint is inserted.

## Open questions

- Verify that a code lens anchored at the line after the selection renders above
  the body hint. If not, fall back to the selection start line and document the
  layout.
- Define the last-line fallback precisely and test it with and without a
  trailing newline.
- Which source languages should the LSP attach to by default?
- How should the extension detect and communicate that inlay hints or code lens
  are off?
- How much of the step body should appear in the hint? First paragraph, hidden
  `hint` field, or a fixed line budget?
- Is the thread-link routing patch in scope, or is a static export acceptable?
- How to handle multiple open tours and multiple workspaces.

## Implementation plan

1. Spike the overlay:
   - Fake LSP with one body hint and one control code lens.
   - Verify code lens/inlay ordering below the selection.
   - Verify last-line and above-placement layouts.
   - Verify two inlay hints still share one style.
   - Test with `inlay_hints.enabled` and `code_lens` on and off.
2. Tour parser:
   - Parse title, thread link, headings, step links, ranges, body.
   - Test relative paths, columns, missing links, malformed files.
3. Tour state and selection:
   - Start/stop/next/previous/goto.
   - `window/showDocument` with character ranges.
4. Overlay:
   - Build body hint text, left rule, padding.
   - Build control code lens at the line after the selection.
   - Apply below/above placement from settings.
   - Refresh hints and lenses on step change.
5. Start controls:
   - Start/stop code lens at line 1.
   - Auto-start and prompt fallbacks.
   - Settings plumbing and schema.
6. Thread link:
   - Store and show the URI.
   - Implement routing patch or document the static fallback.
7. Manual test matrix and polish.

## Test matrix

- Placement above and below.
- Control lens between selected code and body hint for below placement.
- Last-line fallback.
- Two inlay hints vs one hint plus code lens, confirming the style limit.
- Inlay hints on and off.
- Code lens on and off.
- `auto_start` always, prompt, never.
- Tour file in raw editor. Markdown preview only after the Tour language is
  given the Markdown grammar or the server is attached to Markdown instead.
- Long body hint, narrow window, soft wrap.
- Scrolling while active and after refresh.
- Multiple tours and multiple source files.
- Relative and absolute paths, character columns.
- Thread link in agent output and preview.
- Last line with and without a trailing newline.

## Key references

- Multi-line inlays:
  - `crates/editor/src/inlays.rs:54`
  - `crates/editor/src/display_map/inlay_map.rs:1947`, `:1979`
  - `crates/editor/src/editor_tests.rs:14098`
- Stock hint style and background:
  - `crates/editor/src/display_map/inlay_map.rs:338`
  - `crates/editor/src/editor.rs:588`, `:604`
- Character-level `showDocument`:
  - `crates/project/src/lsp_store.rs:1309`
  - `crates/editor/src/items.rs:2541`
  - `crates/lsp/src/lsp.rs:1153`
- Code lens:
  - `crates/editor/src/code_lens.rs:610`, `:661`
  - `crates/project/src/lsp_store.rs:1204`, `:6695`
- Display map layering:
  - `crates/editor/src/display_map.rs:622`
- Settings categories:
  - `crates/extension_host/src/wasm_host/wit/since_v0_8_0.rs:972`
- Thread URI and opening:
  - `crates/acp_thread/src/mention.rs:36`, `:525`
  - `crates/agent_ui/src/conversation_view/thread_view.rs:14176`
- Markdown preview links:
  - `crates/markdown_preview/src/markdown_preview_view.rs:1300`, `:1398`
- HTML comments skipped:
  - `crates/markdown/src/markdown.rs:3346`
- Defaults:
  - `assets/settings/default.json:850`, `:454`
