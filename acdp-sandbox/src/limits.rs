//! Resource limits configuration for sandboxed execution

use serde::{Deserialize, Serialize};
use std::time::Duration;

/// Resource limits for code execution
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceLimits {
    /// Maximum execution time before timeout
    pub max_duration: Option<Duration>,

    /// Maximum memory usage in bytes (for runtimes that support it)
    pub max_memory_bytes: Option<usize>,

    /// Maximum CPU time in milliseconds (wall-clock time)
    pub max_cpu_time_ms: Option<u64>,
}

impl Default for ResourceLimits {
    fn default() -> Self {
        Self {
            max_duration: Some(Duration::from_secs(30)), // 30 seconds default
            max_memory_bytes: Some(100 * 1024 * 1024),   // 100 MB default
            max_cpu_time_ms: Some(30_000),               // 30 seconds CPU time
        }
    }
}

impl ResourceLimits {
    /// Create unlimited resource configuration (dangerous!)
    pub fn unlimited() -> Self {
        Self {
            max_duration: None,
            max_memory_bytes: None,
            max_cpu_time_ms: None,
        }
    }

    /// Create strict limits for untrusted code
    pub fn strict() -> Self {
        Self {
            max_duration: Some(Duration::from_secs(5)), // 5 seconds
            max_memory_bytes: Some(10 * 1024 * 1024),   // 10 MB
            max_cpu_time_ms: Some(5_000),               // 5 seconds CPU
        }
    }

    /// Create permissive limits for trusted code
    pub fn permissive() -> Self {
        Self {
            max_duration: Some(Duration::from_secs(300)), // 5 minutes
            max_memory_bytes: Some(500 * 1024 * 1024),    // 500 MB
            max_cpu_time_ms: Some(300_000),               // 5 minutes CPU
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_limits() {
        let limits = ResourceLimits::default();
        assert_eq!(limits.max_duration, Some(Duration::from_secs(30)));
        assert_eq!(limits.max_memory_bytes, Some(100 * 1024 * 1024));
    }

    #[test]
    fn test_unlimited() {
        let limits = ResourceLimits::unlimited();
        assert!(limits.max_duration.is_none());
        assert!(limits.max_memory_bytes.is_none());
    }

    #[test]
    fn test_strict_limits() {
        let limits = ResourceLimits::strict();
        assert_eq!(limits.max_duration, Some(Duration::from_secs(5)));
        assert_eq!(limits.max_memory_bytes, Some(10 * 1024 * 1024));
    }
}
