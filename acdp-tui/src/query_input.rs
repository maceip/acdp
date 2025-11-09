use ratatui::{
    layout::Rect,
    style::{Color, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
    Frame,
};

use crate::colors;

pub struct QueryInput {
    placeholder: String,
}

impl QueryInput {
    pub fn new() -> Self {
        Self {
            placeholder: "Enter query (prefix @proxy or @gateway to route, default: local LLM)…"
                .to_string(),
        }
    }

    pub fn render(&self, frame: &mut Frame, area: Rect, value: &str, focused: bool) {
        let block = if focused {
            // Vibrant cyan/magenta border when focused
            Block::default()
                .title("Query")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Rgb(48, 174, 209)))
                .title_style(Style::default().fg(Color::Rgb(225, 22, 247)))
                .style(Style::default().bg(colors::BACKGROUND))
        } else {
            // Regular block when not focused
            Block::default()
                .title("Query")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(colors::BORDER))
                .style(Style::default().bg(colors::BACKGROUND))
        };

        let display = if value.is_empty() {
            Line::from(Span::styled(
                &self.placeholder,
                Style::default().fg(colors::TEXT_DIM),
            ))
        } else {
            Line::from(Span::styled(
                value.to_string(),
                Style::default().fg(colors::TEXT_PRIMARY),
            ))
        };

        let paragraph = Paragraph::new(display).block(block);
        frame.render_widget(paragraph, area);
    }
}
