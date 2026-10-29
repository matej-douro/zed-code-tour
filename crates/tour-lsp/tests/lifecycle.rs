use std::{
    io::{BufRead, BufReader, Read, Write},
    process::{Child, ChildStdin, ChildStdout, Command, ExitStatus, Stdio},
    thread,
    time::{Duration, Instant},
};

use serde_json::{Value, json};

const EXIT_TIMEOUT: Duration = Duration::from_secs(5);

struct LspClient {
    child: Child,
    stdin: Option<ChildStdin>,
    stdout: BufReader<ChildStdout>,
}

impl LspClient {
    fn start() -> Self {
        let mut child = Command::new(env!("CARGO_BIN_EXE_tour-lsp"))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .expect("failed to start tour-lsp");

        let stdin = child.stdin.take().expect("missing child stdin");
        let stdout = child.stdout.take().expect("missing child stdout");

        Self {
            child,
            stdin: Some(stdin),
            stdout: BufReader::new(stdout),
        }
    }

    fn initialize(&mut self) {
        self.initialize_with_options(None);
    }

    fn initialize_with_auto_start(&mut self, auto_start: &str) {
        self.initialize_with_options(Some(json!({ "auto_start": auto_start })));
    }

    fn initialize_with_options(&mut self, initialization_options: Option<Value>) {
        let mut params = json!({
            "processId": null,
            "rootUri": null,
            "capabilities": {},
        });
        if let Some(options) = initialization_options {
            params["initializationOptions"] = options;
        }

        self.send(json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": params,
        }));

        let initialize_response = self.receive();
        assert_eq!(initialize_response["id"], 1);
        assert_eq!(
            initialize_response["result"]["capabilities"]["inlayHintProvider"],
            json!(true)
        );
        assert_eq!(
            initialize_response["result"]["capabilities"]["codeLensProvider"],
            json!({ "resolveProvider": false })
        );
        assert_eq!(
            initialize_response["result"]["capabilities"]["executeCommandProvider"]["commands"],
            json!([
                "tour.start",
                "tour.stop",
                "tour.next",
                "tour.previous",
                "tour.goto",
                "tour.overview",
                "tour.backToThread",
            ])
        );
        assert_eq!(
            initialize_response["result"]["serverInfo"]["name"],
            "tour-lsp"
        );

        self.send(json!({
            "jsonrpc": "2.0",
            "method": "initialized",
            "params": {},
        }));
    }

    fn send(&mut self, message: Value) {
        let body = serde_json::to_vec(&message).expect("failed to serialize message");
        let stdin = self.stdin.as_mut().expect("stdin is closed");
        write!(stdin, "Content-Length: {}\r\n\r\n", body.len()).expect("failed to write header");
        stdin.write_all(&body).expect("failed to write body");
        stdin.flush().expect("failed to flush message");
    }

    fn receive(&mut self) -> Value {
        let mut content_length = None;
        loop {
            let mut line = String::new();
            let bytes_read = self
                .stdout
                .read_line(&mut line)
                .expect("failed to read header");
            assert!(bytes_read > 0, "server closed stdout unexpectedly");

            let line = line.trim_end();
            if line.is_empty() {
                break;
            }

            if let Some(value) = line.strip_prefix("Content-Length:") {
                content_length = Some(
                    value
                        .trim()
                        .parse::<usize>()
                        .expect("invalid Content-Length"),
                );
            }
        }

        let content_length = content_length.expect("missing Content-Length header");
        let mut body = vec![0; content_length];
        self.stdout
            .read_exact(&mut body)
            .expect("failed to read message body");

        serde_json::from_slice(&body).expect("failed to parse message body")
    }

    fn receive_until_response(&mut self, id: i64) -> Vec<Value> {
        self.receive_until_result(id).0
    }

    fn receive_until_result(&mut self, id: i64) -> (Vec<Value>, Value) {
        let mut messages = Vec::new();

        loop {
            let message = self.receive();
            if message.get("method").is_none() && message["id"] == json!(id) {
                return (messages, message);
            }
            messages.push(message);
        }
    }

    fn close_stdin(&mut self) {
        self.stdin.take();
    }

    fn wait_with_timeout(&mut self) -> ExitStatus {
        let deadline = Instant::now() + EXIT_TIMEOUT;
        loop {
            if let Some(status) = self.child.try_wait().expect("failed to check child status") {
                return status;
            }

            if Instant::now() >= deadline {
                self.child.kill().expect("failed to kill tour-lsp");
                panic!("tour-lsp did not exit within {EXIT_TIMEOUT:?}");
            }

            thread::sleep(Duration::from_millis(10));
        }
    }
}

#[test]
fn completes_initialize_shutdown_and_exit() {
    let mut client = LspClient::start();
    client.initialize();

    client.send(json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "workspace/symbol",
        "params": { "query": "" },
    }));

    let unsupported_response = client.receive();
    assert_eq!(unsupported_response["id"], 2);
    assert_eq!(unsupported_response["error"]["code"], -32601);

    client.send(json!({
        "jsonrpc": "2.0",
        "id": 3,
        "method": "shutdown",
        "params": null,
    }));

    let shutdown_response = client.receive();
    assert_eq!(shutdown_response["id"], 3);
    assert_eq!(shutdown_response["result"], Value::Null);

    client.send(json!({
        "jsonrpc": "2.0",
        "method": "exit",
        "params": null,
    }));

    let status = client.wait_with_timeout();
    assert!(status.success(), "tour-lsp exited with {status}");
}

#[test]
fn exits_with_failure_when_exit_arrives_before_shutdown() {
    let mut client = LspClient::start();
    client.initialize();

    client.send(json!({
        "jsonrpc": "2.0",
        "method": "exit",
        "params": null,
    }));

    let status = client.wait_with_timeout();
    assert_eq!(
        status.code(),
        Some(1),
        "tour-lsp should exit with code 1 on exit before shutdown, got {status}"
    );
}

#[test]
fn exits_when_stdin_closes() {
    let mut client = LspClient::start();
    client.initialize();

    client.close_stdin();

    let status = client.wait_with_timeout();
    assert!(status.success(), "tour-lsp exited with {status}");
}

#[test]
fn refreshes_hints_and_lenses_for_tour_documents_only() {
    let mut client = LspClient::start();
    client.initialize_with_auto_start("never");

    client.send(json!({
        "jsonrpc": "2.0",
        "method": "textDocument/didOpen",
        "params": {
            "textDocument": {
                "uri": "file:///tours/notes.md",
                "languageId": "Markdown",
                "version": 1,
                "text": "# Notes\n",
            },
        },
    }));

    client.send(json!({
        "jsonrpc": "2.0",
        "method": "textDocument/didOpen",
        "params": {
            "textDocument": {
                "uri": "file:///tours/example.tour.md",
                "languageId": "Tour",
                "version": 1,
                "text": "# Tour\n\n## 1. Entry\n[entry](src/main.rs#L1:1-L1:5)\n",
            },
        },
    }));

    client.send(json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "shutdown",
        "params": null,
    }));

    let messages = client.receive_until_response(2);
    let methods: Vec<_> = messages
        .iter()
        .map(|message| message["method"].as_str().expect("server request method"))
        .collect();
    assert_eq!(
        methods,
        vec!["workspace/inlayHint/refresh", "workspace/codeLens/refresh"]
    );

    client.send(json!({
        "jsonrpc": "2.0",
        "method": "exit",
        "params": null,
    }));
    assert!(client.wait_with_timeout().success());
}

fn open_source(client: &mut LspClient, uri: &str, text: &str) {
    client.send(json!({
        "jsonrpc": "2.0",
        "method": "textDocument/didOpen",
        "params": {
            "textDocument": {
                "uri": uri,
                "languageId": "Rust",
                "version": 1,
                "text": text,
            },
        },
    }));
}

fn open_navigation_sources(client: &mut LspClient) {
    open_source(
        client,
        "file:///tours/src/main.rs",
        "line one long enough\n",
    );
    open_source(client, "file:///tours/src/router.rs", "a\nlong enough\n");
}

fn open_tour(client: &mut LspClient, text: &str) {
    client.send(json!({
        "jsonrpc": "2.0",
        "method": "textDocument/didOpen",
        "params": {
            "textDocument": {
                "uri": "file:///tours/example.tour.md",
                "languageId": "Tour",
                "version": 1,
                "text": text,
            },
        },
    }));
}

const TWO_STEP_TOUR: &str = "# Tour\n\n## 1. Entry\n[entry](src/main.rs#L1:1-L1:5)\n\n## 2. Router\n[router](src/router.rs#L2:1-L2:4)\n";
const THREAD_TOUR: &str = "# Tour\n\n[Back to the thread](zed:///agent/thread/session-1?name=Auth)\n\n## 1. Entry\n[entry](src/main.rs#L1:1-L1:5)\n";

#[test]
fn auto_start_always_exposes_the_active_start_lens() {
    let mut client = LspClient::start();
    client.initialize_with_auto_start("always");
    open_navigation_sources(&mut client);
    open_tour(&mut client, TWO_STEP_TOUR);

    client.send(json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "textDocument/codeLens",
        "params": { "textDocument": { "uri": "file:///tours/example.tour.md" } },
    }));

    let (messages, response) = client.receive_until_result(2);
    let methods: Vec<_> = messages
        .iter()
        .map(|message| message["method"].as_str().expect("server request method"))
        .collect();
    assert_eq!(
        methods,
        vec![
            "window/showDocument",
            "workspace/inlayHint/refresh",
            "workspace/codeLens/refresh",
        ]
    );

    let lenses = response["result"].as_array().expect("code lens array");
    let titles: Vec<_> = lenses
        .iter()
        .map(|lens| lens["command"]["title"].as_str().expect("lens title"))
        .collect();
    assert_eq!(titles, vec!["■ Stop tour", "⟳ Restart | 1/2"]);

    client.send(json!({
        "jsonrpc": "2.0",
        "id": 3,
        "method": "shutdown",
        "params": null,
    }));
    let shutdown = client.receive();
    assert_eq!(shutdown["id"], 3);
    client.send(json!({
        "jsonrpc": "2.0",
        "method": "exit",
        "params": null,
    }));
    assert!(client.wait_with_timeout().success());
}

#[test]
fn answers_inlay_hint_requests_without_an_active_tour() {
    let mut client = LspClient::start();
    client.initialize();

    client.send(json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "textDocument/inlayHint",
        "params": {
            "textDocument": {
                "uri": "file:///tours/src/main.rs",
            },
            "range": {
                "start": { "line": 0, "character": 0 },
                "end": { "line": 100, "character": 0 },
            },
        },
    }));

    let response = client.receive();
    assert_eq!(response["id"], 2);
    assert_eq!(response["result"], Value::Null);

    client.send(json!({
        "jsonrpc": "2.0",
        "id": 3,
        "method": "shutdown",
        "params": null,
    }));
    let shutdown_response = client.receive();
    assert_eq!(shutdown_response["id"], 3);

    client.send(json!({
        "jsonrpc": "2.0",
        "method": "exit",
        "params": null,
    }));
    assert!(client.wait_with_timeout().success());
}

#[test]
fn returns_the_body_hint_for_the_active_step() {
    let mut client = LspClient::start();
    client.initialize_with_auto_start("always");
    open_navigation_sources(&mut client);
    open_tour(
        &mut client,
        "# Tour\n\n## 1. Entry\n[entry](src/main.rs#L1:1-L1:5)\n\nBody text.\n",
    );

    client.send(json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "textDocument/inlayHint",
        "params": {
            "textDocument": {
                "uri": "file:///tours/src/main.rs",
            },
            "range": {
                "start": { "line": 0, "character": 0 },
                "end": { "line": 100, "character": 0 },
            },
        },
    }));

    let (_, response) = client.receive_until_result(2);
    let hints = response["result"].as_array().expect("inlay hint array");
    assert_eq!(hints.len(), 1);
    assert_eq!(hints[0]["position"], json!({ "line": 1, "character": 0 }));
    assert_eq!(hints[0]["paddingLeft"], json!(true));
    assert!(hints[0].get("kind").is_none());
    assert_eq!(hints[0]["label"], json!(" ▎ Body text.\n"));

    client.send(json!({
        "jsonrpc": "2.0",
        "id": 3,
        "method": "shutdown",
        "params": null,
    }));
    let shutdown = client.receive();
    assert_eq!(shutdown["id"], 3);
    client.send(json!({
        "jsonrpc": "2.0",
        "method": "exit",
        "params": null,
    }));
    assert!(client.wait_with_timeout().success());
}

#[test]
fn auto_start_prompt_starts_after_confirmation() {
    let mut client = LspClient::start();
    client.initialize_with_auto_start("prompt");
    open_navigation_sources(&mut client);
    open_tour(&mut client, TWO_STEP_TOUR);

    let prompt = client.receive();
    assert_eq!(prompt["method"], "window/showMessageRequest");
    let actions: Vec<_> = prompt["params"]["actions"]
        .as_array()
        .expect("prompt actions")
        .iter()
        .map(|action| action["title"].as_str().expect("action title"))
        .collect();
    assert_eq!(actions, vec!["Start tour", "Not now"]);

    let initial = [client.receive(), client.receive()];
    let initial_methods: Vec<_> = initial
        .iter()
        .map(|message| message["method"].as_str().expect("server request method"))
        .collect();
    assert_eq!(
        initial_methods,
        vec!["workspace/inlayHint/refresh", "workspace/codeLens/refresh"]
    );

    client.send(json!({
        "jsonrpc": "2.0",
        "id": prompt["id"].clone(),
        "result": { "title": "Start tour" },
    }));

    let after_start = [client.receive(), client.receive(), client.receive()];
    let methods: Vec<_> = after_start
        .iter()
        .map(|message| message["method"].as_str().expect("server request method"))
        .collect();
    assert_eq!(
        methods,
        vec![
            "window/showDocument",
            "workspace/inlayHint/refresh",
            "workspace/codeLens/refresh",
        ]
    );

    client.send(json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "textDocument/codeLens",
        "params": { "textDocument": { "uri": "file:///tours/example.tour.md" } },
    }));
    let (_, response) = client.receive_until_result(2);
    let lenses = response["result"].as_array().expect("code lens array");
    let titles: Vec<_> = lenses
        .iter()
        .map(|lens| lens["command"]["title"].as_str().expect("lens title"))
        .collect();
    assert_eq!(titles, vec!["■ Stop tour", "⟳ Restart | 1/2"]);

    client.send(json!({
        "jsonrpc": "2.0",
        "id": 3,
        "method": "shutdown",
        "params": null,
    }));
    let shutdown = client.receive();
    assert_eq!(shutdown["id"], 3);
    client.send(json!({
        "jsonrpc": "2.0",
        "method": "exit",
        "params": null,
    }));
    assert!(client.wait_with_timeout().success());
}

#[test]
fn navigation_commands_select_exact_utf16_ranges() {
    let mut client = LspClient::start();
    client.initialize_with_auto_start("never");

    client.send(json!({
        "jsonrpc": "2.0",
        "method": "textDocument/didOpen",
        "params": {
            "textDocument": {
                "uri": "file:///tours/src/main.rs",
                "languageId": "Rust",
                "version": 1,
                "text": "let 😀 = 1;\n",
            },
        },
    }));

    client.send(json!({
        "jsonrpc": "2.0",
        "method": "textDocument/didOpen",
        "params": {
            "textDocument": {
                "uri": "file:///tours/example.tour.md",
                "languageId": "Tour",
                "version": 1,
                "text": "# Tour\n\n## 1. Emoji\n[emoji](src/main.rs#L1:5-L1:6)\n",
            },
        },
    }));

    client.send(json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "workspace/executeCommand",
        "params": {
            "command": "tour.start",
            "arguments": [],
        },
    }));

    client.send(json!({
        "jsonrpc": "2.0",
        "id": 3,
        "method": "shutdown",
        "params": null,
    }));

    let messages = client.receive_until_response(3);
    let execute_response = messages
        .iter()
        .find(|message| message["id"] == json!(2))
        .expect("missing executeCommand response");
    assert_eq!(execute_response["result"], Value::Null);

    let show_document = messages
        .iter()
        .find(|message| message["method"] == json!("window/showDocument"))
        .expect("missing window/showDocument request");
    assert_eq!(
        show_document["params"],
        json!({
            "uri": "file:///tours/src/main.rs",
            "external": false,
            "takeFocus": true,
            "selection": {
                "start": {
                    "line": 0,
                    "character": 4,
                },
                "end": {
                    "line": 0,
                    "character": 6,
                },
            },
        })
    );

    let methods: Vec<_> = messages
        .iter()
        .filter_map(|message| message["method"].as_str())
        .collect();
    assert_eq!(
        methods,
        vec![
            "workspace/inlayHint/refresh",
            "workspace/codeLens/refresh",
            "window/showDocument",
            "workspace/inlayHint/refresh",
            "workspace/codeLens/refresh",
        ]
    );

    client.send(json!({
        "jsonrpc": "2.0",
        "method": "exit",
        "params": null,
    }));
    assert!(client.wait_with_timeout().success());
}

#[test]
fn back_to_thread_focuses_the_link_and_reports_the_fallback() {
    let mut client = LspClient::start();
    client.initialize_with_auto_start("never");
    open_navigation_sources(&mut client);
    open_tour(&mut client, THREAD_TOUR);

    client.send(json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "workspace/executeCommand",
        "params": {
            "command": "tour.backToThread",
            "arguments": [],
        },
    }));

    client.send(json!({
        "jsonrpc": "2.0",
        "id": 3,
        "method": "shutdown",
        "params": null,
    }));

    let messages = client.receive_until_response(3);
    let execute_response = messages
        .iter()
        .find(|message| message["id"] == json!(2))
        .expect("missing executeCommand response");
    assert_eq!(execute_response["result"], Value::Null);

    let show_document = messages
        .iter()
        .find(|message| message["method"] == json!("window/showDocument"))
        .expect("missing showDocument request");
    assert_eq!(
        show_document["params"],
        json!({
            "uri": "file:///tours/example.tour.md",
            "external": false,
            "takeFocus": true,
            "selection": {
                "start": { "line": 2, "character": 0 },
                "end": { "line": 2, "character": 0 },
            },
        })
    );

    let message = messages
        .iter()
        .find(|message| message["method"] == json!("window/showMessage"))
        .expect("missing fallback message");
    assert_eq!(message["params"]["type"], json!(3));
    let text = message["params"]["message"]
        .as_str()
        .expect("message should be text");
    assert!(
        text.contains("does not route zed:///agent/thread links"),
        "{text}"
    );
    assert!(text.contains("session-1"), "{text}");

    let show_document_index = messages
        .iter()
        .position(|message| message["method"] == json!("window/showDocument"))
        .expect("missing showDocument request");
    assert!(
        messages[show_document_index..]
            .iter()
            .all(|message| !message["method"]
                .as_str()
                .is_some_and(|method| method.contains("/refresh"))),
        "backToThread should not refresh overlays"
    );

    client.send(json!({
        "jsonrpc": "2.0",
        "method": "exit",
        "params": null,
    }));
    assert!(client.wait_with_timeout().success());
}
