use std::collections::HashMap;

use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::Style,
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState},
    Frame,
};

use crate::colors;
use crate::components::{Client, Server};

pub struct ServersPanel {
    state: ListState,
}

impl ServersPanel {
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
        servers: &HashMap<String, Server>,
        clients: &HashMap<String, Client>,
        focused: bool,
        selected_proxy_id: Option<&String>,
    ) {
        // Split area vertically: Servers (60%) | Clients (40%)
        let chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(60), Constraint::Percentage(40)])
            .split(area);

        // Render servers list
        let mut server_items: Vec<&Server> = servers.values().collect();
        server_items.sort_by(|a, b| a.name.cmp(&b.name));

        let rendered: Vec<ListItem> = server_items
            .iter()
            .map(|server| render_item(server, selected_proxy_id))
            .collect();
        let mut block = Block::default()
            .title("Servers")
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
            let max = rendered.len().saturating_sub(1);
            state.select(Some(selected.min(max)));
        }
        frame.render_stateful_widget(List::new(rendered).block(block), chunks[0], &mut state);
        self.state = state;

        // Render clients list
        let mut client_items: Vec<&Client> = clients.values().collect();
        client_items.sort_by(|a, b| a.name.cmp(&b.name));

        let client_rendered: Vec<ListItem> = client_items
            .iter()
            .map(|client| render_client_item(client))
            .collect();
        let client_block = Block::default()
            .title("Clients")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(colors::BORDER))
            .style(Style::default().bg(colors::BACKGROUND));

        frame.render_widget(List::new(client_rendered).block(client_block), chunks[1]);
    }
}

fn render_item(server: &Server, selected_proxy_id: Option<&String>) -> ListItem<'static> {
    use crate::components::ServerType;

    let is_selected = selected_proxy_id.map_or(false, |id| id == &server.id);
    let selection_marker = if is_selected { "► " } else { "" };

    let mut first_line = vec![
        Span::styled(
            format!("{}{}", selection_marker, server.name),
            Style::default().fg(colors::TEXT_PRIMARY),
        ),
        Span::raw(" "),
        Span::styled(
            format!("[{}]", server.status.label()),
            server.status.style(),
        ),
    ];

    // Add routing mode for proxies
    if matches!(server.server_type, ServerType::Proxy) {
        if let Some(mode) = &server.routing_mode {
            first_line.push(Span::raw(" "));
            first_line.push(Span::styled(
                format!("({})", mode),
                Style::default().fg(colors::HONEY),
            ));
        }
    }

    let mut content = vec![
        Line::from(first_line),
        Line::from(vec![
            Span::styled("  ", Style::default()),
            Span::styled(
                server.description.clone(),
                Style::default().fg(colors::TEXT_DIM),
            ),
        ]),
    ];

    // Add target address line for HTTP-SSE proxies
    if matches!(server.server_type, ServerType::Proxy) {
        if let Some(addr) = &server.target_address {
            // Only show address if it looks like an HTTP URL (not a command)
            if addr.starts_with("http://") || addr.starts_with("https://") {
                content.push(Line::from(vec![
                    Span::styled("  addr: ", Style::default().fg(colors::TEXT_DIM)),
                    Span::styled(addr.clone(), Style::default().fg(colors::HONEY)),
                ]));
            }
        }
    }

    ListItem::new(content)
}

fn render_client_item(client: &Client) -> ListItem<'static> {
    // Show stats if client has sent requests
    let stats_line = if client.requests_sent > 0 {
        Some(Line::from(vec![
            Span::styled("  ", Style::default()),
            Span::styled(
                format!(
                    "Requests: {} | Last: {}",
                    client.requests_sent,
                    client.last_activity.format("%H:%M:%S")
                ),
                Style::default().fg(colors::TEXT_DIM),
            ),
        ]))
    } else {
        None
    };
    let status = client.status.label();
    let status_style = client.status.style();
    let mut content = vec![
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

    // Add stats line if available
    if let Some(stats) = stats_line {
        content.push(stats);
    }

    ListItem::new(content)
}
