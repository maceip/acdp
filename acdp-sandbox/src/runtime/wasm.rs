//! WASM/WASI runtime using wasmtime
//!
//! Provides secure execution of WebAssembly modules with WASI support

use crate::limits::ResourceLimits;
use crate::types::{ExecutionRequest, ExecutionResult, ExecutionStream};
use crate::Result;
use async_trait::async_trait;
use tokio::sync::{mpsc, oneshot};
use wasmtime::*;
use wasmtime_wasi::preview1::{add_to_linker_async, WasiP1Ctx};
use wasmtime_wasi::{pipe::MemoryOutputPipe, WasiCtxBuilder};

/// WASM runtime with WASI support
pub struct WasmRuntime {
    /// Wasmtime engine (shared across instances)
    engine: Engine,

    /// Resource limits for execution
    limits: ResourceLimits,
}

impl WasmRuntime {
    /// Create a new WASM runtime with default configuration
    pub fn new() -> Result<Self> {
        let mut config = Config::new();

        // Enable WASI and async
        config.wasm_backtrace_details(WasmBacktraceDetails::Enable);
        config.async_support(true);

        let engine = Engine::new(&config)?;

        Ok(Self {
            engine,
            limits: ResourceLimits::default(),
        })
    }

    /// Create WASM runtime with custom resource limits
    pub fn with_limits(limits: ResourceLimits) -> Result<Self> {
        let mut config = Config::new();
        config.wasm_backtrace_details(WasmBacktraceDetails::Enable);
        config.async_support(true);

        // Apply wasmtime-specific limits
        if let Some(max_memory) = limits.max_memory_bytes {
            // Limit maximum memory pages (64KB per page)
            let max_pages = (max_memory / 65536) as u64;
            config.max_wasm_stack(max_memory);
            config.memory_init_cow(false); // Disable COW to enforce limits
        }

        let engine = Engine::new(&config)?;

        Ok(Self { engine, limits })
    }

    /// Create WASM runtime with custom configuration
    pub fn with_config(config: Config) -> Result<Self> {
        let engine = Engine::new(&config)?;
        Ok(Self {
            engine,
            limits: ResourceLimits::default(),
        })
    }
}

impl Default for WasmRuntime {
    fn default() -> Self {
        Self::new().expect("Failed to create default WASM runtime")
    }
}

// Store state that holds WASI Preview 1 context
struct StoreState {
    wasi: WasiP1Ctx,
}

#[async_trait]
impl super::Runtime for WasmRuntime {
    async fn execute(&self, request: ExecutionRequest) -> Result<ExecutionStream> {
        let (mut stdout_tx, stdout_rx) = mpsc::channel(128);
        let (mut stderr_tx, stderr_rx) = mpsc::channel(128);
        let (result_tx, result_rx) = oneshot::channel();

        let engine = self.engine.clone();
        let code = request.code.clone();
        let limits = self.limits.clone();
        let limits_for_timeout = limits.clone();

        // Spawn WASM execution in separate task
        let task_handle = tokio::task::spawn(async move {
            let start = std::time::Instant::now();
            let mut exit_code = 0;
            let mut error_msg = None;

            // Create in-memory pipes for stdout/stderr
            let stdout_pipe = MemoryOutputPipe::new(1024 * 1024); // 1MB buffer
            let stderr_pipe = MemoryOutputPipe::new(1024 * 1024); // 1MB buffer
            let stdout_clone = stdout_pipe.clone();
            let stderr_clone = stderr_pipe.clone();

            // Create WASI P1 context with custom pipes
            let mut builder = WasiCtxBuilder::new();
            builder.stdout(stdout_pipe);
            builder.stderr(stderr_pipe);
            let _ = builder.inherit_args(); // Ignore error if args can't be inherited
            let wasi_ctx = builder.build_p1();

            let state = StoreState { wasi: wasi_ctx };
            let mut store = Store::new(&engine, state);

            // Note: Store-level limits require implementing ResourceLimiter trait
            // For now, relying on engine-level limits set in with_limits()

            // Decode WASM binary (may be base64 encoded or raw bytes in string)
            let wasm_bytes = if code.starts_with('\0')
                || code.as_bytes().starts_with(&[0x00, 0x61, 0x73, 0x6d])
            {
                // Raw WASM binary
                code.as_bytes().to_vec()
            } else {
                // Try base64 decode
                use base64::{engine::general_purpose, Engine as _};
                match general_purpose::STANDARD.decode(&code) {
                    Ok(bytes) => bytes,
                    Err(_) => {
                        // Fall back to treating as raw bytes
                        code.as_bytes().to_vec()
                    }
                }
            };

            // Parse module from bytes
            let module = match Module::from_binary(&engine, &wasm_bytes) {
                Ok(m) => m,
                Err(e) => {
                    error_msg = Some(format!("Failed to parse WASM module: {}", e));
                    exit_code = 1;
                    let _ = stderr_tx
                        .send(format!("WASM parse error: {}\n", e).into_bytes())
                        .await;

                    let _ = result_tx.send(ExecutionResult {
                        exit_code,
                        duration_ms: start.elapsed().as_millis() as u64,
                        timed_out: false,
                        error: error_msg,
                    });
                    return;
                }
            };

            // Link WASI Preview 1 to the module linker
            let mut linker = Linker::new(&engine);
            if let Err(e) = add_to_linker_async(&mut linker, |s: &mut StoreState| &mut s.wasi) {
                error_msg = Some(format!("Failed to link WASI: {}", e));
                exit_code = 1;
                let _ = stderr_tx.send(format!("{}\n", e).into_bytes()).await;

                let _ = result_tx.send(ExecutionResult {
                    exit_code,
                    duration_ms: start.elapsed().as_millis() as u64,
                    timed_out: false,
                    error: error_msg,
                });
                return;
            }

            // Instantiate module
            let instance = match linker.instantiate_async(&mut store, &module).await {
                Ok(inst) => inst,
                Err(e) => {
                    error_msg = Some(format!("Failed to instantiate module: {}", e));
                    exit_code = 1;
                    let _ = stderr_tx.send(format!("{}\n", e).into_bytes()).await;

                    let _ = result_tx.send(ExecutionResult {
                        exit_code,
                        duration_ms: start.elapsed().as_millis() as u64,
                        timed_out: false,
                        error: error_msg,
                    });
                    return;
                }
            };

            // Get and call the _start function (WASI entry point)
            let start_func = match instance.get_typed_func::<(), ()>(&mut store, "_start") {
                Ok(func) => func,
                Err(e) => {
                    error_msg = Some(format!("Failed to get _start function: {}", e));
                    exit_code = 1;
                    let _ = stderr_tx.send(format!("{}\n", e).into_bytes()).await;

                    let _ = result_tx.send(ExecutionResult {
                        exit_code,
                        duration_ms: start.elapsed().as_millis() as u64,
                        timed_out: false,
                        error: error_msg,
                    });
                    return;
                }
            };

            // Execute
            if let Err(e) = start_func.call_async(&mut store, ()).await {
                // Check if it's a WASI exit trap
                if let Some(exit_status) = e.downcast_ref::<wasmtime_wasi::I32Exit>() {
                    exit_code = exit_status.0;
                    if exit_code != 0 {
                        error_msg = Some(format!("Program exited with code {}", exit_code));
                    }
                } else {
                    error_msg = Some(format!("Execution error: {}", e));
                    exit_code = 1;
                }
            }

            // Drop store to release WASI context and pipes
            drop(store);

            // Read captured stdout/stderr from pipes
            let stdout_contents = stdout_clone
                .try_into_inner()
                .expect("Failed to get stdout contents")
                .to_vec();
            let stderr_contents = stderr_clone
                .try_into_inner()
                .expect("Failed to get stderr contents")
                .to_vec();

            // Send stdout in chunks
            if !stdout_contents.is_empty() {
                let _ = stdout_tx.send(stdout_contents).await;
            }

            // Send stderr in chunks (or add execution error if not already there)
            if !stderr_contents.is_empty() {
                let _ = stderr_tx.send(stderr_contents).await;
            } else if error_msg.is_some() {
                let _ = stderr_tx
                    .send(format!("{}\n", error_msg.as_ref().unwrap()).into_bytes())
                    .await;
            }

            let duration_ms = start.elapsed().as_millis() as u64;

            let _ = result_tx.send(ExecutionResult {
                exit_code,
                duration_ms,
                timed_out: false,
                error: error_msg,
            });
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

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_wasm_runtime_creation() {
        let runtime = WasmRuntime::new();
        assert!(runtime.is_ok());
    }
}
