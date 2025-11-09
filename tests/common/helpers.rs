//! Common test helpers and utilities

use anyhow::Result;
use std::time::Duration;
use tokio::time::timeout;

/// Helper to create test timeouts
pub fn test_timeout() -> Duration {
    Duration::from_secs(30)
}

/// Helper to run async operations with timeout
pub async fn with_timeout<T, F>(duration: Duration, future: F) -> Result<T>
where
    F: std::future::Future<Output = Result<T>>,
{
    timeout(duration, future)
        .await
        .map_err(|_| anyhow::anyhow!("Operation timed out after {:?}", duration))?
}

/// Generate a random test port
pub fn random_test_port() -> u16 {
    8000 + (rand::random::<u16>() % 1000)
}

/// Create a test session ID
pub fn test_session_id() -> String {
    format!("test-session-{}", uuid::Uuid::new_v4())
}

/// Wait for a condition to be true with timeout
pub async fn wait_for_condition<F, Fut>(
    condition: F,
    timeout_duration: Duration,
    check_interval: Duration,
) -> Result<()>
where
    F: Fn() -> Fut,
    Fut: std::future::Future<Output = bool>,
{
    let start = std::time::Instant::now();

    while start.elapsed() < timeout_duration {
        if condition().await {
            return Ok(());
        }
        tokio::time::sleep(check_interval).await;
    }

    Err(anyhow::anyhow!("Condition not met within timeout"))
}
