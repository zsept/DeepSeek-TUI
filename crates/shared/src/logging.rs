//! Verbose-logging re-exports from `deepseek_utils`.
//!
//! All call sites in `core/` and `main.rs` reference a `logging` module
//! that lives in the engine crate root. This thin module forwards to
//! the shared helpers in `deepseek_utils` so the callers don't need to
//! be rewritten.

pub use deepseek_utils::{
    env_requests_verbose_logging,
    info,
    is_verbose,
    set_verbose,
    warn,
};
