use serde::{Deserialize, Serialize};

/// Simplified per-domain allow/deny policy for the skills crate.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct NetworkPolicy {
    /// Decision for hosts that match neither `allow` nor `deny`.
    #[serde(default)]
    pub default: DecisionToml,
    /// Hosts that should be allowed without prompting.
    #[serde(default)]
    pub allow: Vec<String>,
    /// Hosts that should always be denied.
    #[serde(default)]
    pub deny: Vec<String>,
}

/// TOML-level decision representing the default fallback.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default)]
pub enum DecisionToml {
    #[default]
    Prompt,
    Allow,
    Deny,
}

/// Outcome of a network policy check.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    Allow,
    Deny,
    Prompt,
}

impl Decision {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Allow => "Allow",
            Self::Deny => "Deny",
            Self::Prompt => "Prompt",
        }
    }
}

impl From<DecisionToml> for Decision {
    fn from(d: DecisionToml) -> Self {
        match d {
            DecisionToml::Allow => Self::Allow,
            DecisionToml::Deny => Self::Deny,
            DecisionToml::Prompt => Self::Prompt,
        }
    }
}

impl NetworkPolicy {
    /// Decide whether a request to `host` should be allowed, denied, or
    /// prompt for approval.
    #[must_use]
    pub fn decide(&self, host: &str) -> Decision {
        let normalized = normalize_host(host);
        if normalized.is_empty() {
            return self.default.into();
        }
        if self.deny.iter().any(|e| host_matches(e, &normalized)) {
            return Decision::Deny;
        }
        if self.allow.iter().any(|e| host_matches(e, &normalized)) {
            return Decision::Allow;
        }
        self.default.into()
    }
}

fn normalize_host(host: &str) -> String {
    let trimmed = host.trim().trim_end_matches('.').to_ascii_lowercase();
    if let Some(rest) = trimmed.strip_prefix("*.") {
        format!(".{rest}")
    } else {
        trimmed
    }
}

fn host_matches(entry: &str, normalized_host: &str) -> bool {
    let entry_norm = normalize_host(entry);
    if let Some(suffix) = entry_norm.strip_prefix('.') {
        if suffix.is_empty() {
            return false;
        }
        normalized_host.ends_with(&format!(".{suffix}"))
    } else {
        entry_norm == normalized_host
    }
}

/// Extract a hostname from a URL string. Returns `None` for malformed URLs.
#[must_use]
pub fn host_from_url(url: &str) -> Option<String> {
    url.split("://")
        .nth(1)
        .unwrap_or(url)
        .split('/')
        .next()
        .unwrap_or("")
        .split(':')
        .next()
        .map(str::to_string)
        .filter(|h| !h.is_empty())
}
