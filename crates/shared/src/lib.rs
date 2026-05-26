//! DeepSeek TUI engine library — core logic types re-exported for external crates.
pub use deepseek_base::{mode_types, network_policy, retry_status, context_ref, child_env, command_safety, localization};


pub mod hooks;
pub mod logging;
pub mod artifacts;
pub mod auto_reasoning;
pub mod auto_route;
pub mod capacity;
pub mod config;
pub mod core;
pub mod cost_status;
pub mod dependencies;
pub mod palette;
pub mod pricing;
pub mod project_context;
pub mod prompts;
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