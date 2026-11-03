# Character-range navigation

This document describes the `window/showDocument` navigation added for issue
#8. Hints and lenses that react to a step change are still implemented by
issues #5, #6, and #7.

## Position conversion

Tour links use 1-based lines and 1-based character columns. LSP uses zero-based
lines and zero-based UTF-16 code unit columns. `crates/tour-lsp/src/navigation.rs`
converts both directions:

- `tour_position_to_lsp` and `tour_range_to_lsp` convert tour links for
  `window/showDocument`.
- `lsp_position_to_tour` and `lsp_range_to_tour` convert back for diagnostics
  and tests.
- CRLF input is treated like LF input. A line never includes its trailing `\r`.
- Columns count Unicode scalar values. An astral-plane character such as an
  emoji uses one tour column but two UTF-16 code units.
- A column at end of line maps to the line's UTF-16 length, so selecting to the
  end of a line works without a trailing separator.
- A UTF-16 column that splits a surrogate pair, a line outside the file, and a
  column beyond the line end are rejected with a clear error instead of being
  rounded.

## Source text

Source columns depend on the source content, not just the tour file. The server
keeps the text of open documents from `didOpen` and full `didChange`
notifications. If a target file is not open, it reads the file from disk. This
means an open source buffer with unsaved changes is measured using the content
the user is looking at.

If the target file cannot be read, or the tour range does not fit the current
text, the command fails and the active step, selection, and running state are
left unchanged. The failure is reported both as a `window/showMessage` error and
as the `workspace/executeCommand` error response. Client-side failures are also
reported: the server tracks the `window/showDocument` request id and sends an
error message if the client responds with `success: false`.

## Commands

The navigation commands are:

| Command | Argument | Behavior |
| --- | --- | --- |
| `tour.start` | none | Mark the tour running at step 0 and show it |
| `tour.resume` | none | Show the current step and mark the tour running, without resetting to step 0 |
| `tour.next` | none | Move to the next step, or no-op at the last step |
| `tour.previous` | none | Move to the previous step, or no-op at the first step |
| `tour.goto` | zero-based step index | Select that step |

`tour.goto` uses a zero-based index to match active tour state and the code lens
arguments planned for issue #7. An absent or out-of-range index is reported as
an error.

After a successful transition the server sends, in order:

1. `window/showDocument` with the target URI, exact `Range`, and
   `takeFocus: true`.
2. `workspace/codeLens/refresh`.
3. `workspace/inlayHint/refresh`, after a short delay.

The hint refresh is deferred because `window/showDocument` only schedules the
editor autoscroll. A refresh sent while the scroll is pending makes Zed
invalidate hints against the pre-scroll visible rows and remove the body hint in
files longer than 50 rows. See issue #42 and
[body-hint.md](body-hint.md#refresh-and-removal).

Stopping the tour, the start lens, and auto-start/prompt are implemented by
issue #5. Starting or restarting a tour selects step 0 through the same
navigation path. Resuming selects the current step through that path instead.
