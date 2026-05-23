//! Toggle command: show/hide sidebar panels or file tree.
//!
//! Provides `/toggle` as a keyboard-shortcut fallback so users can control
//! panel visibility even when the terminal intercepts `Ctrl+Alt+0` for tab
//! switching or when `Ctrl+B` is bound to shell control rather than sidebar
//! toggle (cf. #657).

use super::CommandResult;
use crate::ui::app::{App, SidebarFocus};

pub fn toggle(app: &mut App, arg: Option<&str>) -> CommandResult {
    let sub = arg
        .map(|s| s.trim().to_ascii_lowercase())
        .unwrap_or_default();

    match sub.as_str() {
        "" | "sidebar" | "panel" | "right" => {
            if app.sidebar_focus == SidebarFocus::Hidden {
                app.set_sidebar_focus(SidebarFocus::Auto);
                CommandResult::message("Sidebar: auto")
            } else {
                app.set_sidebar_focus(SidebarFocus::Hidden);
                CommandResult::message("Sidebar hidden")
            }
        }
        "filetree" | "files" | "tree" | "left" | "explorer" => {
            if app.file_tree.is_some() {
                app.file_tree = None;
                app.status_message = Some("File tree closed".to_string());
                app.needs_redraw = true;
                CommandResult::message("File tree closed")
            } else {
                let state = crate::ui::file_tree::FileTreeState::new(&app.workspace);
                app.file_tree = Some(state);
                app.status_message = Some(
                    "File tree: \u{2191}/\u{2193} navigate  Enter select  Esc close".to_string(),
                );
                app.needs_redraw = true;
                CommandResult::message("File tree opened")
            }
        }
        _ => CommandResult::error(format!(
            "Unknown toggle target: {sub}. Use: /toggle [sidebar|filetree]"
        )),
    }
}
