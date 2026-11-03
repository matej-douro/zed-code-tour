---
name: zed-code-tour
description: Build and start a guided code tour in Zed from a .tour.md file. Use when the user asks for a code walkthrough, onboarding tour, guided reading, or a step-by-step explanation tied to exact source ranges.
---

# Zed Code Tour

Create a `*.tour.md` file that the Zed Code Tour extension turns into a guided
walkthrough. The extension selects exact source ranges, renders a short
annotation below or above the selection, and adds Previous / Next / Overview /
Stop controls. It never edits source buffers.

Use this skill when the user asks for a code tour, a walkthrough, an onboarding
path through a codebase, or a guided explanation of how a feature works.

## Before you start

- The user needs the `zed-code-tour` extension installed and `tour-lsp` on their
  shell `PATH`. If `tour-lsp was not found in PATH` appears, point them at the
  [install guide](https://github.com/matej-douro/zed-code-tour/blob/main/docs/install.md);
  do not try to install system binaries without permission.
- The overlays are gated by Zed settings and are off by default:
  - `"code_lens": "on"` shows the start lens and the step controls.
  - `"inlay_hints": { "enabled": true }` shows the annotation.
- The tour server attaches to these source languages only: C, C++, Go,
  JavaScript, Python, Rust, TSX, TypeScript. A tour targeting another language
  still opens, but the source overlays do not render.
- Put the tour file inside the user's workspace.

## Workflow

1. Read the relevant code first. Do not write a tour from filenames alone.
2. Choose 2-8 steps that build a coherent story: entry point, setup, core logic,
   error handling, tests. One or two steps is fine for a narrow question.
3. For each step, find the exact line and character range to highlight.
4. Write the tour file, for example `docs/auth.tour.md`.
5. Re-read every target link and check the file exists and the range fits.
6. Give the user the path and, if the client supports ACP resource links, emit a
   `ResourceLink` named `Start tour`.

## Tour file format

```markdown
# Auth flow

[Back to thread](zed:///agent/thread/<session-id>?name=Auth%20flow)

## 1. Entry point
[src/main.rs:12:5-12:24](src/main.rs#L12:5-L12:24)

The handler starts here, then calls `route` on the next line.

## 2. Router setup
[src/router.rs:40:8-40:58](src/router.rs#L40:8-L40:58)

Paths are registered in this table.
```

Rules:

- `# Title` names the tour.
- `## <number>. <title>` starts a step. Steps run in document order.
- The first link in a step is the target. The link text is human-readable. The
  destination carries the path and range.
- The fragment is `#L<line>:<column>-L<line>:<column>`. Lines and columns are
  1-based. The start is inclusive and the end is exclusive, matching LSP
  ranges. Ranges may span lines.
- Relative paths resolve against the directory that contains the tour file. A
  tour in `docs/` refers to `../src/main.rs`.
- Absolute paths work but make the tour fragile. Prefer relative paths.
- The first paragraph after the target link is the annotation hint. Keep it to
  one to three short sentences and at most six wrapped lines. Put the full
  explanation in the rest of the step body.
- A hidden comment overrides the hint without changing the visible Markdown:

  ```markdown
  <!-- tour-step: {"hint": "Short plain-text annotation"} -->
  ```

- The optional `zed:///agent/thread/<session-id>?name=<title>` link records the
  creating thread. Add it only if you have the current thread URI. Leave it out
  otherwise; never invent a session id.
- Do not put placement or auto-start settings in the tour file. They are user
  settings under `lsp.tour.settings`.

## Computing ranges

Columns count **Unicode scalar values**, not bytes and not UTF-16 code units.
A tab is one column. An emoji is one column even though it is two UTF-16 code
units.

- Start column: the 1-based position of the first character to select.
- End column: the 1-based position just after the last character to select.
- For a whole line, use column 1 through the line length plus one.
- The range must fit the current file. If the target file is not open, the
  server reads it from disk.

If you are unsure, count characters from the start of the line with a tool that
prints the line exactly. Do not use byte offsets from `grep` output.

## Starting the tour

When the client supports ACP resource links, emit a link to the tour file:

```json
{
  "type": "resource_link",
  "name": "Start tour",
  "uri": "file:///absolute/path/to/docs/auth.tour.md",
  "mimeType": "text/markdown"
}
```

Point the URI at the tour file, not at a source file. The user's `auto_start`
setting decides what happens when the file opens:

- `always` starts at step 0 immediately.
- `prompt` asks `Start tour` or `Not now`.
- `never` waits for the `▶ Start tour (N steps)` lens.

With code lens on, the start lens at the top of the tour file is the manual
fallback in every mode. Step controls live in the source file and also require
code lens.

## Verification checklist

- [ ] The file name ends in `.tour.md`.
- [ ] The title and at least one step exist.
- [ ] Every target path resolves relative to the tour file and exists.
- [ ] Every range fits its line and uses scalar columns, with an exclusive end.
- [ ] The hint paragraph is short; the full explanation is in the step body.
- [ ] No placement or auto-start fields are in the tour file.
- [ ] The resource link points at the tour file with an absolute `file://` URI.
- [ ] No source file was modified.

## Troubleshooting

- **No start lens or step controls.** `code_lens` is off. The tour can still
  start through `auto_start: always` or the prompt; with `auto_start: never`
  there is no visible start control in v1.
- **No annotation.** `inlay_hints.enabled` is off. Navigation still works.
- **`tour-lsp was not found in PATH`.** Install the language server and restart
  Zed so it sees the new `PATH`.
- **The tour file looks like plain text.** The Tour language has no grammar yet.
  That is expected; the raw Markdown stays readable and the server still parses
  it.
- **`Back to thread` does not open the agent panel.** This Zed build does not
  route `zed:///agent/thread` links from code lens. The lens focuses the link
  line and shows the URI; open the thread from the Agent panel instead.

## References

- [README](https://github.com/matej-douro/zed-code-tour/blob/main/README.md) for the tour format and settings.
- [HANDOFF](https://github.com/matej-douro/zed-code-tour/blob/main/HANDOFF.md) for the design and known gaps.
- [Install](https://github.com/matej-douro/zed-code-tour/blob/main/docs/install.md) for extension and `tour-lsp` setup.
- [Thread link](https://github.com/matej-douro/zed-code-tour/blob/main/docs/thread-link.md) for the thread-link fallback.
