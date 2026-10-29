# Release checklist

Use this for a private or public tagged release. Public release is blocked while
`README.md` says `License: TBD`; see [packaging.md](packaging.md#license-gate).

## Prerequisites

- The working tree is clean and `main` is up to date.
- `./scripts/test.sh` passes.
- A license is chosen, `README.md` is updated, and `LICENSE` is added at the
  repository root. Until then, only local artifact verification is allowed.
- The CI workflow is enabled. If the workflow could not be pushed, follow
  [ci.md](ci.md) and move the templates into `.github/workflows/` first.

## 1. Bump the version

```sh
./scripts/bump-version.sh 0.1.1
```

The script updates `extension.toml`, `Cargo.toml`, and the workspace entries in
`Cargo.lock`, then runs the packaging metadata check. Review the diff:

```sh
git diff -- extension.toml Cargo.toml Cargo.lock
```

The extension version and the workspace version must stay equal.

## 2. Verify

```sh
./scripts/test.sh
./scripts/build-extension.sh
```

For a packaging dry run before the license is resolved, use the local override
and do not publish the output:

```sh
ALLOW_UNLICENSED_RELEASE=1 ./scripts/package-release.sh
```

Inspect the archives under `dist/` and check `dist/SHA256SUMS`. The extension
archive must contain `extension.toml`, `extension.wasm`, and
`languages/tour/config.toml`. The server archive must contain `tour-lsp`
(`tour-lsp.exe` on Windows).

## 3. Release notes

Add highlights, usage changes, and known limitations to
[release-notes-template.md](release-notes-template.md), or let GitHub generate
notes from pull requests. Every release must state the supported Zed extension
API revision and the license status.

## 4. Tag and publish

```sh
git tag -a v0.1.1 -m "zed-code-tour v0.1.1"
git push origin v0.1.1
```

When the release workflow is enabled, the tag builds the host artifact set and
publishes a GitHub release with generated notes. If Actions is not available,
run the packaging script manually and publish:

```sh
./scripts/package-release.sh
gh release create v0.1.1 \
  dist/extension-0.1.1.wasm \
  dist/zed-code-tour-0.1.1-extension.tar.gz \
  dist/tour-lsp-0.1.1-<host-target>.tar.gz \
  dist/SHA256SUMS \
  --title "zed-code-tour v0.1.1" \
  --notes-file docs/release-notes-template.md
```

Replace `<host-target>` with the output of `rustc -vV | awk '/^host: / { print $2 }'`.
For a registry release, repeat the server packaging on each target in
[packaging.md](packaging.md#future-zed-extension-registry-release) and verify
each archive.

## 5. After publishing

- Install the published extension archive in a clean Zed profile and run the
  smoke check in [install.md](install.md#6-verify-the-install).
- Confirm the release assets and `SHA256SUMS` are downloadable.
- Do not move the tag or replace assets after publishing. Cut a new patch
  release instead.
