use std::{collections::HashMap, fmt};

use anyhow::{Context, Result};
use crossbeam_channel::Sender;
use lsp_server::{ErrorCode, Message, Notification, Request, RequestId, Response};
use lsp_types::{
    CodeLens, CodeLensOptions, CodeLensParams, Command, DidChangeConfigurationParams,
    DidChangeTextDocumentParams, DidCloseTextDocumentParams, DidOpenTextDocumentParams,
    ExecuteCommandOptions, ExecuteCommandParams, InitializeResult, InlayHint, InlayHintParams,
    MessageActionItem, MessageType, OneOf, Position, Range, ServerCapabilities, ServerInfo,
    ShowDocumentResult, ShowMessageParams, ShowMessageRequestParams, TextDocumentSyncCapability,
    TextDocumentSyncKind, Uri,
};
use serde_json::{Value, json};

use crate::{
    controls::control_lenses,
    hint::body_hint,
    navigation::{PositionError, show_document_params, tour_range_to_lsp},
    settings::{AutoStart, TourSettings},
    state::{TourState, UpdateOutcome},
    tour::TourRange,
    uri::{file_path, file_uri, is_tour_document},
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
const SHOW_DOCUMENT: &str = "window/showDocument";
const SHOW_MESSAGE: &str = "window/showMessage";
const START_TOUR_ACTION: &str = "Start tour";
const NOT_NOW_ACTION: &str = "Not now";

const INACTIVE_START_COMMAND: &str = "tour.start";
const STOP_COMMAND: &str = "tour.stop";

/// LSP request handlers and the single active tour state.
pub struct TourServer {
    settings: TourSettings,
    state: TourState,
    /// Open document text, including unsaved source buffers. Source text is
    /// used to resolve exact UTF-16 columns; tour documents are cached too so
    /// didClose removes every document uniformly.
    documents: HashMap<Uri, String>,
    next_request_id: u64,
    pending_start_prompt: Option<RequestId>,
    pending_show_documents: HashMap<RequestId, Uri>,
}

impl TourServer {
    pub fn new(settings: TourSettings) -> Self {
        Self {
            settings,
            state: TourState::default(),
            documents: HashMap::new(),
            next_request_id: 0,
            pending_start_prompt: None,
            pending_show_documents: HashMap::new(),
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
            "textDocument/inlayHint" => {
                match serde_json::from_value::<InlayHintParams>(request.params.clone()) {
                    Ok(params) => Response::new_ok(request.id.clone(), self.inlay_hints(&params)),
                    Err(error) => invalid_params(
                        request,
                        format!("invalid textDocument/inlayHint params: {error}"),
                    ),
                }
            }
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
                        Err(error) => {
                            let message = error.to_string();
                            if let Err(send_error) = self.show_error(sender, &message) {
                                eprintln!(
                                    "{SERVER_NAME}: failed to report command error to the \
                                     client: {send_error}"
                                );
                            }
                            Response::new_err(
                                request.id.clone(),
                                ErrorCode::RequestFailed as i32,
                                message,
                            )
                        }
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
        if let Some(uri) = self.pending_show_documents.remove(&response.id) {
            self.report_show_document_response(response, uri, sender)?;
        }

        if self.pending_start_prompt.as_ref() != Some(&response.id) {
            return Ok(());
        }
        self.pending_start_prompt = None;

        if response_action_title(response) == Some(START_TOUR_ACTION) {
            match self.start_active_tour(sender) {
                Ok(_) => {}
                Err(error) => self.show_error(sender, &error.to_string())?,
            }
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
            "textDocument/didClose" => {
                let params: DidCloseTextDocumentParams =
                    serde_json::from_value(notification.params.clone())
                        .context("invalid textDocument/didClose params")?;
                self.did_close(params, sender)
            }
            "workspace/didChangeConfiguration" => {
                let params: DidChangeConfigurationParams =
                    serde_json::from_value(notification.params.clone())
                        .context("invalid workspace/didChangeConfiguration params")?;
                let settings = settings_from_configuration(&params.settings, self.settings);
                if settings == self.settings {
                    return Ok(());
                }
                self.settings = settings;
                self.refresh_all(sender)
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
        let is_tour = is_tour_document(&document.uri);
        let is_active_source = self.is_active_source(&document.uri);
        self.documents
            .insert(document.uri.clone(), document.text.clone());
        if !is_tour {
            self.resolve_active_source_range();
            if is_active_source {
                self.refresh_all(sender)?;
            }
            return Ok(());
        }

        self.pending_start_prompt = None;
        match self.state.open_tour(document.uri, &document.text) {
            Ok(()) => {
                self.resolve_active_source_range();
                let starts_by_itself = self.settings.auto_start == AutoStart::Always;
                self.apply_auto_start(sender)?;
                if !starts_by_itself {
                    self.refresh_all(sender)?;
                }
            }
            Err(error) => {
                eprintln!("{SERVER_NAME}: failed to open tour: {error}");
                self.refresh_all(sender)?;
            }
        }

        Ok(())
    }

    fn did_change(
        &mut self,
        params: DidChangeTextDocumentParams,
        sender: &Sender<Message>,
    ) -> Result<()> {
        let uri = params.text_document.uri;
        let is_tour = is_tour_document(&uri);
        let Some(change) = params.content_changes.into_iter().last() else {
            return Ok(());
        };
        if change.range.is_some() {
            eprintln!(
                "{SERVER_NAME}: ignoring incremental change for {}; full sync is advertised",
                uri.as_str()
            );
            // Full sync is advertised, but if a client sends an incremental
            // change anyway, drop the cached text instead of keeping stale
            // source content. The next navigation falls back to disk.
            self.documents.remove(&uri);
            if !is_tour {
                self.resolve_active_source_range();
            }
            return Ok(());
        }

        let is_active_source = self.is_active_source(&uri);
        self.documents.insert(uri.clone(), change.text.clone());
        if !is_tour {
            self.resolve_active_source_range();
            if is_active_source {
                self.refresh_all(sender)?;
            }
            return Ok(());
        }

        match self.state.update_active_tour(&uri, &change.text) {
            Ok(UpdateOutcome::Ignored) => Ok(()),
            Ok(UpdateOutcome::Updated) => {
                self.resolve_active_source_range();
                self.refresh_all(sender)
            }
            Err(error) => {
                eprintln!("{SERVER_NAME}: failed to update active tour: {error}");
                self.refresh_all(sender)
            }
        }
    }

    fn current_step_and_count(&self) -> std::result::Result<(usize, usize), CommandError> {
        let active = self.state.active().ok_or(CommandError::NoActiveTour)?;
        Ok((active.current_step, active.step_count()))
    }

    /// Select a step, resolve its exact UTF-16 range from the source document,
    /// then ask the client to show it and refresh both overlays.
    ///
    /// Source loading and range conversion happen before the step index is
    /// changed, so a missing file or an out-of-range column leaves the current
    /// selection and running state untouched.
    fn select_step(
        &mut self,
        step: usize,
        running: Option<bool>,
        sender: &Sender<Message>,
    ) -> std::result::Result<(), CommandError> {
        let (source_uri, tour_range) = self.target_for_step(step)?;
        let source_text = self.source_text(&source_uri)?;
        let range = tour_range_to_lsp(&source_text, tour_range)?;

        {
            let active = self.state.active_mut().ok_or(CommandError::NoActiveTour)?;
            if !active.set_current_step(step) {
                return Err(CommandError::StepOutOfRange {
                    step,
                    step_count: active.step_count(),
                });
            }
            active.source_range = Some(range);
            if let Some(running) = running {
                active.running = running;
            }
        }

        self.send_show_document(source_uri, range, sender)?;
        self.refresh_all(sender)
            .map_err(|error| CommandError::Internal(error.to_string()))
    }

    fn target_for_step(&self, step: usize) -> std::result::Result<(Uri, TourRange), CommandError> {
        let active = self.state.active().ok_or(CommandError::NoActiveTour)?;
        let tour_step = active
            .tour
            .steps
            .get(step)
            .ok_or(CommandError::StepOutOfRange {
                step,
                step_count: active.step_count(),
            })?;
        let uri = file_uri(&tour_step.target.path).ok_or_else(|| {
            CommandError::UnsupportedTarget(tour_step.target.path.display().to_string())
        })?;

        Ok((uri, tour_step.target.range))
    }

    fn current_target(&self) -> Option<(Uri, TourRange)> {
        let active = self.state.active()?;
        let step = active.current_step()?;
        Some((file_uri(&step.target.path)?, step.target.range))
    }

    /// Recompute the active step's LSP range from current source text. State
    /// stores the source URI and the range, but navigation is the only producer
    /// of the UTF-16 range so stale naive columns cannot leak into overlays.
    fn resolve_active_source_range(&mut self) {
        let Some((uri, tour_range)) = self.current_target() else {
            return;
        };
        let Ok(text) = self.source_text(&uri) else {
            return;
        };
        let Ok(range) = tour_range_to_lsp(&text, tour_range) else {
            return;
        };
        if let Some(active) = self.state.active_mut() {
            active.source_range = Some(range);
        }
    }

    fn source_text(&self, uri: &Uri) -> std::result::Result<String, CommandError> {
        if let Some(text) = self.documents.get(uri) {
            return Ok(text.clone());
        }

        let path =
            file_path(uri).ok_or_else(|| CommandError::UnsupportedTarget(uri.to_string()))?;
        std::fs::read_to_string(&path).map_err(|error| CommandError::UnreadableSource {
            uri: uri.to_string(),
            reason: error.to_string(),
        })
    }

    fn send_show_document(
        &mut self,
        uri: Uri,
        range: Range,
        sender: &Sender<Message>,
    ) -> std::result::Result<(), CommandError> {
        self.next_request_id += 1;
        let id = RequestId::from(format!("tour-show-document-{}", self.next_request_id));
        let params = serde_json::to_value(show_document_params(uri.clone(), range))
            .map_err(|error| CommandError::Internal(error.to_string()))?;
        sender
            .send(Message::Request(Request::new(
                id.clone(),
                SHOW_DOCUMENT.to_owned(),
                params,
            )))
            .map_err(|error| CommandError::Internal(error.to_string()))?;
        self.pending_show_documents.insert(id, uri);
        Ok(())
    }

    fn report_show_document_response(
        &self,
        response: &Response,
        uri: Uri,
        sender: &Sender<Message>,
    ) -> Result<()> {
        match &response.response_result {
            Ok(value) => {
                let success = serde_json::from_value::<ShowDocumentResult>(value.clone())
                    .map(|result| result.success)
                    .unwrap_or(false);
                if !success {
                    self.show_error(
                        sender,
                        &format!("Zed could not show tour target `{}`", uri.as_str()),
                    )?;
                }
            }
            Err(error) => self.show_error(
                sender,
                &format!(
                    "Zed could not show tour target `{}`: {}",
                    uri.as_str(),
                    error.message
                ),
            )?,
        }
        Ok(())
    }

    fn show_error(&self, sender: &Sender<Message>, message: &str) -> Result<()> {
        let params = ShowMessageParams {
            typ: MessageType::ERROR,
            message: message.to_owned(),
        };
        sender.send(Message::Notification(Notification::new(
            SHOW_MESSAGE.to_owned(),
            params,
        )))?;
        Ok(())
    }

    fn code_lenses_for(&self, params: &CodeLensParams) -> Vec<CodeLens> {
        let Some(active) = self.state.active() else {
            return Vec::new();
        };
        if active.tour_uri == params.text_document.uri {
            return self.tour_code_lenses(active);
        }
        if !active.running || active.source_uri.as_ref() != Some(&params.text_document.uri) {
            return Vec::new();
        }

        let Some(source_range) = active.source_range else {
            return Vec::new();
        };
        let Some(source) = self.documents.get(&params.text_document.uri) else {
            return Vec::new();
        };

        control_lenses(
            active.current_step,
            active.step_count(),
            source_range,
            source,
            self.settings.placement,
            active.thread_link().is_some(),
        )
    }

    fn tour_code_lenses(&self, active: &crate::state::ActiveTour) -> Vec<CodeLens> {
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

    fn show_info(&self, sender: &Sender<Message>, message: &str) -> Result<()> {
        self.show_message(sender, MessageType::INFO, message)
    }

    fn show_message(
        &self,
        sender: &Sender<Message>,
        typ: MessageType,
        message: &str,
    ) -> Result<()> {
        let params = ShowMessageParams {
            typ,
            message: message.to_owned(),
        };
        sender.send(Message::Notification(Notification::new(
            SHOW_MESSAGE.to_owned(),
            params,
        )))?;
        Ok(())
    }

    fn execute_command(
        &mut self,
        params: &ExecuteCommandParams,
        sender: &Sender<Message>,
    ) -> std::result::Result<(), CommandError> {
        match params.command.as_str() {
            "tour.start" => {
                self.start_active_tour(sender)?;
            }
            "tour.stop" => {
                if self.stop_active_tour() {
                    self.refresh_all(sender)
                        .map_err(|error| CommandError::Internal(error.to_string()))?;
                }
            }
            "tour.overview" => {
                let tour_uri = self
                    .state
                    .active()
                    .ok_or(CommandError::NoActiveTour)?
                    .tour_uri
                    .clone();
                let range = Range::new(Position::new(0, 0), Position::new(0, 0));
                self.send_show_document(tour_uri, range, sender)?;
            }
            "tour.backToThread" => {
                self.back_to_thread(sender)?;
            }
            "tour.next" => {
                let (step, step_count) = self.current_step_and_count()?;
                if step_count == 0 {
                    return Err(CommandError::NoSteps);
                }
                let next = step.saturating_add(1);
                if next < step_count {
                    self.select_step(next, None, sender)?;
                }
            }
            "tour.previous" => {
                let (step, step_count) = self.current_step_and_count()?;
                if step_count == 0 {
                    return Err(CommandError::NoSteps);
                }
                if step > 0 {
                    self.select_step(step - 1, None, sender)?;
                }
            }
            "tour.goto" => {
                let step = params
                    .arguments
                    .first()
                    .and_then(Value::as_u64)
                    .ok_or_else(|| {
                        CommandError::InvalidArguments(
                            "tour.goto expects a zero-based step index as its first argument"
                                .to_owned(),
                        )
                    })?;
                let step = usize::try_from(step).map_err(|_| {
                    CommandError::InvalidArguments(
                        "tour.goto step index does not fit in this platform".to_owned(),
                    )
                })?;
                self.select_step(step, None, sender)?;
            }
            _ => {}
        }
        Ok(())
    }

    /// Focus the tour file at the recorded thread link and explain the v1
    /// fallback. zed:///agent/thread URIs are not routed from code lens, so the
    /// command never pretends to have opened the thread.
    fn back_to_thread(
        &mut self,
        sender: &Sender<Message>,
    ) -> std::result::Result<(), CommandError> {
        let (tour_uri, line, link_text, thread_uri) = {
            let active = self.state.active().ok_or(CommandError::NoActiveTour)?;
            let link = active.thread_link().ok_or(CommandError::NoThreadLink)?;
            (
                active.tour_uri.clone(),
                link.line.saturating_sub(1),
                link.text.clone(),
                link.uri.clone(),
            )
        };

        let range = Range::new(Position::new(line, 0), Position::new(line, 0));
        self.send_show_document(tour_uri, range, sender)?;
        self.show_info(
            sender,
            &format!(
                "This Zed build does not route zed:///agent/thread links from code lens. \
                 Open the thread from the Agent panel. The tour file records `{link_text}` \
                 on line {}: `{thread_uri}`.",
                line.saturating_add(1)
            ),
        )
        .map_err(|error| CommandError::Internal(error.to_string()))
    }

    fn did_close(
        &mut self,
        params: DidCloseTextDocumentParams,
        sender: &Sender<Message>,
    ) -> Result<()> {
        let uri = params.text_document.uri;
        let is_active_source = self.is_active_source(&uri);
        self.documents.remove(&uri);
        self.resolve_active_source_range();
        if is_active_source {
            self.refresh_all(sender)?;
        }
        Ok(())
    }

    fn apply_auto_start(&mut self, sender: &Sender<Message>) -> Result<()> {
        match self.settings.auto_start {
            AutoStart::Always => {
                if let Err(error) = self.start_active_tour(sender) {
                    self.show_error(sender, &error.to_string())?;
                    self.refresh_all(sender)?;
                }
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

    fn start_active_tour(
        &mut self,
        sender: &Sender<Message>,
    ) -> std::result::Result<bool, CommandError> {
        let Some(active) = self.state.active() else {
            return Ok(false);
        };
        if active.step_count() == 0 {
            if let Some(active) = self.state.active_mut() {
                active.running = true;
            }
            self.refresh_all(sender)
                .map_err(|error| CommandError::Internal(error.to_string()))?;
            return Ok(true);
        }

        self.select_step(0, Some(true), sender)?;
        Ok(true)
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

    /// Return the single body hint for the active step in the requested source
    /// document. Inactive tours, stopped tours, and other documents have no
    /// hint.
    pub fn inlay_hints(&self, params: &InlayHintParams) -> Option<Vec<InlayHint>> {
        let active = self.state.active()?;
        if !active.running {
            return None;
        }
        if active.source_uri.as_ref() != Some(&params.text_document.uri) {
            return None;
        }

        let step = active.current_step()?;
        let source_range = active.source_range?;
        let source = self.documents.get(&params.text_document.uri)?;
        let hint = body_hint(step, source_range, source, self.settings.placement)?;

        if hint.position < params.range.start || hint.position > params.range.end {
            return None;
        }

        Some(vec![hint])
    }

    fn is_active_source(&self, uri: &Uri) -> bool {
        self.state
            .active()
            .and_then(|active| active.source_uri.as_ref())
            == Some(uri)
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

/// Errors returned by tour commands. They are both reported to the client as a
/// `window/showMessage` notification and returned as the command response.
#[derive(Debug)]
enum CommandError {
    NoActiveTour,
    NoThreadLink,
    NoSteps,
    InvalidArguments(String),
    StepOutOfRange { step: usize, step_count: usize },
    UnsupportedTarget(String),
    UnreadableSource { uri: String, reason: String },
    Position(PositionError),
    Internal(String),
}

impl fmt::Display for CommandError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoActiveTour => f.write_str("no tour is active"),
            Self::NoThreadLink => f.write_str("the active tour has no thread link"),
            Self::NoSteps => f.write_str("the active tour has no steps"),
            Self::InvalidArguments(message) => f.write_str(message),
            Self::StepOutOfRange { step, step_count } => write!(
                f,
                "step index {step} is outside the active tour, which has {step_count} step(s)"
            ),
            Self::UnsupportedTarget(target) => {
                write!(f, "tour target `{target}` is not a local file URI")
            }
            Self::UnreadableSource { uri, reason } => {
                write!(f, "cannot read tour target `{uri}`: {reason}")
            }
            Self::Position(error) => write!(f, "tour target range is invalid: {error}"),
            Self::Internal(message) => f.write_str(message),
        }
    }
}

impl From<PositionError> for CommandError {
    fn from(error: PositionError) -> Self {
        Self::Position(error)
    }
}

impl std::error::Error for CommandError {}

fn settings_from_configuration(value: &Value, current: TourSettings) -> TourSettings {
    let tour_settings = value
        .pointer("/lsp/tour/settings")
        .or_else(|| value.get("settings"))
        .unwrap_or(value);
    if !has_tour_settings(tour_settings) {
        return current;
    }
    TourSettings::from_initialization_options(Some(tour_settings))
}

fn has_tour_settings(value: &Value) -> bool {
    value
        .as_object()
        .is_some_and(|object| object.contains_key("placement") || object.contains_key("auto_start"))
}

#[cfg(test)]
mod tests {
    use lsp_server::{Message, Request, RequestId, Response};
    use lsp_types::{
        InlayHint, InlayHintLabel, InlayHintParams, Position, Range, ShowMessageRequestParams,
        TextDocumentContentChangeEvent, TextDocumentIdentifier, TextDocumentItem,
        VersionedTextDocumentIdentifier,
    };
    use serde_json::{Value, json};

    use super::{TOUR_COMMANDS, TourServer};
    use crate::settings::{AutoStart, Placement, TourSettings};
    use crate::state::UpdateOutcome;

    const SOURCE_URI: &str = "file:///tours/src/main.rs";
    const ROUTER_URI: &str = "file:///tours/src/router.rs";
    const TOUR_URI: &str = "file:///tours/example.tour.md";
    const SOURCE: &str = "line0\nline1\nline2\nline3\nline4\n";
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
    const BODY_TOUR: &str = r#"# Tour

## 1. Entry
[entry](src/main.rs#L2:1-L2:5)

Entry body.
"#;
    const TOUR_WITH_TWO_STEPS: &str = r#"# Tour

## 1. Entry
[entry](src/main.rs#L2:1-L2:5)

Entry body.

## 2. Next
[next](src/main.rs#L4:1-L4:5)

Next body.
"#;
    const THREAD_TOUR: &str = r#"# Thread tour

[Back to the thread](zed:///agent/thread/session-1?name=Auth)

## 1. Entry
[entry](src/main.rs#L2:3-L2:9)
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

    fn execute_command_request_with_arguments(
        id: i32,
        command: &str,
        arguments: Vec<Value>,
    ) -> Request {
        Request::new(
            RequestId::from(id),
            "workspace/executeCommand".to_owned(),
            json!({
                "command": command,
                "arguments": arguments,
            }),
        )
    }

    fn drain_server_requests(
        receiver: &crossbeam_channel::Receiver<Message>,
    ) -> Vec<(RequestId, String, Value)> {
        receiver
            .try_iter()
            .map(|message| match message {
                Message::Request(request) => (request.id, request.method, request.params),
                other => panic!("unexpected message: {other:?}"),
            })
            .collect()
    }

    fn assert_command_ok(response: &Response) {
        match &response.response_result {
            Ok(value) => assert_eq!(value, &json!(null)),
            Err(error) => panic!("command failed: {error:?}"),
        }
    }

    fn assert_show_document(
        requests: &[(RequestId, String, Value)],
        uri: &str,
        start: (u32, u32),
        end: (u32, u32),
    ) {
        let (_, method, params) = requests
            .iter()
            .find(|(_, method, _)| method == "window/showDocument")
            .expect("missing window/showDocument request");
        assert_eq!(method, "window/showDocument");
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

    fn assert_refreshes(requests: &[(RequestId, String, Value)]) {
        let refresh_methods: Vec<_> = requests
            .iter()
            .filter(|(_, method, _)| method.contains("/refresh"))
            .map(|(_, method, _)| method.as_str())
            .collect();
        assert_eq!(
            refresh_methods,
            vec!["workspace/inlayHint/refresh", "workspace/codeLens/refresh"]
        );
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
        did_open_notification_with_language(uri, text, "Tour")
    }

    fn did_open_source_notification(uri: &str, text: &str) -> lsp_server::Notification {
        did_open_notification_with_language(uri, text, "Rust")
    }

    fn did_open_notification_with_language(
        uri: &str,
        text: &str,
        language_id: &str,
    ) -> lsp_server::Notification {
        lsp_server::Notification::new(
            "textDocument/didOpen".to_owned(),
            json!({
                "textDocument": TextDocumentItem {
                    uri: uri.parse().expect("valid URI"),
                    language_id: language_id.to_owned(),
                    version: 1,
                    text: text.to_owned(),
                },
            }),
        )
    }

    fn open_navigation_sources(
        server: &mut TourServer,
        sender: &crossbeam_channel::Sender<Message>,
    ) {
        for (uri, text) in [
            ("file:///tours/src/main.rs", "a\nline two long\n"),
            ("file:///tours/src/router.rs", "a\nb\nc\nlong enough\n"),
        ] {
            server
                .handle_notification(&did_open_notification(uri, text), sender)
                .expect("source should open");
        }
    }

    fn did_close_notification(uri: &str) -> lsp_server::Notification {
        lsp_server::Notification::new(
            "textDocument/didClose".to_owned(),
            json!({
                "textDocument": TextDocumentIdentifier {
                    uri: uri.parse().expect("valid URI"),
                },
            }),
        )
    }

    fn did_change_configuration_notification(
        settings: serde_json::Value,
    ) -> lsp_server::Notification {
        lsp_server::Notification::new(
            "workspace/didChangeConfiguration".to_owned(),
            json!({ "settings": settings }),
        )
    }

    fn inlay_hint_params(uri: &str) -> InlayHintParams {
        InlayHintParams {
            work_done_progress_params: Default::default(),
            text_document: TextDocumentIdentifier {
                uri: uri.parse().expect("valid URI"),
            },
            range: Range::new(Position::new(0, 0), Position::new(u32::MAX, 0)),
        }
    }

    fn hint_label(hint: &InlayHint) -> &str {
        match &hint.label {
            InlayHintLabel::String(label) => label,
            InlayHintLabel::LabelParts(_) => panic!("expected a string label"),
        }
    }

    fn active_hint(source: &str, placement: Placement) -> Option<InlayHint> {
        let (sender, _receiver) = crossbeam_channel::unbounded();
        let mut server = TourServer::new(TourSettings {
            placement,
            ..Default::default()
        });

        server
            .handle_notification(&did_open_source_notification(SOURCE_URI, source), &sender)
            .expect("source should open");
        server
            .handle_notification(
                &did_open_notification("file:///tours/example.tour.md", BODY_TOUR),
                &sender,
            )
            .expect("tour should open");
        server
            .state
            .active_mut()
            .expect("tour should be active")
            .running = true;

        server
            .inlay_hints(&inlay_hint_params(SOURCE_URI))
            .and_then(|hints| hints.into_iter().next())
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
    fn returns_one_padded_body_hint_for_the_active_source() {
        let hint = active_hint(SOURCE, Placement::Below).expect("hint should exist");

        assert_eq!(hint.position, Position::new(2, 0));
        assert_eq!(hint.kind, None);
        assert_eq!(hint.padding_left, Some(true));
        assert_eq!(hint_label(&hint), " ▎ Entry body.\n");

        let value = serde_json::to_value(&hint).expect("hint should serialize");
        assert!(value.get("kind").is_none());
        assert!(value.get("paddingRight").is_none());
        assert_eq!(value["paddingLeft"], json!(true));
        assert!(
            value["label"]
                .as_str()
                .expect("string label")
                .lines()
                .all(|line| line.starts_with(" ▎ "))
        );
    }

    #[test]
    fn above_placement_anchors_at_the_selection_start_line() {
        let hint = active_hint(SOURCE, Placement::Above).expect("hint should exist");

        assert_eq!(hint.position, Position::new(1, 0));
        assert_eq!(hint_label(&hint), " ▎ Entry body.\n");
    }

    #[test]
    fn below_placement_falls_back_at_the_end_of_the_last_line() {
        let hint = active_hint("line0\nline1", Placement::Below).expect("hint should exist");

        assert_eq!(hint.position, Position::new(1, 5));
        assert_eq!(hint_label(&hint), "\n ▎ Entry body.\n");
    }

    #[test]
    fn trailing_newline_uses_the_empty_line_after_the_selection() {
        let hint = active_hint("line0\nline1\n", Placement::Below).expect("hint should exist");

        assert_eq!(hint.position, Position::new(2, 0));
        assert_eq!(hint_label(&hint), " ▎ Entry body.\n");
    }

    #[test]
    fn hint_refreshes_on_step_change_and_is_removed_on_stop() {
        let (sender, receiver) = crossbeam_channel::unbounded();
        let mut server = TourServer::new(TourSettings::default());

        server
            .handle_notification(&did_open_source_notification(SOURCE_URI, SOURCE), &sender)
            .expect("source should open");
        server
            .handle_notification(
                &did_open_notification("file:///tours/example.tour.md", TOUR_WITH_TWO_STEPS),
                &sender,
            )
            .expect("tour should open");
        server
            .state
            .active_mut()
            .expect("tour should be active")
            .running = true;
        let _ = receiver.try_iter().count();

        let hint = server
            .inlay_hints(&inlay_hint_params(SOURCE_URI))
            .expect("active tour should have a hint")
            .remove(0);
        assert_eq!(hint_label(&hint), " ▎ Entry body.\n");

        let next = server.handle_request(&execute_command_request(2, "tour.next"), &sender);
        assert_command_ok(&next);
        let requests = drain_server_requests(&receiver);
        assert!(
            requests
                .iter()
                .any(|(_, method, _)| method == "workspace/inlayHint/refresh"),
            "step change should refresh hints"
        );

        let hint = server
            .inlay_hints(&inlay_hint_params(SOURCE_URI))
            .expect("new step should have a hint")
            .remove(0);
        assert_eq!(hint_label(&hint), " ▎ Next body.\n");

        let stop = server.handle_request(&execute_command_request(3, "tour.stop"), &sender);
        assert_command_ok(&stop);
        let requests = drain_server_requests(&receiver);
        assert!(
            requests
                .iter()
                .any(|(_, method, _)| method == "workspace/inlayHint/refresh"),
            "stop should refresh hints"
        );
        assert!(
            server.inlay_hints(&inlay_hint_params(SOURCE_URI)).is_none(),
            "a stopped tour should not return a hint"
        );
    }

    #[test]
    fn placement_change_updates_the_hint_and_refreshes() {
        let (sender, receiver) = crossbeam_channel::unbounded();
        let mut server = TourServer::new(TourSettings::default());

        server
            .handle_notification(&did_open_source_notification(SOURCE_URI, SOURCE), &sender)
            .expect("source should open");
        server
            .handle_notification(
                &did_open_notification("file:///tours/example.tour.md", BODY_TOUR),
                &sender,
            )
            .expect("tour should open");
        server
            .state
            .active_mut()
            .expect("tour should be active")
            .running = true;
        let _ = receiver.try_iter().count();

        let hint = server
            .inlay_hints(&inlay_hint_params(SOURCE_URI))
            .expect("active tour should have a hint")
            .remove(0);
        assert_eq!(hint.position, Position::new(2, 0));

        server
            .handle_notification(
                &did_change_configuration_notification(json!({ "placement": "above" })),
                &sender,
            )
            .expect("configuration change should be handled");

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

        let hint = server
            .inlay_hints(&inlay_hint_params(SOURCE_URI))
            .expect("placement change should still have a hint")
            .remove(0);
        assert_eq!(hint.position, Position::new(1, 0));
    }

    #[test]
    fn empty_configuration_payload_keeps_initialization_settings() {
        let (sender, receiver) = crossbeam_channel::unbounded();
        let initialized = TourSettings {
            placement: Placement::Above,
            auto_start: AutoStart::Never,
        };
        let mut server = TourServer::new(initialized);

        server
            .handle_notification(&did_open_source_notification(SOURCE_URI, SOURCE), &sender)
            .expect("source should open");
        server
            .handle_notification(
                &did_open_notification("file:///tours/example.tour.md", BODY_TOUR),
                &sender,
            )
            .expect("tour should open");
        server
            .state
            .active_mut()
            .expect("tour should be active")
            .running = true;
        let _ = receiver.try_iter().count();

        server
            .handle_notification(&did_change_configuration_notification(json!({})), &sender)
            .expect("empty configuration should be ignored");

        assert_eq!(server.settings(), initialized);
        assert!(
            receiver.try_recv().is_err(),
            "an empty configuration should not refresh"
        );
        let hint = server
            .inlay_hints(&inlay_hint_params(SOURCE_URI))
            .expect("above placement should survive")
            .remove(0);
        assert_eq!(hint.position, Position::new(1, 0));
    }

    #[test]
    fn inlay_hint_is_scoped_to_the_requested_range() {
        let hint = active_hint(SOURCE, Placement::Below).expect("hint should exist");
        assert_eq!(hint.position, Position::new(2, 0));

        let (sender, _receiver) = crossbeam_channel::unbounded();
        let mut server = TourServer::new(TourSettings::default());
        server
            .handle_notification(&did_open_source_notification(SOURCE_URI, SOURCE), &sender)
            .expect("source should open");
        server
            .handle_notification(
                &did_open_notification("file:///tours/example.tour.md", BODY_TOUR),
                &sender,
            )
            .expect("tour should open");
        server
            .state
            .active_mut()
            .expect("tour should be active")
            .running = true;

        let mut params = inlay_hint_params(SOURCE_URI);
        params.range = Range::new(Position::new(0, 0), Position::new(1, 0));
        assert!(server.inlay_hints(&params).is_none());

        params.range = Range::new(Position::new(2, 0), Position::new(2, 0));
        assert!(server.inlay_hints(&params).is_some());
    }

    #[test]
    fn closing_the_active_source_refreshes_and_removes_the_hint() {
        let (sender, receiver) = crossbeam_channel::unbounded();
        let mut server = TourServer::new(TourSettings::default());

        server
            .handle_notification(&did_open_source_notification(SOURCE_URI, SOURCE), &sender)
            .expect("source should open");
        server
            .handle_notification(
                &did_open_notification("file:///tours/example.tour.md", BODY_TOUR),
                &sender,
            )
            .expect("tour should open");
        server
            .state
            .active_mut()
            .expect("tour should be active")
            .running = true;
        let _ = receiver.try_iter().count();
        assert!(server.inlay_hints(&inlay_hint_params(SOURCE_URI)).is_some());

        server
            .handle_notification(&did_close_notification(SOURCE_URI), &sender)
            .expect("source should close");

        let refresh = receiver.try_recv().expect("close should refresh hints");
        assert!(matches!(
            refresh,
            Message::Request(request) if request.method == "workspace/inlayHint/refresh"
        ));
        assert!(server.inlay_hints(&inlay_hint_params(SOURCE_URI)).is_none());
    }

    #[test]
    fn inlay_hint_request_returns_a_null_result_when_inactive() {
        let (sender, _receiver) = crossbeam_channel::unbounded();
        let mut server = TourServer::new(TourSettings::default());
        let request = lsp_server::Request::new(
            RequestId::from(1),
            "textDocument/inlayHint".to_owned(),
            serde_json::to_value(inlay_hint_params(SOURCE_URI)).expect("params should serialize"),
        );

        let response = server.handle_request(&request, &sender);

        match response.response_result {
            Ok(result) => assert_eq!(result, json!(null)),
            Err(error) => panic!("unexpected error response: {error:?}"),
        }
    }

    #[test]
    fn opens_tours_and_sends_both_refreshes() {
        let (sender, receiver) = crossbeam_channel::unbounded();
        let mut server = TourServer::new(settings(AutoStart::Never));

        server
            .handle_notification(
                &did_open_notification("file:///tours/src/main.rs", "a\nline two long\n"),
                &sender,
            )
            .expect("source should open");
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
    fn running_tour_returns_the_control_row_for_the_active_source() {
        let (sender, receiver) = crossbeam_channel::unbounded();
        let mut server = TourServer::new(settings(AutoStart::Never));

        open_navigation_sources(&mut server, &sender);
        server
            .handle_notification(&did_open_notification(TOUR_URI, TWO_STEP_TOUR), &sender)
            .expect("notification should be handled");
        let _ = receiver.try_iter().count();

        let start = server.handle_request(&execute_command_request(1, "tour.start"), &sender);
        assert_command_ok(&start);
        let _ = receiver.try_iter().count();

        let response = server.handle_request(&code_lens_request(2, SOURCE_URI), &sender);
        assert_eq!(
            code_lens_titles(&response),
            vec!["◀ Prev", "1/2 | Next ▶", "Overview", "Stop"]
        );
        assert_eq!(
            code_lens_commands(&response),
            vec![
                crate::controls::PREVIOUS_COMMAND,
                crate::controls::NEXT_COMMAND,
                crate::controls::OVERVIEW_COMMAND,
                crate::controls::STOP_COMMAND,
            ]
        );
        for lens in response
            .response_result
            .as_ref()
            .expect("code lens result")
            .as_array()
            .expect("code lens array")
        {
            assert_eq!(
                lens["range"]["start"]["line"],
                json!(2),
                "below placement should anchor after the selected line"
            );
            assert_eq!(lens["command"].get("arguments"), None);
        }

        let next = server.handle_request(&execute_command_request(3, "tour.next"), &sender);
        assert_command_ok(&next);
        let requests = drain_server_requests(&receiver);
        assert!(
            requests
                .iter()
                .any(|(_, method, _)| method == "workspace/codeLens/refresh"),
            "step change should refresh the control row"
        );

        let response = server.handle_request(&code_lens_request(4, SOURCE_URI), &sender);
        assert!(
            response
                .response_result
                .as_ref()
                .expect("code lens result")
                .as_array()
                .expect("code lens array")
                .is_empty(),
            "a source that is no longer the active step should have no control row"
        );

        let response = server.handle_request(&code_lens_request(5, ROUTER_URI), &sender);
        assert_eq!(
            code_lens_titles(&response),
            vec!["◀ Prev", "2/2 | Next ▶", "Overview", "Stop"]
        );

        let stop = server.handle_request(&execute_command_request(6, "tour.stop"), &sender);
        assert_command_ok(&stop);
        let requests = drain_server_requests(&receiver);
        assert!(
            requests
                .iter()
                .any(|(_, method, _)| method == "workspace/codeLens/refresh"),
            "stop should refresh the control row away"
        );
        let response = server.handle_request(&code_lens_request(7, ROUTER_URI), &sender);
        assert!(
            response
                .response_result
                .as_ref()
                .expect("code lens result")
                .as_array()
                .expect("code lens array")
                .is_empty(),
            "a stopped tour should have no control row"
        );
    }

    #[test]
    fn thread_tour_control_row_exposes_back_to_thread() {
        let (sender, receiver) = crossbeam_channel::unbounded();
        let mut server = TourServer::new(settings(AutoStart::Never));

        open_navigation_sources(&mut server, &sender);
        server
            .handle_notification(&did_open_notification(TOUR_URI, THREAD_TOUR), &sender)
            .expect("notification should be handled");
        let _ = receiver.try_iter().count();
        let start = server.handle_request(&execute_command_request(1, "tour.start"), &sender);
        assert_command_ok(&start);
        let _ = receiver.try_iter().count();

        let response = server.handle_request(&code_lens_request(2, SOURCE_URI), &sender);
        assert_eq!(
            code_lens_titles(&response),
            vec!["1/1 | Overview", "Back to thread", "Stop"]
        );
        assert_eq!(
            code_lens_commands(&response),
            vec![
                crate::controls::OVERVIEW_COMMAND,
                crate::controls::THREAD_COMMAND,
                crate::controls::STOP_COMMAND,
            ]
        );
    }

    #[test]
    fn back_to_thread_focuses_the_link_and_explains_the_static_fallback() {
        let (sender, receiver) = crossbeam_channel::unbounded();
        let mut server = TourServer::new(settings(AutoStart::Never));

        open_navigation_sources(&mut server, &sender);
        server
            .handle_notification(&did_open_notification(TOUR_URI, THREAD_TOUR), &sender)
            .expect("notification should be handled");
        let _ = receiver.try_iter().count();

        let response =
            server.handle_request(&execute_command_request(1, "tour.backToThread"), &sender);
        assert_command_ok(&response);

        let messages: Vec<_> = receiver.try_iter().collect();
        let show_document = messages
            .iter()
            .find_map(|message| match message {
                Message::Request(request) if request.method == "window/showDocument" => {
                    Some(request.params.clone())
                }
                _ => None,
            })
            .expect("backToThread should focus the tour file");
        assert_eq!(
            show_document,
            json!({
                "uri": TOUR_URI,
                "external": false,
                "takeFocus": true,
                "selection": {
                    "start": { "line": 2, "character": 0 },
                    "end": { "line": 2, "character": 0 },
                },
            })
        );

        let notification = messages
            .iter()
            .find_map(|message| match message {
                Message::Notification(notification)
                    if notification.method == "window/showMessage" =>
                {
                    Some(&notification.params)
                }
                _ => None,
            })
            .expect("backToThread should explain the fallback");
        assert_eq!(notification["type"], json!(3));
        let text = notification["message"]
            .as_str()
            .expect("message should be text");
        assert!(
            text.contains("does not route zed:///agent/thread links"),
            "{text}"
        );
        assert!(
            text.contains("zed:///agent/thread/session-1?name=Auth"),
            "{text}"
        );
        assert!(
            messages.iter().all(|message| !matches!(
                message,
                Message::Request(request) if request.method.contains("/refresh")
            )),
            "backToThread changes no tour state and should not refresh overlays"
        );
    }

    #[test]
    fn back_to_thread_without_a_link_reports_an_error() {
        let (sender, receiver) = crossbeam_channel::unbounded();
        let mut server = TourServer::new(settings(AutoStart::Never));

        server
            .handle_notification(&did_open_notification(TOUR_URI, TOUR), &sender)
            .expect("notification should be handled");
        let _ = receiver.try_iter().count();

        let response =
            server.handle_request(&execute_command_request(1, "tour.backToThread"), &sender);
        let error = response
            .response_result
            .expect_err("a tour without a thread link should fail");
        assert!(error.message.contains("no thread link"), "{error:?}");

        let messages: Vec<_> = receiver.try_iter().collect();
        assert_eq!(messages.len(), 1);
        match &messages[0] {
            Message::Notification(notification) => {
                assert_eq!(notification.method, "window/showMessage");
                assert_eq!(notification.params["type"], json!(1));
                assert!(
                    notification.params["message"]
                        .as_str()
                        .expect("message text")
                        .contains("no thread link")
                );
            }
            other => panic!("unexpected message: {other:?}"),
        }
    }

    #[test]
    fn overview_shows_the_tour_file_at_the_top() {
        let (sender, receiver) = crossbeam_channel::unbounded();
        let mut server = TourServer::new(settings(AutoStart::Never));

        open_navigation_sources(&mut server, &sender);
        server
            .handle_notification(&did_open_notification(TOUR_URI, TWO_STEP_TOUR), &sender)
            .expect("notification should be handled");
        let _ = receiver.try_iter().count();
        let start = server.handle_request(&execute_command_request(1, "tour.start"), &sender);
        assert_command_ok(&start);
        let _ = receiver.try_iter().count();

        let response = server.handle_request(&execute_command_request(2, "tour.overview"), &sender);
        assert_command_ok(&response);
        let requests = drain_server_requests(&receiver);
        assert_show_document(&requests, TOUR_URI, (0, 0), (0, 0));
        assert!(
            requests
                .iter()
                .all(|(_, method, _)| method != "workspace/inlayHint/refresh"),
            "overview only changes focus and should not refresh overlays"
        );
    }

    #[test]
    fn start_stop_and_restart_transition_state_refresh_and_update_labels() {
        let (sender, receiver) = crossbeam_channel::unbounded();
        let mut server = TourServer::new(settings(AutoStart::Never));
        let tour_uri = "file:///tours/example.tour.md";

        open_navigation_sources(&mut server, &sender);
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
                "window/showDocument".to_owned(),
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
        assert_eq!(receiver.try_iter().count(), 3);

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

        open_navigation_sources(&mut server, &sender);
        let _ = receiver.try_iter().count();
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
                "window/showDocument".to_owned(),
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

        open_navigation_sources(&mut server, &sender);
        let _ = receiver.try_iter().count();
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
                "window/showDocument".to_owned(),
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

    #[test]
    fn selects_exact_utf16_ranges_on_start_and_step_transitions() {
        let (sender, receiver) = crossbeam_channel::unbounded();
        let mut server = TourServer::new(settings(AutoStart::Never));
        let source = "let 😀 = 1;\nlet café = 2;\n";
        let tour = r#"# Tour

## 1. Emoji
[emoji](src/main.rs#L1:5-L1:6)

## 2. Accent
[accent](src/main.rs#L2:5-L2:8)

## 3. Span
[span](src/main.rs#L1:9-L2:3)
"#;

        server
            .handle_notification(
                &did_open_notification("file:///tours/src/main.rs", source),
                &sender,
            )
            .expect("source should open");
        server
            .handle_notification(
                &did_open_notification("file:///tours/example.tour.md", tour),
                &sender,
            )
            .expect("tour should open");
        assert_eq!(receiver.try_iter().count(), 2);

        let response = server.handle_request(
            &execute_command_request_with_arguments(2, "tour.start", Vec::new()),
            &sender,
        );
        assert_command_ok(&response);
        let requests = drain_server_requests(&receiver);
        assert_show_document(&requests, "file:///tours/src/main.rs", (0, 4), (0, 6));
        assert_refreshes(&requests);

        let active = server.state().active().expect("tour is active");
        assert!(active.running);
        assert_eq!(active.current_step, 0);
        assert_eq!(
            active.source_range,
            Some(Range::new(Position::new(0, 4), Position::new(0, 6)))
        );

        let response = server.handle_request(
            &execute_command_request_with_arguments(3, "tour.next", Vec::new()),
            &sender,
        );
        assert_command_ok(&response);
        let requests = drain_server_requests(&receiver);
        assert_show_document(&requests, "file:///tours/src/main.rs", (1, 4), (1, 7));
        assert_refreshes(&requests);
        assert_eq!(
            server.state().active().map(|active| active.current_step),
            Some(1)
        );

        let response = server.handle_request(
            &execute_command_request_with_arguments(4, "tour.next", Vec::new()),
            &sender,
        );
        assert_command_ok(&response);
        let requests = drain_server_requests(&receiver);
        assert_show_document(&requests, "file:///tours/src/main.rs", (0, 9), (1, 2));
        assert_refreshes(&requests);

        let response = server.handle_request(
            &execute_command_request_with_arguments(5, "tour.next", Vec::new()),
            &sender,
        );
        assert_command_ok(&response);
        assert!(drain_server_requests(&receiver).is_empty());
        assert_eq!(
            server.state().active().map(|active| active.current_step),
            Some(2),
            "next at the last step is a no-op"
        );

        let response = server.handle_request(
            &execute_command_request_with_arguments(6, "tour.previous", Vec::new()),
            &sender,
        );
        assert_command_ok(&response);
        let requests = drain_server_requests(&receiver);
        assert_show_document(&requests, "file:///tours/src/main.rs", (1, 4), (1, 7));
        assert_eq!(
            server.state().active().map(|active| active.current_step),
            Some(1)
        );

        let response = server.handle_request(
            &execute_command_request_with_arguments(7, "tour.goto", vec![json!(0)]),
            &sender,
        );
        assert_command_ok(&response);
        let requests = drain_server_requests(&receiver);
        assert_show_document(&requests, "file:///tours/src/main.rs", (0, 4), (0, 6));
        assert_eq!(
            server.state().active().map(|active| active.current_step),
            Some(0)
        );
    }

    #[test]
    fn source_range_stays_utf16_correct_after_a_tour_change() {
        let (sender, receiver) = crossbeam_channel::unbounded();
        let mut server = TourServer::new(settings(AutoStart::Never));
        let source = "let 😀 = 1;\n";
        let tour = "# Tour\n\n## 1. Emoji\n[emoji](src/main.rs#L1:5-L1:6)\n";

        server
            .handle_notification(
                &did_open_notification("file:///tours/src/main.rs", source),
                &sender,
            )
            .expect("source should open");
        server
            .handle_notification(
                &did_open_notification("file:///tours/example.tour.md", tour),
                &sender,
            )
            .expect("tour should open");
        let _ = receiver.try_iter().count();

        assert_eq!(
            server
                .state()
                .active()
                .and_then(|active| active.source_range),
            Some(Range::new(Position::new(0, 4), Position::new(0, 6)))
        );

        server
            .handle_notification(
                &did_change_notification("file:///tours/example.tour.md", tour),
                &sender,
            )
            .expect("tour change should be handled");
        let _ = receiver.try_iter().count();

        assert_eq!(
            server
                .state()
                .active()
                .and_then(|active| active.source_range),
            Some(Range::new(Position::new(0, 4), Position::new(0, 6))),
            "a reparse must not replace the UTF-16 range with a naive column range"
        );
    }

    #[test]
    fn missing_source_and_invalid_ranges_report_errors_without_moving() {
        let (sender, receiver) = crossbeam_channel::unbounded();
        let mut server = TourServer::new(settings(AutoStart::Never));
        let tour = r#"# Tour

## 1. Available
[available](src/main.rs#L1:1-L1:4)

## 2. Missing
[missing](src/missing.rs#L1:1-L1:2)

## 3. Bad column
[bad](src/main.rs#L1:99-L1:99)
"#;

        server
            .handle_notification(
                &did_open_notification("file:///tours/src/main.rs", "abc\n"),
                &sender,
            )
            .expect("source should open");
        server
            .handle_notification(
                &did_open_notification("file:///tours/example.tour.md", tour),
                &sender,
            )
            .expect("tour should open");
        assert_eq!(receiver.try_iter().count(), 2);

        let response = server.handle_request(
            &execute_command_request_with_arguments(2, "tour.start", Vec::new()),
            &sender,
        );
        assert_command_ok(&response);
        assert_refreshes(&drain_server_requests(&receiver));

        let response = server.handle_request(
            &execute_command_request_with_arguments(3, "tour.next", Vec::new()),
            &sender,
        );
        let error = response
            .response_result
            .expect_err("missing source should fail");
        assert_eq!(error.code, -32803);
        assert!(error.message.contains("missing.rs"), "{error:?}");
        assert!(
            error.message.contains("cannot read tour target"),
            "{error:?}"
        );
        assert_eq!(
            server.state().active().map(|active| active.current_step),
            Some(0),
            "a failed transition must not move the active step"
        );
        assert_eq!(
            server.state().active().map(|active| active.running),
            Some(true),
            "a failed transition must not change the running state"
        );
        let messages: Vec<_> = receiver.try_iter().collect();
        assert_eq!(messages.len(), 1, "only the error message is sent");
        match &messages[0] {
            Message::Notification(notification) => {
                assert_eq!(notification.method, "window/showMessage");
                assert_eq!(notification.params["type"], json!(1));
                assert!(
                    notification.params["message"]
                        .as_str()
                        .expect("message text")
                        .contains("cannot read tour target")
                );
            }
            other => panic!("unexpected message: {other:?}"),
        }

        let response = server.handle_request(
            &execute_command_request_with_arguments(4, "tour.goto", vec![json!(2)]),
            &sender,
        );
        let error = response
            .response_result
            .expect_err("bad column should fail");
        assert_eq!(error.code, -32803);
        assert!(error.message.contains("tour column 99"), "{error:?}");
        assert!(error.message.contains("line 1"), "{error:?}");
        assert!(receiver.try_iter().all(|message| matches!(
            message,
            Message::Notification(notification) if notification.method == "window/showMessage"
        )));
        assert_eq!(
            server.state().active().map(|active| active.current_step),
            Some(0)
        );
    }

    #[test]
    fn reports_client_side_show_document_failures() {
        let (sender, receiver) = crossbeam_channel::unbounded();
        let mut server = TourServer::new(settings(AutoStart::Never));

        server
            .handle_notification(
                &did_open_notification("file:///tours/src/main.rs", "abc\nline two long\n"),
                &sender,
            )
            .expect("source should open");
        server
            .handle_notification(
                &did_open_notification("file:///tours/example.tour.md", TOUR),
                &sender,
            )
            .expect("tour should open");
        let _ = receiver.try_iter().count();

        let response = server.handle_request(
            &execute_command_request_with_arguments(2, "tour.start", Vec::new()),
            &sender,
        );
        assert_command_ok(&response);
        let requests = drain_server_requests(&receiver);
        let show_document_id = requests
            .iter()
            .find(|(_, method, _)| method == "window/showDocument")
            .expect("missing showDocument")
            .0
            .clone();

        server
            .handle_response(
                &Response::new_ok(show_document_id, json!({ "success": false })),
                &sender,
            )
            .expect("showDocument response should be handled");

        let messages: Vec<_> = receiver.try_iter().collect();
        assert_eq!(messages.len(), 1);
        match &messages[0] {
            Message::Notification(notification) => {
                assert_eq!(notification.method, "window/showMessage");
                assert_eq!(notification.params["type"], json!(1));
                assert!(
                    notification.params["message"]
                        .as_str()
                        .expect("message text")
                        .contains("could not show tour target")
                );
            }
            other => panic!("unexpected message: {other:?}"),
        }
    }

    #[test]
    fn rejects_goto_arguments_and_out_of_range_steps() {
        let (sender, receiver) = crossbeam_channel::unbounded();
        let mut server = TourServer::new(settings(AutoStart::Never));

        server
            .handle_notification(
                &did_open_notification("file:///tours/example.tour.md", TOUR),
                &sender,
            )
            .expect("tour should open");
        assert_eq!(receiver.try_iter().count(), 2);

        let response = server.handle_request(
            &execute_command_request_with_arguments(2, "tour.goto", Vec::new()),
            &sender,
        );
        let error = response
            .response_result
            .expect_err("goto needs an argument");
        assert!(error.message.contains("zero-based step index"), "{error:?}");

        let response = server.handle_request(
            &execute_command_request_with_arguments(3, "tour.goto", vec![json!(7)]),
            &sender,
        );
        let error = response
            .response_result
            .expect_err("goto out of range should fail");
        assert!(error.message.contains("step index 7"), "{error:?}");
        assert_eq!(
            server.state().active().map(|active| active.current_step),
            Some(0)
        );
        assert!(receiver.try_iter().all(|message| matches!(
            message,
            Message::Notification(notification) if notification.method == "window/showMessage"
        )));
    }
}
