# zed-code-tour <version>

<One paragraph summary of the release.>

## Highlights

- <User-visible change.>

## Changes

- <Pull request or change.>

## Artifacts

- `extension-<version>.wasm`: prebuilt extension WASM.
- `zed-code-tour-<version>-extension.tar.gz`: dev extension bundle.
- `tour-lsp-<version>-<target>.tar.gz`: native language server for one target.
- `SHA256SUMS`: checksums for the attached archives and WASM.

## Requirements

- Zed with extension API `since_v0_8_0` support (Zed HEAD at
  `37faef81f74fc1bb08be9939c4cffc8ede6ced2f` or newer).
- `code_lens: "on"` and `inlay_hints.enabled: true` for the start/step controls
  and body annotation.
- `tour-lsp` on `PATH` for version 0.1.0 local installs.

## License

License: TBD. Do not publish this release until the repository license is
resolved and this section is updated.
