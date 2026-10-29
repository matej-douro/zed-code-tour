# Step control code lens row

While a tour is running, the language server returns one single-line control
row for the active step's source file. It uses the stock code lens style. There
is no per-lens styling, no inlay-part fallback, and no source-buffer edit.

The row is implemented by issue #7. The tour-file start/stop lens is described
in [lsp-state.md](lsp-state.md), and the body hint in
[body-hint.md](body-hint.md).

## Row content

LSP code lenses carry one command each, so the row is a sequence of lenses on
the same line. Zed joins same-line lenses with ` | `. For a tour with more than
one step, the server returns four lenses:

| Title | Command | Behavior |
| --- | --- | --- |
| `◀ Prev` | `tour.previous` | Select the previous step, or no-op at the first step |
| `<current>/<total> \| Next ▶` | `tour.next` | Select the next step, or no-op at the last step |
| `Overview` | `tour.overview` | Focus the tour file at the top |
| `Stop` | `tour.stop` | Stop the tour and remove both overlays |

Zed renders them as:

```text
◀ Prev | 2/N | Next ▶ | Overview | Stop
```

If the tour has a creating-thread link, the row appends `Back to thread` before
Stop:

```text
◀ Prev | 2/N | Next ▶ | Overview | Back to thread | Stop
```

The command focuses the recorded link line and explains the current routing
gap. Tours without a thread link keep the exact row above, so the base layout
from issue #7 is unchanged. See [thread-link.md](thread-link.md).

The step counter is part of the Next lens so the row needs no non-interactive
lens. In a multi-step tour, Previous and Next stay visible at the first and last
step; their commands are no-ops at those boundaries, so the row does not change
shape while moving through the tour. Control commands take no arguments.

### Single-step tours

A one-step tour omits Previous and Next because both would be no-ops. The row
becomes:

```text
1/1 | Overview | Stop
```

The counter is attached to Overview instead. This keeps the step count visible
and leaves only useful controls. A single-step tour with a thread link renders
`1/1 | Overview | Back to thread | Stop`.

## Anchor

The row is anchored at the selection start of a zero-width range on one source
line:

| Placement | Anchor line |
| --- | --- |
| `below` | The line after the selection end line, so the row renders between the selected code and the body hint |
| `above` | The selection start line |
| `below`, selection ends on the last line | The selection start line (fallback because there is no line below) |

The last-line fallback matches the body hint's fallback in
[body-hint.md](body-hint.md#placement). A source file that ends with a newline
has an empty final LSP line, so the normal below placement uses that empty line
instead of the fallback.

The last-line decision uses the open source document text, the same text that
positions the body hint. The row is only returned for the active source file
while the tour is running.

## Overview

For v1, `tour.overview` focuses the active tour document at line 0, character 0
with `window/showDocument` and `takeFocus: true`. It does not change the current
step or the running flag, and it does not refresh overlays because nothing in
the tour state changed. The active step stays selected, so Previous, Next, or
`tour.goto` returns focus to its source file. Running `tour.start` again
restarts the tour at step 0.

Opening the tour file instead of a generated index keeps v1 simple and needs no
custom panel or webview. A richer overview is out of scope.

## Refresh and removal

The server sends `workspace/codeLens/refresh` after step changes, start,
restart, stop, settings changes, and tour updates. The row always reflects the
current step index and step count when the client requests lenses again.
Stopping the tour makes the next `textDocument/codeLens` request for the source
return an empty array, so Zed removes the row. The tour-file start lens remains.

Opening, changing, or closing the active source document also refreshes both
hints and lenses, so the row appears, moves, or disappears with the document.

## Code lens off

Step controls require code lens in v1. There is no inlay-part fallback. With
code lens off, the tour can still start through `auto_start: "always"` or the
start prompt, and the body hint still renders if inlay hints are enabled, but no
step controls are shown. Enabling code lens (`code_lens: "on"`) restores the
tour-file lens and the step control row.

## Tests

`crates/tour-lsp/src/controls.rs` covers the anchor line and command wiring for
below, above, and last-line placement, the trailing-newline case, the
single-step row, and the optional thread control. `crates/tour-lsp/src/server.rs`
covers the running-only source row, switching to the new active source on a step
change, removal on stop, Overview's show-document request, thread-link fallback
behavior, and code lens refreshes.
