//! MCP UI helpers stub — full implementation pending migration.

use std::path::Path;

pub fn init_config(_path: &Path, _force: bool) -> anyhow::Result<deepseek_mcp::McpWriteStatus> {
    Ok(deepseek_mcp::McpWriteStatus::Created)
}
pub fn add_server_config(
    _path: &Path,
    _name: String,
    _command: Option<String>,
    _url: Option<String>,
    _args: Vec<String>,
) -> anyhow::Result<deepseek_mcp::McpWriteStatus> {
    Ok(deepseek_mcp::McpWriteStatus::Created)
}
pub fn set_server_enabled(
    _path: &Path, _name: &str, _enabled: bool,
) -> anyhow::Result<deepseek_mcp::McpWriteStatus> {
    Ok(deepseek_mcp::McpWriteStatus::Created)
}
pub fn remove_server_config(
    _path: &Path, _name: &str,
) -> anyhow::Result<deepseek_mcp::McpWriteStatus> {
    Ok(deepseek_mcp::McpWriteStatus::Created)
}
pub async fn discover_manager_snapshot(
    _path: &Path,
    _network_policy: Option<deepseek_shared::network_policy::NetworkPolicyDecider>,
    _restart_required: bool,
) -> anyhow::Result<deepseek_mcp::McpManagerSnapshot> {
    Ok(deepseek_mcp::McpManagerSnapshot::default())
}
pub fn manager_snapshot_from_config(
    _path: &Path,
    _restart_required: bool,
) -> anyhow::Result<deepseek_mcp::McpManagerSnapshot> {
    Ok(deepseek_mcp::McpManagerSnapshot::default())
}
