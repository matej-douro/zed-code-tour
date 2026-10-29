# CI and release workflows

The CI workflow runs formatting, lint, tests, and the WASM build on pull
requests to `main` and pushes to `main`. The release workflow runs on `v*` tags,
packages the host artifacts, and publishes a GitHub release.

## Workflow status

The repository token used to create the packaging pull request did not have the
GitHub `workflow` scope, so GitHub rejected adding files under
`.github/workflows/`. The exact templates are checked in at:

- `ci/ci.yml` for pull-request and `main` CI.
- `ci/release.yml` for tagged releases.

## Enable CI

A repository admin with a `workflow`-scoped token moves the templates into
place:

```sh
mkdir -p .github/workflows
git mv ci/ci.yml .github/workflows/ci.yml
git mv ci/release.yml .github/workflows/release.yml
git commit -m "Enable CI and release workflows"
git push
```

The workflow contents do not change during the move. After the push, open a pull
request to confirm the checks run. This is the only remaining setup step.

If the move needs to happen without a local checkout, use the GitHub web UI to
create `.github/workflows/ci.yml` and `.github/workflows/release.yml` with the
contents of the two files.

## What runs

`ci/ci.yml`:

- checks out the repository,
- installs Rust 1.99.1 with `rustfmt`, `clippy`, and `wasm32-wasip2`,
- runs `./scripts/test.sh`, which covers `cargo fmt --check`, `cargo clippy`,
  `cargo build`, `cargo test`, and the packaging metadata check,
- runs `./scripts/build-extension.sh`,
- uploads `extension.wasm` as a build artifact.

`ci/release.yml`:

- runs on a `v*` tag,
- installs Rust 1.99.1 and `wasm32-wasip2`,
- runs `./scripts/package-release.sh`,
- publishes the archives, WASM, and `SHA256SUMS` as a GitHub release with
  generated notes.

The release job stops while `README.md` says `License: TBD`. That is intentional:
the license decision must be resolved before a public release. The release job
builds only the host target. Registry releases need the additional target
archives listed in [packaging.md](packaging.md#future-zed-extension-registry-release).
