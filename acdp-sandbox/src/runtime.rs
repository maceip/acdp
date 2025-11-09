//! Runtime trait and implementations

mod process;
#[cfg(feature = "v8")]
pub mod v8;
#[cfg(feature = "wasm")]
mod wasm;

use crate::types::{ExecutionRequest, ExecutionStream};
use crate::Result;
use async_trait::async_trait;

pub use process::ProcessRuntime;
#[cfg(feature = "v8")]
pub use v8::{SnapshotBuilder, SnapshotConfig, SnapshotManager, V8Runtime};
#[cfg(feature = "wasm")]
pub use wasm::WasmRuntime as WasmRuntimeImpl;

/// Runtime abstraction for executing code
#[async_trait]
pub trait Runtime: Send + Sync {
    /// Execute code and return streaming output
    async fn execute(&self, request: ExecutionRequest) -> Result<ExecutionStream>;

    /// Get runtime name
    fn name(&self) -> &str;
}

/// WASM/WASI runtime implementation
#[cfg(feature = "wasm")]
pub use WasmRuntimeImpl as WasmRuntime;

/// WASM runtime placeholder (when feature is disabled)
#[cfg(not(feature = "wasm"))]
pub struct WasmRuntime;

#[cfg(not(feature = "wasm"))]
impl WasmRuntime {
    pub fn new() -> Self {
        Self
    }
}

#[cfg(not(feature = "wasm"))]
impl Default for WasmRuntime {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(not(feature = "wasm"))]
#[async_trait]
impl Runtime for WasmRuntime {
    async fn execute(&self, _request: ExecutionRequest) -> Result<ExecutionStream> {
        use tokio::sync::{mpsc, oneshot};
        let (_stdout_tx, stdout_rx) = mpsc::channel(128);
        let (_stderr_tx, stderr_rx) = mpsc::channel(128);
        let (result_tx, result_rx) = oneshot::channel();

        // Send error result immediately
        let _ = result_tx.send(crate::types::ExecutionResult {
            exit_code: 1,
            duration_ms: 0,
            timed_out: false,
            error: Some("WASM runtime not enabled. Compile with --features wasm".to_string()),
        });

        Ok(ExecutionStream {
            stdout: stdout_rx,
            stderr: stderr_rx,
            result: result_rx,
        })
    }

    fn name(&self) -> &str {
        "wasm"
    }
}
