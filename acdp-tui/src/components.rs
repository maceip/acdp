use crate::colors;
use chrono::{DateTime, Utc};
use ratatui::style::Style;

pub use crate::activity_feed::ActivityFeed;
pub use crate::clients_panel::ClientsPanel;
pub use crate::diagnostics::{DiagnosticsData, DiagnosticsPanel};
pub use crate::query_input::QueryInput;
pub use crate::quick_access::{QuickAccess, QuickAction};
pub use crate::semantic_status::{SemanticPrediction, SemanticStatusBar};
pub use crate::servers_panel::ServersPanel;
pub use crate::settings_panel::SettingsPanel;
pub use crate::status_bar::StatusBarData;

/// Identifies which widget currently owns input focus.
/// Simplified to just tabs and overlays for cleaner UX.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FocusArea {
    /// Main tabbed area (Activity, Servers, Settings)
    MainTabs,
    /// Query input field at bottom
    QueryInput,
    /// Overlay: Settings panel (full screen)
    Settings,
    /// Overlay: Diagnostics panel (full screen)
    Diagnostics,
    /// Overlay: Search (over activity feed)
    Search,
}

/// Connection status for a client.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClientStatus {
    Connected,
    Disconnected,
    Error,
}

impl ClientStatus {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Connected => "Connected",
            Self::Disconnected => "Disconnected",
            Self::Error => "Error",
        }
    }

    pub fn style(&self) -> Style {
        match self {
            Self::Connected => Style::default().fg(colors::STATUS_SUCCESS),
            Self::Disconnected => Style::default().fg(colors::TEXT_DIM),
            Self::Error => Style::default().fg(colors::STATUS_ERROR),
        }
    }
}

/// Type of server.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ServerType {
    Proxy,
    Gateway,
    Server,
}

/// Status for a server.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ServerStatus {
    Starting,
    Running,
    Degraded,
    Stopped,
    Error,
}

impl ServerStatus {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Starting => "Starting",
            Self::Running => "Running",
            Self::Degraded => "Degraded",
            Self::Stopped => "Stopped",
            Self::Error => "Error",
        }
    }

    pub fn style(&self) -> Style {
        match self {
            Self::Starting => Style::default().fg(colors::STATUS_WARNING),
            Self::Running => Style::default().fg(colors::STATUS_SUCCESS),
            Self::Degraded => Style::default().fg(colors::HONEY),
            Self::Stopped => Style::default().fg(colors::TEXT_DIM),
            Self::Error => Style::default().fg(colors::STATUS_ERROR),
        }
    }
}

/// Activity execution status.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ActivityStatus {
    Processing,
    Success,
    Failed,
}

impl ActivityStatus {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Processing => "Processing",
            Self::Success => "Success",
            Self::Failed => "Failed",
        }
    }

    pub fn style(&self) -> Style {
        match self {
            Self::Processing => Style::default().fg(colors::STATUS_INFO),
            Self::Success => Style::default().fg(colors::STATUS_SUCCESS),
            Self::Failed => Style::default().fg(colors::STATUS_ERROR),
        }
    }
}

/// Domain model for a known client.
#[derive(Debug, Clone)]
pub struct Client {
    pub id: String,
    pub name: String,
    pub description: String,
    pub status: ClientStatus,
    pub requests_sent: u64,
    pub last_activity: DateTime<Utc>,
}

impl Client {
    pub fn new(
        id: impl Into<String>,
        name: impl Into<String>,
        description: impl Into<String>,
        status: ClientStatus,
    ) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            description: description.into(),
            status,
            requests_sent: 0,
            last_activity: Utc::now(),
        }
    }
}

/// Domain model for a known MCP server.
#[derive(Debug, Clone)]
pub struct Server {
    pub id: String,
    pub name: String,
    pub description: String,
    pub server_type: ServerType,
    pub status: ServerStatus,
    pub requests_received: u64,
    pub last_activity: DateTime<Utc>,
    /// For proxies: current routing mode (bypass/semantic/hybrid)
    pub routing_mode: Option<String>,
    /// For proxies: model status (NotLoaded/Loading/Ready/Error)
    pub model_status: Option<String>,
    /// For proxies: target address (HTTP-SSE URL or command)
    pub target_address: Option<String>,
}

impl Server {
    pub fn new(
        id: impl Into<String>,
        name: impl Into<String>,
        description: impl Into<String>,
        status: ServerStatus,
    ) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            description: description.into(),
            server_type: ServerType::Server, // Default to Server
            status,
            requests_received: 0,
            last_activity: Utc::now(),
            routing_mode: None,
            model_status: None,
            target_address: None,
        }
    }
}

/// Metrics for an activity (mainly LLM queries)
#[derive(Debug, Clone)]
pub struct ActivityMetrics {
    pub ttft_ms: Option<f64>,
    pub tokens_per_sec: Option<f64>,
    pub total_tokens: Option<u64>,
    /// Total interceptor delay in milliseconds (includes LLM reasoning, routing decisions, etc.)
    pub interceptor_delay_ms: Option<f64>,
}

/// Item rendered in the activity feed.
#[derive(Debug, Clone)]
pub struct ActivityItem {
    pub timestamp: DateTime<Utc>,
    pub client: String,
    pub server: String,
    pub action: String,
    pub status: ActivityStatus,
    /// Optional metrics for LLM queries (ttft in milliseconds, tokens/sec, total tokens)
    pub metrics: Option<ActivityMetrics>,
}
