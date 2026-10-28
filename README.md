# zed-code-tour

A Zed extension plus language server for LSP-driven code tours written as
`.tour.md` files. A tour file selects exact character ranges in source files and
renders a virtual annotation block below the selection, with Previous / Next /
Overview / Stop controls as a code lens row. Source buffers are never edited.

Status: design and scaffolding. Nothing is implemented yet. The full design,
constraints, and open questions live in [HANDOFF.md](HANDOFF.md).

## Roadmap

- [ ] Spike: verify code lens and inlay hint ordering and placement
- [ ] Scaffold Zed extension and tour LSP workspace
- [ ] Tour file parser and tour model
- [ ] LSP initialization, settings, and active tour state
- [ ] Start/stop code lens and start flow
- [ ] Body inlay hint renderer
- [ ] Step control code lens row
- [ ] Character-level range navigation with `window/showDocument`
- [ ] Agent integration, sample tour, and usage docs
- [ ] Thread link support and routing gap
- [ ] Test harness and manual verification matrix
- [ ] Packaging, install docs, and CI/release

## Layout

- `HANDOFF.md`: design handoff and source of truth.
- Issues track the implementation plan in small, reviewable PRs.

License: TBD
