# Headless Zed end-to-end checks

`scripts/e2e-zed.sh` runs the real extension and language server inside a real
Zed window, without a desktop session. It is the layer between the Rust test
suite, which drives `tour-lsp` over stdio, and the manual matrix in
[testing.md](testing.md), which requires a person to look at the screen.

The script downloads a pinned Zed release, loads the built extension WASM into
an isolated profile, starts `tour-lsp` from the login-shell `PATH`, opens a
`.tour.md` fixture under Xvfb, and records:

- the LSP transcript from `Zed.log`,
- the final extension index,
- a screenshot of the rendered window.

It then asserts the transcript: settings passed to the server, the tour
`didOpen` language, the ordered `window/showDocument` sequence, code lens titles
and anchors, inlay hint positions and labels, prompt requests, and the settings
gates for code lens and inlay hints. Interaction cases also OCR the screenshots,
click real controls, and assert the resulting UI text and background pixels.
Screenshots are saved for visual inspection; they are not golden images.

## Requirements

- Linux with Xvfb, `xwd` (`x11-apps`), `xwdtopnm`/`pnmtopng` (`netpbm`),
  `xdotool`, and Tesseract (`tesseract-ocr`).
- A Vulkan software driver. On Debian and Ubuntu:

  ```sh
  sudo apt-get install -y xvfb x11-apps netpbm xdotool tesseract-ocr \
    libvulkan1 mesa-vulkan-drivers
  ```

- Python 3.10 or newer.
- Rust and the `wasm32-wasip2` target for the extension build. The script calls
  [build-extension.sh](../scripts/build-extension.sh), which installs the target
  through rustup when needed.

The script uses the prebuilt Zed release for the host architecture. It does not
build Zed from source. The pinned version is `DEFAULT_ZED_VERSION` in
`scripts/e2e_zed.py`; the first run downloads it to `target/e2e/zed/`.

## Run

```sh
./scripts/e2e-zed.sh
```

The run prints one line per case and writes artifacts to
`target/e2e/runs/<UTC timestamp>/`. Exit status is non-zero if any case fails.

Useful flags:

```sh
./scripts/e2e-zed.sh --list
./scripts/e2e-zed.sh --case below --case above
./scripts/e2e-zed.sh --keep-going
./scripts/e2e-zed.sh --no-build            # reuse extension.wasm and tour-lsp
./scripts/e2e-zed.sh --zed /path/to/zed    # use a local Zed instead of downloading
./scripts/e2e-zed.sh --keep                # keep temporary profile and project dirs
```

`--case` is repeatable. Without it, all cases run.

## Cases

| Case | What it checks |
| --- | --- |
| `below` | Body hint and control row below the selected range, with a hint background. |
| `above` | Body hint and control row above the selected range. |
| `last-line` | End-of-line fallback on a last line without a trailing newline. |
| `last-line-newline` | The empty final line is used when the file ends with a newline. |
| `relative-dotdot` | Targets with `..` resolve against the tour directory. |
| `absolute-path` | Absolute targets resolve without the tour directory. |
| `unicode` | Emoji selection uses UTF-16 columns while tour columns count scalars. |
| `unicode-cafe` | Accented BMP text selects the exact scalar range. |
| `unicode-last` | Last-line fallback after a multibyte character. |
| `long-body` | Body wrapping and the six-line budget with an ellipsis. |
| `background-off` | `show_background: false` removes the hint background. |
| `code-lens-off` | The tour starts and the body hint renders with code lens disabled. |
| `inlay-off` | The control row renders and the tour starts with inlay hints disabled. |
| `inlay-off-navigate` | Next still moves the tour when inlay hints are disabled. |
| `prompt` | `auto_start: prompt` sends `window/showMessageRequest` and does not start. |
| `prompt-accept` | Clicking `Start tour` starts the tour and renders the overlays. |
| `prompt-decline` | Clicking `Not now` leaves the tour inactive. |
| `prompt-code-lens-off` | Prompt start works when code lens is disabled. |
| `never` | `auto_start: never` leaves the tour inactive and shows the start lens. |
| `navigate` | Next, Previous, Overview, tab switching, and Stop via real lens clicks. |
| `thread-link` | `Back to thread` focuses the link line and explains the static fallback. |
| `multiple-tours` | Opening a second tour replaces the active tour and clears old overlays. |
| `scroll` | Scrolling down and back keeps the hint attached to the viewport. |
| `narrow` | Resizing a window with soft wrap keeps a long hint inside the edge. |

## Interaction

Cases with actions click real UI controls. The harness OCRs the current
screenshot with Tesseract, finds the requested word inside an optional screen
region, and clicks its center with `xdotool`. After each action it waits for the
expected `window/showDocument` selection and saves `<action>.png`.

The assertions cover:

- the ordered `window/showDocument` sequence, including tour-file overview and
  thread-link targets,
- source and tour-file code lens titles in the final state,
- code lens and inlay hint cleanup after Stop,
- OCR text that must appear or disappear after an action,
- the notification action row for `Start tour` and `Not now`,
- the hint background color with `show_background` on and off,
- hint movement when scrolling, and
- the hint staying inside a resized window.

The prompt cases click the notification action row rather than the message line,
which also contains the words `Start tour`.

## Artifacts

Each case directory contains:

- `screen.png`: the rendered Zed window at screenshot time.
- `<action>.png`: one screenshot per UI action, for example `next.png`,
  `stop.png`, or `narrow-window.png`.
- `Zed.log`: the full Zed log, including the LSP transcript.
- `lsp.jsonl`: the parsed LSP messages as one JSON object per line, with a
  `direction` field.
- `result.txt`: `all checks passed` or the list of failures.
- `zed.stdout.log` and `warmup.stdout.log`: process output.
- `workspace-path.txt`: the temporary profile and project path. The directory
  itself is kept only with `--keep`.

Read `lsp.jsonl` to see exactly what the extension passed to the server and what
the server returned. Open `screen.png` when a protocol check passes but the
rendering needs a second look.

## How the isolated profile works

Each case gets a throwaway profile and project in a short directory under
`/tmp`. The extension is installed as a prebuilt extension: the script copies
`extension.toml`, `extension.wasm`, and `languages/tour/config.toml` into
`<profile>/extensions/installed/zed-code-tour/`. Zed does not recompile it.

The profile is warmed up once with the default language server list, then the
measured run starts from the resulting extension index. This avoids the
extension reload that a cold profile performs after writing
`extensions/index.json`, which can detach a language server from an already open
source buffer.

Two details are easy to trip over when extending the harness:

- The profile path must stay short. Zed creates `<profile>/zed-stable.sock`, and
  Unix socket paths are limited to roughly 100 bytes. A long case name or a
  profile under `target/e2e/runs/...` is enough to make Zed exit with
  `zed is already running`.
- Fixtures must live outside the repository worktree. A project under
  `target/`, which is gitignored, does not receive inlay hint requests in this
  Zed build. The harness therefore puts the project in the temporary directory
  too.

The measured run sets `languages.Rust.language_servers` to
`["tour", "!rust-analyzer"]` so the transcript contains only the tour server.
The extension server has to be named explicitly; the `...` wildcard only expands
to built-in language servers.

## Adding a case

Add a `Case` to `cases()` in `scripts/e2e_zed.py`. A case provides the fixture
files, the file to open, the settings, and the expected transcript values. The
fields are documented on the `Case` dataclass. Keep the expectations tied to the
tour format and the documented placement rules, not to the current
implementation.

Run the single case with `--case <name>` and inspect `lsp.jsonl` and
`screen.png` before adding it to the full run.

## Limitations

M18's agent-output `ResourceLink` path still needs an agent and is not covered
here; the `thread-link` case covers the `Back to thread` fallback. The
screenshots are not compared against golden images, so painting regressions that
do not change text or background pixels still need a visual check. The scroll
case asserts that the hint moves with the viewport, not a subjective judgment
about scroll smoothness.

## CI

`scripts/e2e-zed.sh` is not part of `scripts/test.sh`. It downloads a Zed
release, needs a display server, and is much slower than the Rust suite. It is
suitable for a separate job on a Linux runner that installs the packages above;
see [ci.md](ci.md) for the workflow-scope blocker tracked in #25.
