//! LLM Responses panel - displays streaming responses from TUI's LLM

use ratatui::{
    layout::Rect,
    style::Style,
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph, Wrap},
    Frame,
};

use crate::colors;

#[derive(Debug, Clone)]
pub struct LlmResponse {
    pub query: String,
    pub response: String,
    pub is_streaming: bool,
    pub timestamp: chrono::DateTime<chrono::Utc>,
}

pub struct LlmResponsesPanel {
    state: ListState,
    responses: Vec<LlmResponse>,
}

impl LlmResponsesPanel {
    pub fn new() -> Self {
        let mut state = ListState::default();
        state.select(Some(0));
        Self {
            state,
            responses: Vec::new(),
        }
    }

    pub fn add_response(&mut self, response: LlmResponse) {
        self.responses.push(response);
        // Auto-scroll to latest
        if !self.responses.is_empty() {
            self.state.select(Some(self.responses.len() - 1));
        }
    }

    pub fn update_streaming_response(&mut self, partial_response: String) {
        if let Some(last) = self.responses.last_mut() {
            if last.is_streaming {
                last.response = partial_response;
            }
        }
    }

    pub fn complete_streaming_response(&mut self) {
        if let Some(last) = self.responses.last_mut() {
            last.is_streaming = false;
        }
    }

    pub fn len(&self) -> usize {
        self.responses.len()
    }

    pub fn next(&mut self) {
        let len = self.responses.len();
        if len == 0 {
            self.state.select(None);
            return;
        }
        let idx = self.state.selected().unwrap_or(0);
        let next = if idx + 1 >= len { 0 } else { idx + 1 };
        self.state.select(Some(next));
    }

    pub fn previous(&mut self) {
        if self.responses.is_empty() {
            self.state.select(None);
            return;
        }
        let idx = self.state.selected().unwrap_or(0);
        let prev = if idx == 0 {
            self.responses.len() - 1
        } else {
            idx - 1
        };
        self.state.select(Some(prev));
    }

    pub fn render(&mut self, frame: &mut Frame, area: Rect, focused: bool) {
        let list_items: Vec<ListItem> = self
            .responses
            .iter()
            .map(|resp| render_response_item(resp))
            .collect();

        let mut block = Block::default()
            .title("LLM Responses")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(colors::BORDER))
            .style(Style::default().bg(colors::BACKGROUND));

        if focused {
            block = block
                .border_style(Style::default().fg(colors::BORDER_FOCUSED))
                .title_style(Style::default().fg(colors::ACCENT_BRIGHT));
        }

        let mut state = self.state.clone();
        if let Some(selected) = state.selected() {
            let max = list_items.len().saturating_sub(1);
            state.select(Some(selected.min(max)));
        }

        if list_items.is_empty() {
            // Show helpful message when no responses yet
            let help_text = vec![
                Line::from(vec![Span::styled(
                    "No LLM responses yet.",
                    Style::default().fg(colors::TEXT_DIM),
                )]),
                Line::from(vec![Span::styled(
                    "Type a query and press Enter to chat with the LLM.",
                    Style::default().fg(colors::TEXT_DIM),
                )]),
            ];
            let paragraph = Paragraph::new(help_text)
                .block(block)
                .wrap(Wrap { trim: true });
            frame.render_widget(paragraph, area);
        } else {
            frame.render_stateful_widget(List::new(list_items).block(block), area, &mut state);
        }

        self.state = state;
    }
}

fn render_response_item(resp: &LlmResponse) -> ListItem<'static> {
    let timestamp = resp.timestamp.format("%H:%M:%S").to_string();
    let streaming_indicator = if resp.is_streaming { " ⋯" } else { "" };

    // Truncate long responses for list view (limit to 100 chars to reduce cloning overhead)
    let response_preview: String = if resp.response.len() > 100 {
        format!("{}...", &resp.response[..100])
    } else {
        resp.response.clone()
    };

    // Clone query string (typically short)
    let query = resp.query.clone();

    let content = vec![
        Line::from(vec![
            Span::styled(
                format!("[{}] ", timestamp),
                Style::default().fg(colors::TEXT_DIM),
            ),
            Span::styled(query, Style::default().fg(colors::ACCENT_BRIGHT)),
            Span::styled(
                streaming_indicator.to_string(),
                Style::default().fg(colors::STATUS_INFO),
            ),
        ]),
        Line::from(vec![
            Span::styled("  ".to_string(), Style::default()),
            Span::styled(
                response_preview,
                Style::default().fg(colors::TEXT_SECONDARY),
            ),
        ]),
    ];

    ListItem::new(content)
}
