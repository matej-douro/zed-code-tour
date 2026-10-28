use anyhow::Result;
use lsp_server::{Connection, ErrorCode, Message, Response};
use lsp_types::{InitializeParams, ServerCapabilities};

const SERVER_NAME: &str = "tour-lsp";

fn main() -> Result<()> {
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
            Message::Notification(_) => {}
            Message::Response(_) => {}
        }
    }

    drop(connection);
    io_threads.join()?;
    Ok(())
}
