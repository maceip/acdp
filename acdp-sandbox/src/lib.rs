//! MCP Sandbox - Secure code execution service
//!
//! Provides a simple, extensible interface for executing code in isolated environments.
//! Supports multiple runtime backends (WASM, processes, remote execution).

mod execution;
mod gencode;
mod limits;
mod policy;
mod runtime;
mod selector;
mod service;
mod types;

pub use execution::{
    ExecutionHandle, ExecutionId, ExecutionState, ExecutionStatus, MonitoredExecution,
};
pub use gencode::{CodeGenerator, LlmCodeGenerator, NullCodeGenerator};
pub use limits::ResourceLimits;
pub use policy::{
    Language, RuntimeRequirement, RuntimeType, SecurityPolicy, ToolPolicy, TrustLevel,
};
#[cfg(feature = "v8-worker")]
pub use runtime::v8::worker;
pub use runtime::{ProcessRuntime, Runtime, WasmRuntime};
#[cfg(feature = "v8")]
pub use runtime::{SnapshotBuilder, SnapshotConfig, SnapshotManager, V8Runtime};
pub use selector::{RuntimeDecision, RuntimeSelector, SelectionError, ToolDefinition};
pub use service::SandboxService;
pub use types::{ExecutionRequest, ExecutionResult, ExecutionStream};

/// Re-export common error types
pub type Result<T> = anyhow::Result<T>;
