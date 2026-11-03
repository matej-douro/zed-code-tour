#!/usr/bin/env python3
"""Record a showcase clip of zed-code-tour in a real Zed window.

The script reuses the headless harness in ``scripts/e2e_zed.py`` for the
isolated profile, extension install, Xvfb display, and OCR-driven clicks. It
records the whole Xvfb display with ffmpeg x11grab, drives the tour through the
real Start/Next/Overview/Stop lenses, and writes a timeline plus screenshots for
post-processing.
"""

from __future__ import annotations

import json
import os
import shutil
import subprocess
import sys
import tempfile
import time
from datetime import datetime, timezone
from pathlib import Path

SCRIPTS = Path(__file__).resolve().parent.parent
REPO_ROOT = SCRIPTS.parent
sys.path.insert(0, str(SCRIPTS))

import e2e_zed as e2e  # noqa: E402

SCREEN = "1600x1000x24"
TOUR = "showcase.tour.md"

SETTINGS = {
    "session": {"trust_all_worktrees": True},
    "code_lens": "on",
    "inlay_hints": {"enabled": True, "show_background": True},
    "theme": "One Dark",
    "buffer_font_family": "JetBrains Mono",
    "buffer_font_size": 20,
    "ui_font_family": "Inter",
    "ui_font_size": 15,
    "prettier": {"allowed": False},
    "telemetry": {"diagnostics": False, "metrics": False},
    "auto_update": False,
    "languages": {"Rust": {"language_servers": ["tour", "!rust-analyzer"]}},
    "project_panel": {"hide_gitignore": True, "default_width": 240},
    "file_scan_exclusions": [
        "**/.git",
        "**/target",
        "**/showcase",
        "**/__pycache__",
    ],
    "lsp": {
        "tour": {
            "settings": {"placement": "below", "auto_start": "never"},
        }
    },
}


class Recorder:
    def __init__(self, out_dir: Path, display: str, env: dict[str, str]) -> None:
        self.out_dir = out_dir
        self.display = display
        self.env = env
        self.started = 0.0
        self.events: list[dict] = []
        self.raw = out_dir / "raw.mkv"
        self.process: subprocess.Popen | None = None

    def start(self) -> None:
        self.process = subprocess.Popen(
            [
                "ffmpeg",
                "-y",
                "-loglevel",
                "error",
                "-f",
                "x11grab",
                "-draw_mouse",
                "1",
                "-framerate",
                "30",
                "-video_size",
                SCREEN.rsplit("x", 1)[0],
                "-i",
                self.display,
                "-c:v",
                "libx264",
                "-preset",
                "ultrafast",
                "-qp",
                "0",
                str(self.raw),
            ],
            stdout=subprocess.DEVNULL,
            stderr=subprocess.PIPE,
            start_new_session=True,
        )
        self.started = time.time()
        time.sleep(1.0)
        self.mark("recording")

    def mark(self, name: str, **extra) -> dict:
        event = {"name": name, "t": round(time.time() - self.started, 3), **extra}
        self.events.append(event)
        print(f"{event['t']:7.2f}s {name}")
        return event

    def stop(self) -> None:
        if self.process is None:
            return
        self.process.send_signal(2)
        try:
            self.process.wait(timeout=30)
        except subprocess.TimeoutExpired:
            self.process.kill()
            self.process.wait()
        stderr = self.process.stderr.read().decode(errors="replace") if self.process.stderr else ""
        if stderr.strip():
            print(stderr.strip())


def snapshot(env: dict[str, str], out_dir: Path, name: str) -> Path:
    png, _ = e2e.capture_screen(env, out_dir, name)
    print(f"  screenshot {png.name}")
    return png


def smooth_click(env: dict[str, str], word: dict) -> None:
    x = int(word["x"] + word["w"] / 2)
    y = int(word["y"] + word["h"] / 2)
    start = subprocess.run(
        ["xdotool", "getmouselocation", "--shell"],
        capture_output=True,
        text=True,
        env=env,
    ).stdout
    values = dict(line.split("=", 1) for line in start.splitlines() if "=" in line)
    sx, sy = int(values.get("X", 0)), int(values.get("Y", 0))
    steps = 14
    for index in range(1, steps + 1):
        px = sx + (x - sx) * index // steps
        py = sy + (y - sy) * index // steps
        subprocess.run(["xdotool", "mousemove", str(px), str(py)], check=True, env=env)
        time.sleep(0.02)
    time.sleep(0.15)
    subprocess.run(["xdotool", "click", "1"], check=True, env=env)


def click_point(env: dict[str, str], x: int, y: int, recorder=None, label: str = "") -> None:
    word = {"x": x - 4, "y": y - 4, "w": 8, "h": 8}
    if recorder is not None:
        recorder.mark("click", label=label)
    smooth_click(env, word)


def ocr_words_fast(png: Path, region: tuple[int, int, int, int]) -> list[dict]:
    x0, y0, x1, y1 = region
    width, height = x1 - x0, y1 - y0
    small = Path("/tmp/showcase-ocr.png")
    subprocess.run(
        [
            "magick",
            str(png),
            "-crop",
            f"{width}x{height}+{x0}+{y0}",
            "+repage",
            "-resize",
            "50%",
            "-colorspace",
            "Gray",
            str(small),
        ],
        check=True,
    )
    tsv = subprocess.run(
        ["tesseract", str(small), "stdout", "--psm", "11", "tsv"],
        capture_output=True,
        text=True,
    ).stdout
    words = []
    for line in tsv.splitlines()[1:]:
        columns = line.split("\t")
        if len(columns) < 12 or not columns[11].strip():
            continue
        words.append(
            {
                "text": columns[11],
                "x": x0 + int(columns[6]) * 2,
                "y": y0 + int(columns[7]) * 2,
                "w": int(columns[8]) * 2,
                "h": int(columns[9]) * 2,
            }
        )
    return words


def click_text(
    env: dict[str, str],
    png: Path,
    text: str,
    region: tuple[int, int, int, int],
    timeout: float = 12.0,
    recorder=None,
    label: str = "",
) -> dict:
    deadline = time.time() + timeout
    while time.time() < deadline:
        word = e2e.find_word(ocr_words_fast(png, region), text, region)
        if word is not None:
            if recorder is not None:
                recorder.mark("click", label=label)
            smooth_click(env, word)
            return word
        time.sleep(0.3)
        png, _ = e2e.capture_screen(env, png.parent, png.stem)
    raise RuntimeError(f"could not find {text!r}")


def wait_document(log_path: Path, relative_path: str, selection, timeout: float = 15.0) -> None:
    if not e2e.wait_for_document(log_path, REPO_ROOT, relative_path, selection, timeout):
        raise RuntimeError(f"timed out waiting for {relative_path} {selection}")


def main() -> int:
    zed = e2e.ensure_zed(
        os.environ.get("SHOWCASE_ZED_VERSION", e2e.DEFAULT_ZED_VERSION),
        os.environ.get("SHOWCASE_ZED"),
        REPO_ROOT / "target" / "e2e" / "zed",
    )

    timestamp = datetime.now(timezone.utc).strftime("%Y%m%dT%H%M%SZ")
    out_root = Path(os.environ.get("SHOWCASE_OUT", REPO_ROOT / "target" / "showcase-runs"))
    out_dir = out_root / "zed-code-tour" / timestamp
    out_dir.mkdir(parents=True, exist_ok=True)
    workspace = Path(tempfile.mkdtemp(prefix="zt-showcase-"))
    profile = workspace / "d"
    profile.mkdir()
    project = REPO_ROOT

    case = e2e.Case(
        name="showcase",
        description="showcase recording",
        files={},
        open_file=TOUR,
        settings=SETTINGS,
        active=False,
    )
    wasm = REPO_ROOT / "extension.wasm"
    lsp = REPO_ROOT / "target" / "debug" / "tour-lsp"
    if not wasm.exists() or not lsp.exists():
        wasm, lsp = e2e.build_artifacts()
    e2e.SCREEN = SCREEN
    e2e.prepare_profile(case, wasm, lsp, profile, project)
    e2e.stop_zed(profile)

    xvfb, display = e2e.start_xvfb()
    env = e2e.zed_env(profile, display)

    print("warming profile")
    e2e.warm_profile(zed, profile, project, SETTINGS, env)
    recorder = Recorder(out_dir, display, env)
    log_path = profile / "logs" / "Zed.log"
    stdout_path = out_dir / "zed.stdout.log"

    try:
        recorder.start()
        with stdout_path.open("wb") as stdout:
            process = subprocess.Popen(
                [
                    str(zed),
                    "--foreground",
                    "--user-data-dir",
                    str(profile),
                    str(project),
                    str(project / TOUR),
                ],
                cwd=out_dir,
                env=env,
                stdout=stdout,
                stderr=subprocess.STDOUT,
                start_new_session=True,
            )
            try:
                e2e.wait_for_log(log_path, '"method":"textDocument/didOpen"', timeout=60)
                time.sleep(2.5)
                png = snapshot(env, out_dir, "01-tour-file")
                recorder.mark("tour-file", png=str(png))

                click_text(env, png, "Start", (60, 80, 1300, 600), recorder=recorder, label="start")
                wait_document(log_path, "src/lib.rs", (16, 0, 19, 5))
                time.sleep(2.4)
                png = snapshot(env, out_dir, "02-step-1")
                recorder.mark("step-1", png=str(png))

                steps = [
                    ("src/lib.rs", (30, 8, 36, 12), "03-step-2"),
                    ("crates/tour-lsp/src/tour/parser.rs", (67, 0, 67, 23), "04-step-3"),
                    ("crates/tour-lsp/src/hint.rs", (16, 0, 21, 24), "05-step-4"),
                    ("crates/tour-lsp/src/controls.rs", (27, 0, 34, 20), "06-step-5"),
                ]
                for relative, selection, name in steps:
                    click_text(env, png, "Next", (60, 80, 1300, 900), recorder=recorder, label=name)
                    wait_document(log_path, relative, selection)
                    time.sleep(2.4)
                    png = snapshot(env, out_dir, name)
                    recorder.mark(name, png=str(png))

                click_point(env, 180, 50, recorder=recorder, label="overview")
                subprocess.run(["xdotool", "mousemove", "900", "460"], check=True, env=env)
                time.sleep(1.6)
                png = snapshot(env, out_dir, "07-overview")
                recorder.mark("overview", png=str(png))

                click_text(env, png, "Stop", (60, 80, 1300, 320), recorder=recorder, label="stop")
                subprocess.run(["xdotool", "mousemove", "900", "460"], check=True, env=env)
                time.sleep(2.0)
                png = snapshot(env, out_dir, "08-stopped")
                recorder.mark("stopped", png=str(png))

                time.sleep(0.5)
            finally:
                e2e.stop_zed(profile, process)
    finally:
        recorder.stop()
        e2e.terminate(xvfb)
        (out_dir / "timeline.json").write_text(json.dumps(recorder.events, indent=2) + "\n")
        if log_path.exists():
            shutil.copy2(log_path, out_dir / "Zed.log")
        if not os.environ.get("SHOWCASE_KEEP"):
            shutil.rmtree(workspace, ignore_errors=True)
    print(f"\nartifacts: {out_dir}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
