//! Diagnostics panel for displaying model and system metrics

use crate::colors;
use acdp_llm::{DownloadProgress, ModelStatus};
use ratatui::{
    layout::Rect,
    style::Style,
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
    Frame,
};

/// Diagnostics data structure
#[derive(Debug, Clone)]
pub struct DiagnosticsData {
    /// Model serving status
    pub model_status: ModelStatus,
    /// Current model name
    pub model_name: Option<String>,
    /// Download progress if downloading
    pub download_progress: Option<DownloadProgress>,
    /// Time to first token (seconds)
    pub ttft: Option<f64>,
    /// Tokens per second
    pub tokens_per_sec: Option<f64>,
    /// GEPA optimization percentage
    pub gepa_optimization: Option<f64>,
    /// DSPy prediction accuracy
    pub dspy_accuracy: Option<f64>,
    /// Last query sent
    pub last_query: Option<String>,
    /// Last response received
    pub last_response: Option<String>,
    /// Active routing mode
    pub routing_mode: Option<String>,
    /// Session-specific accuracy value
    pub session_accuracy: Option<f64>,
    /// Session prediction counts
    pub session_total_predictions: Option<u64>,
    pub session_successful_predictions: Option<u64>,
    /// Interceptor statistics
    pub interceptor_count: Option<u64>,
    pub interceptor_modifications: Option<u64>,
    pub interceptor_blocks: Option<u64>,
    /// Gateway state
    pub gateway_active_proxies: Option<u64>,
    pub gateway_total_clients: Option<u64>,
    pub gateway_total_servers: Option<u64>,
    pub gateway_uptime_seconds: Option<u64>,
    /// Gateway metrics
    pub gateway_total_requests: Option<u64>,
    pub gateway_total_errors: Option<u64>,
    pub gateway_avg_response_time: Option<f64>,
    pub gateway_requests_per_sec: Option<f64>,
    /// MCP Server metrics (when running in server mode)
    pub mcp_server_metrics: Option<String>,
    /// MCP Server health status
    pub mcp_server_health: Option<crate::acdp_server::ServerHealth>,
    /// MCP Server active connections
    pub mcp_server_connections: Option<usize>,
    /// HTTP+SSE server bind address (if enabled)
    pub http_server_bind_addr: Option<String>,
}

/// Diagnostics panel component
pub struct DiagnosticsPanel {
    data: DiagnosticsData,
}

impl DiagnosticsPanel {
    pub fn new() -> Self {
        Self {
            data: DiagnosticsData::default(),
        }
    }

    /// Update diagnostics data
    pub fn update(&mut self, data: DiagnosticsData) {
        self.data = data;
    }

    /// Render the diagnostics panel
    pub fn render(&self, frame: &mut Frame, area: Rect, focused: bool) {
        let mut block = Block::default()
            .title("Diagnostics")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(colors::BORDER))
            .style(Style::default().bg(colors::BACKGROUND));

        if focused {
            block = block
                .border_style(Style::default().fg(colors::BORDER_FOCUSED))
                .title_style(Style::default().fg(colors::ACCENT_BRIGHT));
        }

        let mut lines = Vec::new();

        // Model Status
        let status_str = match &self.data.model_status {
            ModelStatus::NotLoaded => "Not Loaded",
            ModelStatus::Loading => "Loading...",
            ModelStatus::Ready => "Ready",
            ModelStatus::Error(e) => {
                return frame
                    .render_widget(Paragraph::new(format!("Error: {}", e)).block(block), area)
            }
        };
        let status_color = match &self.data.model_status {
            ModelStatus::Ready => colors::STATUS_SUCCESS,
            ModelStatus::Loading => colors::STATUS_WARNING,
            _ => colors::TEXT_DIM,
        };
        lines.push(Line::from(vec![
            Span::styled("Model: ", Style::default().fg(colors::TEXT_SECONDARY)),
            Span::styled(status_str, Style::default().fg(status_color)),
        ]));

        // Model name
        if let Some(ref name) = self.data.model_name {
            lines.push(Line::from(vec![
                Span::styled("Model Name: ", Style::default().fg(colors::TEXT_SECONDARY)),
                Span::styled(name, Style::default().fg(colors::ACCENT)),
            ]));
        }

        // Download progress
        if let Some(ref progress) = self.data.download_progress {
            lines.push(Line::from(vec![
                Span::styled("Download: ", Style::default().fg(colors::TEXT_SECONDARY)),
                Span::styled(
                    format!("{:.1}%", progress.percentage),
                    Style::default().fg(colors::STATUS_WARNING),
                ),
            ]));
            if let Some(total) = progress.total_bytes {
                let downloaded_mb = progress.bytes_downloaded as f64 / (1024.0 * 1024.0);
                let total_mb = total as f64 / (1024.0 * 1024.0);
                lines.push(Line::from(vec![Span::styled(
                    format!("  {:.1} MB / {:.1} MB", downloaded_mb, total_mb),
                    Style::default().fg(colors::TEXT_DIM),
                )]));
            }
        }

        // TTFT
        if let Some(ttft) = self.data.ttft {
            lines.push(Line::from(vec![
                Span::styled("TTFT: ", Style::default().fg(colors::TEXT_SECONDARY)),
                Span::styled(
                    format!("{:.3}s", ttft),
                    Style::default().fg(colors::STATUS_SUCCESS),
                ),
            ]));
        }

        // Tokens/sec
        if let Some(tps) = self.data.tokens_per_sec {
            lines.push(Line::from(vec![
                Span::styled("Tokens/sec: ", Style::default().fg(colors::TEXT_SECONDARY)),
                Span::styled(
                    format!("{:.1}", tps),
                    Style::default().fg(colors::STATUS_SUCCESS),
                ),
            ]));
        }

        // GEPA optimization
        if let Some(gepa) = self.data.gepa_optimization {
            lines.push(Line::from(vec![
                Span::styled(
                    "GEPA Optimization: ",
                    Style::default().fg(colors::TEXT_SECONDARY),
                ),
                Span::styled(
                    format!("{:.1}%", gepa * 100.0),
                    Style::default().fg(colors::ACCENT),
                ),
            ]));
        }

        // DSPy accuracy
        if let Some(accuracy) = self.data.dspy_accuracy {
            lines.push(Line::from(vec![
                Span::styled(
                    "DSPy Accuracy: ",
                    Style::default().fg(colors::TEXT_SECONDARY),
                ),
                Span::styled(
                    format!("{:.1}%", accuracy * 100.0),
                    Style::default().fg(colors::ACCENT),
                ),
            ]));
        }

        if let Some(session_accuracy) = self.data.session_accuracy {
            lines.push(Line::from(vec![
                Span::styled(
                    "Session Accuracy: ",
                    Style::default().fg(colors::TEXT_SECONDARY),
                ),
                Span::styled(
                    format!("{:.1}%", session_accuracy * 100.0),
                    Style::default().fg(colors::ACCENT_BRIGHT),
                ),
            ]));
        }

        if let (Some(total), Some(success)) = (
            self.data.session_total_predictions,
            self.data.session_successful_predictions,
        ) {
            lines.push(Line::from(vec![
                Span::styled(
                    "Session Predictions: ",
                    Style::default().fg(colors::TEXT_SECONDARY),
                ),
                Span::styled(
                    format!("{}/{}", success, total),
                    Style::default().fg(colors::ACCENT),
                ),
            ]));
        }

        if let Some(mode) = &self.data.routing_mode {
            lines.push(Line::from(vec![
                Span::styled(
                    "Routing Mode: ",
                    Style::default().fg(colors::TEXT_SECONDARY),
                ),
                Span::styled(mode.to_string(), Style::default().fg(colors::ACCENT_BRIGHT)),
            ]));
        }

        // MCP Server metrics
        if let Some(ref metrics) = self.data.mcp_server_metrics {
            lines.push(Line::from(vec![
                Span::styled("MCP Server: ", Style::default().fg(colors::TEXT_SECONDARY)),
                Span::styled(metrics.clone(), Style::default().fg(colors::ACCENT)),
            ]));

            // Add performance metrics if available
            if metrics.contains("Peak RPS") {
                lines.push(Line::from(vec![
                    Span::styled("  Performance: ", Style::default().fg(colors::TEXT_DIM)),
                    Span::styled(
                        metrics.split("Peak RPS").nth(1).unwrap_or(""),
                        Style::default().fg(colors::TEXT_SECONDARY),
                    ),
                ]));
            }
        }

        // HTTP+SSE Server status
        if let Some(ref addr) = self.data.http_server_bind_addr {
            lines.push(Line::from(vec![
                Span::styled("HTTP Server: ", Style::default().fg(colors::TEXT_SECONDARY)),
                Span::styled(
                    format!("Listening on {}", addr),
                    Style::default().fg(colors::STATUS_SUCCESS),
                ),
            ]));
        }

        // If no data, show placeholder
        if lines.is_empty() {
            lines.push(Line::from(Span::styled(
                "No diagnostics data available",
                Style::default().fg(colors::TEXT_DIM),
            )));
        }

        let paragraph = Paragraph::new(lines).block(block);
        frame.render_widget(paragraph, area);
    }
}

impl Default for DiagnosticsPanel {
    fn default() -> Self {
        Self::new()
    }
}

impl Default for DiagnosticsData {
    fn default() -> Self {
        Self {
            model_status: ModelStatus::NotLoaded,
            model_name: None,
            download_progress: None,
            ttft: None,
            tokens_per_sec: None,
            gepa_optimization: None,
            dspy_accuracy: None,
            last_query: None,
            last_response: None,
            routing_mode: None,
            session_accuracy: None,
            session_total_predictions: None,
            session_successful_predictions: None,
            interceptor_count: None,
            interceptor_modifications: None,
            interceptor_blocks: None,
            gateway_active_proxies: None,
            gateway_total_clients: None,
            gateway_total_servers: None,
            gateway_uptime_seconds: None,
            gateway_total_requests: None,
            gateway_total_errors: None,
            gateway_avg_response_time: None,
            gateway_requests_per_sec: None,
            mcp_server_metrics: None,
            mcp_server_health: None,
            mcp_server_connections: None,
            http_server_bind_addr: None,
        }
    }
}
