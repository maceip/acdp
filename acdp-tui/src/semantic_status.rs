//! Semantic routing status bar (shows query → prediction)

use crate::colors;
use ratatui::{
    layout::{Alignment, Rect},
    style::Style,
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
    Frame,
};

/// Semantic routing prediction info
#[derive(Debug, Clone, Default)]
pub struct SemanticPrediction {
    pub query: Option<String>,
    pub predicted_tool: Option<String>,
    pub confidence: Option<f64>,
    pub actual_tool: Option<String>,
    pub success: Option<bool>,
}

/// Semantic status bar component
pub struct SemanticStatusBar;

impl SemanticStatusBar {
    pub fn render(frame: &mut Frame, area: Rect, prediction: &SemanticPrediction) {
        let mut left_spans = Vec::new();
        let mut right_spans = Vec::new();

        // Left side: Query (extract method and params from JSON)
        if let Some(ref query) = prediction.query {
            let (method, params_info) = extract_query_info(query);

            left_spans.push(Span::styled(
                "Request: ",
                Style::default().fg(colors::TEXT_DIM),
            ));
            left_spans.push(Span::styled(
                method,
                Style::default().fg(colors::TEXT_SECONDARY),
            ));

            if let Some(info) = params_info {
                left_spans.push(Span::styled(" ", Style::default()));
                left_spans.push(Span::styled(
                    format!("({})", info),
                    Style::default().fg(colors::TEXT_DIM),
                ));
            }
        }

        // Right side: LLM Prediction → Result
        if let Some(ref tool) = prediction.predicted_tool {
            right_spans.push(Span::styled(
                "LLM → ",
                Style::default().fg(colors::TEXT_DIM),
            ));
            right_spans.push(Span::styled(
                tool.clone(),
                Style::default().fg(colors::ACCENT),
            ));

            // Confidence with color coding
            if let Some(conf) = prediction.confidence {
                right_spans.push(Span::raw(" "));
                let conf_color = if conf >= 0.8 {
                    colors::STATUS_SUCCESS
                } else if conf >= 0.5 {
                    colors::STATUS_WARNING
                } else {
                    colors::STATUS_ERROR
                };
                right_spans.push(Span::styled(
                    format!("{:.0}%", conf * 100.0),
                    Style::default().fg(conf_color),
                ));
            }

            // Success/failure indicator with actual tool if different
            if let Some(success) = prediction.success {
                right_spans.push(Span::raw(" "));
                if success {
                    right_spans.push(Span::styled(
                        "✓",
                        Style::default().fg(colors::STATUS_SUCCESS),
                    ));
                } else {
                    right_spans.push(Span::styled("✗", Style::default().fg(colors::STATUS_ERROR)));
                    // Show what it actually was
                    if let Some(ref actual) = prediction.actual_tool {
                        right_spans.push(Span::styled(
                            format!(" (actual: {})", actual),
                            Style::default().fg(colors::STATUS_WARNING),
                        ));
                    }
                }
            }
        }

        // Build the line
        let mut all_spans = left_spans;
        if !right_spans.is_empty() {
            // Calculate padding
            let left_width: usize = all_spans.iter().map(|s| s.content.len()).sum();
            let right_width: usize = right_spans.iter().map(|s| s.content.len()).sum();
            let available_width = area.width.saturating_sub(4) as usize; // -4 for borders
            let padding = available_width.saturating_sub(left_width + right_width);

            if padding > 0 {
                all_spans.push(Span::raw(" ".repeat(padding)));
            }
            all_spans.extend(right_spans);
        }

        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(colors::ACCENT))
            .style(Style::default().bg(colors::BACKGROUND));

        let line = Line::from(all_spans);
        let paragraph = Paragraph::new(line).block(block).alignment(Alignment::Left);
        frame.render_widget(paragraph, area);
    }
}

fn extract_query_info(query_json: &str) -> (String, Option<String>) {
    // Try to parse as JSON to extract method and useful params
    if let Ok(json) = serde_json::from_str::<serde_json::Value>(query_json) {
        let method = json
            .get("method")
            .and_then(|m| m.as_str())
            .unwrap_or("unknown")
            .to_string();

        // Extract useful param info based on method
        let params_info = json.get("params").and_then(|params| {
            match method.as_str() {
                "tools/call" => {
                    // Show tool name
                    params
                        .get("name")
                        .and_then(|n| n.as_str())
                        .map(|s| format!("tool: {}", s))
                }
                "resources/read" => {
                    // Show resource URI
                    params
                        .get("uri")
                        .and_then(|u| u.as_str())
                        .map(|s| truncate_string(s, 25))
                }
                "prompts/get" => {
                    // Show prompt name
                    params
                        .get("name")
                        .and_then(|n| n.as_str())
                        .map(|s| format!("prompt: {}", s))
                }
                _ => None,
            }
        });

        (method, params_info)
    } else {
        // Fallback if not JSON
        (truncate_string(query_json, 30), None)
    }
}

fn truncate_string(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else {
        format!("{}...", &s[..max_len.saturating_sub(3)])
    }
}
