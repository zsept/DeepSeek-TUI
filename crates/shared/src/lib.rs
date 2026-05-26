//! DeepSeek TUI engine library — core logic types re-exported for external crates.
pub use deepseek_support::{pricing, utils, dependencies, artifacts, workspace_trust, hooks, logging};
pub use deepseek_base::{mode_types, network_policy, retry_status, context_ref, child_env, command_safety, localization, auto_reasoning};


pub mod auto_route;
pub mod config;
// core migrated to deepseek-engine crate
// project_context migrated to deepseek-context crate
pub mod palette {
    pub use deepseek_palette::*;
}
// vision migrated to deepseek-engine crate
pub mod cost_status {
    pub use deepseek_support::cost_status::*;
}
// prompts migrated to deepseek-context crate
// runtime migrated to deepseek-engine crate
pub mod session {
    pub use deepseek_session_mgr::*;
}
// tools migrated to deepseek-engine crate
// capacity migrated to deepseek-capacity crate
// working_set migrated to deepseek-context crate

pub use auto_route::resolve_cli_auto_route;
pub use context_ref::{ContextReference, ContextReferenceKind, ContextReferenceSource};

#[cfg(test)]
mod test_support;