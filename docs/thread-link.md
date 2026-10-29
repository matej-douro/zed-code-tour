# Thread link and routing gap

A tour can record the agent thread that created it. The tour file format is
`[Back to the creating thread](zed:///agent/thread/<session_id>?name=<name>)`.
This document describes how the tour model exposes that link, what `Back to
thread` does in v1, which Zed surfaces can open a thread URI today, and the
upstream change needed to make code lens and preview links work.

The v1 implementation uses a static fallback. It does not modify Zed core.

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

The lens runs `tour.backToThread`. Tours without a thread link keep the exact
row from issue #7. The anchor line, stock code lens style, single-step shape,
and ordering of the existing controls are unchanged.

`tour.backToThread` is a static fallback in v1:

1. It focuses the active tour file at the recorded link line with
   `window/showDocument` and `takeFocus: true`.
2. It shows an informational `window/showMessage` containing the thread URI and
   explaining that this Zed build does not route `zed:///agent/thread` links
   from code lens. It points the user to the Agent panel.

The command does not change the active step, the running flag, or overlays, so
it sends no refresh requests. If the tour has no thread link, the command fails
with `the active tour has no thread link`, and the server shows the same text as
an error message. It never silently does nothing.

The command is also available to any future preview affordance that can execute
LSP commands. Today the Markdown preview does not execute tour commands.

## Current behavior in Zed

The behavior below was checked against Zed HEAD
`37faef81f74fc1bb08be9939c4cffc8ede6ced2f`.

### Agent output ResourceLink

Works. Agent output renders a `ResourceLink` and its click handler resolves the
URI as a `MentionUri`. `MentionUri::Thread` carries the session id and name, and
the handler calls `AgentPanel::open_thread`.

- `crates/agent_ui/src/conversation_view/thread_view.rs`
  (`render_resource_link`, `open_link`)
- `crates/acp_thread/src/mention.rs` (`MentionUri::Thread`, `MentionUri::parse`)

This is why the start flow can use a `ResourceLink` named `Start tour` and why a
thread `ResourceLink` already works in agent output.

### Markdown preview

Does not open a thread. `open_preview_url` sends scheme URLs to
`workspace.open_url_or_file`. That helper only handles `http`, `https`, and
`file` internally and sends every other scheme to `cx.open_url`, the platform
URL handler. A `zed:///agent/thread/...` destination therefore leaves Zed
instead of resolving to `MentionUri::Thread`.

- `crates/markdown_preview/src/markdown_preview_view.rs` (`open_preview_url`)
- `crates/workspace/src/workspace.rs` (`open_url_or_file`)

### Code lens command

A code lens click executes a workspace code action, which sends
`workspace/executeCommand` to the language server. The server can request
`window/showDocument`.

- `crates/editor/src/code_lens.rs` (lens click handler)
- `crates/project/src/lsp_store.rs` (`ExecuteCommand`, `ShowDocument`)
- `crates/editor/src/items.rs` (`handle_lsp_show_document`)

With `external: false`, `handle_lsp_show_document` requires a file URI and
rejects `zed:///agent/thread/...` as unsupported. With `external: true`, it calls
`cx.open_url`, which is the platform handler, not the agent panel's
`MentionUri` handler. Returning `external: true` therefore does not give the LSP
a reliable way to open a thread today.

### App open-request path

Does not parse thread paths. `OpenRequest::parse` recognizes `zed://agent` for
the new-thread Agent panel entry point, plus extensions, settings, git clone,
and commit URLs. The thread URI has the form `zed:///agent/thread/<id>`, so its
path is `/agent/thread/<id>` and it does not match the `zed://agent` prefix.
`parse_zed_link` only handles channels. The request is logged as an unhandled
URL.

- `crates/zed/src/zed/open_listener.rs` (`OpenRequest::parse`,
  `handle_open_request`)
- `crates/client/src/client.rs` (`parse_zed_link`)

## Decision

Ship the static fallback in v1:

- store and expose the link,
- add the conditional `Back to thread` control,
- focus the tour file at the link line,
- show the URI and the routing limitation in an info message.

Rationale:

- A core patch is outside this repository and cannot be verified against the
  Zed revision pinned here.
- Sending `window/showDocument` with `external: true` would hand the URI to the
  OS URL handler, whose result is not observable and which may silently fail.
  The fallback is deterministic and tells the user what happened.
- The fallback remains useful after upstream routing lands: the link line and
  stored URI are the same data the future command needs.

## Follow-up: route thread URIs

The preferred core patch keeps the routing in one place and reuses the agent
panel's existing thread handler.

1. In `crates/zed/src/zed/open_listener.rs`, add an open-request kind for a
   specific thread, for example `OpenRequestKind::AgentThread { id, name }`.
   Parse both `zed:///agent/thread/<id>?name=<name>` (three slashes) and
   `zed://agent/thread/<id>?name=<name>` paths before the generic
   `zed://agent` branch. Decode the `name` query parameter and reject a missing
   or empty session id.
2. In `handle_open_request`, resolve the active workspace and `AgentPanel`, then
   call `AgentPanel::open_thread(id, None, Some(name.into()), window, cx)`. This
   is the same call used by `MentionUri::Thread` clicks.
3. In `crates/editor/src/items.rs`, make the `external` branch of
   `handle_lsp_show_document` dispatch `zed_actions::OpenZedUrl` for `zed://`
   URIs so the request goes through `OpenListener` instead of the OS handler. As
   an alternative, register the `zed` scheme at startup and accept the OS
   round trip, but that is less direct and depends on the desktop integration.
4. Optionally route Markdown preview thread links through the same
   open-request path. The minimal version is to detect a thread URI in
   `open_preview_url` and dispatch `OpenZedUrl`.
5. Then change `tour.backToThread` to send `window/showDocument` with
   `external: true` and the thread URI. Keep the info message only if routing is
   unavailable or the client reports a failure.

Tests for the upstream patch should cover parsing both thread URL forms, missing
names, opening the thread from an app open request, and a `showDocument`
external request reaching `AgentPanel::open_thread`.

The follow-up is tracked as issue #22 in this repository. It records the Zed
core path only; this repository does not carry a Zed fork or patch.

## Tests

`crates/tour-lsp/src/tour/parser.rs` covers parsing the link and its line.
`crates/tour-lsp/src/state.rs` covers exposing and clearing the active link.
`crates/tour-lsp/src/controls.rs` covers the conditional control and its
command. `crates/tour-lsp/src/server.rs` covers the control row, the focus and
info fallback, and the no-link error.
