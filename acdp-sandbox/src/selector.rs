//! Runtime selection with policy enforcement

use crate::limits::ResourceLimits;
use crate::policy::{Language, RuntimeRequirement, RuntimeType, SecurityPolicy, TrustLevel};
#[cfg(feature = "v8")]
use crate::runtime::V8Runtime;
#[cfg(feature = "wasm")]
use crate::runtime::WasmRuntime;
use crate::runtime::{ProcessRuntime, Runtime};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use thiserror::Error;

/// Tool definition for runtime selection
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDefinition {
    /// Unique tool identifier
    pub id: String,

    /// Display name
    pub name: String,

    /// Trust level
    pub trust_level: TrustLevel,

    /// Runtime requirements
    #[serde(default)]
    pub runtime: RuntimeRequirement,

    /// Description
    #[serde(default)]
    pub description: String,
}

/// Runtime selection decision
#[derive(Debug, Clone)]
pub struct RuntimeDecision {
    /// Selected runtime type
    pub runtime_type: RuntimeType,

    /// Resource limits to apply
    pub limits: ResourceLimits,

    /// Whether this was an override from requested
    pub is_override: bool,

    /// Reason for selection/override
    pub reason: String,

    /// Whether audit logging is required
    pub audit_required: bool,
}

/// Errors during runtime selection
#[derive(Debug, Error)]
pub enum SelectionError {
    #[error("No compatible runtime available for tool '{0}'")]
    NoCompatibleRuntime(String),

    #[error("Runtime '{0:?}' denied by policy for tool '{1}'")]
    RuntimeDenied(RuntimeType, String),

    #[error("Language '{0:?}' not supported by available runtimes")]
    UnsupportedLanguage(Language),

    #[error("Failed to create runtime: {0}")]
    RuntimeCreation(String),
}

/// Runtime selector with policy enforcement
pub struct RuntimeSelector {
    policy: Arc<SecurityPolicy>,
    available_runtimes: std::collections::HashSet<RuntimeType>,
}

impl RuntimeSelector {
    /// Create a new runtime selector
    pub fn new(policy: SecurityPolicy) -> Self {
        let mut available = std::collections::HashSet::new();

        // Check which runtimes are available based on features
        #[cfg(feature = "process")]
        available.insert(RuntimeType::Process);

        #[cfg(feature = "v8")]
        available.insert(RuntimeType::V8);

        #[cfg(feature = "wasm")]
        available.insert(RuntimeType::Wasm);

        Self {
            policy: Arc::new(policy),
            available_runtimes: available,
        }
    }

    /// Create with custom policy
    pub fn with_policy(policy: Arc<SecurityPolicy>) -> Self {
        let mut available = std::collections::HashSet::new();

        #[cfg(feature = "process")]
        available.insert(RuntimeType::Process);

        #[cfg(feature = "v8")]
        available.insert(RuntimeType::V8);

        #[cfg(feature = "wasm")]
        available.insert(RuntimeType::Wasm);

        Self {
            policy,
            available_runtimes: available,
        }
    }

    /// Select appropriate runtime for a tool
    pub fn select_runtime(&self, tool: &ToolDefinition) -> Result<RuntimeDecision, SelectionError> {
        // Get allowed runtimes from policy
        let policy_allowed = self.policy.allowed_runtimes(&tool.id, tool.trust_level);

        // Intersect with available runtimes
        let mut candidates: Vec<RuntimeType> = policy_allowed
            .intersection(&self.available_runtimes)
            .copied()
            .collect();

        if candidates.is_empty() {
            return Err(SelectionError::NoCompatibleRuntime(tool.id.clone()));
        }

        // Apply tool's runtime requirements
        let (chosen, is_override, reason) = match &tool.runtime {
            RuntimeRequirement::Specific { runtime } => {
                if candidates.contains(runtime) {
                    (
                        *runtime,
                        false,
                        format!("Tool requested {:?} and it's allowed", runtime),
                    )
                } else {
                    // Policy override - pick most secure available
                    candidates.sort_by_key(|rt| rt.security_rank());
                    let fallback = candidates[0];
                    (
                        fallback,
                        true,
                        format!(
                            "Tool requested {:?} but policy only allows {:?}",
                            runtime, candidates
                        ),
                    )
                }
            }

            RuntimeRequirement::Auto {
                language,
                preferred,
            } => {
                // Filter by language support
                candidates.retain(|rt| self.supports_language(*rt, *language));

                if candidates.is_empty() {
                    return Err(SelectionError::UnsupportedLanguage(*language));
                }

                // Check if preferred is available
                if let Some(pref) = preferred {
                    if candidates.contains(pref) {
                        (
                            *pref,
                            false,
                            format!("Auto-selected preferred runtime {:?}", pref),
                        )
                    } else {
                        // Pick best available for language
                        candidates.sort_by_key(|rt| rt.security_rank());
                        let chosen = candidates[0];
                        (
                            chosen,
                            true,
                            format!(
                                "Preferred {:?} not available, selected {:?} for {:?}",
                                pref, chosen, language
                            ),
                        )
                    }
                } else {
                    // No preference - pick most secure
                    candidates.sort_by_key(|rt| rt.security_rank());
                    let chosen = candidates[0];
                    (
                        chosen,
                        false,
                        format!(
                            "Auto-selected most secure runtime {:?} for {:?}",
                            chosen, language
                        ),
                    )
                }
            }

            RuntimeRequirement::AnyOf { runtimes } => {
                // Intersect requested with candidates
                let acceptable: Vec<_> = runtimes
                    .iter()
                    .filter(|rt| candidates.contains(rt))
                    .collect();

                if acceptable.is_empty() {
                    // None acceptable - pick most secure candidate
                    candidates.sort_by_key(|rt| rt.security_rank());
                    let fallback = candidates[0];
                    (
                        fallback,
                        true,
                        format!(
                            "None of requested runtimes {:?} allowed, fallback to {:?}",
                            runtimes, fallback
                        ),
                    )
                } else {
                    // Pick most secure from acceptable
                    let chosen = acceptable
                        .iter()
                        .min_by_key(|rt| rt.security_rank())
                        .copied()
                        .copied()
                        .unwrap();
                    (
                        chosen,
                        false,
                        format!(
                            "Selected most secure from acceptable runtimes: {:?}",
                            chosen
                        ),
                    )
                }
            }
        };

        // Get limits from policy
        let limits = self.policy.get_limits(&tool.id, tool.trust_level);

        // Check if audit is required
        let audit_required = self.policy.requires_audit(&tool.id, tool.trust_level);

        Ok(RuntimeDecision {
            runtime_type: chosen,
            limits,
            is_override,
            reason,
            audit_required,
        })
    }

    /// Create runtime instance from decision
    pub fn create_runtime(
        &self,
        decision: &RuntimeDecision,
    ) -> Result<Box<dyn Runtime>, SelectionError> {
        let runtime: Box<dyn Runtime> = match decision.runtime_type {
            #[cfg(feature = "process")]
            RuntimeType::Process => Box::new(ProcessRuntime::new()),

            #[cfg(not(feature = "process"))]
            RuntimeType::Process => {
                return Err(SelectionError::RuntimeCreation(
                    "Process runtime not compiled in".to_string(),
                ))
            }

            #[cfg(feature = "v8")]
            RuntimeType::V8 => Box::new(V8Runtime::with_limits(decision.limits.clone())),

            #[cfg(not(feature = "v8"))]
            RuntimeType::V8 => {
                return Err(SelectionError::RuntimeCreation(
                    "V8 runtime not compiled in".to_string(),
                ))
            }

            #[cfg(feature = "wasm")]
            RuntimeType::Wasm => Box::new(
                WasmRuntime::with_limits(decision.limits.clone())
                    .map_err(|e| SelectionError::RuntimeCreation(e.to_string()))?,
            ),

            #[cfg(not(feature = "wasm"))]
            RuntimeType::Wasm => {
                return Err(SelectionError::RuntimeCreation(
                    "WASM runtime not compiled in".to_string(),
                ))
            }
        };

        Ok(runtime)
    }

    /// Check if runtime supports a language
    fn supports_language(&self, runtime: RuntimeType, language: Language) -> bool {
        match (runtime, language) {
            (RuntimeType::V8, Language::JavaScript) => true,
            (RuntimeType::Wasm, Language::Wasm) => true,
            (RuntimeType::Wasm, Language::Python) => true, // Via Pyodide
            (RuntimeType::Process, _) => true,             // Process can run anything
            _ => false,
        }
    }

    /// Get policy reference
    pub fn policy(&self) -> &SecurityPolicy {
        &self.policy
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_runtime_selection_untrusted() {
        let policy = SecurityPolicy::default();
        let selector = RuntimeSelector::new(policy);

        let tool = ToolDefinition {
            id: "untrusted_tool".to_string(),
            name: "Untrusted Tool".to_string(),
            trust_level: TrustLevel::Untrusted,
            runtime: RuntimeRequirement::Auto {
                language: Language::Wasm, // WASM language for WASM runtime
                preferred: None,
            },
            description: String::new(),
        };

        #[cfg(feature = "wasm")]
        {
            let decision = selector.select_runtime(&tool).unwrap();
            // Untrusted tools should get WASM
            assert_eq!(decision.runtime_type, RuntimeType::Wasm);
            assert!(decision.audit_required == false); // Untrusted doesn't require audit by default
        }
    }

    #[test]
    fn test_runtime_selection_override() {
        let policy = SecurityPolicy::default();
        let selector = RuntimeSelector::new(policy);

        let tool = ToolDefinition {
            id: "bad_tool".to_string(),
            name: "Bad Tool".to_string(),
            trust_level: TrustLevel::Untrusted,
            runtime: RuntimeRequirement::Specific {
                runtime: RuntimeType::Process, // Tries to request Process
            },
            description: String::new(),
        };

        #[cfg(feature = "wasm")]
        {
            let decision = selector.select_runtime(&tool).unwrap();
            // Should be overridden to WASM
            assert_eq!(decision.runtime_type, RuntimeType::Wasm);
            assert!(decision.is_override);
        }
    }

    #[test]
    fn test_security_ranking() {
        // Verify our security assumptions
        assert!(RuntimeType::Wasm.security_rank() < RuntimeType::V8.security_rank());
        assert!(RuntimeType::V8.security_rank() < RuntimeType::Process.security_rank());
    }
}
