use std::collections::HashMap;

use ratatui::{
    layout::Rect,
    style::Style,
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState},
    Frame,
};

use crate::colors;
use crate::components::Client;

pub struct ClientsPanel {
    state: ListState,
}

impl ClientsPanel {
    pub fn new() -> Self {
        let mut state = ListState::default();
        state.select(Some(0));
        Self { state }
    }

    pub fn next(&mut self, len: usize) {
        if len == 0 {
            self.state.select(None);
            return;
        }
        let idx = self.state.selected().unwrap_or(0);
        let next = if idx + 1 >= len { 0 } else { idx + 1 };
        self.state.select(Some(next));
    }

    pub fn previous(&mut self, len: usize) {
        if len == 0 {
            self.state.select(None);
            return;
        }
        let idx = self.state.selected().unwrap_or(0);
        let prev = if idx == 0 { len - 1 } else { idx - 1 };
        self.state.select(Some(prev));
    }

    pub fn render(
        &mut self,
        frame: &mut Frame,
        area: Rect,
        clients: &HashMap<String, Client>,
        focused: bool,
    ) {
        let mut items: Vec<&Client> = clients.values().collect();
        items.sort_by(|a, b| a.name.cmp(&b.name));

        let list_items: Vec<ListItem> = items.iter().map(|client| render_item(client)).collect();
        let mut block = Block::default()
            .title("Clients")
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
        frame.render_stateful_widget(List::new(list_items).block(block), area, &mut state);
        self.state = state;
    }
}

fn render_item(client: &Client) -> ListItem<'static> {
    let status = client.status.label();
    let status_style = client.status.style();
    let content = vec![
        Line::from(vec![
            Span::styled(
                client.name.clone(),
                Style::default().fg(colors::TEXT_PRIMARY),
            ),
            Span::raw(" "),
            Span::styled(format!("[{}]", status), status_style),
        ]),
        Line::from(vec![
            Span::styled("  ", Style::default()),
            Span::styled(
                client.description.clone(),
                Style::default().fg(colors::TEXT_DIM),
            ),
        ]),
    ];
    ListItem::new(content)
}
