# Language server initialization and state

This document describes the protocol surface, active-tour state, and start/stop
flow. Source-file overlays, step controls, navigation, and agent integration are
implemented by later issues.

## Capabilities and commands

The server advertises:

- `textDocumentSync: full`, so it receives the complete `didChange` text.
- `inlayHintProvider: true`.
- `codeLensProvider: { resolveProvider: false }`.
- `executeCommandProvider` with `tour.start`, `tour.stop`, `tour.next`,
  `tour.previous`, `tour.goto`, `tour.overview`, and `tour.backToThread`.

`tour.start` and `tour.stop` are handled. `tour.start` starts an inactive tour
at step 0 and restarts a running tour at step 0. `tour.stop` marks the tour
inactive. The remaining commands are advertised but are no-ops until issues #7
and #8 implement step controls and navigation.

## Start/stop code lens

`textDocument/codeLens` returns lenses only for the active tour document. Other
documents, including source files, return an empty array.

The lens is anchored at the first line, character 0:

- Inactive: `▶ Start tour (N steps)`, command `tour.start`.
- Active: `■ Stop tour` (command `tour.stop`) followed by
  `⟳ Restart | <current>/<total>` (command `tour.start`).

LSP code lenses carry one command each, so the active row is two lenses. Zed
separates lenses on the same line with ` | `, which renders the documented
`■ Stop tour | ⟳ Restart | 2/N` row. The step counter is part of the restart
lens so it does not need a non-interactive lens. Both starting and restarting
select step 0 and send both refresh notifications.

## Start flow

On `didOpen` of a valid `.tour.md` document:

- `auto_start: "always"` starts the tour at step 0 and sends
  `workspace/inlayHint/refresh` and `workspace/codeLens/refresh`.
- `auto_start: "prompt"` sends `window/showMessageRequest` with the tour title
  and `Start tour` / `Not now` actions. The tour stays inactive until the client
  responds with `Start tour`. Confirmation starts at step 0 and refreshes the
  overlays. `Not now`, a null response, or a stale response leaves the tour
  inactive and does not change the tour file.
- `auto_start: "never"` leaves the tour inactive until the user clicks the start
  lens or runs `tour.start`.

Prompt responses are matched by request id. Opening another tour clears any
pending prompt, so a response from an older prompt cannot start the new tour.

The start flow does not change the user's selection. Character-range navigation
with `window/showDocument` belongs to issue #8.

When code lens is disabled in Zed, the client does not request lenses and the
start/stop lens is not shown. Auto-start and prompt still start the tour because
they are server-side; the missing step controls and manual start lens are
expected in that mode. Enabling code lens (`code_lens: "on"`) restores them.

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
schema documents the same options where they are passed to the server. The
`zed_extension_api` crate `0.7.0` does not expose either schema hook, so the
extension API is pinned to the Zed revision used by `HANDOFF.md` until `0.8.0`
is published.

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
- If parsing a newly opened tour fails, the active tour is cleared so an invalid
  new tour cannot revive stale state.
- If parsing a `didChange` for the active tour fails, the last valid active tour
  is kept and the failure is logged to stderr. The next valid change reparses and
  updates the tour, so transient parse errors while typing do not strand it.

Multi-workspace policy: there is one state per language server instance. The
implementation assumes a single workspace per server. If one server is reused
across workspaces, the last opened tour across those workspaces wins. Per
workspace state is left to a later issue.

## Refreshes

After a tour is opened, updated, or cleared, the server sends
`workspace/inlayHint/refresh` and `workspace/codeLens/refresh`. Starting,
stopping, restarting, and confirming a start prompt also refresh both overlays so
the lens label changes. Non-tour documents, changes to inactive tours, and stop
commands on an already inactive tour do not send refreshes.
