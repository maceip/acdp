//! Process-based runtime

use crate::types::{ExecutionRequest, ExecutionResult, ExecutionStream};
use crate::Result;
use async_trait::async_trait;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;
use tokio::sync::{mpsc, oneshot};

/// Process-based runtime - executes code as shell commands
pub struct ProcessRuntime {
    shell: String,
}

impl ProcessRuntime {
    pub fn new() -> Self {
        Self {
            shell: std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_string()),
        }
    }

    pub fn with_shell(shell: impl Into<String>) -> Self {
        Self {
            shell: shell.into(),
        }
    }
}

impl Default for ProcessRuntime {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl super::Runtime for ProcessRuntime {
    async fn execute(&self, request: ExecutionRequest) -> Result<ExecutionStream> {
        let (stdout_tx, stdout_rx) = mpsc::channel(128);
        let (stderr_tx, stderr_rx) = mpsc::channel(128);
        let (result_tx, result_rx) = oneshot::channel();

        let shell = self.shell.clone();
        let start = std::time::Instant::now();

        tokio::spawn(async move {
            let mut error_msg = None;
            let mut timed_out = false;

            // Spawn process
            let mut child = match Command::new(&shell)
                .arg("-c")
                .arg(&request.code)
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                .stdin(std::process::Stdio::null())
                .spawn()
            {
                Ok(child) => child,
                Err(e) => {
                    let _ = result_tx.send(ExecutionResult {
                        exit_code: 1,
                        duration_ms: start.elapsed().as_millis() as u64,
                        timed_out: false,
                        error: Some(format!("Failed to spawn process: {}", e)),
                    });
                    return;
                }
            };

            // Stream stdout
            if let Some(stdout) = child.stdout.take() {
                let mut reader = BufReader::new(stdout).lines();
                tokio::spawn(async move {
                    while let Ok(Some(line)) = reader.next_line().await {
                        let mut line_bytes = line.into_bytes();
                        line_bytes.push(b'\n');
                        if stdout_tx.send(line_bytes).await.is_err() {
                            break;
                        }
                    }
                });
            }

            // Stream stderr
            if let Some(stderr) = child.stderr.take() {
                let mut reader = BufReader::new(stderr).lines();
                tokio::spawn(async move {
                    while let Ok(Some(line)) = reader.next_line().await {
                        let mut line_bytes = line.into_bytes();
                        line_bytes.push(b'\n');
                        if stderr_tx.send(line_bytes).await.is_err() {
                            break;
                        }
                    }
                });
            }

            // Wait for completion with optional timeout
            let wait_result = if let Some(timeout_secs) = request.timeout_secs {
                match tokio::time::timeout(
                    std::time::Duration::from_secs(timeout_secs),
                    child.wait(),
                )
                .await
                {
                    Ok(Ok(status)) => Ok(status),
                    Ok(Err(e)) => Err(e),
                    Err(_) => {
                        // Timeout - kill the process
                        let _ = child.kill().await;
                        timed_out = true;
                        Ok(std::process::ExitStatus::default())
                    }
                }
            } else {
                child.wait().await
            };

            let exit_code = match wait_result {
                Ok(status) => status.code().unwrap_or(1),
                Err(e) => {
                    error_msg = Some(format!("Process wait error: {}", e));
                    1
                }
            };

            let duration_ms = start.elapsed().as_millis() as u64;

            let _ = result_tx.send(ExecutionResult {
                exit_code,
                duration_ms,
                timed_out,
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
        "process"
    }
}
