//! Slash command stubs — full implementation on branch `commands-migration`.
//! Provides the minimal API surface that the TUI crate needs to compile.

pub mod share;

/// Minimal action enum.
#[derive(Debug, Clone)]
pub enum CommandAction {
    NoOp,
    SendMessage(String),
    /// Delegate to an App-level action.
    App(crate::app::AppAction),
}

impl From<crate::app::AppAction> for CommandAction {
    fn from(action: crate::app::AppAction) -> Self {
        CommandAction::App(action)
    }
}

/// Result of executing a command.
#[derive(Debug, Clone)]
pub struct CommandResult {
    pub message: Option<String>,
    pub action: Option<CommandAction>,
    pub is_error: bool,
}

impl CommandResult {
    pub fn message(msg: impl Into<String>) -> Self {
        Self {
            message: Some(msg.into()),
            action: None,
            is_error: false,
        }
    }
    pub fn with_message_and_action(msg: impl Into<String>, action: CommandAction) -> Self {
        Self {
            message: Some(msg.into()),
            action: Some(action),
            is_error: false,
        }
    }
    pub fn error(message: impl Into<String>) -> Self {
        Self {
            message: Some(message.into()),
            action: None,
            is_error: true,
        }
    }
}

/// Execute a slash command. Stub that returns "not implemented" for all.
pub fn execute(_cmd: &str, _app: &mut crate::app::App) -> CommandResult {
    CommandResult {
        message: None,
        action: None,
        is_error: false,
    }
}

/// Command metadata for help / autocomplete.
#[derive(Debug, Clone, Copy)]
pub struct CommandInfo {
    pub name: &'static str,
    pub aliases: &'static [&'static str],
    pub usage: &'static str,
}

impl CommandInfo {
    pub fn requires_argument(&self) -> bool { self.usage.contains('<') }
    pub fn palette_command(&self) -> String { format!("/{} ", self.name) }
    pub fn palette_description_for(&self, _locale: deepseek_tui::localization::Locale) -> String { self.usage.to_string() }
    pub fn description_for(&self, _locale: deepseek_tui::localization::Locale) -> &'static str { self.usage }
}

/// Command registry stub — empty for now.
pub static COMMANDS: &[CommandInfo] = &[];

/// Re-export auto-route types so TUI code can use `commands::AutoRouteSelection` etc.
pub use deepseek_tui::auto_route::{
    AutoRouteRecommendation, AutoRouteSelection,
    normalize_auto_route_effort, resolve_auto_route_with_flash,
};

/// Get command info by name.
pub fn get_command_info(_name: &str) -> Option<CommandInfo> { None }

/// Update a configuration value.
pub fn set_config_value(_app: &mut crate::app::App, _key: &str, _value: &str, _persist: bool) -> CommandResult {
    CommandResult { message: None, action: None, is_error: false }
}

/// Persist status items.
pub fn persist_status_items(
    _items: &[deepseek_tui::config::StatusItem],
) -> anyhow::Result<std::path::PathBuf> {
    Ok(std::path::PathBuf::new())
}

/// Persist a root-level string key.
pub fn persist_root_string_key(
    _key: &str, _value: &str,
) -> anyhow::Result<std::path::PathBuf> {
    Ok(std::path::PathBuf::new())
}

/// Auto-select model based on input complexity.
pub fn auto_model_heuristic(_input: &str, _current: &str) -> String { String::new() }

/// Switch TUI mode.
pub fn switch_mode(_app: &mut crate::app::App, _mode: deepseek_tui::mode_types::AppMode) -> CommandResult {
    CommandResult { message: None, action: None, is_error: false }
}

/// Return all command names matching a prefix.
pub fn all_command_names_matching(_prefix: &str, _workspace: &std::path::Path) -> Vec<String> { Vec::new() }
