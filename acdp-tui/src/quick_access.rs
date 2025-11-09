use ratatui::{
    layout::Rect,
    style::Style,
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph, Wrap},
    Frame,
};

use crate::colors;

#[derive(Clone, Debug)]
pub struct QuickAction {
    pub label: String,
    pub description: String,
    pub command: String,
}

pub struct QuickAccess {
    items: Vec<QuickAction>,
    state: ListState,
    expanded: bool,
}

impl QuickAccess {
    pub fn new() -> Self {
        let mut state = ListState::default();
        state.select(Some(0));
        Self {
            items: default_actions(),
            state,
            expanded: false,
        }
    }

    /// Update available actions based on proxy count
    pub fn update_actions(&mut self, has_proxies: bool) {
        let new_items = if has_proxies {
            default_actions()
        } else {
            default_actions_with_start_proxy()
        };

        if self.items.len() != new_items.len() {
            let current_idx = self.state.selected().unwrap_or(0);
            self.items = new_items;
            if current_idx >= self.items.len() {
                self.state.select(Some(0));
            }
        }
    }

    pub fn focus(&mut self) {
        if self.state.selected().is_none() && self.expanded {
            self.state.select(Some(0));
        }
    }

    pub fn toggle_expanded(&mut self) {
        self.expanded = !self.expanded;
        if !self.expanded {
            self.state.select(None);
        } else if self.state.selected().is_none() && !self.items.is_empty() {
            self.state.select(Some(0));
        }
    }

    pub fn is_expanded(&self) -> bool {
        self.expanded
    }

    pub fn drawer_height(&self) -> u16 {
        if self.expanded {
            8
        } else {
            3
        }
    }

    pub fn next(&mut self) {
        if !self.expanded {
            return;
        }
        if self.items.is_empty() {
            self.state.select(None);
            return;
        }
        let idx = self.state.selected().unwrap_or(0);
        let next = if idx + 1 >= self.items.len() {
            0
        } else {
            idx + 1
        };
        self.state.select(Some(next));
    }

    pub fn previous(&mut self) {
        if !self.expanded {
            return;
        }
        if self.items.is_empty() {
            self.state.select(None);
            return;
        }
        let idx = self.state.selected().unwrap_or(0);
        let prev = if idx == 0 {
            self.items.len().saturating_sub(1)
        } else {
            idx - 1
        };
        self.state.select(Some(prev));
    }

    pub fn render(&mut self, frame: &mut Frame, area: Rect, focused: bool) {
        if self.expanded {
            self.render_expanded(frame, area, focused);
        } else {
            self.render_collapsed(frame, area, focused);
        }
    }

    fn render_expanded(&mut self, frame: &mut Frame, area: Rect, focused: bool) {
        let mut block = Block::default()
            .title("Quick Actions (press 'a' to collapse)")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(colors::BORDER))
            .style(Style::default().bg(colors::BACKGROUND));

        if focused {
            block = block
                .border_style(Style::default().fg(colors::BORDER_FOCUSED))
                .title_style(Style::default().fg(colors::ACCENT_BRIGHT));
        }

        let items: Vec<ListItem> = self.items.iter().map(render_item).collect();
        let mut state = self.state.clone();
        if let Some(idx) = state.selected() {
            let max = items.len().saturating_sub(1);
            state.select(Some(idx.min(max)));
        }

        let list = List::new(items)
            .block(block)
            .highlight_style(
                Style::default()
                    .fg(colors::ACCENT_BRIGHT)
                    .bg(colors::BORDER),
            )
            .highlight_symbol("▶ ");

        frame.render_stateful_widget(list, area, &mut state);
        self.state = state;
    }

    fn render_collapsed(&self, frame: &mut Frame, area: Rect, focused: bool) {
        let mut block = Block::default()
            .title("Quick Actions (press 'a' to expand)")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(colors::BORDER))
            .style(Style::default().bg(colors::BACKGROUND));

        if focused {
            block = block
                .border_style(Style::default().fg(colors::BORDER_FOCUSED))
                .title_style(Style::default().fg(colors::ACCENT_BRIGHT));
        }

        let mut spans = Vec::new();
        for (idx, action) in self.items.iter().take(3).enumerate() {
            if idx > 0 {
                spans.push(Span::raw("   "));
            }
            spans.push(Span::styled(
                format!("[{}] {}", idx + 1, action.label),
                Style::default().fg(colors::TEXT_PRIMARY),
            ));
        }
        if spans.is_empty() {
            spans.push(Span::styled(
                "No quick actions available",
                Style::default().fg(colors::TEXT_DIM),
            ));
        }
        spans.push(Span::raw("   "));
        spans.push(Span::styled(
            "Enter runs #1",
            Style::default().fg(colors::TEXT_DIM),
        ));

        let paragraph = Paragraph::new(Line::from(spans))
            .block(block)
            .wrap(Wrap { trim: true });

        frame.render_widget(paragraph, area);
    }

    pub fn execute_selected_action(&mut self) -> Option<String> {
        if !self.expanded {
            return None;
        }
        self.state
            .selected()
            .and_then(|idx| self.items.get(idx))
            .map(|item| item.command.clone())
    }

    pub fn execute_shortcut(&self, slot: usize) -> Option<String> {
        self.items.get(slot).map(|item| item.command.clone())
    }

    pub fn get_selected_action(&self) -> Option<&QuickAction> {
        self.state.selected().and_then(|idx| self.items.get(idx))
    }
}

fn render_item(action: &QuickAction) -> ListItem<'static> {
    let lines = vec![
        Line::from(Span::styled(
            action.label.clone(),
            Style::default().fg(colors::TEXT_PRIMARY),
        )),
        Line::from(vec![
            Span::raw("  "),
            Span::styled(
                action.description.clone(),
                Style::default().fg(colors::TEXT_DIM),
            ),
        ]),
    ];
    ListItem::new(lines)
}

fn default_actions() -> Vec<QuickAction> {
    vec![
        QuickAction {
            label: "Start Default Proxy".to_string(),
            description: "Launch new proxy with semantic routing (multi-server support)"
                .to_string(),
            command: "start_default_proxy".to_string(),
        },
        QuickAction {
            label: "Start Gemini Client".to_string(),
            description: "Launch Gemini MCP client in shell".to_string(),
            command: "start_gemini_client".to_string(),
        },
        QuickAction {
            label: "List available tools".to_string(),
            description: "Inspect which tools MCP exposes".to_string(),
            command: "list_tools".to_string(),
        },
        QuickAction {
            label: "Check server health".to_string(),
            description: "Gather latest health metrics".to_string(),
            command: "check_health".to_string(),
        },
        QuickAction {
            label: "Open session".to_string(),
            description: "Start a new interactive session".to_string(),
            command: "open_session".to_string(),
        },
        QuickAction {
            label: "Routing: Bypass".to_string(),
            description: "Force proxy to bypass LLM predictions".to_string(),
            command: "set_mode_bypass".to_string(),
        },
        QuickAction {
            label: "Routing: Semantic".to_string(),
            description: "Enable semantic-only routing via LiteRT".to_string(),
            command: "set_mode_semantic".to_string(),
        },
        QuickAction {
            label: "Routing: Hybrid".to_string(),
            description: "Combine rules with semantic fallback".to_string(),
            command: "set_mode_hybrid".to_string(),
        },
    ]
}

fn default_actions_with_start_proxy() -> Vec<QuickAction> {
    vec![
        QuickAction {
            label: "Start Default Proxy".to_string(),
            description: "Launch proxy with semantic routing enabled".to_string(),
            command: "start_default_proxy".to_string(),
        },
        QuickAction {
            label: "Start Gemini Client".to_string(),
            description: "Launch Gemini MCP client in shell".to_string(),
            command: "start_gemini_client".to_string(),
        },
        QuickAction {
            label: "List available tools".to_string(),
            description: "Inspect which tools MCP exposes".to_string(),
            command: "list_tools".to_string(),
        },
        QuickAction {
            label: "Check server health".to_string(),
            description: "Gather latest health metrics".to_string(),
            command: "check_health".to_string(),
        },
        QuickAction {
            label: "Open session".to_string(),
            description: "Start a new interactive session".to_string(),
            command: "open_session".to_string(),
        },
    ]
}
