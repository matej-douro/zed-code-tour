# Layout spike

Throwaway fake LSP for issue #1. Nothing here is imported by the extension or
the real tour language server.

- `layout.py` builds the code lens and inlay hint payloads, plus a display-row
  model used only by the tests and the report.
- `fake_tour_lsp.py` is a stdio LSP server that returns those payloads.
- `test_layout.py` covers below, above, last-line, trailing-newline, two-hint,
  and disabled-layer cases.
- `sample_source.py` is a file for manual checks in a real editor.

The server omits `kind`, `paddingRight`, and `tooltip`. It can send the
standard `paddingLeft` flag. That flag is not a style, but it also makes Zed
choose a right-biased anchor when the hint has no kind. The report explains why
that matters for code lens ordering above the body.

## Run the tests

```sh
python3 -B -m unittest discover -s spike -v
```

## Inspect payloads and modeled rows

```sh
python3 spike/fake_tour_lsp.py --dump --case below_middle
python3 spike/fake_tour_lsp.py --dump --case below_middle_no_padding
python3 spike/fake_tour_lsp.py --dump --case above_middle
python3 spike/fake_tour_lsp.py --dump --case above_middle_no_padding
python3 spike/fake_tour_lsp.py --dump --case below_last_no_trailing_newline
```

## Manual check in a real editor

The sandbox used for this spike has no Zed GUI, so the ordering in
`docs/spike-layout.md` is source-derived, not visually verified. To check it
by hand:

1. Start the fake server against a source file and an explicit range. Positions
   are 0-based LSP lines and characters.

   ```sh
   python3 /abs/path/to/spike/fake_tour_lsp.py \
     --source /abs/path/to/spike/sample_source.py \
     --start-line 3 --start-character 4 \
     --end-line 3 --end-character 18 \
     --placement below --padding-left
   ```

2. Register it as a custom LSP for Python in a scratch Zed settings file.
   Point `binary.path` at `python3` and pass the script and its arguments, or
   make the script executable and use it directly. Enable
   `inlay_hints.enabled` and set `code_lens` to `on`.
3. Open `sample_source.py` and confirm the order from the report. Repeat with
   `--placement above`, with the same selection but no `--padding-left`, with a
   last-line selection, with and without a trailing newline, with
   `--force-fallback`, and with `--two-hints`.
4. Turn `inlay_hints.enabled` off and `code_lens` off separately and confirm the
   matching layers disappear.
