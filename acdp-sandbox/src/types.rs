//! Core types for sandbox execution

use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;

/// Request to execute code
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionRequest {
    /// The code to execute
    pub code: String,

    /// Optional timeout in seconds (None = runs indefinitely)
    pub timeout_secs: Option<u64>,

    /// Optional environment variables
    #[serde(default)]
    pub env: Vec<(String, String)>,

    /// Optional stdin input
    pub stdin: Option<String>,
}

impl ExecutionRequest {
    /// Create a simple execution request
    pub fn new(code: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            timeout_secs: None,
            env: Vec::new(),
            stdin: None,
        }
    }

    /// Set timeout in seconds
    pub fn with_timeout(mut self, secs: u64) -> Self {
        self.timeout_secs = Some(secs);
        self
    }

    /// Add environment variable
    pub fn with_env(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.env.push((key.into(), value.into()));
        self
    }
}

/// Streaming execution output
pub struct ExecutionStream {
    /// Stdout stream
    pub stdout: mpsc::Receiver<Vec<u8>>,

    /// Stderr stream
    pub stderr: mpsc::Receiver<Vec<u8>>,

    /// Final result when execution completes
    pub result: tokio::sync::oneshot::Receiver<ExecutionResult>,
}

/// Result of code execution
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionResult {
    /// Exit code (0 = success)
    pub exit_code: i32,

    /// Execution duration in milliseconds
    pub duration_ms: u64,

    /// Whether execution timed out
    pub timed_out: bool,

    /// Optional error message
    pub error: Option<String>,
}

impl ExecutionResult {
    /// Check if execution succeeded
    pub fn success(&self) -> bool {
        self.exit_code == 0 && !self.timed_out && self.error.is_none()
    }
}
