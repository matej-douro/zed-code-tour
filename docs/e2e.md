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
`didOpen` language, `window/showDocument` selections, code lens titles and
anchors, inlay hint positions and labels, prompt requests, and the settings gates
for code lens and inlay hints. The screenshots are saved for visual inspection;
they are not compared pixel by pixel.

## Requirements

- Linux with Xvfb, `xwd` (`x11-apps`), and `xwdtopnm`/`pnmtopng` (`netpbm`).
- A Vulkan software driver. On Debian and Ubuntu:

  ```sh
  sudo apt-get install -y xvfb x11-apps netpbm libvulkan1 mesa-vulkan-drivers
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
| `below` | Body hint and control row are anchored below the selected range. |
| `above` | Body hint and control row are anchored above the selected range. |
| `last-line` | The end-of-line fallback on a last line without a trailing newline. |
| `relative-dotdot` | Targets with `..` resolve against the tour directory. |
| `code-lens-off` | The tour starts and the body hint renders with code lens disabled. |
| `inlay-off` | The control row renders and the tour starts with inlay hints disabled. |
| `prompt` | `auto_start: prompt` sends `window/showMessageRequest` and does not start. |
| `never` | `auto_start: never` leaves the tour inactive and shows the start lens. |
| `unicode` | Tour columns count Unicode scalars; the selection uses UTF-16 columns. |

The prompt case stops at the prompt request. The action response path is covered
by `crates/tour-lsp/tests/lifecycle.rs`, so the headless run does not have to
click a notification.

## Artifacts

Each case directory contains:

- `screen.png`: the rendered Zed window at screenshot time.
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

## CI

`scripts/e2e-zed.sh` is not part of `scripts/test.sh`. It downloads a Zed
release, needs a display server, and is much slower than the Rust suite. It is
suitable for a separate job on a Linux runner that installs the packages above;
see [ci.md](ci.md) for the workflow-scope blocker tracked in #25.
