//! Execpolicy rules loaded from TOML configuration.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::Deserialize;

use super::matcher::pattern_matches;
/// Simplified prefix-allow check extracted from command_safety.
fn prefix_allow_matches(pattern: &str, command: &str) -> bool {
    let p: String = pattern.trim().to_ascii_lowercase().split_whitespace().collect::<Vec<_>>().join(" ");
    let c: String = command.trim().to_ascii_lowercase().split_whitespace().collect::<Vec<_>>().join(" ");
    c == p || c.starts_with(&format!("{p} "))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExecPolicyRuleDecision {
    Allow,
    Deny(String),
    AskUser(String),
}

#[derive(Debug, Deserialize, Default)]
pub struct ExecPolicyConfig {
    #[serde(default)]
    pub rules: BTreeMap<String, RuleSet>,
}

#[derive(Debug, Deserialize, Default)]
pub struct RuleSet {
    #[serde(default)]
    pub allow: Vec<String>,
    #[serde(default)]
    pub deny: Vec<String>,
}

impl ExecPolicyConfig {
    pub fn from_str(contents: &str) -> Result<Self> {
        toml::from_str(contents).context("failed to parse execpolicy.toml")
    }

    pub fn from_path(path: &Path) -> Result<Self> {
        let contents = std::fs::read_to_string(path)
            .with_context(|| format!("failed to read execpolicy file {}", path.display()))?;
        Self::from_str(&contents)
    }

    pub fn evaluate(&self, command: &str) -> ExecPolicyRuleDecision {
        for (group, rules) in &self.rules {
            for pattern in &rules.deny {
                if pattern_matches(pattern, command) {
                    return ExecPolicyRuleDecision::Deny(format!(
                        "execpolicy denied by {group}: {pattern}"
                    ));
                }
            }
        }

        for (group, rules) in &self.rules {
            for pattern in &rules.allow {
                // Allow rules use arity-aware prefix matching first so that
                // `allow = ["git status"]` matches `git status -s` but NOT
                // `git push origin main`.  Fall back to regex-style
                // `pattern_matches` for wildcard patterns (e.g. `cargo *`).
                if prefix_allow_matches(pattern, command) || pattern_matches(pattern, command) {
                    let _ = group;
                    return ExecPolicyRuleDecision::Allow;
                }
            }
        }

        ExecPolicyRuleDecision::AskUser("execpolicy: no matching allow rule".to_string())
    }
}

pub fn default_execpolicy_path() -> Option<PathBuf> {
    dirs::home_dir().map(|home| home.join(".deepseek").join("execpolicy.toml"))
}

pub fn load_default_policy() -> Result<Option<ExecPolicyConfig>> {
    let Some(path) = default_execpolicy_path() else {
        return Ok(None);
    };
    if !path.exists() {
        return Ok(None);
    }
    ExecPolicyConfig::from_path(&path).map(Some)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_execpolicy_evaluate() {
        let config = ExecPolicyConfig {
            rules: BTreeMap::from([
                (
                    "git".to_string(),
                    RuleSet {
                        allow: vec!["git status".to_string(), "git log *".to_string()],
                        deny: vec!["git push --force".to_string()],
                    },
                ),
                (
                    "danger".to_string(),
                    RuleSet {
                        allow: vec![],
                        deny: vec!["rm -rf /".to_string()],
                    },
                ),
            ]),
        };

        assert!(matches!(
            config.evaluate("git status"),
            ExecPolicyRuleDecision::Allow
        ));
        assert!(matches!(
            config.evaluate("git log --oneline"),
            ExecPolicyRuleDecision::Allow
        ));
        assert!(matches!(
            config.evaluate("git push --force"),
            ExecPolicyRuleDecision::Deny(_)
        ));
        assert!(matches!(
            config.evaluate("unknown command"),
            ExecPolicyRuleDecision::AskUser(_)
        ));
    }

    #[test]
    fn test_prefix_rule_allows_git_status_with_flags() {
        // Arity-aware: `allow = ["git status"]` must match `git status -s`.
        let config = ExecPolicyConfig {
            rules: BTreeMap::from([(
                "git".to_string(),
                RuleSet {
                    allow: vec!["git status".to_string()],
                    deny: vec![],
                },
            )]),
        };

        assert!(matches!(
            config.evaluate("git status -s"),
            ExecPolicyRuleDecision::Allow
        ));
        assert!(matches!(
            config.evaluate("git status --porcelain"),
            ExecPolicyRuleDecision::Allow
        ));
        // Push must NOT match the "git status" allow rule.
        assert!(matches!(
            config.evaluate("git push origin main"),
            ExecPolicyRuleDecision::AskUser(_)
        ));
    }

    #[test]
    fn test_prefix_rule_allows_cargo_check_variants() {
        let config = ExecPolicyConfig {
            rules: BTreeMap::from([(
                "cargo".to_string(),
                RuleSet {
                    allow: vec!["cargo check".to_string()],
                    deny: vec![],
                },
            )]),
        };

        assert!(matches!(
            config.evaluate("cargo check"),
            ExecPolicyRuleDecision::Allow
        ));
        assert!(matches!(
            config.evaluate("cargo check --workspace"),
            ExecPolicyRuleDecision::Allow
        ));
        assert!(matches!(
            config.evaluate("cargo build --release"),
            ExecPolicyRuleDecision::AskUser(_)
        ));
    }
}
