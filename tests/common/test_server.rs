//! Common test MCP server implementation

use chrono::Utc;
use acdp_common::types::{ProxySession, SessionId, SessionStatus};
use std::sync::Arc;
use tokio::sync::RwLock;

/// A test MCP server for integration testing
pub struct TestMcpServer {
    pub sessions: Arc<RwLock<Vec<ProxySession>>>,
    pub port: u16,
}

impl TestMcpServer {
    pub fn new(port: u16) -> Self {
        Self {
            sessions: Arc::new(RwLock::new(Vec::new())),
            port,
        }
    }

    pub async fn start(&self) -> Result<(), Box<dyn std::error::Error>> {
        // TODO: Implement actual test server startup
        println!("Test MCP server started on port {}", self.port);
        Ok(())
    }

    pub async fn stop(&self) {
        // TODO: Implement graceful shutdown
        println!("Test MCP server stopped");
    }

    pub async fn add_test_session(&self, session_id: SessionId) {
        let mut session = ProxySession::default();
        session.id = session_id;
        session.status = SessionStatus::Active;
        session.last_activity = Utc::now();

        self.sessions.write().await.push(session);
    }

    pub async fn get_sessions(&self) -> Vec<ProxySession> {
        self.sessions.read().await.clone()
    }
}
