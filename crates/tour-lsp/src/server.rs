use anyhow::{Context, Result};
use crossbeam_channel::Sender;
use lsp_server::{ErrorCode, Message, Notification, Request, RequestId, Response};
use lsp_types::{
    CodeLens, CodeLensOptions, CodeLensParams, Command, DidChangeTextDocumentParams,
    DidOpenTextDocumentParams, ExecuteCommandOptions, ExecuteCommandParams, InitializeResult,
    MessageActionItem, MessageType, OneOf, Position, Range, ServerCapabilities, ServerInfo,
    ShowMessageRequestParams, TextDocumentSyncCapability, TextDocumentSyncKind,
};
use serde_json::json;

use crate::{
    settings::{AutoStart, TourSettings},
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
const SHOW_MESSAGE_REQUEST: &str = "window/showMessageRequest";
const START_TOUR_ACTION: &str = "Start tour";
const NOT_NOW_ACTION: &str = "Not now";

const INACTIVE_START_COMMAND: &str = "tour.start";
const STOP_COMMAND: &str = "tour.stop";

/// LSP request handlers and the single active tour state.
pub struct TourServer {
    settings: TourSettings,
    state: TourState,
    next_request_id: u64,
    pending_start_prompt: Option<RequestId>,
}

impl TourServer {
    pub fn new(settings: TourSettings) -> Self {
        Self {
            settings,
            state: TourState::default(),
            next_request_id: 0,
            pending_start_prompt: None,
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

    /// Handle a client request and return its response.
    pub fn handle_request(&mut self, request: &Request, sender: &Sender<Message>) -> Response {
        match request.method.as_str() {
            "textDocument/codeLens" => {
                match serde_json::from_value::<CodeLensParams>(request.params.clone()) {
                    Ok(params) => {
                        Response::new_ok(request.id.clone(), self.code_lenses_for(&params))
                    }
                    Err(error) => invalid_params(
                        request,
                        format!("invalid textDocument/codeLens params: {error}"),
                    ),
                }
            }
            "workspace/executeCommand" => {
                match serde_json::from_value::<ExecuteCommandParams>(request.params.clone()) {
                    Ok(params) => match self.execute_command(&params, sender) {
                        Ok(()) => Response::new_ok(request.id.clone(), json!(null)),
                        Err(error) => Response::new_err(
                            request.id.clone(),
                            ErrorCode::InternalError as i32,
                            format!("failed to execute tour command: {error:#}"),
                        ),
                    },
                    Err(error) => invalid_params(
                        request,
                        format!("invalid workspace/executeCommand params: {error}"),
                    ),
                }
            }
            _ => Response::new_err(
                request.id.clone(),
                ErrorCode::MethodNotFound as i32,
                format!("unsupported request: {}", request.method),
            ),
        }
    }

    /// Handle a response to a request sent by the server.
    pub fn handle_response(&mut self, response: &Response, sender: &Sender<Message>) -> Result<()> {
        if self.pending_start_prompt.as_ref() != Some(&response.id) {
            return Ok(());
        }
        self.pending_start_prompt = None;

        if response_action_title(response) == Some(START_TOUR_ACTION) && self.start_active_tour() {
            self.refresh_all(sender)?;
        }
        Ok(())
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

        self.pending_start_prompt = None;
        match self.state.open_tour(document.uri, &document.text) {
            Ok(()) => self.apply_auto_start(sender)?,
            Err(error) => eprintln!("{SERVER_NAME}: failed to open tour: {error}"),
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
        if change.range.is_some() {
            eprintln!(
                "{SERVER_NAME}: ignoring incremental change for {}; full sync is advertised",
                params.text_document.uri.as_str()
            );
            return Ok(());
        }

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

    fn code_lenses_for(&self, params: &CodeLensParams) -> Vec<CodeLens> {
        let Some(active) = self.state.active() else {
            return Vec::new();
        };
        if active.tour_uri != params.text_document.uri {
            return Vec::new();
        }

        let range = Range::new(Position::new(0, 0), Position::new(0, 0));
        if active.running {
            let step_count = active.step_count();
            let current_step = if step_count == 0 {
                0
            } else {
                active.current_step + 1
            };
            vec![
                code_lens(range, "■ Stop tour", STOP_COMMAND),
                code_lens(
                    range,
                    format!("⟳ Restart | {current_step}/{step_count}"),
                    INACTIVE_START_COMMAND,
                ),
            ]
        } else {
            vec![code_lens(
                range,
                format!("▶ Start tour ({} steps)", active.step_count()),
                INACTIVE_START_COMMAND,
            )]
        }
    }

    fn execute_command(
        &mut self,
        params: &ExecuteCommandParams,
        sender: &Sender<Message>,
    ) -> Result<()> {
        match params.command.as_str() {
            "tour.start" if self.start_active_tour() => self.refresh_all(sender)?,
            "tour.stop" if self.stop_active_tour() => self.refresh_all(sender)?,
            _ => {}
        }
        Ok(())
    }

    fn apply_auto_start(&mut self, sender: &Sender<Message>) -> Result<()> {
        match self.settings.auto_start {
            AutoStart::Always => {
                self.start_active_tour();
            }
            AutoStart::Prompt => self.send_start_prompt(sender)?,
            AutoStart::Never => {}
        }
        Ok(())
    }

    fn send_start_prompt(&mut self, sender: &Sender<Message>) -> Result<()> {
        let title = match self.state.active() {
            Some(active) => active.tour.title.clone(),
            None => return Ok(()),
        };

        self.next_request_id += 1;
        let id = RequestId::from(format!("tour-start-prompt-{}", self.next_request_id));
        let params = ShowMessageRequestParams {
            typ: MessageType::INFO,
            message: format!("Start tour \"{title}\"?"),
            actions: Some(vec![
                MessageActionItem {
                    title: START_TOUR_ACTION.to_owned(),
                    properties: Default::default(),
                },
                MessageActionItem {
                    title: NOT_NOW_ACTION.to_owned(),
                    properties: Default::default(),
                },
            ]),
        };

        self.pending_start_prompt = Some(id.clone());
        sender.send(Message::Request(Request::new(
            id,
            SHOW_MESSAGE_REQUEST.to_owned(),
            params,
        )))?;
        Ok(())
    }

    fn start_active_tour(&mut self) -> bool {
        let Some(active) = self.state.active_mut() else {
            return false;
        };
        active.running = true;
        if active.step_count() > 0 {
            active.set_current_step(0);
        }
        true
    }

    fn stop_active_tour(&mut self) -> bool {
        let Some(active) = self.state.active_mut() else {
            return false;
        };
        if !active.running {
            return false;
        }
        active.running = false;
        true
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
        self.next_request_id += 1;
        let request = Request::new(
            RequestId::from(format!("tour-refresh-{}", self.next_request_id)),
            method.to_owned(),
            json!(null),
        );
        sender.send(Message::Request(request))?;
        Ok(())
    }
}

fn code_lens(range: Range, title: impl Into<String>, command: &str) -> CodeLens {
    let title = title.into();
    CodeLens {
        range,
        command: Some(Command::new(title.clone(), command.to_owned(), None)),
        data: None,
    }
}

fn response_action_title(response: &Response) -> Option<&str> {
    response
        .response_result
        .as_ref()
        .ok()?
        .get("title")?
        .as_str()
}

fn invalid_params(request: &Request, message: String) -> Response {
    Response::new_err(request.id.clone(), ErrorCode::InvalidParams as i32, message)
}

#[cfg(test)]
mod tests {
    use lsp_server::{Message, Request, RequestId, Response};
    use lsp_types::{
        Position, Range, ShowMessageRequestParams, TextDocumentContentChangeEvent,
        TextDocumentItem, VersionedTextDocumentIdentifier,
    };
    use serde_json::json;

    use super::{TOUR_COMMANDS, TourServer};
    use crate::settings::{AutoStart, TourSettings};
    use crate::state::UpdateOutcome;

    const TOUR: &str = r#"# Tour

## 1. Entry
[entry](src/main.rs#L2:3-L2:9)
"#;
    const TWO_STEP_TOUR: &str = r#"# Tour

## 1. Entry
[entry](src/main.rs#L2:3-L2:9)

## 2. Router
[router](src/router.rs#L4:1-L4:4)
"#;

    fn settings(auto_start: AutoStart) -> TourSettings {
        TourSettings {
            auto_start,
            ..TourSettings::default()
        }
    }

    fn code_lens_request(id: i32, uri: &str) -> Request {
        Request::new(
            RequestId::from(id),
            "textDocument/codeLens".to_owned(),
            json!({ "textDocument": { "uri": uri } }),
        )
    }

    fn execute_command_request(id: i32, command: &str) -> Request {
        Request::new(
            RequestId::from(id),
            "workspace/executeCommand".to_owned(),
            json!({ "command": command }),
        )
    }

    fn request_methods(messages: impl IntoIterator<Item = Message>) -> Vec<String> {
        messages
            .into_iter()
            .map(|message| match message {
                Message::Request(request) => request.method,
                other => panic!("unexpected message: {other:?}"),
            })
            .collect()
    }

    fn code_lens_titles(response: &Response) -> Vec<String> {
        response
            .response_result
            .as_ref()
            .expect("code lens response should be ok")
            .as_array()
            .expect("code lens result should be an array")
            .iter()
            .map(|lens| {
                lens["command"]["title"]
                    .as_str()
                    .expect("code lens should have a command title")
                    .to_owned()
            })
            .collect()
    }

    fn code_lens_commands(response: &Response) -> Vec<String> {
        response
            .response_result
            .as_ref()
            .expect("code lens response should be ok")
            .as_array()
            .expect("code lens result should be an array")
            .iter()
            .map(|lens| {
                lens["command"]["command"]
                    .as_str()
                    .expect("code lens should have a command")
                    .to_owned()
            })
            .collect()
    }

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
        did_change_notification_with_range(uri, text, None)
    }

    fn did_change_notification_with_range(
        uri: &str,
        text: &str,
        range: Option<Range>,
    ) -> lsp_server::Notification {
        lsp_server::Notification::new(
            "textDocument/didChange".to_owned(),
            json!({
                "textDocument": VersionedTextDocumentIdentifier {
                    uri: uri.parse().expect("valid URI"),
                    version: 2,
                },
                "contentChanges": [TextDocumentContentChangeEvent {
                    range,
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
        let mut server = TourServer::new(settings(AutoStart::Never));

        server
            .handle_notification(
                &did_open_notification("file:///tours/example.tour.md", TOUR),
                &sender,
            )
            .expect("notification should be handled");

        let active = server.state().active().expect("tour should be active");
        assert_eq!(active.current_step, 0);
        assert!(!active.running);
        assert_eq!(
            active.source_uri.as_ref().map(|uri| uri.as_str()),
            Some("file:///tours/src/main.rs")
        );
        assert_eq!(
            active.source_range,
            Some(Range::new(Position::new(1, 2), Position::new(1, 8)))
        );

        assert_eq!(
            request_methods(receiver.try_iter()),
            vec![
                "workspace/inlayHint/refresh".to_owned(),
                "workspace/codeLens/refresh".to_owned(),
            ]
        );
    }

    #[test]
    fn inactive_tour_lens_has_start_label_and_command() {
        let (sender, _receiver) = crossbeam_channel::unbounded();
        let mut server = TourServer::new(settings(AutoStart::Never));
        let tour_uri = "file:///tours/example.tour.md";

        server
            .handle_notification(&did_open_notification(tour_uri, TOUR), &sender)
            .expect("notification should be handled");

        let response = server.handle_request(&code_lens_request(1, tour_uri), &sender);

        assert_eq!(code_lens_titles(&response), vec!["▶ Start tour (1 steps)"]);
        assert_eq!(code_lens_commands(&response), vec!["tour.start"]);
        assert_eq!(
            response.response_result.as_ref().expect("ok")[0]["range"],
            json!({
                "start": { "line": 0, "character": 0 },
                "end": { "line": 0, "character": 0 },
            })
        );
    }

    #[test]
    fn code_lenses_only_apply_to_the_active_tour_document() {
        let (sender, _receiver) = crossbeam_channel::unbounded();
        let mut server = TourServer::new(settings(AutoStart::Never));

        let empty = server.handle_request(
            &code_lens_request(1, "file:///tours/example.tour.md"),
            &sender,
        );
        assert_eq!(empty.response_result.as_ref().expect("ok"), &json!([]));

        server
            .handle_notification(
                &did_open_notification("file:///tours/example.tour.md", TOUR),
                &sender,
            )
            .expect("notification should be handled");

        let source =
            server.handle_request(&code_lens_request(2, "file:///tours/src/main.rs"), &sender);
        assert_eq!(source.response_result.as_ref().expect("ok"), &json!([]));

        let other_tour = server.handle_request(
            &code_lens_request(3, "file:///tours/other.tour.md"),
            &sender,
        );
        assert_eq!(other_tour.response_result.as_ref().expect("ok"), &json!([]));
    }

    #[test]
    fn start_stop_and_restart_transition_state_refresh_and_update_labels() {
        let (sender, receiver) = crossbeam_channel::unbounded();
        let mut server = TourServer::new(settings(AutoStart::Never));
        let tour_uri = "file:///tours/example.tour.md";

        server
            .handle_notification(&did_open_notification(tour_uri, TWO_STEP_TOUR), &sender)
            .expect("notification should be handled");
        let _ = receiver.try_iter().count();

        let response = server.handle_request(&execute_command_request(1, "tour.start"), &sender);
        assert!(response.response_result.is_ok());
        let active = server.state().active().expect("tour is active");
        assert!(active.running);
        assert_eq!(active.current_step, 0);
        assert_eq!(
            request_methods(receiver.try_iter()),
            vec![
                "workspace/inlayHint/refresh".to_owned(),
                "workspace/codeLens/refresh".to_owned(),
            ]
        );

        let active_lens = server.handle_request(&code_lens_request(2, tour_uri), &sender);
        assert_eq!(
            code_lens_titles(&active_lens),
            vec!["■ Stop tour", "⟳ Restart | 1/2"]
        );
        assert_eq!(
            code_lens_titles(&active_lens).join(" | "),
            "■ Stop tour | ⟳ Restart | 1/2"
        );
        assert_eq!(
            code_lens_commands(&active_lens),
            vec!["tour.stop", "tour.start"]
        );

        server
            .state
            .active_mut()
            .expect("tour is active")
            .set_current_step(1);
        let second_step_lens = server.handle_request(&code_lens_request(3, tour_uri), &sender);
        assert_eq!(
            code_lens_titles(&second_step_lens),
            vec!["■ Stop tour", "⟳ Restart | 2/2"]
        );

        let restart = server.handle_request(&execute_command_request(4, "tour.start"), &sender);
        assert!(restart.response_result.is_ok());
        let active = server.state().active().expect("tour is active");
        assert!(active.running);
        assert_eq!(active.current_step, 0);
        assert_eq!(receiver.try_iter().count(), 2);

        let stop = server.handle_request(&execute_command_request(5, "tour.stop"), &sender);
        assert!(stop.response_result.is_ok());
        assert!(!server.state().active().expect("tour is active").running);
        assert_eq!(receiver.try_iter().count(), 2);
        assert_eq!(
            code_lens_titles(&server.handle_request(&code_lens_request(6, tour_uri), &sender)),
            vec!["▶ Start tour (2 steps)"]
        );
    }

    #[test]
    fn stop_when_inactive_does_not_refresh() {
        let (sender, receiver) = crossbeam_channel::unbounded();
        let mut server = TourServer::new(settings(AutoStart::Never));

        server
            .handle_notification(
                &did_open_notification("file:///tours/example.tour.md", TOUR),
                &sender,
            )
            .expect("notification should be handled");
        let _ = receiver.try_iter().count();

        let response = server.handle_request(&execute_command_request(1, "tour.stop"), &sender);
        assert!(response.response_result.is_ok());
        assert!(!server.state().active().expect("tour is active").running);
        assert!(receiver.try_recv().is_err());
    }

    #[test]
    fn auto_start_always_starts_at_step_zero_without_a_prompt() {
        let (sender, receiver) = crossbeam_channel::unbounded();
        let mut server = TourServer::new(settings(AutoStart::Always));

        server
            .handle_notification(
                &did_open_notification("file:///tours/example.tour.md", TWO_STEP_TOUR),
                &sender,
            )
            .expect("notification should be handled");

        let active = server.state().active().expect("tour is active");
        assert!(active.running);
        assert_eq!(active.current_step, 0);
        assert_eq!(
            request_methods(receiver.try_iter()),
            vec![
                "workspace/inlayHint/refresh".to_owned(),
                "workspace/codeLens/refresh".to_owned(),
            ]
        );
    }

    #[test]
    fn auto_start_never_waits_for_the_start_lens() {
        let (sender, receiver) = crossbeam_channel::unbounded();
        let mut server = TourServer::new(settings(AutoStart::Never));

        server
            .handle_notification(
                &did_open_notification("file:///tours/example.tour.md", TOUR),
                &sender,
            )
            .expect("notification should be handled");

        assert!(!server.state().active().expect("tour is active").running);
        assert_eq!(
            request_methods(receiver.try_iter()),
            vec![
                "workspace/inlayHint/refresh".to_owned(),
                "workspace/codeLens/refresh".to_owned(),
            ]
        );
    }

    #[test]
    fn auto_start_prompt_starts_only_after_confirmation() {
        let (sender, receiver) = crossbeam_channel::unbounded();
        let mut server = TourServer::new(settings(AutoStart::Prompt));

        server
            .handle_notification(
                &did_open_notification("file:///tours/example.tour.md", TWO_STEP_TOUR),
                &sender,
            )
            .expect("notification should be handled");
        assert!(!server.state().active().expect("tour is active").running);

        let messages: Vec<_> = receiver.try_iter().collect();
        assert_eq!(messages.len(), 3);
        let prompt = match &messages[0] {
            Message::Request(request) => request,
            other => panic!("expected a prompt request, got {other:?}"),
        };
        assert_eq!(prompt.method, "window/showMessageRequest");
        let params: ShowMessageRequestParams =
            serde_json::from_value(prompt.params.clone()).expect("valid prompt params");
        assert!(params.message.contains("Tour"));
        assert_eq!(
            params
                .actions
                .expect("prompt actions")
                .into_iter()
                .map(|action| action.title)
                .collect::<Vec<_>>(),
            vec!["Start tour".to_owned(), "Not now".to_owned()]
        );
        assert_eq!(
            request_methods(messages[1..].iter().cloned()),
            vec![
                "workspace/inlayHint/refresh".to_owned(),
                "workspace/codeLens/refresh".to_owned(),
            ]
        );

        server
            .handle_response(
                &Response::new_ok(prompt.id.clone(), json!({ "title": "Start tour" })),
                &sender,
            )
            .expect("prompt response should be handled");

        let active = server.state().active().expect("tour is active");
        assert!(active.running);
        assert_eq!(active.current_step, 0);
        assert_eq!(
            request_methods(receiver.try_iter()),
            vec![
                "workspace/inlayHint/refresh".to_owned(),
                "workspace/codeLens/refresh".to_owned(),
            ]
        );
    }

    #[test]
    fn prompt_dismissal_and_other_responses_leave_the_tour_inactive() {
        for response in [
            Response::new_ok(RequestId::from(1), json!({ "title": "Not now" })),
            Response::new_ok(RequestId::from(1), json!(null)),
        ] {
            let (sender, receiver) = crossbeam_channel::unbounded();
            let mut server = TourServer::new(settings(AutoStart::Prompt));

            server
                .handle_notification(
                    &did_open_notification("file:///tours/example.tour.md", TWO_STEP_TOUR),
                    &sender,
                )
                .expect("notification should be handled");
            let prompt = match receiver.try_recv().expect("prompt request") {
                Message::Request(request) => request,
                other => panic!("expected a prompt request, got {other:?}"),
            };
            let _ = receiver.try_iter().count();

            let mut response = response;
            response.id = prompt.id;
            server
                .handle_response(&response, &sender)
                .expect("prompt response should be handled");

            let active = server.state().active().expect("tour is still active");
            assert!(!active.running);
            assert_eq!(active.current_step, 0);
            assert_eq!(active.tour.title, "Tour");
            assert_eq!(active.tour.steps.len(), 2);
            assert!(receiver.try_recv().is_err());
        }
    }

    #[test]
    fn stale_prompt_responses_are_ignored() {
        let (sender, receiver) = crossbeam_channel::unbounded();
        let mut server = TourServer::new(settings(AutoStart::Prompt));

        server
            .handle_notification(
                &did_open_notification("file:///tours/example.tour.md", TOUR),
                &sender,
            )
            .expect("notification should be handled");
        let prompt = match receiver.try_recv().expect("prompt request") {
            Message::Request(request) => request,
            other => panic!("expected a prompt request, got {other:?}"),
        };
        let _ = receiver.try_iter().count();

        server
            .handle_response(
                &Response::new_ok(
                    RequestId::from(format!("{}-stale", prompt.id)),
                    json!({ "title": "Start tour" }),
                ),
                &sender,
            )
            .expect("stale response should be ignored");

        assert!(!server.state().active().expect("tour is active").running);
        assert!(receiver.try_recv().is_err());
    }

    #[test]
    fn ignores_incremental_changes_and_recovers_after_a_bad_full_change() {
        let (sender, receiver) = crossbeam_channel::unbounded();
        let mut server = TourServer::new(TourSettings::default());

        server
            .handle_notification(
                &did_open_notification("file:///tours/example.tour.md", TOUR),
                &sender,
            )
            .expect("notification should be handled");
        let _ = receiver.try_iter().count();

        server
            .handle_notification(
                &did_change_notification_with_range(
                    "file:///tours/example.tour.md",
                    "incremental",
                    Some(Range::new(Position::new(0, 0), Position::new(0, 1))),
                ),
                &sender,
            )
            .expect("incremental change should be ignored");
        assert!(receiver.try_recv().is_err());

        server
            .handle_notification(
                &did_change_notification(
                    "file:///tours/example.tour.md",
                    "# Bad\n\n## 1. Missing link\n",
                ),
                &sender,
            )
            .expect("bad change should be handled");
        assert_eq!(
            receiver.try_iter().count(),
            2,
            "a failed update still refreshes the kept tour's overlays"
        );
        assert_eq!(
            server
                .state()
                .active()
                .map(|active| active.tour.title.as_str()),
            Some("Tour"),
            "the last valid tour should survive a transient parse failure"
        );

        server
            .handle_notification(
                &did_change_notification("file:///tours/example.tour.md", TOUR),
                &sender,
            )
            .expect("valid change should recover");
        assert_eq!(
            receiver.try_iter().count(),
            2,
            "a recovered update refreshes hints and lenses"
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
