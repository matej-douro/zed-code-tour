#!/usr/bin/env python3
"""Turn a showcase recording into the final clip.

The recording in ``showcase/runs`` is real time, so it contains the OCR pauses
between clicks. This script cuts those gaps out, adds a title card, lower-third
captions, and an end card, then encodes a web-friendly MP4.
"""

from __future__ import annotations

import argparse
import json
import shutil
import subprocess
import sys
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent.parent
MEDIA = REPO_ROOT / "docs" / "media"
WIDTH, HEIGHT = 1600, 1000
FPS = 30
def font_file(query: str, preferred: str) -> str:
    if Path(preferred).exists():
        return preferred
    result = subprocess.run(
        ["fc-match", "-f", "%{file}", query],
        capture_output=True,
        text=True,
    )
    if result.returncode == 0 and result.stdout.strip():
        return result.stdout.strip()
    return preferred


MONO = font_file("JetBrains Mono:style=Regular", "/usr/local/share/fonts/showcase/JetBrainsMono-Regular.ttf")
MONO_BOLD = font_file("JetBrains Mono:style=Bold", "/usr/local/share/fonts/showcase/JetBrainsMono-Bold.ttf")
SANS = font_file("Inter", "/usr/local/share/fonts/showcase/Inter-Variable.ttf")
ACCENT = "#7aa2f7"
TEXT = "#e6edf3"
MUTED = "#7d8590"
SOFT = "#c0caf5"
BASE = "rgba(7,11,17,0.87)"


def run(command: list[str]) -> None:
    subprocess.run(command, check=True)


def magick(*args: str) -> None:
    run(["magick", *args])


def latest_run() -> Path:
    root = REPO_ROOT / "target" / "showcase-runs" / "zed-code-tour"
    if not root.exists():
        raise SystemExit("no showcase run found; run scripts/showcase/record.py first")
    runs = sorted((path for path in root.iterdir() if (path / "timeline.json").exists()), reverse=True)
    if not runs:
        raise SystemExit("no showcase run with a timeline.json found")
    return runs[0]


def click_times(timeline: list[dict]) -> dict[str, float]:
    return {
        event["label"]: event["t"]
        for event in timeline
        if event["name"] == "click" and event.get("label")
    }


def make_title(path: Path, screenshot: Path) -> None:
    magick(
        str(screenshot),
        "-resize",
        f"{WIDTH}x{HEIGHT}!",
        "-blur",
        "0x6",
        "-modulate",
        "80,60,100",
        "-fill",
        BASE,
        "-colorize",
        "87%",
        "(",
        "-size",
        f"{WIDTH}x{HEIGHT}",
        "radial-gradient:rgba(42,66,110,0.42)-rgba(7,11,17,0)",
        ")",
        "-compose",
        "screen",
        "-composite",
        "-fill",
        ACCENT,
        "-font",
        MONO_BOLD,
        "-pointsize",
        "24",
        "-annotate",
        "+120+300",
        "CODE TOURS, IN ZED",
        "-fill",
        TEXT,
        "-pointsize",
        "104",
        "-annotate",
        "+112+420",
        "zed-code-tour",
        "-fill",
        SOFT,
        "-font",
        SANS,
        "-pointsize",
        "38",
        "-annotate",
        "+120+500",
        "Guided code tours, written as Markdown.",
        "-fill",
        MUTED,
        "-font",
        MONO,
        "-pointsize",
        "22",
        "-annotate",
        "+120+560",
        "exact ranges  ·  inlay hints  ·  code lens controls",
        "-fill",
        ACCENT,
        "-draw",
        "roundrectangle 120,580 420,586 3,3",
        "-fill",
        "rgba(122,162,247,0.10)",
        "-stroke",
        "rgba(122,162,247,0.40)",
        "-strokewidth",
        "1.5",
        "-draw",
        "roundrectangle 120,630 466,690 12,12",
        "-stroke",
        "none",
        "-fill",
        ACCENT,
        "-font",
        MONO,
        "-pointsize",
        "22",
        "-annotate",
        "+146+669",
        "▶  Start tour (5 steps)",
        str(path),
    )


def make_end(path: Path, screenshot: Path) -> None:
    magick(
        str(screenshot),
        "-resize",
        f"{WIDTH}x{HEIGHT}!",
        "-blur",
        "0x6",
        "-modulate",
        "80,60,100",
        "-fill",
        "rgba(7,11,17,0.88)",
        "-colorize",
        "88%",
        "(",
        "-size",
        f"{WIDTH}x{HEIGHT}",
        "radial-gradient:rgba(42,66,110,0.38)-rgba(7,11,17,0)",
        ")",
        "-compose",
        "screen",
        "-composite",
        "-fill",
        ACCENT,
        "-font",
        MONO_BOLD,
        "-pointsize",
        "24",
        "-annotate",
        "+120+300",
        "TRY IT",
        "-fill",
        TEXT,
        "-font",
        SANS,
        "-pointsize",
        "62",
        "-annotate",
        "+114+400",
        "A tour is just Markdown.",
        "-fill",
        SOFT,
        "-pointsize",
        "34",
        "-annotate",
        "+120+470",
        "Add a .tour.md file, open it in Zed,",
        "-annotate",
        "+120+520",
        "and press Start tour.",
        "-fill",
        ACCENT,
        "-font",
        MONO,
        "-pointsize",
        "30",
        "-annotate",
        "+120+640",
        "github.com/matej-douro/zed-code-tour",
        "-fill",
        MUTED,
        "-font",
        SANS,
        "-pointsize",
        "24",
        "-annotate",
        "+120+700",
        "Zed extension + tour-lsp  ·  LSP inlay hints and code lenses",
        str(path),
    )


def make_caption(path: Path, label: str, text: str) -> None:
    magick(
        "-size",
        f"{WIDTH}x{HEIGHT}",
        "xc:none",
        "-fill",
        "rgba(8,12,18,0.90)",
        "-stroke",
        "rgba(122,162,247,0.35)",
        "-strokewidth",
        "1.5",
        "-draw",
        "roundrectangle 64,844 1024,936 14,14",
        "-stroke",
        "none",
        "-fill",
        ACCENT,
        "-draw",
        "roundrectangle 88,868 92,912 2,2",
        "-font",
        MONO_BOLD,
        "-pointsize",
        "17",
        "-annotate",
        "+110+882",
        label,
        "-font",
        SANS,
        "-pointsize",
        "30",
        "-fill",
        TEXT,
        "-annotate",
        "+110+918",
        text,
        str(path),
    )


def card_scene(card: Path, out: Path, duration: float, fade_in: float, fade_out: float) -> None:
    frames = int(duration * FPS)
    zoom = min(1.0 + 0.0006 * frames, 1.045)
    filters = [
        f"scale={WIDTH * 2}:{HEIGHT * 2}",
        (
            f"zoompan=z='min(1.0+0.0003*on,{zoom})'"
            f":x='iw/2-(iw/zoom/2)':y='ih/2-(ih/zoom/2)'"
            f":d=1:s={WIDTH}x{HEIGHT}:fps={FPS}"
        ),
        "setsar=1",
    ]
    if fade_in:
        filters.append(f"fade=t=in:st=0:d={fade_in}")
    if fade_out:
        filters.append(f"fade=t=out:st={duration - fade_out}:d={fade_out}")
    run(
        [
            "ffmpeg",
            "-y",
            "-loglevel",
            "error",
            "-loop",
            "1",
            "-framerate",
            str(FPS),
            "-t",
            f"{duration:.3f}",
            "-i",
            str(card),
            "-vf",
            ",".join(filters),
            "-r",
            str(FPS),
            "-c:v",
            "libx264",
            "-preset",
            "medium",
            "-crf",
            "18",
            "-pix_fmt",
            "yuv420p",
            "-an",
            str(out),
        ]
    )


def raw_scene(
    raw: Path,
    out: Path,
    start: float,
    end: float,
    caption: Path | None,
    fade_in: float = 0.0,
    caption_delay: float = 0.25,
) -> None:
    duration = end - start
    filters = [
        f"trim=start=0:end={duration:.3f}",
        "setpts=PTS-STARTPTS",
        f"fps={FPS}",
        f"scale={WIDTH}:{HEIGHT}",
        "setsar=1",
    ]
    if fade_in:
        filters.append(f"fade=t=in:st=0:d={fade_in}")
    command = [
        "ffmpeg",
        "-y",
        "-loglevel",
        "error",
        "-ss",
        f"{start:.3f}",
        "-t",
        f"{duration:.3f}",
        "-i",
        str(raw),
    ]
    if caption is not None:
        command += ["-loop", "1", "-framerate", str(FPS), "-t", f"{duration:.3f}", "-i", str(caption)]
        command += [
            "-filter_complex",
            (
                f"[0:v]{','.join(filters)}[base];"
                f"[1:v]format=rgba,"
                f"fade=t=in:st={caption_delay:.3f}:d=0.30:alpha=1,"
                f"fade=t=out:st={max(duration - 0.55, caption_delay + 0.4):.3f}:d=0.30:alpha=1[cap];"
                f"[base][cap]overlay=0:0:format=auto,format=yuv420p[v]"
            ),
            "-map",
            "[v]",
        ]
    else:
        command += ["-vf", ",".join(filters), "-pix_fmt", "yuv420p"]
    command += [
        "-r",
        str(FPS),
        "-c:v",
        "libx264",
        "-preset",
        "medium",
        "-crf",
        "18",
        "-an",
        str(out),
    ]
    run(command)


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("run", nargs="?", help="showcase run directory")
    parser.add_argument("--out", help="output directory (default: docs/media)")
    args = parser.parse_args()

    run_dir = Path(args.run).resolve() if args.run else latest_run()
    raw = run_dir / "raw.mkv"
    timeline = json.loads((run_dir / "timeline.json").read_text())
    clicks = click_times(timeline)
    stills = {path.stem: path for path in run_dir.glob("*.png")}
    if not raw.exists() or "06-step-5" not in stills:
        raise SystemExit(f"run is missing raw.mkv or screenshots: {run_dir}")

    out_dir = Path(args.out) if args.out else MEDIA
    work = run_dir / "render"
    if work.exists():
        shutil.rmtree(work)
    work.mkdir(parents=True)
    out_dir.mkdir(parents=True, exist_ok=True)

    c0 = clicks["start"]
    c1 = clicks["03-step-2"]
    c2 = clicks["04-step-3"]
    c3 = clicks["05-step-4"]
    c4 = clicks["06-step-5"]
    c5 = clicks["overview"]
    c6 = clicks["stop"]

    title = work / "title.png"
    end = work / "end.png"
    make_title(title, stills["06-step-5"])
    make_end(end, stills["01-tour-file"])

    captions = [
        ("intro", "TOUR FILE", "The tour file is plain Markdown"),
        ("step1", "STEP 01 / 05", "Exact character ranges"),
        ("step2", "STEP 02 / 05", "The extension starts tour-lsp"),
        ("step3", "STEP 03 / 05", "Targets are parsed and validated together"),
        ("step4", "STEP 04 / 05", "The step renders as an inlay hint"),
        ("step5", "STEP 05 / 05", "Prev, Next, Overview, and Stop"),
        ("overview", "OVERVIEW", "Back to the tour file"),
        ("stop", "STOP", "Stop clears every overlay"),
    ]
    caption_paths = {}
    for name, label, text in captions:
        path = work / f"caption-{name}.png"
        make_caption(path, label, text)
        caption_paths[name] = path

    scenes: list[tuple[str, Path]] = []
    title_video = work / "00-title.mp4"
    card_scene(title, title_video, 2.4, 0.45, 0.30)
    scenes.append(("title", title_video))

    def add_raw(
        name: str,
        start: float,
        end: float,
        caption: str | None,
        fade_in: float = 0.0,
        caption_delay: float = 0.25,
    ) -> None:
        path = work / f"{len(scenes):02d}-{name}.mp4"
        raw_scene(
            raw,
            path,
            start,
            end,
            caption_paths.get(caption) if caption else None,
            fade_in,
            caption_delay,
        )
        scenes.append((name, path))

    add_raw("intro", c0 - 2.4, c0 + 0.35, "intro", fade_in=0.25, caption_delay=0.20)
    add_raw("step1", c0 + 0.35, c0 + 3.0, "step1")
    add_raw("step2", c1 - 0.55, c1 + 2.8, "step2", caption_delay=0.75)
    add_raw("step3", c2 - 0.55, c2 + 2.8, "step3", caption_delay=0.75)
    add_raw("step4", c3 - 0.55, c3 + 2.8, "step4", caption_delay=0.75)
    add_raw("step5", c4 - 0.55, c4 + 2.8, "step5", caption_delay=0.75)
    add_raw("overview", c5 - 0.35, c5 + 2.4, "overview", caption_delay=0.95)
    add_raw("stop", c6 - 0.45, c6 + 3.0, "stop", caption_delay=1.25)

    end_video = work / f"{len(scenes):02d}-end.mp4"
    card_scene(end, end_video, 2.8, 0.40, 0.60)
    scenes.append(("end", end_video))

    concat = work / "concat.txt"
    concat.write_text("".join(f"file '{path.resolve()}'\n" for _, path in scenes))
    final = out_dir / "zed-code-tour-showcase.mp4"
    run(
        [
            "ffmpeg",
            "-y",
            "-loglevel",
            "error",
            "-f",
            "concat",
            "-safe",
            "0",
            "-i",
            str(concat),
            "-c",
            "copy",
            "-movflags",
            "+faststart",
            str(final),
        ]
    )

    poster = out_dir / "zed-code-tour-showcase-poster.png"
    shutil.copy2(stills["05-step-4"], poster)
    print(f"video:  {final}")
    print(f"poster: {poster}")
    print(f"work:   {work}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
