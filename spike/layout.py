"""Throwaway layout model for the code tour spike.

This module does not talk to Zed. It builds the LSP payloads and a small
display-row model that follows the ordering documented in docs/spike-layout.md.
The display-row model is source-derived, not a renderer.
"""

from __future__ import annotations

from dataclasses import dataclass
from typing import Iterable

LEFT_RULE = "▎"

DEFAULT_BODY = (
    "Tour step body line one.",
    "Second body line.",
)

LENS_TEXT = "◀ Prev | 2/N | Next ▶ | Overview | Stop"


@dataclass(frozen=True)
class Selection:
    start_line: int
    start_character: int
    end_line: int
    end_character: int


@dataclass(frozen=True)
class LayoutCase:
    name: str
    source: str
    selection: Selection
    placement: str = "below"
    body: tuple[str, ...] = DEFAULT_BODY
    padding_left: bool = False
    two_hints: bool = False
    force_fallback: bool = False


@dataclass(frozen=True)
class Layout:
    hint_position: dict[str, int]
    hint_text: str
    hint_bias: str
    lens_line: int
    fallback: bool
    next_line_exists: bool
    second_hint_position: dict[str, int] | None = None
    second_hint_text: str | None = None


def line_count(source: str) -> int:
    """LSP line count. A trailing newline creates a final empty line."""
    return source.count("\n") + 1


def source_lines(source: str) -> list[str]:
    # split, not splitlines, so a trailing newline keeps the final empty line.
    return source.split("\n")


def line_length(source: str, line: int) -> int:
    lines = source_lines(source)
    if line < 0 or line >= len(lines):
        raise ValueError(f"line {line} is outside 0..{len(lines) - 1}")
    return len(lines[line])


def body_text(body: Iterable[str], padding_left: bool = False) -> str:
    lines = tuple(body)
    if not lines:
        raise ValueError("body must not be empty")
    width = max(len(line) for line in lines)
    prefix = " " if padding_left else ""
    padded = [f"{prefix}{LEFT_RULE} {line.ljust(width)}" for line in lines]
    return "\n".join(padded) + "\n"


def compute_layout(case: LayoutCase) -> Layout:
    if case.placement not in ("below", "above"):
        raise ValueError(f"unknown placement: {case.placement}")

    selection = case.selection
    if selection.start_line > selection.end_line:
        raise ValueError("selection start line is after its end line")

    total_lines = line_count(case.source)
    if not 0 <= selection.start_line < total_lines:
        raise ValueError("selection start line is outside the source")
    if not 0 <= selection.end_line < total_lines:
        raise ValueError("selection end line is outside the source")

    text = body_text(case.body, case.padding_left)

    if case.placement == "above":
        hint_line = selection.start_line
        hint_character = 0
        lens_line = selection.start_line
        fallback = False
        next_line_exists = selection.start_line + 1 < total_lines
        hint_text = text
    else:
        next_line = selection.end_line + 1
        next_line_exists = next_line < total_lines
        fallback = case.force_fallback or not next_line_exists
        if fallback:
            hint_line = selection.end_line
            hint_character = line_length(case.source, selection.end_line)
            lens_line = selection.start_line
            hint_text = "\n" + text
        else:
            hint_line = next_line
            hint_character = 0
            lens_line = next_line
            hint_text = text

    # A hint at line start with no kind and no padding gets a left bias from
    # Zed's surrounding-word heuristic. paddingLeft forces a right bias. A hint
    # at line end is right-biased by the same heuristic.
    hint_bias = "right" if case.padding_left or hint_character > 0 else "left"

    second_position = None
    second_text = None
    if case.two_hints:
        second_line = min(hint_line + 1, total_lines - 1)
        second_position = {"line": second_line, "character": 0}
        second_text = body_text(
            ("Second hint, same stock style.",), case.padding_left
        )

    return Layout(
        hint_position={"line": hint_line, "character": hint_character},
        hint_text=hint_text,
        hint_bias=hint_bias,
        lens_line=lens_line,
        fallback=fallback,
        next_line_exists=next_line_exists,
        second_hint_position=second_position,
        second_hint_text=second_text,
    )


def _hint_payload(position: dict[str, int], text: str, padding_left: bool) -> dict:
    hint = {
        "position": position,
        "label": text,
    }
    if padding_left:
        # This is the standard LSP padding flag, not a style. It also makes
        # Zed choose a right-biased anchor when the hint has no kind.
        hint["paddingLeft"] = True
    return hint


def inlay_hints(case: LayoutCase) -> list[dict]:
    layout = compute_layout(case)
    hints = [
        _hint_payload(layout.hint_position, layout.hint_text, case.padding_left)
    ]
    if case.two_hints:
        hints.append(
            _hint_payload(
                layout.second_hint_position,
                layout.second_hint_text or "",
                case.padding_left,
            )
        )
    return hints


def code_lenses(case: LayoutCase) -> list[dict]:
    layout = compute_layout(case)
    return [
        {
            "range": {
                "start": {"line": layout.lens_line, "character": 0},
                "end": {"line": layout.lens_line, "character": 0},
            },
            "command": {
                "title": LENS_TEXT,
                "command": "tour.next",
            },
        }
    ]


def _body_rows(hint_text: str) -> list[str]:
    chunks = hint_text.split("\n")
    if chunks and chunks[0] == "":
        chunks = chunks[1:]
    if chunks and chunks[-1] == "":
        chunks = chunks[:-1]
    return chunks


def display_rows(
    case: LayoutCase,
    *,
    inlay_hints_enabled: bool = True,
    code_lens_enabled: bool = True,
) -> list[str]:
    """Model display rows using the order derived from Zed source.

    The real renderer is not used here. The model assumes:
    - a code lens block with Above placement renders before the anchored row;
    - at line start, a right-biased hint lets the lens render above it, while
      a left-biased hint pushes the lens below the body;
    - at line end, the lens is above the source line and the body follows it;
    - a trailing newline in the hint text moves the source line after the body.

    Only the primary body hint is rendered here. The two-hint case exists to
    check payload fields and the stock style.
    """

    layout = compute_layout(case)
    lines = source_lines(case.source)
    rows: list[str] = []
    body_rows = _body_rows(layout.hint_text) if inlay_hints_enabled else []
    hint_at_line_start = layout.hint_position["character"] == 0

    for index, line in enumerate(lines):
        lens_here = code_lens_enabled and index == layout.lens_line
        hint_here = inlay_hints_enabled and index == layout.hint_position["line"]

        if hint_here and hint_at_line_start:
            if layout.hint_bias == "right":
                if lens_here:
                    rows.append(f"lens: {LENS_TEXT}")
                rows.extend(f"hint: {row}" for row in body_rows)
            else:
                rows.extend(f"hint: {row}" for row in body_rows)
                if lens_here:
                    rows.append(f"lens: {LENS_TEXT}")
            rows.append(f"{index}: {line}")
        else:
            if lens_here:
                rows.append(f"lens: {LENS_TEXT}")
            rows.append(f"{index}: {line}")
            if hint_here:
                rows.extend(f"hint: {row}" for row in body_rows)

    return rows


def case_set() -> list[LayoutCase]:
    return [
        LayoutCase(
            name="below_middle",
            source="line0\nline1\nline2\nline3\nline4",
            selection=Selection(2, 0, 2, 5),
            padding_left=True,
        ),
        LayoutCase(
            name="below_middle_no_padding",
            source="line0\nline1\nline2\nline3\nline4",
            selection=Selection(2, 0, 2, 5),
        ),
        LayoutCase(
            name="above_middle",
            source="line0\nline1\nline2\nline3\nline4",
            selection=Selection(2, 0, 2, 5),
            placement="above",
            padding_left=True,
        ),
        LayoutCase(
            name="above_middle_no_padding",
            source="line0\nline1\nline2\nline3\nline4",
            selection=Selection(2, 0, 2, 5),
            placement="above",
        ),
        LayoutCase(
            name="below_last_no_trailing_newline",
            source="line0\nline1\nline2",
            selection=Selection(2, 0, 2, 5),
        ),
        LayoutCase(
            name="below_last_with_trailing_newline_forced_fallback",
            source="line0\nline1\nline2\n",
            selection=Selection(2, 0, 2, 5),
            force_fallback=True,
        ),
        LayoutCase(
            name="below_last_with_trailing_newline_normal",
            source="line0\nline1\nline2\n",
            selection=Selection(2, 0, 2, 5),
            padding_left=True,
        ),
        LayoutCase(
            name="below_middle_two_hints",
            source="line0\nline1\nline2\nline3\nline4",
            selection=Selection(2, 0, 2, 5),
            padding_left=True,
            two_hints=True,
        ),
    ]


def case_by_name(name: str) -> LayoutCase:
    for case in case_set():
        if case.name == name:
            return case
    names = ", ".join(case.name for case in case_set())
    raise ValueError(f"unknown case {name!r}; expected one of: {names}")
