//! Security policies for runtime selection and execution

use crate::limits::ResourceLimits;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

/// Runtime type selection
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RuntimeType {
    /// Direct process execution (fastest, no sandboxing)
    Process,
    /// V8 JavaScript runtime (good isolation)
    V8,
    /// WebAssembly runtime (best security)
    Wasm,
}

impl RuntimeType {
    /// Security ranking (lower is more secure)
    pub fn security_rank(self) -> u8 {
        match self {
            RuntimeType::Wasm => 0,    // Most secure
            RuntimeType::V8 => 1,      // Medium security
            RuntimeType::Process => 2, // Least secure
        }
    }

    /// Performance ranking (lower is faster)
    pub fn performance_rank(self) -> u8 {
        match self {
            RuntimeType::Process => 0, // Fastest
            RuntimeType::V8 => 1,      // Medium
            RuntimeType::Wasm => 2,    // Slowest (but still fast)
        }
    }
}

/// Trust level for tools
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TrustLevel {
    /// Completely untrusted (user-provided, unknown source)
    Untrusted,
    /// Third-party verified tool
    Verified,
    /// First-party trusted tool
    Trusted,
    /// System-level tool (full privileges)
    System,
}

/// Language requirements
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Language {
    JavaScript,
    Python,
    Wasm,
    Shell,
}

/// Runtime requirement specification
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum RuntimeRequirement {
    /// Specific runtime required
    Specific { runtime: RuntimeType },

    /// Automatic selection based on constraints
    Auto {
        language: Language,
        #[serde(default)]
        preferred: Option<RuntimeType>,
    },

    /// Any of these runtimes acceptable (service picks best)
    AnyOf { runtimes: Vec<RuntimeType> },
}

impl Default for RuntimeRequirement {
    fn default() -> Self {
        // Default to most secure
        RuntimeRequirement::Specific {
            runtime: RuntimeType::Wasm,
        }
    }
}

/// Security policy for a specific tool
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolPolicy {
    /// Tool identifier
    pub tool_id: String,

    /// Trust level
    pub trust_level: TrustLevel,

    /// Allowed runtimes (empty = use global defaults)
    #[serde(default)]
    pub allowed_runtimes: Vec<RuntimeType>,

    /// Resource limits (None = use defaults for trust level)
    #[serde(default)]
    pub limits: Option<ResourceLimits>,

    /// Whether audit logging is required
    #[serde(default)]
    pub audit_required: bool,
}

/// Global security policy configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecurityPolicy {
    /// Default runtime for each trust level
    #[serde(default = "default_runtime_by_trust")]
    pub default_runtime_by_trust: HashMap<TrustLevel, RuntimeType>,

    /// Allowed runtimes for each trust level
    #[serde(default = "default_allowed_by_trust")]
    pub allowed_runtimes_by_trust: HashMap<TrustLevel, HashSet<RuntimeType>>,

    /// Default limits for each trust level
    #[serde(default = "default_limits_by_trust")]
    pub limits_by_trust: HashMap<TrustLevel, ResourceLimits>,

    /// Per-tool overrides
    #[serde(default)]
    pub tool_policies: HashMap<String, ToolPolicy>,

    /// Global settings
    #[serde(default)]
    pub allow_process_runtime: bool,

    #[serde(default = "default_true")]
    pub prefer_security_over_performance: bool,
}

fn default_runtime_by_trust() -> HashMap<TrustLevel, RuntimeType> {
    let mut map = HashMap::new();
    map.insert(TrustLevel::Untrusted, RuntimeType::Wasm);
    map.insert(TrustLevel::Verified, RuntimeType::Wasm);
    map.insert(TrustLevel::Trusted, RuntimeType::V8);
    map.insert(TrustLevel::System, RuntimeType::V8);
    map
}

fn default_allowed_by_trust() -> HashMap<TrustLevel, HashSet<RuntimeType>> {
    let mut map = HashMap::new();

    // Untrusted: Only WASM
    map.insert(
        TrustLevel::Untrusted,
        vec![RuntimeType::Wasm].into_iter().collect(),
    );

    // Verified: WASM or V8
    map.insert(
        TrustLevel::Verified,
        vec![RuntimeType::Wasm, RuntimeType::V8]
            .into_iter()
            .collect(),
    );

    // Trusted: Any sandboxed runtime
    map.insert(
        TrustLevel::Trusted,
        vec![RuntimeType::Wasm, RuntimeType::V8]
            .into_iter()
            .collect(),
    );

    // System: All runtimes
    map.insert(
        TrustLevel::System,
        vec![RuntimeType::Wasm, RuntimeType::V8, RuntimeType::Process]
            .into_iter()
            .collect(),
    );

    map
}

fn default_limits_by_trust() -> HashMap<TrustLevel, ResourceLimits> {
    let mut map = HashMap::new();
    map.insert(TrustLevel::Untrusted, ResourceLimits::strict());
    map.insert(TrustLevel::Verified, ResourceLimits::default());
    map.insert(TrustLevel::Trusted, ResourceLimits::permissive());
    map.insert(TrustLevel::System, ResourceLimits::permissive());
    map
}

fn default_true() -> bool {
    true
}

impl Default for SecurityPolicy {
    fn default() -> Self {
        Self {
            default_runtime_by_trust: default_runtime_by_trust(),
            allowed_runtimes_by_trust: default_allowed_by_trust(),
            limits_by_trust: default_limits_by_trust(),
            tool_policies: HashMap::new(),
            allow_process_runtime: false,
            prefer_security_over_performance: true,
        }
    }
}

impl SecurityPolicy {
    /// Check if a runtime is allowed for a tool
    pub fn is_runtime_allowed(
        &self,
        tool_id: &str,
        trust_level: TrustLevel,
        runtime: RuntimeType,
    ) -> bool {
        // Check global Process runtime ban
        if runtime == RuntimeType::Process && !self.allow_process_runtime {
            return false;
        }

        // Check tool-specific policy
        if let Some(tool_policy) = self.tool_policies.get(tool_id) {
            if !tool_policy.allowed_runtimes.is_empty() {
                return tool_policy.allowed_runtimes.contains(&runtime);
            }
        }

        // Fall back to trust-level defaults
        self.allowed_runtimes_by_trust
            .get(&trust_level)
            .map(|set| set.contains(&runtime))
            .unwrap_or(false)
    }

    /// Get allowed runtimes for a tool
    pub fn allowed_runtimes(&self, tool_id: &str, trust_level: TrustLevel) -> HashSet<RuntimeType> {
        // Tool-specific overrides
        if let Some(tool_policy) = self.tool_policies.get(tool_id) {
            if !tool_policy.allowed_runtimes.is_empty() {
                let mut allowed: HashSet<_> =
                    tool_policy.allowed_runtimes.iter().copied().collect();

                // Filter out Process if globally disabled
                if !self.allow_process_runtime {
                    allowed.remove(&RuntimeType::Process);
                }

                return allowed;
            }
        }

        // Trust-level defaults
        let mut allowed = self
            .allowed_runtimes_by_trust
            .get(&trust_level)
            .cloned()
            .unwrap_or_default();

        // Filter out Process if globally disabled
        if !self.allow_process_runtime {
            allowed.remove(&RuntimeType::Process);
        }

        allowed
    }

    /// Get resource limits for a tool
    pub fn get_limits(&self, tool_id: &str, trust_level: TrustLevel) -> ResourceLimits {
        // Tool-specific limits
        if let Some(tool_policy) = self.tool_policies.get(tool_id) {
            if let Some(limits) = &tool_policy.limits {
                return limits.clone();
            }
        }

        // Trust-level defaults
        self.limits_by_trust
            .get(&trust_level)
            .cloned()
            .unwrap_or_default()
    }

    /// Check if audit logging is required for a tool
    pub fn requires_audit(&self, tool_id: &str, trust_level: TrustLevel) -> bool {
        // Tool-specific audit requirement
        if let Some(tool_policy) = self.tool_policies.get(tool_id) {
            if tool_policy.audit_required {
                return true;
            }
        }

        // System and Trusted tools always require audit
        matches!(trust_level, TrustLevel::System | TrustLevel::Trusted)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_policy() {
        let policy = SecurityPolicy::default();

        // Untrusted should only allow WASM
        let allowed = policy.allowed_runtimes("unknown_tool", TrustLevel::Untrusted);
        assert_eq!(allowed.len(), 1);
        assert!(allowed.contains(&RuntimeType::Wasm));

        // System should allow all except Process (globally disabled)
        let allowed = policy.allowed_runtimes("system_tool", TrustLevel::System);
        assert!(allowed.contains(&RuntimeType::Wasm));
        assert!(allowed.contains(&RuntimeType::V8));
        assert!(!allowed.contains(&RuntimeType::Process)); // Disabled by default
    }

    #[test]
    fn test_security_ranking() {
        assert!(RuntimeType::Wasm.security_rank() < RuntimeType::V8.security_rank());
        assert!(RuntimeType::V8.security_rank() < RuntimeType::Process.security_rank());
    }

    #[test]
    fn test_tool_specific_policy() {
        let mut policy = SecurityPolicy::default();

        policy.tool_policies.insert(
            "special_tool".to_string(),
            ToolPolicy {
                tool_id: "special_tool".to_string(),
                trust_level: TrustLevel::Trusted,
                allowed_runtimes: vec![RuntimeType::V8],
                limits: Some(ResourceLimits::strict()),
                audit_required: true,
            },
        );

        // Should use tool-specific policy
        assert!(policy.is_runtime_allowed("special_tool", TrustLevel::Trusted, RuntimeType::V8));
        assert!(!policy.is_runtime_allowed("special_tool", TrustLevel::Trusted, RuntimeType::Wasm));

        let limits = policy.get_limits("special_tool", TrustLevel::Trusted);
        assert_eq!(limits.max_duration, ResourceLimits::strict().max_duration);
    }
}
