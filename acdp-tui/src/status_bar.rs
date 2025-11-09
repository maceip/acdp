//! Status bar component for displaying system metrics horizontally

use crate::colors;
use acdp_llm::ModelStatus;
use ratatui::{
    layout::Rect,
    style::Style,
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
    Frame,
};

/// Status bar data
#[derive(Debug, Clone, Default)]
pub struct StatusBarData {
    pub tui_model_status: Option<ModelStatus>, // TUI's model status
    pub proxy_model_status: Option<String>,    // Proxy's model status as string (from IPC)
    pub routing_mode: Option<String>,
    pub ttft: Option<f64>,
    pub tokens_per_sec: Option<f64>,
    pub dspy_accuracy: Option<f64>,
    pub session_accuracy: Option<f64>,
    pub session_predictions: Option<(u64, u64)>, // (successful, total)
    pub mcp_server_health: Option<crate::acdp_server::ServerHealth>, // MCP server health
    pub mcp_server_connections: Option<usize>,   // Active connections
}

/// Status bar component (horizontal metrics display)
pub struct StatusBar;

impl StatusBar {
    pub fn render(frame: &mut Frame, area: Rect, data: &StatusBarData) {
        let mut spans = Vec::new();

        // === TUI SECTION ===
        spans.push(Span::styled(
            "TUI Model: ",
            Style::default().fg(colors::TEXT_DIM),
        ));

        if let Some(ref status) = data.tui_model_status {
            let (label, color) = match status {
                ModelStatus::NotLoaded => ("NotLoaded", colors::TEXT_DIM),
                ModelStatus::Loading => ("Loading", colors::STATUS_WARNING),
                ModelStatus::Ready => ("Ready", colors::STATUS_SUCCESS),
                ModelStatus::Error(_) => ("Error", colors::STATUS_ERROR),
            };
            spans.push(Span::styled(label, Style::default().fg(color)));
        } else {
            spans.push(Span::styled(
                "Unknown",
                Style::default().fg(colors::TEXT_DIM),
            ));
        }

        // TUI tokens/sec
        if let Some(tps) = data.tokens_per_sec {
            spans.push(Span::raw(" │ "));
            spans.push(Span::styled("TPS: ", Style::default().fg(colors::TEXT_DIM)));
            spans.push(Span::styled(
                format!("{:.1}", tps),
                Style::default().fg(colors::TEXT_SECONDARY),
            ));
        }

        // Separator between TUI and Proxy sections
        spans.push(Span::styled(" ◆ ", Style::default().fg(colors::BORDER)));

        // === PROXY SECTION ===
        spans.push(Span::styled(
            "Proxy Model: ",
            Style::default().fg(colors::TEXT_DIM),
        ));

        if let Some(ref status_str) = data.proxy_model_status {
            // Parse status string and determine color
            let (label, color) = if status_str.contains("NotLoaded") {
                ("NotLoaded", colors::TEXT_DIM)
            } else if status_str.contains("Loading") {
                ("Loading", colors::STATUS_WARNING)
            } else if status_str.contains("Ready") {
                ("Ready", colors::STATUS_SUCCESS)
            } else if status_str.contains("Error") {
                (status_str.as_str(), colors::STATUS_ERROR)
            } else {
                (status_str.as_str(), colors::TEXT_SECONDARY)
            };
            spans.push(Span::styled(label, Style::default().fg(color)));
        } else {
            spans.push(Span::styled("N/A", Style::default().fg(colors::TEXT_DIM)));
        }

        // Routing mode
        spans.push(Span::raw(" │ "));
        if let Some(ref mode) = data.routing_mode {
            let mode_icon = match mode.as_str() {
                "bypass" => "🔓",
                "semantic" => "🧠",
                "hybrid" => "⚡",
                _ => "",
            };
            spans.push(Span::styled(
                format!("{} {}", mode_icon, mode),
                Style::default().fg(colors::ACCENT_BRIGHT),
            ));
        } else {
            spans.push(Span::styled(
                "Mode: N/A",
                Style::default().fg(colors::TEXT_DIM),
            ));
        }

        // TTFT
        if let Some(ttft) = data.ttft {
            if ttft < 10.0 {
                spans.push(Span::raw(" │ "));
                spans.push(Span::styled(
                    "TTFT: ",
                    Style::default().fg(colors::TEXT_DIM),
                ));
                spans.push(Span::styled(
                    format!("{:.0}ms", ttft * 1000.0),
                    Style::default().fg(colors::TEXT_SECONDARY),
                ));
            }
        }

        // Session accuracy
        if let Some(accuracy) = data.session_accuracy {
            let color = if accuracy >= 0.8 {
                colors::STATUS_SUCCESS
            } else if accuracy >= 0.5 {
                colors::STATUS_WARNING
            } else {
                colors::STATUS_ERROR
            };
            spans.push(Span::raw(" │ "));
            spans.push(Span::styled("Acc: ", Style::default().fg(colors::TEXT_DIM)));
            spans.push(Span::styled(
                format!("{:.0}%", accuracy * 100.0),
                Style::default().fg(color),
            ));
        }

        // Session predictions
        if let Some((success, total)) = data.session_predictions {
            if total > 0 {
                spans.push(Span::raw(" │ "));
                spans.push(Span::styled(
                    "Pred: ",
                    Style::default().fg(colors::TEXT_DIM),
                ));
                spans.push(Span::styled(
                    format!("{}/{}", success, total),
                    Style::default().fg(colors::TEXT_SECONDARY),
                ));
            }
        }

        // MCP Server health (if running in server mode)
        if let Some(health) = data.mcp_server_health {
            spans.push(Span::raw(" │ "));
            spans.push(Span::styled(
                "MCP Server: ",
                Style::default().fg(colors::TEXT_DIM),
            ));
            let (label, color) = match health {
                crate::acdp_server::ServerHealth::Healthy => ("✓ Healthy", colors::STATUS_SUCCESS),
                crate::acdp_server::ServerHealth::Degraded => ("⚠ Degraded", colors::STATUS_WARNING),
                crate::acdp_server::ServerHealth::Unhealthy => ("✗ Unhealthy", colors::STATUS_ERROR),
            };
            spans.push(Span::styled(label, Style::default().fg(color)));

            if let Some(conns) = data.mcp_server_connections {
                spans.push(Span::raw(" "));
                spans.push(Span::styled(
                    format!("({} conns)", conns),
                    Style::default().fg(colors::TEXT_DIM),
                ));
            }
        }

        // Default if no data (loading state)
        if spans.is_empty() {
            spans.push(Span::styled(
                "Loading...",
                Style::default().fg(colors::TEXT_DIM),
            ));
        }

        // Add keyboard shortcuts help on the right side
        spans.push(Span::raw("  "));
        spans.push(Span::styled(
            "[Ctrl+N: New Proxy] [Ctrl+P: Cycle Proxy] [Esc: Quit]",
            Style::default().fg(colors::TEXT_DIM),
        ));

        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(colors::BORDER))
            .style(Style::default().bg(colors::BACKGROUND));

        let line = Line::from(spans);
        let paragraph = Paragraph::new(line).block(block);
        frame.render_widget(paragraph, area);
    }
}
