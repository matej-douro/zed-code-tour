# Language server initialization and state

This document describes the protocol surface and active-tour state added for
issue #4. Hints, lenses, navigation, and agent integration are implemented by
later issues.

## Capabilities and commands

The server advertises:

- `textDocumentSync: full`, so it receives the complete `didChange` text.
- `inlayHintProvider: true`.
- `codeLensProvider: { resolveProvider: false }`.
- `executeCommandProvider` with `tour.start`, `tour.stop`, `tour.next`,
  `tour.previous`, `tour.goto`, `tour.overview`, and `tour.backToThread`.

Advertised commands are not handled yet. Command behavior belongs to issues #5,
#7, and #8.

## Settings

Settings use the language server settings category:

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

The extension reads `LspSettings::for_worktree("tour", worktree)` and passes
`settings.settings` as the LSP initialization options. The server validates each
field independently:

| Field | Values | Default |
| --- | --- | --- |
| `placement` | `below`, `above` | `below` |
| `auto_start` | `always`, `prompt`, `never` | `prompt` |

An absent field, a non-object options value, or an invalid value falls back to
the default for that field. Unknown fields are ignored. Values are
case-sensitive.

The extension returns the settings schema from both the initialization options
schema hook and the workspace configuration schema hook. `lsp.tour.settings`
uses the workspace configuration schema in Zed, while the initialization options
schema documents the same options where they are passed to the server. Zed
`0.7.0` does not expose either schema hook, so the extension API is pinned to the
Zed revision used by `HANDOFF.md` until `0.8.0` is published.

A first-class top-level `tour` settings section is out of scope. It would need a
core settings or context-server-settings change.

## Document filter

Only Markdown documents whose path ends in `.tour.md` are processed. `didOpen`
and `didChange` for other Markdown buffers, and for non-Markdown documents, are
ignored without changing state or sending refreshes.

## Active tour state

The server holds at most one active tour with:

- tour document URI,
- parsed tour, including ordered steps,
- zero-based current step index,
- resolved source URI and source range for the current step,
- running flag.

Opening a valid `.tour.md` document replaces the previous tour and selects step
0. The tour is not running until the start flow in issue #5 starts it. A tour
with no steps is still parsed and tracked, but it has no resolved source URI or
range until a step is added.

Multiple-open policy:

- Last opened tour wins. A second `didOpen` always replaces the active tour.
- A `didChange` for a tour that is not active is ignored.
- A `didChange` for the active tour reparses it, preserves the running flag, and
  keeps the current step index when possible, clamping it to the new step count.
- If parsing the newly opened or changed active tour fails, the active tour is
  cleared so stale state cannot survive. The failure is logged to stderr.

Multi-workspace policy: there is one state per language server instance. The
implementation assumes a single workspace per server. If one server is reused
across workspaces, the last opened tour across those workspaces wins. Per
workspace state is left to a later issue.

## Refreshes

After a tour is opened, updated, or cleared, the server sends
`workspace/inlayHint/refresh` and `workspace/codeLens/refresh`. Non-tour
documents and changes to inactive tours do not send refreshes.
