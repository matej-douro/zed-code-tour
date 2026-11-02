#!/usr/bin/env python3
"""Headless end-to-end checks against a real Zed build.

The Rust suite in ``crates/tour-lsp`` drives the language server directly. This
script covers the layers above it: Zed loads the built extension WASM, starts
``tour-lsp`` from the login-shell PATH, sends the settings and the tour document,
and renders the code lenses and inlay hints that the server returns.

Every case runs Zed against an isolated profile under Xvfb, records the LSP
transcript from ``Zed.log``, and captures a screenshot. Assertions are made on
the transcript and on the extension index. The screenshots are saved as
artifacts for a human or an agent to inspect.

Only the Python standard library is used. Runtime dependencies:

* Xvfb
* xwd (x11-apps)
* xwdtopnm and pnmtopng (netpbm)
* a Vulkan software driver (mesa-vulkan-drivers / lavapipe on Linux)

The script downloads a pinned Zed release into ``target/e2e/zed`` on first run.
"""

from __future__ import annotations

import argparse
import json
import os
import re
import shutil
import signal
import subprocess
import sys
import tarfile
import tempfile
import time
import urllib.parse
import urllib.request
from dataclasses import dataclass, field
from datetime import datetime, timezone
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent
DEFAULT_ZED_VERSION = "1.26.2"
SCREEN = "1440x900x24"
TOUR_EXTENSION_ID = "zed-code-tour"
TOUR_LANGUAGE = "Tour"

BASE_SETTINGS = {
    "session": {"trust_all_worktrees": True},
    "code_lens": "on",
    "inlay_hints": {"enabled": True, "show_background": True},
    "prettier": {"allowed": False},
    "telemetry": {"diagnostics": False, "metrics": False},
    "auto_update": False,
    # Keep the transcript tour-only: the fixtures use Rust source files, and the
    # tour server is registered for Rust by the extension.
    "languages": {"Rust": {"language_servers": ["tour", "!rust-analyzer"]}},
    "lsp": {"tour": {"settings": {"placement": "below", "auto_start": "always"}}},
}


@dataclass
class Action:
    """A UI action taken after the tour is open, followed by a screenshot."""

    name: str
    click_text: str | None = None
    region: tuple[int, int, int, int] | None = None
    open_file: str | None = None
    scroll: int = 0
    resize: tuple[int, int] | None = None
    # (path relative to the project, (start_line, start_char, end_line, end_char))
    expect_document: tuple[str, tuple[int, int, int, int]] | None = None
    expect_text: list[str] = field(default_factory=list)
    expect_absent_text: list[str] = field(default_factory=list)
    expect_message_contains: str | None = None
    expect_scroll_word: str | None = None
    expect_scroll_direction: str = "down"
    expect_text_inside: tuple[str, int] | None = None
    expect_cleared: str | None = None


@dataclass
class Case:
    name: str
    description: str
    files: dict[str, str]
    open_file: str
    settings: dict
    active: bool
    prompt: bool = False
    code_lens_requests: bool = True
    inlay_requests: bool = True
    show_document: tuple[int, int, int, int] | None = None
    initial_source: str | None = None
    final_tour_file: str | None = None
    source: str = "main.rs"
    final_active: bool | None = None
    hint_position: tuple[int, int] | None = None
    hint_label_prefix: str | None = None
    hint_label_contains: str | None = None
    hint_max_lines: int | None = None
    hint_ellipsis: bool | None = None
    hint_background: bool | None = None
    final_hint: bool = True
    hint_background_word: str = "The"
    source_lenses: list[str] | None = None
    tour_lenses: list[str] | None = None
    cleared_sources: list[str] = field(default_factory=list)
    expect_text: list[str] = field(default_factory=list)
    expect_absent_text: list[str] = field(default_factory=list)
    actions: list[Action] = field(default_factory=list)
    wait_marker: str = '"method":"window/showDocument"'


def deep_merge(base: dict, override: dict) -> dict:
    result = json.loads(json.dumps(base))
    for key, value in override.items():
        if isinstance(value, dict) and isinstance(result.get(key), dict):
            result[key] = deep_merge(result[key], value)
        else:
            result[key] = value
    return result


def tour_settings(placement: str = "below", auto_start: str = "always") -> dict:
    return {"lsp": {"tour": {"settings": {"placement": placement, "auto_start": auto_start}}}}


BASIC_MAIN = """fn main() {
    let total = 1 + 2;
    println!("total = {total}");
}

fn helper() {
    // does nothing
}
"""

BASIC_TOUR = """# Manual tour

[Back to the thread](zed:///agent/thread/manual-session?name=Manual%20tour)

## 1. Entry
[main.rs:2:5-2:21](main.rs#L2:5-L2:21)

The program starts here and computes a total.

## 2. Helper
[main.rs:6:1-7:18](main.rs#L6:1-L7:18)

A helper function with no behavior.
"""


def basic_files() -> dict[str, str]:
    return {"main.rs": BASIC_MAIN, "basic.tour.md": BASIC_TOUR}


def cases() -> list[Case]:
    return [
        Case(
            name="below",
            description="Body hint and control row below the selected range",
            files=basic_files(),
            open_file="basic.tour.md",
            settings=tour_settings("below", "always"),
            active=True,
            show_document=(1, 4, 1, 20),
            hint_position=(2, 0),
            hint_label_prefix=" The program starts here",
            hint_label_contains="computes a total.",
            hint_background=True,
            source_lenses=["1/2 | Next ▶", "Overview", "Back to thread", "Stop"],
            tour_lenses=["■ Stop tour", "⟳ Restart | 1/2"],
            expect_text=["Next"],
        ),
        Case(
            name="above",
            description="Body hint and control row above the selected range",
            files=basic_files(),
            open_file="basic.tour.md",
            settings=tour_settings("above", "always"),
            active=True,
            show_document=(1, 4, 1, 20),
            hint_position=(1, 0),
            hint_label_prefix=" The program starts here",
            source_lenses=["1/2 | Next ▶", "Overview", "Back to thread", "Stop"],
            tour_lenses=["■ Stop tour", "⟳ Restart | 1/2"],
        ),
        Case(
            name="last-line",
            description="Below placement on a last line without a trailing newline",
            files={
                "last.rs": "line0\nline1 long enough\nline2\nlast line",
                "last.tour.md": (
                    "# Last line\n\n"
                    "## 1. End\n"
                    "[last.rs:4:1-4:10](last.rs#L4:1-L4:10)\n\n"
                    "The tour ends on the final line.\n"
                ),
            },
            open_file="last.tour.md",
            settings=tour_settings("below", "always"),
            active=True,
            source="last.rs",
            show_document=(3, 0, 3, 9),
            hint_position=(3, 9),
            hint_label_prefix="\n The tour ends",
            source_lenses=["1/1 | Overview", "Stop"],
            tour_lenses=["■ Stop tour", "⟳ Restart | 1/1"],
        ),
        Case(
            name="relative-dotdot",
            description="Tour targets resolve against the tour directory",
            files={
                "main.rs": BASIC_MAIN,
                "docs/nested.tour.md": (
                    "# Nested tour\n\n"
                    "## 1. Entry\n"
                    "[../main.rs:2:5-2:21](../main.rs#L2:5-L2:21)\n\n"
                    "The target resolves to the parent directory.\n"
                ),
            },
            open_file="docs/nested.tour.md",
            settings=tour_settings("below", "always"),
            active=True,
            show_document=(1, 4, 1, 20),
            hint_position=(2, 0),
            hint_label_contains="parent directory",
            source_lenses=["1/1 | Overview", "Stop"],
        ),
        Case(
            name="code-lens-off",
            description="Tour still starts and the body hint still renders without code lens",
            files=basic_files(),
            open_file="basic.tour.md",
            settings=deep_merge(tour_settings("below", "always"), {"code_lens": "off"}),
            active=True,
            code_lens_requests=False,
            show_document=(1, 4, 1, 20),
            hint_position=(2, 0),
            hint_label_contains="computes a total.",
        ),
        Case(
            name="inlay-off",
            description="Control row still renders and navigation still starts without inlay hints",
            files=basic_files(),
            open_file="basic.tour.md",
            settings=deep_merge(tour_settings("below", "always"), {"inlay_hints": {"enabled": False}}),
            active=True,
            inlay_requests=False,
            show_document=(1, 4, 1, 20),
            source_lenses=["1/2 | Next ▶", "Overview", "Back to thread", "Stop"],
        ),
        Case(
            name="prompt",
            description="auto_start prompt asks before starting and does not start by itself",
            files=basic_files(),
            open_file="basic.tour.md",
            settings=tour_settings("below", "prompt"),
            active=False,
            prompt=True,
            show_document=None,
            expect_text=["Tour"],
            wait_marker='"method":"window/showMessageRequest"',
        ),
        Case(
            name="never",
            description="auto_start never waits for the start lens and renders no source overlays",
            files=basic_files(),
            open_file="basic.tour.md",
            settings=tour_settings("below", "never"),
            active=False,
            show_document=None,
            source_lenses=[],
            tour_lenses=["▶ Start tour (2 steps)"],
            expect_text=["Tour"],
            wait_marker='"method":"textDocument/codeLens"',
        ),
        Case(
            name="last-line-newline",
            description="Below placement uses the empty final line after a trailing newline",
            files={
                "last.rs": "line0\nline1\nline2\nlast\n",
                "last.tour.md": (
                    "# Last line\n\n"
                    "## 1. End\n"
                    "[last.rs:4:1-4:5](last.rs#L4:1-L4:5)\n\n"
                    "The tour ends with a newline.\n"
                ),
            },
            open_file="last.tour.md",
            settings=tour_settings("below", "always"),
            active=True,
            source="last.rs",
            show_document=(3, 0, 3, 4),
            hint_position=(4, 0),
            hint_label_contains="newline",
            source_lenses=["1/1 | Overview", "Stop"],
            tour_lenses=["■ Stop tour", "⟳ Restart | 1/1"],
        ),
        Case(
            name="absolute-path",
            description="Absolute tour targets resolve without the tour directory",
            files={
                "main.rs": BASIC_MAIN,
                "absolute.tour.md": (
                    "# Absolute\n\n"
                    "## 1. Entry\n"
                    "[main.rs:2:5-2:21]({{project}}/main.rs#L2:5-L2:21)\n\n"
                    "The absolute target resolves.\n"
                ),
            },
            open_file="absolute.tour.md",
            settings=tour_settings("below", "always"),
            active=True,
            show_document=(1, 4, 1, 20),
            hint_position=(2, 0),
            hint_label_contains="absolute",
            source_lenses=["1/1 | Overview", "Stop"],
        ),
        Case(
            name="unicode-cafe",
            description="Accented BMP text converts scalar columns to UTF-16 columns",
            files={
                "unicode.rs": "let\t😀 = 1;\r\nlet café = 2;\r\nlast\t値",
                "cafe.tour.md": (
                    "# Cafe\n\n"
                    "## 1. Name\n"
                    "[unicode.rs:2:5-2:9](unicode.rs#L2:5-L2:9)\n\n"
                    "The accented name is selected.\n"
                ),
            },
            open_file="cafe.tour.md",
            settings=tour_settings("below", "always"),
            active=True,
            source="unicode.rs",
            show_document=(1, 4, 1, 8),
            hint_position=(2, 0),
            hint_label_contains="accented",
            source_lenses=["1/1 | Overview", "Stop"],
        ),
        Case(
            name="unicode-last",
            description="Below placement on the last line after a multibyte character",
            files={
                "unicode.rs": "let\t😀 = 1;\r\nlet café = 2;\r\nlast\t値",
                "unicode-last.tour.md": (
                    "# Unicode last\n\n"
                    "## 1. Final\n"
                    "[unicode.rs:3:6-3:7](unicode.rs#L3:6-L3:7)\n\n"
                    "The final character is selected.\n"
                ),
            },
            open_file="unicode-last.tour.md",
            settings=tour_settings("below", "always"),
            active=True,
            source="unicode.rs",
            show_document=(2, 5, 2, 6),
            hint_position=(2, 6),
            hint_label_prefix="\n The final",
            hint_label_contains="character",
            source_lenses=["1/1 | Overview", "Stop"],
        ),
        Case(
            name="long-body",
            description="Long bodies are wrapped and capped to the hint line budget",
            files={
                "main.rs": BASIC_MAIN,
                "long.tour.md": (
                    "# Long body\n\n"
                    "## 1. Entry\n"
                    "[main.rs:2:5-2:21](main.rs#L2:5-L2:21)\n\n"
                    "Wrapping keeps the body readable when it has a lot of words. "
                    + " ".join(["word"] * 200)
                    + "\n"
                ),
            },
            open_file="long.tour.md",
            settings=tour_settings("below", "always"),
            active=True,
            show_document=(1, 4, 1, 20),
            hint_position=(2, 0),
            hint_label_contains="Wrapping",
            hint_max_lines=6,
            hint_ellipsis=True,
            source_lenses=["1/1 | Overview", "Stop"],
        ),
        Case(
            name="background-off",
            description="The hint has no background rectangle when show_background is off",
            files=basic_files(),
            open_file="basic.tour.md",
            settings=deep_merge(
                tour_settings("below", "always"),
                {"inlay_hints": {"enabled": True, "show_background": False}},
            ),
            active=True,
            show_document=(1, 4, 1, 20),
            hint_position=(2, 0),
            hint_label_contains="computes a total.",
            hint_background=False,
            source_lenses=["1/2 | Next ▶", "Overview", "Back to thread", "Stop"],
            tour_lenses=["■ Stop tour", "⟳ Restart | 1/2"],
        ),
        Case(
            name="navigate",
            description="Next, Previous, Overview, tab switching, and Stop through real code lens clicks",
            files=basic_files(),
            open_file="basic.tour.md",
            settings=tour_settings("below", "always"),
            active=True,
            final_active=False,
            show_document=(1, 4, 1, 20),
            source_lenses=[],
            tour_lenses=["▶ Start tour (2 steps)"],
            expect_text=["Next", "1/2"],
            actions=[
                Action(
                    name="next",
                    click_text="Next",
                    region=(100, 100, 1000, 400),
                    expect_document=("main.rs", (5, 0, 6, 17)),
                    expect_text=["2/2"],
                ),
                Action(
                    name="prev",
                    click_text="Prev",
                    region=(100, 100, 1000, 400),
                    expect_document=("main.rs", (1, 4, 1, 20)),
                    expect_text=["1/2"],
                ),
                Action(
                    name="overview",
                    click_text="Overview",
                    region=(100, 100, 1000, 400),
                    expect_document=("basic.tour.md", (0, 0, 0, 0)),
                    expect_text=["Manual"],
                ),
                Action(
                    name="source-tab",
                    click_text="main.rs",
                    region=(0, 30, 600, 70),
                    expect_text=["1/2"],
                ),
                Action(
                    name="next-again",
                    click_text="Next",
                    region=(100, 100, 1000, 400),
                    expect_document=("main.rs", (5, 0, 6, 17)),
                    expect_text=["2/2"],
                ),
                Action(
                    name="stop",
                    click_text="Stop",
                    region=(100, 100, 1000, 400),
                    expect_absent_text=["behavior"],
                ),
            ],
        ),
        Case(
            name="prompt-accept",
            description="Accepting the auto_start prompt starts the tour without code lens",
            files=basic_files(),
            open_file="basic.tour.md",
            settings=tour_settings("below", "prompt"),
            active=False,
            final_active=True,
            prompt=True,
            show_document=None,
            hint_position=(2, 0),
            hint_label_contains="computes a total.",
            source_lenses=["1/2 | Next ▶", "Overview", "Back to thread", "Stop"],
            tour_lenses=["■ Stop tour", "⟳ Restart | 1/2"],
            wait_marker='"method":"window/showMessageRequest"',
            actions=[
                Action(
                    name="accept",
                    click_text="Start",
                    region=(900, 785, 1440, 810),
                    expect_document=("main.rs", (1, 4, 1, 20)),
                    expect_text=["1/2"],
                ),
            ],
        ),
        Case(
            name="prompt-decline",
            description="Declining the auto_start prompt leaves the tour inactive",
            files=basic_files(),
            open_file="basic.tour.md",
            settings=tour_settings("below", "prompt"),
            active=False,
            final_active=False,
            prompt=True,
            show_document=None,
            source_lenses=[],
            tour_lenses=["▶ Start tour (2 steps)"],
            expect_text=["Tour"],
            wait_marker='"method":"window/showMessageRequest"',
            actions=[
                Action(
                    name="decline",
                    click_text="Not",
                    region=(900, 700, 1440, 900),
                    expect_text=["Start"],
                    expect_absent_text=["Not"],
                ),
            ],
        ),
        Case(
            name="prompt-code-lens-off",
            description="Prompt start works when code lens is disabled",
            files=basic_files(),
            open_file="basic.tour.md",
            settings=deep_merge(tour_settings("below", "prompt"), {"code_lens": "off"}),
            active=False,
            final_active=True,
            prompt=True,
            code_lens_requests=False,
            show_document=None,
            hint_position=(2, 0),
            hint_label_contains="computes a total.",
            wait_marker='"method":"window/showMessageRequest"',
            actions=[
                Action(
                    name="accept",
                    click_text="Start",
                    region=(900, 785, 1440, 810),
                    expect_document=("main.rs", (1, 4, 1, 20)),
                    expect_text=["program"],
                ),
            ],
        ),
        Case(
            name="inlay-off-navigate",
            description="Next still moves the tour when inlay hints are disabled",
            files=basic_files(),
            open_file="basic.tour.md",
            settings=deep_merge(tour_settings("below", "always"), {"inlay_hints": {"enabled": False}}),
            active=True,
            final_active=True,
            inlay_requests=False,
            show_document=(1, 4, 1, 20),
            source_lenses=["◀ Prev", "2/2 | Overview", "Back to thread", "Stop"],
            tour_lenses=["■ Stop tour", "⟳ Restart | 2/2"],
            actions=[
                Action(
                    name="next",
                    click_text="Next",
                    region=(100, 100, 1000, 400),
                    expect_document=("main.rs", (5, 0, 6, 17)),
                    expect_text=["2/2"],
                ),
            ],
        ),
        Case(
            name="thread-link",
            description="Back to thread focuses the link line and explains the static fallback",
            files=basic_files(),
            open_file="basic.tour.md",
            settings=tour_settings("below", "always"),
            active=True,
            final_active=True,
            show_document=(1, 4, 1, 20),
            hint_position=(2, 0),
            hint_label_contains="computes a total.",
            source_lenses=["1/2 | Next ▶", "Overview", "Back to thread", "Stop"],
            tour_lenses=["■ Stop tour", "⟳ Restart | 1/2"],
            actions=[
                Action(
                    name="back",
                    click_text="Back",
                    region=(100, 100, 1000, 400),
                    expect_document=("basic.tour.md", (2, 0, 2, 0)),
                    expect_message_contains="zed:///agent/thread/manual-session",
                    expect_text=["Manual"],
                ),
            ],
        ),
        Case(
            name="multiple-tours",
            description="Opening a second tour replaces the active tour and clears old overlays",
            files={
                "main.rs": BASIC_MAIN,
                "other.rs": "fn other() {\n    let second = 2;\n}\n",
                "a.tour.md": (
                    "# First\n\n"
                    "## 1. First\n"
                    "[main.rs:2:5-2:21](main.rs#L2:5-L2:21)\n\n"
                    "The first tour is active.\n"
                ),
                "b.tour.md": (
                    "# Second\n\n"
                    "## 1. Second\n"
                    "[other.rs:2:5-2:18](other.rs#L2:5-L2:18)\n\n"
                    "The second tour wins.\n"
                ),
            },
            open_file="a.tour.md",
            final_tour_file="b.tour.md",
            settings=tour_settings("below", "always"),
            active=True,
            final_active=True,
            initial_source="main.rs",
            source="other.rs",
            show_document=(1, 4, 1, 20),
            hint_position=(2, 0),
            hint_label_contains="second tour wins",
            source_lenses=["1/1 | Overview", "Stop"],
            tour_lenses=["■ Stop tour", "⟳ Restart | 1/1"],
            cleared_sources=["main.rs"],
            actions=[
                Action(
                    name="open-b",
                    open_file="b.tour.md",
                    expect_document=("other.rs", (1, 4, 1, 17)),
                    expect_text=["second"],
                    expect_cleared="main.rs",
                ),
            ],
        ),
        Case(
            name="scroll",
            description="Scrolling keeps the hint attached to the selected line",
            files={
                "tall.rs": "\n".join(
                    f"line {index} padding padding padding" for index in range(1, 101)
                )
                + "\n",
                "scroll.tour.md": (
                    "# Scroll\n\n"
                    "## 1. Middle\n"
                    "[tall.rs:40:1-40:31](tall.rs#L40:1-L40:31)\n\n"
                    "The hint stays attached while the viewport scrolls.\n"
                ),
            },
            open_file="scroll.tour.md",
            settings=tour_settings("below", "always"),
            active=True,
            source="tall.rs",
            show_document=(39, 0, 39, 30),
            hint_position=(40, 0),
            hint_label_contains="viewport",
            final_hint=False,
            source_lenses=["1/1 | Overview", "Stop"],
            expect_text=["viewport"],
            actions=[
                Action(
                    name="scroll-down",
                    scroll=3,
                    expect_text=["viewport"],
                    expect_scroll_word="viewport",
                    expect_scroll_direction="down",
                ),
                Action(
                    name="scroll-up",
                    scroll=-3,
                    expect_text=["viewport"],
                    expect_scroll_word="viewport",
                    expect_scroll_direction="up",
                ),
            ],
        ),
        Case(
            name="narrow",
            description="A narrow window with soft wrap keeps a long hint inside the window",
            files={
                "main.rs": (
                    "fn main() {\n"
                    "    let a_very_long_variable_name = another_function(argument_one, argument_two, argument_three);\n"
                    "}\n"
                ),
                "narrow.tour.md": (
                    "# Narrow\n\n"
                    "## 1. Long line\n"
                    "[main.rs:2:5-2:30](main.rs#L2:5-L2:30)\n\n"
                    "Wrapping keeps this long body readable when the window is narrow and "
                    "soft wrap is enabled, without editing the source buffer or letting the "
                    "hint run past the edge of the editor. budget\n"
                ),
            },
            open_file="narrow.tour.md",
            settings=deep_merge(
                tour_settings("below", "always"),
                {"soft_wrap": "bounded", "preferred_line_length": 60},
            ),
            active=True,
            show_document=(1, 4, 1, 29),
            hint_position=(2, 0),
            hint_label_contains="budget",
            source_lenses=["1/1 | Overview", "Stop"],
            actions=[
                Action(
                    name="narrow-window",
                    resize=(900, 700),
                    expect_text=["budget"],
                    expect_text_inside=("budget", 900),
                ),
            ],
        ),
        Case(
            name="unicode",
            description="Character ranges count Unicode scalars and convert to UTF-16 columns",
            files={
                "unicode.rs": "let\t😀 = 1;\r\nlet café = 2;\r\nlast\t値",
                "unicode.tour.md": (
                    "# Unicode\n\n"
                    "## 1. Emoji\n"
                    "[unicode.rs:1:5-1:6](unicode.rs#L1:5-L1:6)\n\n"
                    "The emoji occupies two UTF-16 code units.\n"
                ),
            },
            open_file="unicode.tour.md",
            settings=tour_settings("below", "always"),
            active=True,
            source="unicode.rs",
            show_document=(0, 4, 0, 6),
            hint_position=(1, 0),
            hint_label_contains="UTF-16",
        ),
    ]


def extension_version() -> str:
    for line in (REPO_ROOT / "extension.toml").read_text().splitlines():
        if line.startswith("version = "):
            return line.split('"')[1]
    fail("could not read version from extension.toml")
    return ""


def fail(message: str) -> None:
    raise RuntimeError(message)


def which(program: str) -> str | None:
    return shutil.which(program)


def check_runtime() -> None:
    missing = [
        name for name in ("Xvfb", "xwd", "xwdtopnm", "pnmtopng", "tesseract") if not which(name)
    ]
    if missing:
        fail(
            "missing runtime tools: "
            + ", ".join(missing)
            + "\nInstall them with: apt-get install -y xvfb x11-apps netpbm "
            "tesseract-ocr libvulkan1 mesa-vulkan-drivers"
        )


def zed_archive_arch() -> str:
    machine = os.uname().machine
    if machine in ("x86_64", "amd64"):
        return "x86_64"
    if machine in ("aarch64", "arm64"):
        return "aarch64"
    fail(f"unsupported architecture for Zed releases: {machine}")
    return ""


def ensure_zed(version: str, explicit: str | None, cache_root: Path) -> Path:
    if explicit:
        path = Path(explicit).resolve()
        if not path.exists():
            fail(f"--zed path does not exist: {path}")
        return path

    install_dir = cache_root / f"zed-{version}"
    binary = install_dir / "zed.app" / "bin" / "zed"
    if binary.exists():
        return binary

    arch = zed_archive_arch()
    url = (
        "https://github.com/zed-industries/zed/releases/download/"
        f"v{version}/zed-linux-{arch}.tar.gz"
    )
    archive = cache_root / f"zed-{version}-{arch}.tar.gz"
    install_dir.mkdir(parents=True, exist_ok=True)
    if not archive.exists():
        print(f"Downloading Zed {version} for {arch}...")
        urllib.request.urlretrieve(url, archive)
    print(f"Extracting Zed {version}...")
    with tarfile.open(archive, "r:gz") as tar:
        try:
            tar.extractall(install_dir, filter="data")
        except TypeError:
            # ``filter`` was added in Python 3.12.
            tar.extractall(install_dir)
    binary.chmod(binary.stat().st_mode | 0o111)
    return binary


def build_artifacts() -> tuple[Path, Path]:
    print("Building extension.wasm and tour-lsp...")
    subprocess.run([str(REPO_ROOT / "scripts" / "build-extension.sh")], check=True)
    subprocess.run(
        ["cargo", "build", "--locked", "-p", "tour-lsp"],
        cwd=REPO_ROOT,
        check=True,
    )
    wasm = REPO_ROOT / "extension.wasm"
    lsp = REPO_ROOT / "target" / "debug" / "tour-lsp"
    if not wasm.exists():
        fail(f"missing extension WASM: {wasm}")
    if not lsp.exists():
        fail(f"missing tour-lsp binary: {lsp}")
    return wasm, lsp


def start_xvfb() -> tuple[subprocess.Popen, str]:
    for number in range(90, 130):
        lock = Path(f"/tmp/.X{number}-lock")
        socket = Path(f"/tmp/.X11-unix/X{number}")
        if lock.exists() or socket.exists():
            continue
        display = f":{number}"
        process = subprocess.Popen(
            ["Xvfb", display, "-screen", "0", SCREEN, "-nolisten", "tcp"],
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
            start_new_session=True,
        )
        for _ in range(100):
            if socket.exists():
                return process, display
            if process.poll() is not None:
                break
            time.sleep(0.1)
        terminate(process)
    fail("could not start Xvfb on displays :90-:129")
    raise AssertionError


def terminate(process: subprocess.Popen) -> None:
    if process.poll() is not None:
        return
    try:
        os.killpg(os.getpgid(process.pid), signal.SIGTERM)
    except ProcessLookupError:
        return
    try:
        process.wait(timeout=10)
    except subprocess.TimeoutExpired:
        try:
            os.killpg(os.getpgid(process.pid), signal.SIGKILL)
        except ProcessLookupError:
            pass
        process.wait(timeout=5)


def zed_pids_for_profile(profile: Path) -> list[int]:
    """Find Zed processes that use this profile.

    The ``zed`` CLI can spawn ``zed-editor`` in a detached session, so killing
    the CLI process group is not enough. A surviving editor also causes the next
    CLI invocation to forward its paths to the old instance instead of starting
    a clean one.
    """
    profile = profile.resolve()
    pids = []
    for entry in Path("/proc").iterdir():
        if not entry.name.isdigit():
            continue
        try:
            comm = (entry / "comm").read_text().strip()
        except OSError:
            continue
        if comm not in ("zed", "zed-editor"):
            continue
        try:
            cmdline = (entry / "cmdline").read_bytes()
            cwd = Path(os.readlink(entry / "cwd"))
        except OSError:
            continue
        # The CLI may pass --user-data-dir relative to the process working
        # directory, so resolve each argument against that directory.
        for raw_argument in cmdline.split(b"\0"):
            if not raw_argument:
                continue
            argument = Path(raw_argument.decode(errors="replace"))
            if not argument.is_absolute():
                argument = cwd / argument
            try:
                if argument.resolve() == profile:
                    pids.append(int(entry.name))
                    break
            except OSError:
                continue
    return pids


def stop_zed(profile: Path, process: subprocess.Popen | None = None) -> None:
    if process is not None:
        terminate(process)
    for sig in (signal.SIGTERM, signal.SIGKILL):
        pids = zed_pids_for_profile(profile)
        if not pids:
            break
        for pid in pids:
            try:
                os.kill(pid, sig)
            except ProcessLookupError:
                pass
        deadline = time.time() + 10
        while time.time() < deadline and zed_pids_for_profile(profile):
            time.sleep(0.2)
    # A killed editor leaves its single-instance socket behind. The next CLI
    # invocation then reports "zed is already running" and exits.
    for socket_path in profile.glob("zed-*.sock"):
        try:
            socket_path.unlink()
        except OSError:
            pass


def prepare_profile(case: Case, wasm: Path, lsp: Path, profile: Path, project: Path) -> None:
    extension_dir = profile / "extensions" / "installed" / TOUR_EXTENSION_ID
    project.mkdir(parents=True, exist_ok=True)
    (profile / "config").mkdir(parents=True)
    (profile / "bin").mkdir(parents=True)
    extension_dir.mkdir(parents=True)

    shutil.copy2(REPO_ROOT / "extension.toml", extension_dir / "extension.toml")
    shutil.copy2(wasm, extension_dir / "extension.wasm")
    language_dir = extension_dir / "languages" / "tour"
    language_dir.mkdir(parents=True)
    shutil.copy2(REPO_ROOT / "languages" / "tour" / "config.toml", language_dir / "config.toml")

    shutil.copy2(lsp, profile / "bin" / "tour-lsp")
    (profile / "bin" / "tour-lsp").chmod(0o755)

    settings = deep_merge(BASE_SETTINGS, case.settings)
    write_settings(profile, settings)

    # Zed resolves binaries through the login shell environment, not the
    # inherited PATH. An isolated HOME keeps that setup self-contained.
    home = profile / "home"
    home.mkdir()
    (home / ".profile").write_text(f'export PATH="{profile / "bin"}:$PATH"\n')

    for name, content in case.files.items():
        path = project / name
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(content.replace("{{project}}", str(project)))


def write_settings(profile: Path, settings: dict) -> None:
    (profile / "config" / "settings.json").write_text(json.dumps(settings, indent=2) + "\n")


def warm_profile(
    zed_binary: Path,
    profile: Path,
    project: Path,
    measured_settings: dict,
    env: dict[str, str],
) -> None:
    """Load the extension once before the measured run.

    On a fresh profile Zed writes ``extensions/index.json`` and then reloads the
    extension, which can detach language servers from buffers that are already
    open. The extension also has to be registered before the
    ``languages.Rust.language_servers`` filter can resolve the ``tour`` server.

    The warm-up uses the default language server list so extension servers start
    without a filter. It does not open the tour file; it only lets the extension
    index settle.
    """
    warm_settings = json.loads(json.dumps(measured_settings))
    warm_settings.pop("languages", None)
    write_settings(profile, warm_settings)

    log_path = profile / "logs" / "Zed.log"
    stdout_path = profile / "warmup.stdout.log"
    shutil.rmtree(profile / "logs", ignore_errors=True)
    with stdout_path.open("wb") as stdout:
        process = subprocess.Popen(
            [
                str(zed_binary),
                "--foreground",
                "--user-data-dir",
                str(profile),
                str(project),
            ],
            env=env,
            stdout=stdout,
            stderr=subprocess.STDOUT,
            start_new_session=True,
        )
        try:
            deadline = time.time() + 40
            while time.time() < deadline:
                if log_path.exists():
                    text = log_path.read_text(errors="replace")
                    if "extensions updated" in text:
                        break
                time.sleep(0.2)
            time.sleep(3)
        finally:
            stop_zed(profile, process)

    shutil.rmtree(profile / "logs", ignore_errors=True)
    write_settings(profile, measured_settings)


def zed_env(profile: Path, display: str) -> dict[str, str]:
    env = os.environ.copy()
    env.update(
        {
            "DISPLAY": display,
            "HOME": str(profile / "home"),
            "SHELL": "/bin/bash",
            "PATH": f"{profile / 'bin'}:{env.get('PATH', '')}",
            "ZED_ALLOW_EMULATED_GPU": "true",
            "LIBGL_ALWAYS_SOFTWARE": "true",
            "RUST_LOG": "info,lsp=trace,extension_host=info",
        }
    )
    if os.geteuid() == 0:
        env["ZED_ALLOW_ROOT"] = "true"
    lavapipe = Path("/usr/share/vulkan/icd.d/lvp_icd.json")
    if lavapipe.exists():
        env["VK_ICD_FILENAMES"] = str(lavapipe)
    return env


def wait_for_log(log_path: Path, marker: str, timeout: float) -> str:
    deadline = time.time() + timeout
    text = ""
    while time.time() < deadline:
        if log_path.exists():
            text = log_path.read_text(errors="replace")
            if "Rendered first frame" in text and marker in text:
                return text
        time.sleep(0.2)
    return text


def wait_for_inlay_response(log_path: Path, source_uri: str, timeout: float) -> None:
    """Wait for a non-empty inlay hint response for the source file.

    Zed requests inlay hints after the buffer is registered with the language
    server. The delay depends on startup work, so poll the transcript instead of
    sleeping for a fixed time.
    """
    deadline = time.time() + timeout
    while time.time() < deadline:
        if log_path.exists():
            session = tour_session(parse_messages(log_path.read_text(errors="replace")))
            for request in outgoing(session, "textDocument/inlayHint"):
                if request.get("params", {}).get("textDocument", {}).get("uri") != source_uri:
                    continue
                response = response_for(session, request)
                if response is not None and response.get("result"):
                    return
        time.sleep(0.2)


def capture_screen(env: dict[str, str], out_dir: Path, name: str) -> tuple[Path, bytes]:
    xwd = subprocess.run(
        ["xwd", "-root", "-silent"], check=True, capture_output=True, env=env
    ).stdout
    pnm = subprocess.run(["xwdtopnm"], input=xwd, capture_output=True).stdout
    png = subprocess.run(["pnmtopng"], input=pnm, capture_output=True).stdout
    png_path = out_dir / f"{name}.png"
    png_path.write_bytes(png)
    return png_path, pnm


def ocr_words(png_path: Path) -> list[dict]:
    try:
        result = subprocess.run(
            ["tesseract", str(png_path), "stdout", "--psm", "11", "tsv"],
            capture_output=True,
            text=True,
            timeout=60,
        )
    except (FileNotFoundError, subprocess.TimeoutExpired):
        return []
    words = []
    for line in result.stdout.splitlines()[1:]:
        parts = line.split("\t")
        if len(parts) < 12 or not parts[11].strip():
            continue
        try:
            words.append(
                {
                    "text": parts[11],
                    "x": int(parts[6]),
                    "y": int(parts[7]),
                    "w": int(parts[8]),
                    "h": int(parts[9]),
                }
            )
        except ValueError:
            continue
    return words


def find_word(
    words: list[dict], text: str, region: tuple[int, int, int, int] | None = None
) -> dict | None:
    for word in words:
        if word["text"].lower() != text.lower():
            continue
        if region is not None:
            x0, y0, x1, y1 = region
            center_x = word["x"] + word["w"] / 2
            center_y = word["y"] + word["h"] / 2
            if not (x0 <= center_x <= x1 and y0 <= center_y <= y1):
                continue
        return word
    return None


def has_word(words: list[dict], text: str) -> bool:
    return any(word["text"].lower() == text.lower() for word in words)


def click_word(env: dict[str, str], word: dict) -> None:
    x = word["x"] + word["w"] // 2
    y = word["y"] + word["h"] // 2
    subprocess.run(
        ["xdotool", "mousemove", str(x), str(y), "click", "1"],
        check=True,
        env=env,
    )


def zed_window_id(env: dict[str, str]) -> str | None:
    for pattern in (" — ", ".*"):
        result = subprocess.run(
            ["xdotool", "search", "--onlyvisible", "--name", pattern],
            capture_output=True,
            text=True,
            env=env,
        )
        ids = result.stdout.split()
        if ids:
            return ids[0]
    return None


def read_ppm(ppm: bytes) -> tuple[int, int, int, bytes]:
    match = re.match(rb"P6\s+(?:#[^\n]*\n\s*)*(\d+)\s+(\d+)\s+(\d+)\s", ppm)
    if not match:
        raise ValueError("unsupported PPM header")
    width = int(match.group(1))
    height = int(match.group(2))
    maxval = int(match.group(3))
    return width, height, maxval, ppm[match.end() :]


def pixel_at(ppm: bytes, x: int, y: int) -> tuple[int, int, int]:
    width, height, maxval, pixels = read_ppm(ppm)
    x = max(0, min(x, width - 1))
    y = max(0, min(y, height - 1))
    if maxval > 255:
        index = (y * width + x) * 6
        return (pixels[index], pixels[index + 2], pixels[index + 4])
    index = (y * width + x) * 3
    return tuple(pixels[index : index + 3])


def color_distance(left: tuple[int, int, int], right: tuple[int, int, int]) -> int:
    return max(abs(a - b) for a, b in zip(left, right))


def ppm_is_blank(ppm: bytes) -> bool:
    if not ppm.startswith(b"P6"):
        return True
    header_end = 0
    fields = 0
    for index, byte in enumerate(ppm[:64]):
        if byte in b" \t\r\n":
            if header_end == 0 and fields == 3:
                header_end = index + 1
                break
            fields += 1
        elif byte == ord("#"):
            break
    if header_end == 0:
        return True
    pixels = ppm[header_end:]
    if len(pixels) < 3 * 100 * 100:
        return True
    return len(set(pixels[::97])) < 8


MESSAGE_RE = re.compile(
    r"\]\s+(outgoing|incoming) message:\s*(\{.*\})\s*$"
)


def parse_messages(log_text: str) -> list[dict]:
    messages = []
    for line in log_text.splitlines():
        match = MESSAGE_RE.search(line)
        if not match:
            continue
        try:
            payload = json.loads(match.group(2))
        except json.JSONDecodeError:
            continue
        messages.append({"direction": match.group(1), "payload": payload})
    return messages


def is_tour_initialize(message: dict) -> bool:
    payload = message["payload"]
    return (
        message["direction"] == "outgoing"
        and payload.get("method") == "initialize"
        and "initializationOptions" in payload.get("params", {})
    )


def tour_session(messages: list[dict]) -> list[dict]:
    starts = [index for index, message in enumerate(messages) if is_tour_initialize(message)]
    if not starts:
        return []
    start = starts[0]
    end = len(messages)
    for index in range(start + 1, len(messages)):
        if messages[index]["direction"] == "outgoing" and messages[index]["payload"].get("method") == "initialize":
            end = index
            break
    return messages[start:end]


def outgoing(session: list[dict], method: str) -> list[dict]:
    return [
        message["payload"]
        for message in session
        if message["direction"] == "outgoing" and message["payload"].get("method") == method
    ]


def incoming(session: list[dict], method: str) -> list[dict]:
    return [
        message["payload"]
        for message in session
        if message["direction"] == "incoming" and message["payload"].get("method") == method
    ]


def response_for(session: list[dict], request: dict) -> dict | None:
    request_id = request.get("id")
    for message in session:
        payload = message["payload"]
        if (
            message["direction"] == "incoming"
            and payload.get("id") == request_id
            and ("result" in payload or "error" in payload)
        ):
            return payload
    return None


def last_response_for_uri(session: list[dict], method: str, uri: str) -> dict | None:
    result = None
    for request in outgoing(session, method):
        if request.get("params", {}).get("textDocument", {}).get("uri") != uri:
            continue
        response = response_for(session, request)
        if response is not None:
            result = response
    return result


class Checker:
    def __init__(self, case: Case) -> None:
        self.case = case
        self.failures: list[str] = []
        self.passes = 0

    def ok(self, condition: bool, message: str) -> None:
        if condition:
            self.passes += 1
        else:
            self.failures.append(message)

    def equal(self, actual, expected, message: str) -> None:
        self.ok(actual == expected, f"{message}: expected {expected!r}, got {actual!r}")


def verify(
    case: Case,
    profile: Path,
    project: Path,
    log_text: str,
    png_path: Path,
    pnm: bytes,
    action_results: list[dict],
) -> Checker:
    check = Checker(case)
    tour_uri = (project / case.open_file).resolve().as_uri()
    final_tour_uri = (project / (case.final_tour_file or case.open_file)).resolve().as_uri()
    source_uri = (project / case.source).resolve().as_uri()
    final_active = case.active if case.final_active is None else case.final_active

    index_path = profile / "extensions" / "index.json"
    check.ok(index_path.exists(), "extension index was not written")
    if index_path.exists():
        index = json.loads(index_path.read_text())
        check.ok(
            TOUR_EXTENSION_ID in index.get("extensions", {}),
            f"{TOUR_EXTENSION_ID} was not indexed",
        )
        check.ok(TOUR_LANGUAGE in index.get("languages", {}), f"{TOUR_LANGUAGE} language was not indexed")
        manifest = index.get("extensions", {}).get(TOUR_EXTENSION_ID, {}).get("manifest", {})
        check.equal(manifest.get("version"), extension_version(), "indexed extension version")

    check.ok(
        "starting language server process" in log_text and "tour-lsp" in log_text,
        "tour-lsp was not started by the extension",
    )
    check.ok(
        'Failed to start language server "tour"' not in log_text,
        "Zed reported a tour language server start failure",
    )
    check.ok("failed to load extension" not in log_text.lower(), "extension failed to load")

    messages = parse_messages(log_text)
    session = tour_session(messages)
    check.ok(bool(session), "no tour LSP session in the transcript")
    if not session:
        return check

    initialize = next(
        (message["payload"] for message in session if is_tour_initialize(message)), {}
    )
    options = initialize.get("params", {}).get("initializationOptions", {})
    expected_tour_settings = case.settings.get("lsp", {}).get("tour", {}).get("settings", {})
    for key, value in expected_tour_settings.items():
        check.equal(options.get(key), value, f"initializationOptions.{key}")

    did_open = outgoing(session, "textDocument/didOpen")
    tour_open = [
        request
        for request in did_open
        if request.get("params", {}).get("textDocument", {}).get("uri") == tour_uri
    ]
    check.ok(bool(tour_open), "the tour document was not opened by Zed")
    if tour_open:
        check.equal(
            tour_open[-1]["params"]["textDocument"].get("languageId"),
            "tour",
            "tour document languageId",
        )

    expected_documents: list[tuple[str, tuple]] = []
    if case.show_document is not None:
        expected_documents.append((case.initial_source or case.source, case.show_document))
    for action in case.actions:
        if action.expect_document is not None:
            expected_documents.append(action.expect_document)
    actual_documents: list[tuple[str, tuple]] = []
    for message in incoming(session, "window/showDocument"):
        params = message.get("params", {})
        uri = params.get("uri", "")
        try:
            path = Path(urllib.parse.urlparse(uri).path).resolve()
            relative = path.relative_to(project.resolve()).as_posix()
        except (TypeError, ValueError):
            relative = uri
        selection = params.get("selection", {})
        start = selection.get("start", {})
        end = selection.get("end", {})
        actual_documents.append(
            (
                relative,
                (
                    start.get("line"),
                    start.get("character"),
                    end.get("line"),
                    end.get("character"),
                ),
            )
        )
    check.equal(actual_documents, expected_documents, "window/showDocument sequence")

    code_lens_requests = outgoing(session, "textDocument/codeLens")
    source_lens_requests = [
        request
        for request in code_lens_requests
        if request.get("params", {}).get("textDocument", {}).get("uri") == source_uri
    ]
    if not case.code_lens_requests:
        check.equal(len(code_lens_requests), 0, "textDocument/codeLens request count")
    else:
        check.ok(bool(code_lens_requests), "Zed did not request code lenses")
        source_lens_response = last_response_for_uri(session, "textDocument/codeLens", source_uri)
        if final_active:
            if case.source_lenses == []:
                check.equal(len(source_lens_requests), 0, "source code lens request count")
            elif case.source_lenses is not None:
                check.ok(source_lens_response is not None, "no code lens response for the source file")
                if source_lens_response is not None:
                    titles = [
                        lens.get("command", {}).get("title", "")
                        for lens in source_lens_response.get("result") or []
                    ]
                    check.equal(titles, case.source_lenses, "source code lens titles")
        elif source_lens_requests:
            check.ok(
                source_lens_response is None
                or (source_lens_response.get("result") or []) == [],
                "source code lenses should be cleared",
            )
        tour_lens_response = last_response_for_uri(session, "textDocument/codeLens", final_tour_uri)
        check.ok(tour_lens_response is not None, "no code lens response for the tour file")
        if tour_lens_response is not None and case.tour_lenses is not None:
            titles = [
                lens.get("command", {}).get("title", "")
                for lens in tour_lens_response.get("result") or []
            ]
            check.equal(titles, case.tour_lenses, "tour code lens titles")

    for relative in case.cleared_sources:
        cleared_uri = (project / relative).resolve().as_uri()
        cleared_hint = last_response_for_uri(session, "textDocument/inlayHint", cleared_uri)
        check.ok(
            cleared_hint is None or (cleared_hint.get("result") or []) == [],
            f"{relative} inlay hints should be cleared",
        )
        cleared_lens = last_response_for_uri(session, "textDocument/codeLens", cleared_uri)
        check.ok(
            cleared_lens is None or (cleared_lens.get("result") or []) == [],
            f"{relative} code lenses should be cleared",
        )

    inlay_requests = outgoing(session, "textDocument/inlayHint")
    source_inlay_requests = [
        request
        for request in inlay_requests
        if request.get("params", {}).get("textDocument", {}).get("uri") == source_uri
    ]
    if not case.inlay_requests:
        check.equal(len(inlay_requests), 0, "textDocument/inlayHint request count")
    else:
        check.ok(bool(inlay_requests), "Zed did not request inlay hints")
        hint_response = last_response_for_uri(session, "textDocument/inlayHint", source_uri)
        if not final_active:
            check.ok(
                hint_response is None or (hint_response.get("result") or []) == [],
                "source inlay hints should be cleared",
            )
        elif case.final_hint:
            check.ok(hint_response is not None, "no inlay hint response for the source file")
            if hint_response is not None:
                hints = hint_response.get("result") or []
                check.equal(len(hints), 1, "active source inlay hint count")
                if hints:
                    hint = hints[0]
                    position = hint.get("position", {})
                    if case.hint_position is not None:
                        check.equal(
                            (position.get("line"), position.get("character")),
                            case.hint_position,
                            "inlay hint position",
                        )
                    label = hint.get("label", "")
                    if case.hint_label_prefix is not None:
                        check.ok(
                            label.startswith(case.hint_label_prefix),
                            f"inlay hint label prefix: expected {case.hint_label_prefix!r}, got {label!r}",
                        )
                    if case.hint_label_contains is not None:
                        check.ok(
                            case.hint_label_contains in label,
                            f"inlay hint label should contain {case.hint_label_contains!r}, got {label!r}",
                        )
                    if case.hint_max_lines is not None:
                        check.ok(
                            len(label.splitlines()) <= case.hint_max_lines,
                            f"inlay hint should use at most {case.hint_max_lines} lines, got {len(label.splitlines())}",
                        )
                    if case.hint_ellipsis is not None:
                        check.equal(
                            "…" in label,
                            case.hint_ellipsis,
                            "inlay hint ellipsis",
                        )
                    check.ok("\u258e" not in label, "inlay hint should not contain a left rule")

    prompts = incoming(session, "window/showMessageRequest")
    if case.prompt:
        check.ok(bool(prompts), "no window/showMessageRequest for auto_start prompt")
        if prompts:
            params = prompts[-1].get("params", {})
            actions = [action.get("title") for action in params.get("actions", [])]
            check.equal(actions, ["Start tour", "Not now"], "prompt actions")
            check.ok(
                "Start tour" in params.get("message", ""),
                f"prompt message should name the tour, got {params.get('message')!r}",
            )
    else:
        check.equal(len(prompts), 0, "unexpected window/showMessageRequest")

    notifications = incoming(session, "window/showMessage")
    for action in case.actions:
        if action.expect_message_contains is not None:
            check.ok(
                any(
                    action.expect_message_contains in message.get("params", {}).get("message", "")
                    for message in notifications
                ),
                f"no window/showMessage containing {action.expect_message_contains!r}",
            )

    check.equal(len(outgoing(session, "textDocument/didChange")), 0, "source buffers should not be edited")
    check.ok(png_path.exists() and png_path.stat().st_size > 10_000, "screenshot was not captured")
    check.ok(not ppm_is_blank(pnm), "screenshot appears blank")

    initial_words = ocr_words(png_path)
    for text in case.expect_text:
        check.ok(has_word(initial_words, text), f"initial screenshot should contain {text!r}")
    for text in case.expect_absent_text:
        check.ok(not has_word(initial_words, text), f"initial screenshot should not contain {text!r}")

    check.equal(len(action_results), len(case.actions), "action count")
    previous_words = initial_words
    for action, result in zip(case.actions, action_results):
        if result.get("error"):
            check.ok(False, f"action {action.name}: {result['error']}")
            continue
        action_png = result.get("png")
        if action_png is None:
            check.ok(False, f"action {action.name}: no screenshot")
            continue
        words = ocr_words(action_png)
        for text in action.expect_text:
            check.ok(
                has_word(words, text),
                f"action {action.name}: screenshot should contain {text!r}",
            )
        for text in action.expect_absent_text:
            check.ok(
                not has_word(words, text),
                f"action {action.name}: screenshot should not contain {text!r}",
            )
        if action.expect_scroll_word is not None:
            before = find_word(previous_words, action.expect_scroll_word)
            after = find_word(words, action.expect_scroll_word)
            moved = (
                before is not None
                and after is not None
                and (
                    after["y"] < before["y"]
                    if action.expect_scroll_direction == "down"
                    else after["y"] > before["y"]
                )
            )
            check.ok(
                moved,
                f"action {action.name}: {action.expect_scroll_word!r} should move "
                f"{action.expect_scroll_direction} after scrolling",
            )
        if action.expect_text_inside is not None:
            text, max_x = action.expect_text_inside
            word = find_word(words, text)
            check.ok(
                word is not None and word["x"] + word["w"] < max_x,
                f"action {action.name}: {text!r} should stay inside x < {max_x}",
            )
        if action.expect_cleared is not None:
            cleared_uri = (project / action.expect_cleared).resolve().as_uri()
            cleared_hint = last_response_for_uri(session, "textDocument/inlayHint", cleared_uri)
            check.ok(
                cleared_hint is None or (cleared_hint.get("result") or []) == [],
                f"action {action.name}: {action.expect_cleared} inlay hints should be cleared",
            )
        previous_words = words

    if case.hint_background is not None:
        word = find_word(initial_words, case.hint_background_word)
        if word is None:
            check.ok(False, "could not locate the hint text for the background check")
        else:
            x = max(0, word["x"] - 3)
            y = word["y"] + word["h"] // 2
            hint_pixel = pixel_at(pnm, x, y)
            background_pixel = pixel_at(pnm, 900, y)
            distance = color_distance(hint_pixel, background_pixel)
            if case.hint_background:
                check.ok(
                    distance > 8,
                    f"hint background should be distinct, got {hint_pixel} vs {background_pixel}",
                )
            else:
                check.ok(
                    distance <= 3,
                    f"hint background should be off, got {hint_pixel} vs {background_pixel}",
                )

    return check


def add_file_to_zed(
    zed_binary: Path, profile: Path, path: Path, env: dict[str, str]
) -> bool:
    result = subprocess.run(
        [
            str(zed_binary),
            "--add",
            "--user-data-dir",
            str(profile),
            str(path),
        ],
        env=env,
        timeout=30,
        check=False,
    )
    return result.returncode == 0


def scroll_editor(env: dict[str, str], amount: int) -> None:
    button = "5" if amount > 0 else "4"
    for _ in range(abs(amount)):
        subprocess.run(
            ["xdotool", "mousemove", "700", "400", "click", button],
            check=True,
            env=env,
        )
        time.sleep(0.2)


def resize_zed(env: dict[str, str], size: tuple[int, int]) -> bool:
    window_id = zed_window_id(env)
    if window_id is None:
        return False
    subprocess.run(
        ["xdotool", "windowsize", window_id, str(size[0]), str(size[1])],
        check=False,
        env=env,
    )
    time.sleep(1)
    return True


def wait_for_document(
    log_path: Path,
    project: Path,
    relative_path: str,
    selection: tuple[int, int, int, int],
    timeout: float,
) -> bool:
    expected_uri = (project / relative_path).resolve().as_uri()
    deadline = time.time() + timeout
    while time.time() < deadline:
        if log_path.exists():
            session = tour_session(parse_messages(log_path.read_text(errors="replace")))
            for message in incoming(session, "window/showDocument"):
                params = message.get("params", {})
                if params.get("uri") != expected_uri:
                    continue
                start = params.get("selection", {}).get("start", {})
                end = params.get("selection", {}).get("end", {})
                actual = (
                    start.get("line"),
                    start.get("character"),
                    end.get("line"),
                    end.get("character"),
                )
                if actual == selection:
                    return True
        time.sleep(0.2)
    return False


def run_case(
    case: Case,
    zed_binary: Path,
    wasm: Path,
    lsp: Path,
    display: str,
    out_root: Path,
    keep: bool,
) -> Checker:
    run_dir = out_root / case.name
    if run_dir.exists():
        shutil.rmtree(run_dir)
    run_dir.mkdir(parents=True)
    # Keep the profile path short: Unix sockets are limited to roughly 100 bytes
    # and Zed creates <profile>/zed-stable.sock.
    workspace = Path(tempfile.mkdtemp(prefix=f"zt-{case.name[:10]}-"))
    project = workspace / "p"
    profile = workspace / "d"
    profile.mkdir()
    prepare_profile(case, wasm, lsp, profile, project)
    env = zed_env(profile, display)
    stop_zed(profile)
    settings = deep_merge(BASE_SETTINGS, case.settings)
    warm_profile(zed_binary, profile, project, settings, env)
    log_path = profile / "logs" / "Zed.log"
    stdout_path = run_dir / "zed.stdout.log"
    open_file = project / case.open_file

    with stdout_path.open("wb") as stdout:
        process = subprocess.Popen(
            [
                str(zed_binary),
                "--foreground",
                "--user-data-dir",
                str(profile),
                str(project),
                str(open_file),
            ],
            cwd=run_dir,
            env=env,
            stdout=stdout,
            stderr=subprocess.STDOUT,
            start_new_session=True,
        )
        try:
            log_text = wait_for_log(log_path, case.wait_marker, timeout=45)
            if case.active and case.inlay_requests:
                source_uri = (project / case.source).resolve().as_uri()
                wait_for_inlay_response(log_path, source_uri, timeout=20)
            time.sleep(1)
            png_path, pnm = capture_screen(env, run_dir, "screen")

            action_results: list[dict] = []
            current_png = png_path
            for action in case.actions:
                result: dict = {"name": action.name, "png": None, "error": None, "pnm": None}
                if action.click_text is not None:
                    word = find_word(ocr_words(current_png), action.click_text, action.region)
                    if word is None:
                        result["error"] = f"OCR could not find {action.click_text!r}"
                        action_results.append(result)
                        break
                    click_word(env, word)
                if action.open_file is not None and not add_file_to_zed(
                    zed_binary, profile, project / action.open_file, env
                ):
                    result["error"] = f"could not open {action.open_file!r}"
                    action_results.append(result)
                    break
                if action.scroll:
                    scroll_editor(env, action.scroll)
                if action.resize is not None and not resize_zed(env, action.resize):
                    result["error"] = "could not find the Zed window to resize"
                    action_results.append(result)
                    break
                if action.expect_document is not None:
                    relative_path, selection = action.expect_document
                    wait_for_document(log_path, project, relative_path, selection, timeout=15)
                # The server sends showDocument before Zed has applied the
                # selection and refreshed overlays. Give the UI a moment before
                # taking the screenshot.
                time.sleep(1)
                action_png, action_pnm = capture_screen(env, run_dir, action.name)
                result["png"] = action_png
                result["pnm"] = action_pnm
                action_results.append(result)
                current_png = action_png
        finally:
            stop_zed(profile, process)

    log_text = log_path.read_text(errors="replace") if log_path.exists() else log_text
    (run_dir / "lsp.jsonl").write_text(
        "\n".join(
            json.dumps({"direction": message["direction"], **message["payload"]})
            for message in parse_messages(log_text)
        )
        + "\n"
    )
    if log_path.exists():
        shutil.copy2(log_path, run_dir / "Zed.log")
    warm_stdout = profile / "warmup.stdout.log"
    if warm_stdout.exists():
        shutil.copy2(warm_stdout, run_dir / "warmup.stdout.log")
    (run_dir / "workspace-path.txt").write_text(str(workspace) + "\n")
    checker = verify(case, profile, project, log_text, png_path, pnm, action_results)
    (run_dir / "result.txt").write_text(
        "\n".join(checker.failures) or "all checks passed"
    )
    if not keep:
        shutil.rmtree(workspace, ignore_errors=True)
    return checker


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--case", action="append", help="run only the named case (repeatable)")
    parser.add_argument("--list", action="store_true", help="list cases and exit")
    parser.add_argument("--zed-version", default=DEFAULT_ZED_VERSION)
    parser.add_argument("--zed", help="use an existing zed binary instead of downloading one")
    parser.add_argument("--no-build", action="store_true", help="reuse existing build artifacts")
    parser.add_argument("--out", help="artifact directory (default: target/e2e/runs/<timestamp>)")
    parser.add_argument("--keep-going", action="store_true", help="run all cases after a failure")
    parser.add_argument("--keep", action="store_true", help="keep temporary profile and project directories")
    args = parser.parse_args()

    all_cases = cases()
    if args.list:
        for case in all_cases:
            print(f"{case.name}: {case.description}")
        return 0

    selected = all_cases
    if args.case:
        names = set(args.case)
        unknown = names - {case.name for case in all_cases}
        if unknown:
            parser.error(f"unknown case(s): {', '.join(sorted(unknown))}")
        selected = [case for case in all_cases if case.name in names]

    check_runtime()
    cache_root = REPO_ROOT / "target" / "e2e" / "zed"
    cache_root.mkdir(parents=True, exist_ok=True)
    zed_binary = ensure_zed(args.zed_version, args.zed, cache_root)

    if args.no_build:
        wasm = REPO_ROOT / "extension.wasm"
        lsp = REPO_ROOT / "target" / "debug" / "tour-lsp"
        if not wasm.exists() or not lsp.exists():
            fail("--no-build requested but build artifacts are missing")
    else:
        wasm, lsp = build_artifacts()
    print(f"Zed binary: {zed_binary}")

    timestamp = datetime.now(timezone.utc).strftime("%Y%m%dT%H%M%SZ")
    out_root = Path(args.out) if args.out else REPO_ROOT / "target" / "e2e" / "runs" / timestamp
    out_root.mkdir(parents=True, exist_ok=True)
    print(f"Artifacts: {out_root}")

    xvfb, display = start_xvfb()
    failures: list[tuple[str, Checker]] = []
    try:
        for case in selected:
            print(f"\n=== {case.name}: {case.description}")
            try:
                checker = run_case(case, zed_binary, wasm, lsp, display, out_root, args.keep)
            except Exception as error:  # noqa: BLE001 - report and continue
                checker = Checker(case)
                checker.failures.append(str(error))
            if checker.failures:
                failures.append((case.name, checker))
                print(f"FAIL ({len(checker.failures)} failed, {checker.passes} passed)")
                for failure in checker.failures:
                    print(f"  - {failure}")
                if not args.keep_going:
                    break
            else:
                print(f"PASS ({checker.passes} checks)")
                print(f"  screenshot: {out_root / case.name / 'screen.png'}")
    finally:
        terminate(xvfb)

    print("\nSummary")
    if failures:
        for name, _ in failures:
            print(f"  FAIL {name}")
        print(f"  {len(selected) - len(failures)} cases passed, {len(failures)} failed")
        return 1
    print(f"  {len(selected)} cases passed")
    return 0


if __name__ == "__main__":
    sys.exit(main())
