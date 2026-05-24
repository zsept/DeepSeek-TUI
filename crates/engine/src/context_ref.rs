//! Shared context-reference types used by both library and binary targets.
use serde::{Serialize, Deserialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContextReference {
    pub kind: ContextReferenceKind,
    pub source: ContextReferenceSource,
    pub badge: String,
    pub label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
    #[serde(default)]
    pub included: bool,
    #[serde(default)]
    pub expanded: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ContextReferenceKind { File, AtSymbol }

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ContextReferenceSource { AtMention }
