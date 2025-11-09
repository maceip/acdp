//! Rate limiting interceptor that prevents request flooding

use futures::future::{BoxFuture, FutureExt};
use acdp_core::interceptor::{
    InterceptionResult, InterceptorStats, MessageContext, MessageDirection, MessageInterceptor,
};
use acdp_core::McpResult;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tokio::time::Instant;
use tracing::warn;

/// Rate limiter using a sliding window algorithm
struct RateLimiter {
    /// Max requests per window
    max_requests: usize,
    /// Window duration in seconds
    window_secs: u64,
    /// Request timestamps per method
    request_history: HashMap<String, Vec<Instant>>,
}

impl RateLimiter {
    fn new(max_requests: usize, window_secs: u64) -> Self {
        Self {
            max_requests,
            window_secs,
            request_history: HashMap::new(),
        }
    }

    /// Check if a request should be allowed
    fn check_and_record(&mut self, method: &str) -> bool {
        let now = Instant::now();
        let window_start = now - tokio::time::Duration::from_secs(self.window_secs);

        // Get or create history for this method
        let history = self.request_history.entry(method.to_string()).or_default();

        // Remove old requests outside the window
        history.retain(|&timestamp| timestamp > window_start);

        // Check if we're under the limit
        if history.len() < self.max_requests {
            // Record this request
            history.push(now);
            true
        } else {
            false
        }
    }

    /// Get current rate for a method
    fn get_current_rate(&self, method: &str) -> usize {
        let now = Instant::now();
        let window_start = now - tokio::time::Duration::from_secs(self.window_secs);

        self.request_history
            .get(method)
            .map(|history| history.iter().filter(|&&ts| ts > window_start).count())
            .unwrap_or(0)
    }
}

/// Interceptor that rate-limits MCP requests
pub struct RateLimitInterceptor {
    name: String,
    stats: Arc<RwLock<InterceptorStats>>,
    limiter: Arc<RwLock<RateLimiter>>,
}

impl RateLimitInterceptor {
    /// Create a new rate limit interceptor
    ///
    /// # Arguments
    /// * `max_requests` - Maximum requests allowed per window
    /// * `window_secs` - Window duration in seconds
    pub fn new(max_requests: usize, window_secs: u64) -> Self {
        Self {
            name: "RateLimitInterceptor".to_string(),
            stats: Arc::new(RwLock::new(InterceptorStats::default())),
            limiter: Arc::new(RwLock::new(RateLimiter::new(max_requests, window_secs))),
        }
    }

    /// Create a permissive rate limiter (100 req/min)
    pub fn permissive() -> Self {
        Self::new(100, 60)
    }

    /// Create a moderate rate limiter (30 req/min)
    pub fn moderate() -> Self {
        Self::new(30, 60)
    }

    /// Create a strict rate limiter (10 req/min)
    pub fn strict() -> Self {
        Self::new(10, 60)
    }
}

impl MessageInterceptor for RateLimitInterceptor {
    fn name(&self) -> &str {
        &self.name
    }

    fn priority(&self) -> u32 {
        // Run early (low priority) to block before expensive operations
        30
    }

    fn should_intercept<'a>(&'a self, context: &'a MessageContext) -> BoxFuture<'a, bool> {
        async move {
            matches!(context.direction, MessageDirection::Outgoing) && context.method().is_some()
        }
        .boxed()
    }

    fn intercept<'a>(
        &'a self,
        context: MessageContext,
    ) -> BoxFuture<'a, McpResult<InterceptionResult>> {
        async move {
            let start = std::time::Instant::now();

            let method = context.method().unwrap_or("unknown");

            // Check rate limit
            let mut limiter = self.limiter.write().await;
            let allowed = limiter.check_and_record(method);
            let current_rate = limiter.get_current_rate(method);
            drop(limiter); // Release lock

            // Update stats
            let mut stats = self.stats.write().await;
            stats.total_intercepted += 1;
            stats.last_processed = Some(chrono::Utc::now());

            let elapsed = start.elapsed().as_millis() as f64;
            stats.avg_processing_time_ms =
                (stats.avg_processing_time_ms * (stats.total_intercepted - 1) as f64 + elapsed)
                    / stats.total_intercepted as f64;

            if allowed {
                // Under rate limit
                Ok(InterceptionResult::pass_through(context.message))
            } else {
                // Rate limit exceeded
                stats.total_blocked += 1;
                drop(stats);

                warn!(
                    "[{}] Rate limit exceeded for method '{}' (current rate: {}/window)",
                    self.name, method, current_rate
                );

                Ok(InterceptionResult::blocked(format!(
                    "Rate limit exceeded for method '{}' ({}/window)",
                    method, current_rate
                )))
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
    use acdp_core::messages::{JsonRpcMessage, JsonRpcRequest, RequestId};
    use tokio::time::{sleep, Duration};

    #[tokio::test]
    async fn test_rate_limiter_allows_under_limit() {
        let mut limiter = RateLimiter::new(5, 1);

        // Should allow first 5 requests
        for _ in 0..5 {
            assert!(limiter.check_and_record("test/method"));
        }

        // 6th request should be blocked
        assert!(!limiter.check_and_record("test/method"));
    }

    #[tokio::test]
    async fn test_rate_limiter_sliding_window() {
        let mut limiter = RateLimiter::new(2, 1); // 2 requests per second

        // Use up the limit
        assert!(limiter.check_and_record("test/method"));
        assert!(limiter.check_and_record("test/method"));
        assert!(!limiter.check_and_record("test/method"));

        // Wait for window to slide
        sleep(Duration::from_millis(1100)).await;

        // Should allow again
        assert!(limiter.check_and_record("test/method"));
    }

    #[tokio::test]
    async fn test_rate_limiter_per_method() {
        let mut limiter = RateLimiter::new(2, 1);

        // Different methods have separate limits
        assert!(limiter.check_and_record("method1"));
        assert!(limiter.check_and_record("method1"));
        assert!(!limiter.check_and_record("method1")); // method1 blocked

        // method2 still has quota
        assert!(limiter.check_and_record("method2"));
        assert!(limiter.check_and_record("method2"));
        assert!(!limiter.check_and_record("method2")); // method2 blocked
    }

    #[tokio::test]
    async fn test_rate_limit_interceptor() {
        let interceptor = RateLimitInterceptor::new(3, 1);

        // Send 3 requests (should pass)
        for i in 0..3 {
            let request = JsonRpcRequest {
                jsonrpc: "2.0".to_string(),
                id: RequestId::from(i),
                method: "tools/list".to_string(),
                params: None,
            };

            let context =
                MessageContext::new(JsonRpcMessage::Request(request), MessageDirection::Outgoing);

            let result = interceptor.intercept(context).await.unwrap();
            assert!(!result.block, "Request {} should not be blocked", i);
        }

        // 4th request should be blocked
        let request = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: RequestId::from(4i64),
            method: "tools/list".to_string(),
            params: None,
        };

        let context =
            MessageContext::new(JsonRpcMessage::Request(request), MessageDirection::Outgoing);

        let result = interceptor.intercept(context).await.unwrap();
        assert!(result.block, "4th request should be blocked");

        // Verify stats
        let stats = interceptor.get_stats().await;
        assert_eq!(stats.total_intercepted, 4);
        assert_eq!(stats.total_blocked, 1);
    }

    #[tokio::test]
    async fn test_rate_limit_presets() {
        let permissive = RateLimitInterceptor::permissive();
        let moderate = RateLimitInterceptor::moderate();
        let strict = RateLimitInterceptor::strict();

        assert_eq!(permissive.name(), "RateLimitInterceptor");
        assert_eq!(moderate.name(), "RateLimitInterceptor");
        assert_eq!(strict.name(), "RateLimitInterceptor");
    }
}
