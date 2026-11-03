# Thread link

A tour can record the agent thread that created it. The tour file format is
`[Back to the creating thread](zed:///agent/thread/<session_id>?name=<name>)`.
This document describes how the tour model exposes that link and what
`Back to thread` does.

Agent output opens thread `ResourceLink`s in the Agent panel. The code lens and
Markdown preview paths do not route `zed:///agent/thread` URIs, so the tour uses
a static fallback.

## Model and state

The parser stores the first `zed:///agent/thread/` link in the tour as
`ThreadLink`:

- `text`: the human-readable link text,
- `uri`: the full thread URI, including the `name` query parameter,
- `line`: the 1-based line in the tour file that contains the link.

`ActiveTour::thread_link()` exposes the optional link from the active state.
Editing a tour reparses it, so a changed or removed link is reflected after the
next valid `didChange`. A tour without a link exposes `None`.

The link line lets the command focus the tour file where the link is written
without scanning the file again.

## Control row

When a tour has a thread link, the active source control row appends one lens:

```text
◀ Prev | 2/N | Next ▶ | Overview | Back to thread | Stop
```

The lens runs `tour.backToThread`. Tours without a thread link keep the rows in
[step-controls.md](step-controls.md). The anchor line, stock code lens style,
single-step shape, and ordering of the existing controls are unchanged.

`tour.backToThread` is a static fallback:

1. It focuses the active tour file at the recorded link line with
   `window/showDocument` and `takeFocus: true`.
2. It shows an informational `window/showMessage` containing the thread URI and
   explaining that this Zed build does not route `zed:///agent/thread` links
   from code lens. It points the user to the Agent panel.

The command does not change the active step, the running flag, or overlays, so
it sends no refresh requests. If the tour has no thread link, the command fails
with `the active tour has no thread link`, and the server shows the same text as
an error message. It never silently does nothing.

The Markdown preview does not execute tour commands.

## Tests

`crates/tour-lsp/src/tour/parser.rs` covers parsing the link and its line.
`crates/tour-lsp/src/state.rs` covers exposing and clearing the active link.
`crates/tour-lsp/src/controls.rs` covers the conditional control and its
command. `crates/tour-lsp/src/server.rs` covers the control row, the focus and
info fallback, and the no-link error.
