//! Configuration management for MCP LLM

use crate::error::{LlmError, LlmResult};
use crate::litert_wrapper::LiteRTBackend;
use crate::routing_modes::RoutingMode;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Application configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    /// Model configuration
    #[serde(default)]
    pub model: ModelConfig,
    /// LLM configuration
    #[serde(default)]
    pub llm: LlmConfig,
    /// MCP Server configuration
    #[serde(default)]
    pub mcp_server: McpServerConfig,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            model: ModelConfig::default(),
            llm: LlmConfig::default(),
            mcp_server: McpServerConfig::default(),
        }
    }
}

/// Model configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelConfig {
    /// Preferred model name
    pub preferred_model: Option<String>,
    /// Cache directory for models
    pub cache_dir: String,
}

impl Default for ModelConfig {
    fn default() -> Self {
        Self {
            preferred_model: None,
            cache_dir: "~/.litert-lm/models".to_string(),
        }
    }
}

/// LLM configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmConfig {
    /// Backend to use (cpu or gpu)
    pub backend: String,
    /// Temperature for generation
    pub temperature: f32,
    /// Maximum tokens to generate
    pub max_tokens: usize,
    /// Path to the SQLite database used for routing/prediction data
    #[serde(default = "LlmConfig::default_database_path")]
    pub database_path: String,
    /// Initial routing mode to apply (bypass, semantic, hybrid)
    #[serde(default = "LlmConfig::default_routing_mode")]
    pub routing_mode: String,
    /// Whether to attempt semantic routing via LiteRT/DSPy
    #[serde(default)]
    pub semantic_routing: bool,
    /// Optional semantic model path override
    #[serde(default = "LlmConfig::default_semantic_model_path")]
    pub semantic_model_path: Option<String>,
}

impl Default for LlmConfig {
    fn default() -> Self {
        Self {
            backend: "gpu".to_string(), // Default to GPU, will fallback to CPU if unavailable
            temperature: 0.7,
            max_tokens: 1000,
            database_path: LlmConfig::default_database_path(),
            routing_mode: LlmConfig::default_routing_mode(),
            semantic_routing: false,
            semantic_model_path: LlmConfig::default_semantic_model_path(),
        }
    }
}

impl AppConfig {
    /// Load configuration from file
    pub fn load() -> LlmResult<Self> {
        let config_path = Self::config_path()?;

        if !config_path.exists() {
            // Create default config
            let config = Self::default();
            config.save()?;
            return Ok(config);
        }

        let content = std::fs::read_to_string(&config_path)
            .map_err(|e| LlmError::ConfigError(format!("Failed to read config: {}", e)))?;

        let config: AppConfig = toml::from_str(&content)
            .map_err(|e| LlmError::ConfigError(format!("Failed to parse config: {}", e)))?;

        Ok(config)
    }

    /// Save configuration to file
    pub fn save(&self) -> LlmResult<()> {
        let config_path = Self::config_path()?;

        // Create parent directory if it doesn't exist
        if let Some(parent) = config_path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| {
                LlmError::ConfigError(format!("Failed to create config directory: {}", e))
            })?;
        }

        let content = toml::to_string_pretty(self)
            .map_err(|e| LlmError::ConfigError(format!("Failed to serialize config: {}", e)))?;

        std::fs::write(&config_path, content)
            .map_err(|e| LlmError::ConfigError(format!("Failed to write config: {}", e)))?;

        Ok(())
    }

    /// Get the configuration file path
    pub fn config_path() -> LlmResult<PathBuf> {
        let home = dirs::home_dir()
            .ok_or_else(|| LlmError::ConfigError("Cannot determine home directory".to_string()))?;

        Ok(home.join(".config").join("assist-mcp").join("config.toml"))
    }

    /// Get the expanded cache directory path
    pub fn cache_dir(&self) -> LlmResult<PathBuf> {
        Self::expand_path(&self.model.cache_dir)
    }

    /// Get the SQLite database path
    pub fn database_path(&self) -> LlmResult<PathBuf> {
        Self::expand_path(&self.llm.database_path)
    }

    /// Get the backend type
    pub fn backend(&self) -> LlmResult<LiteRTBackend> {
        match self.llm.backend.to_lowercase().as_str() {
            "cpu" => Ok(LiteRTBackend::Cpu),
            "gpu" => Ok(LiteRTBackend::Gpu),
            _ => Err(LlmError::ConfigError(format!(
                "Invalid backend: {}",
                self.llm.backend
            ))),
        }
    }

    fn expand_path(path: &str) -> LlmResult<PathBuf> {
        if path.starts_with("~/") {
            let home = dirs::home_dir().ok_or_else(|| {
                LlmError::ConfigError("Cannot determine home directory".to_string())
            })?;
            Ok(home.join(&path[2..]))
        } else if path.starts_with('~') {
            let home = dirs::home_dir().ok_or_else(|| {
                LlmError::ConfigError("Cannot determine home directory".to_string())
            })?;
            Ok(home.join(&path[1..]))
        } else {
            Ok(PathBuf::from(path))
        }
    }
}

impl LlmConfig {
    fn default_database_path() -> String {
        "~/.local/share/assist-mcp/llm.sqlite".to_string()
    }

    fn default_routing_mode() -> String {
        "hybrid".to_string()
    }

    fn default_semantic_model_path() -> Option<String> {
        Some("~/.litert-lm/models/gemma3-1b-it-int4.litertlm".to_string())
    }

    /// Resolve routing mode enum from config.
    pub fn routing_mode(&self) -> LlmResult<RoutingMode> {
        self.routing_mode
            .parse::<RoutingMode>()
            .map_err(|e| LlmError::ConfigError(e))
    }

    /// Expanded semantic model path if provided.
    pub fn semantic_model_path(&self) -> LlmResult<Option<PathBuf>> {
        match &self.semantic_model_path {
            Some(path) => AppConfig::expand_path(path).map(Some),
            None => Ok(None),
        }
    }
}

/// MCP Server configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpServerConfig {
    /// Enable MCP server mode
    #[serde(default)]
    pub enabled: bool,
    /// Address to bind TCP MCP server
    #[serde(default = "McpServerConfig::default_bind_addr")]
    pub bind_address: String,
    /// Address to bind HTTP+SSE MCP server (empty = disabled)
    #[serde(default)]
    pub http_bind_address: String,
    /// Backend MCP server command to spawn per client (if backend_url not set)
    /// If backend_url is set, this is ignored
    #[serde(default = "McpServerConfig::default_backend_command")]
    pub backend_command: String,
    /// Backend upstream server URL (alternative to backend_command)
    /// If set, proxy connects to this URL instead of spawning a process
    /// Format: "http://host:port/path" for HTTP-SSE or HTTP-Stream
    /// Auto-detects transport from URL if backend_transport not set
    #[serde(default)]
    pub backend_url: Option<String>,
    /// Backend transport type (auto-detected from backend_url if not set)
    /// Options: "http-sse", "http-stream"
    #[serde(default)]
    pub backend_transport: Option<String>,
    /// Server bind address for HTTP-SSE/HTTP-Stream proxy server
    /// Separate from backend_url (which is where to connect TO)
    /// If not set, uses http_bind_address or bind_address
    #[serde(default)]
    pub server_bind_addr: Option<String>,
    /// Maximum concurrent clients
    #[serde(default = "McpServerConfig::default_max_clients")]
    pub max_concurrent_clients: usize,
    /// Connection timeout in seconds
    #[serde(default = "McpServerConfig::default_connection_timeout")]
    pub connection_timeout_secs: u64,
    /// Whether to restart backend processes on crash
    #[serde(default = "McpServerConfig::default_auto_restart")]
    pub auto_restart_backend: bool,
    /// ACME/TLS configuration
    #[serde(default)]
    pub tls: TlsConfig,
}

impl Default for McpServerConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            bind_address: Self::default_bind_addr(),
            http_bind_address: String::new(), // Empty = disabled
            backend_command: Self::default_backend_command(),
            backend_url: None,
            backend_transport: None,
            server_bind_addr: None,
            max_concurrent_clients: Self::default_max_clients(),
            connection_timeout_secs: Self::default_connection_timeout(),
            auto_restart_backend: Self::default_auto_restart(),
            tls: TlsConfig::default(),
        }
    }
}

/// TLS/ACME configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TlsConfig {
    /// Enable TLS/HTTPS
    #[serde(default)]
    pub enabled: bool,
    /// Domain name(s) for certificate
    #[serde(default)]
    pub domains: Vec<String>,
    /// ACME directory URL (default: Let's Encrypt production)
    #[serde(default = "TlsConfig::default_acme_directory")]
    pub acme_directory: String,
    /// ACME challenge type: "dns-01" or "http-01"
    #[serde(default = "TlsConfig::default_challenge_type")]
    pub challenge_type: String,
    /// Email for ACME registration
    #[serde(default)]
    pub email: Option<String>,
    /// DNS provider for DNS-01 challenge (e.g., "cloudflare", "route53", "manual")
    #[serde(default)]
    pub dns_provider: Option<String>,
    /// DNS provider credentials (JSON string or path to file)
    #[serde(default)]
    pub dns_credentials: Option<String>,
    /// Certificate storage directory
    #[serde(default = "TlsConfig::default_cert_dir")]
    pub cert_dir: String,
    /// Use Let's Encrypt staging (for testing)
    #[serde(default)]
    pub use_staging: bool,
}

impl Default for TlsConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            domains: Vec::new(),
            acme_directory: Self::default_acme_directory(),
            challenge_type: Self::default_challenge_type(),
            email: None,
            dns_provider: None,
            dns_credentials: None,
            cert_dir: Self::default_cert_dir(),
            use_staging: false,
        }
    }
}

impl TlsConfig {
    fn default_acme_directory() -> String {
        "https://acme-v02.api.letsencrypt.org/directory".to_string()
    }

    fn default_challenge_type() -> String {
        "http-01".to_string()
    }

    fn default_cert_dir() -> String {
        "./certs".to_string()
    }
}

impl McpServerConfig {
    fn default_bind_addr() -> String {
        "0.0.0.0:9000".to_string()
    }

    fn default_backend_command() -> String {
        String::new() // Must be explicitly configured
    }

    fn default_max_clients() -> usize {
        100
    }

    fn default_connection_timeout() -> u64 {
        300 // 5 minutes
    }

    fn default_auto_restart() -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = AppConfig::default();
        assert_eq!(config.llm.backend, "cpu");
        assert_eq!(config.llm.temperature, 0.7);
        assert_eq!(config.llm.max_tokens, 1000);
    }

    #[test]
    fn test_cache_dir_expansion() {
        let mut config = AppConfig::default();
        config.model.cache_dir = "~/test/cache".to_string();

        let expanded = config.cache_dir().unwrap();
        assert!(expanded.to_string_lossy().contains("test/cache"));
        assert!(!expanded.to_string_lossy().contains("~"));
    }

    #[test]
    fn test_database_path_expansion() {
        let mut config = AppConfig::default();
        config.llm.database_path = "~/assist/db.sqlite".to_string();

        let expanded = config.database_path().unwrap();
        assert!(expanded.to_string_lossy().contains("assist/db.sqlite"));
        assert!(!expanded.to_string_lossy().contains('~'));
    }
}
