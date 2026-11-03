#![allow(dead_code)]

//! Fake LSP client used by the integration tests.
//!
//! Each integration test binary compiles this module separately, so helpers
//! used by one test target are dead code in the others.
//!
//! The client drives the real `tour-lsp` binary over stdio, including the
//! handshake, document notifications, requests, and the server-initiated
//! requests that the protocol assertions inspect.

use std::{
    io::{BufRead, BufReader, Read, Write},
    process::{Child, ChildStdin, ChildStdout, Command, ExitStatus, Stdio},
    sync::mpsc::{self, Receiver, RecvTimeoutError},
    thread,
    time::{Duration, Instant},
};

use serde_json::{Value, json};

const MESSAGE_TIMEOUT: Duration = Duration::from_secs(10);
const EXIT_TIMEOUT: Duration = Duration::from_secs(5);

pub struct LspClient {
    child: Child,
    stdin: Option<ChildStdin>,
    messages: Receiver<Result<Value, String>>,
    next_id: i64,
}

impl LspClient {
    pub fn start() -> Self {
        let mut child = Command::new(env!("CARGO_BIN_EXE_tour-lsp"))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .expect("failed to start tour-lsp");

        let stdin = child.stdin.take().expect("missing child stdin");
        let stdout = child.stdout.take().expect("missing child stdout");
        let (sender, messages) = mpsc::channel();

        thread::spawn(move || {
            let mut stdout = BufReader::new(stdout);
            loop {
                match read_message(&mut stdout) {
                    Ok(Some(message)) => {
                        if sender.send(Ok(message)).is_err() {
                            break;
                        }
                    }
                    Ok(None) => break,
                    Err(error) => {
                        let _ = sender.send(Err(error));
                        break;
                    }
                }
            }
        });

        Self {
            child,
            stdin: Some(stdin),
            messages,
            next_id: 1,
        }
    }

    pub fn initialize(&mut self) -> Value {
        self.initialize_with_options(None)
    }

    pub fn initialize_with_auto_start(&mut self, auto_start: &str) -> Value {
        self.initialize_with_options(Some(json!({ "auto_start": auto_start })))
    }

    pub fn initialize_with_options(&mut self, initialization_options: Option<Value>) -> Value {
        let mut params = json!({
            "processId": null,
            "rootUri": null,
            "capabilities": {},
        });
        if let Some(options) = initialization_options {
            params["initializationOptions"] = options;
        }

        let id = self.send_request("initialize", params);
        let response = self.receive();
        assert_eq!(response["id"], json!(id));
        let result = response["result"].clone();

        assert_eq!(result["capabilities"]["inlayHintProvider"], json!(true));
        assert_eq!(
            result["capabilities"]["codeLensProvider"],
            json!({ "resolveProvider": false })
        );
        assert_eq!(
            result["capabilities"]["executeCommandProvider"]["commands"],
            json!([
                "tour.start",
                "tour.resume",
                "tour.stop",
                "tour.next",
                "tour.previous",
                "tour.goto",
                "tour.overview",
                "tour.backToThread",
            ])
        );
        assert_eq!(result["serverInfo"]["name"], "tour-lsp");

        self.send_notification("initialized", json!({}));
        result
    }

    pub fn send_request(&mut self, method: &str, params: Value) -> i64 {
        let id = self.next_id;
        self.next_id += 1;
        self.send(json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params,
        }));
        id
    }

    pub fn send_notification(&mut self, method: &str, params: Value) {
        self.send(json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params,
        }));
    }

    pub fn send_response(&mut self, id: Value, result: Value) {
        self.send(json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": result,
        }));
    }

    pub fn send(&mut self, message: Value) {
        let body = serde_json::to_vec(&message).expect("failed to serialize message");
        let stdin = self.stdin.as_mut().expect("stdin is closed");
        write!(stdin, "Content-Length: {}\r\n\r\n", body.len()).expect("failed to write header");
        stdin.write_all(&body).expect("failed to write body");
        stdin.flush().expect("failed to flush message");
    }

    pub fn receive(&mut self) -> Value {
        match self.messages.recv_timeout(MESSAGE_TIMEOUT) {
            Ok(Ok(message)) => message,
            Ok(Err(error)) => panic!("failed to read server message: {error}"),
            Err(RecvTimeoutError::Timeout) => {
                panic!("tour-lsp did not send a message within {MESSAGE_TIMEOUT:?}")
            }
            Err(RecvTimeoutError::Disconnected) => {
                panic!("tour-lsp closed stdout unexpectedly")
            }
        }
    }

    pub fn receive_exact(&mut self, count: usize) -> Vec<Value> {
        (0..count).map(|_| self.receive()).collect()
    }

    pub fn receive_until_response(&mut self, id: i64) -> (Vec<Value>, Value) {
        self.receive_until_result(id)
    }

    pub fn receive_until_result(&mut self, id: i64) -> (Vec<Value>, Value) {
        let mut messages = Vec::new();

        loop {
            let message = self.receive();
            if message.get("method").is_none() && message["id"] == json!(id) {
                return (messages, message);
            }
            messages.push(message);
        }
    }

    pub fn open_document(&mut self, uri: &str, language_id: &str, text: &str) {
        self.send_notification(
            "textDocument/didOpen",
            json!({
                "textDocument": {
                    "uri": uri,
                    "languageId": language_id,
                    "version": 1,
                    "text": text,
                },
            }),
        );
    }

    pub fn open_source(&mut self, uri: &str, text: &str) {
        self.open_document(uri, "Rust", text);
    }

    pub fn open_tour(&mut self, uri: &str, text: &str) {
        self.open_document(uri, "Tour", text);
    }

    pub fn close_document(&mut self, uri: &str) {
        self.send_notification(
            "textDocument/didClose",
            json!({ "textDocument": { "uri": uri } }),
        );
    }

    pub fn execute_command(&mut self, command: &str, arguments: Vec<Value>) -> (Vec<Value>, Value) {
        let params = if arguments.is_empty() {
            json!({ "command": command })
        } else {
            json!({ "command": command, "arguments": arguments })
        };
        let id = self.send_request("workspace/executeCommand", params);
        self.receive_until_result(id)
    }

    pub fn code_lenses(&mut self, uri: &str) -> (Vec<Value>, Value) {
        let id = self.send_request(
            "textDocument/codeLens",
            json!({ "textDocument": { "uri": uri } }),
        );
        self.receive_until_result(id)
    }

    pub fn inlay_hints(&mut self, uri: &str) -> (Vec<Value>, Value) {
        let id = self.send_request(
            "textDocument/inlayHint",
            json!({
                "textDocument": { "uri": uri },
                "range": {
                    "start": { "line": 0, "character": 0 },
                    "end": { "line": 1000, "character": 0 },
                },
            }),
        );
        self.receive_until_result(id)
    }

    pub fn shutdown(&mut self) {
        let id = self.send_request("shutdown", Value::Null);
        let (_, response) = self.receive_until_result(id);
        assert_eq!(response["result"], Value::Null);
        self.send_notification("exit", Value::Null);
    }

    pub fn close_stdin(&mut self) {
        self.stdin.take();
    }

    pub fn wait_with_timeout(&mut self) -> ExitStatus {
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

    pub fn finish(mut self) {
        self.shutdown();
        let status = self.wait_with_timeout();
        assert!(status.success(), "tour-lsp exited with {status}");
    }
}

impl Drop for LspClient {
    fn drop(&mut self) {
        self.stdin.take();
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn read_message(stdout: &mut BufReader<ChildStdout>) -> Result<Option<Value>, String> {
    let mut content_length = None;
    loop {
        let mut line = String::new();
        let bytes_read = stdout
            .read_line(&mut line)
            .map_err(|error| format!("failed to read header: {error}"))?;
        if bytes_read == 0 {
            return Ok(None);
        }

        let line = line.trim_end();
        if line.is_empty() {
            break;
        }

        if let Some(value) = line.strip_prefix("Content-Length:") {
            content_length = Some(
                value
                    .trim()
                    .parse::<usize>()
                    .map_err(|error| format!("invalid Content-Length: {error}"))?,
            );
        }
    }

    let content_length =
        content_length.ok_or_else(|| "missing Content-Length header".to_owned())?;
    let mut body = vec![0; content_length];
    stdout
        .read_exact(&mut body)
        .map_err(|error| format!("failed to read message body: {error}"))?;

    serde_json::from_slice(&body)
        .map(Some)
        .map_err(|error| format!("failed to parse message body: {error}"))
}

/// Request methods sent by the server, in order.
pub fn request_methods(messages: &[Value]) -> Vec<&str> {
    messages
        .iter()
        .filter_map(|message| message["method"].as_str())
        .collect()
}

/// Server-initiated request params for the first message with `method`.
pub fn request_params<'a>(messages: &'a [Value], method: &str) -> Option<&'a Value> {
    messages
        .iter()
        .find(|message| message["method"] == json!(method))
        .map(|message| &message["params"])
}

/// Result of a successful client request.
pub fn result(response: &Value) -> &Value {
    assert!(
        response.get("error").is_none(),
        "unexpected error response: {response}"
    );
    &response["result"]
}

/// Titles from a code lens response.
pub fn lens_titles(response: &Value) -> Vec<&str> {
    result(response)
        .as_array()
        .expect("code lens result should be an array")
        .iter()
        .map(|lens| {
            lens["command"]["title"]
                .as_str()
                .expect("code lens should have a command title")
        })
        .collect()
}

/// Commands from a code lens response.
pub fn lens_commands(response: &Value) -> Vec<&str> {
    result(response)
        .as_array()
        .expect("code lens result should be an array")
        .iter()
        .map(|lens| {
            lens["command"]["command"]
                .as_str()
                .expect("code lens should have a command")
        })
        .collect()
}

/// Lines from a code lens response.
pub fn lens_lines(response: &Value) -> Vec<u64> {
    result(response)
        .as_array()
        .expect("code lens result should be an array")
        .iter()
        .map(|lens| {
            lens["range"]["start"]["line"]
                .as_u64()
                .expect("code lens should have a line")
        })
        .collect()
}

/// Labels from an inlay hint response.
pub fn hint_labels(response: &Value) -> Vec<&str> {
    match result(response) {
        Value::Null => Vec::new(),
        Value::Array(hints) => hints
            .iter()
            .map(|hint| {
                hint["label"]
                    .as_str()
                    .expect("body hint should have a string label")
            })
            .collect(),
        other => panic!("unexpected inlay hint result: {other}"),
    }
}
