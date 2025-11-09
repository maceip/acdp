use ratatui::{
    layout::Rect,
    style::Style,
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState},
    Frame,
};

use crate::colors;
use crate::components::ActivityItem;

pub struct ActivityFeed {
    state: ListState,
}

impl ActivityFeed {
    pub fn new() -> Self {
        let mut state = ListState::default();
        state.select(Some(0));
        Self { state }
    }

    pub fn focus(&mut self) {
        if self.state.selected().is_none() {
            self.state.select(Some(0));
        }
    }

    pub fn next(&mut self, len: usize) {
        let idx = self.state.selected().unwrap_or(0);
        if len == 0 {
            self.state.select(None);
            return;
        }
        let next = if idx + 1 >= len { len - 1 } else { idx + 1 };
        self.state.select(Some(next));
    }

    pub fn previous(&mut self) {
        let idx = self.state.selected().unwrap_or(0);
        self.state.select(Some(idx.saturating_sub(1)));
    }

    pub fn render(
        &mut self,
        frame: &mut Frame,
        area: Rect,
        activities: &[ActivityItem],
        focused: bool,
    ) {
        let mut block = Block::default()
            .title("Activity Feed")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(colors::BORDER))
            .style(Style::default().bg(colors::BACKGROUND));

        if focused {
            block = block
                .border_style(Style::default().fg(colors::BORDER_FOCUSED))
                .title_style(Style::default().fg(colors::ACCENT_BRIGHT));
        }

        let items: Vec<ListItem> = activities
            .iter()
            .rev()
            .map(|activity| list_item(activity))
            .collect();

        let mut state = self.state.clone();
        // Ensure selection stays inside bounds after updates.
        if let Some(idx) = state.selected() {
            let max_index = items.len().saturating_sub(1);
            state.select(Some(idx.min(max_index)));
        }

        frame.render_stateful_widget(List::new(items).block(block), area, &mut state);
        self.state = state;
    }
}

fn list_item(item: &ActivityItem) -> ListItem<'static> {
    use ratatui::style::Color;

    let status_style = item.status.style();
    let timestamp = item.timestamp.format("%H:%M:%S");

    // Shorten proxy IDs: "proxy-f8633494-471c-4f0a-93e5-b5363ae90d93" → "proxy-f86"
    let client = shorten_proxy_id(&item.client);
    let server = shorten_proxy_id(&item.server);

    let mut lines = vec![Line::from(vec![
        Span::styled(
            format!("[{}] ", timestamp),
            Style::default().fg(colors::TEXT_DIM),
        ),
        Span::styled(
            format!("{} → {} ", client, server),
            Style::default().fg(colors::TEXT_SECONDARY),
        ),
        Span::styled(
            item.action.clone(),
            Style::default().fg(colors::TEXT_PRIMARY),
        ),
        Span::raw(" "),
        Span::styled(format!("[{}]", item.status.label()), status_style),
    ])];

    // Add metrics line if available
    if let Some(ref metrics) = item.metrics {
        let mut metric_spans = vec![Span::raw("  ├─ ")];
        let mut has_metric = false;

        if let Some(ttft) = metrics.ttft_ms {
            metric_spans.push(Span::styled(
                format!("TTFT: {:.0}ms", ttft),
                Style::default().fg(Color::Cyan),
            ));
            has_metric = true;
        }

        if let Some(tps) = metrics.tokens_per_sec {
            if has_metric {
                metric_spans.push(Span::raw(" | "));
            }
            metric_spans.push(Span::styled(
                format!("{:.1} tok/s", tps),
                Style::default().fg(Color::Green),
            ));
            has_metric = true;
        }

        if let Some(tokens) = metrics.total_tokens {
            if has_metric {
                metric_spans.push(Span::raw(" | "));
            }
            metric_spans.push(Span::styled(
                format!("{} tokens", tokens),
                Style::default().fg(Color::Yellow),
            ));
            has_metric = true;
        }

        if let Some(delay) = metrics.interceptor_delay_ms {
            if has_metric {
                metric_spans.push(Span::raw(" | "));
            }
            metric_spans.push(Span::styled(
                format!("routing: {:.0}ms", delay),
                Style::default().fg(Color::Magenta),
            ));
        }

        lines.push(Line::from(metric_spans));
    }

    ListItem::new(lines)
}

/// Shorten proxy IDs from "proxy-f8633494-471c-4f0a-93e5-b5363ae90d93" to "proxy-f86"
fn shorten_proxy_id(id: &str) -> String {
    if id.starts_with("proxy-") && id.len() > 15 {
        // Extract first 3 chars after "proxy-"
        let prefix = &id[0..9]; // "proxy-" + first 3 chars of UUID
        prefix.to_string()
    } else {
        id.to_string()
    }
}
