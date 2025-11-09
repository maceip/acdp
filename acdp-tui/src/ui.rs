/// New simplified UI - 3 tabs, clean layout
use std::collections::HashMap;

use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
    Frame,
};

use crate::{
    colors,
    components::{
        ActivityFeed, ActivityItem, Client, DiagnosticsData, FocusArea, Server, ServersPanel,
        SettingsPanel,
    },
    query_input::QueryInput,
    status_bar::{StatusBar, StatusBarData},
    tabs::{self, MainTab},
};

pub struct UI {
    /// Current main tab
    pub current_tab: MainTab,
    /// Current focus area (tabs or overlays)
    pub focus: FocusArea,

    // Tab panels
    pub activity_feed: ActivityFeed,
    pub servers_panel: ServersPanel,
    pub settings_panel: SettingsPanel,

    // Other UI elements
    pub query_input: QueryInput,

    // Overlays (full-screen when active)
    diagnostics_prev_focus: FocusArea,
    search_prev_focus: FocusArea,
    search_overlay: Option<SearchOverlay>,

    // Track if models have been refreshed for current settings session
    models_refreshed_for_session: bool,
}

impl UI {
    pub fn new() -> Self {
        Self {
            current_tab: MainTab::Activity,
            focus: FocusArea::MainTabs,
            activity_feed: ActivityFeed::new(),
            servers_panel: ServersPanel::new(),
            settings_panel: SettingsPanel::new(),
            query_input: QueryInput::new(),
            diagnostics_prev_focus: FocusArea::MainTabs,
            search_prev_focus: FocusArea::MainTabs,
            search_overlay: None,
            models_refreshed_for_session: false,
        }
    }

    pub fn draw(
        &mut self,
        frame: &mut Frame,
        clients: &HashMap<String, Client>,
        servers: &HashMap<String, Server>,
        activities: &[ActivityItem],
        query_input: &str,
        diagnostics: &DiagnosticsData,
        selected_proxy_id: Option<&String>,
    ) {
        let area = frame.size();

        // Main layout: Status bar, Main content, Input area
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3), // Status bar
                Constraint::Min(10),   // Main content area
                Constraint::Length(5), // Input + help
            ])
            .split(area);

        // Render status bar
        self.render_status_bar(frame, chunks[0], diagnostics, servers, selected_proxy_id);

        // Render main content based on focus
        if self.focus == FocusArea::Settings {
            // Settings overlay takes full screen
            self.settings_panel.render(frame, chunks[1], true);
        } else if self.focus == FocusArea::Diagnostics {
            // Diagnostics overlay takes full screen
            self.render_diagnostics_overlay(frame, chunks[1], diagnostics);
        } else {
            // Normal view: tabs + content
            self.render_main_content(
                frame,
                chunks[1],
                clients,
                servers,
                activities,
                selected_proxy_id,
            );
        }

        // Render input area
        self.render_input_area(frame, chunks[2], query_input);
    }

    fn render_status_bar(
        &self,
        frame: &mut Frame,
        area: Rect,
        diagnostics: &DiagnosticsData,
        servers: &HashMap<String, Server>,
        selected_proxy_id: Option<&String>,
    ) {
        // Get proxy model status from the selected proxy (or first proxy if none selected)
        let proxy_model_status = if let Some(proxy_id) = selected_proxy_id {
            servers.get(proxy_id).and_then(|s| s.model_status.clone())
        } else {
            servers
                .values()
                .find(|s| s.server_type == crate::components::ServerType::Proxy)
                .and_then(|s| s.model_status.clone())
        };

        let status_data = if diagnostics.model_status == acdp_llm::ModelStatus::NotLoaded
            && diagnostics.routing_mode.is_none()
        {
            StatusBarData::default()
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

        StatusBar::render(frame, area, &status_data);
    }

    fn render_main_content(
        &mut self,
        frame: &mut Frame,
        area: Rect,
        clients: &HashMap<String, Client>,
        servers: &HashMap<String, Server>,
        activities: &[ActivityItem],
        selected_proxy_id: Option<&String>,
    ) {
        // Split into tabs and content
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(3), Constraint::Min(7)])
            .split(area);

        // Render tab bar
        tabs::render_tab_bar(frame, chunks[0], self.current_tab);

        // Render current tab content
        let focused = self.focus == FocusArea::MainTabs;
        match self.current_tab {
            MainTab::Activity => {
                let (filtered_activities, _) = self.filtered_activities(activities);
                self.activity_feed
                    .render(frame, chunks[1], &filtered_activities, focused);

                // Render search overlay if active
                if let Some(search) = &self.search_overlay {
                    self.render_search_overlay(frame, chunks[1], search, activities);
                }
            }
            MainTab::Servers => {
                self.servers_panel.render(
                    frame,
                    chunks[1],
                    servers,
                    clients,
                    focused,
                    selected_proxy_id,
                );
            }
            MainTab::Settings => {
                self.settings_panel.render(frame, chunks[1], focused);
            }
        }
    }

    fn render_input_area(&self, frame: &mut Frame, area: Rect, query_input: &str) {
        // Split into query input and contextual help
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(3), Constraint::Length(2)])
            .split(area);

        // Render query input
        let focused = self.focus == FocusArea::QueryInput;
        self.query_input
            .render(frame, chunks[0], query_input, focused);

        // Render contextual help
        let help_text = if self.focus == FocusArea::QueryInput {
            "Enter: submit | Ctrl+R: semantic route | Ctrl+H: health | Tab: switch focus"
        } else {
            &tabs::render_tab_help(self.current_tab)
        };

        let help_paragraph = Paragraph::new(Line::from(Span::styled(
            help_text,
            Style::default().fg(colors::TEXT_DIM),
        )))
        .alignment(Alignment::Center)
        .block(
            Block::default()
                .borders(Borders::TOP)
                .border_style(Style::default().fg(colors::BORDER)),
        );
        frame.render_widget(help_paragraph, chunks[1]);
    }

    fn render_diagnostics_overlay(
        &self,
        frame: &mut Frame,
        area: Rect,
        diagnostics: &DiagnosticsData,
    ) {
        use ratatui::widgets::Paragraph;

        let mut lines = vec![Line::from(vec![
            Span::styled("Model Status: ", Style::default().fg(colors::TEXT_DIM)),
            Span::styled(
                format!("{:?}", diagnostics.model_status),
                Style::default().fg(colors::ACCENT_BRIGHT),
            ),
        ])];

        if let Some(ref mode) = diagnostics.routing_mode {
            lines.push(Line::from(vec![
                Span::styled("Routing Mode: ", Style::default().fg(colors::TEXT_DIM)),
                Span::styled(mode.clone(), Style::default().fg(colors::ACCENT_BRIGHT)),
            ]));
        }

        if let Some(ttft) = diagnostics.ttft {
            lines.push(Line::from(vec![
                Span::styled("TTFT: ", Style::default().fg(colors::TEXT_DIM)),
                Span::styled(
                    format!("{:.2}s", ttft),
                    Style::default().fg(colors::STATUS_SUCCESS),
                ),
            ]));
        }

        if let Some(tps) = diagnostics.tokens_per_sec {
            lines.push(Line::from(vec![
                Span::styled("Tokens/sec: ", Style::default().fg(colors::TEXT_DIM)),
                Span::styled(
                    format!("{:.1}", tps),
                    Style::default().fg(colors::STATUS_SUCCESS),
                ),
            ]));
        }

        if let Some(acc) = diagnostics.dspy_accuracy {
            lines.push(Line::from(vec![
                Span::styled("DSPy Accuracy: ", Style::default().fg(colors::TEXT_DIM)),
                Span::styled(
                    format!("{:.1}%", acc * 100.0),
                    Style::default().fg(colors::STATUS_SUCCESS),
                ),
            ]));
        }

        if let Some(acc) = diagnostics.session_accuracy {
            lines.push(Line::from(vec![
                Span::styled("Session Accuracy: ", Style::default().fg(colors::TEXT_DIM)),
                Span::styled(
                    format!("{:.1}%", acc * 100.0),
                    Style::default().fg(colors::STATUS_SUCCESS),
                ),
            ]));
        }

        if let (Some(succ), Some(total)) = (
            diagnostics.session_successful_predictions,
            diagnostics.session_total_predictions,
        ) {
            lines.push(Line::from(vec![
                Span::styled(
                    "Session Predictions: ",
                    Style::default().fg(colors::TEXT_DIM),
                ),
                Span::styled(
                    format!("{}/{}", succ, total),
                    Style::default().fg(colors::STATUS_SUCCESS),
                ),
            ]));
        }

        let paragraph = Paragraph::new(lines)
            .block(
                Block::default()
                    .title("Diagnostics (Press Esc to close)")
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(colors::ACCENT))
                    .style(Style::default().bg(colors::BACKGROUND)),
            )
            .alignment(Alignment::Left);

        frame.render_widget(paragraph, area);
    }

    fn filtered_activities<'a>(
        &self,
        activities: &'a [ActivityItem],
    ) -> (std::borrow::Cow<'a, [ActivityItem]>, usize) {
        if let Some(search) = &self.search_overlay {
            if search.query.trim().is_empty() {
                return (std::borrow::Cow::Borrowed(activities), activities.len());
            }
            let needle = search.query.to_ascii_lowercase();
            let filtered: Vec<ActivityItem> = activities
                .iter()
                .cloned()
                .filter(|item| search.matches(item, &needle))
                .collect();
            let count = filtered.len();
            (std::borrow::Cow::Owned(filtered), count)
        } else {
            (std::borrow::Cow::Borrowed(activities), activities.len())
        }
    }

    fn render_search_overlay(
        &self,
        frame: &mut Frame,
        area: Rect,
        overlay: &SearchOverlay,
        activities: &[ActivityItem],
    ) {
        use ratatui::widgets::{Clear, Paragraph, Wrap};

        let (_, match_count) = self.filtered_activities(activities);

        let popup = Rect {
            x: area.x + 2,
            y: area.y + 1,
            width: area.width.saturating_sub(4),
            height: area.height.min(5),
        };

        let lines = vec![
            Line::from(Span::styled(
                format!("Search: {}", overlay.query),
                Style::default().fg(colors::TEXT_PRIMARY),
            )),
            Line::from(Span::styled(
                format!("Matches: {} / {}", match_count, activities.len()),
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
                        .title("Search")
                        .borders(Borders::ALL)
                        .border_style(Style::default().fg(colors::ACCENT))
                        .style(Style::default().bg(colors::BACKGROUND)),
                )
                .wrap(Wrap { trim: true }),
            popup,
        );
    }

    // Navigation methods
    pub fn next_tab(&mut self) {
        self.current_tab = self.current_tab.next();
    }

    pub fn prev_tab(&mut self) {
        self.current_tab = self.current_tab.prev();
    }

    pub fn switch_to_tab(&mut self, tab: MainTab) {
        self.current_tab = tab;
    }

    pub fn get_focus(&self) -> FocusArea {
        self.focus
    }

    pub fn toggle_focus(&mut self) {
        self.focus = match self.focus {
            FocusArea::MainTabs => FocusArea::QueryInput,
            FocusArea::QueryInput => FocusArea::MainTabs,
            _ => FocusArea::MainTabs, // Overlays don't toggle
        };
    }

    // Overlay methods
    pub fn open_settings(&mut self) {
        tracing::info!("open_settings() called, current focus: {:?}", self.focus);
        if self.focus != FocusArea::Settings {
            tracing::info!("Changing focus to Settings");
            self.focus = FocusArea::Settings;
        } else {
            tracing::info!("Already in Settings");
        }
    }

    pub fn close_settings(&mut self) {
        if self.focus == FocusArea::Settings {
            self.focus = FocusArea::MainTabs;
        }
    }

    pub fn open_diagnostics(&mut self) {
        if self.focus != FocusArea::Diagnostics {
            self.diagnostics_prev_focus = self.focus;
            self.focus = FocusArea::Diagnostics;
        }
    }

    pub fn close_diagnostics(&mut self) {
        if self.focus == FocusArea::Diagnostics {
            self.focus = self.diagnostics_prev_focus;
        }
    }

    pub fn open_search(&mut self) {
        if !matches!(self.focus, FocusArea::Search) {
            self.search_prev_focus = self.focus;
            self.search_overlay = Some(SearchOverlay::new());
            self.focus = FocusArea::Search;
        }
    }

    pub fn close_search(&mut self) {
        if self.focus == FocusArea::Search {
            self.search_overlay = None;
            self.focus = self.search_prev_focus;
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

    pub fn should_refresh_models(&mut self) -> bool {
        // Refresh models once per settings session (when first opened)
        if self.focus == FocusArea::Settings && !self.models_refreshed_for_session {
            tracing::info!(
                "should_refresh_models: YES (first time opening settings this session, models.len()={})",
                self.settings_panel.models.len()
            );
            self.models_refreshed_for_session = true;
            true
        } else {
            if self.focus == FocusArea::Settings {
                tracing::debug!(
                    "should_refresh_models: NO (already refreshed this session, models.len()={})",
                    self.settings_panel.models.len()
                );
            }
            false
        }
    }

    /// Reset models refresh flag when leaving settings
    pub fn reset_models_refresh_flag(&mut self) {
        if self.focus != FocusArea::Settings && self.models_refreshed_for_session {
            tracing::debug!("Resetting models_refreshed_for_session flag");
            self.models_refreshed_for_session = false;
        }
    }

    // Handle navigation within current tab
    pub fn handle_up(&mut self, ctx: &NavigationContext) {
        match self.current_tab {
            MainTab::Activity => self.activity_feed.previous(),
            MainTab::Servers => self.servers_panel.previous(ctx.server_len),
            MainTab::Settings => match self.settings_panel.current_tab {
                crate::settings_panel::SettingsTab::Models => self.settings_panel.prev_model(),
                _ => self.settings_panel.prev_setting(),
            },
        }
    }

    pub fn handle_down(&mut self, ctx: &NavigationContext) {
        match self.current_tab {
            MainTab::Activity => self.activity_feed.next(ctx.activity_len),
            MainTab::Servers => self.servers_panel.next(ctx.server_len),
            MainTab::Settings => match self.settings_panel.current_tab {
                crate::settings_panel::SettingsTab::Models => self.settings_panel.next_model(),
                _ => self.settings_panel.next_setting(),
            },
        }
    }

    pub fn handle_left(&mut self) {
        if self.current_tab == MainTab::Settings {
            match self.settings_panel.current_tab {
                crate::settings_panel::SettingsTab::TuiEngineSettings
                | crate::settings_panel::SettingsTab::ProxyEngineSettings => {
                    self.settings_panel.adjust_setting(false);
                }
                _ => self.settings_panel.prev_tab(),
            }
        }
    }

    pub fn handle_right(&mut self) {
        if self.current_tab == MainTab::Settings {
            match self.settings_panel.current_tab {
                crate::settings_panel::SettingsTab::TuiEngineSettings
                | crate::settings_panel::SettingsTab::ProxyEngineSettings => {
                    self.settings_panel.adjust_setting(true);
                }
                _ => self.settings_panel.next_tab(),
            }
        }
    }
}

pub struct NavigationContext {
    pub activity_len: usize,
    pub server_len: usize,
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
