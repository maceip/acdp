//! Configuration wrapper for TUI

use anyhow::Result;
use acdp_llm::config::AppConfig as LlmAppConfig;

/// TUI configuration wrapper
#[derive(Clone)]
pub struct Config {
    inner: LlmAppConfig,
}

impl Config {
    /// Load configuration
    pub fn load() -> Result<Self> {
        let inner =
            LlmAppConfig::load().map_err(|e| anyhow::anyhow!("Failed to load config: {}", e))?;
        Ok(Self { inner })
    }

    /// Get LLM configuration
    pub fn llm_config(&self) -> &LlmAppConfig {
        &self.inner
    }

    /// Get preferred model name
    pub fn preferred_model(&self) -> Option<&str> {
        self.inner.model.preferred_model.as_deref()
    }

    /// Get cache directory
    pub fn cache_dir(&self) -> Result<std::path::PathBuf> {
        self.inner
            .cache_dir()
            .map_err(|e| anyhow::anyhow!("Failed to get cache dir: {}", e))
    }

    /// Check if MCP server mode is enabled
    pub fn enable_mcp_server(&self) -> bool {
        // Environment variable takes precedence
        if let Ok(value) = std::env::var("MCP_SERVER_MODE") {
            return matches!(
                value.trim().to_ascii_lowercase().as_str(),
                "1" | "true" | "yes" | "on"
            );
        }
        // Fall back to config file
        self.inner.mcp_server.enabled
    }

    /// Get MCP server bind address
    pub fn mcp_server_bind_addr(&self) -> String {
        std::env::var("MCP_SERVER_BIND_ADDR")
            .unwrap_or_else(|_| self.inner.mcp_server.bind_address.clone())
    }

    /// Check if backend command is needed (i.e., backend_url is not set)
    pub fn needs_backend_command(&self) -> bool {
        self.backend_url().is_none()
    }

    /// Get backend MCP server command to spawn per client
    /// Returns error if not configured (empty) and backend_url not set
    /// Returns Ok(None) if backend_url is set (command not needed)
    pub fn backend_command(&self) -> Result<Option<String>> {
        // Check if backend_url is set (takes precedence)
        if self.backend_url().is_some() {
            return Ok(None); // backend_url is set, command not needed
        }

        let cmd = std::env::var("MCP_BACKEND_COMMAND")
            .unwrap_or_else(|_| self.inner.mcp_server.backend_command.clone());

        if cmd.is_empty() {
            Err(anyhow::anyhow!(
                "Either backend_command or backend_url is required but neither is configured. \
                Set MCP_BACKEND_COMMAND or MCP_BACKEND_URL environment variable, or configure in config file."
            ))
        } else {
            Ok(Some(cmd))
        }
    }

    /// Get backend command as String (for backward compatibility)
    /// Returns empty string if backend_url is set
    #[deprecated(note = "Use backend_command() which returns Option<String> instead")]
    pub fn backend_command_string(&self) -> Result<String> {
        match self.backend_command() {
            Ok(Some(cmd)) => Ok(cmd),
            Ok(None) => Ok(String::new()),
            Err(e) => Err(e),
        }
    }

    /// Get backend upstream server URL (if configured)
    pub fn backend_url(&self) -> Option<String> {
        std::env::var("MCP_BACKEND_URL")
            .ok()
            .or_else(|| self.inner.mcp_server.backend_url.clone())
    }

    /// Get backend transport type (auto-detected from URL if not set)
    pub fn backend_transport(&self) -> Option<String> {
        std::env::var("MCP_BACKEND_TRANSPORT")
            .ok()
            .or_else(|| {
                // Auto-detect from URL
                self.backend_url().and_then(|url| {
                    if url.contains("/stream") || url.contains("/message") {
                        Some("http-stream".to_string())
                    } else if url.starts_with("http://") || url.starts_with("https://") {
                        Some("http-sse".to_string())
                    } else {
                        None
                    }
                })
            })
            .or_else(|| self.inner.mcp_server.backend_transport.clone())
    }

    /// Get server bind address (for proxy server mode)
    pub fn server_bind_addr(&self) -> Option<String> {
        std::env::var("MCP_SERVER_BIND_ADDR")
            .ok()
            .or_else(|| self.inner.mcp_server.server_bind_addr.clone())
            .or_else(|| {
                // Fallback to http_bind_address or bind_address
                let http_addr = self.http_server_bind_addr();
                if !http_addr.is_empty() {
                    Some(http_addr)
                } else {
                    Some(self.mcp_server_bind_addr())
                }
            })
    }

    /// Get maximum concurrent clients
    pub fn max_concurrent_clients(&self) -> usize {
        std::env::var("MCP_MAX_CLIENTS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(self.inner.mcp_server.max_concurrent_clients)
    }

    /// Get connection timeout in seconds
    pub fn connection_timeout_secs(&self) -> u64 {
        std::env::var("MCP_CONNECTION_TIMEOUT")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(self.inner.mcp_server.connection_timeout_secs)
    }

    /// Get whether to auto-restart backend processes
    pub fn auto_restart_backend(&self) -> bool {
        std::env::var("MCP_AUTO_RESTART")
            .map(|value| {
                matches!(
                    value.trim().to_ascii_lowercase().as_str(),
                    "1" | "true" | "yes" | "on"
                )
            })
            .unwrap_or(self.inner.mcp_server.auto_restart_backend)
    }

    /// Get HTTP+SSE server bind address (empty = disabled)
    pub fn http_server_bind_addr(&self) -> String {
        std::env::var("MCP_HTTP_BIND_ADDR")
            .unwrap_or_else(|_| self.inner.mcp_server.http_bind_address.clone())
    }

    /// Check if HTTP+SSE server is enabled
    pub fn enable_http_server(&self) -> bool {
        !self.http_server_bind_addr().is_empty()
    }

    /// Get TLS configuration
    pub fn tls_config(&self) -> &acdp_llm::config::TlsConfig {
        &self.inner.mcp_server.tls
    }

    /// Check if TLS is enabled
    pub fn enable_tls(&self) -> bool {
        std::env::var("MCP_TLS_ENABLED")
            .map(|value| {
                matches!(
                    value.trim().to_ascii_lowercase().as_str(),
                    "1" | "true" | "yes" | "on"
                )
            })
            .unwrap_or(self.inner.mcp_server.tls.enabled)
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            inner: LlmAppConfig::default(),
        }
    }
}
