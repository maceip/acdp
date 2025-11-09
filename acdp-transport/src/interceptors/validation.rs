//! Validation interceptor that ensures MCP messages conform to the protocol

use futures::future::{BoxFuture, FutureExt};
use acdp_core::interceptor::{
    InterceptionResult, InterceptorStats, MessageContext, MessageInterceptor,
};
use acdp_core::messages::JsonRpcMessage;
use acdp_core::McpResult;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::warn;

/// Interceptor that validates MCP messages for protocol compliance
pub struct ValidationInterceptor {
    name: String,
    stats: Arc<RwLock<InterceptorStats>>,
    /// Whether to block invalid messages (or just warn)
    strict_mode: bool,
}

impl ValidationInterceptor {
    /// Create a new validation interceptor
    pub fn new(strict_mode: bool) -> Self {
        Self {
            name: "ValidationInterceptor".to_string(),
            stats: Arc::new(RwLock::new(InterceptorStats::default())),
            strict_mode,
        }
    }

    /// Validate a JSON-RPC message
    fn validate_message(&self, message: &JsonRpcMessage) -> Result<(), String> {
        match message {
            JsonRpcMessage::Request(req) => {
                // Validate request structure
                if req.jsonrpc != "2.0" {
                    return Err(format!("Invalid JSON-RPC version: {}", req.jsonrpc));
                }
                if req.method.is_empty() {
                    return Err("Request method cannot be empty".to_string());
                }
                // Validate method follows MCP naming conventions (e.g., "tools/list")
                if !req.method.contains('/') && !req.method.starts_with("initialize") {
                    warn!(
                        "Method '{}' doesn't follow MCP naming convention",
                        req.method
                    );
                }
                Ok(())
            }
            JsonRpcMessage::Response(resp) => {
                // Validate response structure
                if resp.jsonrpc != "2.0" {
                    return Err(format!("Invalid JSON-RPC version: {}", resp.jsonrpc));
                }
                // Must have either result or error, not both
                match (&resp.result, &resp.error) {
                    (Some(_), Some(_)) => Err("Response has both result and error".to_string()),
                    (None, None) => Err("Response must have either result or error".to_string()),
                    _ => Ok(()),
                }
            }
            JsonRpcMessage::Notification(notif) => {
                // Validate notification structure
                if notif.jsonrpc != "2.0" {
                    return Err(format!("Invalid JSON-RPC version: {}", notif.jsonrpc));
                }
                if notif.method.is_empty() {
                    return Err("Notification method cannot be empty".to_string());
                }
                Ok(())
            }
        }
    }
}

impl MessageInterceptor for ValidationInterceptor {
    fn name(&self) -> &str {
        &self.name
    }

    fn priority(&self) -> u32 {
        // Run early (low priority) to validate before other modifications
        20
    }

    fn should_intercept<'a>(&'a self, _context: &'a MessageContext) -> BoxFuture<'a, bool> {
        async move { true }.boxed()
    }

    fn intercept<'a>(
        &'a self,
        context: MessageContext,
    ) -> BoxFuture<'a, McpResult<InterceptionResult>> {
        async move {
            let start = std::time::Instant::now();

            // Validate the message
            match self.validate_message(&context.message) {
                Ok(()) => {
                    // Message is valid, update stats and pass through
                    let mut stats = self.stats.write().await;
                    stats.total_intercepted += 1;
                    stats.last_processed = Some(chrono::Utc::now());

                    let elapsed = start.elapsed().as_millis() as f64;
                    stats.avg_processing_time_ms = (stats.avg_processing_time_ms
                        * (stats.total_intercepted - 1) as f64
                        + elapsed)
                        / stats.total_intercepted as f64;

                    Ok(InterceptionResult::pass_through(context.message))
                }
                Err(err) => {
                    warn!(
                        "[{}] Validation failed: {} for message: {:?}",
                        self.name, err, context.message
                    );

                    // Update stats
                    let mut stats = self.stats.write().await;
                    stats.total_intercepted += 1;
                    stats.last_processed = Some(chrono::Utc::now());

                    if self.strict_mode {
                        // Block invalid messages in strict mode
                        stats.total_blocked += 1;
                        Ok(InterceptionResult::blocked(format!(
                            "Protocol validation failed: {}",
                            err
                        )))
                    } else {
                        // Just warn but pass through
                        Ok(InterceptionResult::pass_through(context.message))
                    }
                }
            }
        }
        .boxed()
    }

    fn get_stats<'a>(&'a self) -> BoxFuture<'a, InterceptorStats> {
        async move { self.stats.read().await.clone() }.boxed()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use acdp_core::interceptor::MessageDirection;
    use acdp_core::messages::{
        JsonRpcError, JsonRpcNotification, JsonRpcRequest, JsonRpcResponse, RequestId,
    };
    use serde_json::json;

    #[tokio::test]
    async fn test_validation_interceptor_valid_request() {
        let interceptor = ValidationInterceptor::new(true);

        let request = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: RequestId::from(1i64),
            method: "tools/list".to_string(),
            params: Some(json!({})),
        };

        let context =
            MessageContext::new(JsonRpcMessage::Request(request), MessageDirection::Outgoing);

        let result = interceptor.intercept(context).await.unwrap();

        assert!(!result.modified);
        assert!(!result.block);
    }

    #[tokio::test]
    async fn test_validation_interceptor_invalid_version() {
        let interceptor = ValidationInterceptor::new(true);

        let request = JsonRpcRequest {
            jsonrpc: "1.0".to_string(), // Invalid version
            id: RequestId::from(1i64),
            method: "tools/list".to_string(),
            params: Some(json!({})),
        };

        let context =
            MessageContext::new(JsonRpcMessage::Request(request), MessageDirection::Outgoing);

        let result = interceptor.intercept(context).await.unwrap();

        assert!(result.block); // Should be blocked in strict mode
    }

    #[tokio::test]
    async fn test_validation_interceptor_lenient_mode() {
        let interceptor = ValidationInterceptor::new(false); // Lenient mode

        let request = JsonRpcRequest {
            jsonrpc: "1.0".to_string(), // Invalid version
            id: RequestId::from(1i64),
            method: "tools/list".to_string(),
            params: Some(json!({})),
        };

        let context =
            MessageContext::new(JsonRpcMessage::Request(request), MessageDirection::Outgoing);

        let result = interceptor.intercept(context).await.unwrap();

        assert!(!result.block); // Should pass through in lenient mode
    }

    #[tokio::test]
    async fn test_validation_interceptor_response_both_result_and_error() {
        let interceptor = ValidationInterceptor::new(true);

        let response = JsonRpcResponse {
            jsonrpc: "2.0".to_string(),
            id: RequestId::from(1i64),
            result: Some(json!({"status": "ok"})), // Has both result and error - invalid!
            error: Some(JsonRpcError {
                code: -1,
                message: "error".to_string(),
                data: None,
            }),
        };

        let context = MessageContext::new(
            JsonRpcMessage::Response(response),
            MessageDirection::Incoming,
        );

        let result = interceptor.intercept(context).await.unwrap();

        assert!(result.block);

        let stats = interceptor.get_stats().await;
        assert_eq!(stats.total_blocked, 1);
    }

    #[tokio::test]
    async fn test_validation_interceptor_notification() {
        let interceptor = ValidationInterceptor::new(true);

        let notification = JsonRpcNotification {
            jsonrpc: "2.0".to_string(),
            method: "notifications/message".to_string(),
            params: Some(json!({"level": "info", "message": "test"})),
        };

        let context = MessageContext::new(
            JsonRpcMessage::Notification(notification),
            MessageDirection::Incoming,
        );

        let result = interceptor.intercept(context).await.unwrap();

        assert!(!result.block);
        assert!(!result.modified);
    }
}
