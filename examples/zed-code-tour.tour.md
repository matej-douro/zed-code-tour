# Inside zed-code-tour

This sample tour walks through the parser, the tour model, and the start lens in
this repository. The thread link below is illustrative and does not resolve.

[Back to the creating thread](zed:///agent/thread/sample-session?name=Code%20tour%20example)

## 1. Parse a tour file
[../crates/tour-lsp/src/tour/parser.rs:66:1-66:18](../crates/tour-lsp/src/tour/parser.rs#L66:1-L66:18)

`parse_tour` reads the Markdown, resolves every target relative to the tour file,
and returns all validation errors together.

## 2. A parsed tour
[../crates/tour-lsp/src/tour/model.rs:5:1-9:2](../crates/tour-lsp/src/tour/model.rs#L5:1-L9:2)

The model keeps the title, the optional thread link, and the ordered steps. Each
step stores its target path, range, body, and optional hint.

## 3. Start and stop lens
[../crates/tour-lsp/src/server.rs:467:5-498:6](../crates/tour-lsp/src/server.rs#L467:5-L498:6)

The tour file gets one lens at the top. It starts the tour when inactive and
offers stop and restart while the tour is running.
