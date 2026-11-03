# A guided tour of zed-code-tour

This is the tour a new contributor opens first. It is a plain Markdown file.
Each stop links to an exact range in the code and adds a short note.

## 1. Start at the front door
[src/lib.rs:17:1-20:6](src/lib.rs#L17:1-L20:6)

This is where Zed hands the worktree to the extension. A tour can start
anywhere, but the entry point is a good first stop.

## 2. Meet the tour engine
[src/lib.rs:31:9-37:13](src/lib.rs#L31:9-L37:13)

The extension starts the language server that turns a tour file into a
walkthrough. The reader never has to install it for each project.

## 3. Land on the exact line
[crates/tour-lsp/src/tour/parser.rs:68:1-68:24](crates/tour-lsp/src/tour/parser.rs#L68:1-L68:24)

A tour link carries a file, a line, and a character range. The reader lands on
the exact code the note is talking about.

## 4. Read the note beside the code
[crates/tour-lsp/src/hint.rs:17:1-22:25](crates/tour-lsp/src/hint.rs#L17:1-L22:25)

The note appears below the selected code. The file itself is never changed, so
the reader can keep editing and scrolling as usual.

## 5. Keep your place
[crates/tour-lsp/src/controls.rs:28:1-35:21](crates/tour-lsp/src/controls.rs#L28:1-L35:21)

Previous, Next, Overview, and Stop stay in the control row. The reader can move
through the tour without losing the code they were reading.
