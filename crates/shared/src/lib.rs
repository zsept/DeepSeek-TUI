//! DeepSeek TUI engine library — core logic types re-exported for external crates.


pub mod artifacts;
pub mod auto_reasoning;
pub mod auto_route;
pub mod capacity;
pub mod child_env;
pub mod command_safety;
pub mod config;
pub mod context_ref;
pub mod core;
pub mod cost_status;
pub mod dependencies;
pub mod hooks;
pub mod localization;
pub mod logging;
pub mod mode_types;
pub mod network_policy;
pub mod palette;
pub mod pricing;
pub mod project_context;
pub mod prompts;
pub mod retry_status;
pub mod runtime;
pub mod session;
pub mod tools;
pub mod utils;
pub mod vision;
pub mod working_set;
pub mod workspace_trust;

pub use context_ref::{ContextReference, ContextReferenceKind, ContextReferenceSource};
pub use auto_route::resolve_cli_auto_route;

#[cfg(test)]
mod test_support;