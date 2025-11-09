//! Execution handle with reconnection support

use crate::types::{ExecutionRequest, ExecutionResult, ExecutionStream};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::{mpsc, oneshot, RwLock};

/// Unique execution identifier
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ExecutionId(pub uuid::Uuid);

impl ExecutionId {
    pub fn new() -> Self {
        Self(uuid::Uuid::new_v4())
    }
}

impl Default for ExecutionId {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for ExecutionId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Execution state that persists across reconnections
#[derive(Debug, Clone)]
pub struct ExecutionState {
    pub id: ExecutionId,
    pub request: ExecutionRequest,
    pub status: ExecutionStatus,
    pub result: Option<ExecutionResult>,
    /// Buffered output (limited size)
    pub stdout_buffer: Vec<Vec<u8>>,
    pub stderr_buffer: Vec<Vec<u8>>,
}

/// Execution status
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ExecutionStatus {
    Queued,
    Running,
    Completed,
    Failed,
    Cancelled,
}

impl ExecutionState {
    pub fn new(id: ExecutionId, request: ExecutionRequest) -> Self {
        Self {
            id,
            request,
            status: ExecutionStatus::Queued,
            result: None,
            stdout_buffer: Vec::new(),
            stderr_buffer: Vec::new(),
        }
    }

    /// Add output line with buffer limit
    pub fn push_stdout(&mut self, line: Vec<u8>, max_lines: usize) {
        if self.stdout_buffer.len() >= max_lines {
            self.stdout_buffer.remove(0);
        }
        self.stdout_buffer.push(line);
    }

    pub fn push_stderr(&mut self, line: Vec<u8>, max_lines: usize) {
        if self.stderr_buffer.len() >= max_lines {
            self.stderr_buffer.remove(0);
        }
        self.stderr_buffer.push(line);
    }
}

/// Reconnectable execution handle
pub struct ExecutionHandle {
    pub id: ExecutionId,
    state: Arc<RwLock<ExecutionState>>,
    stream: ExecutionStream,
}

impl ExecutionHandle {
    /// Create a new execution handle
    pub fn new(id: ExecutionId, request: ExecutionRequest, stream: ExecutionStream) -> Self {
        let state = Arc::new(RwLock::new(ExecutionState::new(id, request)));
        Self { id, state, stream }
    }

    /// Get current execution state
    pub async fn state(&self) -> ExecutionState {
        self.state.read().await.clone()
    }

    /// Reconnect to this execution (get new stream with buffered output)
    pub async fn reconnect(&self) -> crate::Result<ExecutionStream> {
        let state = self.state.read().await;

        let (stdout_tx, stdout_rx) = mpsc::channel(128);
        let (stderr_tx, stderr_rx) = mpsc::channel(128);

        // Send buffered output first
        for line in &state.stdout_buffer {
            let _ = stdout_tx.send(line.clone()).await;
        }
        for line in &state.stderr_buffer {
            let _ = stderr_tx.send(line.clone()).await;
        }

        // If completed, send result immediately
        let (result_tx, result_rx) = oneshot::channel();
        if let Some(result) = &state.result {
            let _ = result_tx.send(result.clone());
        } else {
            // Otherwise, need to subscribe to completion
            // For now, return empty stream (would need a broadcast channel)
            drop(result_tx);
        }

        Ok(ExecutionStream {
            stdout: stdout_rx,
            stderr: stderr_rx,
            result: result_rx,
        })
    }

    /// Get the execution ID
    pub fn id(&self) -> ExecutionId {
        self.id
    }

    /// Check if execution is complete
    pub async fn is_complete(&self) -> bool {
        matches!(
            self.state.read().await.status,
            ExecutionStatus::Completed | ExecutionStatus::Failed | ExecutionStatus::Cancelled
        )
    }

    /// Cancel the execution
    pub async fn cancel(&self) {
        let mut state = self.state.write().await;
        state.status = ExecutionStatus::Cancelled;
    }

    /// Take the stream (consumes handle)
    pub fn into_stream(self) -> ExecutionStream {
        self.stream
    }

    /// Start monitoring execution (updates state as output arrives)
    pub fn monitor(self) -> MonitoredExecution {
        MonitoredExecution::new(self)
    }
}

/// Monitored execution that updates state in background
pub struct MonitoredExecution {
    id: ExecutionId,
    state: Arc<RwLock<ExecutionState>>,
}

impl MonitoredExecution {
    fn new(handle: ExecutionHandle) -> Self {
        let id = handle.id;
        let state_arc = handle.state.clone();
        let ExecutionStream {
            mut stdout,
            mut stderr,
            result,
        } = handle.stream;

        // Spawn background task to update state
        let state_for_task = state_arc.clone();
        tokio::spawn(async move {
            // Update status to running
            state_for_task.write().await.status = ExecutionStatus::Running;

            // Monitor stdout
            let state_clone = state_for_task.clone();
            tokio::spawn(async move {
                while let Some(line) = stdout.recv().await {
                    state_clone.write().await.push_stdout(line, 1000);
                }
            });

            // Monitor stderr
            let state_clone = state_for_task.clone();
            tokio::spawn(async move {
                while let Some(line) = stderr.recv().await {
                    state_clone.write().await.push_stderr(line, 1000);
                }
            });

            // Wait for result
            if let Ok(result) = result.await {
                let mut state = state_for_task.write().await;
                state.status = if result.success() {
                    ExecutionStatus::Completed
                } else {
                    ExecutionStatus::Failed
                };
                state.result = Some(result);
            }
        });

        Self {
            id,
            state: state_arc,
        }
    }

    /// Get the execution ID
    pub fn id(&self) -> ExecutionId {
        self.id
    }

    /// Get current state
    pub async fn state(&self) -> ExecutionState {
        self.state.read().await.clone()
    }

    /// Wait for completion
    pub async fn wait(&self) -> crate::Result<ExecutionResult> {
        loop {
            let state = self.state.read().await;
            if matches!(
                state.status,
                ExecutionStatus::Completed | ExecutionStatus::Failed | ExecutionStatus::Cancelled
            ) {
                if let Some(result) = state.result.clone() {
                    return Ok(result);
                }
            }
            drop(state);
            tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
        }
    }

    /// Reconnect with fresh stream (returns buffered output)
    pub async fn reconnect(&self) -> crate::Result<ExecutionStream> {
        let state = self.state.read().await;

        let (stdout_tx, stdout_rx) = mpsc::channel(128);
        let (stderr_tx, stderr_rx) = mpsc::channel(128);

        // Send buffered output
        for line in &state.stdout_buffer {
            let _ = stdout_tx.send(line.clone()).await;
        }
        for line in &state.stderr_buffer {
            let _ = stderr_tx.send(line.clone()).await;
        }

        let (result_tx, result_rx) = oneshot::channel();
        if let Some(result) = &state.result {
            let _ = result_tx.send(result.clone());
        }

        Ok(ExecutionStream {
            stdout: stdout_rx,
            stderr: stderr_rx,
            result: result_rx,
        })
    }
}
