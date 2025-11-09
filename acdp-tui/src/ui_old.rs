use std::{borrow::Cow, collections::HashMap};

use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::Style,
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph, Wrap},
    Frame,
};

use crate::{
    colors,
    components::{
        ActivityFeed, ActivityItem, Client, ClientsPanel, DiagnosticsData, DiagnosticsPanel,
        FocusArea, QueryInput, QuickAccess, SemanticPrediction, SemanticStatusBar, Server,
        ServersPanel, SettingsPanel,
    },
    events::Event,
    llm_responses_panel::LlmResponsesPanel,
    status_bar::{StatusBar, StatusBarData},
};

const FOCUS_ORDER: [FocusArea; 7] = [
    FocusArea::Activity,
    FocusArea::LlmResponses,
    FocusArea::Servers,
    FocusArea::ClientsList,
    FocusArea::ActionDrawer,
    FocusArea::ModalToolbar,
    FocusArea::QueryInput,
];

pub struct NavigationContext {
    pub client_len: usize,
    pub server_len: usize,
    pub activity_len: usize,
}

pub struct UI {
    focus: FocusArea,
    settings_prev_focus: FocusArea,
    diagnostics_prev_focus: FocusArea,
    search_prev_focus: FocusArea,
    pub llm_responses_panel: LlmResponsesPanel,
    pub clients_panel: ClientsPanel,
    pub servers_panel: ServersPanel,
    pub activity_feed: ActivityFeed,
    pub query_input: QueryInput,
    pub quick_access: QuickAccess,
    pub diagnostics_panel: DiagnosticsPanel,
    pub settings_panel: SettingsPanel,
    modal_toolbar: ModalToolbar,
    search_overlay: Option<SearchOverlay>,
}

impl UI {
    pub fn new() -> Self {
        Self {
            focus: FocusArea::Activity,
            settings_prev_focus: FocusArea::Activity,
            diagnostics_prev_focus: FocusArea::Activity,
            search_prev_focus: FocusArea::Activity,
            llm_responses_panel: LlmResponsesPanel::new(),
            clients_panel: ClientsPanel::new(),
            servers_panel: ServersPanel::new(),
            activity_feed: ActivityFeed::new(),
            query_input: QueryInput::new(),
            quick_access: QuickAccess::new(),
            diagnostics_panel: DiagnosticsPanel::new(),
            settings_panel: SettingsPanel::new(),
            modal_toolbar: ModalToolbar::new(),
            search_overlay: None,
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn draw(
        &mut self,
        frame: &mut Frame,
        clients: &HashMap<String, Client>,
        servers: &HashMap<String, Server>,
        activities: &[ActivityItem],
        query_input: &str,
        diagnostics: &DiagnosticsData,
        semantic_prediction: &SemanticPrediction,
        selected_proxy_id: Option<&String>,
    ) {
        self.diagnostics_panel.update(diagnostics.clone());
        let area = frame.size();

        // Get the routing mode from the selected proxy (or first proxy)
        let proxy_routing_mode = if let Some(proxy_id) = selected_proxy_id {
            servers.get(proxy_id).and_then(|s| s.routing_mode.clone())
        } else {
            // If no proxy selected, use first proxy's routing mode
            servers
                .values()
                .find(|s| s.server_type == crate::components::ServerType::Proxy)
                .and_then(|s| s.routing_mode.clone())
        };

        // Check if we should show semantic prediction bar (only in semantic/hybrid mode with active prediction)
        let show_semantic_bar = matches!(
            proxy_routing_mode.as_deref(),
            Some("semantic") | Some("Semantic") | Some("hybrid") | Some("Hybrid")
        ) && semantic_prediction.query.is_some();

        // Main layout: Main area, optional semantic bar, status bar, command strip, query input
        let constraints = if show_semantic_bar {
            vec![
                Constraint::Min(15),
                Constraint::Length(3), // Semantic prediction bar
                Constraint::Length(3), // Status bar
                Constraint::Length(2), // Command strip
                Constraint::Length(5), // Query input + contextual help
            ]
        } else {
            vec![
                Constraint::Min(15),
                Constraint::Length(3), // Status bar
                Constraint::Length(2), // Command strip
                Constraint::Length(5), // Query input + contextual help
            ]
        };

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints(constraints)
            .split(area);

        if self.focus == FocusArea::Settings {
            self.settings_panel.render(frame, chunks[0], true);
        } else if self.focus == FocusArea::Diagnostics {
            self.diagnostics_panel.render(frame, chunks[0], true);
        } else {
            let main_columns = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Percentage(60), Constraint::Percentage(40)])
                .split(chunks[0]);

            let left_pane = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Percentage(70), Constraint::Percentage(30)])
                .split(main_columns[0]);

            let (activity_view, match_count) = self.filtered_activities(activities);
            self.activity_feed.render(
                frame,
                left_pane[0],
                &activity_view,
                self.focus == FocusArea::Activity,
            );
            self.llm_responses_panel.render(
                frame,
                left_pane[1],
                self.focus == FocusArea::LlmResponses,
            );

            let drawer_height = self.quick_access.drawer_height();
            let right_sections = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Percentage(55),
                    Constraint::Length(drawer_height),
                    Constraint::Length(3),
                    Constraint::Length(5),
                ])
                .split(main_columns[1]);

            let list_sections = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
                .split(right_sections[0]);

            self.servers_panel.render(
                frame,
                list_sections[0],
                servers,
                clients,
                self.focus == FocusArea::Servers,
                selected_proxy_id,
            );
            self.clients_panel.render(
                frame,
                list_sections[1],
                clients,
                self.focus == FocusArea::ClientsList,
            );

            self.quick_access.render(
                frame,
                right_sections[1],
                self.focus == FocusArea::ActionDrawer,
            );
            self.modal_toolbar.render(
                frame,
                right_sections[2],
                self.focus == FocusArea::ModalToolbar,
            );
            self.render_diagnostics_mini(frame, right_sections[3], diagnostics);

            if let Some(search) = &self.search_overlay {
                self.render_search_overlay(
                    frame,
                    left_pane[0],
                    search,
                    match_count,
                    activities.len(),
                );
            }
        }

        // Get proxy model status from the selected proxy (or first proxy if none selected)
        let proxy_model_status = if let Some(proxy_id) = selected_proxy_id {
            servers.get(proxy_id).and_then(|s| s.model_status.clone())
        } else {
            // If no proxy selected, use first proxy's model status
            servers
                .values()
                .find(|s| s.server_type == crate::components::ServerType::Proxy)
                .and_then(|s| s.model_status.clone())
        };

        // Convert diagnostics to status bar data (show "Loading..." if not initialized)
        let status_data = if diagnostics.model_status == acdp_llm::ModelStatus::NotLoaded
            && diagnostics.routing_mode.is_none()
        {
            StatusBarData::default() // Will show "Loading..."
        } else {
            StatusBarData {
                tui_model_status: Some(diagnostics.model_status.clone()),
                proxy_model_status,
                routing_mode: diagnostics.routing_mode.clone(),
                ttft: diagnostics.ttft,
                tokens_per_sec: diagnostics.tokens_per_sec,
                dspy_accuracy: diagnostics.dspy_accuracy,
                session_accuracy: diagnostics.session_accuracy,
                session_predictions: diagnostics
                    .session_successful_predictions
                    .and_then(|s| diagnostics.session_total_predictions.map(|t| (s, t))),
                mcp_server_health: diagnostics.mcp_server_health,
                mcp_server_connections: diagnostics.mcp_server_connections,
            }
        };

        if show_semantic_bar {
            SemanticStatusBar::render(frame, chunks[1], semantic_prediction);
            StatusBar::render(frame, chunks[2], &status_data);
            self.render_command_strip(frame, chunks[3], &status_data, servers);
            self.render_query_row(frame, chunks[4], query_input);
        } else {
            StatusBar::render(frame, chunks[1], &status_data);
            self.render_command_strip(frame, chunks[2], &status_data, servers);
            self.render_query_row(frame, chunks[3], query_input);
        }
    }

    pub fn get_focus(&self) -> FocusArea {
        self.focus
    }

    pub fn cycle_focus(&mut self) {
        self.focus_next();
    }

    pub fn focus_next(&mut self) {
        if self.focus == FocusArea::Settings {
            self.close_settings();
            return;
        }
        if matches!(self.focus, FocusArea::Diagnostics | FocusArea::Search) {
            return;
        }
        let idx = focus_index(self.focus);
        let next = FOCUS_ORDER[(idx + 1) % FOCUS_ORDER.len()];
        self.set_focus(next);
    }

    pub fn focus_prev(&mut self) {
        if self.focus == FocusArea::Settings {
            self.close_settings();
            return;
        }
        if matches!(self.focus, FocusArea::Diagnostics | FocusArea::Search) {
            return;
        }
        let idx = focus_index(self.focus);
        let prev = if idx == 0 {
            FOCUS_ORDER[FOCUS_ORDER.len() - 1]
        } else {
            FOCUS_ORDER[idx - 1]
        };
        self.set_focus(prev);
    }

    pub fn handle_navigation(&mut self, ctx: NavigationContext, event: Event) -> bool {
        if matches!(self.focus, FocusArea::Diagnostics | FocusArea::Search) {
            return false;
        }
        match event {
            Event::Tab => {
                if self.focus == FocusArea::Settings {
                    self.settings_panel.next_tab();
                    return true;
                }
                false
            }
            Event::Up => {
                match self.focus {
                    FocusArea::Activity => self.activity_feed.previous(),
                    FocusArea::LlmResponses => self.llm_responses_panel.previous(),
                    FocusArea::Servers => self.servers_panel.previous(ctx.server_len),
                    FocusArea::ClientsList => self.clients_panel.previous(ctx.client_len),
                    FocusArea::ActionDrawer => {
                        self.set_focus(FocusArea::ClientsList);
                        return true;
                    }
                    FocusArea::ModalToolbar => {
                        self.set_focus(FocusArea::ActionDrawer);
                        return true;
                    }
                    FocusArea::QueryInput => {
                        self.set_focus(FocusArea::ModalToolbar);
                        return true;
                    }
                    FocusArea::Settings => match self.settings_panel.current_tab {
                        crate::settings_panel::SettingsTab::Models => {
                            self.settings_panel.prev_model()
                        }
                        _ => self.settings_panel.prev_setting(),
                    },
                    _ => return false,
                }
                true
            }
            Event::Down => {
                match self.focus {
                    FocusArea::Activity => self.activity_feed.next(ctx.activity_len),
                    FocusArea::LlmResponses => self.llm_responses_panel.next(),
                    FocusArea::Servers => self.servers_panel.next(ctx.server_len),
                    FocusArea::ClientsList => self.clients_panel.next(ctx.client_len),
                    FocusArea::ActionDrawer => {
                        self.set_focus(FocusArea::ModalToolbar);
                        return true;
                    }
                    FocusArea::ModalToolbar => {
                        self.set_focus(FocusArea::QueryInput);
                        return true;
                    }
                    FocusArea::Settings => match self.settings_panel.current_tab {
                        crate::settings_panel::SettingsTab::Models => {
                            self.settings_panel.next_model()
                        }
                        _ => self.settings_panel.next_setting(),
                    },
                    _ => return false,
                }
                true
            }
            Event::Left => {
                if self.focus == FocusArea::Settings {
                    match self.settings_panel.current_tab {
                        crate::settings_panel::SettingsTab::TuiEngineSettings
                        | crate::settings_panel::SettingsTab::ProxyEngineSettings => {
                            self.settings_panel.adjust_setting(false);
                        }
                        _ => self.settings_panel.prev_tab(),
                    }
                    return true;
                }
                if self.focus == FocusArea::ModalToolbar {
                    self.modal_toolbar.previous();
                    return true;
                }
                let next_focus = match self.focus {
                    FocusArea::LlmResponses => Some(FocusArea::Activity),
                    FocusArea::Servers => Some(FocusArea::LlmResponses),
                    FocusArea::ClientsList => Some(FocusArea::Servers),
                    FocusArea::ActionDrawer => Some(FocusArea::ClientsList),
                    FocusArea::ModalToolbar => Some(FocusArea::ActionDrawer),
                    FocusArea::QueryInput => Some(FocusArea::ModalToolbar),
                    _ => None,
                };
                if let Some(focus) = next_focus {
                    self.set_focus(focus);
                    return true;
                }
                false
            }
            Event::Right => {
                if self.focus == FocusArea::Settings {
                    match self.settings_panel.current_tab {
                        crate::settings_panel::SettingsTab::TuiEngineSettings
                        | crate::settings_panel::SettingsTab::ProxyEngineSettings => {
                            self.settings_panel.adjust_setting(true);
                        }
                        _ => self.settings_panel.next_tab(),
                    }
                    return true;
                }
                if self.focus == FocusArea::ModalToolbar {
                    self.modal_toolbar.next();
                    return true;
                }
                let next_focus = match self.focus {
                    FocusArea::Activity => Some(FocusArea::LlmResponses),
                    FocusArea::LlmResponses => Some(FocusArea::Servers),
                    FocusArea::Servers => Some(FocusArea::ClientsList),
                    FocusArea::ClientsList => Some(FocusArea::ActionDrawer),
                    FocusArea::ActionDrawer => Some(FocusArea::ModalToolbar),
                    FocusArea::ModalToolbar => Some(FocusArea::QueryInput),
                    _ => None,
                };
                if let Some(focus) = next_focus {
                    self.set_focus(focus);
                    return true;
                }
                false
            }
            _ => false,
        }
    }

    fn render_command_strip(
        &self,
        frame: &mut Frame,
        area: Rect,
        status_data: &StatusBarData,
        servers: &HashMap<String, Server>,
    ) {
        let mut spans = Vec::new();
        if let Some(mode) = &status_data.routing_mode {
            spans.push(Span::styled(
                "Mode: ",
                Style::default().fg(colors::TEXT_DIM),
            ));
            spans.push(Span::styled(
                mode.to_string(),
                Style::default().fg(colors::ACCENT_BRIGHT),
            ));
            spans.push(Span::raw("   "));
        }
        if let Some(ttft) = status_data.ttft {
            spans.push(Span::styled(
                "TTFT: ",
                Style::default().fg(colors::TEXT_DIM),
            ));
            spans.push(Span::styled(
                format!("{:.2}s", ttft),
                Style::default().fg(colors::STATUS_SUCCESS),
            ));
            spans.push(Span::raw("   "));
        }
        if let Some(tps) = status_data.tokens_per_sec {
            spans.push(Span::styled(
                "Tokens/s: ",
                Style::default().fg(colors::TEXT_DIM),
            ));
            spans.push(Span::styled(
                format!("{:.1}", tps),
                Style::default().fg(colors::STATUS_SUCCESS),
            ));
            spans.push(Span::raw("   "));
        }
        spans.push(Span::styled(
            "Proxies:",
            Style::default().fg(colors::TEXT_DIM),
        ));
        spans.extend(self.proxy_badges(servers));

        let paragraph = Paragraph::new(Line::from(spans))
            .block(
                Block::default()
                    .title("Command Strip")
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(colors::BORDER))
                    .style(Style::default().bg(colors::BACKGROUND)),
            )
            .alignment(Alignment::Left);
        frame.render_widget(paragraph, area);
    }

    fn proxy_badges(&self, servers: &HashMap<String, Server>) -> Vec<Span<'static>> {
        use crate::components::ServerStatus;
        let mut counts: HashMap<ServerStatus, usize> = HashMap::new();
        for server in servers.values() {
            if server.server_type == crate::components::ServerType::Proxy {
                *counts.entry(server.status.clone()).or_default() += 1;
            }
        }
        if counts.is_empty() {
            return vec![Span::styled(" none", Style::default().fg(colors::TEXT_DIM))];
        }
        let order = [
            ServerStatus::Running,
            ServerStatus::Starting,
            ServerStatus::Degraded,
            ServerStatus::Error,
        ];
        let mut spans = Vec::new();
        for status in order {
            if let Some(count) = counts.get(&status) {
                spans.push(Span::raw(" "));
                spans.push(Span::styled(
                    format!("{}{}", status_icon(&status), count),
                    status.style(),
                ));
            }
        }
        spans
    }

    fn render_query_row(&self, frame: &mut Frame, area: Rect, query_input: &str) {
        let sections = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(70), Constraint::Percentage(30)])
            .split(area);
        self.query_input.render(
            frame,
            sections[0],
            query_input,
            self.focus == FocusArea::QueryInput,
        );
        self.render_context_help(frame, sections[1]);
    }

    fn render_context_help(&self, frame: &mut Frame, area: Rect) {
        let text = self.context_help_text();
        let paragraph = Paragraph::new(Line::from(Span::styled(
            text,
            Style::default().fg(colors::TEXT_DIM),
        )))
        .alignment(Alignment::Left)
        .block(
            Block::default()
                .title("Help")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(colors::BORDER))
                .style(Style::default().bg(colors::BACKGROUND)),
        );
        frame.render_widget(paragraph, area);
    }

    fn context_help_text(&self) -> String {
        match self.focus {
            FocusArea::Activity => "Activity: ↑/↓ scroll | Enter = details".into(),
            FocusArea::LlmResponses => "LLM Responses: ↑/↓ scroll entries".into(),
            FocusArea::Servers => "Servers: ↑/↓ select | ←/→ move panes".into(),
            FocusArea::ClientsList => "Clients: ↑/↓ select | ←/→ move panes".into(),
            FocusArea::ActionDrawer if self.quick_access.is_expanded() => {
                "Quick Actions: ↑/↓ select | Enter run | 'a' collapse".into()
            }
            FocusArea::ActionDrawer => {
                "Quick Actions: press 1/2/3 to run | 'a' expand | Enter = #1".into()
            }
            FocusArea::ModalToolbar => {
                "Toolbar: ←/→ select | Enter open | Shortcuts /, d, s".into()
            }
            FocusArea::QueryInput => "Query: type + Enter to route (@proxy to target)".into(),
            FocusArea::Settings => "Settings: ←/→ tabs | ↑/↓ navigate | Esc exit".into(),
            FocusArea::Diagnostics => "Diagnostics: Esc to close".into(),
            FocusArea::Search => "Search: type to filter logs | Enter apply | Esc cancel".into(),
        }
    }

    fn render_diagnostics_mini(
        &self,
        frame: &mut Frame,
        area: Rect,
        diagnostics: &DiagnosticsData,
    ) {
        let mut lines = Vec::new();
        let status_label = match diagnostics.model_status {
            acdp_llm::ModelStatus::Ready => ("Ready", colors::STATUS_SUCCESS),
            acdp_llm::ModelStatus::Loading => ("Loading", colors::STATUS_WARNING),
            acdp_llm::ModelStatus::NotLoaded => ("Not Loaded", colors::TEXT_DIM),
            acdp_llm::ModelStatus::Error(_) => ("Error", colors::STATUS_ERROR),
        };
        lines.push(Line::from(vec![
            Span::styled("Model: ", Style::default().fg(colors::TEXT_DIM)),
            Span::styled(status_label.0, Style::default().fg(status_label.1)),
        ]));
        if let Some(ttft) = diagnostics.ttft {
            lines.push(Line::from(vec![
                Span::styled("TTFT: ", Style::default().fg(colors::TEXT_DIM)),
                Span::styled(
                    format!("{:.2}s", ttft),
                    Style::default().fg(colors::STATUS_SUCCESS),
                ),
            ]));
        }
        if let Some(tokens) = diagnostics.tokens_per_sec {
            lines.push(Line::from(vec![
                Span::styled("Tokens/s: ", Style::default().fg(colors::TEXT_DIM)),
                Span::styled(
                    format!("{:.1}", tokens),
                    Style::default().fg(colors::STATUS_SUCCESS),
                ),
            ]));
        }
        let paragraph = Paragraph::new(lines)
            .block(
                Block::default()
                    .title("Diagnostics")
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(colors::BORDER))
                    .style(Style::default().bg(colors::BACKGROUND)),
            )
            .wrap(Wrap { trim: true });
        frame.render_widget(paragraph, area);
    }

    fn filtered_activities<'a>(
        &self,
        activities: &'a [ActivityItem],
    ) -> (Cow<'a, [ActivityItem]>, usize) {
        if let Some(search) = &self.search_overlay {
            if search.query.trim().is_empty() {
                return (Cow::Borrowed(activities), activities.len());
            }
            let needle = search.query.to_ascii_lowercase();
            let filtered: Vec<ActivityItem> = activities
                .iter()
                .cloned()
                .filter(|item| search.matches(item, &needle))
                .collect();
            let count = filtered.len();
            (Cow::Owned(filtered), count)
        } else {
            (Cow::Borrowed(activities), activities.len())
        }
    }

    fn render_search_overlay(
        &self,
        frame: &mut Frame,
        area: Rect,
        overlay: &SearchOverlay,
        match_count: usize,
        total: usize,
    ) {
        let popup = Rect {
            x: area.x + 2,
            y: area.y + 1,
            width: area.width.saturating_sub(4),
            height: area.height.min(5),
        };
        let lines = vec![
            Line::from(Span::styled(
                format!("Query: {}", overlay.query),
                Style::default().fg(colors::TEXT_PRIMARY),
            )),
            Line::from(Span::styled(
                format!("Matches: {} / {}", match_count, total),
                Style::default().fg(colors::TEXT_DIM),
            )),
            Line::from(Span::styled(
                "Type to filter · Enter apply · Esc cancel",
                Style::default().fg(colors::TEXT_DIM),
            )),
        ];
        frame.render_widget(Clear, popup);
        frame.render_widget(
            Paragraph::new(lines)
                .block(
                    Block::default()
                        .title("Search Logs")
                        .borders(Borders::ALL)
                        .border_style(Style::default().fg(colors::ACCENT))
                        .style(Style::default().bg(colors::BACKGROUND)),
                )
                .wrap(Wrap { trim: true }),
            popup,
        );
    }

    fn set_focus(&mut self, focus: FocusArea) {
        if focus == FocusArea::Settings && self.focus != FocusArea::Settings {
            self.settings_prev_focus = self.focus;
        }
        if focus == FocusArea::Diagnostics && self.focus != FocusArea::Diagnostics {
            self.diagnostics_prev_focus = self.focus;
        }
        if focus == FocusArea::Search && self.focus != FocusArea::Search {
            self.search_prev_focus = self.focus;
        }
        self.focus = focus;
        match self.focus {
            FocusArea::ActionDrawer => self.quick_access.focus(),
            FocusArea::Activity => self.activity_feed.focus(),
            FocusArea::Settings => {
                self.settings_panel.current_tab = crate::settings_panel::SettingsTab::Models;
            }
            FocusArea::ModalToolbar => self.modal_toolbar.ensure_selected(),
            _ => {}
        }
    }

    pub fn should_refresh_models(&self) -> bool {
        self.focus == FocusArea::Settings && self.settings_panel.models.is_empty()
    }

    pub fn open_settings(&mut self) {
        if self.focus != FocusArea::Settings {
            self.set_focus(FocusArea::Settings);
        }
    }

    pub fn close_settings(&mut self) {
        if self.focus == FocusArea::Settings {
            let target = self.settings_prev_focus;
            self.set_focus(target);
        }
    }

    pub fn open_diagnostics(&mut self) {
        if self.focus != FocusArea::Diagnostics {
            self.set_focus(FocusArea::Diagnostics);
        }
    }

    pub fn close_diagnostics(&mut self) {
        if self.focus == FocusArea::Diagnostics {
            let target = self.diagnostics_prev_focus;
            self.set_focus(target);
        }
    }

    pub fn open_search(&mut self) {
        if !matches!(self.focus, FocusArea::Search) {
            self.search_overlay = Some(SearchOverlay::new());
            self.set_focus(FocusArea::Search);
        }
    }

    pub fn close_search(&mut self) {
        if self.focus == FocusArea::Search {
            let target = self.search_prev_focus;
            self.search_overlay = None;
            self.set_focus(target);
        }
    }

    pub fn search_active(&self) -> bool {
        matches!(self.focus, FocusArea::Search)
    }

    pub fn handle_search_input(&mut self, ch: char) {
        if let Some(search) = &mut self.search_overlay {
            if !ch.is_control() {
                search.query.push(ch);
            }
        }
    }

    pub fn search_backspace(&mut self) {
        if let Some(search) = &mut self.search_overlay {
            search.query.pop();
        }
    }

    pub fn activate_toolbar(&mut self) -> Option<ToolbarAction> {
        self.modal_toolbar.activate()
    }

    pub fn toolbar_shortcut(&mut self, ch: char) -> Option<ToolbarAction> {
        self.modal_toolbar.action_for_shortcut(ch)
    }
}

fn status_icon(status: &crate::components::ServerStatus) -> &'static str {
    use crate::components::ServerStatus::*;
    match status {
        Running => "●",
        Starting => "◐",
        Degraded => "◒",
        Stopped => "◌",
        Error => "◍",
    }
}

fn focus_index(focus: FocusArea) -> usize {
    FOCUS_ORDER
        .iter()
        .position(|item| *item == focus)
        .unwrap_or(0)
}

#[derive(Clone, Debug)]
struct SearchOverlay {
    query: String,
}

impl SearchOverlay {
    fn new() -> Self {
        Self {
            query: String::new(),
        }
    }

    fn matches(&self, item: &ActivityItem, needle: &str) -> bool {
        item.client.to_ascii_lowercase().contains(needle)
            || item.server.to_ascii_lowercase().contains(needle)
            || item.action.to_ascii_lowercase().contains(needle)
            || item.status.label().to_ascii_lowercase().contains(needle)
    }
}

#[derive(Clone, Copy, Debug)]
pub enum ToolbarAction {
    Search,
    Diagnostics,
    Settings,
}

struct ToolbarButton {
    label: &'static str,
    hint: &'static str,
    shortcut: char,
    action: ToolbarAction,
}

struct ModalToolbar {
    buttons: Vec<ToolbarButton>,
    selected: usize,
}

impl ModalToolbar {
    fn new() -> Self {
        Self {
            buttons: vec![
                ToolbarButton {
                    label: "Search",
                    hint: "[/]",
                    shortcut: '/',
                    action: ToolbarAction::Search,
                },
                ToolbarButton {
                    label: "Diagnostics",
                    hint: "[d]",
                    shortcut: 'd',
                    action: ToolbarAction::Diagnostics,
                },
                ToolbarButton {
                    label: "Settings",
                    hint: "[s]",
                    shortcut: 's',
                    action: ToolbarAction::Settings,
                },
            ],
            selected: 0,
        }
    }

    fn ensure_selected(&mut self) {
        if self.buttons.is_empty() {
            self.selected = 0;
        } else {
            self.selected = self.selected.min(self.buttons.len() - 1);
        }
    }

    fn next(&mut self) {
        if self.buttons.is_empty() {
            return;
        }
        self.selected = (self.selected + 1) % self.buttons.len();
    }

    fn previous(&mut self) {
        if self.buttons.is_empty() {
            return;
        }
        if self.selected == 0 {
            self.selected = self.buttons.len() - 1;
        } else {
            self.selected -= 1;
        }
    }

    fn render(&self, frame: &mut Frame, area: Rect, focused: bool) {
        let chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Percentage(33),
                Constraint::Percentage(34),
                Constraint::Percentage(33),
            ])
            .split(area);
        for (idx, chunk) in chunks.iter().enumerate() {
            let button = &self.buttons[idx];
            let mut block = Block::default()
                .borders(Borders::ALL)
                .title(button.hint)
                .border_style(Style::default().fg(colors::BORDER))
                .style(Style::default().bg(colors::BACKGROUND));
            if focused && idx == self.selected {
                block = block
                    .border_style(Style::default().fg(colors::BORDER_FOCUSED))
                    .title_style(Style::default().fg(colors::ACCENT_BRIGHT));
            } else if idx == self.selected {
                block = block.border_style(Style::default().fg(colors::ACCENT));
            }
            let label = Paragraph::new(Line::from(Span::styled(
                button.label,
                Style::default().fg(colors::TEXT_PRIMARY),
            )))
            .alignment(Alignment::Center)
            .block(block);
            frame.render_widget(label, *chunk);
        }
    }

    fn activate(&self) -> Option<ToolbarAction> {
        self.buttons.get(self.selected).map(|b| b.action)
    }

    fn action_for_shortcut(&self, ch: char) -> Option<ToolbarAction> {
        self.buttons
            .iter()
            .find(|b| b.shortcut.eq_ignore_ascii_case(&ch))
            .map(|b| b.action)
    }
}
