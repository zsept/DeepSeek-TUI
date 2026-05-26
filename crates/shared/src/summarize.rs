//! Tool result summarization stubs — full impl in crates/tui.

use serde_json::Value;

pub fn summarize_tool_args(_name: &str, _args: &Value) -> String {
    format!("{}", _name)
}

pub fn summarize_tool_output(_name: &str, _output: &Value) -> String {
    String::new()
}
