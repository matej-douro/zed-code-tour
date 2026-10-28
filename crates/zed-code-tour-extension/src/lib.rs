use zed_extension_api::{
    self as zed, Command, LanguageServerId, Result, Worktree, serde_json::Value,
    settings::LspSettings,
};

const TOUR_LANGUAGE_SERVER_ID: &str = "tour";
const TOUR_LSP_BINARY: &str = "tour-lsp";

struct CodeTourExtension;

impl CodeTourExtension {
    fn is_tour_server(language_server_id: &LanguageServerId) -> bool {
        language_server_id.as_ref() == TOUR_LANGUAGE_SERVER_ID
    }

    fn settings_schema() -> Value {
        zed::serde_json::json!({
            "$schema": "https://json-schema.org/draft/2020-12/schema",
            "title": "Code Tour settings",
            "description": "Settings for the Code Tour language server.",
            "type": "object",
            "properties": {
                "placement": {
                    "type": "string",
                    "description": "Where the tour body is rendered relative to the selected source range.",
                    "enum": ["below", "above"],
                    "default": "below"
                },
                "auto_start": {
                    "type": "string",
                    "description": "Whether to start a tour when its file opens.",
                    "enum": ["always", "prompt", "never"],
                    "default": "prompt"
                }
            },
            "additionalProperties": true
        })
    }
}

impl zed::Extension for CodeTourExtension {
    fn new() -> Self {
        Self
    }

    fn language_server_command(
        &mut self,
        language_server_id: &LanguageServerId,
        worktree: &Worktree,
    ) -> Result<Command> {
        if !Self::is_tour_server(language_server_id) {
            return Err(format!("unsupported language server: {language_server_id}"));
        }

        let command = worktree.which(TOUR_LSP_BINARY).ok_or_else(|| {
            format!(
                "{TOUR_LSP_BINARY} was not found in PATH. Build and install it with \
                 `cargo install --path crates/tour-lsp --locked`."
            )
        })?;

        Ok(Command {
            command,
            args: Vec::new(),
            env: Default::default(),
        })
    }

    fn language_server_initialization_options(
        &mut self,
        language_server_id: &LanguageServerId,
        worktree: &Worktree,
    ) -> Result<Option<Value>> {
        if !Self::is_tour_server(language_server_id) {
            return Ok(None);
        }

        let settings = LspSettings::for_worktree(TOUR_LANGUAGE_SERVER_ID, worktree)?;
        Ok(settings.settings)
    }

    fn language_server_workspace_configuration(
        &mut self,
        language_server_id: &LanguageServerId,
        worktree: &Worktree,
    ) -> Result<Option<Value>> {
        if !Self::is_tour_server(language_server_id) {
            return Ok(None);
        }

        let settings = LspSettings::for_worktree(TOUR_LANGUAGE_SERVER_ID, worktree)?;
        Ok(settings.settings)
    }

    fn language_server_initialization_options_schema(
        &mut self,
        language_server_id: &LanguageServerId,
        _worktree: &Worktree,
    ) -> Option<Value> {
        Self::is_tour_server(language_server_id).then(Self::settings_schema)
    }

    fn language_server_workspace_configuration_schema(
        &mut self,
        language_server_id: &LanguageServerId,
        _worktree: &Worktree,
    ) -> Option<Value> {
        Self::is_tour_server(language_server_id).then(Self::settings_schema)
    }
}

zed::register_extension!(CodeTourExtension);
