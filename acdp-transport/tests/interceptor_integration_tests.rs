//! Integration tests for interceptors with StdioHandler

use acdp_core::interceptor::InterceptorManager;
use acdp_transport::interceptors::{
    LoggingInterceptor, RateLimitInterceptor, ValidationInterceptor,
};
use std::sync::Arc;

#[tokio::test]
async fn test_interceptor_manager_with_logging() {
    let manager = InterceptorManager::new();

    // Add logging interceptor
    let logging = Arc::new(LoggingInterceptor::new(false));
    manager.add_interceptor(logging.clone()).await;

    // Verify it's registered
    let interceptors = manager.list_interceptors().await;
    assert_eq!(interceptors.len(), 1);
    assert_eq!(interceptors[0], "LoggingInterceptor");

    // Get stats (should be zero initially)
    let stats = manager.get_stats().await;
    assert_eq!(stats.total_messages_processed, 0);
}

#[tokio::test]
async fn test_interceptor_chain_priority_ordering() {
    let manager = InterceptorManager::new();

    // Add interceptors in mixed order
    manager
        .add_interceptor(Arc::new(RateLimitInterceptor::permissive()))
        .await;
    manager
        .add_interceptor(Arc::new(LoggingInterceptor::new(false)))
        .await;
    manager
        .add_interceptor(Arc::new(ValidationInterceptor::new(false)))
        .await;

    // Should be sorted by priority: Logging (10), Validation (20), RateLimit (30)
    let interceptors = manager.list_interceptors().await;
    assert_eq!(interceptors.len(), 3);
}

#[tokio::test]
async fn test_validation_interceptor_blocks_invalid_messages() {
    let manager = InterceptorManager::new();

    // Add strict validation interceptor
    manager
        .add_interceptor(Arc::new(ValidationInterceptor::new(true)))
        .await;

    // Try to process an invalid message (wrong JSON-RPC version)
    use acdp_core::interceptor::MessageDirection;
    use acdp_core::messages::{JsonRpcMessage, JsonRpcRequest, RequestId};
    use serde_json::json;

    let bad_request = JsonRpcRequest {
        jsonrpc: "1.0".to_string(), // Invalid!
        id: RequestId::from(1i64),
        method: "test/method".to_string(),
        params: Some(json!({})),
    };

    let result = manager
        .process_message(
            JsonRpcMessage::Request(bad_request),
            MessageDirection::Outgoing,
        )
        .await
        .unwrap();

    assert!(result.block);
    assert!(result.reasoning.is_some());
}

#[tokio::test]
async fn test_rate_limiter_blocks_excess_requests() {
    let manager = InterceptorManager::new();

    // Add very strict rate limiter (2 requests per second)
    manager
        .add_interceptor(Arc::new(RateLimitInterceptor::new(2, 1)))
        .await;

    use acdp_core::interceptor::MessageDirection;
    use acdp_core::messages::{JsonRpcMessage, JsonRpcRequest, RequestId};

    // First 2 should pass
    for i in 0..2 {
        let request = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: RequestId::from(i),
            method: "tools/list".to_string(),
            params: None,
        };

        let result = manager
            .process_message(JsonRpcMessage::Request(request), MessageDirection::Outgoing)
            .await
            .unwrap();

        assert!(!result.block, "Request {} should not be blocked", i);
    }

    // 3rd should be blocked
    let request = JsonRpcRequest {
        jsonrpc: "2.0".to_string(),
        id: RequestId::from(3i64),
        method: "tools/list".to_string(),
        params: None,
    };

    let result = manager
        .process_message(JsonRpcMessage::Request(request), MessageDirection::Outgoing)
        .await
        .unwrap();

    assert!(result.block, "3rd request should be blocked");

    // Verify stats
    let stats = manager.get_stats().await;
    assert_eq!(stats.total_messages_processed, 3);
    assert_eq!(stats.total_messages_blocked, 1);
}

#[tokio::test]
async fn test_interceptor_manager_stats_tracking() {
    let manager = InterceptorManager::new();

    // Add logging interceptor
    manager
        .add_interceptor(Arc::new(LoggingInterceptor::new(false)))
        .await;

    use acdp_core::interceptor::MessageDirection;
    use acdp_core::messages::{JsonRpcMessage, JsonRpcRequest, RequestId};

    // Process several messages
    for i in 0..5 {
        let request = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: RequestId::from(i),
            method: "tools/list".to_string(),
            params: None,
        };

        manager
            .process_message(JsonRpcMessage::Request(request), MessageDirection::Outgoing)
            .await
            .unwrap();
    }

    // Check stats
    let stats = manager.get_stats().await;
    assert_eq!(stats.total_messages_processed, 5);
    assert_eq!(stats.total_modifications_made, 0); // Logging doesn't modify
    assert_eq!(stats.total_messages_blocked, 0);

    // Check messages by method
    assert_eq!(*stats.messages_by_method.get("tools/list").unwrap(), 5);
}
