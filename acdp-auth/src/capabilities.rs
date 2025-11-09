//! MCP Capabilities and Rate Limiting
//!
//! Defines what MCP tools an agent can access and rate limits.

use crate::error::{ACDPError, Result};
use serde::{Deserialize, Serialize};
use std::time::Duration;
use validator::Validate;

/// MCP Capabilities
///
/// Controls which MCP tools an agent can access.
/// Similar to AWS IAM policies but for MCP tools.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Validate)]
pub struct MCPCapabilities {
    /// Allowed tool patterns (glob-style)
    ///
    /// Examples:
    /// - "filesystem/*" - All filesystem tools
    /// - "filesystem/read_file" - Specific tool
    /// - "web-search/query" - Web search query
    #[validate(length(min = 1))]
    pub allowed_tools: Vec<ToolPattern>,

    /// Denied tool patterns (takes precedence over allowed)
    #[serde(default)]
    pub denied_tools: Vec<ToolPattern>,

    /// Resource limits
    #[serde(default)]
    pub resource_limits: ResourceLimits,

    /// Rate limit parameters
    pub rate_limit: RateLimitParams,
}

impl MCPCapabilities {
    /// Check if a tool is allowed
    pub fn is_tool_allowed(&self, tool_name: &str) -> Result<()> {
        // Check denied list first (takes precedence)
        for pattern in &self.denied_tools {
            if pattern.matches(tool_name) {
                return Err(ACDPError::ToolNotAllowed {
                    tool: tool_name.to_string(),
                });
            }
        }

        // Check allowed list
        for pattern in &self.allowed_tools {
            if pattern.matches(tool_name) {
                return Ok(());
            }
        }

        Err(ACDPError::ToolNotAllowed {
            tool: tool_name.to_string(),
        })
    }

    /// Check if capabilities are a subset of another (for delegation)
    pub fn is_subset_of(&self, parent: &MCPCapabilities) -> bool {
        // All allowed tools must be subset of parent's allowed tools
        for child_pattern in &self.allowed_tools {
            let mut found = false;
            for parent_pattern in &parent.allowed_tools {
                if child_pattern.is_subset_of(parent_pattern) {
                    found = true;
                    break;
                }
            }
            if !found {
                return false;
            }
        }

        // Rate limit must be <= parent
        if self.rate_limit.max_presentations > parent.rate_limit.max_presentations {
            return false;
        }

        // Resource limits must be <= parent
        if !self.resource_limits.is_subset_of(&parent.resource_limits) {
            return false;
        }

        true
    }
}

/// Tool Pattern (glob-style matching)
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ToolPattern {
    /// Pattern string
    ///
    /// Supports:
    /// - Exact match: "filesystem/read_file"
    /// - Wildcard: "filesystem/*"
    /// - Prefix: "filesystem/"
    pub pattern: String,
}

impl ToolPattern {
    /// Create a new tool pattern
    pub fn new(pattern: impl Into<String>) -> Self {
        Self {
            pattern: pattern.into(),
        }
    }

    /// Check if a tool name matches this pattern
    pub fn matches(&self, tool_name: &str) -> bool {
        if self.pattern.ends_with('*') {
            // Wildcard matching: "filesystem/*"
            let prefix = &self.pattern[..self.pattern.len() - 1];
            tool_name.starts_with(prefix)
        } else if self.pattern.ends_with('/') {
            // Prefix matching: "filesystem/"
            tool_name.starts_with(&self.pattern)
        } else {
            // Exact matching: "filesystem/read_file"
            tool_name == self.pattern
        }
    }

    /// Check if this pattern is a subset of another pattern
    pub fn is_subset_of(&self, parent: &ToolPattern) -> bool {
        if parent.pattern.ends_with('*') {
            // Parent is wildcard, check if we match prefix
            let prefix = &parent.pattern[..parent.pattern.len() - 1];
            self.pattern.starts_with(prefix)
        } else {
            // Parent is exact, we must match exactly
            self.pattern == parent.pattern
        }
    }
}

/// Rate Limit Parameters
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Validate)]
pub struct RateLimitParams {
    /// Maximum presentations allowed
    #[validate(range(min = 1))]
    pub max_presentations: u64,

    /// Time window for rate limiting
    #[serde(with = "duration_serde")]
    pub window: Duration,
}

impl RateLimitParams {
    /// Create new rate limit params
    pub fn new(max_presentations: u64, window: Duration) -> Self {
        Self {
            max_presentations,
            window,
        }
    }

    /// Create 24-hour window rate limit
    pub fn daily(max_presentations: u64) -> Self {
        Self::new(max_presentations, Duration::from_secs(24 * 60 * 60))
    }

    /// Create hourly rate limit
    pub fn hourly(max_presentations: u64) -> Self {
        Self::new(max_presentations, Duration::from_secs(60 * 60))
    }
}

/// Resource Limits
///
/// Limits on file read/write sizes, etc.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct ResourceLimits {
    /// Max bytes to read in one operation
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_read_bytes: Option<u64>,

    /// Max bytes to write in one operation
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_write_bytes: Option<u64>,

    /// Max concurrent requests
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_concurrent_requests: Option<u32>,
}

impl ResourceLimits {
    /// Check if these limits are a subset of parent limits
    pub fn is_subset_of(&self, parent: &ResourceLimits) -> bool {
        // If parent has limit, child must have same or lower limit
        if let Some(parent_max) = parent.max_read_bytes {
            if let Some(child_max) = self.max_read_bytes {
                if child_max > parent_max {
                    return false;
                }
            } else {
                // Child has no limit, parent has limit = not subset
                return false;
            }
        }

        if let Some(parent_max) = parent.max_write_bytes {
            if let Some(child_max) = self.max_write_bytes {
                if child_max > parent_max {
                    return false;
                }
            } else {
                return false;
            }
        }

        if let Some(parent_max) = parent.max_concurrent_requests {
            if let Some(child_max) = self.max_concurrent_requests {
                if child_max > parent_max {
                    return false;
                }
            } else {
                return false;
            }
        }

        true
    }
}

/// Serde module for Duration serialization
mod duration_serde {
    use serde::{Deserialize, Deserializer, Serializer};
    use std::time::Duration;

    pub fn serialize<S>(duration: &Duration, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_u64(duration.as_secs())
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Duration, D::Error>
    where
        D: Deserializer<'de>,
    {
        let secs = u64::deserialize(deserializer)?;
        Ok(Duration::from_secs(secs))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tool_pattern_matching() {
        let pattern = ToolPattern::new("filesystem/*");
        assert!(pattern.matches("filesystem/read_file"));
        assert!(pattern.matches("filesystem/write_file"));
        assert!(!pattern.matches("web-search/query"));

        let exact = ToolPattern::new("filesystem/read_file");
        assert!(exact.matches("filesystem/read_file"));
        assert!(!exact.matches("filesystem/write_file"));
    }

    #[test]
    fn test_tool_allowed() {
        let capabilities = MCPCapabilities {
            allowed_tools: vec![
                ToolPattern::new("filesystem/*"),
                ToolPattern::new("web-search/query"),
            ],
            denied_tools: vec![ToolPattern::new("filesystem/execute")],
            resource_limits: ResourceLimits::default(),
            rate_limit: RateLimitParams::daily(1000),
        };

        // Allowed
        assert!(capabilities.is_tool_allowed("filesystem/read_file").is_ok());
        assert!(capabilities.is_tool_allowed("web-search/query").is_ok());

        // Denied (takes precedence)
        assert!(capabilities.is_tool_allowed("filesystem/execute").is_err());

        // Not allowed
        assert!(capabilities.is_tool_allowed("database/query").is_err());
    }

    #[test]
    fn test_capabilities_subset() {
        let parent = MCPCapabilities {
            allowed_tools: vec![ToolPattern::new("filesystem/*")],
            denied_tools: vec![],
            resource_limits: ResourceLimits {
                max_read_bytes: Some(1_000_000),
                max_write_bytes: Some(100_000),
                max_concurrent_requests: None,
            },
            rate_limit: RateLimitParams::daily(1000),
        };

        let child = MCPCapabilities {
            allowed_tools: vec![ToolPattern::new("filesystem/read_file")],
            denied_tools: vec![],
            resource_limits: ResourceLimits {
                max_read_bytes: Some(100_000),
                max_write_bytes: Some(10_000),
                max_concurrent_requests: None,
            },
            rate_limit: RateLimitParams::daily(100),
        };

        assert!(child.is_subset_of(&parent));

        // Invalid subset (higher rate limit)
        let invalid_child = MCPCapabilities {
            allowed_tools: vec![ToolPattern::new("filesystem/read_file")],
            denied_tools: vec![],
            resource_limits: ResourceLimits::default(),
            rate_limit: RateLimitParams::daily(2000),
        };

        assert!(!invalid_child.is_subset_of(&parent));
    }

    #[test]
    fn test_rate_limit_helpers() {
        let daily = RateLimitParams::daily(1000);
        assert_eq!(daily.max_presentations, 1000);
        assert_eq!(daily.window, Duration::from_secs(24 * 60 * 60));

        let hourly = RateLimitParams::hourly(100);
        assert_eq!(hourly.max_presentations, 100);
        assert_eq!(hourly.window, Duration::from_secs(60 * 60));
    }
}
