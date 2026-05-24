//! Auto-route stubs — full implementation in crates/tui.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutoRouteRecommendation {
    pub model: String,
    pub reason: String,
    pub role: Option<String>,
}

#[derive(Debug, Clone, Copy)]
pub enum AutoRouteSource {
    FlashRouter,
    Heuristic,
}

impl AutoRouteSource {
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            AutoRouteSource::FlashRouter => "flash-router",
            AutoRouteSource::Heuristic => "heuristic",
        }
    }
}

#[derive(Debug, Clone)]
pub struct AutoRouteSelection {
    pub model: String,
    pub provider: String,
    pub reasoning_effort: Option<String>,
    pub objective: Option<String>,
    pub source: AutoRouteSource,
}

pub async fn resolve_auto_route_with_flash(
    _config: &crate::config::Config,
    _model: &str,
    _prompt: &str,
) -> Option<AutoRouteSelection> {
    None
}

pub fn auto_model_heuristic(_prompt: &str, _model: &str) -> Option<AutoRouteRecommendation> {
    None
}

pub fn parse_auto_route_recommendation(_text: &str) -> Option<AutoRouteRecommendation> {
    None
}

pub fn normalize_auto_route_effort(_effort: &str) -> String {
    String::new()
}

pub async fn resolve_cli_auto_route(_config: &crate::config::Config, _model: &str, _prompt: &str) -> AutoRouteSelection {
    AutoRouteSelection { model: String::new(), provider: String::new(), reasoning_effort: None, objective: None, source: AutoRouteSource::Heuristic }
}
