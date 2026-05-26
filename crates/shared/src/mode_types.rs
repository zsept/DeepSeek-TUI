//! Domain mode enums — part of engine logic, not UI.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AppMode {
    Limited,
    Yolo,
    Plan,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum ApprovalMode {
    #[default]
    Suggest,
    Auto,
    Never,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ReasoningEffort {
    Off,
    Low,
    Medium,
    High,
    Auto,
    #[default]
    Max,
}

impl AppMode {
    pub fn as_setting(&self) -> &'static str {
        match self {
            AppMode::Limited => "limited",
            AppMode::Yolo => "yolo",
            AppMode::Plan => "plan",
        }
    }
    
    pub fn label(&self) -> &'static str {
        self.as_setting()
    }
}

impl ReasoningEffort {
    #[must_use]
    pub fn from_setting(s: &str) -> Option<Self> {
        match s {
            "off" => Some(Self::Off),
            "low" => Some(Self::Low),
            "medium" => Some(Self::Medium),
            "high" => Some(Self::High),
            "auto" => Some(Self::Auto),
            "max" => Some(Self::Max),
            _ => None,
        }
    }

    pub fn as_setting(&self) -> &'static str {
        match self {
            ReasoningEffort::Off => "off",
            ReasoningEffort::Low => "low",
            ReasoningEffort::Medium => "medium",
            ReasoningEffort::High => "high",
            ReasoningEffort::Auto => "auto",
            ReasoningEffort::Max => "max",
        }
    }
}

impl ApprovalMode {
    #[must_use]
    pub fn from_config_value(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "auto" => Some(Self::Auto),
            "suggest" => Some(Self::Suggest),
            "never" => Some(Self::Never),
            _ => None,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            ApprovalMode::Auto => "AUTO",
            ApprovalMode::Suggest => "SUGGEST",
            ApprovalMode::Never => "NEVER",
        }
    }
}
