use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;

/// High-level description of a code-generation task.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct CodeGenerationSpec {
    /// Free-form description of the intent (e.g. "summarise CSV data").
    pub description: String,
    /// Preferred target language or runtime (e.g. "python", "wasm", "bash").
    pub target_language: Option<String>,
    /// Optional structured hints (tool schemas, column names, etc.).
    #[serde(default)]
    pub context: HashMap<String, serde_json::Value>,
}

/// Security / resource policy for an execution plan.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ExecutionPolicy {
    /// Maximum wall-clock runtime in seconds (`None` = unlimited).
    pub timeout_secs: Option<u64>,
    /// CPU quota in seconds (`None` = unlimited).
    pub max_cpu_seconds: Option<f64>,
    /// Memory cap in bytes (`None` = unlimited).
    pub max_memory_bytes: Option<u64>,
    /// Expected heartbeat cadence in seconds (`None` = sandbox default).
    pub heartbeat_interval_secs: Option<u64>,
    /// Allowed missed heartbeats before the execution is considered stale.
    pub decay_threshold: Option<u32>,
    /// Files or directories to mount from the host into the sandbox.
    #[serde(default)]
    pub mounts: Vec<MountSpec>,
    /// Network access policy.
    pub network: NetworkPolicy,
    /// Optional caller identity (JWT).
    pub identity_jwt: Option<String>,
    /// Capability whitelist used for deterministic plan execution.
    #[serde(default)]
    pub capabilities: Vec<CapabilityToken>,
}

impl Default for ExecutionPolicy {
    fn default() -> Self {
        Self {
            timeout_secs: Some(30),
            max_cpu_seconds: None,
            max_memory_bytes: None,
            heartbeat_interval_secs: Some(5),
            decay_threshold: Some(3),
            mounts: Vec::new(),
            network: NetworkPolicy::Disabled,
            identity_jwt: None,
            capabilities: Vec::new(),
        }
    }
}

/// Host path that may be exposed to the sandbox.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MountSpec {
    /// Absolute path on the trusted host.
    pub host_path: String,
    /// Mount point inside the sandbox.
    pub sandbox_path: String,
    /// Access mode.
    pub mode: MountMode,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum MountMode {
    ReadOnly,
    ReadWrite,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum NetworkPolicy {
    Disabled,
    Limited,
    Full,
}

impl Default for NetworkPolicy {
    fn default() -> Self {
        NetworkPolicy::Disabled
    }
}

/// Request sent to the code-generation layer.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CodeGenerationRequest {
    pub request_id: Uuid,
    pub spec: CodeGenerationSpec,
    pub policy: ExecutionPolicy,
    /// Arbitrary metadata for logging / correlation.
    #[serde(default)]
    pub metadata: HashMap<String, String>,
}

impl CodeGenerationRequest {
    pub fn new(spec: CodeGenerationSpec, policy: ExecutionPolicy) -> Self {
        Self {
            request_id: Uuid::new_v4(),
            spec,
            policy,
            metadata: HashMap::new(),
        }
    }
}

/// Result of a code-generation request.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CodeGenerationResponse {
    pub plan: ExecutionPlan,
    #[serde(default)]
    pub warnings: Vec<String>,
    #[serde(default)]
    pub diagnostics: Vec<String>,
}

impl CodeGenerationResponse {
    pub fn new(plan: ExecutionPlan) -> Self {
        Self {
            plan,
            warnings: Vec::new(),
            diagnostics: Vec::new(),
        }
    }

    pub fn empty() -> Self {
        Self::new(ExecutionPlan::empty())
    }
}

/// Execution-ready artefact produced by the code-generation service.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ExecutionPlan {
    pub plan_id: Uuid,
    pub payload: ExecutionPayload,
    pub graph: PlanGraph,
    pub policy: ExecutionPolicy,
    #[serde(default)]
    pub metadata: HashMap<String, String>,
    pub created_at: DateTime<Utc>,
}

impl ExecutionPlan {
    pub fn empty() -> Self {
        Self {
            plan_id: Uuid::new_v4(),
            payload: ExecutionPayload::Empty,
            graph: PlanGraph::default(),
            policy: ExecutionPolicy::default(),
            metadata: HashMap::new(),
            created_at: Utc::now(),
        }
    }
}

/// Structured execution graph used for deterministic interpretation.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct PlanGraph {
    #[serde(default)]
    pub nodes: Vec<PlanNode>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PlanNode {
    pub id: String,
    pub kind: PlanNodeKind,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum PlanNodeKind {
    RunPython {
        code: String,
        capability: CapabilityId,
    },
    ReadFile {
        path: String,
        output_capability: CapabilityId,
    },
    WriteFile {
        path: String,
        input_capability: CapabilityId,
    },
    Emit {
        input_capability: CapabilityId,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct CapabilityId(pub String);

impl From<&str> for CapabilityId {
    fn from(value: &str) -> Self {
        Self(value.to_string())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CapabilityToken {
    pub id: CapabilityId,
    pub kind: CapabilityKind,
    pub origin: CapabilityOrigin,
    #[serde(default)]
    pub allowed_sinks: Vec<CapabilitySink>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum CapabilityKind {
    SandboxExecution,
    FileRead,
    FileWrite,
    Network,
    CodeFragment,
    Data,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum CapabilityOrigin {
    Trusted,
    UserProvided,
    External,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum CapabilitySink {
    SandboxExecution,
    FileRead,
    FileWrite,
    NetworkRequest,
    Emit,
}

/// Payload that the sandbox can execute.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ExecutionPayload {
    /// Single script executable by an existing runtime.
    Script { language: String, code: String },
    /// Container image + command specification.
    Container {
        image: String,
        command: Vec<String>,
        env: HashMap<String, String>,
    },
    /// Multi-step workflow (each step references another payload).
    Workflow { steps: Vec<ExecutionStep> },
    /// No-op placeholder (used by stub generators).
    Empty,
}

/// Step in a composite execution plan.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ExecutionStep {
    pub name: String,
    pub payload: Box<ExecutionPayload>,
    #[serde(default)]
    pub metadata: HashMap<String, String>,
}
