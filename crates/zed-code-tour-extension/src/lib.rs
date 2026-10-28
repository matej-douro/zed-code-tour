use zed_extension_api::{self as zed, Command, LanguageServerId, Result, Worktree};

const TOUR_LANGUAGE_SERVER_ID: &str = "tour";
const TOUR_LSP_BINARY: &str = "tour-lsp";

struct CodeTourExtension;

impl zed::Extension for CodeTourExtension {
    fn new() -> Self {
        Self
    }

    fn language_server_command(
        &mut self,
        language_server_id: &LanguageServerId,
        worktree: &Worktree,
    ) -> Result<Command> {
        if language_server_id.as_ref() != TOUR_LANGUAGE_SERVER_ID {
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
}

zed::register_extension!(CodeTourExtension);
