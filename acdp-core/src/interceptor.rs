//! Message Interceptor for MCP Traffic
//!
//! This module provides interfaces for intercepting, watching, and modifying
//! MCP protocol messages as they flow between client and server.

use futures::future::BoxFuture;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

use crate::messages::JsonRpcMessage;
use crate::McpResult;

/// Direction of message flow
#[derive(Debug, Clone, PartialEq)]
pub enum MessageDirection {
    /// Message from client to server
    Outgoing,
    /// Message from server to client
    Incoming,
}

/// Context for intercepted messages
#[derive(Debug, Clone)]
pub struct MessageContext {
    /// The message being intercepted
    pub message: JsonRpcMessage,
    /// Direction of message flow
    pub direction: MessageDirection,
    /// Timestamp when message was intercepted
    pub timestamp: chrono::DateTime<chrono::Utc>,
    /// Session identifier
    pub session_id: Option<String>,
    /// Additional metadata
    pub metadata: HashMap<String, serde_json::Value>,
}

impl MessageContext {
    /// Create a new message context
    pub fn new(message: JsonRpcMessage, direction: MessageDirection) -> Self {
        Self {
            message,
            direction,
            timestamp: chrono::Utc::now(),
            session_id: None,
            metadata: HashMap::new(),
        }
    }

    /// Get the method name if this is a request
    pub fn method(&self) -> Option<&str> {
        match &self.message {
            JsonRpcMessage::Request(req) => Some(&req.method),
            JsonRpcMessage::Notification(notif) => Some(&notif.method),
            _ => None,
        }
    }

    /// Get the message ID if this is a request/response
    pub fn id(&self) -> Option<String> {
        match &self.message {
            JsonRpcMessage::Request(req) => Some(req.id.to_string()),
            JsonRpcMessage::Response(resp) => Some(resp.id.to_string()),
            _ => None,
        }
    }
}

/// Result of message interception
#[derive(Debug, Clone)]
pub struct InterceptionResult {
    /// Whether the message was modified
    pub modified: bool,
    /// The potentially modified message
    pub message: JsonRpcMessage,
    /// Whether to block the message (don't send/receive it)
    pub block: bool,
    /// Reason for blocking/modification
    pub reasoning: Option<String>,
    /// Confidence score for modifications (0.0 to 1.0)
    pub confidence: Option<f64>,
}

impl InterceptionResult {
    /// Create a result that passes the message through unchanged
    pub fn pass_through(message: JsonRpcMessage) -> Self {
        Self {
            modified: false,
            message,
            block: false,
            reasoning: None,
            confidence: None,
        }
    }

    /// Create a result that modifies the message
    pub fn modified(message: JsonRpcMessage, reasoning: String, confidence: f64) -> Self {
        Self {
            modified: true,
            message,
            block: false,
            reasoning: Some(reasoning),
            confidence: Some(confidence),
        }
    }

    /// Create a result that blocks the message
    pub fn blocked(reasoning: String) -> Self {
        Self {
            modified: false,
            // Message doesn't matter when blocked
            message: JsonRpcMessage::Notification(crate::messages::JsonRpcNotification {
                jsonrpc: "2.0".to_string(),
                method: "blocked".to_string(),
                params: None,
            }),
            block: true,
            reasoning: Some(reasoning),
            confidence: None,
        }
    }
}

/// Trait for message interceptors that can watch and modify MCP traffic
pub trait MessageInterceptor: Send + Sync {
    /// Get the name of this interceptor
    fn name(&self) -> &str;

    /// Get the priority of this interceptor (higher = runs later)
    fn priority(&self) -> u32 {
        50
    }

    /// Determine if this interceptor should handle the given message
    fn should_intercept<'a>(&'a self, context: &'a MessageContext) -> BoxFuture<'a, bool>;

    /// Intercept and potentially modify a message
    fn intercept<'a>(
        &'a self,
        context: MessageContext,
    ) -> BoxFuture<'a, McpResult<InterceptionResult>>;

    /// Get statistics about this interceptor
    fn get_stats<'a>(&'a self) -> BoxFuture<'a, InterceptorStats>;
}

/// Statistics for an interceptor
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct InterceptorStats {
    /// Total messages intercepted
    pub total_intercepted: u64,
    /// Total messages modified
    pub total_modified: u64,
    /// Total messages blocked
    pub total_blocked: u64,
    /// Average processing time in milliseconds
    pub avg_processing_time_ms: f64,
    /// Last processed timestamp
    pub last_processed: Option<chrono::DateTime<chrono::Utc>>,
}

/// Manager for multiple message interceptors
pub struct InterceptorManager {
    interceptors: Arc<RwLock<Vec<Arc<dyn MessageInterceptor>>>>,
    stats: Arc<RwLock<InterceptorManagerStats>>,
}

/// Statistics for the interceptor manager
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct InterceptorManagerStats {
    /// Total messages processed
    pub total_messages_processed: u64,
    /// Total modifications made
    pub total_modifications_made: u64,
    /// Total messages blocked
    pub total_messages_blocked: u64,
    /// Average processing time across all interceptors
    pub avg_processing_time_ms: f64,
    /// Messages processed by method
    pub messages_by_method: HashMap<String, u64>,
}

/// Metadata supplied alongside a message when invoking interceptors.
#[derive(Debug, Clone, Default)]
pub struct InterceptionMetadata {
    /// Optional session identifier associated with the message.
    pub session_id: Option<String>,
    /// Arbitrary metadata key/value pairs forwarded to interceptors.
    pub extra: HashMap<String, Value>,
}

impl InterceptionMetadata {
    /// Create a new empty metadata container.
    pub fn new() -> Self {
        Self::default()
    }

    /// Attach a session identifier to the metadata.
    pub fn with_session_id(mut self, session_id: impl Into<String>) -> Self {
        self.session_id = Some(session_id.into());
        self
    }

    /// Insert a custom metadata entry.
    pub fn insert(mut self, key: impl Into<String>, value: Value) -> Self {
        self.extra.insert(key.into(), value);
        self
    }
}

impl InterceptorManager {
    /// Create a new interceptor manager
    pub fn new() -> Self {
        Self {
            interceptors: Arc::new(RwLock::new(Vec::new())),
            stats: Arc::new(RwLock::new(InterceptorManagerStats::default())),
        }
    }

    /// Add an interceptor to the manager
    pub async fn add_interceptor(&self, interceptor: Arc<dyn MessageInterceptor>) {
        let mut interceptors = self.interceptors.write().await;
        interceptors.push(interceptor);

        // Sort by priority (lower priority runs first)
        interceptors.sort_by_key(|i| i.priority());
    }

    /// Remove an interceptor by name
    pub async fn remove_interceptor(&self, name: &str) -> bool {
        let mut interceptors = self.interceptors.write().await;
        let initial_len = interceptors.len();
        interceptors.retain(|i| i.name() != name);
        interceptors.len() != initial_len
    }

    /// Process a message through all applicable interceptors
    pub async fn process_message(
        &self,
        message: JsonRpcMessage,
        direction: MessageDirection,
    ) -> McpResult<InterceptionResult> {
        self.process_message_with_metadata(message, direction, InterceptionMetadata::default())
            .await
    }

    /// Process a message through all applicable interceptors with additional metadata.
    pub async fn process_message_with_metadata(
        &self,
        message: JsonRpcMessage,
        direction: MessageDirection,
        metadata: InterceptionMetadata,
    ) -> McpResult<InterceptionResult> {
        let start_time = std::time::Instant::now();
        let mut context = MessageContext::new(message.clone(), direction);
        context.session_id = metadata.session_id;
        for (key, value) in metadata.extra {
            context.metadata.insert(key, value);
        }

        let interceptors = self.interceptors.read().await;
        let mut current_message = message;
        let mut was_modified = false;
        let mut modification_reasoning = Vec::new();
        let mut confidence_sum = 0.0;
        let mut confidence_count = 0;

        for interceptor in interceptors.iter() {
            if interceptor.should_intercept(&context).await {
                let interceptor_start = std::time::Instant::now();

                // Update context with current message
                context.message = current_message.clone();

                match interceptor.intercept(context.clone()).await {
                    Ok(result) => {
                        if result.block {
                            // Update stats
                            let mut stats = self.stats.write().await;
                            stats.total_messages_processed += 1;
                            stats.total_messages_blocked += 1;
                            stats.avg_processing_time_ms = (stats.avg_processing_time_ms
                                * (stats.total_messages_processed - 1) as f64
                                + start_time.elapsed().as_millis() as f64)
                                / stats.total_messages_processed as f64;

                            return Ok(result);
                        }

                        if result.modified {
                            was_modified = true;
                            current_message = result.message.clone();
                            if let Some(reasoning) = &result.reasoning {
                                modification_reasoning.push(reasoning.clone());
                            }
                            if let Some(confidence) = result.confidence {
                                confidence_sum += confidence;
                                confidence_count += 1;
                            }
                        }
                    }
                    Err(e) => {
                        tracing::warn!("Interceptor {} failed: {}", interceptor.name(), e);
                        // Continue with other interceptors
                    }
                }

                let interceptor_time = interceptor_start.elapsed();
                tracing::debug!(
                    "Interceptor {} processed message in {}ms",
                    interceptor.name(),
                    interceptor_time.as_millis()
                );
            }
        }

        let total_time = start_time.elapsed();

        // Update final stats
        {
            let mut stats = self.stats.write().await;
            stats.total_messages_processed += 1;
            if was_modified {
                stats.total_modifications_made += 1;
            }
            stats.avg_processing_time_ms = (stats.avg_processing_time_ms
                * (stats.total_messages_processed - 1) as f64
                + total_time.as_millis() as f64)
                / stats.total_messages_processed as f64;

            if let Some(method) = context.method() {
                *stats
                    .messages_by_method
                    .entry(method.to_string())
                    .or_insert(0) += 1;
            }
        }

        let final_confidence = if confidence_count > 0 {
            confidence_sum / confidence_count as f64
        } else {
            0.0
        };

        Ok(if was_modified {
            InterceptionResult::modified(
                current_message,
                modification_reasoning.join("; "),
                final_confidence,
            )
        } else {
            InterceptionResult::pass_through(current_message)
        })
    }

    /// Get manager statistics
    pub async fn get_stats(&self) -> InterceptorManagerStats {
        self.stats.read().await.clone()
    }

    /// List all registered interceptors
    pub async fn list_interceptors(&self) -> Vec<String> {
        let interceptors = self.interceptors.read().await;
        interceptors.iter().map(|i| i.name().to_string()).collect()
    }
}

impl Default for InterceptorManager {
    fn default() -> Self {
        Self::new()
    }
}
