//! Agent runtime types — re-exports from the full implementation in
//! [`super::role`]. All public items that callers used to import from
//! `tools::agent` are now thin re-exports so the module stays the
//! canonical path without duplicating definitions.

pub use super::role::{
    // ── Core types ──
    AgentAssignment,
    AgentCompletion,
    AgentForkContext,
    AgentResult,
    AgentRole,
    AgentRuntime,
    AgentStatus,
    // ── Manager ──
    SharedAgentManager,
    new_shared_agent_manager,
    // ── Constants ──
    DEFAULT_MAX_SPAWN_DEPTH,
    // ── Functions ──
    builtin_role_configs,
    // ── Mailbox ──
    Mailbox,
    MailboxMessage,
    // ── Tool structs (registry registrations) ──
    AgentCloseTool,
    AgentEvalTool,
    AgentOpenTool,
    // ── Session projection ──
    AgentSessionProjection,
    AgentPrefixCacheProjection,
};

// These are `pub(crate)` in role; re-export at the same visibility so
// intra-crate callers can still reach them (used by role/tests.rs).
#[allow(unused_imports)]
pub(crate) use super::role::{
    build_agent_system_prompt,
    resolve_agent_assignment_route,
    AGENT_OUTPUT_FORMAT,
    GENERAL_AGENT_INTRO,
};

/// Re-export the mailbox submodule so callers that do
/// `use crate::tools::agent::mailbox::Mailbox` continue to compile.
pub mod mailbox {
    pub use super::super::role::mailbox::{
        Mailbox,
        MailboxEnvelope,
        MailboxMessage,
        MailboxReceiver,
    };
}
