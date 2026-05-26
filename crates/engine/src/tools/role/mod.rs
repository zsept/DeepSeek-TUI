//! Agent spawning system.
//!
//! Provides tools to spawn background agents, query their status,
//! and retrieve results. Agents run with a filtered toolset and
//! inherit the workspace configuration from the main session.
//!
//! v0.8.33's new model-facing surface is `agent_open` / `agent_eval` /
//! `agent_close`. Some older structs and manager helpers remain in this
//! module while the durable runtime is being reused by the new surface.
#![allow(dead_code)]

use std::collections::{HashMap, VecDeque};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tokio::sync::{Mutex, RwLock};

use anyhow::{Result, anyhow};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio::{sync::mpsc, task::JoinHandle};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::core::client::DeepSeekClient;
use deepseek_config::MAX_CONCURRENT_AGENTS;
use crate::core::events::Event;
use crate::core::llm_client::LlmClient;
use deepseek_models::{ContentBlock, Message, MessageRequest, SystemPrompt, Tool};
use crate::tools::handle::VarHandle;
use crate::tools::plan::{PlanState, SharedPlanState};
use crate::tools::registry::{ToolRegistry, ToolRegistryBuilder};
use crate::tools::spec::{
    ApprovalRequirement, ToolCapability, ToolContext, ToolError, ToolResult, ToolSpec,
    optional_bool, optional_u64, required_str,
};
use crate::tools::todo::{SharedTodoList, TodoList};
use deepseek_support::utils::spawn_supervised;

pub mod mailbox;
#[allow(unused_imports)]
pub use mailbox::{Mailbox, MailboxEnvelope, MailboxMessage, MailboxReceiver};

// === Constants ===

/// Global ownership table for cache-aware resident file agents (#529).
/// Maps file path → agent id. Agents hold a lease on a file while running;
/// the lease is released when the agent reaches a terminal state.
static RESIDENT_LEASES: std::sync::OnceLock<
    std::sync::Mutex<std::collections::HashMap<String, String>>,
> = std::sync::OnceLock::new();

/// Release all resident file leases held by `agent_id`. Called when an
/// agent transitions to a terminal state (completed, failed, cancelled).
fn release_resident_leases_for(agent_id: &str) {
    if let Some(lock) = RESIDENT_LEASES.get()
        && let Ok(mut guard) = lock.lock()
    {
        guard.retain(|_, owner| owner != agent_id);
    }
}

const DEFAULT_MAX_STEPS: u32 = 100;
const TOOL_TIMEOUT: Duration = Duration::from_secs(30);
/// Per-step LLM API call timeout. Each `create_message` request must complete
/// within this window or the step is treated as timed out. Prevents a single
/// stuck API call from blocking the agent indefinitely.
const STEP_API_TIMEOUT: Duration = Duration::from_secs(120);
const RESULT_POLL_INTERVAL: Duration = Duration::from_millis(250);
const DEFAULT_RESULT_TIMEOUT_MS: u64 = 30_000;
#[allow(dead_code)] // Legacy agent_wait clamp; new agent_eval uses DEFAULT/MAX.
const MIN_WAIT_TIMEOUT_MS: u64 = 10_000;
const MAX_RESULT_TIMEOUT_MS: u64 = 3_600_000;
const COMPLETED_AGENT_RETENTION: Duration = Duration::from_secs(60 * 60);
const AGENT_STATE_SCHEMA_VERSION: u32 = 1;
const AGENT_STATE_FILE: &str = "agents.v1.json";
const AGENT_RESTART_REASON: &str = "Interrupted by process restart";

const VALID_AGENT_ROLES: &str = "general, worker, default, \
     or any custom role name defined in roles/<name>/role.toml";
/// Whale species names rotated through `whale_nickname_for_index` to label
/// agents in the UI. English and Simplified-Chinese names are interleaved
/// so any newly spawned agent has a roughly even chance of either — the goal
/// is friendly variety, not a strict locale match.
pub const WHALE_NICKNAMES: &[&str] = &[
    "Blue",
    "蓝鲸",
    "Humpback",
    "座头鲸",
    "Sperm",
    "抹香鲸",
    "Fin",
    "长须鲸",
    "Sei",
    "塞鲸",
    "Bryde's",
    "布氏鲸",
    "Minke",
    "小须鲸",
    "Antarctic Minke",
    "南极小须鲸",
    "Gray",
    "灰鲸",
    "Bowhead",
    "弓头鲸",
    "North Atlantic Right",
    "北大西洋露脊鲸",
    "North Pacific Right",
    "北太平洋露脊鲸",
    "Southern Right",
    "南露脊鲸",
    "Beluga",
    "白鲸",
    "Narwhal",
    "独角鲸",
    "Orca",
    "虎鲸",
    "Pilot",
    "领航鲸",
    "False Killer",
    "伪虎鲸",
    "Pygmy Killer",
    "小虎鲸",
    "Melon-headed",
    "瓜头鲸",
    "Beaked",
    "喙鲸",
    "Cuvier's Beaked",
    "柯氏喙鲸",
    "Baird's Beaked",
    "贝氏喙鲸",
    "Blainville's Beaked",
    "柏氏喙鲸",
];

/// Removal version for deprecated tool aliases.
const DEPRECATION_REMOVAL_VERSION: &str = "0.8.0";

#[must_use]
pub fn whale_nickname_for_index(index: usize) -> String {
    let base = WHALE_NICKNAMES[index % WHALE_NICKNAMES.len()];
    if index < WHALE_NICKNAMES.len() {
        base.to_string()
    } else {
        format!("{base} {}", index / WHALE_NICKNAMES.len() + 1)
    }
}

// === Deprecation helpers ===

/// Wrap a `ToolResult` with a `_deprecation` block in its metadata.
///
/// Applied exclusively on alias paths (not on canonical tool names) so the
/// model can detect and migrate away from the old name before removal in
/// v`DEPRECATION_REMOVAL_VERSION`.
///
/// The `_deprecation` key is merged into any existing metadata so other
/// metadata (e.g. `status`, `timed_out`) is preserved unchanged.
fn wrap_with_deprecation_notice(
    mut result: ToolResult,
    this_tool: &str,
    use_instead: &str,
) -> ToolResult {
    tracing::warn!(
        "Deprecated tool '{}' invoked — use '{}' instead (removal: v{})",
        this_tool,
        use_instead,
        DEPRECATION_REMOVAL_VERSION,
    );

    let notice = json!({
        "_deprecation": {
            "this_tool": this_tool,
            "use_instead": use_instead,
            "removed_in": DEPRECATION_REMOVAL_VERSION,
            "message": format!(
                "Tool '{}' is deprecated; switch to '{}' before v{}.",
                this_tool, use_instead, DEPRECATION_REMOVAL_VERSION
            )
        }
    });

    result.metadata = Some(match result.metadata.take() {
        Some(Value::Object(mut map)) => {
            if let Value::Object(notice_map) = notice {
                map.extend(notice_map);
            }
            Value::Object(map)
        }
        Some(other) => {
            // Existing metadata was not an object — keep it as-is and add
            // the deprecation notice as a sibling under a wrapper.
            json!({ "_deprecation": notice["_deprecation"].clone(), "_original_metadata": other })
        }
        None => notice,
    });

    result
}

// === Types ===

/// Assignment metadata for agent orchestration.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AgentAssignment {
    pub objective: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
}

impl AgentAssignment {
    fn new(objective: String, role: Option<String>) -> Self {
        Self { objective, role }
    }
}

/// Agent execution type.
///
/// Only `General` and user-defined custom types (`Named`) remain here.
/// All other built-in postures (explore, plan, review, implementer, verifier,
/// custom) are now defined as default configurations in
/// `crates/tui/assets/builtin-roles/` and resolved at runtime via
/// `AgentRoleConfig`.
#[derive(Debug, Clone, Serialize, PartialEq, Default)]
#[serde(rename_all = "snake_case")]
pub enum AgentRole {
    /// General purpose - full tool access for multi-step tasks.
    #[default]
    General,
    /// User-defined or built-in role resolved from config.
    /// Carries the type name so the spawn path can look up
    /// system prompt, allowed tools, model, and knowledge paths.
    Named(String),
}

impl<'de> Deserialize<'de> for AgentRole {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        struct AgentRoleVisitor;

        impl<'de> serde::de::Visitor<'de> for AgentRoleVisitor {
            type Value = AgentRole;

            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("a string like \"general\" or an object {\"named\": \"...\"}")
            }

            fn visit_str<E: serde::de::Error>(self, v: &str) -> Result<AgentRole, E> {
                Ok(AgentRole::from_str(v).unwrap_or_else(|| AgentRole::Named(v.to_string())))
            }

            fn visit_map<M: serde::de::MapAccess<'de>>(self, mut map: M) -> Result<AgentRole, M::Error> {
                let mut name: Option<String> = None;
                while let Some(key) = map.next_key::<String>()? {
                    match key.as_str() {
                        "named" => {
                            if name.is_some() {
                                return Err(serde::de::Error::duplicate_field("named"));
                            }
                            name = Some(map.next_value()?);
                        }
                        other => {
                            return Err(serde::de::Error::unknown_field(other, &["named"]));
                        }
                    }
                }
                match name {
                    Some(n) => Ok(AgentRole::Named(n)),
                    None => Err(serde::de::Error::missing_field("named")),
                }
            }
        }

        deserializer.deserialize_any(AgentRoleVisitor)
    }
}

impl AgentRole {
    /// Parse an agent type from user input.
    #[must_use]
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "general" | "general-purpose" | "general_purpose" | "worker" | "default"
                => Some(Self::General),
            // Canonicalize built-in aliases to their config names.
            "explore" | "exploration" | "explorer"
                => Some(Self::Named("explore".to_string())),
            "plan" | "planning" | "awaiter"
                => Some(Self::Named("plan".to_string())),
            "review" | "code-review" | "code_review" | "reviewer"
                => Some(Self::Named("review".to_string())),
            "implementer" | "implement" | "implementation" | "builder"
                => Some(Self::Named("implementer".to_string())),
            "verifier" | "verify" | "verification" | "validator" | "tester"
                => Some(Self::Named("verifier".to_string())),
            "custom" => Some(Self::Named("custom".to_string())),
            "" => None,
            other => Some(Self::Named(other.to_string())),
        }
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        match self {
            Self::General => "general",
            Self::Named(name) => name.as_str(),
        }
    }

}

/// Status of a agent execution.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum AgentStatus {
    Running,
    Completed,
    Interrupted(String),
    Failed(String),
    Cancelled,
}

/// Snapshot of agent state for tool results.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentResult {
    pub name: String,
    pub agent_id: String,
    pub context_mode: String,
    pub fork_context: bool,
    pub agent_type: AgentRole,
    pub assignment: AgentAssignment,
    #[serde(default)]
    pub model: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub nickname: Option<String>,
    pub status: AgentStatus,
    pub result: Option<String>,
    pub steps_taken: u32,
    pub duration_ms: u64,
    /// `true` when this agent was loaded from a prior-session persisted
    /// state file rather than spawned in the current session (#405).
    /// Lets `agent_list` filter out historical noise by default while
    /// keeping the records reachable via `include_archived=true`.
    #[serde(default, skip_serializing_if = "is_false")]
    pub from_prior_session: bool,
}

fn is_false(b: &bool) -> bool {
    !*b
}

#[derive(Debug, Clone, Default)]
pub(crate) struct AgentSpawnOptions {
    pub name: Option<String>,
    pub model: Option<String>,
    pub nickname: Option<String>,
    pub fork_context: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WaitMode {
    Any,
    All,
}

impl WaitMode {
    fn from_str(value: &str) -> Option<Self> {
        match value.to_ascii_lowercase().as_str() {
            "any" | "first" => Some(Self::Any),
            "all" => Some(Self::All),
            _ => None,
        }
    }

    #[allow(dead_code)] // Legacy wait metadata while registry moves to agent_eval.
    fn as_str(self) -> &'static str {
        match self {
            Self::Any => "any",
            Self::All => "all",
        }
    }

    fn condition_met(self, snapshots: &[AgentResult]) -> bool {
        match self {
            Self::Any => snapshots
                .iter()
                .any(|snapshot| snapshot.status != AgentStatus::Running),
            Self::All => snapshots
                .iter()
                .all(|snapshot| snapshot.status != AgentStatus::Running),
        }
    }
}

#[derive(Debug, Clone)]
struct AgentInput {
    text: String,
    interrupt: bool,
}

#[derive(Debug, Clone)]
struct SpawnRequest {
    session_name: Option<String>,
    prompt: String,
    agent_type: AgentRole,
    assignment: AgentAssignment,
    allowed_tools: Option<Vec<String>>,
    model: Option<String>,
    /// Optional working directory for the child. Must canonicalize to a
    /// path inside the parent's workspace. Used to dispatch parallel work
    /// into separate git worktrees: parent runs `git worktree add` first,
    /// then spawns children with the worktree path as `cwd`.
    cwd: Option<PathBuf>,
    /// Optional file path for cache-aware resident mode (#529). When set,
    /// the child's prompt is prefixed with the file contents for prefix-cache
    /// locality. A global ownership table prevents two agents from holding
    /// a resident lease on the same file simultaneously.
    resident_file: Option<String>,
    /// When true, seed the child with the parent's system prompt and message
    /// prefix before appending the child task.
    fork_context: bool,
    /// Optional recursion budget for descendants opened by this child.
    /// `0` means the child may not call `agent_open` recursively.
    max_depth: Option<u32>,
}

#[derive(Debug, Clone)]
struct AssignRequest {
    agent_id: String,
    objective: Option<String>,
    role: Option<String>,
    message: Option<String>,
    interrupt: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PersistedAgent {
    id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    session_name: Option<String>,
    #[serde(default)]
    fork_context: bool,
    agent_type: AgentRole,
    prompt: String,
    assignment: AgentAssignment,
    #[serde(default)]
    model: String,
    #[serde(default)]
    nickname: Option<String>,
    status: AgentStatus,
    result: Option<String>,
    steps_taken: u32,
    duration_ms: u64,
    allowed_tools: Vec<String>,
    updated_at_ms: u64,
    /// Stable id of the manager / process boot that spawned this agent
    /// (#405). Lets a fresh manager filter out agents that were
    /// persisted by a prior session. Optional with `#[serde(default)]`
    /// for backward compatibility — older records lack the field and
    /// load with an empty string, which the manager treats as
    /// "from_prior_session" because it can't match any current id.
    #[serde(default)]
    session_boot_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PersistedAgentState {
    schema_version: u32,
    agents: Vec<PersistedAgent>,
}

impl Default for PersistedAgentState {
    fn default() -> Self {
        Self {
            schema_version: AGENT_STATE_SCHEMA_VERSION,
            agents: Vec::new(),
        }
    }
}

/// Default cap on agent recursion depth. Override via
/// `[runtime] max_spawn_depth = N` in `~/.deepseek/config.toml`.
pub const DEFAULT_MAX_SPAWN_DEPTH: u32 = 3;

/// Terminal-state notification emitted to the engine's parent turn loop
/// when one of its direct children finishes (issue #756). Carries the
/// already-rendered `<deepseek:agent.done>` sentinel that the model
/// expects in the transcript per `prompts/base.md`.
#[derive(Debug, Clone)]
pub struct AgentCompletion {
    /// The completing child's agent id. Held for routing/logging — the
    /// engine's turn loop does not currently key on it (it just injects
    /// the payload), but downstream tooling and tests need the field.
    #[allow(dead_code)]
    pub agent_id: String,
    /// Human summary on line 1, sentinel on line 2. Same payload shape as
    /// `Event::AgentComplete::result`.
    pub payload: String,
}

/// Parent transcript snapshot available to agents that opt into context
/// forking. The system prompt and leading messages are kept byte-identical to
/// the parent request so DeepSeek's prefix cache can reuse the warmed prefix.
#[derive(Clone, Debug)]
pub struct AgentForkContext {
    pub system: Option<SystemPrompt>,
    pub messages: Vec<Message>,
    pub structured_state_block: Option<String>,
}

/// Runtime configuration for spawning agents.
///
/// Carries everything a child needs to (a) build its own tool registry —
/// including the manager so grandchildren can spawn — and (b) cooperate
/// with the rest of the spawn tree on cancellation and depth cap.
#[derive(Clone)]
pub struct AgentRuntime {
    pub client: DeepSeekClient,
    pub model: String,
    pub auto_model: bool,
    pub reasoning_effort: Option<String>,
    pub reasoning_effort_auto: bool,
    pub role_models: HashMap<String, String>,
    pub context: ToolContext,
    pub allow_shell: bool,
    pub event_tx: Option<mpsc::Sender<Event>>,
    /// Manager handle so children can recurse via `agent_spawn`. All agents
    /// at every depth share the same manager.
    pub manager: SharedAgentManager,
    /// Depth in the spawn tree. 0 = top-level user turn; 1 = direct child;
    /// etc. Children clone the parent runtime and increment this on spawn.
    pub spawn_depth: u32,
    /// Hard cap on recursion depth. A child whose `spawn_depth + 1` would
    /// exceed this is rejected at the spawn entry. Use `>` (strictly
    /// greater than) so equality is allowed — matches codex's pattern.
    pub max_spawn_depth: u32,
    /// Cooperative cancellation token. Children derive a child_token() from
    /// the parent so cancelling the root cascades down.
    pub cancel_token: CancellationToken,
    /// Structured progress / lifecycle stream. Cloned across children so the
    /// whole spawn tree publishes into one ordered, fan-out-able mailbox.
    /// `None` only when no consumer is wired (legacy entry points / tests).
    pub mailbox: Option<Mailbox>,
    /// Wakeup channel for the engine's parent turn loop (issue #756). Only
    /// the engine's direct children fire on this — propagated to descendants
    /// via clone but gated to `spawn_depth == 1` at the send site so the
    /// parent isn't flooded with grandchild completions it didn't directly
    /// orchestrate. `None` when no consumer is wired (tests / legacy paths).
    pub parent_completion_tx: Option<mpsc::UnboundedSender<AgentCompletion>>,
    /// Snapshot of the request prefix visible to an opt-in forked child.
    pub fork_context: Option<AgentForkContext>,
    /// User-defined custom type definitions from `roles/<name>/role.toml`.
    /// Used at spawn time to resolve system prompts, allowed tools,
    /// model, reasoning effort, and knowledge paths for `Named` types.
    pub role_configs: std::collections::HashMap<String, deepseek_config::AgentRoleConfig>,
}

impl AgentRuntime {
    /// Create a top-level runtime configuration for agent execution.
    /// Use this from the engine when constructing the runtime that the
    /// parent's tool registry passes through. Children should derive their
    /// runtime via `Self::child_runtime` instead.
    #[must_use]
    pub fn new(
        client: DeepSeekClient,
        model: String,
        context: ToolContext,
        allow_shell: bool,
        event_tx: Option<mpsc::Sender<Event>>,
        manager: SharedAgentManager,
    ) -> Self {
        Self {
            client,
            model,
            auto_model: false,
            reasoning_effort: None,
            reasoning_effort_auto: false,
            role_models: HashMap::new(),
            context,
            allow_shell,
            event_tx,
            manager,
            spawn_depth: 0,
            max_spawn_depth: DEFAULT_MAX_SPAWN_DEPTH,
            cancel_token: CancellationToken::new(),
            mailbox: None,
            parent_completion_tx: None,
            fork_context: None,
            role_configs: HashMap::new(),
        }
    }

    /// Attach the wakeup channel so the engine's parent turn loop can resume
    /// when this runtime's direct children finish (issue #756). The channel
    /// is propagated to descendants via clone, but only `spawn_depth == 1`
    /// agents fire on it — see `run_agent_task`.
    #[must_use]
    pub fn with_parent_completion_tx(
        mut self,
        tx: mpsc::UnboundedSender<AgentCompletion>,
    ) -> Self {
        self.parent_completion_tx = Some(tx);
        self
    }

    /// Attach the current parent request prefix for `fork_context` spawns.
    #[must_use]
    pub fn with_fork_context(mut self, context: AgentForkContext) -> Self {
        self.fork_context = Some(context);
        self
    }

    /// Attach a `Mailbox` so this runtime (and every descendant — children
    /// clone it) publishes structured `MailboxMessage` envelopes alongside
    /// the legacy `Event` stream. Pair with [`Self::with_cancel_token`] when
    /// you want close-as-cancel to propagate the same way.
    #[must_use]
    #[allow(dead_code)] // wired by #128 (in-transcript cards) when it lands.
    pub fn with_mailbox(mut self, mailbox: Mailbox) -> Self {
        self.mailbox = Some(mailbox);
        self
    }

    /// Replace the cancellation token (e.g. when the engine constructs the
    /// runtime alongside a mailbox bound to the same token).
    #[must_use]
    #[allow(dead_code)] // wired by #128 alongside `with_mailbox`.
    pub fn with_cancel_token(mut self, token: CancellationToken) -> Self {
        self.cancel_token = token;
        self
    }

    /// Override the maximum spawn depth (default `DEFAULT_MAX_SPAWN_DEPTH`).
    /// Used by config wiring (`[runtime] max_spawn_depth = N`) and tests.
    #[must_use]
    #[allow(dead_code)]
    pub fn with_max_spawn_depth(mut self, max: u32) -> Self {
        self.max_spawn_depth = max;
        self
    }

    /// Attach raw role/type model overrides. Values are intentionally
    /// validated at spawn time so bad config fails before a partial spawn.
    #[must_use]
    pub fn with_role_models(mut self, role_models: HashMap<String, String>) -> Self {
        self.role_models = role_models;
        self
    }

    /// Attach user-defined custom type definitions from `roles/<name>/role.toml`.
    #[must_use]
    pub fn with_role_configs(
        mut self,
        configs: std::collections::HashMap<String, deepseek_config::AgentRoleConfig>,
    ) -> Self {
        self.role_configs = configs;
        self
    }

    /// Preserve whether the parent session is using per-turn model routing.
    #[must_use]
    pub fn with_auto_model(mut self, auto_model: bool) -> Self {
        self.auto_model = auto_model;
        self
    }

    /// Preserve the parent's thinking configuration. `reasoning_effort_auto`
    /// stays true even when the parent turn itself was sent with a concrete
    /// flash-router recommendation, so children can resolve their own tier.
    #[must_use]
    pub fn with_reasoning_effort(
        mut self,
        reasoning_effort: Option<String>,
        reasoning_effort_auto: bool,
    ) -> Self {
        self.reasoning_effort = reasoning_effort;
        self.reasoning_effort_auto = reasoning_effort_auto;
        self
    }

    /// Return a child runtime that is deliberately detached from the parent
    /// turn cancellation token. Background agents should keep running when
    /// the parent turn is cancelled; explicit agent cancellation still
    /// aborts their task handles through the manager.
    #[must_use]
    pub fn background_runtime(&self) -> Self {
        let mut runtime = self.child_runtime();
        let token = CancellationToken::new();
        runtime.cancel_token = token.clone();
        runtime.context.cancel_token = Some(token);
        runtime
    }

    /// Build a child runtime cloning this one, incrementing `spawn_depth`,
    /// and deriving a child cancellation token. Used at spawn entry to
    /// construct the runtime the new agent will see.
    ///
    /// Children inherit the parent's approval state. A non-auto parent can
    /// still delegate read-only investigation, but approval-gated child tools
    /// are blocked by the agent registry instead of being silently run
    /// without a prompt.
    #[must_use]
    pub fn child_runtime(&self) -> Self {
        let mut child_context = self.context.clone();
        child_context.auto_approve = self.context.auto_approve;
        Self {
            client: self.client.clone(),
            model: self.model.clone(),
            auto_model: self.auto_model,
            reasoning_effort: self.reasoning_effort.clone(),
            reasoning_effort_auto: self.reasoning_effort_auto,
            role_models: self.role_models.clone(),
            context: child_context,
            allow_shell: self.allow_shell,
            event_tx: self.event_tx.clone(),
            manager: self.manager.clone(),
            spawn_depth: self.spawn_depth + 1,
            max_spawn_depth: self.max_spawn_depth,
            cancel_token: self.cancel_token.child_token(),
            mailbox: self.mailbox.clone(),
            role_configs: self.role_configs.clone(),
            parent_completion_tx: self.parent_completion_tx.clone(),
            fork_context: self.fork_context.clone(),
        }
    }

    /// Whether the next spawn would exceed the depth cap.
    #[must_use]
    pub fn would_exceed_depth(&self) -> bool {
        self.spawn_depth + 1 > self.max_spawn_depth
    }
}

/// A running agent instance.
pub struct Agent {
    pub id: String,
    pub session_name: String,
    pub fork_context: bool,
    pub agent_type: AgentRole,
    pub prompt: String,
    pub assignment: AgentAssignment,
    pub model: String,
    pub nickname: Option<String>,
    pub status: AgentStatus,
    pub result: Option<String>,
    pub steps_taken: u32,
    pub started_at: Instant,
    /// `None` = full registry inheritance, with approval-gated tools still
    /// blocked unless the parent runtime is auto-approved.
    /// `Some(list)` = explicit narrow allowlist (Custom agents, legacy).
    pub allowed_tools: Option<Vec<String>>,
    /// Stable id of the manager that spawned this agent (#405). Compared
    /// against the manager's `current_session_boot_id` to classify the
    /// agent as in-session vs prior-session at list time.
    pub session_boot_id: String,
    input_tx: Option<mpsc::UnboundedSender<AgentInput>>,
    task_handle: Option<JoinHandle<()>>,
}

impl Agent {
    /// Create a new agent.
    #[allow(clippy::too_many_arguments)]
    fn new(
        agent_type: AgentRole,
        prompt: String,
        assignment: AgentAssignment,
        model: String,
        nickname: Option<String>,
        allowed_tools: Option<Vec<String>>,
        input_tx: mpsc::UnboundedSender<AgentInput>,
        session_boot_id: String,
    ) -> Self {
        let id = format!("agent_{}", &Uuid::new_v4().to_string()[..8]);
        let session_name = id.clone();

        Self {
            id,
            session_name,
            fork_context: false,
            agent_type,
            prompt,
            assignment,
            model,
            nickname,
            status: AgentStatus::Running,
            result: None,
            steps_taken: 0,
            started_at: Instant::now(),
            allowed_tools,
            session_boot_id,
            input_tx: Some(input_tx),
            task_handle: None,
        }
    }

    /// Get a snapshot of the current state.
    #[must_use]
    pub fn snapshot(&self) -> AgentResult {
        AgentResult {
            name: self.session_name.clone(),
            agent_id: self.id.clone(),
            context_mode: if self.fork_context { "forked" } else { "fresh" }.to_string(),
            fork_context: self.fork_context,
            agent_type: self.agent_type.clone(),
            assignment: self.assignment.clone(),
            model: self.model.clone(),
            nickname: self.nickname.clone(),
            status: self.status.clone(),
            result: self.result.clone(),
            steps_taken: self.steps_taken,
            duration_ms: u64::try_from(self.started_at.elapsed().as_millis()).unwrap_or(u64::MAX),
            // Snapshots from the agent itself don't know the manager's
            // current boot id, so default to false. The manager fills
            // this in when it produces a snapshot via its own
            // `snapshot_for_listing` helper (#405).
            from_prior_session: false,
        }
    }
}

/// Manager for active agents.
pub struct AgentManager {
    agents: HashMap<String, Agent>,
    #[allow(dead_code)] // Stored for future workspace-scoped operations
    workspace: PathBuf,
    state_path: Option<PathBuf>,
    max_steps: u32,
    max_agents: usize,
    /// Stable id assigned at manager construction (#405). Stamped on
    /// every agent the manager spawns; agents loaded from the
    /// persisted state file carry whatever id the prior session
    /// stamped (or empty for pre-#405 records). The manager classifies
    /// agents whose `session_boot_id` doesn't match this value as
    /// "from prior session" so `agent_list` can hide them by default.
    current_session_boot_id: String,
}

impl AgentManager {
    /// Create a new manager for agents.
    #[must_use]
    pub fn new(workspace: PathBuf, max_agents: usize) -> Self {
        Self {
            agents: HashMap::new(),
            workspace,
            state_path: None,
            max_steps: DEFAULT_MAX_STEPS,
            max_agents,
            // Fresh boot id per manager. Used by #405 to classify
            // re-loaded persisted agents as "prior session".
            current_session_boot_id: format!("boot_{}", &Uuid::new_v4().to_string()[..12]),
        }
    }

    /// Return the boot id this manager stamps on agents it spawns.
    /// Exposed for tests; internal callers use the field directly.
    #[cfg(test)]
    pub fn session_boot_id(&self) -> &str {
        &self.current_session_boot_id
    }

    /// Classify an agent by its `session_boot_id`: `true` when the
    /// agent was either (a) loaded from disk with no id, or (b) carries
    /// a different id than the manager's current boot. Filters
    /// `agent_list` output by default (#405).
    fn is_from_prior_session(&self, agent: &Agent) -> bool {
        agent.session_boot_id.is_empty() || agent.session_boot_id != self.current_session_boot_id
    }

    #[must_use]
    fn with_state_path(mut self, path: PathBuf) -> Self {
        self.state_path = Some(path);
        self
    }

    fn persist_state(&self) -> Result<()> {
        let Some(path) = self.state_path.as_ref() else {
            return Ok(());
        };
        let now_ms = epoch_millis_now();
        let mut agents = Vec::with_capacity(self.agents.len());
        for agent in self.agents.values() {
            agents.push(PersistedAgent {
                id: agent.id.clone(),
                session_name: Some(agent.session_name.clone()),
                fork_context: agent.fork_context,
                agent_type: agent.agent_type.clone(),
                prompt: agent.prompt.clone(),
                assignment: agent.assignment.clone(),
                model: agent.model.clone(),
                nickname: agent.nickname.clone(),
                status: agent.status.clone(),
                result: agent.result.clone(),
                steps_taken: agent.steps_taken,
                duration_ms: u64::try_from(agent.started_at.elapsed().as_millis())
                    .unwrap_or(u64::MAX),
                // Backward-compat: Vec on disk. None → empty vec; Some(list) → list.
                // Reload converts empty vec back to None (full inheritance).
                allowed_tools: agent.allowed_tools.clone().unwrap_or_default(),
                updated_at_ms: now_ms,
                session_boot_id: agent.session_boot_id.clone(),
            });
        }
        agents.sort_by(|a, b| a.id.cmp(&b.id));

        let payload = PersistedAgentState {
            schema_version: AGENT_STATE_SCHEMA_VERSION,
            agents,
        };
        write_json_atomic(path, &payload)
    }

    fn persist_state_best_effort(&self) {
        if let Err(err) = self.persist_state() {
            // Must not be `eprintln!` — raw stderr inside the alt-screen
            // leaks into the buffer and produces the scroll-demon
            // regression (#1085). Routed through tracing so the
            // file-backed subscriber in `runtime_log` captures it.
            tracing::warn!(target: "agent", ?err, "failed to persist agent state");
        }
    }

    fn load_state(&mut self) -> Result<()> {
        let Some(path) = self.state_path.as_ref() else {
            return Ok(());
        };
        if !path.exists() {
            return Ok(());
        }

        let raw = fs::read_to_string(path)?;
        let state = serde_json::from_str::<PersistedAgentState>(&raw)?;
        if state.schema_version != AGENT_STATE_SCHEMA_VERSION {
            return Err(anyhow!(
                "Unsupported agent state schema {}",
                state.schema_version
            ));
        }

        self.agents.clear();
        for persisted in state.agents {
            let mut status = persisted.status;
            if matches!(status, AgentStatus::Running) {
                status = AgentStatus::Interrupted(AGENT_RESTART_REASON.to_string());
            }

            let started_at = instant_from_duration(Duration::from_millis(persisted.duration_ms));
            // Empty vec on disk → None (full inheritance, v0.6.6 default).
            // Non-empty vec → Some(list) (preserves narrow scope from older sessions).
            let allowed_tools = if persisted.allowed_tools.is_empty() {
                None
            } else {
                Some(persisted.allowed_tools)
            };
            let agent = Agent {
                id: persisted.id.clone(),
                session_name: persisted
                    .session_name
                    .filter(|name| !name.trim().is_empty())
                    .unwrap_or_else(|| persisted.id.clone()),
                fork_context: persisted.fork_context,
                agent_type: persisted.agent_type,
                prompt: persisted.prompt,
                assignment: persisted.assignment,
                model: if persisted.model.is_empty() {
                    "unknown".to_string()
                } else {
                    persisted.model
                },
                nickname: persisted.nickname,
                status,
                result: persisted.result,
                steps_taken: persisted.steps_taken,
                started_at,
                allowed_tools,
                // Empty string when loading pre-#405 records; the
                // manager treats that the same as a non-matching id —
                // i.e. agent classified as prior-session.
                session_boot_id: persisted.session_boot_id,
                input_tx: None,
                task_handle: None,
            };
            self.agents.insert(persisted.id, agent);
        }

        Ok(())
    }

    /// Count running agents.
    pub fn running_count(&self) -> usize {
        self.agents
            .values()
            .filter(|agent| {
                // Exclude non-running statuses
                if agent.status != AgentStatus::Running {
                    return false;
                }
                // Exclude persisted agents with no task_handle (they're not actually running)
                let Some(handle) = agent.task_handle.as_ref() else {
                    return false;
                };
                // Exclude agents whose task has finished (status will be updated to Completed shortly)
                !handle.is_finished()
            })
            .count()
    }

    /// Spawn a new background agent.
    pub fn spawn_background(
        &mut self,
        manager_handle: SharedAgentManager,
        runtime: AgentRuntime,
        agent_type: AgentRole,
        prompt: String,
        allowed_tools: Option<Vec<String>>,
    ) -> Result<AgentResult> {
        self.spawn_background_with_assignment(
            manager_handle,
            runtime,
            agent_type,
            prompt.clone(),
            AgentAssignment::new(prompt, None),
            allowed_tools,
        )
    }

    /// Spawn a new background agent with explicit assignment metadata.
    pub fn spawn_background_with_assignment(
        &mut self,
        manager_handle: SharedAgentManager,
        runtime: AgentRuntime,
        agent_type: AgentRole,
        prompt: String,
        assignment: AgentAssignment,
        allowed_tools: Option<Vec<String>>,
    ) -> Result<AgentResult> {
        self.spawn_background_with_assignment_options(
            manager_handle,
            runtime,
            agent_type,
            prompt,
            assignment,
            allowed_tools,
            AgentSpawnOptions::default(),
        )
    }

    /// Spawn a new background agent with explicit assignment and display
    /// metadata.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn spawn_background_with_assignment_options(
        &mut self,
        manager_handle: SharedAgentManager,
        mut runtime: AgentRuntime,
        agent_type: AgentRole,
        prompt: String,
        assignment: AgentAssignment,
        allowed_tools: Option<Vec<String>>,
        options: AgentSpawnOptions,
    ) -> Result<AgentResult> {
        self.cleanup(COMPLETED_AGENT_RETENTION);

        if self.running_count() >= self.max_agents {
            return Err(anyhow!(
                "Agent limit reached (max {}, running {}). Cancel, close, or wait for an existing agent to finish. Consider issuing multiple tool calls in one turn (the dispatcher runs them in parallel) for parallel one-shot work.",
                self.max_agents,
                self.running_count()
            ));
        }

        if let Some(model) = options.model.as_deref() {
            runtime.model = model.to_string();
        }
        let effective_model = runtime.model.clone();
        let nickname = options
            .nickname
            .or_else(|| Some(whale_nickname_for_index(self.agents.len())));
        let tools = build_allowed_tools(&agent_type, allowed_tools, runtime.allow_shell)?;
        let (input_tx, input_rx) = mpsc::unbounded_channel();
        let mut agent = Agent::new(
            agent_type.clone(),
            prompt.clone(),
            assignment.clone(),
            effective_model,
            nickname,
            tools.clone(),
            input_tx,
            self.current_session_boot_id.clone(),
        );
        if let Some(name) = options
            .name
            .as_deref()
            .map(str::trim)
            .filter(|name| !name.is_empty())
        {
            if self
                .agents
                .values()
                .any(|existing| existing.session_name == name)
            {
                return Err(anyhow!("Agent session name '{name}' is already in use"));
            }
            agent.session_name = name.to_string();
        }
        agent.fork_context = options.fork_context;
        let agent_id = agent.id.clone();
        let started_at = agent.started_at;
        let max_steps = self.max_steps;

        if let Some(event_tx) = runtime.event_tx.clone() {
            let _ = event_tx.try_send(Event::AgentSpawned {
                id: agent_id.clone(),
                prompt: prompt.clone(),
            });
        }

        let task = AgentTask {
            manager_handle,
            runtime,
            agent_id: agent_id.clone(),
            agent_type,
            prompt,
            assignment,
            allowed_tools: tools,
            fork_context: options.fork_context,
            started_at,
            max_steps,
            input_rx,
        };
        let handle = spawn_supervised(
            "agent-task",
            std::panic::Location::caller(),
            run_agent_task(task),
        );
        agent.task_handle = Some(handle);
        self.agents.insert(agent_id.clone(), agent);
        self.persist_state_best_effort();

        Ok(self
            .agents
            .get(&agent_id)
            .expect("agent should exist after spawn")
            .snapshot())
    }

    /// Get the current snapshot for an agent.
    pub fn get_result(&self, agent_id: &str) -> Result<AgentResult> {
        let agent = self
            .agents
            .get(agent_id)
            .ok_or_else(|| anyhow!("Agent {agent_id} not found"))?;
        Ok(agent.snapshot())
    }

    /// Resolve either a durable agent id or a model-facing session name.
    fn resolve_agent_ref(&self, agent_ref: &str) -> Result<String> {
        let agent_ref = agent_ref.trim();
        if self.agents.contains_key(agent_ref) {
            return Ok(agent_ref.to_string());
        }

        let matches = self
            .agents
            .values()
            .filter(|agent| agent.session_name == agent_ref)
            .map(|agent| agent.id.clone())
            .collect::<Vec<_>>();

        match matches.as_slice() {
            [id] => Ok(id.clone()),
            [] => Err(anyhow!("Agent session {agent_ref} not found")),
            _ => Err(anyhow!(
                "Agent session name '{agent_ref}' is ambiguous; use an agent_id"
            )),
        }
    }

    /// Cancel a running agent.
    pub fn cancel(&mut self, agent_id: &str) -> Result<AgentResult> {
        let (snapshot, changed) = {
            let agent = self
                .agents
                .get_mut(agent_id)
                .ok_or_else(|| anyhow!("Agent {agent_id} not found"))?;

            let mut changed = false;
            if agent.status == AgentStatus::Running {
                agent.status = AgentStatus::Cancelled;
                release_resident_leases_for(&agent.id);
                if let Some(handle) = agent.task_handle.take() {
                    handle.abort();
                }
                changed = true;
            }
            (agent.snapshot(), changed)
        };

        if changed {
            self.persist_state_best_effort();
        }
        Ok(snapshot)
    }

    /// Resume a non-running agent by restarting it with the original assignment.
    #[allow(dead_code)] // Legacy agent_resume path; retained until registry migration.
    pub fn resume(
        &mut self,
        manager_handle: SharedAgentManager,
        runtime: AgentRuntime,
        agent_id: &str,
    ) -> Result<AgentResult> {
        let status = self
            .agents
            .get(agent_id)
            .ok_or_else(|| anyhow!("Agent {agent_id} not found"))?
            .status
            .clone();

        if status == AgentStatus::Running {
            let agent = self
                .agents
                .get(agent_id)
                .ok_or_else(|| anyhow!("Agent {agent_id} not found"))?;
            return Ok(agent.snapshot());
        }

        if self.running_count() >= self.max_agents {
            return Err(anyhow!(
                "Agent limit reached (max {}, running {}). Close or wait for an existing agent before resuming. Consider issuing multiple tool calls in one turn (the dispatcher runs them in parallel) for parallel one-shot work.",
                self.max_agents,
                self.running_count()
            ));
        }

        let snapshot = {
            let agent = self
                .agents
                .get_mut(agent_id)
                .ok_or_else(|| anyhow!("Agent {agent_id} not found"))?;

            let (input_tx, input_rx) = mpsc::unbounded_channel();
            let restarted_at = Instant::now();
            let mut restart_runtime = runtime.clone();
            if !agent.model.trim().is_empty() && agent.model != "unknown" {
                restart_runtime.model.clone_from(&agent.model);
            }
            let task = AgentTask {
                manager_handle,
                runtime: restart_runtime,
                agent_id: agent.id.clone(),
                agent_type: agent.agent_type.clone(),
                prompt: agent.prompt.clone(),
                assignment: agent.assignment.clone(),
                allowed_tools: agent.allowed_tools.clone(),
                fork_context: false,
                started_at: restarted_at,
                max_steps: self.max_steps,
                input_rx,
            };
            let handle = spawn_supervised(
                "agent-task-resume",
                std::panic::Location::caller(),
                run_agent_task(task),
            );

            agent.status = AgentStatus::Running;
            agent.result = None;
            agent.steps_taken = 0;
            agent.started_at = restarted_at;
            agent.input_tx = Some(input_tx);
            agent.task_handle = Some(handle);

            if let Some(event_tx) = runtime.event_tx {
                let _ = event_tx.try_send(Event::AgentSpawned {
                    id: agent.id.clone(),
                    prompt: format!("(resumed) {}", agent.prompt),
                });
            }

            agent.snapshot()
        };
        self.persist_state_best_effort();

        Ok(snapshot)
    }

    /// Send input to a running agent.
    pub fn send_input(&mut self, agent_id: &str, text: String, interrupt: bool) -> Result<()> {
        let agent = self
            .agents
            .get_mut(agent_id)
            .ok_or_else(|| anyhow!("Agent {agent_id} not found"))?;

        if agent.status != AgentStatus::Running {
            return Err(anyhow!("Agent {agent_id} is not running"));
        }

        let tx = agent
            .input_tx
            .as_ref()
            .ok_or_else(|| anyhow!("Agent {agent_id} cannot accept input"))?;

        tx.send(AgentInput { text, interrupt })
            .map_err(|_| anyhow!("Failed to send input to agent {agent_id}"))?;

        Ok(())
    }

    /// Update assignment metadata and optionally send immediate guidance.
    pub fn assign(
        &mut self,
        agent_id: &str,
        objective: Option<String>,
        role: Option<String>,
        message: Option<String>,
        interrupt: bool,
    ) -> Result<AgentResult> {
        if objective.is_none() && role.is_none() && message.is_none() {
            return Err(anyhow!(
                "Provide at least one of objective, role, or message"
            ));
        }

        if message.is_some() {
            let status = self
                .agents
                .get(agent_id)
                .ok_or_else(|| anyhow!("Agent {agent_id} not found"))?
                .status
                .clone();
            if status != AgentStatus::Running {
                return Err(anyhow!(
                    "Agent {agent_id} is not running; cannot deliver assignment message"
                ));
            }
        }

        let mut changed = false;
        let (input_tx, payload) = {
            let agent = self
                .agents
                .get_mut(agent_id)
                .ok_or_else(|| anyhow!("Agent {agent_id} not found"))?;

            let mut assignment_lines = Vec::new();
            if let Some(objective) = objective {
                let objective = objective.trim();
                if objective.is_empty() {
                    return Err(anyhow!("objective cannot be empty"));
                }
                if agent.assignment.objective != objective {
                    agent.assignment.objective = objective.to_string();
                    changed = true;
                }
                assignment_lines.push(format!("- objective: {}", agent.assignment.objective));
            }

            if let Some(role) = role {
                let normalized = normalize_role_alias(&role)
                    .ok_or_else(|| {
                        anyhow!(
                            "Invalid role alias '{role}'. Use: worker, explorer, awaiter, default"
                        )
                    })?
                    .to_string();
                if agent.assignment.role.as_deref() != Some(normalized.as_str()) {
                    agent.assignment.role = Some(normalized.clone());
                    changed = true;
                }
                assignment_lines.push(format!("- role: {normalized}"));
            }

            let mut payload_parts = Vec::new();
            if !assignment_lines.is_empty() && agent.status == AgentStatus::Running {
                payload_parts.push(format!(
                    "Assignment updated:\n{}",
                    assignment_lines.join("\n")
                ));
            }
            if let Some(message) = message {
                let message = message.trim();
                if message.is_empty() {
                    return Err(anyhow!("message cannot be empty"));
                }
                payload_parts.push(format!("Coordinator note:\n{message}"));
            }

            let payload = if payload_parts.is_empty() {
                None
            } else {
                Some(payload_parts.join("\n\n"))
            };

            (agent.input_tx.clone(), payload)
        };

        if let Some(payload) = payload {
            let tx = input_tx
                .ok_or_else(|| anyhow!("Agent {agent_id} cannot accept assignment input"))?;
            tx.send(AgentInput {
                text: payload,
                interrupt,
            })
            .map_err(|_| anyhow!("Failed to send assignment to agent {agent_id}"))?;
        }

        if changed {
            self.persist_state_best_effort();
        }

        self.get_result(agent_id)
    }

    /// List all agents and their status.
    #[must_use]
    /// Snapshot a single agent and tag it with the manager's
    /// classification. The bare `Agent::snapshot` defaults
    /// `from_prior_session` to `false`; only the manager knows the
    /// matching boot id, so listing goes through here.
    fn snapshot_for_listing(&self, agent: &Agent) -> AgentResult {
        let mut snap = agent.snapshot();
        snap.from_prior_session = self.is_from_prior_session(agent);
        snap
    }

    /// List all agents currently held by the manager, regardless of
    /// session origin. Use [`Self::list_filtered`] in user-facing tool
    /// paths so prior-session agents stay hidden by default (#405).
    pub fn list(&self) -> Vec<AgentResult> {
        self.agents
            .values()
            .map(|agent| self.snapshot_for_listing(agent))
            .collect()
    }

    /// List agents respecting the session-boundary filter (#405).
    ///
    /// `include_archived = false` (the default for `agent_list`) drops
    /// any prior-session agent that is no longer running. Prior-session
    /// agents that are still `Running` (e.g. interrupted by a process
    /// restart) stay visible — they may matter for ongoing recovery.
    ///
    /// `include_archived = true` returns everything, with the
    /// `from_prior_session` flag on each `AgentResult` so the model
    /// can tell active and archived apart at a glance.
    pub fn list_filtered(&self, include_archived: bool) -> Vec<AgentResult> {
        self.agents
            .values()
            .filter(|agent| {
                if include_archived {
                    return true;
                }
                if agent.status == AgentStatus::Running {
                    return true;
                }
                !self.is_from_prior_session(agent)
            })
            .map(|agent| self.snapshot_for_listing(agent))
            .collect()
    }

    /// Clean up completed agents older than the given duration.
    pub fn cleanup(&mut self, max_age: Duration) {
        let before = self.agents.len();
        self.agents.retain(|_, agent| {
            if agent.status == AgentStatus::Running {
                true
            } else {
                agent.started_at.elapsed() < max_age
            }
        });
        if self.agents.len() != before {
            self.persist_state_best_effort();
        }
    }

    fn update_from_result(&mut self, agent_id: &str, result: AgentResult) {
        let mut changed = false;
        if let Some(agent) = self.agents.get_mut(agent_id) {
            agent.status = result.status;
            agent.assignment = result.assignment;
            agent.result = result.result;
            agent.steps_taken = result.steps_taken;
            agent.task_handle = None;
            changed = true;
        }
        if changed {
            self.persist_state_best_effort();
        }
    }

    fn update_failed(&mut self, agent_id: &str, error: String) {
        let mut changed = false;
        if let Some(agent) = self.agents.get_mut(agent_id) {
            agent.status = AgentStatus::Failed(error);
            release_resident_leases_for(agent_id);
            agent.task_handle = None;
            changed = true;
        }
        if changed {
            self.persist_state_best_effort();
        }
    }
}

/// Thread-safe wrapper for `AgentManager`.
pub type SharedAgentManager = Arc<RwLock<AgentManager>>;

/// Model-facing session projection returned by the v0.8.33 agent API.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentSessionProjection {
    pub name: String,
    pub agent_id: String,
    pub status: String,
    pub terminal: bool,
    pub context_mode: String,
    pub fork_context: bool,
    pub prefix_cache: AgentPrefixCacheProjection,
    pub transcript_handle: VarHandle,
    pub snapshot: AgentResult,
    #[serde(default, skip_serializing_if = "is_false")]
    pub timed_out: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentPrefixCacheProjection {
    pub mode: String,
    pub parent_prefix: String,
    pub deepseek_prefix_cache_reuse: String,
}

fn agent_prefix_cache_projection(snapshot: &AgentResult) -> AgentPrefixCacheProjection {
    if snapshot.fork_context {
        AgentPrefixCacheProjection {
            mode: "forked".to_string(),
            parent_prefix: "preserved_byte_identical_when_available".to_string(),
            deepseek_prefix_cache_reuse: "optimized_for_existing_parent_prefill".to_string(),
        }
    } else {
        AgentPrefixCacheProjection {
            mode: "fresh".to_string(),
            parent_prefix: "not_inherited".to_string(),
            deepseek_prefix_cache_reuse: "independent_child_prefill".to_string(),
        }
    }
}

async fn agent_session_projection(
    snapshot: AgentResult,
    timed_out: bool,
    context: &ToolContext,
) -> AgentSessionProjection {
    let transcript_payload = json!({
        "kind": "agent_session_snapshot",
        "agent_id": snapshot.agent_id.clone(),
        "name": snapshot.name.clone(),
        "status": agent_status_name(&snapshot.status),
        "context_mode": snapshot.context_mode.clone(),
        "fork_context": snapshot.fork_context,
        "result": snapshot.result.clone(),
        "steps_taken": snapshot.steps_taken,
        "duration_ms": snapshot.duration_ms,
        "assignment": snapshot.assignment.clone(),
        "snapshot": snapshot.clone(),
    });
    let transcript_handle = {
        let mut store = context.runtime.handle_store.lock().await;
        store.insert_json(
            format!("agent:{}", snapshot.agent_id),
            "transcript",
            transcript_payload,
        )
    };

    AgentSessionProjection {
        name: snapshot.name.clone(),
        agent_id: snapshot.agent_id.clone(),
        status: agent_status_name(&snapshot.status).to_string(),
        terminal: snapshot.status != AgentStatus::Running,
        context_mode: snapshot.context_mode.clone(),
        fork_context: snapshot.fork_context,
        prefix_cache: agent_prefix_cache_projection(&snapshot),
        transcript_handle,
        snapshot,
        timed_out,
    }
}

fn default_state_path(workspace: &Path) -> PathBuf {
    workspace
        .join(".deepseek")
        .join("state")
        .join(AGENT_STATE_FILE)
}

fn epoch_millis_now() -> u64 {
    match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(duration) => u64::try_from(duration.as_millis()).unwrap_or(u64::MAX),
        Err(_) => 0,
    }
}

fn instant_from_duration(duration: Duration) -> Instant {
    Instant::now()
        .checked_sub(duration)
        .unwrap_or_else(Instant::now)
}

fn write_json_atomic<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let payload = serde_json::to_string_pretty(value)?;
    let tmp_path = path.with_extension("tmp");
    fs::write(&tmp_path, payload)?;
    fs::rename(tmp_path, path)?;
    Ok(())
}

/// Create a shared agent manager with a configurable limit.
#[must_use]
pub fn new_shared_agent_manager(workspace: PathBuf, max_agents: usize) -> SharedAgentManager {
    let max_agents = max_agents.clamp(1, MAX_CONCURRENT_AGENTS);
    let state_path = default_state_path(&workspace);
    let mut manager = AgentManager::new(workspace, max_agents).with_state_path(state_path);
    if let Err(err) = manager.load_state() {
        // Routed through tracing instead of stderr — see comment in
        // `persist_state_best_effort` above.
        tracing::warn!(target: "agent", ?err, "failed to load agent state");
    }
    Arc::new(RwLock::new(manager))
}

// === Tool Implementations ===

/// Open a named background agent session.
#[allow(dead_code)] // Registered by the adjacent v0.8.33 registry surface update.
pub struct AgentOpenTool {
    manager: SharedAgentManager,
    runtime: AgentRuntime,
}

impl AgentOpenTool {
    #[allow(dead_code)] // Registered by the adjacent v0.8.33 registry surface update.
    #[must_use]
    pub fn new(manager: SharedAgentManager, runtime: AgentRuntime) -> Self {
        Self { manager, runtime }
    }
}

#[async_trait]
impl ToolSpec for AgentOpenTool {
    fn name(&self) -> &'static str {
        "agent_open"
    }

    fn description(&self) -> &'static str {
        concat!(
            "Open a named child agent session for focused background work. Returns the session name, status, agent_id, context_mode, prefix_cache metadata, and a handle_read-compatible transcript_handle. ",
            "Use agent_eval to fetch or wait on the session, and agent_close to cancel/close it.\n\n",
            "Context control is explicit: omit fork_context or set it false for a fresh child with an independent prefill; set fork_context=true for perspective fanout over the current parent context. ",
            "Forked children preserve the parent system prompt and leading message prefix byte-identically where the runtime has that prefix, so DeepSeek can reuse its prefix cache before the child-specific task is appended.\n\n",
            "Agent results are self-reports. Re-verify claimed side effects such as file edits, commands, network writes, tests, or git operations before reporting them as facts."
        )
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "name": {
                    "type": "string",
                    "description": "Stable model-facing session name. Defaults to the generated agent_id when omitted."
                },
                "session_name": {
                    "type": "string",
                    "description": "Alias for name"
                },
                "prompt": {
                    "type": "string",
                    "description": "Initial task description for the child session"
                },
                "message": {
                    "type": "string",
                    "description": "Alias for prompt"
                },
                "objective": {
                    "type": "string",
                    "description": "Alias for prompt"
                },
                "items": {
                    "type": "array",
                    "description": "Structured input items (text, mention, skill, local_image, image)",
                    "items": { "type": "object" }
                },
                "agent_role": {
                    "type": "string",
                    "description": "Agent role: general, explore, plan, review, implementer, verifier, custom, or role alias (worker, explorer, awaiter, default)"
                },
                "role": {
                    "type": "string",
                    "description": "Alias for agent_role"
                },
                "allowed_tools": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Explicit tool allowlist (required for custom type)"
                },
                "model": {
                    "type": "string",
                    "description": "Optional DeepSeek model id for this child"
                },
                "cwd": {
                    "type": "string",
                    "description": "Optional working directory for the child; must be inside the parent workspace"
                },
                "resident_file": {
                    "type": "string",
                    "description": "Optional file path for cache-aware resident mode"
                },
                "fork_context": {
                    "type": "boolean",
                    "description": "false (default): fresh child with independent context/prefill. true: forked child that preserves the parent's byte-identical system/message prefix where available, then appends this task for DeepSeek prefix-cache reuse."
                },
                "max_depth": {
                    "type": "integer",
                    "minimum": 0,
                    "maximum": 3,
                    "description": "Recursive child-agent budget for this session. 0 blocks agent_open from the child; 1-3 allow that many descendant levels."
                }
            }
        })
    }

    fn capabilities(&self) -> Vec<ToolCapability> {
        vec![
            ToolCapability::ExecutesCode,
            ToolCapability::RequiresApproval,
        ]
    }

    fn approval_requirement(&self) -> ApprovalRequirement {
        ApprovalRequirement::Required
    }

    async fn execute(&self, input: Value, context: &ToolContext) -> Result<ToolResult, ToolError> {
        let spawn_tool = AgentSpawnTool::new(self.manager.clone(), self.runtime.clone());
        let result = spawn_tool.execute(input, context).await?;
        let snapshot: AgentResult = serde_json::from_str(&result.content).map_err(|e| {
            ToolError::execution_failed(format!("agent_open projection failed: {e}"))
        })?;
        let projection = agent_session_projection(snapshot, false, context).await;
        let mut tool_result = ToolResult::json(&projection)
            .map_err(|e| ToolError::execution_failed(e.to_string()))?;
        tool_result.metadata = Some(json!({
            "status": projection.status,
            "terminal": projection.terminal,
            "context_mode": projection.context_mode,
            "prefix_cache": projection.prefix_cache,
        }));
        Ok(tool_result)
    }
}

/// Tool to spawn a background agent.
pub struct AgentSpawnTool {
    manager: SharedAgentManager,
    runtime: AgentRuntime,
    name: &'static str,
}

impl AgentSpawnTool {
    /// Create a new spawn tool.
    #[must_use]
    pub fn new(manager: SharedAgentManager, runtime: AgentRuntime) -> Self {
        Self::with_name(manager, runtime, "agent_spawn")
    }

    /// Create a new spawn tool with a custom tool name alias.
    #[must_use]
    pub fn with_name(
        manager: SharedAgentManager,
        runtime: AgentRuntime,
        name: &'static str,
    ) -> Self {
        Self {
            manager,
            runtime,
            name,
        }
    }
}

#[async_trait]
impl ToolSpec for AgentSpawnTool {
    fn name(&self) -> &'static str {
        self.name
    }

    fn description(&self) -> &'static str {
        concat!(
            "Spawn a background agent for a focused task. Returns an agent_id immediately; follow with agent_eval to fetch or wait on the session, and agent_close to cancel/close it. Default cap of 10 concurrent agents (configurable via `[agents].max_concurrent` in config.toml); each is a full agent loop, so cancel or wait if you hit the cap. For parallel one-shot LLM queries, just emit multiple tool calls in one turn — the dispatcher runs them in parallel.\n\n",
            "## Trust model: agent results are self-reports, not verified facts\n\n",
            "`agent_result` returns the child's narrative summary of what happened. For operations with external side effects, the child's summary may be wrong. Re-verify before reporting success to the user:\n\n",
            "| Side effect | Re-verify with |\n|---|---|\n| URL claimed posted/written | `fetch_url` and check the response |\n| File claimed created | `read_file` or `list_dir` |\n| File claimed edited | `read_file` and check the change is present |\n| HTTP POST/PUT response | inspect status code and body |\n| Git operation | `git_status` / `git_diff` |\n| Test claimed passing | `run_tests` |\n| Process claimed started | `exec_shell` (e.g. `pgrep`, `lsof -i`) |\n\n",
            "If the child returns a verifiable handle (URL, file path, exit code, commit SHA), check it. If it doesn't, ask the child to return one or verify yourself before proceeding."
        )
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "prompt": {
                    "type": "string",
                    "description": "Task description for the agent"
                },
                "message": {
                    "type": "string",
                    "description": "Alias for prompt"
                },
                "objective": {
                    "type": "string",
                    "description": "Alias for prompt"
                },
                "items": {
                    "type": "array",
                    "description": "Structured input items (text, mention, skill, local_image, image)",
                    "items": {
                        "type": "object"
                    }
                },
                "agent_role": {
                    "type": "string",
                    "description": "Agent role: general, explore, plan, review, implementer, verifier, custom, or a user-defined name from roles/ directory. Role alias (worker, explorer, awaiter, default) also accepted. See docs/AGENT_ROLES.md."
                },
                "role": {
                    "type": "string",
                    "description": "Alias for agent_role"
                },
                "allowed_tools": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Explicit tool allowlist (required for custom type). Default behavior is full registry inheritance from the parent; approval-gated tools still require an auto-approved parent."
                },
                "model": {
                    "type": "string",
                    "description": "Optional DeepSeek model id for this child. Explicit model wins over role/type defaults; omit to inherit."
                },
                "cwd": {
                    "type": "string",
                    "description": "Optional working directory for the child. Must be inside the parent's workspace (use a relative path or an absolute path under the workspace root). Used for the parallel-worktree pattern: parent runs `git worktree add .worktrees/feature-x ...` then spawns the child with `cwd: \".worktrees/feature-x\"`."
                },
                "resident_file": {
                    "type": "string",
                    "description": "Optional file path for cache-aware resident mode. When set, the child's system prefix is augmented with the full contents of this file so DeepSeek's prefix cache stays warm across follow-up send_input calls. Only one agent may hold a resident lease on a given file at a time — a second spawn with the same path receives a conflict warning in the result."
                },
                "fork_context": {
                    "type": "boolean",
                    "description": "When true, inherit the parent's system prompt and conversation prefix before appending this task. This preserves DeepSeek prefix-cache reuse and gives the child full parent context. Defaults to false for independent exploration."
                }
            }
        })
    }

    fn capabilities(&self) -> Vec<ToolCapability> {
        vec![
            ToolCapability::ExecutesCode,
            ToolCapability::RequiresApproval,
        ]
    }

    fn approval_requirement(&self) -> ApprovalRequirement {
        ApprovalRequirement::Required
    }

    async fn execute(&self, input: Value, _context: &ToolContext) -> Result<ToolResult, ToolError> {
        let spawn_request = parse_spawn_request(&input)?;

        // Depth cap: reject before locking the manager so we don't introduce
        // unnecessary contention. Mirrors codex's pattern (allow-equal at the
        // boundary; reject when `next > max`).
        if self.runtime.would_exceed_depth() {
            return Err(ToolError::execution_failed(format!(
                "Agent depth limit reached (current depth {}, max {}). \
                 Increase via [runtime] max_spawn_depth in config.toml.",
                self.runtime.spawn_depth, self.runtime.max_spawn_depth
            )));
        }

        // Validate cwd if supplied: must canonicalize inside the parent
        // workspace. Catches accidents like `cwd: "/etc"`.
        let validated_cwd = if let Some(requested_cwd) = spawn_request.cwd.as_ref() {
            let parent_workspace = &self.runtime.context.workspace;
            let resolved = if requested_cwd.is_absolute() {
                requested_cwd.clone()
            } else {
                parent_workspace.join(requested_cwd)
            };
            let canonical = resolved.canonicalize().map_err(|e| {
                ToolError::invalid_input(format!(
                    "Invalid cwd '{}': {e} (path may not exist yet — create the worktree first)",
                    requested_cwd.display()
                ))
            })?;
            let workspace_canonical = parent_workspace
                .canonicalize()
                .unwrap_or_else(|_| parent_workspace.clone());
            if !canonical.starts_with(&workspace_canonical) {
                return Err(ToolError::invalid_input(format!(
                    "cwd must be inside the parent workspace: {} is not under {}",
                    canonical.display(),
                    workspace_canonical.display()
                )));
            }
            Some(canonical)
        } else {
            None
        };

        // Derive the child's runtime as a durable background job: it keeps
        // its own cancellation token, inherits the parent approval state, and
        // optionally overrides cwd if the caller passed one (used for the
        // parallel-worktree pattern).
        let mut child_runtime = self.runtime.background_runtime();
        if let Some(max_depth) = spawn_request.max_depth {
            child_runtime.max_spawn_depth = child_runtime.spawn_depth.saturating_add(max_depth);
        }
        if let Some(cwd) = validated_cwd {
            child_runtime.context.workspace = cwd;
        }
        let configured_model = match spawn_request.model.clone() {
            Some(model) => Some(model),
            None => configured_model_for_role_or_type(
                &self.runtime,
                spawn_request.assignment.role.as_deref(),
                &spawn_request.agent_type,
            )?,
        };

        // Cache-aware resident mode (#529): prepend file contents to the prompt
        // so the child's prefix is byte-stable for DeepSeek prefix caching.
        let (effective_prompt, resident_conflict) =
            if let Some(ref file_path) = spawn_request.resident_file {
                let abs_path = if std::path::Path::new(file_path).is_absolute() {
                    std::path::PathBuf::from(file_path)
                } else {
                    self.runtime.context.workspace.join(file_path)
                };
                let file_contents = std::fs::read_to_string(&abs_path)
                    .unwrap_or_else(|e| format!("<!-- resident_file read error: {e} -->"));
                let prefixed = format!(
                    "<!-- resident_file: {file_path} -->\n```\n{file_contents}\n```\n\n{}",
                    spawn_request.prompt
                );
                // Check ownership (best-effort, non-blocking).
                let conflict = {
                    let leases = RESIDENT_LEASES
                        .get_or_init(|| std::sync::Mutex::new(std::collections::HashMap::new()));
                    let mut guard = leases.lock().unwrap_or_else(|p| p.into_inner());
                    if let Some(owner) = guard.get(file_path) {
                        Some(format!(
                            "Warning: agent {owner} already holds a resident lease on {file_path}"
                        ))
                    } else {
                        guard.insert(file_path.clone(), "pending".to_string());
                        None
                    }
                };
                (prefixed, conflict)
            } else {
                (spawn_request.prompt, None)
            };

        let route =
            resolve_agent_assignment_route(&self.runtime, configured_model, &effective_prompt)
                .await;
        child_runtime.model = route.model.clone();
        child_runtime.reasoning_effort = route.reasoning_effort.clone();
        child_runtime.reasoning_effort_auto = false;
        let effective_model = route.model;

        let mut manager = self.manager.write().await;

        let result = manager
            .spawn_background_with_assignment_options(
                Arc::clone(&self.manager),
                child_runtime,
                spawn_request.agent_type,
                effective_prompt,
                spawn_request.assignment,
                spawn_request.allowed_tools,
                AgentSpawnOptions {
                    name: spawn_request.session_name.clone(),
                    model: Some(effective_model),
                    nickname: None,
                    fork_context: spawn_request.fork_context,
                },
            )
            .map_err(|e| ToolError::execution_failed(format!("Failed to spawn agent: {e}")))?;

        // Replace the "pending" lease placeholder with the real agent id now that
        // the manager has assigned one. Without this, `release_resident_leases_for`
        // (which matches by agent id at terminal-state transitions) can never find
        // the entry — leases would stay stamped as "pending" forever, defeating the
        // release machinery added in #660.
        if let Some(ref file_path) = spawn_request.resident_file
            && let Some(lock) = RESIDENT_LEASES.get()
            && let Ok(mut guard) = lock.lock()
            && let Some(owner) = guard.get_mut(file_path)
            && owner == "pending"
        {
            *owner = result.agent_id.clone();
        }

        let mut tool_result = if self.name == "spawn_agent" {
            let mut payload = json!({
                "agent_id": result.agent_id.clone(),
                "nickname": result.nickname.clone(),
                "model": result.model.clone()
            });
            if let Some(ref warning) = resident_conflict {
                payload["resident_conflict"] = json!(warning);
            }
            ToolResult::json(&payload).map_err(|e| ToolError::execution_failed(e.to_string()))?
        } else {
            ToolResult::json(&result).map_err(|e| ToolError::execution_failed(e.to_string()))?
        };
        if result.status == AgentStatus::Running {
            if self.name == "spawn_agent" {
                tool_result.metadata = Some(json!({
                    "status": "Running",
                    "snapshot": result
                }));
            } else {
                tool_result.metadata = Some(json!({ "status": "Running" }));
            }
        }
        // Annotate alias invocations with a deprecation notice so the model
        // can migrate to the canonical name before removal in v0.8.0.
        if self.name == "spawn_agent" {
            tool_result = wrap_with_deprecation_notice(tool_result, "spawn_agent", "agent_spawn");
        }
        Ok(tool_result)
    }
}

/// Evaluate/fetch a child session boundary for the v0.8.33 agent API.
#[allow(dead_code)] // Registered by the adjacent v0.8.33 registry surface update.
pub struct AgentEvalTool {
    manager: SharedAgentManager,
}

impl AgentEvalTool {
    #[allow(dead_code)] // Registered by the adjacent v0.8.33 registry surface update.
    #[must_use]
    pub fn new(manager: SharedAgentManager) -> Self {
        Self { manager }
    }
}

#[async_trait]
impl ToolSpec for AgentEvalTool {
    fn name(&self) -> &'static str {
        "agent_eval"
    }

    fn description(&self) -> &'static str {
        "Fetch or wait on a child agent session. Optionally deliver a message/items to a running session, then return the latest session projection. With block=true (default), waits for the session to reach a terminal boundary; block=false is a non-blocking status fetch."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "name": {
                    "type": "string",
                    "description": "Session name returned by agent_open"
                },
                "agent_id": {
                    "type": "string",
                    "description": "Generated agent id returned by agent_open"
                },
                "id": {
                    "type": "string",
                    "description": "Alias for agent_id"
                },
                "message": {
                    "type": "string",
                    "description": "Optional message to deliver before evaluating the session"
                },
                "input": {
                    "type": "string",
                    "description": "Alias for message"
                },
                "items": {
                    "type": "array",
                    "description": "Structured input items (text, mention, skill, local_image, image)",
                    "items": { "type": "object" }
                },
                "interrupt": {
                    "type": "boolean",
                    "description": "When sending input, prioritize it over pending inputs"
                },
                "block": {
                    "type": "boolean",
                    "description": "Wait for a terminal boundary before returning (default true)"
                },
                "timeout_ms": {
                    "type": "integer",
                    "description": "Max wait time in milliseconds (default: 30000, clamped to 1000-3600000)"
                }
            }
        })
    }

    fn capabilities(&self) -> Vec<ToolCapability> {
        vec![ToolCapability::ReadOnly]
    }

    async fn execute(&self, input: Value, context: &ToolContext) -> Result<ToolResult, ToolError> {
        let agent_ref = input
            .get("name")
            .or_else(|| input.get("agent_id"))
            .or_else(|| input.get("id"))
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| ToolError::missing_field("name"))?;
        let message = parse_optional_text_or_items(&input, &["message", "input"], "items")?;
        let interrupt = optional_bool(&input, "interrupt", false);
        let block = optional_bool(&input, "block", true);
        let timeout_ms = optional_u64(&input, "timeout_ms", DEFAULT_RESULT_TIMEOUT_MS)
            .clamp(1000, MAX_RESULT_TIMEOUT_MS);

        let agent_id = {
            let manager = self.manager.read().await;
            manager
                .resolve_agent_ref(agent_ref)
                .map_err(|e| ToolError::execution_failed(e.to_string()))?
        };

        if let Some(message) = message {
            let mut manager = self.manager.write().await;
            manager
                .send_input(&agent_id, message, interrupt)
                .map_err(|e| ToolError::execution_failed(e.to_string()))?;
        }

        let (snapshot, timed_out) = if block {
            wait_for_result(&self.manager, &agent_id, Duration::from_millis(timeout_ms)).await?
        } else {
            let manager = self.manager.read().await;
            (
                manager
                    .get_result(&agent_id)
                    .map_err(|e| ToolError::execution_failed(e.to_string()))?,
                false,
            )
        };

        let projection = agent_session_projection(snapshot, timed_out, context).await;
        let mut result = ToolResult::json(&projection)
            .map_err(|e| ToolError::execution_failed(e.to_string()))?;
        result.metadata = Some(json!({
            "status": if timed_out { "TimedOut".to_string() } else { projection.status.clone() },
            "timed_out": timed_out,
            "terminal": projection.terminal,
            "context_mode": projection.context_mode,
            "timeout_ms": timeout_ms
        }));
        Ok(result)
    }
}

/// Tool to fetch a agent's result.
#[allow(dead_code)] // Legacy surface superseded by agent_eval.
pub struct AgentResultTool {
    manager: SharedAgentManager,
}

impl AgentResultTool {
    /// Create a new result tool.
    #[allow(dead_code)] // Legacy surface superseded by agent_eval.
    #[must_use]
    pub fn new(manager: SharedAgentManager) -> Self {
        Self { manager }
    }
}

#[async_trait]
impl ToolSpec for AgentResultTool {
    fn name(&self) -> &'static str {
        "agent_result"
    }

    fn description(&self) -> &'static str {
        "Get the latest status or final result for a agent. Set `block: true` to wait until the \
         agent reaches a terminal state (respects `timeout_ms`)."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "agent_id": {
                    "type": "string",
                    "description": "ID returned by agent_spawn"
                },
                "id": {
                    "type": "string",
                    "description": "Alias for agent_id"
                },
                "block": {
                    "type": "boolean",
                    "description": "Wait for completion (default: false)"
                },
                "timeout_ms": {
                    "type": "integer",
                    "description": "Max wait time in milliseconds (default: 30000, clamped to 1000-3600000)"
                }
            }
        })
    }

    fn capabilities(&self) -> Vec<ToolCapability> {
        vec![ToolCapability::ReadOnly]
    }

    async fn execute(&self, input: Value, _context: &ToolContext) -> Result<ToolResult, ToolError> {
        let agent_id = input
            .get("agent_id")
            .or_else(|| input.get("id"))
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::missing_field("agent_id"))?;
        let block = optional_bool(&input, "block", false);
        let timeout_ms = optional_u64(&input, "timeout_ms", DEFAULT_RESULT_TIMEOUT_MS)
            .clamp(1000, MAX_RESULT_TIMEOUT_MS);

        let (result, timed_out) = if block {
            wait_for_result(&self.manager, agent_id, Duration::from_millis(timeout_ms)).await?
        } else {
            let manager = self.manager.read().await;
            (
                manager
                    .get_result(agent_id)
                    .map_err(|e| ToolError::execution_failed(e.to_string()))?,
                false,
            )
        };

        let mut tool_result =
            ToolResult::json(&result).map_err(|e| ToolError::execution_failed(e.to_string()))?;
        if timed_out {
            tool_result.metadata = Some(json!({
                "status": "TimedOut",
                "timed_out": true,
                "timeout_ms": timeout_ms
            }));
        } else if result.status == AgentStatus::Running {
            tool_result.metadata = Some(json!({ "status": "Running" }));
        }
        Ok(tool_result)
    }
}

/// Tool to cancel a agent.
#[allow(dead_code)] // Legacy surface superseded by agent_close.
pub struct AgentCancelTool {
    manager: SharedAgentManager,
}

impl AgentCancelTool {
    /// Create a new cancel tool.
    #[allow(dead_code)] // Legacy surface superseded by agent_close.
    #[must_use]
    pub fn new(manager: SharedAgentManager) -> Self {
        Self { manager }
    }
}

#[async_trait]
impl ToolSpec for AgentCancelTool {
    fn name(&self) -> &'static str {
        "agent_cancel"
    }

    fn description(&self) -> &'static str {
        "Cancel a running agent. Returns the final snapshot with the cancelled status."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "agent_id": {
                    "type": "string",
                    "description": "ID returned by agent_spawn"
                }
            },
            "required": ["agent_id"]
        })
    }

    fn capabilities(&self) -> Vec<ToolCapability> {
        vec![
            ToolCapability::ExecutesCode,
            ToolCapability::RequiresApproval,
        ]
    }

    fn approval_requirement(&self) -> ApprovalRequirement {
        ApprovalRequirement::Required
    }

    async fn execute(&self, input: Value, _context: &ToolContext) -> Result<ToolResult, ToolError> {
        let agent_id = required_str(&input, "agent_id")?;
        let mut manager = self.manager.write().await;
        let result = manager
            .cancel(agent_id)
            .map_err(|e| ToolError::execution_failed(format!("Failed to cancel agent: {e}")))?;

        ToolResult::json(&result).map_err(|e| ToolError::execution_failed(e.to_string()))
    }
}

/// Tool to list all agents.
#[allow(dead_code)] // Legacy surface superseded by named agent_open/eval/close sessions.
pub struct AgentListTool {
    manager: SharedAgentManager,
}

/// Tool to close a running agent (alias for cancel).
pub struct AgentCloseTool {
    manager: SharedAgentManager,
}

impl AgentCloseTool {
    /// Create a new close tool.
    #[must_use]
    pub fn new(manager: SharedAgentManager) -> Self {
        Self { manager }
    }
}

#[async_trait]
impl ToolSpec for AgentCloseTool {
    fn name(&self) -> &'static str {
        "agent_close"
    }

    fn description(&self) -> &'static str {
        "Close a child agent session by cancelling it if still running. Returns the final session projection with transcript_handle metadata."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "id": {
                    "type": "string",
                    "description": "Agent id returned by agent_open"
                },
                "name": {
                    "type": "string",
                    "description": "Session name returned by agent_open"
                },
                "agent_id": {
                    "type": "string",
                    "description": "Alias for id"
                }
            }
        })
    }

    fn capabilities(&self) -> Vec<ToolCapability> {
        vec![
            ToolCapability::ExecutesCode,
            ToolCapability::RequiresApproval,
        ]
    }

    fn approval_requirement(&self) -> ApprovalRequirement {
        ApprovalRequirement::Required
    }

    async fn execute(&self, input: Value, context: &ToolContext) -> Result<ToolResult, ToolError> {
        let agent_id = input
            .get("name")
            .or_else(|| input.get("id"))
            .or_else(|| input.get("agent_id"))
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::missing_field("id"))?;
        let agent_id = {
            let manager = self.manager.read().await;
            manager
                .resolve_agent_ref(agent_id)
                .map_err(|e| ToolError::execution_failed(e.to_string()))?
        };
        let mut manager = self.manager.write().await;
        let result = manager
            .cancel(&agent_id)
            .map_err(|e| ToolError::execution_failed(format!("Failed to close agent: {e}")))?;
        let projection = agent_session_projection(result, false, context).await;
        ToolResult::json(&projection).map_err(|e| ToolError::execution_failed(e.to_string()))
    }
}

/// Tool to resume an existing agent.
#[allow(dead_code)] // Legacy surface superseded by agent_open/eval.
pub struct AgentResumeTool {
    manager: SharedAgentManager,
    runtime: AgentRuntime,
}

impl AgentResumeTool {
    /// Create a new resume tool.
    #[allow(dead_code)] // Legacy surface superseded by agent_open/eval.
    #[must_use]
    pub fn new(manager: SharedAgentManager, runtime: AgentRuntime) -> Self {
        Self { manager, runtime }
    }
}

#[async_trait]
impl ToolSpec for AgentResumeTool {
    fn name(&self) -> &'static str {
        "resume_agent"
    }

    fn description(&self) -> &'static str {
        "Resume a previously closed or completed agent by restarting its assignment."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "id": {
                    "type": "string",
                    "description": "Agent id to resume"
                },
                "agent_id": {
                    "type": "string",
                    "description": "Alias for id"
                }
            }
        })
    }

    fn capabilities(&self) -> Vec<ToolCapability> {
        vec![
            ToolCapability::ExecutesCode,
            ToolCapability::RequiresApproval,
        ]
    }

    fn approval_requirement(&self) -> ApprovalRequirement {
        ApprovalRequirement::Required
    }

    async fn execute(&self, input: Value, _context: &ToolContext) -> Result<ToolResult, ToolError> {
        let agent_id = input
            .get("id")
            .or_else(|| input.get("agent_id"))
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::missing_field("id"))?;
        let mut manager = self.manager.write().await;
        let result = manager
            .resume(Arc::clone(&self.manager), self.runtime.clone(), agent_id)
            .map_err(|e| ToolError::execution_failed(format!("Failed to resume agent: {e}")))?;
        ToolResult::json(&result).map_err(|e| ToolError::execution_failed(e.to_string()))
    }
}

impl AgentListTool {
    /// Create a new list tool.
    #[allow(dead_code)] // Legacy surface superseded by named agent_open/eval/close sessions.
    #[must_use]
    pub fn new(manager: SharedAgentManager) -> Self {
        Self { manager }
    }
}

#[async_trait]
impl ToolSpec for AgentListTool {
    fn name(&self) -> &'static str {
        "agent_list"
    }

    fn description(&self) -> &'static str {
        "List agents from the current session with their status, type, assignment, steps, \
         and duration. Pass `include_archived=true` to also see agents that were spawned in a \
         prior session (e.g. before the TUI restarted) and persisted on disk; those carry \
         `from_prior_session: true` in the result. Default is the current-session view because \
         prior-session agents almost never matter for the live turn."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "include_archived": {
                    "type": "boolean",
                    "description": "When true, include agents from prior sessions in the listing. Default false."
                }
            }
        })
    }

    fn capabilities(&self) -> Vec<ToolCapability> {
        vec![ToolCapability::ReadOnly]
    }

    async fn execute(&self, input: Value, _context: &ToolContext) -> Result<ToolResult, ToolError> {
        let include_archived = input
            .get("include_archived")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let mut manager = self.manager.write().await;
        manager.cleanup(COMPLETED_AGENT_RETENTION);
        let results = manager.list_filtered(include_archived);
        ToolResult::json(&results).map_err(|e| ToolError::execution_failed(e.to_string()))
    }
}

/// Tool to send input to a running agent.
#[allow(dead_code)] // Legacy surface superseded by agent_eval.
pub struct AgentSendInputTool {
    manager: SharedAgentManager,
    name: &'static str,
}

impl AgentSendInputTool {
    /// Create a new send-input tool.
    #[allow(dead_code)] // Legacy surface superseded by agent_eval.
    #[must_use]
    pub fn new(manager: SharedAgentManager, name: &'static str) -> Self {
        Self { manager, name }
    }
}

#[async_trait]
impl ToolSpec for AgentSendInputTool {
    fn name(&self) -> &'static str {
        self.name
    }

    fn description(&self) -> &'static str {
        "Send input to a running agent. Returns the agent's current snapshot after delivery."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "agent_id": {
                    "type": "string",
                    "description": "ID returned by agent_spawn"
                },
                "id": {
                    "type": "string",
                    "description": "Alias for agent_id"
                },
                "message": {
                    "type": "string",
                    "description": "Message to deliver to the agent"
                },
                "input": {
                    "type": "string",
                    "description": "Alias for message"
                },
                "items": {
                    "type": "array",
                    "description": "Structured input items (text, mention, skill, local_image, image)",
                    "items": {
                        "type": "object"
                    }
                },
                "interrupt": {
                    "type": "boolean",
                    "description": "Prioritize this message over pending inputs"
                }
            }
        })
    }

    fn capabilities(&self) -> Vec<ToolCapability> {
        vec![]
    }

    async fn execute(&self, input: Value, _context: &ToolContext) -> Result<ToolResult, ToolError> {
        let agent_id = input
            .get("agent_id")
            .or_else(|| input.get("id"))
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::missing_field("agent_id"))?;
        let message = parse_text_or_items(&input, &["message", "input"], "items", "message")?;
        let interrupt = optional_bool(&input, "interrupt", false);

        let mut manager = self.manager.write().await;
        manager
            .send_input(agent_id, message, interrupt)
            .map_err(|e| ToolError::execution_failed(e.to_string()))?;
        let snapshot = manager
            .get_result(agent_id)
            .map_err(|e| ToolError::execution_failed(e.to_string()))?;

        let tool_result =
            ToolResult::json(&snapshot).map_err(|e| ToolError::execution_failed(e.to_string()))?;
        // Annotate the alias name "send_input" with a deprecation notice;
        // the canonical name "agent_send_input" passes through unchanged.
        if self.name == "send_input" {
            Ok(wrap_with_deprecation_notice(
                tool_result,
                "send_input",
                "agent_send_input",
            ))
        } else {
            Ok(tool_result)
        }
    }
}

/// Tool to update assignment metadata for a agent.
#[allow(dead_code)] // Legacy surface superseded by agent_eval/open metadata.
pub struct AgentAssignTool {
    manager: SharedAgentManager,
    name: &'static str,
}

impl AgentAssignTool {
    /// Create a new assignment tool.
    #[allow(dead_code)] // Legacy surface superseded by agent_eval/open metadata.
    #[must_use]
    pub fn new(manager: SharedAgentManager, name: &'static str) -> Self {
        Self { manager, name }
    }
}

#[async_trait]
impl ToolSpec for AgentAssignTool {
    fn name(&self) -> &'static str {
        self.name
    }

    fn description(&self) -> &'static str {
        "Update a agent's assignment (objective, role) and optionally deliver an immediate \
         coordinator note. The update is delivered as a high-priority message when `interrupt` is \
         true (the default). Returns the agent's current snapshot."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "agent_id": {
                    "type": "string",
                    "description": "Agent id returned by agent_spawn"
                },
                "id": {
                    "type": "string",
                    "description": "Alias for agent_id"
                },
                "objective": {
                    "type": "string",
                    "description": "Updated assignment objective"
                },
                "role": {
                    "type": "string",
                    "description": "Updated role alias: worker, explorer, awaiter, default"
                },
                "agent_role": {
                    "type": "string",
                    "description": "Alias for role"
                },
                "message": {
                    "type": "string",
                    "description": "Optional coordinator note to send to the agent"
                },
                "input": {
                    "type": "string",
                    "description": "Alias for message"
                },
                "items": {
                    "type": "array",
                    "description": "Structured input items (text, mention, skill, local_image, image)",
                    "items": {
                        "type": "object"
                    }
                },
                "interrupt": {
                    "type": "boolean",
                    "description": "Prioritize this assignment update in the agent inbox (default: true)"
                }
            }
        })
    }

    fn capabilities(&self) -> Vec<ToolCapability> {
        vec![]
    }

    async fn execute(&self, input: Value, _context: &ToolContext) -> Result<ToolResult, ToolError> {
        let request = parse_assign_request(&input)?;
        let mut manager = self.manager.write().await;
        let result = manager
            .assign(
                &request.agent_id,
                request.objective,
                request.role,
                request.message,
                request.interrupt,
            )
            .map_err(|e| ToolError::execution_failed(format!("Failed to assign agent: {e}")))?;

        ToolResult::json(&result).map_err(|e| ToolError::execution_failed(e.to_string()))
    }
}

/// Tool to wait for agents to complete.
#[allow(dead_code)] // Legacy surface superseded by agent_eval.
pub struct AgentWaitTool {
    manager: SharedAgentManager,
    name: &'static str,
}

impl AgentWaitTool {
    /// Create a new wait tool.
    #[allow(dead_code)] // Legacy surface superseded by agent_eval.
    #[must_use]
    pub fn new(manager: SharedAgentManager, name: &'static str) -> Self {
        Self { manager, name }
    }
}

#[async_trait]
impl ToolSpec for AgentWaitTool {
    fn name(&self) -> &'static str {
        self.name
    }

    fn description(&self) -> &'static str {
        "Wait for one or more agents to reach a terminal status. Use `wait_mode: \"all\"` to block \
         until every listed agent finishes, or `wait_mode: \"any\"` (default) to return as soon as \
         one finishes. When no ids are given, waits on all currently running agents."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "ids": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Agent IDs to wait on. When omitted, waits on all currently running agents."
                },
                "agent_ids": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Alias for ids"
                },
                "agent_id": {
                    "type": "string",
                    "description": "Single agent ID"
                },
                "id": {
                    "type": "string",
                    "description": "Alias for agent_id"
                },
                "wait_mode": {
                    "type": "string",
                    "description": "Wait behavior: any (default) or all"
                },
                "timeout_ms": {
                    "type": "integer",
                    "description": "Max wait time in milliseconds (default: 30000, clamped to 10000-3600000)"
                }
            }
        })
    }

    fn capabilities(&self) -> Vec<ToolCapability> {
        vec![ToolCapability::ReadOnly]
    }

    async fn execute(&self, input: Value, _context: &ToolContext) -> Result<ToolResult, ToolError> {
        let timeout_ms = optional_u64(&input, "timeout_ms", DEFAULT_RESULT_TIMEOUT_MS)
            .clamp(MIN_WAIT_TIMEOUT_MS, MAX_RESULT_TIMEOUT_MS);
        let mut ids = parse_wait_ids(&input);
        if ids.is_empty() {
            let manager = self.manager.read().await;
            ids = manager
                .list()
                .into_iter()
                .filter(|snapshot| snapshot.status == AgentStatus::Running)
                .map(|snapshot| snapshot.agent_id)
                .collect();
        }
        let wait_mode = parse_wait_mode(&input)?;

        if ids.is_empty() {
            let empty: Vec<AgentResult> = Vec::new();
            let mut result =
                ToolResult::json(&empty).map_err(|e| ToolError::execution_failed(e.to_string()))?;
            result.metadata = Some(json!({
                "wait_mode": wait_mode.as_str(),
                "timed_out": false,
                "status": "Completed",
                "timeout_ms": timeout_ms,
                "waited_ids": [],
                "completed_ids": [],
                "running_ids": [],
                "status_by_id": {}
            }));
            return Ok(result);
        }

        let waited_ids = ids.clone();

        let (snapshots, timed_out) = wait_for_agents(
            &self.manager,
            &ids,
            wait_mode,
            Duration::from_millis(timeout_ms),
        )
        .await?;

        let all_done = snapshots
            .iter()
            .all(|snapshot| snapshot.status != AgentStatus::Running);
        let completed_ids = snapshots
            .iter()
            .filter(|snapshot| snapshot.status != AgentStatus::Running)
            .map(|snapshot| snapshot.agent_id.clone())
            .collect::<Vec<_>>();
        let running_ids = snapshots
            .iter()
            .filter(|snapshot| snapshot.status == AgentStatus::Running)
            .map(|snapshot| snapshot.agent_id.clone())
            .collect::<Vec<_>>();
        let status_by_id = snapshots
            .iter()
            .map(|snapshot| {
                (
                    snapshot.agent_id.clone(),
                    agent_status_name(&snapshot.status).to_string(),
                )
            })
            .collect::<HashMap<_, _>>();

        let mut result =
            ToolResult::json(&snapshots).map_err(|e| ToolError::execution_failed(e.to_string()))?;
        result.metadata = Some(json!({
            "wait_mode": wait_mode.as_str(),
            "timed_out": timed_out,
            "status": if timed_out { "TimedOut" } else if all_done { "Completed" } else { "Partial" },
            "timeout_ms": timeout_ms,
            "waited_ids": waited_ids,
            "completed_ids": completed_ids,
            "running_ids": running_ids,
            "status_by_id": status_by_id
        }));
        Ok(result)
    }
}

/// Compatibility delegate tool. It routes through `agent_spawn`, but defaults
/// to `fork_context=true` because delegation is usually continuation work.
#[allow(dead_code)] // Legacy alias superseded by agent_open(fork_context=true).
pub struct DelegateToAgentTool {
    manager: SharedAgentManager,
    runtime: AgentRuntime,
}

impl DelegateToAgentTool {
    /// Create a new delegation tool.
    #[allow(dead_code)] // Legacy alias superseded by agent_open(fork_context=true).
    #[must_use]
    pub fn new(manager: SharedAgentManager, runtime: AgentRuntime) -> Self {
        Self { manager, runtime }
    }
}

#[async_trait]
impl ToolSpec for DelegateToAgentTool {
    fn name(&self) -> &'static str {
        "delegate_to_agent"
    }

    fn description(&self) -> &'static str {
        "Delegate a task to a specialized agent. Compatibility wrapper around agent_spawn; \
         defaults fork_context=true so the child inherits the parent transcript. Use `agent_role` \
         (or `role`) to pick the agent flavor."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "agent_role": {
                    "type": "string",
                    "description": "Agent role: general, explore, plan, review, implementer, verifier, custom, or role alias (worker, explorer, awaiter, default)"
                },
                "role": {
                    "type": "string",
                    "description": "Alias for agent_role"
                },
                "objective": {
                    "type": "string",
                    "description": "The goal or task description for the agent"
                },
                "prompt": {
                    "type": "string",
                    "description": "Alias for objective"
                },
                "message": {
                    "type": "string",
                    "description": "Alias for objective"
                },
                "items": {
                    "type": "array",
                    "description": "Structured input items (text, mention, skill, local_image, image)",
                    "items": {
                        "type": "object"
                    }
                },
                "allowed_tools": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Explicit tool allowlist (required for custom type)"
                },
                "fork_context": {
                    "type": "boolean",
                    "description": "When true, inherit the parent's system prompt and conversation prefix before appending this task. delegate_to_agent defaults this to true."
                }
            }
        })
    }

    fn capabilities(&self) -> Vec<ToolCapability> {
        vec![
            ToolCapability::ExecutesCode,
            ToolCapability::RequiresApproval,
        ]
    }

    fn approval_requirement(&self) -> ApprovalRequirement {
        ApprovalRequirement::Required
    }

    async fn execute(&self, input: Value, context: &ToolContext) -> Result<ToolResult, ToolError> {
        let spawn_tool = AgentSpawnTool::new(self.manager.clone(), self.runtime.clone());
        let input = with_default_fork_context(input, true);
        let result = spawn_tool.execute(input, context).await?;
        Ok(wrap_with_deprecation_notice(
            result,
            "delegate_to_agent",
            "agent_spawn",
        ))
    }
}

// === Agent Execution ===

/// Build the system prompt for a agent.
///
/// Starts with the per-type prompt (`AgentRole::system_prompt`) and
/// appends a one-line role overlay when `assignment.role` is set. The
/// full role library — TOML overlays from `~/.deepseek/roles/`, the
/// `/roles` slash command, model overrides per role — lands in 0.6.7.
/// For 0.6.6 we just don't drop the role on the floor: the model sees
/// "You are operating in the role of `{name}`." as a final line so its
/// behavior reflects the user's choice.
pub(crate) fn build_agent_system_prompt(
    agent_type: &AgentRole,
    assignment: &AgentAssignment,
    role_configs: &HashMap<String, deepseek_config::AgentRoleConfig>,
) -> String {
    let base = match agent_type {
        AgentRole::General => format!("{GENERAL_AGENT_INTRO}{AGENT_OUTPUT_FORMAT}"),
        AgentRole::Named(name) => {
            if let Some(ct) = role_configs.get(name) {
                format!("{}\n\n{}", ct.system_prompt.trim(), AGENT_OUTPUT_FORMAT)
            } else {
                // Unknown role name — use a minimal fallback.
                format!("You are a `{name}` agent.\n\n{AGENT_OUTPUT_FORMAT}")
            }
        }
    };
    match assignment.role.as_deref() {
        Some(role) if !role.trim().is_empty() => {
            format!(
                "{base}\n\nYou are operating in the role of `{}`.",
                role.trim()
            )
        }
        _ => base,
    }
}

fn agent_request_system_prompt(
    agent_system_prompt: &str,
    fork_context: Option<&AgentForkContext>,
) -> SystemPrompt {
    fork_context
        .and_then(|context| context.system.clone())
        .unwrap_or_else(|| SystemPrompt::Text(agent_system_prompt.to_string()))
}

fn build_initial_agent_messages(
    prompt: &str,
    assignment: &AgentAssignment,
    agent_type: &AgentRole,
    fork_context: Option<&AgentForkContext>,
    role_configs: &HashMap<String, deepseek_config::AgentRoleConfig>,
) -> Vec<Message> {
    let mut messages = fork_context
        .map(|context| context.messages.clone())
        .unwrap_or_default();

    if let Some(context) = fork_context {
        if let Some(state) = context
            .structured_state_block
            .as_deref()
            .map(str::trim)
            .filter(|state| !state.is_empty())
        {
            messages.push(system_text_message(format!(
                "<deepseek:fork_state>\n{state}\n</deepseek:fork_state>"
            )));
        }

        messages.push(system_text_message(format!(
            "<deepseek:agent_context>\n{}\n</deepseek:agent_context>",
            build_agent_system_prompt(agent_type, assignment, role_configs)
        )));
    }

    messages.push(Message {
        role: "user".to_string(),
        content: vec![ContentBlock::Text {
            text: build_assignment_prompt(prompt, assignment, agent_type),
            cache_control: None,
        }],
    });

    messages
}

fn system_text_message(text: String) -> Message {
    Message {
        role: "system".to_string(),
        content: vec![ContentBlock::Text {
            text,
            cache_control: None,
        }],
    }
}

struct AgentTask {
    manager_handle: SharedAgentManager,
    runtime: AgentRuntime,
    agent_id: String,
    agent_type: AgentRole,
    prompt: String,
    assignment: AgentAssignment,
    /// `None` = full registry inheritance. `Some(list)` = explicit narrow.
    /// Approval-gated tools still require an auto-approved parent runtime.
    allowed_tools: Option<Vec<String>>,
    fork_context: bool,
    started_at: Instant,
    max_steps: u32,
    input_rx: mpsc::UnboundedReceiver<AgentInput>,
}

#[allow(clippy::too_many_lines)]
async fn run_agent_task(task: AgentTask) {
    let result = run_agent(
        &task.runtime,
        task.agent_id.clone(),
        task.agent_type,
        task.prompt,
        task.assignment,
        task.allowed_tools,
        task.fork_context,
        task.started_at,
        task.max_steps,
        task.input_rx,
    )
    .await;

    let mut manager = task.manager_handle.write().await;
    match &result {
        Ok(res) => manager.update_from_result(&task.agent_id, res.clone()),
        Err(err) => manager.update_failed(&task.agent_id, err.to_string()),
    }

    // Emit BOTH a human-friendly summary (rendered in the parent's
    // sidebar / cell) AND a structured sentinel the model can recognize
    // on its next turn. Format: human summary on the first line,
    // sentinel on the second. The sentinel uses an opaque tag
    // (`deepseek:agent.done`) to avoid collision with normal user
    // text.
    let (summary, sentinel) = match &result {
        Ok(res) => (
            summarize_agent_result(res),
            agent_done_sentinel(&task.agent_id, res),
        ),
        Err(err) => (
            format!("Failed: {err}"),
            agent_failed_sentinel(&task.agent_id, &err.to_string()),
        ),
    };

    if let Some(mb) = task.runtime.mailbox.as_ref() {
        let envelope = match &result {
            Ok(_) => MailboxMessage::Completed {
                agent_id: task.agent_id.clone(),
                summary: summary.clone(),
            },
            Err(err) => MailboxMessage::Failed {
                agent_id: task.agent_id.clone(),
                error: err.to_string(),
            },
        };
        let _ = mb.send(envelope);
    }

    let payload = format!("{summary}\n{sentinel}");

    // Wake the engine's parent turn loop if this is one of its direct
    // children (issue #756). Gating by `spawn_depth == 1` means the parent
    // only sees completions for agents it directly orchestrated, not for
    // grandchildren spawned recursively inside its children.
    emit_parent_completion(&task.runtime, &task.agent_id, &payload);

    if let Some(event_tx) = task.runtime.event_tx {
        let _ = event_tx.try_send(Event::AgentComplete {
            id: task.agent_id,
            result: payload,
        });
    }
}

/// Notify the engine's parent turn loop that a direct child finished
/// (issue #756). Returns `true` if a send was attempted, `false` if the
/// notification was skipped because this isn't a direct child or no channel
/// is wired. Skips silently when the channel sender has no receiver — the
/// engine outlives the runtime, so a dropped receiver means we're shutting
/// down anyway.
pub(crate) fn emit_parent_completion(
    runtime: &AgentRuntime,
    agent_id: &str,
    payload: &str,
) -> bool {
    if runtime.spawn_depth != 1 {
        return false;
    }
    let Some(tx) = runtime.parent_completion_tx.as_ref() else {
        return false;
    };
    let _ = tx.send(AgentCompletion {
        agent_id: agent_id.to_string(),
        payload: payload.to_string(),
    });
    true
}

/// Build a `<deepseek:agent.done>` JSON sentinel for a successful child.
/// Intended to surface in the parent's transcript so the model recognizes
/// child completion and can decide whether to read the full result via
/// `agent_eval`.
///
/// Keep this payload deliberately lean. The human summary is emitted on the
/// line immediately before the sentinel; duplicating it here bloats the next
/// parent request's cache-miss tail. Wall-clock duration is useful UI
/// telemetry, but it is volatile and not useful for model coordination.
fn agent_done_sentinel(agent_id: &str, res: &AgentResult) -> String {
    let payload = json!({
        "agent_id": agent_id,
        "agent_type": res.agent_type.as_str(),
        "status": agent_status_name(&res.status),
        "summary_location": "previous_line",
        "details": "agent_eval",
    });
    format!("<deepseek:agent.done>{payload}</deepseek:agent.done>")
}

/// Build a `<deepseek:agent.done>` sentinel for a failed child.
fn agent_failed_sentinel(agent_id: &str, _err: &str) -> String {
    let payload = json!({
        "agent_id": agent_id,
        "status": "failed",
        "error_location": "previous_line",
        "details": "agent_eval",
    });
    format!("<deepseek:agent.done>{payload}</deepseek:agent.done>")
}

#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
async fn run_agent(
    runtime: &AgentRuntime,
    agent_id: String,
    agent_type: AgentRole,
    prompt: String,
    assignment: AgentAssignment,
    allowed_tools: Option<Vec<String>>,
    fork_context: bool,
    started_at: Instant,
    max_steps: u32,
    mut input_rx: mpsc::UnboundedReceiver<AgentInput>,
) -> Result<AgentResult> {
    let system_prompt = build_agent_system_prompt(&agent_type, &assignment, &runtime.role_configs);
    let fork_context_enabled = fork_context;
    let fork_context = fork_context_enabled
        .then_some(runtime.fork_context.as_ref())
        .flatten();
    let request_system = agent_request_system_prompt(&system_prompt, fork_context);

    // Load knowledge reference files for user-defined custom types.
    let prompt_with_knowledge = if let AgentRole::Named(ref name) = agent_type {
        if let Some(ct) = runtime.role_configs.get(name) {
            augment_prompt_with_knowledge(&prompt, ct)
        } else {
            prompt.clone()
        }
    } else {
        prompt.clone()
    };

    let mut messages =
        build_initial_agent_messages(&prompt_with_knowledge, &assignment, &agent_type, fork_context, &runtime.role_configs);
    let runtime_for_tools = runtime.clone().with_fork_context(AgentForkContext {
        system: Some(request_system.clone()),
        messages: messages.clone(),
        structured_state_block: None,
    });
    let tool_registry = AgentToolRegistry::new(
        runtime_for_tools,
        allowed_tools.clone(),
        Arc::new(Mutex::new(TodoList::new())),
        Arc::new(Mutex::new(PlanState::default())),
    );
    let unavailable_tools = tool_registry.unavailable_allowed_tools();
    if !unavailable_tools.is_empty() {
        return Err(anyhow!(
            "Agent requested unavailable tools: {}",
            unavailable_tools.join(", ")
        ));
    }
    let tools = tool_registry.tools_for_model(&agent_type);
    if let Some(mb) = runtime.mailbox.as_ref() {
        let _ = mb.send(MailboxMessage::started(&agent_id, agent_type.clone()));
    }
    emit_agent_progress(
        runtime.event_tx.as_ref(),
        runtime.mailbox.as_ref(),
        &agent_id,
        format!("started ({})", agent_type.as_str()),
    );

    let mut steps = 0;
    let mut final_result: Option<String> = None;
    let mut pending_inputs: VecDeque<AgentInput> = VecDeque::new();

    for _step in 0..max_steps {
        // Cooperative cancellation: bail if the parent (or root) cancelled
        // us while we were between steps. Children derive their token from
        // the parent's via `child_token()` so this propagates the whole tree.
        if runtime.cancel_token.is_cancelled() {
            emit_agent_progress(
                runtime.event_tx.as_ref(),
                runtime.mailbox.as_ref(),
                &agent_id,
                format!("step {steps}/{max_steps}: cancelled"),
            );
            if let Some(mb) = runtime.mailbox.as_ref() {
                let _ = mb.send(MailboxMessage::Cancelled {
                    agent_id: agent_id.clone(),
                });
            }
            return Ok(AgentResult {
                name: agent_id.clone(),
                agent_id: agent_id.clone(),
                context_mode: if fork_context_enabled {
                    "forked"
                } else {
                    "fresh"
                }
                .to_string(),
                fork_context: fork_context_enabled,
                agent_type: agent_type.clone(),
                assignment: assignment.clone(),
                model: runtime.model.clone(),
                nickname: None,
                status: AgentStatus::Cancelled,
                result: None,
                steps_taken: steps,
                duration_ms: u64::try_from(started_at.elapsed().as_millis()).unwrap_or(u64::MAX),
                from_prior_session: false,
            });
        }

        steps += 1;
        emit_agent_progress(
            runtime.event_tx.as_ref(),
            runtime.mailbox.as_ref(),
            &agent_id,
            format!("step {steps}/{max_steps}: requesting model response"),
        );

        while let Ok(input) = input_rx.try_recv() {
            if input.interrupt {
                pending_inputs.clear();
            }
            pending_inputs.push_back(input);
        }

        while let Some(input) = pending_inputs.pop_front() {
            if !input.text.trim().is_empty() {
                messages.push(Message {
                    role: "user".to_string(),
                    content: vec![ContentBlock::Text {
                        text: input.text,
                        cache_control: None,
                    }],
                });
            }
        }

        let request = MessageRequest {
            model: runtime.model.clone(),
            messages: messages.clone(),
            max_tokens: 4096,
            system: Some(request_system.clone()),
            tools: Some(tools.clone()),
            tool_choice: Some(json!({ "type": "auto" })),
            metadata: None,
            thinking: None,
            reasoning_effort: runtime.reasoning_effort.clone(),
            stream: Some(false),
            temperature: None,
            top_p: None,
        };

        // Race the API call against the cancellation token so a parent
        // cancel during a long thinking turn doesn't have to wait for the
        // step timeout.
        let response = tokio::select! {
            biased;
            () = runtime.cancel_token.cancelled() => {
                emit_agent_progress(
                    runtime.event_tx.as_ref(),
                    runtime.mailbox.as_ref(),
                    &agent_id,
                    format!("step {steps}/{max_steps}: cancelled mid-request"),
                );
                if let Some(mb) = runtime.mailbox.as_ref() {
                    let _ = mb.send(MailboxMessage::Cancelled {
                        agent_id: agent_id.clone(),
                    });
                }
                return Ok(AgentResult {
                    name: agent_id.clone(),
                    agent_id: agent_id.clone(),
                    context_mode: if fork_context_enabled { "forked" } else { "fresh" }.to_string(),
                    fork_context: fork_context_enabled,
                    agent_type: agent_type.clone(),
                    assignment: assignment.clone(),
                    model: runtime.model.clone(),
                    nickname: None,
                    status: AgentStatus::Cancelled,
                    result: None,
                    steps_taken: steps,
                    duration_ms: u64::try_from(started_at.elapsed().as_millis())
                        .unwrap_or(u64::MAX),
                    from_prior_session: false,
                });
            }
            api = tokio::time::timeout(STEP_API_TIMEOUT, runtime.client.create_message(request)) => {
                api.map_err(|_| anyhow!("API call timed out after {}s", STEP_API_TIMEOUT.as_secs()))??
            }
        };

        let mut tool_uses = Vec::new();

        // Report token usage so the parent's cost counter updates live.
        if let Some(mb) = runtime.mailbox.as_ref() {
            let _ = mb.send(MailboxMessage::token_usage(
                &agent_id,
                response.model.clone(),
                response.usage.clone(),
            ));
        }

        for block in &response.content {
            match block {
                ContentBlock::Text { text, .. } if !text.trim().is_empty() => {
                    final_result = Some(text.clone());
                }
                ContentBlock::ToolUse {
                    id, name, input, ..
                } => {
                    tool_uses.push((id.clone(), name.clone(), input.clone()));
                }
                _ => {}
            }
        }

        messages.push(Message {
            role: "assistant".to_string(),
            content: response.content.clone(),
        });

        if tool_uses.is_empty() {
            while let Ok(input) = input_rx.try_recv() {
                if input.interrupt {
                    pending_inputs.clear();
                }
                pending_inputs.push_back(input);
            }
            if pending_inputs.is_empty() {
                emit_agent_progress(
                    runtime.event_tx.as_ref(),
                    runtime.mailbox.as_ref(),
                    &agent_id,
                    format!("step {steps}/{max_steps}: complete"),
                );
                break;
            }
            continue;
        }

        emit_agent_progress(
            runtime.event_tx.as_ref(),
            runtime.mailbox.as_ref(),
            &agent_id,
            format!(
                "step {steps}/{max_steps}: executing {} tool call(s)",
                tool_uses.len()
            ),
        );
        let mut tool_results: Vec<ContentBlock> = Vec::new();
        for (tool_id, tool_name, tool_input) in tool_uses {
            emit_agent_progress(
                runtime.event_tx.as_ref(),
                runtime.mailbox.as_ref(),
                &agent_id,
                format!("step {steps}/{max_steps}: running tool '{tool_name}'"),
            );
            if let Some(mb) = runtime.mailbox.as_ref() {
                let _ = mb.send(MailboxMessage::ToolCallStarted {
                    agent_id: agent_id.clone(),
                    tool_name: tool_name.clone(),
                    step: steps,
                });
            }
            let result = match tokio::time::timeout(TOOL_TIMEOUT, async {
                tool_registry
                    .execute(&agent_id, &tool_name, tool_input)
                    .await
            })
            .await
            {
                Ok(Ok(output)) => output,
                Ok(Err(e)) => format!("Error: {e}"),
                Err(_) => format!("Error: Tool {tool_name} timed out"),
            };
            let tool_ok = !result.starts_with("Error:");
            emit_agent_progress(
                runtime.event_tx.as_ref(),
                runtime.mailbox.as_ref(),
                &agent_id,
                format!("step {steps}/{max_steps}: finished tool '{tool_name}'"),
            );
            if let Some(mb) = runtime.mailbox.as_ref() {
                let _ = mb.send(MailboxMessage::ToolCallCompleted {
                    agent_id: agent_id.clone(),
                    tool_name: tool_name.clone(),
                    step: steps,
                    ok: tool_ok,
                });
            }

            tool_results.push(ContentBlock::ToolResult {
                tool_use_id: tool_id,
                content: result,
                is_error: None,
                content_blocks: None,
            });
        }

        if !tool_results.is_empty() {
            messages.push(Message {
                role: "user".to_string(),
                content: tool_results,
            });
        }
    }

    release_resident_leases_for(&agent_id);

    Ok(AgentResult {
        name: agent_id.clone(),
        agent_id,
        context_mode: if fork_context_enabled {
            "forked"
        } else {
            "fresh"
        }
        .to_string(),
        fork_context: fork_context_enabled,
        agent_type,
        assignment,
        model: runtime.model.clone(),
        nickname: None,
        status: AgentStatus::Completed,
        result: final_result,
        steps_taken: steps,
        duration_ms: u64::try_from(started_at.elapsed().as_millis()).unwrap_or(u64::MAX),
        from_prior_session: false,
    })
}

async fn wait_for_result(
    manager: &SharedAgentManager,
    agent_id: &str,
    timeout: Duration,
) -> Result<(AgentResult, bool), ToolError> {
    let deadline = Instant::now() + timeout;

    loop {
        let snapshot = {
            let manager = manager.read().await;
            manager
                .get_result(agent_id)
                .map_err(|e| ToolError::execution_failed(e.to_string()))?
        };

        if snapshot.status != AgentStatus::Running {
            return Ok((snapshot, false));
        }
        if Instant::now() >= deadline {
            return Ok((snapshot, true));
        }

        tokio::time::sleep(RESULT_POLL_INTERVAL).await;
    }
}

#[allow(dead_code)] // Legacy agent_wait helper; agent_eval uses wait_for_result.
async fn wait_for_agents(
    manager: &SharedAgentManager,
    ids: &[String],
    wait_mode: WaitMode,
    timeout: Duration,
) -> Result<(Vec<AgentResult>, bool), ToolError> {
    let deadline = Instant::now() + timeout;

    loop {
        let snapshots = {
            let manager = manager.read().await;
            ids.iter()
                .map(|id| {
                    manager
                        .get_result(id)
                        .map_err(|e| ToolError::execution_failed(e.to_string()))
                })
                .collect::<Result<Vec<_>, _>>()?
        };

        if wait_mode.condition_met(&snapshots) {
            return Ok((snapshots, false));
        }
        if Instant::now() >= deadline {
            return Ok((snapshots, true));
        }

        tokio::time::sleep(RESULT_POLL_INTERVAL).await;
    }
}

fn parse_wait_mode(input: &Value) -> Result<WaitMode, ToolError> {
    let raw_mode = input
        .get("wait_mode")
        .and_then(|v| v.as_str())
        .unwrap_or("any");
    WaitMode::from_str(raw_mode).ok_or_else(|| {
        ToolError::invalid_input(format!("Invalid wait_mode '{raw_mode}'. Use: any or all"))
    })
}

fn parse_wait_ids(input: &Value) -> Vec<String> {
    let mut ids = Vec::new();
    for key in ["ids", "agent_ids"] {
        if let Some(list) = input.get(key).and_then(|v| v.as_array()) {
            for value in list {
                if let Some(id) = value.as_str() {
                    let id = id.trim();
                    if !id.is_empty() && !ids.iter().any(|existing| existing == id) {
                        ids.push(id.to_string());
                    }
                }
            }
        }
    }

    for key in ["agent_id", "id"] {
        if let Some(id) = input.get(key).and_then(|v| v.as_str()) {
            let id = id.trim();
            if !id.is_empty() && !ids.iter().any(|existing| existing == id) {
                ids.push(id.to_string());
            }
        }
    }

    ids
}

fn optional_input_str<'a>(input: &'a Value, keys: &[&str]) -> Option<&'a str> {
    keys.iter()
        .filter_map(|key| input.get(*key).and_then(Value::as_str))
        .map(str::trim)
        .find(|value| !value.is_empty())
}

fn parse_text_or_items(
    input: &Value,
    text_keys: &[&str],
    items_key: &str,
    required_field: &str,
) -> Result<String, ToolError> {
    let text = optional_input_str(input, text_keys).map(str::to_string);
    let items = parse_items_text(input, items_key)?;
    match (text, items) {
        (Some(_), Some(_)) => Err(ToolError::invalid_input(format!(
            "Provide either {required_field} text or {items_key}, but not both"
        ))),
        (Some(text), None) => Ok(text),
        (None, Some(items)) => Ok(items),
        (None, None) => Err(ToolError::missing_field(required_field)),
    }
}

fn parse_optional_text_or_items(
    input: &Value,
    text_keys: &[&str],
    items_key: &str,
) -> Result<Option<String>, ToolError> {
    let text = optional_input_str(input, text_keys).map(str::to_string);
    let items = parse_items_text(input, items_key)?;
    match (text, items) {
        (Some(_), Some(_)) => Err(ToolError::invalid_input(format!(
            "Provide either {} text or {}, but not both",
            text_keys[0], items_key
        ))),
        (Some(text), None) => Ok(Some(text)),
        (None, Some(items)) => Ok(Some(items)),
        (None, None) => Ok(None),
    }
}

fn parse_items_text(input: &Value, key: &str) -> Result<Option<String>, ToolError> {
    let Some(items) = input.get(key) else {
        return Ok(None);
    };
    let array = items
        .as_array()
        .ok_or_else(|| ToolError::invalid_input(format!("'{key}' must be an array")))?;
    if array.is_empty() {
        return Err(ToolError::invalid_input(format!("'{key}' cannot be empty")));
    }

    let mut lines = Vec::new();
    for item in array {
        let object = item
            .as_object()
            .ok_or_else(|| ToolError::invalid_input("each item must be an object"))?;
        let item_type = object
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or("text")
            .trim();
        let rendered = match item_type {
            "text" => object
                .get("text")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|text| !text.is_empty())
                .map(str::to_string)
                .ok_or_else(|| ToolError::invalid_input("text item requires non-empty text"))?,
            "mention" => {
                let name = object
                    .get("name")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|text| !text.is_empty())
                    .ok_or_else(|| ToolError::invalid_input("mention item requires name"))?;
                let path = object
                    .get("path")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|text| !text.is_empty())
                    .ok_or_else(|| ToolError::invalid_input("mention item requires path"))?;
                format!("[mention:${name}]({path})")
            }
            "skill" => {
                let name = object
                    .get("name")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|text| !text.is_empty())
                    .ok_or_else(|| ToolError::invalid_input("skill item requires name"))?;
                let path = object
                    .get("path")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|text| !text.is_empty())
                    .ok_or_else(|| ToolError::invalid_input("skill item requires path"))?;
                format!("[skill:${name}]({path})")
            }
            "local_image" => {
                let path = object
                    .get("path")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|text| !text.is_empty())
                    .ok_or_else(|| ToolError::invalid_input("local_image item requires path"))?;
                format!("[local_image:{path}]")
            }
            "image" => {
                let url = object
                    .get("image_url")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|text| !text.is_empty())
                    .ok_or_else(|| ToolError::invalid_input("image item requires image_url"))?;
                format!("[image:{url}]")
            }
            _ => object
                .get("text")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|text| !text.is_empty())
                .map(str::to_string)
                .unwrap_or_else(|| "[input]".to_string()),
        };
        lines.push(rendered);
    }

    Ok(Some(lines.join("\n")))
}

fn parse_spawn_request(input: &Value) -> Result<SpawnRequest, ToolError> {
    let prompt = parse_text_or_items(
        input,
        &["prompt", "message", "objective"],
        "items",
        "prompt",
    )?;
    let session_name = optional_input_str(input, &["name", "session_name"])
        .map(validate_session_name)
        .transpose()?;

    let role_input = optional_input_str(input, &["role", "agent_role"]);

    let agent_type = role_input
        .map(|kind| {
            AgentRole::from_str(kind).ok_or_else(|| {
                ToolError::invalid_input(format!(
                    "Invalid agent role '{kind}'. Use: {VALID_AGENT_ROLES}"
                ))
            })
        })
        .transpose()?
        .unwrap_or(AgentRole::General);

    if let Some(role) = role_input
        && normalize_role_alias(role).is_none()
    {
        return Err(ToolError::invalid_input(format!(
            "Invalid role alias '{role}'. Use: worker, explorer, awaiter, default"
        )));
    }

    let role = role_input
        .and_then(normalize_role_alias)
        .map(str::to_string);

    let allowed_tools = input
        .get("allowed_tools")
        .and_then(|v| v.as_array())
        .map(|items| {
            let mut tools = Vec::new();
            for item in items {
                if let Some(tool) = item.as_str() {
                    let trimmed = tool.trim();
                    if !trimmed.is_empty() && !tools.iter().any(|existing| existing == trimmed) {
                        tools.push(trimmed.to_string());
                    }
                }
            }
            tools
        });

    let cwd = parse_optional_cwd(input)?;
    let model = parse_optional_agent_model(input, "model")?;
    let resident_file = input
        .get("resident_file")
        .and_then(|v| v.as_str())
        .map(str::to_string)
        .filter(|s| !s.trim().is_empty());
    let fork_context =
        parse_optional_bool(input, &["fork_context", "forkContext", "inherit_context"])
            .unwrap_or(false);
    let max_depth = input
        .get("max_depth")
        .or_else(|| input.get("maxDepth"))
        .or_else(|| input.get("max_spawn_depth"))
        .and_then(Value::as_u64)
        .map(|depth| {
            u32::try_from(depth)
                .map_err(|_| ToolError::invalid_input("max_depth must be between 0 and 3"))
                .and_then(|depth| {
                    if depth <= 3 {
                        Ok(depth)
                    } else {
                        Err(ToolError::invalid_input(
                            "max_depth must be between 0 and 3",
                        ))
                    }
                })
        })
        .transpose()?;

    Ok(SpawnRequest {
        session_name,
        prompt: prompt.clone(),
        agent_type,
        assignment: AgentAssignment::new(prompt, role),
        allowed_tools,
        model,
        cwd,
        resident_file,
        fork_context,
        max_depth,
    })
}

fn validate_session_name(name: &str) -> Result<String, ToolError> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Err(ToolError::invalid_input("name cannot be blank"));
    }
    if trimmed.chars().any(char::is_whitespace) {
        return Err(ToolError::invalid_input(
            "name must not contain whitespace; use letters, numbers, '-', '_', or '.'",
        ));
    }
    if !trimmed
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.'))
    {
        return Err(ToolError::invalid_input(
            "name may only contain ASCII letters, numbers, '-', '_', or '.'",
        ));
    }
    Ok(trimmed.to_string())
}

fn parse_optional_bool(input: &Value, names: &[&str]) -> Option<bool> {
    names
        .iter()
        .find_map(|name| input.get(*name))
        .and_then(Value::as_bool)
}

fn with_default_fork_context(mut input: Value, default: bool) -> Value {
    let Some(object) = input.as_object_mut() else {
        return input;
    };
    if !object.contains_key("fork_context")
        && !object.contains_key("forkContext")
        && !object.contains_key("inherit_context")
    {
        object.insert("fork_context".to_string(), Value::Bool(default));
    }
    input
}

pub(crate) fn normalize_requested_agent_model(
    value: &str,
    field: &str,
) -> Result<String, ToolError> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(ToolError::invalid_input(format!("{field} cannot be blank")));
    }
    deepseek_config::normalize_model_name(trimmed).ok_or_else(|| {
        ToolError::invalid_input(format!(
            "Invalid {field} '{trimmed}'. Expected a DeepSeek model id such as deepseek-v4-pro or deepseek-v4-flash"
        ))
    })
}

pub(crate) fn configured_model_for_role_or_type(
    runtime: &AgentRuntime,
    role: Option<&str>,
    agent_type: &AgentRole,
) -> Result<Option<String>, ToolError> {
    let mut keys = Vec::new();
    if let Some(role) = role.map(str::trim).filter(|role| !role.is_empty()) {
        keys.push(role.to_ascii_lowercase());
    }
    keys.push(agent_type.as_str().to_string());
    keys.push("default".to_string());

    for key in keys {
        if let Some(model) = runtime.role_models.get(&key) {
            return normalize_requested_agent_model(model, &format!("agents.{key}.model"))
                .map(Some);
        }
    }
    Ok(None)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AgentResolvedRoute {
    pub(crate) model: String,
    pub(crate) reasoning_effort: Option<String>,
}

pub(crate) async fn resolve_agent_assignment_route(
    runtime: &AgentRuntime,
    configured_model: Option<String>,
    prompt: &str,
) -> AgentResolvedRoute {
    let explicit_model = configured_model.is_some();
    let mut route = fallback_agent_assignment_route(runtime, configured_model, prompt);

    if should_use_agent_flash_router(runtime)
        && let Ok(Some(recommendation)) = agent_flash_router(runtime, prompt).await
    {
        if runtime.auto_model && !explicit_model {
            route.model = recommendation.model;
        }
        if runtime.reasoning_effort_auto {
            // AutoRouteRecommendation doesn't carry reasoning_effort;
            // keep the fallback value resolved by fallback_agent_assignment_route.
        }
    }

    route
}

fn should_use_agent_flash_router(runtime: &AgentRuntime) -> bool {
    runtime.auto_model
}

fn fallback_agent_assignment_route(
    runtime: &AgentRuntime,
    configured_model: Option<String>,
    prompt: &str,
) -> AgentResolvedRoute {
    let model = if let Some(model) = configured_model {
        model
    } else if runtime.auto_model {
        deepseek_shared::auto_route::auto_model_heuristic(prompt, &runtime.model)
            .map(|r| r.model)
            .unwrap_or_else(|| runtime.model.clone())
    } else {
        runtime.model.clone()
    };

    let reasoning_effort = if runtime.reasoning_effort_auto {
        let effort = match deepseek_base::auto_reasoning::select(false, prompt) {
            deepseek_base::mode_types::ReasoningEffort::Low | deepseek_base::mode_types::ReasoningEffort::Medium => {
                deepseek_base::mode_types::ReasoningEffort::High
            }
            other => other,
        };
        Some(effort.as_setting().to_string())
    } else {
        runtime.reasoning_effort.clone()
    };

    AgentResolvedRoute {
        model,
        reasoning_effort,
    }
}

async fn agent_flash_router(
    runtime: &AgentRuntime,
    prompt: &str,
) -> Result<Option<deepseek_shared::auto_route::AutoRouteRecommendation>> {
    if cfg!(test) {
        return Ok(None);
    }

    let request = MessageRequest {
        model: "deepseek-v4-flash".to_string(),
        messages: vec![Message {
            role: "user".to_string(),
            content: vec![ContentBlock::Text {
                text: agent_router_prompt(runtime, prompt),
                cache_control: None,
            }],
        }],
        max_tokens: 96,
        system: Some(SystemPrompt::Text(
            SUBAGENT_ROUTER_SYSTEM_PROMPT.to_string(),
        )),
        tools: None,
        tool_choice: None,
        metadata: None,
        thinking: None,
        reasoning_effort: Some("off".to_string()),
        stream: Some(false),
        temperature: Some(0.0),
        top_p: None,
    };

    let response = tokio::time::timeout(
        Duration::from_secs(4),
        runtime.client.create_message(request),
    )
    .await??;
    Ok(deepseek_shared::auto_route::parse_auto_route_recommendation(
        &message_response_text(&response.content),
    ))
}

const SUBAGENT_ROUTER_SYSTEM_PROMPT: &str = "\
You are the DeepSeek TUI agent routing manager. Return only compact JSON: \
{\"model\":\"deepseek-v4-flash|deepseek-v4-pro\",\"thinking\":\"off|high|max\"}. \
Treat each child assignment like a customer request entering a team queue: decide the least \
sufficient worker and thinking budget for that assignment. Do not treat being a agent as \
important by itself. Use Flash for trivial, read-only, status, lookup, or single-step work. \
Use Pro for coding, debugging, release work, multi-file changes, security, architecture, \
high-risk decisions, ambiguous requests, or work likely to need tool-call judgment. Use thinking \
off for trivial no-tool work, high for ordinary reasoning, and max only for hard, risky, \
multi-step, uncertain, or tool-heavy work.";

fn agent_router_prompt(runtime: &AgentRuntime, prompt: &str) -> String {
    format!(
        "Parent selected model mode: {}\nParent selected thinking mode: {}\n\nAgent assignment:\n{}\n\nReturn JSON only.",
        if runtime.auto_model { "auto" } else { "fixed" },
        if runtime.reasoning_effort_auto {
            "auto"
        } else {
            runtime
                .reasoning_effort
                .as_deref()
                .unwrap_or("provider-default")
        },
        truncate_agent_router_prompt(prompt, 4_000)
    )
}

fn truncate_agent_router_prompt(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_string();
    }
    let mut out = text.chars().take(max_chars).collect::<String>();
    out.push_str("\n[truncated]");
    out
}

fn message_response_text(blocks: &[ContentBlock]) -> String {
    let mut out = String::new();
    for block in blocks {
        match block {
            ContentBlock::Text { text, .. } => {
                if !out.is_empty() {
                    out.push('\n');
                }
                out.push_str(text);
            }
            ContentBlock::Thinking { thinking } => {
                if !out.is_empty() {
                    out.push('\n');
                }
                out.push_str(thinking);
            }
            _ => {}
        }
    }
    out
}

fn parse_optional_agent_model(input: &Value, key: &str) -> Result<Option<String>, ToolError> {
    match input.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(value)) => normalize_requested_agent_model(value, key).map(Some),
        Some(_) => Err(ToolError::invalid_input(format!("{key} must be a string"))),
    }
}

/// Extract an optional `cwd: String` from spawn input and convert to a
/// `PathBuf`. Empty / absent → `None`. Workspace-boundary check happens
/// at spawn time (the parent's workspace is known there, not here).
fn parse_optional_cwd(input: &Value) -> Result<Option<PathBuf>, ToolError> {
    let raw = input.get("cwd").and_then(|v| v.as_str()).map(str::trim);
    match raw {
        None | Some("") => Ok(None),
        Some(s) => Ok(Some(PathBuf::from(s))),
    }
}

fn parse_assign_request(input: &Value) -> Result<AssignRequest, ToolError> {
    let agent_id = input
        .get("agent_id")
        .or_else(|| input.get("id"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|id| !id.is_empty())
        .ok_or_else(|| ToolError::missing_field("agent_id"))?
        .to_string();
    let objective = optional_input_str(input, &["objective"]).map(str::to_string);
    let role = optional_input_str(input, &["role", "agent_role"])
        .map(|role| {
            normalize_role_alias(role).ok_or_else(|| {
                ToolError::invalid_input(format!(
                    "Invalid role alias '{role}'. Use: worker, explorer, awaiter, default"
                ))
            })
        })
        .transpose()?
        .map(str::to_string);
    let message = parse_optional_text_or_items(input, &["message", "input"], "items")?;
    let interrupt = optional_bool(input, "interrupt", true);

    if objective.is_none() && role.is_none() && message.is_none() {
        return Err(ToolError::invalid_input(
            "Provide at least one of objective, role/agent_role, message/input, or items"
                .to_string(),
        ));
    }

    Ok(AssignRequest {
        agent_id,
        objective,
        role,
        message,
        interrupt,
    })
}

fn normalize_role_alias(input: &str) -> Option<&'static str> {
    match input.to_ascii_lowercase().as_str() {
        "default" => Some("default"),
        "worker" | "general" => Some("worker"),
        "explorer" | "explore" => Some("explorer"),
        "awaiter" | "plan" | "planner" => Some("awaiter"),
        _ => None,
    }
}

fn build_assignment_prompt(
    prompt: &str,
    assignment: &AgentAssignment,
    agent_type: &AgentRole,
) -> String {
    let role = assignment.role.as_deref().unwrap_or("default");
    format!(
        "Assignment metadata:\n- objective: {}\n- role: {}\n- resolved_type: {}\n\nTask:\n{}",
        assignment.objective,
        role,
        agent_type.as_str(),
        prompt
    )
}

fn emit_agent_progress(
    event_tx: Option<&mpsc::Sender<Event>>,
    mailbox: Option<&Mailbox>,
    agent_id: &str,
    status: String,
) {
    if let Some(mb) = mailbox {
        let _ = mb.send(MailboxMessage::progress(agent_id, status.clone()));
    }
    if let Some(event_tx) = event_tx {
        let _ = event_tx.try_send(Event::AgentProgress {
            id: agent_id.to_string(),
            status,
        });
    }
}

// === Tool Registry Helpers ===

/// Per-agent tool registry.
///
/// Two modes:
/// - **Full inheritance** (`allowed_tools = None`): the child sees the same
///   tool surface as the parent's Agent mode — every tool family including
///   `with_agent_tools` (so it can recurse). Approval-gated tools are
///   callable only when the parent runtime is auto-approved.
/// - **Explicit narrow** (`allowed_tools = Some(list)`): legacy / Custom
///   path. The registry still builds the full surface, but only the listed
///   tool names are visible to the model and callable.
struct AgentToolRegistry {
    /// `None` → full inheritance (no allowlist filter applied). `Some(list)` →
    /// only the listed tools are visible to the model and callable.
    allowed_tools: Option<Vec<String>>,
    auto_approve: bool,
    registry: ToolRegistry,
}

impl AgentToolRegistry {
    fn new(
        runtime: AgentRuntime,
        explicit_allowed_tools: Option<Vec<String>>,
        todo_list: SharedTodoList,
        plan_state: SharedPlanState,
    ) -> Self {
        // Build the full agent surface — same as the parent's Agent mode.
        // Children inherit shell, file, patch, search, web, git, diagnostics,
        // review, RLM, agent management (so grandchildren can spawn),
        // plus per-child fresh todo/plan state.
        let context = runtime.context.clone();
        let registry = ToolRegistryBuilder::new()
            .with_full_agent_surface(
                Some(runtime.client.clone()),
                runtime.model.clone(),
                runtime.manager.clone(),
                runtime.clone(),
                runtime.allow_shell,
                todo_list,
                plan_state,
            )
            .build(context);

        Self {
            allowed_tools: explicit_allowed_tools,
            auto_approve: runtime.context.auto_approve,
            registry,
        }
    }

    /// Whether a given tool name is permitted under this child's filter.
    /// `None` filter = everything permitted.
    fn is_tool_allowed(&self, name: &str) -> bool {
        match &self.allowed_tools {
            None => true,
            Some(list) => list.iter().any(|t| t == name),
        }
    }

    fn tools_for_model(&self, agent_type: &AgentRole) -> Vec<Tool> {
        let disallowed = match agent_type {
            // Review agents should not spawn agents (#1489).
            AgentRole::Named(name) if name == "review" => &["agent_spawn"][..],
            _ => &[][..],
        };
        let api_tools = self.registry.to_api_tools();
        let filtered = match &self.allowed_tools {
            None => api_tools,
            Some(list) => api_tools
                .into_iter()
                .filter(|tool| list.contains(&tool.name))
                .collect::<Vec<_>>(),
        };
        if disallowed.is_empty() {
            filtered
        } else {
            filtered
                .into_iter()
                .filter(|tool| !disallowed.contains(&tool.name.as_str()))
                .collect()
        }
    }

    fn unavailable_allowed_tools(&self) -> Vec<String> {
        match &self.allowed_tools {
            None => Vec::new(),
            Some(list) => list
                .iter()
                .filter(|name| !self.registry.contains(name))
                .cloned()
                .collect(),
        }
    }

    async fn execute(&self, _agent_id: &str, name: &str, input: Value) -> Result<String> {
        if !self.is_tool_allowed(name) {
            return Err(anyhow!("Tool {name} not allowed for this agent"));
        }
        if !self.auto_approve {
            let Some(spec) = self.registry.get(name) else {
                return Err(anyhow!("Tool {name} is not registered"));
            };
            if spec.approval_requirement() != ApprovalRequirement::Auto {
                return Err(anyhow!(
                    "Tool {name} requires approval and cannot run inside this agent unless the parent session is auto-approved"
                ));
            }
        }
        reject_agent_terminal_takeover(name, &input)?;
        self.registry
            .execute(name, input)
            .await
            .map_err(|e| anyhow!(e))
    }
}

fn reject_agent_terminal_takeover(name: &str, input: &Value) -> Result<()> {
    let wants_interactive_shell = name == "exec_shell"
        && input
            .get("interactive")
            .and_then(Value::as_bool)
            .unwrap_or(false);
    if wants_interactive_shell {
        return Err(anyhow!(
            "Agents run in the background and cannot use exec_shell with interactive=true \
             because that would take over the parent TUI terminal. Use non-interactive \
             exec_shell, background=true, tty=true, or task_shell_start instead."
        ));
    }
    Ok(())
}

/// Resolve the effective allowed-tools list for a child.
///
/// **v0.6.6 default: full inheritance.** Returning `Ok(None)` means the
/// child sees the same tool surface as the parent's Agent mode — every
/// family including `with_agent_tools` so it can recurse. The narrowing
/// path (`Ok(Some(list))`) is only used by:
/// - `Custom` agent types (which require an explicit list).
/// - Callers that pass `explicit_tools` (advanced / legacy use).
///
/// `allow_shell = false` no longer narrows the tool LIST — the child's
/// registry simply doesn't register shell tools, which has the same
/// effect without papering over the parent's choice with a deny-list.
fn build_allowed_tools(
    agent_type: &AgentRole,
    explicit_tools: Option<Vec<String>>,
    _allow_shell: bool,
) -> Result<Option<Vec<String>>> {
    if let Some(tools) = explicit_tools {
        let mut deduped = Vec::new();
        for tool in tools {
            let name = tool.trim();
            if !name.is_empty() && !deduped.iter().any(|existing: &String| existing == name) {
                deduped.push(name.to_string());
            }
        }
        if matches!(agent_type, AgentRole::Named(name) if name == "custom") && deduped.is_empty() {
            return Err(anyhow!(
                "Custom agent requires a non-empty allowed_tools list"
            ));
        }
        return Ok(Some(deduped));
    }

    if matches!(agent_type, AgentRole::Named(name) if name == "custom") {
        return Err(anyhow!(
            "Custom agent requires a non-empty allowed_tools list"
        ));
    }

    // Default: full registry inheritance from the parent. The child sees every
    // tool the parent has, including the agent management family. The
    // registry execution guard still blocks approval-gated tools unless the
    // parent runtime is auto-approved.
    Ok(None)
}

fn summarize_agent_result(result: &AgentResult) -> String {
    match (&result.status, result.result.as_ref()) {
        (AgentStatus::Completed, Some(text)) => truncate_preview(text),
        (AgentStatus::Completed, None) => "Completed (no output)".to_string(),
        (AgentStatus::Interrupted(error), _) => format!("Interrupted: {error}"),
        (AgentStatus::Cancelled, _) => "Cancelled".to_string(),
        (AgentStatus::Failed(error), _) => format!("Failed: {error}"),
        (AgentStatus::Running, _) => "Running".to_string(),
    }
}

fn agent_status_name(status: &AgentStatus) -> &'static str {
    match status {
        AgentStatus::Running => "running",
        AgentStatus::Completed => "completed",
        AgentStatus::Interrupted(_) => "interrupted",
        AgentStatus::Failed(_) => "failed",
        AgentStatus::Cancelled => "cancelled",
    }
}

fn truncate_preview(text: &str) -> String {
    const MAX_LEN: usize = 240;
    if text.len() <= MAX_LEN {
        text.to_string()
    } else {
        format!("{}...", text.chars().take(MAX_LEN).collect::<String>())
    }
}

/// Prepend knowledge reference files to the agent prompt.
///
/// Each `knowledge_paths` entry is expanded with `expand_path` so `~`
/// and env vars work. File contents are wrapped in a markdown block with
/// the path as label, then prepended before the user prompt.
fn augment_prompt_with_knowledge(prompt: &str, ct: &deepseek_config::AgentRoleConfig) -> String {
    let Some(ref paths) = ct.knowledge_paths else {
        return prompt.to_string();
    };
    let mut knowledge_blocks = Vec::new();
    for raw_path in paths {
        let path = deepseek_config::expand_path(raw_path);
        match std::fs::read_to_string(&path) {
            Ok(content) => {
                let label = path.display();
                // Truncate per-file to 50 KiB to avoid blowing the prompt budget.
                let cap = 50 * 1024;
                let body = if content.len() > cap {
                    let truncated: String = content.chars().take(cap).collect();
                    format!("{truncated}\n\n[truncated to {cap} chars]")
                } else {
                    content
                };
                knowledge_blocks.push(format!(
                    "## Knowledge: {label}\n\n```\n{body}\n```"
                ));
            }
            Err(err) => {
                knowledge_blocks.push(format!(
                    "<!-- Failed to read knowledge file {}: {err} -->",
                    path.display()
                ));
            }
        }
    }
    if knowledge_blocks.is_empty() {
        return prompt.to_string();
    }
    format!("{navigation}\n\n{prompt}", navigation = knowledge_blocks.join("\n\n"))
}

pub(crate) const AGENT_OUTPUT_FORMAT: &str = include_str!("../../../../../assets/prompts/subagent_output_format.md");

pub(crate) const GENERAL_AGENT_INTRO: &str = concat!(
    "You are a general-purpose agent spawned to handle a specific task autonomously.\n",
    "Stay inside the assigned scope; put adjacent work under RISKS/BLOCKERS.\n",
    "Plan multi-step work with `checklist_write`; add `update_plan` for complex strategy.\n\n"
);

/// Embedded default configurations for built-in agent roles.
///
/// Each TOML file lives in `crates/tui/assets/builtin-roles/` and is compiled
/// into the binary via `include_str!`. At runtime these defaults are overlaid by
/// `~/.deepseek/roles/<name>/role.toml` and `config.toml [agents.types.<name>]`
/// entries, which take precedence.
const BUILTIN_ROLE_EXPLORE: &str = include_str!("../../../../../assets/builtin-roles/explore.toml");
const BUILTIN_ROLE_PLAN: &str = include_str!("../../../../../assets/builtin-roles/plan.toml");
const BUILTIN_ROLE_REVIEW: &str = include_str!("../../../../../assets/builtin-roles/review.toml");
const BUILTIN_ROLE_IMPLEMENTER: &str = include_str!("../../../../../assets/builtin-roles/implementer.toml");
const BUILTIN_ROLE_VERIFIER: &str = include_str!("../../../../../assets/builtin-roles/verifier.toml");
const BUILTIN_ROLE_CUSTOM: &str = include_str!("../../../../../assets/builtin-roles/custom.toml");

/// Returns the built-in default agent role configurations.
///
/// These are the base layer that `~/.deepseek/roles/` and
/// `config.toml [agents.types]` overlay at runtime.
pub fn builtin_role_configs() -> std::collections::HashMap<String, deepseek_config::AgentRoleConfig> {
    let mut map = std::collections::HashMap::new();
    for (name, toml_str) in [
        ("explore", BUILTIN_ROLE_EXPLORE),
        ("plan", BUILTIN_ROLE_PLAN),
        ("review", BUILTIN_ROLE_REVIEW),
        ("implementer", BUILTIN_ROLE_IMPLEMENTER),
        ("verifier", BUILTIN_ROLE_VERIFIER),
        ("custom", BUILTIN_ROLE_CUSTOM),
    ] {
        if let Ok(config) = toml::from_str::<deepseek_config::AgentRoleConfig>(toml_str) {
            map.insert(name.to_string(), config);
        }
    }
    map
}

// === Tests ===

#[cfg(test)]
mod tests;