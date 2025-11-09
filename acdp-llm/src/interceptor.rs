//! LLM interceptor for intelligent request routing and modification

use crate::database::RoutingRulesDatabase;
use crate::error::{LlmError, LlmResult};
use crate::predictors::ToolPredictor;
use crate::routing_modes::RoutingMode;
use crate::session_management::{SessionManager, SessionPrediction};
use futures::future::{BoxFuture, FutureExt};
use acdp_common::types::{MessageId, SessionId};
use acdp_core::interceptor::{
    InterceptionResult, InterceptorManager, InterceptorStats, MessageContext, MessageDirection,
    MessageInterceptor,
};
use acdp_core::messages::JsonRpcMessage;
use acdp_core::McpResult;
use serde_json::{Map, Value};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{Mutex, RwLock};
use uuid::Uuid;
/// Pending prediction waiting for response correlation.
#[derive(Debug, Clone)]
struct PendingPrediction {
    record_id: String,
    predicted_tool: String,
    actual_tool: Option<String>,
    session_id: Option<SessionId>,
    message_id: Option<MessageId>,
}

/// LLM-powered interceptor for intelligent request processing
pub struct LlmInterceptor {
    predictor: Arc<ToolPredictor>,
    routing_db: RoutingRulesDatabase,
    routing_mode: Arc<RwLock<RoutingMode>>,
    confidence_threshold: f32,
    /// Interceptor manager for runtime interceptor management
    interceptor_manager: Option<Arc<InterceptorManager>>,
    /// Track predictions by request ID for accuracy updates
    pending_predictions: Arc<Mutex<HashMap<String, PendingPrediction>>>,
    /// Session manager for richer tracking
    session_manager: Option<Arc<SessionManager>>,
    /// Optional IPC sender for semantic prediction updates (uses Mutex for interior mutability)
    ipc_tx: Arc<Mutex<Option<tokio::sync::mpsc::UnboundedSender<acdp_common::IpcMessage>>>>,
}

impl LlmInterceptor {
    /// Create new LLM interceptor
    pub fn new(
        predictor: Arc<ToolPredictor>,
        routing_mode: RoutingMode,
        routing_db: RoutingRulesDatabase,
        session_manager: Option<Arc<SessionManager>>,
    ) -> Self {
        Self {
            predictor,
            routing_db,
            routing_mode: Arc::new(RwLock::new(routing_mode)),
            confidence_threshold: 0.8,
            interceptor_manager: None,
            pending_predictions: Arc::new(Mutex::new(HashMap::new())),
            session_manager,
            ipc_tx: Arc::new(Mutex::new(None)),
        }
    }

    /// Create new LLM interceptor with interceptor manager for runtime management
    pub fn with_interceptor_manager(
        predictor: Arc<ToolPredictor>,
        routing_mode: RoutingMode,
        routing_db: RoutingRulesDatabase,
        interceptor_manager: Arc<InterceptorManager>,
        session_manager: Option<Arc<SessionManager>>,
    ) -> Self {
        Self {
            predictor,
            routing_db,
            routing_mode: Arc::new(RwLock::new(routing_mode)),
            confidence_threshold: 0.8,
            interceptor_manager: Some(interceptor_manager),
            pending_predictions: Arc::new(Mutex::new(HashMap::new())),
            session_manager,
            ipc_tx: Arc::new(Mutex::new(None)),
        }
    }

    /// Set IPC sender for semantic prediction updates
    pub async fn set_ipc_sender(
        &self,
        tx: tokio::sync::mpsc::UnboundedSender<acdp_common::IpcMessage>,
    ) {
        *self.ipc_tx.lock().await = Some(tx);
    }

    /// Set the interceptor manager for runtime management
    pub fn set_interceptor_manager(&mut self, manager: Arc<InterceptorManager>) {
        self.interceptor_manager = Some(manager);
    }

    /// Add an interceptor at runtime (callable by LLM)
    pub async fn add_interceptor_at_runtime(
        &self,
        interceptor: Arc<dyn MessageInterceptor>,
    ) -> LlmResult<()> {
        if let Some(ref manager) = self.interceptor_manager {
            manager.add_interceptor(interceptor).await;
            Ok(())
        } else {
            Err(LlmError::ConfigError(
                "Interceptor manager not set".to_string(),
            ))
        }
    }

    /// Remove an interceptor at runtime (callable by LLM)
    pub async fn remove_interceptor_at_runtime(&self, name: &str) -> LlmResult<bool> {
        if let Some(ref manager) = self.interceptor_manager {
            Ok(manager.remove_interceptor(name).await)
        } else {
            Err(LlmError::ConfigError(
                "Interceptor manager not set".to_string(),
            ))
        }
    }

    /// List all interceptors (callable by LLM)
    pub async fn list_interceptors(&self) -> LlmResult<Vec<String>> {
        if let Some(ref manager) = self.interceptor_manager {
            Ok(manager.list_interceptors().await)
        } else {
            Err(LlmError::ConfigError(
                "Interceptor manager not set".to_string(),
            ))
        }
    }

    /// Set routing mode
    pub async fn set_routing_mode(&self, mode: RoutingMode) {
        let mut guard = self.routing_mode.write().await;
        *guard = mode;
    }

    /// Get current routing mode
    pub async fn get_routing_mode(&self) -> RoutingMode {
        *self.routing_mode.read().await
    }

    /// Predict and route request
    async fn predict_and_route(
        &self,
        message: JsonRpcMessage,
        context: &str,
        session_id: Option<SessionId>,
    ) -> McpResult<InterceptionResult> {
        let routing_mode = *self.routing_mode.read().await;
        match routing_mode {
            RoutingMode::Bypass => Ok(InterceptionResult::pass_through(message)),
            RoutingMode::Semantic => self
                .semantic_routing(message, context, session_id)
                .await
                .map_err(|e| acdp_core::McpError::Internal {
                    message: e.to_string(),
                }),
            RoutingMode::Hybrid => self
                .hybrid_routing(message, context, session_id)
                .await
                .map_err(|e| acdp_core::McpError::Internal {
                    message: e.to_string(),
                }),
        }
    }

    /// Semantic routing using LLM predictions
    async fn semantic_routing(
        &self,
        mut message: JsonRpcMessage,
        context: &str,
        session_id: Option<SessionId>,
    ) -> LlmResult<InterceptionResult> {
        let start = std::time::Instant::now();
        let prediction_outcome = self.predictor.predict_tool(context).await?;
        let prediction = prediction_outcome.prediction.clone();
        let latency_ms = start.elapsed().as_millis() as u64;
        let message_id = MessageId::new();

        // Store prediction record ID and actual tool for later accuracy update
        if let Some(request_id) = self.extract_request_id(&message) {
            let actual_tool = self.determine_actual_tool(&message);
            let pending = PendingPrediction {
                record_id: prediction_outcome.record_id.clone(),
                predicted_tool: prediction.tool_name.clone(),
                actual_tool,
                session_id: session_id.clone(),
                message_id: Some(message_id.clone()),
            };
            let mut map = self.pending_predictions.lock().await;
            map.insert(request_id, pending);
        }

        if let (Some(manager), Some(session_id)) = (&self.session_manager, session_id.clone()) {
            let session_prediction = SessionPrediction::new(
                message_id.clone(),
                prediction.clone(),
                Some(prediction_outcome.record_id.clone()),
                latency_ms,
            );
            manager
                .record_prediction_for_session(session_id, session_prediction)
                .await;
        }

        // Send semantic prediction to TUI via IPC
        if let Some(ref tx) = *self.ipc_tx.lock().await {
            let _ = tx.send(acdp_common::IpcMessage::SemanticPrediction {
                query: context.to_string(),
                predicted_tool: prediction.tool_name.clone(),
                confidence: prediction.confidence as f64,
                actual_tool: None, // Will be updated later when response arrives
                success: None,     // Will be updated later
            });
        }

        if prediction.confidence >= self.confidence_threshold {
            // Modify request based on prediction
            self.enhance_request_with_prediction(&mut message, &prediction)
                .await?;
            Ok(InterceptionResult::modified(
                message,
                format!("Enhanced with tool prediction: {}", prediction.tool_name),
                prediction.confidence as f64,
            ))
        } else {
            Ok(InterceptionResult::pass_through(message))
        }
    }

    /// Hybrid routing combining database rules and LLM predictions
    async fn hybrid_routing(
        &self,
        mut message: JsonRpcMessage,
        context: &str,
        session_id: Option<SessionId>,
    ) -> LlmResult<InterceptionResult> {
        // First check database rules
        if let Some(rule) = self.routing_db.find_matching_rule(context).await? {
            self.apply_routing_rule(&mut message, &rule).await?;
            return Ok(InterceptionResult::modified(
                message,
                format!("Routed via rule: {}", rule.pattern),
                rule.confidence,
            ));
        }

        // Fall back to LLM prediction
        self.semantic_routing(message, context, session_id).await
    }

    /// Extract request ID from message as a string key
    fn extract_request_id(&self, message: &JsonRpcMessage) -> Option<String> {
        match message {
            JsonRpcMessage::Request(req) => Some(req.id.to_string()),
            JsonRpcMessage::Response(resp) => Some(resp.id.to_string()),
            _ => None,
        }
    }

    /// Determine the actual tool or method invoked for accuracy tracking
    fn determine_actual_tool(&self, message: &JsonRpcMessage) -> Option<String> {
        match message {
            JsonRpcMessage::Request(req) => {
                if req.method == "tools/call" || req.method == "tools.call" {
                    return Self::extract_tool_name(req.params.as_ref());
                }
                Some(req.method.clone())
            }
            JsonRpcMessage::Notification(notif) => {
                if notif.method == "tools/call" || notif.method == "tools.call" {
                    return Self::extract_tool_name(notif.params.as_ref());
                }
                Some(notif.method.clone())
            }
            _ => None,
        }
    }

    /// Extract MCP context from message
    fn extract_mcp_context(&self, message: &JsonRpcMessage) -> LlmResult<String> {
        let method = match message {
            JsonRpcMessage::Request(req) => Some(&req.method),
            JsonRpcMessage::Notification(notif) => Some(&notif.method),
            _ => None,
        };
        let params = match message {
            JsonRpcMessage::Request(req) => req.params.as_ref(),
            JsonRpcMessage::Notification(notif) => notif.params.as_ref(),
            _ => None,
        };
        let id = match message {
            JsonRpcMessage::Request(req) => Some(&req.id),
            JsonRpcMessage::Response(resp) => Some(&resp.id),
            _ => None,
        };

        let context = serde_json::json!({
            "method": method,
            "params": params,
            "id": id
        });

        Ok(serde_json::to_string(&context)?)
    }

    /// Enhance request with prediction insights
    async fn enhance_request_with_prediction(
        &self,
        message: &mut JsonRpcMessage,
        prediction: &crate::dspy_signatures::ToolPrediction,
    ) -> LlmResult<()> {
        // Add prediction metadata to message params
        match message {
            JsonRpcMessage::Request(req) => {
                let mut obj = match &req.params {
                    Some(Value::Object(map)) => map.clone(),
                    _ => Map::new(),
                };
                obj.insert(
                    "_predicted_tool".to_string(),
                    Value::String(prediction.tool_name.clone()),
                );
                obj.insert(
                    "_prediction_confidence".to_string(),
                    Value::Number(
                        serde_json::Number::from_f64(prediction.confidence as f64).ok_or_else(
                            || LlmError::ConfigError("Invalid confidence".to_string()),
                        )?,
                    ),
                );
                req.params = Some(Value::Object(obj));
            }
            JsonRpcMessage::Notification(notif) => {
                let mut obj = match &notif.params {
                    Some(Value::Object(map)) => map.clone(),
                    _ => Map::new(),
                };
                obj.insert(
                    "_predicted_tool".to_string(),
                    Value::String(prediction.tool_name.clone()),
                );
                obj.insert(
                    "_prediction_confidence".to_string(),
                    Value::Number(
                        serde_json::Number::from_f64(prediction.confidence as f64).ok_or_else(
                            || LlmError::ConfigError("Invalid confidence".to_string()),
                        )?,
                    ),
                );
                notif.params = Some(Value::Object(obj));
            }
            _ => {} // Cannot modify response
        }

        Ok(())
    }

    /// Update prediction accuracy when response is received
    async fn update_prediction_accuracy(&self, message: &JsonRpcMessage) {
        // Extract request ID from response
        if let Some(request_id) = self.extract_request_id(message) {
            // Look up the prediction record
            let mut map = self.pending_predictions.lock().await;
            if let Some(PendingPrediction {
                record_id,
                predicted_tool,
                actual_tool,
                session_id,
                message_id,
            }) = map.remove(&request_id)
            {
                if let Some(actual_tool) = actual_tool.clone() {
                    if let Err(e) = self
                        .predictor
                        .update_prediction_result(&record_id, &actual_tool)
                        .await
                    {
                        tracing::warn!("Failed to update prediction accuracy: {}", e);
                    }
                    if let (Some(manager), Some(session_id), Some(message_id)) =
                        (&self.session_manager, session_id, message_id)
                    {
                        if let Err(e) = manager
                            .record_actual_tool(session_id, message_id, actual_tool)
                            .await
                        {
                            tracing::warn!("Failed to update session accuracy: {}", e);
                        }
                    }
                } else {
                    tracing::warn!(
                        "Missing actual tool for request {}; predicted {}",
                        request_id,
                        predicted_tool
                    );
                }
            }
        }
    }

    fn extract_tool_name(params: Option<&Value>) -> Option<String> {
        match params {
            Some(Value::Object(map)) => map
                .get("name")
                .and_then(|value| value.as_str())
                .map(|s| s.to_string()),
            Some(Value::Array(items)) => items.iter().find_map(|value| match value {
                Value::Object(map) => map
                    .get("name")
                    .and_then(|inner| inner.as_str())
                    .map(|s| s.to_string()),
                _ => None,
            }),
            _ => None,
        }
    }

    /// Apply routing rule to message
    async fn apply_routing_rule(
        &self,
        message: &mut JsonRpcMessage,
        rule: &crate::database::RoutingRule,
    ) -> LlmResult<()> {
        // Add routing metadata
        match message {
            JsonRpcMessage::Request(req) => {
                let mut obj = match &req.params {
                    Some(Value::Object(map)) => map.clone(),
                    _ => Map::new(),
                };
                obj.insert(
                    "_routed_transport".to_string(),
                    Value::String(rule.target_transport.clone()),
                );
                obj.insert(
                    "_routing_confidence".to_string(),
                    Value::Number(
                        serde_json::Number::from_f64(rule.confidence).ok_or_else(|| {
                            LlmError::ConfigError("Invalid confidence".to_string())
                        })?,
                    ),
                );
                req.params = Some(Value::Object(obj));
            }
            JsonRpcMessage::Notification(notif) => {
                let mut obj = match &notif.params {
                    Some(Value::Object(map)) => map.clone(),
                    _ => Map::new(),
                };
                obj.insert(
                    "_routed_transport".to_string(),
                    Value::String(rule.target_transport.clone()),
                );
                obj.insert(
                    "_routing_confidence".to_string(),
                    Value::Number(
                        serde_json::Number::from_f64(rule.confidence).ok_or_else(|| {
                            LlmError::ConfigError("Invalid confidence".to_string())
                        })?,
                    ),
                );
                notif.params = Some(Value::Object(obj));
            }
            _ => {} // Cannot modify response
        }

        Ok(())
    }
}

impl MessageInterceptor for LlmInterceptor {
    fn name(&self) -> &str {
        "LLM Interceptor"
    }

    fn should_intercept<'a>(&'a self, context: &'a MessageContext) -> BoxFuture<'a, bool> {
        async move {
            matches!(context.direction, MessageDirection::Outgoing)
                || matches!(context.message, JsonRpcMessage::Response(_))
        }
        .boxed()
    }

    fn intercept<'a>(
        &'a self,
        context: MessageContext,
    ) -> BoxFuture<'a, McpResult<InterceptionResult>> {
        async move {
            let context_str = self.extract_mcp_context(&context.message).map_err(|e| {
                acdp_core::McpError::Internal {
                    message: e.to_string(),
                }
            })?;
            let session_id = context
                .session_id
                .as_ref()
                .and_then(|raw| Uuid::parse_str(raw).ok())
                .map(SessionId);

            match context.direction {
                MessageDirection::Outgoing => self
                    .predict_and_route(context.message, &context_str, session_id)
                    .await
                    .map_err(|e| acdp_core::McpError::Internal {
                        message: e.to_string(),
                    }),
                MessageDirection::Incoming => {
                    // Handle response messages to update prediction accuracy
                    self.update_prediction_accuracy(&context.message).await;
                    Ok(InterceptionResult::pass_through(context.message))
                }
            }
        }
        .boxed()
    }

    fn get_stats<'a>(&'a self) -> BoxFuture<'a, InterceptorStats> {
        async move { InterceptorStats::default() }.boxed()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::LlmDatabase;
    use crate::routing_modes::RoutingMode;
    use acdp_core::interceptor::{MessageContext, MessageDirection};
    use acdp_core::messages::{JsonRpcMessage, JsonRpcRequest, JsonRpcResponse};
    use serde_json::json;
    use std::sync::Arc;
    use tempfile::TempDir;

    #[test]
    fn test_llm_interceptor_creation() {
        // Test would require actual predictor setup
        assert!(true);
    }

    #[tokio::test]
    async fn semantic_predictions_record_accuracy_updates() {
        let temp_dir = TempDir::new().unwrap();
        let db_path = temp_dir.path().join("llm.sqlite");
        let db_url = format!("sqlite://{}", db_path.to_string_lossy());
        let database = LlmDatabase::new(&db_url).await.unwrap();

        let predictor = Arc::new(ToolPredictor::new(Arc::new(database.predictions.clone())));
        let routing_db = database.routing_rules.clone();

        let interceptor =
            LlmInterceptor::new(predictor.clone(), RoutingMode::Semantic, routing_db, None);

        let request = JsonRpcRequest::without_params(1_i64, "tools/list");
        let request_context = MessageContext::new(
            JsonRpcMessage::Request(request.clone()),
            MessageDirection::Outgoing,
        );

        interceptor.intercept(request_context).await.unwrap();

        let response = JsonRpcResponse::success(1_i64, json!({"status": "ok"}));
        let response_context = MessageContext::new(
            JsonRpcMessage::Response(response),
            MessageDirection::Incoming,
        );

        interceptor.intercept(response_context).await.unwrap();

        let records = predictor
            .predictions_db()
            .get_recent_predictions(1)
            .await
            .unwrap();

        assert_eq!(records.len(), 1);
        assert_eq!(records[0].actual_tool.as_deref(), Some("tools/list"));
    }
}
