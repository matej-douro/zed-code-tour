# Inside zed-code-tour

A code tour is a Markdown file. Each step links to an exact character range in a
source file and renders a virtual annotation without editing the buffer.

## 1. The extension entry point
[src/lib.rs:17:1-20:6](src/lib.rs#L17:1-L20:6)

The extension implements `zed::Extension`. Zed creates it when the worktree
loads and asks it for the language server command.

## 2. Resolve the server binary
[src/lib.rs:31:9-37:13](src/lib.rs#L31:9-L37:13)

The extension finds `tour-lsp` on PATH and starts it for the worktree. No
per-project install step or build hook is required.

## 3. Parse the tour file
[crates/tour-lsp/src/tour/parser.rs:68:1-68:24](crates/tour-lsp/src/tour/parser.rs#L68:1-L68:24)

`parse_tour` reads the Markdown, resolves every target, and returns every
validation error together instead of stopping at the first one.

## 4. Render the active step
[crates/tour-lsp/src/hint.rs:17:1-22:25](crates/tour-lsp/src/hint.rs#L17:1-L22:25)

The active step becomes one multi-line inlay hint below the selected range. The
source buffer itself is never changed.

## 5. Drive the tour from code lenses
[crates/tour-lsp/src/controls.rs:28:1-35:21](crates/tour-lsp/src/controls.rs#L28:1-L35:21)

Previous, Next, Overview, and Stop are code lenses backed by LSP commands. The
row is anchored between the selected code and the body hint.
