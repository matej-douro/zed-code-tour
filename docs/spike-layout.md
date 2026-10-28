# Spike: code lens and inlay hint ordering and placement

Status: complete as a source-derived spike. The environment has no Zed binary or
GUI, so the visual ordering below is derived from Zed source and tests, not from
a screenshot. The fake LSP under `spike/` verifies the payload geometry and the
tests verify the row model. The Zed repository was only read, never modified.

Zed reference: `37faef81f74fc1bb08be9939c4cffc8ede6ced2f` in
`/workspace/zed-industries/zed`.

## Verdict

The design assumption is only partly correct. The block map is layered above the
inlay map, but a code lens anchored at the same line as a body inlay hint renders
above the hint only when the hint is right-biased.

- With the design as written, a body hint at line start with no `kind` and no
  padding gets a left bias. A block anchored at that line start maps after the
  left-biased inlay, so the lens renders below the body. Below order becomes
  selected code, body, lens, next code line.
- If the body hint is right-biased, the intended below order holds: selected
  code, control lens, body hint, next code line.
- The same split applies to above placement. With a right-biased hint the order
  is lens, body, selected code. With a left-biased hint the order is body, lens,
  selected code, so the lens is between the body and the selection.
- The last-line fallback at the end of a line is naturally right-biased and works
  without extra padding. The lens stays above the selected line and the body
  follows it.

Recommended fix: send `paddingLeft: true` on the body hint and start every body
line with one padding space before the `▎` rule. This gives the hint a right
bias and keeps the lines aligned. It is a standard LSP padding flag, not a
per-hint style. The source-derived order then matches the design.

Fallback if the hint must stay left-biased, or if a future Zed changes this
behavior: anchor the control lens at the selection start line. See
[Fallback and required handoff changes](#fallback-and-required-handoff-changes).

Both orderings are source-derived, not visually verified.

## How this was checked

The sandbox has no Zed binary and no GUI, so the spike used two kinds of
evidence:

1. Zed source and existing tests:
   - display map layering,
   - code lens block placement,
   - inlay position and bias,
   - block mapping around left-biased and right-biased inlays,
   - stock hint style,
   - settings gates for code lens and inlay hints.
2. A throwaway fake LSP under `spike/` that returns the exact code lens and
   inlay hint payloads, plus tests for the expected anchors and modeled display
   rows. The fake LSP is not imported by the extension or the real tour server.

Run the spike tests:

```sh
python3 -B -m unittest discover -s spike -v
```

Result: 15 tests pass.

Inspect one case as JSON, including payloads and modeled rows:

```sh
python3 spike/fake_tour_lsp.py --dump --case below_middle
python3 spike/fake_tour_lsp.py --dump --case below_middle_no_padding
```

## Source evidence

### Display map layering

`crates/editor/src/display_map.rs:7-15` describes the layer order as inlay,
fold, tab, wrap, then block. Each layer builds on the preceding layer. The code
lens is a block, and the body is an inlay, so the inlay text is part of the
text that the block map sees.

### Code lens placement and inlay bias

A code lens becomes a custom block with `BlockPlacement::Above(anchor)`:

- `crates/editor/src/code_lens.rs:384`.
- The anchor is the code lens range start, converted with `anchor_before`:
  `crates/project/src/lsp_command.rs:4968-4971`.
- `BlockPlacement::Above` forces the position to column 0 and maps it to a wrap
  row with `Bias::Left`:
  `crates/editor/src/display_map/block_map.rs:237-241`.

That wrap mapping goes through `WrapSnapshot::make_wrap_point`
(`crates/editor/src/display_map/wrap_map.rs:978-979`), which calls
`TabSnapshot::point_to_tab_point`
(`crates/editor/src/display_map/tab_map.rs:422-425`), which calls
`InlaySnapshot::to_inlay_point`
(`crates/editor/src/display_map/inlay_map.rs:978-979`). `to_inlay_point` always
maps with `Bias::Left`.

`InlayPointCursor::map`
(`crates/editor/src/display_map/inlay_map.rs:1362-1396`) then decides where the
buffer point lands relative to inlay text:

- At a left-biased inlay, it advances past the inlay before returning
  (`inlay_map.rs:1387-1388`). The block anchored at that buffer point therefore
  lands after the inlay text.
- At a right-biased inlay, with `Bias::Left`, it stops before the inlay
  (`inlay_map.rs:1387-1391`). The block lands before the inlay text.

This is the key correction to the design assumption. The block map is above the
inlay map, but the inlay bias determines whether the block row is above or below
the inlay at the same buffer position.

### Which bias the body hint gets

`hint_position_and_bias` in `crates/project/src/lsp_command.rs:3898-3932` picks
the inlay anchor bias:

- `kind: Type` gives right bias, but it also subjects the hint to
  `show_type_hints`.
- `kind: Parameter` gives left bias.
- With no kind, `paddingLeft: true` and `paddingRight: false` gives right bias:
  `lsp_command.rs:3919`.
- Otherwise the surrounding-word heuristic applies. At line start it gives left
  bias. At line end it gives right bias.

The fake LSP omits `kind`, matching the design. Its default line-start hint is
therefore left-biased. Adding `paddingLeft: true` makes it right-biased.

`Inlay::hint` in `crates/editor/src/inlays.rs:60-69` only prepends a space when
`paddingLeft` is set and the text does not already start with a space. Starting
every body line with one padding space keeps all lines aligned and avoids an
extra first-line-only space.

### Existing test evidence

`test_inlays_with_newlines_after_blocks`
(`crates/editor/src/display_map.rs:3579-3640`) inserts a block with `Above`
placement at line 0, then inserts a newline inlay at offset 0 with
`anchor_after`, which is right-biased. The text becomes `"\n\n\na"`: the block
rows come first, then the inlay newline, then the buffer text. This confirms the
right-biased case. It does not cover a left-biased inlay at the same anchor,
which is why the bias finding above matters.

### Stock hint style

All inlay hints use one style. `InlayId::Hint(_)` maps to
`self.highlight_styles.inlay_hint` in
`crates/editor/src/display_map/inlay_map.rs:338-347`. `make_inlay_hints_style`
(`crates/editor/src/editor.rs:588-611`) uses the syntax theme's `hint` style,
adds the theme hint color if needed, and only adds a background when
`inlay_hints.show_background` is enabled. There is no per-hint style field in
the LSP inlay hint payload.

### Settings gates

Both features are off by default:

- `code_lens: "off"` in `assets/settings/default.json:448-454`.
- `inlay_hints.enabled: false` in `assets/settings/default.json:847-850`.

When code lens is off, `refresh_code_lenses` returns early
(`crates/editor/src/code_lens.rs:214-222`) and `clear_code_lenses` removes the
lens blocks (`crates/editor/src/code_lens.rs:546-559`).

When inlay hints are off, `should_show` is false
(`crates/editor/src/inlays/inlay_hints.rs:344-346`) and refreshes return early
when inlay hint state is absent (`crates/editor/src/inlays/inlay_hints.rs:798-805`).

## Results

Positions are 0-based LSP positions. `lineN` is a source line for the case, not
the file line in this report. The labels below are shown with one space between
the rule and the text, and with trailing padding spaces kept where they matter
for the background rectangle.

### Below placement, right-biased hint

Case: `below_middle`. Selection is line 2, line 3 exists. The hint omits `kind`
and sets `paddingLeft: true`; each line starts with one padding space.

```text
codeLens range:     3:0 to 3:0
inlayHint position: 3:0
inlayHint padding:  paddingLeft: true
inlayHint label:    " ▎ Tour step body line one.\n ▎ Second body line.       \n"
```

Modeled row order:

```text
0: line0
1: line1
2: line2          <- selected code
lens: ◀ Prev | 2/N | Next ▶ | Overview | Stop
hint:  ▎ Tour step body line one.
hint:  ▎ Second body line.
3: line3          <- next code line
4: line4
```

Result: selected code, lens, body, next code line. This matches the design when
the hint is right-biased.

### Below placement, default left-biased hint

Case: `below_middle_no_padding`. Same anchors, but no `paddingLeft` and no
leading padding space. At line start the hint gets left bias.

```text
codeLens range:     3:0 to 3:0
inlayHint position: 3:0
inlayHint padding:  none
inlayHint label:    "▎ Tour step body line one.\n▎ Second body line.       \n"
```

Modeled row order:

```text
0: line0
1: line1
2: line2          <- selected code
hint: ▎ Tour step body line one.
hint: ▎ Second body line.
lens: ◀ Prev | 2/N | Next ▶ | Overview | Stop
3: line3          <- next code line
4: line4
```

Result: selected code, body, lens, next code line. The lens is below the body.
This contradicts the intended design. The design as written does not specify
`paddingLeft`, so this is the behavior it would get today.

### Above placement, right-biased hint

Case: `above_middle`. Selection is line 2. Both the lens and the hint are at
line 2, character 0. The hint sets `paddingLeft: true`.

```text
codeLens range:     2:0 to 2:0
inlayHint position: 2:0
inlayHint padding:  paddingLeft: true
inlayHint label:    " ▎ Tour step body line one.\n ▎ Second body line.       \n"
```

Modeled row order:

```text
0: line0
1: line1
lens: ◀ Prev | 2/N | Next ▶ | Overview | Stop
hint:  ▎ Tour step body line one.
hint:  ▎ Second body line.
2: line2          <- selected code
3: line3
4: line4
```

Result: the lens is above the body and above the selection. It is not between
the selection and the body. The body is directly above the selected code.

### Above placement, default left-biased hint

Case: `above_middle_no_padding`. Same anchors, but no padding.

Modeled row order:

```text
0: line0
1: line1
hint: ▎ Tour step body line one.
hint: ▎ Second body line.
lens: ◀ Prev | 2/N | Next ▶ | Overview | Stop
2: line2          <- selected code
3: line3
4: line4
```

Result: the lens is between the body and the selection. This violates the above
placement requirement that the lens is not between the selection and the hint.

### Last line, below placement, no trailing newline

Case: `below_last_no_trailing_newline`. The selection ends on line 2, and line 2
is the last line. LSP line count is 3, so there is no line after the selection.
The fallback anchors the hint at the end of the selection line with a leading
`\n`, and the lens at the selection start line.

```text
codeLens range:     2:0 to 2:0
inlayHint position: 2:5   (end of "line2")
inlayHint padding:  none
inlayHint label:    "\n▎ Tour step body line one.\n▎ Second body line.       \n"
```

Modeled row order:

```text
0: line0
1: line1
lens: ◀ Prev | 2/N | Next ▶ | Overview | Stop
2: line2          <- selected code
hint: ▎ Tour step body line one.
hint: ▎ Second body line.
```

Result: the control lens is above the selected code, not between the selected
code and the body. This is the documented last-line fallback. The hint at line
end is right-biased by the surrounding-word heuristic, so this case works
without `paddingLeft`. Setting `paddingLeft: true` also keeps right bias and
does not change the order.

### Last line, below placement, with trailing newline

A trailing newline creates an empty final LSP line, so there are two different
situations. The spike covers both.

Case: `below_last_with_trailing_newline_normal`. Selection is line 2 in
`line0\nline1\nline2\n`. LSP line count is 4, so line 3 is an empty line after
the selection. Normal below placement uses line 3 with a right-biased hint.

```text
codeLens range:     3:0 to 3:0
inlayHint position: 3:0
inlayHint padding:  paddingLeft: true
inlayHint label:    " ▎ Tour step body line one.\n ▎ Second body line.       \n"
```

Modeled row order:

```text
0: line0
1: line1
2: line2          <- selected code
lens: ◀ Prev | 2/N | Next ▶ | Overview | Stop
hint:  ▎ Tour step body line one.
hint:  ▎ Second body line.
3:                <- empty final line
```

Without `paddingLeft`, this line-start hint is left-biased and the body and lens
swap, the same as the `below_middle_no_padding` case.

Case: `below_last_with_trailing_newline_forced_fallback`. This is the same file
and selection, but the spike forces the issue's fallback geometry to show what
it would do with a trailing newline. A real server would normally use the empty
line instead.

```text
codeLens range:     2:0 to 2:0
inlayHint position: 2:5
inlayHint label:    "\n▎ Tour step body line one.\n▎ Second body line.       \n"
```

Modeled row order:

```text
0: line0
1: line1
lens: ◀ Prev | 2/N | Next ▶ | Overview | Stop
2: line2          <- selected code
hint: ▎ Tour step body line one.
hint: ▎ Second body line.
3:                <- empty final line
```

Result: with a trailing newline, the last content line is not the last LSP line.
The normal below path is available. The fallback only triggers when the selected
end line has no following LSP line. Both geometries keep the lens and body from
overlapping.

## Two inlay hints and the stock style

Case: `below_middle_two_hints`. The fake server returns two hints. Both omit
`kind`, set `paddingLeft: true`, and carry only `position`, `label`, and
`paddingLeft`. They send no `paddingRight` or `tooltip`.

Both hints become `InlayId::Hint`, and every hint inlay uses the same
`inlay_hints_style`. There is no per-hint style in the LSP payload or in Zed's
hint rendering path. The two hints therefore share the same stock `hint` style,
including the optional background from `inlay_hints.show_background`.

This confirms the design constraint: do not split body and controls into two
inlay hints for styling. All hints look the same. Controls must use the separate
code lens style. `paddingLeft` is a standard padding flag, not a style, and both
hints use the same one.

## Inlay hints and code lens on and off

The spike models the client-side layer gates. The source references are in
[Settings gates](#settings-gates).

| `inlay_hints.enabled` | `code_lens` | body hint | control row | layout note |
| --- | --- | --- | --- | --- |
| true | `on` | yes | yes | intended layout if the hint is right-biased |
| true | `off` | yes | no | body renders, no step controls |
| false | `on` | no | yes | only the lens row renders above its anchor line |
| false | `off` | no | no | no tour overlay |

With below placement and the lens enabled, the lens still anchors at the line
after the selection even when hints are off. It then appears between the
selected code and the next code line, with no body. With above placement it
appears above the selection. With code lens off, the step controls are missing
until the user enables code lens.

The `menu` code lens mode is out of scope for this spike. The spike models
`on` and `off` only.

## Fallback and required handoff changes

The source shows that the design as written does not hold for a default
no-kind hint at line start. There are two ways to resolve it.

### Recommended: right-bias the body hint

Set `paddingLeft: true` on the body hint and start each body line with one
padding space before `▎`. This is the least invasive fix and preserves the
intended layout. It needs no new blocks, no per-item styling, and no core
changes. The body is indented by one space, which is consistent with the
padding rule.

If that fix is adopted, these `HANDOFF.md` changes are needed. The file is not
edited by this spike.

- `Final design`: add "Set `paddingLeft: true` on the body hint and start each
  line with a padding space before `▎` so the hint is right-biased. A
  left-biased hint at the same anchor renders the code lens below the body."
- `LSP behavior`, inlay hint: add the same `paddingLeft` and leading-space rule,
  and note that it is required for the below and above ordering.
- `Layout and visual rules`: replace "The block map is layered above the inlay
  map, so the lens should render above the body hint" with "The block map is
  above the inlay map, but a left-biased inlay at the same anchor maps after the
  inlay. The lens renders above the body only when the body hint is
  right-biased."
- `Key references`: add `crates/editor/src/display_map/inlay_map.rs:1362-1396`
  for the bias-dependent mapping.

If the fix is adopted, the acceptance criterion "For below placement, the
control lens renders between the selected code and the body hint" can stay as
is.

### Fallback: anchor the lens at the selection start line

If the body hint must stay left-biased, or if a future Zed version changes the
block behavior, use the fallback named in `HANDOFF.md`: anchor the control lens
at the selection start line for below placement.

Below placement would then look like:

```text
control code lens
selected code
body inlay hint
next code line
```

The control row is above the selection instead of between the selected code and
the body. The body stays below the selection. This works with either hint bias
because the lens and the body are no longer at the same anchor.

If that fallback is needed, these exact `HANDOFF.md` changes are required.

- `Final design`: change "For below placement, the control code lens is anchored
  at the line after the selection, so it appears between the selected code and
  the body hint" to "anchored at the selection start line, above the selected
  code".
- `LSP behavior`, code lens for an active source file: change the below rule from
  "return one control row at the line after the selection end line" to "return
  one control row at the selection start line".
- `Layout and visual rules`: change the below order from "selected code, control
  code lens, body inlay hint, next code line" to "control code lens, selected
  code, body inlay hint, next code line", and remove the block-map assumption.
- `Acceptance criteria`: change "For below placement, the control lens renders
  between the selected code and the body hint" to "For below placement, the
  control lens renders above the selected code".

## Caveats

- Visual rendering, screenshots, scroll behavior, and soft wrap were not tested.
  There is no Zed binary or GUI in this environment.
- The ordering is source-derived. The fake LSP verifies anchors and the modeled
  rows, not GPUI layout.
- Line numbers in Zed source can drift. The behavior claims are tied to the
  reference commit above.
- The spike uses ASCII source, so Python character offsets match LSP UTF-16
  offsets. Non-ASCII sources need UTF-16 handling in the real server.
- The body hint omits `kind`, matching the design. Its bias comes from
  `paddingLeft` or from Zed's surrounding-word heuristic. Faking a kind only to
  force a bias would also change hint-kind filtering.
- `code_lens: "menu"` was not tested. The design requires code lens `on` for v1.
- The recommended `paddingLeft` fix is source-derived. A real visual check should
  confirm it in Zed before the body renderer and control row are finalized.

## Acceptance criteria mapping

- [x] Below-placement order recorded with source and spike evidence, for both
  right-biased and default left-biased hints. Marked source-derived, not
  visually verified.
- [x] Above-placement ordering recorded for both biases.
- [x] Last-line behavior recorded with and without a trailing newline.
- [x] Behavior with inlay hints and code lens disabled recorded.
- [x] Fallback documented if the lens does not render above the hint, plus the
  recommended bias fix.
- [x] No spike code is referenced by extension or LSP crates. There are no
  extension or LSP crates in this branch, and `spike/` is standalone.
