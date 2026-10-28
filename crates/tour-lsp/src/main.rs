use std::process::ExitCode;

use anyhow::Result;
use lsp_server::{Connection, ErrorCode, Message, Response};
use lsp_types::{InitializeParams, ServerCapabilities};

const SERVER_NAME: &str = "tour-lsp";

fn main() -> ExitCode {
    match run() {
        Ok(exit_code) => exit_code,
        Err(error) => {
            eprintln!("tour-lsp: {error:#}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<ExitCode> {
    let (connection, io_threads) = Connection::stdio();

    let (initialize_id, initialize_params) = connection.initialize_start()?;
    let _: InitializeParams = serde_json::from_value(initialize_params)?;

    let initialize_result = serde_json::json!({
        "capabilities": ServerCapabilities::default(),
        "serverInfo": {
            "name": SERVER_NAME,
            "version": env!("CARGO_PKG_VERSION"),
        },
    });
    connection.initialize_finish(initialize_id, initialize_result)?;

    for message in &connection.receiver {
        match message {
            Message::Request(request) => {
                if connection.handle_shutdown(&request)? {
                    break;
                }

                let response = Response::new_err(
                    request.id,
                    ErrorCode::MethodNotFound as i32,
                    format!("unsupported request: {}", request.method),
                );
                connection.sender.send(Message::Response(response))?;
            }
            Message::Notification(notification) => {
                if notification.method == "exit" {
                    eprintln!("tour-lsp: received exit before shutdown");
                    return Ok(ExitCode::FAILURE);
                }
            }
            Message::Response(_) => {}
        }
    }

    drop(connection);
    io_threads.join()?;
    Ok(ExitCode::SUCCESS)
}
