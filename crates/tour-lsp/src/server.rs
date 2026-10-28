use anyhow::{Context, Result};
use crossbeam_channel::Sender;
use lsp_server::{Message, Notification, Request, RequestId};
use lsp_types::{
    CodeLensOptions, DidChangeTextDocumentParams, DidOpenTextDocumentParams, ExecuteCommandOptions,
    InitializeResult, OneOf, ServerCapabilities, ServerInfo, TextDocumentSyncCapability,
    TextDocumentSyncKind,
};
use serde_json::json;

use crate::{
    settings::TourSettings,
    state::{TourState, UpdateOutcome},
    uri::is_tour_document,
};

pub const SERVER_NAME: &str = "tour-lsp";

pub const TOUR_COMMANDS: &[&str] = &[
    "tour.start",
    "tour.stop",
    "tour.next",
    "tour.previous",
    "tour.goto",
    "tour.overview",
    "tour.backToThread",
];

const INLAY_HINT_REFRESH: &str = "workspace/inlayHint/refresh";
const CODE_LENS_REFRESH: &str = "workspace/codeLens/refresh";

/// LSP request handlers and the single active tour state.
pub struct TourServer {
    settings: TourSettings,
    state: TourState,
    next_refresh_id: u64,
}

impl TourServer {
    pub fn new(settings: TourSettings) -> Self {
        Self {
            settings,
            state: TourState::default(),
            next_refresh_id: 0,
        }
    }

    pub fn settings(&self) -> TourSettings {
        self.settings
    }

    pub fn state(&self) -> &TourState {
        &self.state
    }

    pub fn initialize_result() -> InitializeResult {
        InitializeResult {
            capabilities: Self::capabilities(),
            server_info: Some(ServerInfo {
                name: SERVER_NAME.to_owned(),
                version: Some(env!("CARGO_PKG_VERSION").to_owned()),
            }),
        }
    }

    pub fn capabilities() -> ServerCapabilities {
        ServerCapabilities {
            text_document_sync: Some(TextDocumentSyncCapability::Kind(TextDocumentSyncKind::FULL)),
            code_lens_provider: Some(CodeLensOptions {
                resolve_provider: Some(false),
            }),
            inlay_hint_provider: Some(OneOf::Left(true)),
            execute_command_provider: Some(ExecuteCommandOptions {
                commands: TOUR_COMMANDS
                    .iter()
                    .map(|command| (*command).to_owned())
                    .collect(),
                work_done_progress_options: Default::default(),
            }),
            ..Default::default()
        }
    }

    /// Handle a notification that can change tour state.
    pub fn handle_notification(
        &mut self,
        notification: &Notification,
        sender: &Sender<Message>,
    ) -> Result<()> {
        match notification.method.as_str() {
            "textDocument/didOpen" => {
                let params: DidOpenTextDocumentParams =
                    serde_json::from_value(notification.params.clone())
                        .context("invalid textDocument/didOpen params")?;
                self.did_open(params, sender)
            }
            "textDocument/didChange" => {
                let params: DidChangeTextDocumentParams =
                    serde_json::from_value(notification.params.clone())
                        .context("invalid textDocument/didChange params")?;
                self.did_change(params, sender)
            }
            _ => Ok(()),
        }
    }

    fn did_open(
        &mut self,
        params: DidOpenTextDocumentParams,
        sender: &Sender<Message>,
    ) -> Result<()> {
        let document = params.text_document;
        if !is_tour_document(&document.uri) {
            return Ok(());
        }

        if let Err(error) = self.state.open_tour(document.uri, &document.text) {
            eprintln!("{SERVER_NAME}: failed to open tour: {error}");
        }

        self.refresh_all(sender)
    }

    fn did_change(
        &mut self,
        params: DidChangeTextDocumentParams,
        sender: &Sender<Message>,
    ) -> Result<()> {
        if !is_tour_document(&params.text_document.uri) {
            return Ok(());
        }

        let Some(change) = params.content_changes.into_iter().last() else {
            return Ok(());
        };

        match self
            .state
            .update_active_tour(&params.text_document.uri, &change.text)
        {
            Ok(UpdateOutcome::Ignored) => Ok(()),
            Ok(UpdateOutcome::Updated) => self.refresh_all(sender),
            Err(error) => {
                eprintln!("{SERVER_NAME}: failed to update active tour: {error}");
                self.refresh_all(sender)
            }
        }
    }

    /// Ask the client to recompute inlay hints.
    pub fn refresh_inlay_hints(&mut self, sender: &Sender<Message>) -> Result<()> {
        self.send_refresh(sender, INLAY_HINT_REFRESH)
    }

    /// Ask the client to recompute code lenses.
    pub fn refresh_code_lenses(&mut self, sender: &Sender<Message>) -> Result<()> {
        self.send_refresh(sender, CODE_LENS_REFRESH)
    }

    /// Refresh both hints and lenses after a state change.
    pub fn refresh_all(&mut self, sender: &Sender<Message>) -> Result<()> {
        self.refresh_inlay_hints(sender)?;
        self.refresh_code_lenses(sender)
    }

    fn send_refresh(&mut self, sender: &Sender<Message>, method: &str) -> Result<()> {
        self.next_refresh_id += 1;
        let request = Request::new(
            RequestId::from(format!("tour-refresh-{}", self.next_refresh_id)),
            method.to_owned(),
            json!(null),
        );
        sender.send(Message::Request(request))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use lsp_server::Message;
    use lsp_types::{
        Position, Range, TextDocumentContentChangeEvent, TextDocumentItem,
        VersionedTextDocumentIdentifier,
    };
    use serde_json::json;

    use super::{TOUR_COMMANDS, TourServer};
    use crate::settings::TourSettings;
    use crate::state::UpdateOutcome;

    const TOUR: &str = r#"# Tour

## 1. Entry
[entry](src/main.rs#L2:3-L2:9)
"#;

    fn did_open_notification(uri: &str, text: &str) -> lsp_server::Notification {
        lsp_server::Notification::new(
            "textDocument/didOpen".to_owned(),
            json!({
                "textDocument": TextDocumentItem {
                    uri: uri.parse().expect("valid URI"),
                    language_id: "Tour".to_owned(),
                    version: 1,
                    text: text.to_owned(),
                },
            }),
        )
    }

    fn did_change_notification(uri: &str, text: &str) -> lsp_server::Notification {
        lsp_server::Notification::new(
            "textDocument/didChange".to_owned(),
            json!({
                "textDocument": VersionedTextDocumentIdentifier {
                    uri: uri.parse().expect("valid URI"),
                    version: 2,
                },
                "contentChanges": [TextDocumentContentChangeEvent {
                    range: None,
                    range_length: None,
                    text: text.to_owned(),
                }],
            }),
        )
    }

    #[test]
    fn advertises_capabilities_and_tour_commands() {
        let capabilities = serde_json::to_value(TourServer::capabilities()).expect("capabilities");

        assert_eq!(capabilities["inlayHintProvider"], json!(true));
        assert_eq!(
            capabilities["codeLensProvider"],
            json!({ "resolveProvider": false })
        );
        assert_eq!(capabilities["textDocumentSync"], json!(1));
        assert_eq!(
            capabilities["executeCommandProvider"]["commands"],
            json!(TOUR_COMMANDS)
        );
    }

    #[test]
    fn ignores_non_tour_markdown_and_other_documents() {
        let (sender, receiver) = crossbeam_channel::unbounded();
        let mut server = TourServer::new(TourSettings::default());

        server
            .handle_notification(
                &did_open_notification("file:///tours/notes.md", TOUR),
                &sender,
            )
            .expect("notification should be handled");
        server
            .handle_notification(
                &did_open_notification("file:///tours/notes.tour.md.txt", TOUR),
                &sender,
            )
            .expect("notification should be handled");

        assert!(receiver.try_recv().is_err());
        assert!(server.state().active().is_none());
    }

    #[test]
    fn opens_tours_and_sends_both_refreshes() {
        let (sender, receiver) = crossbeam_channel::unbounded();
        let mut server = TourServer::new(TourSettings::default());

        server
            .handle_notification(
                &did_open_notification("file:///tours/example.tour.md", TOUR),
                &sender,
            )
            .expect("notification should be handled");

        let active = server.state().active().expect("tour should be active");
        assert_eq!(active.current_step, 0);
        assert_eq!(
            active.source_uri.as_ref().map(|uri| uri.as_str()),
            Some("file:///tours/src/main.rs")
        );
        assert_eq!(
            active.source_range,
            Some(Range::new(Position::new(1, 2), Position::new(1, 8)))
        );

        let methods: Vec<_> = receiver
            .try_iter()
            .map(|message| match message {
                Message::Request(request) => request.method,
                other => panic!("unexpected message: {other:?}"),
            })
            .collect();
        assert_eq!(
            methods,
            vec![
                "workspace/inlayHint/refresh".to_owned(),
                "workspace/codeLens/refresh".to_owned(),
            ]
        );
    }

    #[test]
    fn did_change_only_updates_the_active_tour() {
        let (sender, _receiver) = crossbeam_channel::unbounded();
        let mut server = TourServer::new(TourSettings::default());

        server
            .handle_notification(
                &did_open_notification("file:///tours/first.tour.md", TOUR),
                &sender,
            )
            .expect("notification should be handled");

        let outcome = server
            .state
            .update_active_tour(
                &"file:///tours/other.tour.md".parse().expect("valid URI"),
                TOUR,
            )
            .expect("inactive change should not fail");
        assert_eq!(outcome, UpdateOutcome::Ignored);

        server
            .handle_notification(
                &did_change_notification("file:///tours/first.tour.md", TOUR),
                &sender,
            )
            .expect("notification should be handled");

        assert_eq!(
            server
                .state()
                .active()
                .map(|active| active.tour_uri.as_str()),
            Some("file:///tours/first.tour.md")
        );
    }
}
