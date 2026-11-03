use std::{process::ExitCode, time::Instant};

use anyhow::Result;
use crossbeam_channel::RecvTimeoutError;
use lsp_server::{Connection, Message};
use lsp_types::InitializeParams;
use tour_lsp::{server::TourServer, settings::TourSettings};

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
    let initialize_params: InitializeParams = serde_json::from_value(initialize_params)?;
    let settings = TourSettings::from_initialization_options(
        initialize_params.initialization_options.as_ref(),
    );

    let initialize_result = serde_json::to_value(TourServer::initialize_result())?;
    connection.initialize_finish(initialize_id, initialize_result)?;

    let mut server = TourServer::new(settings);

    loop {
        let message = match server.hint_refresh_timeout() {
            Some(timeout) => match connection.receiver.recv_timeout(timeout) {
                Ok(message) => Some(message),
                Err(RecvTimeoutError::Timeout) => None,
                Err(RecvTimeoutError::Disconnected) => break,
            },
            None => match connection.receiver.recv() {
                Ok(message) => Some(message),
                Err(_) => break,
            },
        };

        match message {
            Some(Message::Request(request)) => {
                if connection.handle_shutdown(&request)? {
                    break;
                }

                let response = server.handle_request(&request, &connection.sender);
                connection.sender.send(Message::Response(response))?;
            }
            Some(Message::Notification(notification)) => {
                if notification.method == "exit" {
                    eprintln!("tour-lsp: received exit before shutdown");
                    return Ok(ExitCode::FAILURE);
                }

                if let Err(error) = server.handle_notification(&notification, &connection.sender) {
                    eprintln!("tour-lsp: failed to handle notification: {error:#}");
                }
            }
            Some(Message::Response(response)) => {
                if let Err(error) = server.handle_response(&response, &connection.sender) {
                    eprintln!("tour-lsp: failed to handle response: {error:#}");
                }
            }
            None => {}
        }

        if let Err(error) = server.flush_due_hint_refresh(Instant::now(), &connection.sender) {
            eprintln!("tour-lsp: failed to send deferred hint refresh: {error:#}");
        }
    }

    drop(connection);
    io_threads.join()?;
    Ok(ExitCode::SUCCESS)
}
