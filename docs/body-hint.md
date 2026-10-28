# Body inlay hint

The language server returns exactly one multi-line inlay hint for the active
step. The hint contains body text only; controls belong to the code lens row in
[step-controls.md](step-controls.md). It uses the stock `hint` style with no
per-item style fields.

## Request behavior

`textDocument/inlayHint` returns a hint only when all of these are true:

- a tour is active and running,
- the requested document is the active step's source file,
- the step has body text,
- the requested position is inside the range the client asked about,
- the source document is open, so its current text is available for the
  last-line fallback.

Open document text is used instead of the file on disk, so unsaved edits affect
the last-line fallback. Closing the active source document removes its cached
text and suppresses the hint until the document is opened again. A stopped tour
returns no hint.

The response is `null` when no hint applies.

## Text format

The body is the step's explicit `tour-step` hint metadata when present, or the
first paragraph of the visible step body. See
[Tour file format](../HANDOFF.md#tour-file-format).

Every line has the same shape:

```text
 ▎ <body text padded to the widest line>
```

- The leading space is required. Together with `paddingLeft: true` it makes the
  hint right-biased, so a code lens anchored at the same line stays above the
  body. See [the layout spike](spike-layout.md).
- `▎` is the left rule.
- Shorter lines are padded with spaces to the widest line for a consistent
  background rectangle.
- The text ends with `\n`, so the next source line starts on its own row.
- The hint has no `kind`, no `paddingRight`, no tooltip, and no text edits.

The body budget is six lines and 80 characters per line. Extra lines are
dropped and the last retained line gets a trailing `…`. Longer lines are
truncated to 80 characters with a trailing `…`. The limits are constants in
`crates/tour-lsp/src/hint.rs`.

## Placement

With `placement: "below"` (the default), the hint is anchored at the start of
the line after the selection end line.

If there is no line after the selection, the hint is anchored at the end of the
selection line and the label starts with `\n`. The end position uses UTF-16 code
units, matching LSP `Position.character`. A source file that ends with a newline
has an empty final LSP line, so the normal below placement is used instead of
the fallback. Zed applies the `paddingLeft` space before the leading newline in
this case; the extra space is cosmetic and does not change the ordering.

With `placement: "above"`, the hint is anchored at the start of the selection
start line.

The selection itself is never changed.

## Refresh and removal

The server sends `workspace/inlayHint/refresh`:

- after a tour is opened, replaced, or updated,
- when an active source document opens, changes, or closes,
- when tour settings change through `workspace/didChangeConfiguration`,
- during start, step change, and stop commands.

Stopping means setting the tour's running flag to false; the next hint request
then returns `null` and Zed removes the old hint. The user's selection is left
alone.

## Tests

`crates/tour-lsp/src/hint.rs` covers formatting, the body budget, both
placements, the last-line fallback with and without a trailing newline, UTF-16
line lengths, and the bias fields. `crates/tour-lsp/src/server.rs` covers one
hint per active source, requested-range filtering, step changes, placement
changes, empty configuration payloads, stop, and source close.
