//! Routing modes for LLM interceptor

use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;

/// Routing mode for LLM interceptor
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum RoutingMode {
    /// Pass through all requests without modification
    Bypass,
    /// Use LLM predictions for routing decisions
    Semantic,
    /// Combine database rules with LLM predictions
    Hybrid,
}

impl RoutingMode {
    /// Get display name for routing mode
    pub fn display_name(&self) -> &'static str {
        match self {
            RoutingMode::Bypass => "Bypass",
            RoutingMode::Semantic => "Semantic",
            RoutingMode::Hybrid => "Hybrid",
        }
    }

    /// Get icon for routing mode
    pub fn icon(&self) -> &'static str {
        match self {
            RoutingMode::Bypass => "🔓",
            RoutingMode::Semantic => "🧠",
            RoutingMode::Hybrid => "⚡",
        }
    }

    /// Get description for routing mode
    pub fn description(&self) -> &'static str {
        match self {
            RoutingMode::Bypass => "Direct pass-through without LLM processing",
            RoutingMode::Semantic => "LLM predicts optimal routing for each request",
            RoutingMode::Hybrid => "Database rules with LLM fallback",
        }
    }

    /// Serialize to canonical string (lowercase) for configs/IPCs.
    pub fn as_str(&self) -> &'static str {
        match self {
            RoutingMode::Bypass => "bypass",
            RoutingMode::Semantic => "semantic",
            RoutingMode::Hybrid => "hybrid",
        }
    }
}

impl Default for RoutingMode {
    fn default() -> Self {
        RoutingMode::Hybrid
    }
}

impl fmt::Display for RoutingMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for RoutingMode {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_ascii_lowercase().as_str() {
            "bypass" => Ok(RoutingMode::Bypass),
            "semantic" => Ok(RoutingMode::Semantic),
            "hybrid" => Ok(RoutingMode::Hybrid),
            other => Err(format!("Unsupported routing_mode '{}'", other)),
        }
    }
}

/// Routing configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoutingConfig {
    pub mode: RoutingMode,
    pub confidence_threshold: f32,
    pub enable_learning: bool,
    pub fallback_to_bypass: bool,
}

impl Default for RoutingConfig {
    fn default() -> Self {
        Self {
            mode: RoutingMode::default(),
            confidence_threshold: 0.8,
            enable_learning: true,
            fallback_to_bypass: true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_routing_mode_display() {
        assert_eq!(RoutingMode::Bypass.display_name(), "Bypass");
        assert_eq!(RoutingMode::Semantic.display_name(), "Semantic");
        assert_eq!(RoutingMode::Hybrid.display_name(), "Hybrid");
    }

    #[test]
    fn test_routing_mode_icons() {
        assert_eq!(RoutingMode::Bypass.icon(), "🔓");
        assert_eq!(RoutingMode::Semantic.icon(), "🧠");
        assert_eq!(RoutingMode::Hybrid.icon(), "⚡");
    }
}
