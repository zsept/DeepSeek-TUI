//! Role command: list and switch parent agent roles.

use crate::tui::app::App;

use super::CommandResult;

pub fn agent(app: &mut App, arg: Option<&str>) -> CommandResult {
    let input = arg.map(str::trim).unwrap_or("");

    if input.is_empty() || input.eq_ignore_ascii_case("list") {
        return list_agents(app);
    }
    if input.eq_ignore_ascii_case("reset") || input.eq_ignore_ascii_case("default") {
        app.active_agent_type = None;
        app.agent_system_prompt_override = None;
        return CommandResult::message("Agent role reset to default. Next turn will use the base system prompt.");
    }

    // Look up in user-defined custom types.
    if let Some(ct) = app.agent_role_configs.get(input) {
        app.active_agent_type = Some(input.to_string());
        app.agent_system_prompt_override = Some(ct.system_prompt.clone());
        let model_note = ct.model.as_ref()
            .map(|m| format!("\nModel override: {m} (use /model to apply)"))
            .unwrap_or_default();
        return CommandResult::message(format!(
            "Active agent role: {input}\n\n{}{model_note}",
            ct.system_prompt.trim()
        ));
    }

    // Check built-in types — these are child-agent only.
    if crate::tools::subagent::AgentRole::from_str(input).is_some() {
        return CommandResult::message(format!(
            "'{input}' is a built-in agent role, not a switchable parent role.\n\
             Use agent_open(type=\"{input}\") to spawn it as a child agent.\n\
             To switch the parent agent, define a custom type in roles/{input}/role.toml."
        ));
    }

    CommandResult::error(format!(
        "Unknown agent type: {input}\n\
         Use /role to list available types, or define a custom type in roles/{input}/role.toml."
    ))
}

fn list_agents(app: &App) -> CommandResult {
    let mut out = String::from("Available sub-agent types:\n\n");

    // Built-in
    let builtins: &[(&str, &str)] = &[
        ("general",      "Full tool access, multi-step autonomous tasks"),
        ("explore",      "Read-only codebase reconnaissance"),
        ("plan",         "Architectural planning, no code edits"),
        ("review",       "Code review with severity-scored findings"),
        ("implementer",  "Focused code changes, minimal surrounding edits"),
        ("verifier",     "Run tests and validation gates, report pass/fail"),
        ("custom",       "Narrowed toolset defined at spawn time"),
    ];
    out.push_str("── Built-in (sub-agent only) ──\n");
    for (name, desc) in builtins {
        out.push_str(&format!("  {name:<14} {desc}\n"));
    }

    // User-defined custom types
    if !app.agent_role_configs.is_empty() {
        out.push_str(&format!(
            "\n── Custom ({}) ──\n",
            app.agent_role_configs.len()
        ));
        for (name, ct) in &app.agent_role_configs {
            let marker = if app.active_agent_type.as_deref() == Some(name) {
                " [active]"
            } else {
                ""
            };
            let tools = ct.allowed_tools.as_ref()
                .map(|t| t.join(", "))
                .unwrap_or_else(|| "full".to_string());
            let model = ct.model.as_deref().unwrap_or("inherit");
            let effort = ct.reasoning_effort.as_deref().unwrap_or("inherit");
            out.push_str(&format!(
                "  {name}{marker}\n    model={model}  think={effort}  tools=[{tools}]\n\n"
            ));
        }
        if app.active_agent_type.is_some() {
            out.push_str("Use /role reset to restore default parent behavior.\n");
        }
    } else {
        out.push_str("\nNo custom types defined. Add them in [subagents.types] to create switchable parent roles.\n");
    }

    CommandResult::message(out)
}
