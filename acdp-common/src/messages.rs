use crate::{
    AppliedTransformation, ClientId, ClientInfo, GatewayMetrics, GatewayState, HealthMetrics,
    LogEntry, MessageFlow, ProxyId, ProxyInfo, ProxySession, ProxyStats, RoutingDecision,
    RoutingRule, ServerId, ServerInfo, SessionId, TransformationRule,
};
use crate::{JsonRpcRequest, JsonRpcResponse};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Statistics for an interceptor
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InterceptorInfo {
    pub name: String,
    pub priority: u32,
    pub enabled: bool,
    pub total_intercepted: u64,
    pub total_modified: u64,
    pub total_blocked: u64,
    pub avg_processing_time_ms: f64,
}

/// Manager-level interceptor statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InterceptorManagerInfo {
    pub total_messages_processed: u64,
    pub total_modifications_made: u64,
    pub total_messages_blocked: u64,
    pub avg_processing_time_ms: f64,
    pub messages_by_method: HashMap<String, u64>,
    pub interceptors: Vec<InterceptorInfo>,
}

/// Session-level LLM metrics emitted over IPC.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionMetrics {
    pub proxy_id: ProxyId,
    pub session_id: SessionId,
    pub total_predictions: u64,
    pub successful_predictions: u64,
    pub accuracy: f32,
    pub optimization_score: f32,
    pub message_count: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum IpcMessage {
    // Proxy -> Monitor messages
    ProxyStarted(ProxyInfo),
    ProxyStopped(ProxyId),
    LogEntry(LogEntry),
    StatsUpdate(ProxyStats),
    InterceptorStats {
        proxy_id: ProxyId,
        stats: InterceptorManagerInfo,
    },
    ClientConnected(ClientInfo),
    ClientDisconnected(ClientId),
    ClientUpdated(ClientInfo),
    ClientRequest {
        client_id: ClientId,
        request: JsonRpcRequest,
        session_id: Option<SessionId>,
    },
    ServerConnected(ServerInfo),
    ServerDisconnected(ServerId),
    ServerUpdated(ServerInfo),
    ServerResponse {
        server_id: ServerId,
        response: JsonRpcResponse,
        session_id: Option<SessionId>,
    },
    ServerHealthUpdate {
        server_id: ServerId,
        metrics: HealthMetrics,
    },
    SessionStarted(ProxySession),
    SessionUpdated(ProxySession),
    SessionEnded(SessionId),
    TransformationRules(Vec<TransformationRule>),
    TransformationApplied {
        session_id: SessionId,
        transformation: AppliedTransformation,
    },
    RoutingRules(Vec<RoutingRule>),
    RoutingDecision(RoutingDecision),
    SemanticPrediction {
        query: String,
        predicted_tool: String,
        confidence: f64,
        actual_tool: Option<String>,
        success: Option<bool>,
    },
    GatewayStateUpdated(GatewayState),
    GatewayMetrics(GatewayMetrics),
    MessageFlowUpdate(MessageFlow),
    SessionStats(SessionMetrics),

    // Monitor -> Proxy messages
    GetStatus(ProxyId),
    GetLogs {
        proxy_id: ProxyId,
        limit: Option<usize>,
    },
    Shutdown(ProxyId),
    ToggleInterceptor {
        proxy_id: ProxyId,
        interceptor_name: String,
    },

    // TUI -> Proxy query routing (legacy text-based)
    TuiQuery {
        query: String,
        correlation_id: uuid::Uuid,
    },

    // TUI -> Proxy MCP method call (structured)
    TuiMcpRequest {
        method: String,
        params: Option<serde_json::Value>,
        correlation_id: uuid::Uuid,
    },

    // Proxy -> TUI query/method response
    TuiQueryResponse {
        correlation_id: uuid::Uuid,
        response: String,
        error: Option<String>,
        /// Optional metrics from LLM processing
        ttft_ms: Option<f64>,
        tokens_per_sec: Option<f64>,
        total_tokens: Option<usize>,
        /// Total interceptor delay in milliseconds (routing + LLM reasoning)
        interceptor_delay_ms: Option<f64>,
    },
    RoutingModeChange {
        proxy_id: ProxyId,
        mode: String,
    },
    RoutingModeChanged {
        proxy_id: ProxyId,
        mode: String,
    },

    // Bidirectional messages
    Ping,
    Pong,

    // Error handling
    Error {
        message: String,
        proxy_id: Option<ProxyId>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IpcEnvelope {
    pub message: IpcMessage,
    pub timestamp: chrono::DateTime<chrono::Utc>,
    pub correlation_id: Option<uuid::Uuid>,
}
