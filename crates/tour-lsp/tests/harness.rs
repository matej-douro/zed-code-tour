mod support;

use serde_json::{Value, json};

use support::{
    LspClient, hint_labels, lens_commands, lens_lines, lens_titles, request_methods,
    request_params, result,
};

const SOURCE_URI: &str = "file:///tours/src/main.rs";
const ROUTER_URI: &str = "file:///tours/src/router.rs";
const TOUR_URI: &str = "file:///tours/example.tour.md";
const SOURCE: &str = "line0\nline1 long enough\nline2\nline3 long enough\n";
const TOUR: &str = r#"# Tour

## 1. Entry
[entry](src/main.rs#L2:3-L2:9)

Entry body.
"#;
const TWO_STEP_TOUR: &str = r#"# Tour

## 1. Entry
[entry](src/main.rs#L2:3-L2:9)

Entry body.

## 2. Router
[router](src/router.rs#L4:1-L4:4)

Router body.
"#;
const TWO_STEP_SAME_SOURCE: &str = r#"# Tour

## 1. Entry
[entry](src/main.rs#L2:3-L2:9)

Entry body.

## 2. Next
[next](src/main.rs#L4:1-L4:4)

Next body.
"#;
const MULTILINE_TOUR: &str = r#"# Tour

## 1. Entry
[entry](src/main.rs#L2:3-L2:9)

Entry body line one.
Second.

## 2. Router
[router](src/router.rs#L4:1-L4:4)

Router body.
"#;

fn open_inactive(client: &mut LspClient, source: &str, tour: &str) {
    client.open_source(SOURCE_URI, source);
    client.open_tour(TOUR_URI, tour);
    let messages = client.receive_exact(2);
    assert_eq!(
        request_methods(&messages),
        vec!["workspace/inlayHint/refresh", "workspace/codeLens/refresh"]
    );
}

fn start(client: &mut LspClient) -> Vec<Value> {
    let (messages, response) = client.execute_command("tour.start", Vec::new());
    assert_eq!(result(&response), &json!(null));
    messages
}

fn next(client: &mut LspClient) -> Vec<Value> {
    let (messages, response) = client.execute_command("tour.next", Vec::new());
    assert_eq!(result(&response), &json!(null));
    messages
}

fn stop(client: &mut LspClient) -> Vec<Value> {
    let (messages, response) = client.execute_command("tour.stop", Vec::new());
    assert_eq!(result(&response), &json!(null));
    messages
}

fn assert_show_document(messages: &[Value], uri: &str, start: (u64, u64), end: (u64, u64)) {
    let params = request_params(messages, "window/showDocument")
        .expect("missing window/showDocument request");
    assert_eq!(
        params,
        &json!({
            "uri": uri,
            "external": false,
            "takeFocus": true,
            "selection": {
                "start": {
                    "line": start.0,
                    "character": start.1,
                },
                "end": {
                    "line": end.0,
                    "character": end.1,
                },
            },
        })
    );
}

fn assert_transition_refreshes(messages: &[Value]) {
    assert_eq!(
        request_methods(messages),
        vec![
            "window/showDocument",
            "workspace/inlayHint/refresh",
            "workspace/codeLens/refresh",
        ]
    );
}

fn assert_body_hint(value: &Value, position: (u64, u64), expected: &str) {
    assert_eq!(
        value["position"],
        json!({ "line": position.0, "character": position.1 })
    );
    assert_eq!(value["paddingLeft"], json!(true));
    assert!(value.get("kind").is_none(), "body hint must not set a kind");
    assert!(
        value.get("paddingRight").is_none(),
        "body hint must not set paddingRight"
    );
    assert!(
        value.get("tooltip").is_none(),
        "body hint must not set a tooltip"
    );
    assert_eq!(value["label"], json!(expected));
    assert!(
        expected.ends_with('\n'),
        "body hint should end with a newline"
    );
    let lines: Vec<_> = expected
        .split_terminator('\n')
        .filter(|line| !line.is_empty())
        .collect();
    for line in &lines {
        assert!(
            line.starts_with(" ▎ "),
            "body line should start with a padding space and the left rule: {line:?}"
        );
    }
    if let Some(first) = lines.first() {
        let width = first.chars().count();
        assert!(
            lines.iter().all(|line| line.chars().count() == width),
            "body lines should be padded to the same width: {lines:?}"
        );
    }
}

#[test]
fn layout_below_anchors_the_control_row_between_code_and_body() {
    let mut client = LspClient::start();
    client.initialize_with_options(Some(json!({
        "placement": "below",
        "auto_start": "never",
    })));
    open_inactive(&mut client, SOURCE, MULTILINE_TOUR);

    let (messages, response) = client.code_lenses(TOUR_URI);
    assert!(messages.is_empty());
    assert_eq!(lens_titles(&response), vec!["▶ Start tour (2 steps)"]);
    assert_eq!(lens_commands(&response), vec!["tour.start"]);

    let (messages, response) = client.inlay_hints(SOURCE_URI);
    assert!(messages.is_empty());
    assert_eq!(result(&response), &Value::Null);

    let messages = start(&mut client);
    assert_show_document(&messages, SOURCE_URI, (1, 2), (1, 8));
    assert_transition_refreshes(&messages);

    let (_, response) = client.code_lenses(SOURCE_URI);
    assert_eq!(
        lens_lines(&response),
        vec![2, 2, 2, 2],
        "below placement anchors the row on the line after the selection"
    );
    assert_eq!(
        lens_titles(&response),
        vec!["◀ Prev", "1/2 | Next ▶", "Overview", "Stop"]
    );

    let (_, response) = client.inlay_hints(SOURCE_URI);
    let hints = result(&response).as_array().expect("inlay hint array");
    assert_eq!(hints.len(), 1);
    assert_eq!(hints[0]["position"], json!({ "line": 2, "character": 0 }));
    let expected = format!(" ▎ Entry body line one.\n ▎ Second.{}\n", " ".repeat(13));
    assert_body_hint(&hints[0], (2, 0), &expected);

    client.finish();
}

#[test]
fn layout_above_anchors_the_body_and_controls_at_the_selection_start() {
    let mut client = LspClient::start();
    client.initialize_with_options(Some(json!({
        "placement": "above",
        "auto_start": "never",
    })));
    open_inactive(&mut client, SOURCE, TWO_STEP_TOUR);

    let messages = start(&mut client);
    assert_show_document(&messages, SOURCE_URI, (1, 2), (1, 8));
    assert_transition_refreshes(&messages);

    let (_, response) = client.code_lenses(SOURCE_URI);
    assert_eq!(lens_lines(&response), vec![1, 1, 1, 1]);

    let (_, response) = client.inlay_hints(SOURCE_URI);
    let hints = result(&response).as_array().expect("inlay hint array");
    assert_eq!(hints.len(), 1);
    assert_body_hint(&hints[0], (1, 0), " ▎ Entry body.\n");

    client.finish();
}

#[test]
fn layout_last_line_without_a_trailing_newline_uses_the_end_of_the_line() {
    let mut client = LspClient::start();
    client.initialize_with_options(Some(json!({
        "placement": "below",
        "auto_start": "never",
    })));
    open_inactive(
        &mut client,
        "line0\nline1",
        "# Tour\n\n## 1. Entry\n[entry](src/main.rs#L2:1-L2:5)\n\nEntry body.\n",
    );

    let messages = start(&mut client);
    assert_show_document(&messages, SOURCE_URI, (1, 0), (1, 4));
    assert_transition_refreshes(&messages);

    let (_, response) = client.code_lenses(SOURCE_URI);
    assert_eq!(
        lens_lines(&response),
        vec![1, 1],
        "the last-line fallback anchors controls at the selection start line"
    );

    let (_, response) = client.inlay_hints(SOURCE_URI);
    let hints = result(&response).as_array().expect("inlay hint array");
    assert_eq!(hints.len(), 1);
    assert_eq!(hints[0]["position"], json!({ "line": 1, "character": 5 }));
    assert_body_hint(&hints[0], (1, 5), "\n ▎ Entry body.\n");

    client.finish();
}

#[test]
fn layout_last_line_with_a_trailing_newline_uses_the_empty_final_line() {
    let mut client = LspClient::start();
    client.initialize_with_options(Some(json!({
        "placement": "below",
        "auto_start": "never",
    })));
    open_inactive(
        &mut client,
        "line0\nline1\n",
        "# Tour\n\n## 1. Entry\n[entry](src/main.rs#L2:1-L2:5)\n\nEntry body.\n",
    );

    let messages = start(&mut client);
    assert_show_document(&messages, SOURCE_URI, (1, 0), (1, 4));
    assert_transition_refreshes(&messages);

    let (_, response) = client.code_lenses(SOURCE_URI);
    assert_eq!(lens_lines(&response), vec![2, 2]);

    let (_, response) = client.inlay_hints(SOURCE_URI);
    let hints = result(&response).as_array().expect("inlay hint array");
    assert_eq!(hints.len(), 1);
    assert_body_hint(&hints[0], (2, 0), " ▎ Entry body.\n");

    client.finish();
}

#[test]
fn multiple_tours_and_sources_replace_overlays_and_clean_up_on_stop() {
    let mut client = LspClient::start();
    client.initialize_with_options(Some(json!({ "auto_start": "never" })));

    client.open_source(SOURCE_URI, SOURCE);
    client.open_source(ROUTER_URI, "a\nb\nc\nlong enough\n");
    client.open_tour(TOUR_URI, TWO_STEP_TOUR);
    assert_eq!(
        request_methods(&client.receive_exact(2)),
        vec!["workspace/inlayHint/refresh", "workspace/codeLens/refresh"]
    );

    let messages = start(&mut client);
    assert_show_document(&messages, SOURCE_URI, (1, 2), (1, 8));
    let (_, response) = client.inlay_hints(SOURCE_URI);
    assert_eq!(hint_labels(&response), vec![" ▎ Entry body.\n"]);
    let (_, response) = client.code_lenses(SOURCE_URI);
    assert_eq!(lens_lines(&response), vec![2, 2, 2, 2]);

    let second_tour = r#"# Second

## 1. Router
[router](src/router.rs#L4:1-L4:4)

Router body.

## 2. Main
[main](src/main.rs#L1:1-L1:4)

Main body.
"#;
    client.open_tour("file:///tours/second.tour.md", second_tour);
    assert_eq!(
        request_methods(&client.receive_exact(2)),
        vec!["workspace/inlayHint/refresh", "workspace/codeLens/refresh"],
        "opening a second tour replaces the first and refreshes both overlays"
    );

    let (_, response) = client.inlay_hints(SOURCE_URI);
    assert_eq!(result(&response), &Value::Null);
    let (_, response) = client.code_lenses(SOURCE_URI);
    assert!(result(&response).as_array().expect("lens array").is_empty());
    let (_, response) = client.code_lenses(TOUR_URI);
    assert!(result(&response).as_array().expect("lens array").is_empty());

    let (_, response) = client.code_lenses("file:///tours/second.tour.md");
    assert_eq!(lens_titles(&response), vec!["▶ Start tour (2 steps)"]);

    let messages = start(&mut client);
    assert_show_document(&messages, ROUTER_URI, (3, 0), (3, 3));
    let (_, response) = client.inlay_hints(ROUTER_URI);
    assert_eq!(hint_labels(&response), vec![" ▎ Router body.\n"]);
    let (_, response) = client.code_lenses(ROUTER_URI);
    assert_eq!(lens_lines(&response), vec![4, 4, 4, 4]);

    let messages = next(&mut client);
    assert_show_document(&messages, SOURCE_URI, (0, 0), (0, 3));
    let (_, response) = client.inlay_hints(ROUTER_URI);
    assert_eq!(result(&response), &Value::Null);
    let (_, response) = client.code_lenses(ROUTER_URI);
    assert!(result(&response).as_array().expect("lens array").is_empty());
    let (_, response) = client.inlay_hints(SOURCE_URI);
    assert_eq!(hint_labels(&response), vec![" ▎ Main body.\n"]);

    let messages = stop(&mut client);
    assert_eq!(
        request_methods(&messages),
        vec!["workspace/inlayHint/refresh", "workspace/codeLens/refresh"]
    );
    let (_, response) = client.inlay_hints(SOURCE_URI);
    assert_eq!(result(&response), &Value::Null);
    let (_, response) = client.code_lenses(SOURCE_URI);
    assert!(result(&response).as_array().expect("lens array").is_empty());
    let (_, response) = client.code_lenses("file:///tours/second.tour.md");
    assert_eq!(lens_titles(&response), vec!["▶ Start tour (2 steps)"]);

    client.finish();
}

#[test]
fn closing_the_active_source_removes_its_overlays() {
    let mut client = LspClient::start();
    client.initialize_with_options(Some(json!({ "auto_start": "never" })));
    open_inactive(&mut client, SOURCE, TOUR);
    start(&mut client);

    client.close_document(SOURCE_URI);
    assert_eq!(
        request_methods(&client.receive_exact(2)),
        vec!["workspace/inlayHint/refresh", "workspace/codeLens/refresh"]
    );

    let (_, response) = client.inlay_hints(SOURCE_URI);
    assert_eq!(result(&response), &Value::Null);
    let (_, response) = client.code_lenses(SOURCE_URI);
    assert!(result(&response).as_array().expect("lens array").is_empty());

    client.finish();
}

#[test]
fn prompt_starts_without_code_lens_and_inlay_hints_still_render() {
    let mut client = LspClient::start();
    client.initialize_with_options(Some(json!({ "auto_start": "prompt" })));
    client.open_source(SOURCE_URI, SOURCE);
    client.open_source(ROUTER_URI, "a\nb\nc\nlong enough\n");
    client.open_tour(TOUR_URI, TWO_STEP_TOUR);

    let messages = client.receive_exact(3);
    let prompt = messages
        .iter()
        .find(|message| message["method"] == json!("window/showMessageRequest"))
        .expect("missing start prompt");
    assert_eq!(
        request_methods(&messages[1..]),
        vec!["workspace/inlayHint/refresh", "workspace/codeLens/refresh",]
    );
    let actions: Vec<_> = prompt["params"]["actions"]
        .as_array()
        .expect("prompt actions")
        .iter()
        .map(|action| action["title"].as_str().expect("action title"))
        .collect();
    assert_eq!(actions, vec!["Start tour", "Not now"]);

    client.send_response(prompt["id"].clone(), json!({ "title": "Start tour" }));
    let messages = client.receive_exact(3);
    assert_show_document(&messages, SOURCE_URI, (1, 2), (1, 8));
    assert_transition_refreshes(&messages);

    let (_, response) = client.inlay_hints(SOURCE_URI);
    assert_eq!(hint_labels(&response), vec![" ▎ Entry body.\n"]);

    let messages = next(&mut client);
    assert_show_document(&messages, ROUTER_URI, (3, 0), (3, 3));
    assert_transition_refreshes(&messages);

    client.finish();
}

// Zed settings are client-side, so this is a protocol proxy: it proves the
// server keeps serving lenses and commands when no hint request arrives.
#[test]
fn inlay_hints_off_with_code_lens_on_keeps_the_control_row() {
    let mut client = LspClient::start();
    client.initialize_with_options(Some(json!({ "auto_start": "never" })));
    open_inactive(&mut client, SOURCE, TWO_STEP_TOUR);
    start(&mut client);

    let (_, response) = client.code_lenses(SOURCE_URI);
    assert_eq!(
        lens_titles(&response),
        vec!["◀ Prev", "1/2 | Next ▶", "Overview", "Stop"]
    );

    let messages = stop(&mut client);
    assert_eq!(
        request_methods(&messages),
        vec!["workspace/inlayHint/refresh", "workspace/codeLens/refresh"]
    );
    let (_, response) = client.code_lenses(SOURCE_URI);
    assert!(result(&response).as_array().expect("lens array").is_empty());

    client.finish();
}

// Zed settings are client-side, so this is a protocol proxy: it proves commands
// and navigation do not depend on the client requesting hints or lenses.
#[test]
fn both_overlays_off_still_execute_the_tour_commands() {
    let mut client = LspClient::start();
    client.initialize_with_options(Some(json!({ "auto_start": "never" })));
    open_inactive(&mut client, SOURCE, TWO_STEP_SAME_SOURCE);

    let messages = start(&mut client);
    assert_show_document(&messages, SOURCE_URI, (1, 2), (1, 8));

    let messages = next(&mut client);
    assert_show_document(&messages, SOURCE_URI, (3, 0), (3, 3));

    let (messages, response) = client.execute_command("tour.previous", Vec::new());
    assert_eq!(result(&response), &json!(null));
    assert_show_document(&messages, SOURCE_URI, (1, 2), (1, 8));

    let (messages, response) = client.execute_command("tour.goto", vec![json!(1)]);
    assert_eq!(result(&response), &json!(null));
    assert_show_document(&messages, SOURCE_URI, (3, 0), (3, 3));

    let (messages, response) = client.execute_command("tour.next", Vec::new());
    assert_eq!(result(&response), &json!(null));
    assert!(
        messages.is_empty(),
        "next at the last step should be a no-op"
    );

    let (messages, response) = client.execute_command("tour.goto", vec![json!(2)]);
    assert_eq!(response["error"]["code"], json!(-32803));
    assert_eq!(
        request_methods(&messages),
        vec!["window/showMessage"],
        "an out-of-range goto should report an error without moving"
    );

    let (messages, response) = client.execute_command("tour.goto", vec![json!(0)]);
    assert_eq!(result(&response), &json!(null));
    assert_show_document(&messages, SOURCE_URI, (1, 2), (1, 8));

    let (messages, response) = client.execute_command("tour.previous", Vec::new());
    assert_eq!(result(&response), &json!(null));
    assert!(
        messages.is_empty(),
        "previous at the first step should be a no-op"
    );

    let (messages, response) = client.execute_command("tour.overview", Vec::new());
    assert_eq!(result(&response), &json!(null));
    assert_show_document(&messages, TOUR_URI, (0, 0), (0, 0));
    assert_eq!(
        request_methods(&messages),
        vec!["window/showDocument"],
        "overview only focuses the tour file"
    );

    let messages = stop(&mut client);
    assert_eq!(
        request_methods(&messages),
        vec!["workspace/inlayHint/refresh", "workspace/codeLens/refresh"]
    );

    client.finish();
}

#[test]
fn character_columns_tabs_multibyte_text_and_crlf_use_utf16_ranges() {
    let mut client = LspClient::start();
    client.initialize_with_options(Some(json!({
        "placement": "below",
        "auto_start": "never",
    })));

    let source = "let\t😀 = 1;\r\nlet café = 2;\r\nlast\t値";
    let tour = r#"# Unicode

## 1. Tab
[tab](src/main.rs#L1:1-L1:5)

Tab body.

## 2. Emoji
[emoji](src/main.rs#L1:5-L1:6)

Emoji body.

## 3. Accent
[accent](src/main.rs#L2:5-L2:9)

Accent body.

## 4. Last
[last](src/main.rs#L3:1-L3:6)

Last body.
"#;
    open_inactive(&mut client, source, tour);

    let messages = start(&mut client);
    assert_show_document(&messages, SOURCE_URI, (0, 0), (0, 4));
    let (_, response) = client.inlay_hints(SOURCE_URI);
    assert_eq!(hint_labels(&response), vec![" ▎ Tab body.\n"]);

    let messages = next(&mut client);
    assert_show_document(&messages, SOURCE_URI, (0, 4), (0, 6));
    let (_, response) = client.inlay_hints(SOURCE_URI);
    assert_eq!(hint_labels(&response), vec![" ▎ Emoji body.\n"]);

    let messages = next(&mut client);
    assert_show_document(&messages, SOURCE_URI, (1, 4), (1, 8));
    let (_, response) = client.inlay_hints(SOURCE_URI);
    assert_eq!(hint_labels(&response), vec![" ▎ Accent body.\n"]);

    let messages = next(&mut client);
    assert_show_document(&messages, SOURCE_URI, (2, 0), (2, 5));
    let (_, response) = client.inlay_hints(SOURCE_URI);
    let hints = result(&response).as_array().expect("inlay hint array");
    assert_eq!(hints.len(), 1);
    assert_eq!(hints[0]["position"], json!({ "line": 2, "character": 6 }));
    assert_body_hint(&hints[0], (2, 6), "\n ▎ Last body.\n");

    let (_, response) = client.code_lenses(SOURCE_URI);
    assert_eq!(
        lens_lines(&response),
        vec![2, 2, 2, 2],
        "last-line fallback anchors the row on the selected line"
    );

    client.finish();
}
