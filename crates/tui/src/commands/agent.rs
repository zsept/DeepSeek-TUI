//! Role command: list and switch parent agent roles.


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

    // Check if input matches a known agent role (general or from config).
    if let Some(role) = deepseek_tui::tools::agent::AgentRole::from_str(input) {
        if matches!(role, deepseek_tui::tools::agent::AgentRole::General) {
            return CommandResult::message(
                "'general' is the default parent role. Use /role reset to restore it."
            );
        }
        return CommandResult::message(format!(
            "'{input}' is a sub-agent role, not a switchable parent role.\n\
             Use agent_open(agent_role=\"{input}\") to spawn it as a child agent.\n\
             To use it as a parent role, define it in roles/{input}/role.toml or [agents.types.{input}] in config.toml."
        ));
    }

    CommandResult::error(format!(
        "Unknown agent type: {input}\n\
         Use /role to list available types, or define a custom type in roles/{input}/role.toml."
    ))
}

fn list_agents(app: &App) -> CommandResult {
    let mut out = String::from("Available agent types:\n\n");

    // General is always available as the default.
    out.push_str("── General ──\n");
    out.push_str("  general         Full tool access, multi-step autonomous tasks\n");

    // All roles from config (built-in defaults + user overrides).
    if !app.agent_role_configs.is_empty() {
        // Sort for stable output: general first, then alphabetical.
        let mut names: Vec<&String> = app.agent_role_configs.keys().collect();
        names.sort();

        out.push_str(&format!(
            "\n── Agent roles ({}) ──\n",
            app.agent_role_configs.len()
        ));
        for name in names {
            let ct = &app.agent_role_configs[name];
            let marker = if app.active_agent_type.as_deref() == Some(name.as_str()) {
                " [active]"
            } else {
                ""
            };
            let tools = ct.allowed_tools.as_ref()
                .map(|t| t.join(", "))
                .unwrap_or_else(|| "full".to_string());
            let model = ct.model.as_deref().unwrap_or("inherit");
            let effort = ct.reasoning_effort.as_deref().unwrap_or("inherit");
            // Show a one-line summary from the system prompt.
            let summary = ct.system_prompt
                .lines()
                .next()
                .map(|line| line.trim().trim_end_matches('.'))
                .unwrap_or("");
            out.push_str(&format!(
                "  {name:<14}{marker}\n    {summary}\n    model={model}  think={effort}  tools=[{tools}]\n\n"
            ));
        }
        if app.active_agent_type.is_some() {
            out.push_str("Use /role reset to restore default parent behavior.\n");
        }
    }

    out.push_str("\nOverride built-in roles in ~/.deepseek/roles/<name>/role.toml or [agents.types.<name>] in config.toml.\n");

    CommandResult::message(out)
}
