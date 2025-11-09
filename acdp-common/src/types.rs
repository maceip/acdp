use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;

use crate::{JsonRpcRequest, JsonRpcResponse};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct ProxyId(pub Uuid);

impl ProxyId {
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

impl Default for ProxyId {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum LogLevel {
    Debug,
    Info,
    Warning,
    Error,
    Request,
    Response,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogEntry {
    pub id: Uuid,
    pub timestamp: DateTime<Utc>,
    pub level: LogLevel,
    pub message: String,
    pub proxy_id: ProxyId,
    pub request_id: Option<String>,
    pub metadata: Option<serde_json::Value>,
}

impl LogEntry {
    pub fn new(level: LogLevel, message: String, proxy_id: ProxyId) -> Self {
        Self {
            id: Uuid::new_v4(),
            timestamp: Utc::now(),
            level,
            message,
            proxy_id,
            request_id: None,
            metadata: None,
        }
    }

    pub fn with_request_id(mut self, request_id: String) -> Self {
        self.request_id = Some(request_id);
        self
    }

    pub fn with_metadata(mut self, metadata: serde_json::Value) -> Self {
        self.metadata = Some(metadata);
        self
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProxyStats {
    pub proxy_id: ProxyId,
    pub total_requests: u64,
    pub successful_requests: u64,
    pub failed_requests: u64,
    pub active_connections: u32,
    pub uptime: std::time::Duration,
    pub bytes_transferred: u64,
}

impl Default for ProxyStats {
    fn default() -> Self {
        Self {
            proxy_id: ProxyId::new(),
            total_requests: 0,
            successful_requests: 0,
            failed_requests: 0,
            active_connections: 0,
            uptime: std::time::Duration::from_secs(0),
            bytes_transferred: 0,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProxyInfo {
    pub id: ProxyId,
    pub name: String,
    pub listen_address: String,
    pub target_command: Vec<String>,
    pub status: ProxyStatus,
    pub stats: ProxyStats,
    pub transport_type: TransportType,
    #[serde(default)]
    pub routing_mode: Option<String>,
    #[serde(default)]
    pub model_status: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ProxyStatus {
    Starting,
    Running,
    Stopped,
    Error(String),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum TransportType {
    Stdio,
    HttpSse,
    HttpStream,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MCPRequest {
    pub id: String,
    pub method: String,
    pub params: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MCPResponse {
    pub id: String,
    pub result: Option<serde_json::Value>,
    pub error: Option<MCPError>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MCPError {
    pub code: i32,
    pub message: String,
    pub data: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct ClientId(pub Uuid);

impl ClientId {
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

impl Default for ClientId {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct ServerId(pub Uuid);

impl ServerId {
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

impl Default for ServerId {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct SessionId(pub Uuid);

impl SessionId {
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

impl Default for SessionId {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct MessageId(pub Uuid);

impl MessageId {
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

impl Default for MessageId {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum ClientConnectionType {
    WebSocket,
    Stdio,
    Http,
    HttpSse,
    Custom(String),
}

impl Default for ClientConnectionType {
    fn default() -> Self {
        Self::Custom("unknown".to_string())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClientInfo {
    pub id: ClientId,
    pub name: String,
    pub connection_type: ClientConnectionType,
    pub connected_at: DateTime<Utc>,
    pub last_activity: DateTime<Utc>,
    pub active_sessions: Vec<SessionId>,
    pub total_requests: u64,
}

impl Default for ClientInfo {
    fn default() -> Self {
        let now = Utc::now();
        Self {
            id: ClientId::new(),
            name: "unknown-client".to_string(),
            connection_type: ClientConnectionType::default(),
            connected_at: now,
            last_activity: now,
            active_sessions: Vec::new(),
            total_requests: 0,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ServerEndpoint {
    Command { program: String, args: Vec<String> },
    Url(String),
    Socket(String),
    Custom(String),
}

impl Default for ServerEndpoint {
    fn default() -> Self {
        Self::Custom("unknown".to_string())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ServerStatus {
    Starting,
    Running,
    Degraded(String),
    Stopped,
    Error(String),
}

impl Default for ServerStatus {
    fn default() -> Self {
        Self::Starting
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ServerCapabilities {
    pub tools: Vec<String>,
    pub resources: Vec<String>,
    pub prompts: Vec<String>,
    pub features: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct HealthMetrics {
    pub uptime_seconds: u64,
    pub response_time_ms: f64,
    pub success_rate: f64,
    pub cpu_percent: f64,
    pub memory_bytes: u64,
    pub last_error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerInfo {
    pub id: ServerId,
    pub name: String,
    pub endpoint: ServerEndpoint,
    pub status: ServerStatus,
    pub capabilities: Option<ServerCapabilities>,
    pub active_sessions: Vec<SessionId>,
    pub health_metrics: HealthMetrics,
}

impl Default for ServerInfo {
    fn default() -> Self {
        Self {
            id: ServerId::new(),
            name: "unknown-server".to_string(),
            endpoint: ServerEndpoint::default(),
            status: ServerStatus::default(),
            capabilities: None,
            active_sessions: Vec::new(),
            health_metrics: HealthMetrics::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SessionStatus {
    Starting,
    Active,
    Idle,
    Completed,
    Failed(String),
}

impl Default for SessionStatus {
    fn default() -> Self {
        Self::Starting
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActiveRequest {
    pub request_id: String,
    pub method: String,
    pub started_at: DateTime<Utc>,
    pub duration_ms: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_server: Option<ServerId>,
}

impl Default for ActiveRequest {
    fn default() -> Self {
        Self {
            request_id: String::new(),
            method: String::new(),
            started_at: Utc::now(),
            duration_ms: 0,
            target_server: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TransformDirection {
    Request,
    Response,
}

impl Default for TransformDirection {
    fn default() -> Self {
        Self::Request
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppliedTransformation {
    pub rule_id: String,
    pub rule_name: String,
    pub direction: TransformDirection,
    pub duration_ms: u64,
    pub success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
}

impl Default for AppliedTransformation {
    fn default() -> Self {
        Self {
            rule_id: String::new(),
            rule_name: String::new(),
            direction: TransformDirection::default(),
            duration_ms: 0,
            success: true,
            notes: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProxySession {
    pub id: SessionId,
    pub client_id: ClientId,
    pub server_id: ServerId,
    pub started_at: DateTime<Utc>,
    pub last_activity: DateTime<Utc>,
    pub request_count: u64,
    pub bytes_in: u64,
    pub bytes_out: u64,
    pub status: SessionStatus,
    pub active_requests: Vec<ActiveRequest>,
    pub transformations_applied: Vec<AppliedTransformation>,
}

impl Default for ProxySession {
    fn default() -> Self {
        let now = Utc::now();
        Self {
            id: SessionId::new(),
            client_id: ClientId::new(),
            server_id: ServerId::new(),
            started_at: now,
            last_activity: now,
            request_count: 0,
            bytes_in: 0,
            bytes_out: 0,
            status: SessionStatus::default(),
            active_requests: Vec::new(),
            transformations_applied: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TransformationStats {
    pub applied: u64,
    pub blocked: u64,
    pub errors: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_applied: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransformationRule {
    pub id: String,
    pub name: String,
    pub description: String,
    pub enabled: bool,
    pub scope: String,
    pub stats: TransformationStats,
}

impl Default for TransformationRule {
    fn default() -> Self {
        Self {
            id: String::new(),
            name: String::new(),
            description: String::new(),
            enabled: true,
            scope: String::new(),
            stats: TransformationStats::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoutingRule {
    pub id: String,
    pub name: String,
    pub description: String,
    pub match_criteria: HashMap<String, String>,
    pub target_servers: Vec<ServerId>,
    pub enabled: bool,
}

impl Default for RoutingRule {
    fn default() -> Self {
        Self {
            id: String::new(),
            name: String::new(),
            description: String::new(),
            match_criteria: HashMap::new(),
            target_servers: Vec::new(),
            enabled: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoutingDecision {
    pub session_id: SessionId,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rule_id: Option<String>,
    pub target_server: ServerId,
    pub reason: String,
    pub timestamp: DateTime<Utc>,
}

impl Default for RoutingDecision {
    fn default() -> Self {
        Self {
            session_id: SessionId::new(),
            rule_id: None,
            target_server: ServerId::new(),
            reason: String::new(),
            timestamp: Utc::now(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum GatewayStatus {
    Starting,
    Running,
    Degraded,
    Stopped,
}

impl Default for GatewayStatus {
    fn default() -> Self {
        Self::Starting
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct GatewayMetrics {
    pub requests_per_minute: f64,
    pub bytes_in_per_sec: f64,
    pub bytes_out_per_sec: f64,
    pub active_sessions: usize,
    pub error_rate: f64,
    pub average_latency_ms: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GatewayState {
    pub status: GatewayStatus,
    pub uptime_seconds: u64,
    pub metrics: GatewayMetrics,
}

impl Default for GatewayState {
    fn default() -> Self {
        Self {
            status: GatewayStatus::default(),
            uptime_seconds: 0,
            metrics: GatewayMetrics::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct MessageTiming {
    pub received_at: DateTime<Utc>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub forwarded_at: Option<DateTime<Utc>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub responded_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MessageStatus {
    Pending,
    InFlight,
    Completed,
    Failed(String),
}

impl Default for MessageStatus {
    fn default() -> Self {
        Self::Pending
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageFlow {
    pub id: MessageId,
    pub session_id: SessionId,
    pub client_request: JsonRpcRequest,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub server_request: Option<JsonRpcRequest>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub server_response: Option<JsonRpcResponse>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_response: Option<JsonRpcResponse>,
    pub transformations: Vec<AppliedTransformation>,
    pub timing: MessageTiming,
    pub status: MessageStatus,
}

impl Default for MessageFlow {
    fn default() -> Self {
        Self {
            id: MessageId::new(),
            session_id: SessionId::new(),
            client_request: JsonRpcRequest {
                jsonrpc: "2.0".to_string(),
                id: serde_json::Value::Null,
                method: String::new(),
                params: None,
            },
            server_request: None,
            server_response: None,
            client_response: None,
            transformations: Vec::new(),
            timing: MessageTiming {
                received_at: Utc::now(),
                forwarded_at: None,
                responded_at: None,
            },
            status: MessageStatus::default(),
        }
    }
}
