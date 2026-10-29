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

- Zed Stable or Preview with extension API 0.7.0 support. The 0.8.0 schema
  hooks are deferred to a draft PR and are not required.
- `code_lens: "on"` and `inlay_hints.enabled: true` for the start/step controls
  and body annotation.
- `tour-lsp` on `PATH` for version 0.1.0 local installs.

## License

License: TBD. Do not publish this release until the repository license is
resolved and this section is updated.
