# Showcase recording

The showcase clip is a real Zed window, not a mockup.
`record.py` reuses the headless harness in [scripts/e2e_zed.py](../e2e_zed.py)
to install the extension into an isolated profile, opens
[showcase.tour.md](../../showcase.tour.md), clicks the real Start, Next, and
Stop lenses with OCR, and records the Xvfb display with ffmpeg. `render.py`
cuts out the OCR pauses, adds the title and end cards and the lower-third
captions, and writes the final MP4.

## Requirements

- The [headless end-to-end requirements](../../docs/e2e.md): Xvfb, xwd, netpbm,
  xdotool, Tesseract, and a Vulkan software driver.
- ffmpeg and ImageMagick.
- Zed 1.26.2 or newer. `record.py` downloads the pinned e2e release on the
  first run, the same way `scripts/e2e-zed.sh` does.
- JetBrains Mono and Inter for the cards. The scripts fall back to fonts found
  by `fc-match` when the preferred files are missing.

## Run

```sh
python3 scripts/showcase/record.py
python3 scripts/showcase/render.py
```

The recording lands in `target/showcase-runs/zed-code-tour/<timestamp>/`.
`render.py` uses the newest run by default and writes the MP4 and poster to
`out/` in that run directory.

Use `SHOWCASE_ZED=/path/to/zed` to reuse a local binary, or
`SHOWCASE_OUT=/path/to/runs` to write elsewhere.

## Publish

The clip is not committed. Open `README.md` on GitHub, click Edit, drag the
rendered MP4 into the editor, and leave the generated
`https://github.com/user-attachments/assets/<id>` URL alone on its own line.
GitHub renders that URL as the inline player.
