//! Core engine module for `DeepSeek` CLI.
//!
//! This module provides the event-driven architecture that separates
//! the UI from the AI interaction logic:
//!
//! - `engine`: The main engine that processes operations
//! - `events`: Events emitted by the engine to the UI
//! - `ops`: Operations submitted by the UI to the engine
//! - `session`: Session state management
//! - `turn`: Turn context and tracking

// Engine code runs inside the TUI alt-screen — see `runtime_log` for why
// raw stdio prints must not appear here. Use `tracing::*` instead.
#![deny(clippy::print_stdout)]
#![deny(clippy::print_stderr)]

pub mod client;
pub mod compaction;
pub mod cycle_manager;
pub mod engine;
pub mod error_taxonomy;
pub mod events;
pub mod llm_client;
pub mod ops;
pub mod prefix_cache;
pub mod repl;
pub mod rlm;
pub mod seam_manager;
pub mod session;
pub mod tool_parser;
pub mod turn;

// Re-exports
