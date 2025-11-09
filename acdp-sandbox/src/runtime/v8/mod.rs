//! V8 JavaScript runtime using deno_core

mod snapshot;
#[cfg(feature = "v8-worker")]
pub mod worker;

use crate::limits::ResourceLimits;
use crate::types::{ExecutionRequest, ExecutionResult, ExecutionStream};
use crate::Result;
use async_trait::async_trait;
use tokio::sync::{mpsc, oneshot};

pub use snapshot::{SnapshotBuilder, SnapshotConfig, SnapshotManager};

/// V8 JavaScript runtime
pub struct V8Runtime {
    /// Optional snapshot for fast startup
    /// Using Box to own the snapshot data with 'static lifetime
    snapshot: Option<&'static [u8]>,

    /// Resource limits for execution
    limits: ResourceLimits,
}

impl V8Runtime {
    /// Create a new V8 runtime without snapshot
    pub fn new() -> Self {
        Self {
            snapshot: None,
            limits: ResourceLimits::default(),
        }
    }

    /// Create V8 runtime with custom resource limits
    pub fn with_limits(limits: ResourceLimits) -> Self {
        Self {
            snapshot: None,
            limits,
        }
    }

    /// Create V8 runtime with snapshot support
    /// The snapshot vec is leaked to get 'static lifetime
    pub fn with_snapshot(snapshot: Vec<u8>) -> Self {
        let snapshot_static: &'static [u8] = Box::leak(snapshot.into_boxed_slice());
        Self {
            snapshot: Some(snapshot_static),
            limits: ResourceLimits::default(),
        }
    }

    /// Create V8 runtime with both snapshot and custom limits
    pub fn with_snapshot_and_limits(snapshot: Vec<u8>, limits: ResourceLimits) -> Self {
        let snapshot_static: &'static [u8] = Box::leak(snapshot.into_boxed_slice());
        Self {
            snapshot: Some(snapshot_static),
            limits,
        }
    }

    /// Create V8 runtime from snapshot config
    pub fn from_snapshot_config(config: SnapshotConfig) -> Result<Self> {
        let manager = SnapshotManager::new(config);
        let snapshot = manager.get_or_create()?.map(|data| {
            let snapshot_static: &'static [u8] = Box::leak(data.into_boxed_slice());
            snapshot_static
        });

        Ok(Self {
            snapshot,
            limits: ResourceLimits::default(),
        })
    }

    /// Builder for snapshot configuration
    pub fn snapshot_builder(path: impl Into<std::path::PathBuf>) -> SnapshotBuilder {
        SnapshotBuilder::new(path)
    }
}

impl Default for V8Runtime {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl super::Runtime for V8Runtime {
    async fn execute(&self, request: ExecutionRequest) -> Result<ExecutionStream> {
        use deno_core::JsRuntime;
        use deno_core::RuntimeOptions;

        let (mut stdout_tx, stdout_rx) = mpsc::channel(128);
        let (mut stderr_tx, stderr_rx) = mpsc::channel(128);
        let (result_tx, result_rx) = oneshot::channel();

        let code = request.code.clone();
        let start = std::time::Instant::now();
        let snapshot = self.snapshot.clone();
        let limits = self.limits.clone();

        // Spawn V8 execution in separate task with timeout
        let task_handle = tokio::task::spawn_blocking(move || {
            let mut exit_code = 0;
            let mut error_msg = None;

            // Create V8 isolate with optional snapshot and memory limits
            let mut runtime_options = if let Some(snapshot_data) = snapshot {
                RuntimeOptions {
                    startup_snapshot: Some(snapshot_data.into()),
                    ..Default::default()
                }
            } else {
                RuntimeOptions::default()
            };

            // Set heap limits if specified
            if let Some(max_bytes) = limits.max_memory_bytes {
                // V8 heap limits are set in bytes (initial, max)
                // Set initial to 10MB or 10% of max, whichever is smaller
                let initial_bytes = (max_bytes / 10).min(10 * 1024 * 1024);
                runtime_options.create_params = Some(
                    deno_core::v8::CreateParams::default().heap_limits(initial_bytes, max_bytes),
                );
            }

            let mut runtime = JsRuntime::new(runtime_options);

            // Execute JavaScript
            match runtime.execute_script("<sandbox>", code) {
                Ok(global) => {
                    // Try to get the result value
                    let scope = &mut runtime.handle_scope();
                    let local = deno_core::v8::Local::new(scope, global);
                    let result_str = local.to_rust_string_lossy(scope);

                    if result_str != "undefined" {
                        let output = format!("{}\n", result_str);
                        let _ = stdout_tx.blocking_send(output.into_bytes());
                    }
                }
                Err(e) => {
                    error_msg = Some(format!("Execution error: {}", e));
                    exit_code = 1;
                    let _ = stderr_tx.blocking_send(format!("{}\n", e).into_bytes());
                }
            }

            let duration_ms = start.elapsed().as_millis() as u64;

            ExecutionResult {
                exit_code,
                duration_ms,
                timed_out: false,
                error: error_msg,
            }
        });

        // Wrap with timeout if specified
        tokio::spawn(async move {
            let result = if let Some(max_duration) = limits.max_duration {
                match tokio::time::timeout(max_duration, task_handle).await {
                    Ok(Ok(exec_result)) => exec_result,
                    Ok(Err(join_err)) => ExecutionResult {
                        exit_code: 1,
                        duration_ms: start.elapsed().as_millis() as u64,
                        timed_out: false,
                        error: Some(format!("Task panicked: {}", join_err)),
                    },
                    Err(_timeout) => ExecutionResult {
                        exit_code: 124,
                        duration_ms: start.elapsed().as_millis() as u64,
                        timed_out: true,
                        error: Some(format!("Execution timed out after {:?}", max_duration)),
                    },
                }
            } else {
                match task_handle.await {
                    Ok(exec_result) => exec_result,
                    Err(join_err) => ExecutionResult {
                        exit_code: 1,
                        duration_ms: start.elapsed().as_millis() as u64,
                        timed_out: false,
                        error: Some(format!("Task panicked: {}", join_err)),
                    },
                }
            };

            let _ = result_tx.send(result);
        });

        Ok(ExecutionStream {
            stdout: stdout_rx,
            stderr: stderr_rx,
            result: result_rx,
        })
    }

    fn name(&self) -> &str {
        "v8"
    }
}
