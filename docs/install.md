# Install and local setup

This page covers the local install for version 0.1.0: build the extension,
install the `tour-lsp` binary, load the extension as a dev extension in Zed, and
enable the Zed settings the tour overlays need.

Release artifacts and the future extension-registry path are described in
[packaging.md](packaging.md).

## Requirements

- A Zed build whose extension host supports `since_v0_8_0`. The extension API is
  pinned to Zed revision `37faef81f74fc1bb08be9939c4cffc8ede6ced2f`, so use Zed
  HEAD at that revision or newer. Older stable Zed releases cannot load the
  extension.
- Rust 1.99.1. The repository pins it in `rust-toolchain.toml`.
- The `wasm32-wasip2` target. `scripts/build-extension.sh` installs it through
  rustup when it is missing.
- Network access for the pinned `zed_extension_api` git dependency on the first
  build.
- A shell where the directory containing `tour-lsp` is on `PATH`. Zed resolves
  the binary with `Worktree::which("tour-lsp")`.

## 1. Get the repository

```sh
git clone https://github.com/matej-douro/zed-code-tour.git
cd zed-code-tour
```

Run the remaining commands from the repository root.

## 2. Install the `tour-lsp` binary

The extension WASM cannot contain a native binary. It starts `tour-lsp` as a
separate process, and it only looks on `PATH`. The supported reproducible local
path is `$HOME/.local/bin/tour-lsp`:

```sh
mkdir -p "$HOME/.local/bin"
cargo install --path crates/tour-lsp --locked --root "$HOME/.local"
command -v tour-lsp
```

`command -v` must print `$HOME/.local/bin/tour-lsp`. If it does not, add
`$HOME/.local/bin` to `PATH` and restart Zed so the new environment is visible.

The default `cargo install --path crates/tour-lsp --locked` also works. It
installs to `$HOME/.cargo/bin/tour-lsp`, which is normally already on `PATH`.

To use a binary built directly from the checkout:

```sh
cargo build --locked --release -p tour-lsp
install -m 755 target/release/tour-lsp "$HOME/.local/bin/tour-lsp"
```

`tour-lsp` is a stdio language server, not a command-line tool. It has no
`--version` or `--help` flags, so verify it with `command -v` or `ls -l`, not by
running it without an LSP client.

## 3. Build the extension

```sh
./scripts/build-extension.sh
```

The script builds `crates/zed-code-tour-extension` for `wasm32-wasip2` and
writes `extension.wasm` at the repository root. The WASM file is build output
and is not committed.

## 4. Install the dev extension

1. In Zed, run `zed: extensions`.
2. Click `Install Dev Extension`.
3. Select the repository root, the directory that contains `extension.toml`,
   `extension.wasm`, and `languages/`.

After changing Rust code, run `./scripts/build-extension.sh` and use
`Rebuild Dev Extension` in the extensions view. A plain Zed restart does not
rebuild the WASM.

## 5. Enable the required Zed settings

Both the body annotation and the controls are gated by Zed settings that are off
by default. Open Settings with `zed: open settings` and use:

```json
{
  "code_lens": "on",
  "inlay_hints": {
    "enabled": true,
    "show_background": false
  },
  "lsp": {
    "tour": {
      "settings": {
        "placement": "below",
        "auto_start": "prompt"
      }
    }
  }
}
```

- `code_lens: "on"` is required for the tour-file start/stop lens and the
  source-file Previous / Next / Overview / Stop row.
- `inlay_hints.enabled: true` is required for the body annotation.
- `inlay_hints.show_background` is optional. When enabled, the padded hint gets
  a rectangular background.
- `lsp.tour.settings.placement` is `below` (default) or `above`.
- `lsp.tour.settings.auto_start` is `always`, `prompt` (default), or `never`.

With code lens off, auto-start and prompt still start a tour, but the start lens
and step controls are not rendered. With inlay hints off, navigation still works
but the body annotation is absent. See [lsp-state.md](lsp-state.md) for the
settings contract.

## 6. Verify the install

1. Open `examples/zed-code-tour.tour.md`.
2. Accept the `Start tour` prompt, or run the start lens if `auto_start` is
   `never`.
3. Zed selects the first source range. The body hint appears below the
   selection and the control row appears between the selected code and the
   body hint.
4. Use Next and Previous. The selection follows the tour steps.

If the extension cannot find the server, Zed reports an error containing
`tour-lsp was not found in PATH`. Check `command -v tour-lsp` in the shell that
launched Zed, reinstall the binary, and restart Zed.

For the full automated and manual test procedure, see [testing.md](testing.md).

## Uninstall

1. Remove the dev extension from `zed: extensions`.
2. Remove the binary:

   ```sh
   cargo uninstall tour-lsp --root "$HOME/.local"
   ```

   If it was installed with the default Cargo root, use
   `cargo uninstall tour-lsp` instead.
3. Delete the untracked `extension.wasm` from the checkout if it is no longer
   needed.
