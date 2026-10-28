use std::{
    io::{BufRead, BufReader, Read, Write},
    process::{Child, ChildStdin, ChildStdout, Command, Stdio},
};

use serde_json::{Value, json};

struct LspClient {
    child: Child,
    stdin: ChildStdin,
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
            stdin,
            stdout: BufReader::new(stdout),
        }
    }

    fn send(&mut self, message: Value) {
        let body = serde_json::to_vec(&message).expect("failed to serialize message");
        write!(self.stdin, "Content-Length: {}\r\n\r\n", body.len())
            .expect("failed to write header");
        self.stdin.write_all(&body).expect("failed to write body");
        self.stdin.flush().expect("failed to flush message");
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

    fn finish(mut self) {
        let status = self.child.wait().expect("failed to wait for tour-lsp");
        assert!(status.success(), "tour-lsp exited with {status}");
    }
}

#[test]
fn completes_initialize_shutdown_and_exit() {
    let mut client = LspClient::start();

    client.send(json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {
            "processId": null,
            "rootUri": null,
            "capabilities": {},
        },
    }));

    let initialize_response = client.receive();
    assert_eq!(initialize_response["id"], 1);
    assert_eq!(initialize_response["result"]["capabilities"], json!({}));
    assert_eq!(
        initialize_response["result"]["serverInfo"]["name"],
        "tour-lsp"
    );

    client.send(json!({
        "jsonrpc": "2.0",
        "method": "initialized",
        "params": {},
    }));

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

    client.finish();
}
