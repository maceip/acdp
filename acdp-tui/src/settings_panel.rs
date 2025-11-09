use crate::colors;
use acdp_llm::LlmService;
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph, Wrap},
    Frame,
};
use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct LiteRTModel {
    pub name: String,
    pub size: String,
    pub description: String,
    pub installed: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsTab {
    Models,
    TuiEngineSettings,
    ProxyEngineSettings,
    Advanced,
}

impl SettingsTab {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Models => "Models",
            Self::TuiEngineSettings => "TUI Engine",
            Self::ProxyEngineSettings => "Proxy Engine",
            Self::Advanced => "Advanced",
        }
    }

    pub fn next(&self) -> Self {
        match self {
            Self::Models => Self::TuiEngineSettings,
            Self::TuiEngineSettings => Self::ProxyEngineSettings,
            Self::ProxyEngineSettings => Self::Advanced,
            Self::Advanced => Self::Models,
        }
    }

    pub fn prev(&self) -> Self {
        match self {
            Self::Models => Self::Advanced,
            Self::TuiEngineSettings => Self::Models,
            Self::ProxyEngineSettings => Self::TuiEngineSettings,
            Self::Advanced => Self::ProxyEngineSettings,
        }
    }
}

#[derive(Debug, Clone)]
pub struct EngineSettings {
    pub backend: String,   // "cpu" or "gpu"
    pub temperature: f32,  // 0.0 - 2.0
    pub max_tokens: usize, // 1 - 4096
    pub semantic_routing: bool,
    pub semantic_model_path: Option<String>,
    pub active_model: Option<String>, // Currently selected model
}

impl Default for EngineSettings {
    fn default() -> Self {
        Self {
            backend: "cpu".to_string(),
            temperature: 0.7,
            max_tokens: 1000,
            semantic_routing: false,
            semantic_model_path: None,
            active_model: Some("gemma3-1b-it-int4".to_string()),
        }
    }
}

pub struct SettingsPanel {
    pub current_tab: SettingsTab,
    pub models: Vec<LiteRTModel>,
    pub model_list_state: ListState,
    pub tui_engine_settings: EngineSettings,
    pub proxy_engine_settings: EngineSettings,
    pub selected_setting: usize, // For engine settings tabs
    pub hf_token: String,
    pub loading: bool,
    pub status_message: Option<String>,
    pub panel_focus: PanelFocus, // Track which panel (models list vs details) is focused
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PanelFocus {
    ModelsList,
    Details,
}

impl SettingsPanel {
    pub fn new() -> Self {
        Self {
            current_tab: SettingsTab::Models,
            models: Vec::new(),
            model_list_state: ListState::default(),
            tui_engine_settings: EngineSettings::default(),
            proxy_engine_settings: EngineSettings::default(),
            selected_setting: 0,
            hf_token: std::env::var("HF_TOKEN").unwrap_or_else(|_| String::new()), // Load from environment or empty
            loading: false,
            status_message: None,
            panel_focus: PanelFocus::ModelsList,
        }
    }

    pub async fn refresh_models(&mut self, llm_service: Option<Arc<LlmService>>) {
        self.loading = true;
        self.status_message = Some("Loading models...".to_string());

        tracing::info!(
            "refresh_models called, llm_service present: {}",
            llm_service.is_some()
        );

        if let Some(service) = llm_service {
            // Get cached models from mcp-llm
            tracing::info!("Calling list_cached_models()...");
            match service.list_cached_models().await {
                Ok(cached) => {
                    tracing::info!("list_cached_models returned {} models", cached.len());
                    let mut models = Vec::new();

                    // Add cached models
                    for model in cached {
                        tracing::debug!("Adding cached model: {}", model.name);
                        models.push(LiteRTModel {
                            name: model.name.clone(),
                            size: Self::format_size(model.size_bytes),
                            description: format!("Cached model: {}", model.path.display()),
                            installed: true,
                        });
                    }

                    // Get available models from remote
                    tracing::info!("Calling list_available_models()...");
                    match service.list_available_models().await {
                        Ok(available) => {
                            tracing::info!(
                                "list_available_models returned {} models",
                                available.len()
                            );
                            for model in available {
                                // Only add if not already in cached list
                                if !models.iter().any(|m| m.name == model.name) {
                                    tracing::debug!("Adding available model: {}", model.name);
                                    models.push(LiteRTModel {
                                        name: model.name.clone(),
                                        size: Self::format_size(model.size_bytes),
                                        description: "Available for download".to_string(),
                                        installed: false,
                                    });
                                }
                            }
                        }
                        Err(e) => {
                            tracing::warn!("Failed to list available models: {}", e);
                        }
                    }

                    tracing::info!("Setting self.models to {} items", models.len());
                    self.models = models;
                    self.status_message = Some(format!("Found {} models", self.models.len()));
                }
                Err(e) => {
                    tracing::error!("Failed to load models: {}", e);
                    self.status_message = Some(format!("Failed to load models: {}", e));
                }
            }
        } else {
            tracing::warn!("LLM service not available for refresh_models");
            self.status_message = Some("LLM service not available".to_string());
        }

        self.loading = false;
        tracing::info!(
            "refresh_models completed, models.len() = {}",
            self.models.len()
        );
    }

    fn format_size(bytes: u64) -> String {
        const KB: u64 = 1024;
        const MB: u64 = KB * 1024;
        const GB: u64 = MB * 1024;

        if bytes >= GB {
            format!("{:.2}GB", bytes as f64 / GB as f64)
        } else if bytes >= MB {
            format!("{:.0}MB", bytes as f64 / MB as f64)
        } else if bytes >= KB {
            format!("{:.0}KB", bytes as f64 / KB as f64)
        } else {
            format!("{}B", bytes)
        }
    }

    pub async fn pull_model(&mut self, model_name: &str, llm_service: Option<Arc<LlmService>>) {
        self.loading = true;
        self.status_message = Some(format!("Downloading {}...", model_name));

        if let Some(service) = llm_service {
            match service.download_model(model_name).await {
                Ok(_) => {
                    self.status_message = Some(format!("Successfully downloaded {}", model_name));
                    self.refresh_models(Some(service)).await;
                }
                Err(e) => {
                    self.status_message = Some(format!("Failed to download: {}", e));
                }
            }
        } else {
            self.status_message = Some("LLM service not available".to_string());
        }

        self.loading = false;
    }

    pub fn next_tab(&mut self) {
        self.current_tab = self.current_tab.next();
    }

    pub fn prev_tab(&mut self) {
        self.current_tab = self.current_tab.prev();
    }

    pub fn toggle_panel_focus(&mut self) {
        self.panel_focus = match self.panel_focus {
            PanelFocus::ModelsList => PanelFocus::Details,
            PanelFocus::Details => PanelFocus::ModelsList,
        };
    }

    pub fn next_model(&mut self) {
        if self.models.is_empty() {
            return;
        }

        let i = match self.model_list_state.selected() {
            Some(i) => {
                if i >= self.models.len() - 1 {
                    0
                } else {
                    i + 1
                }
            }
            None => 0,
        };
        self.model_list_state.select(Some(i));
    }

    pub fn prev_model(&mut self) {
        if self.models.is_empty() {
            return;
        }

        let i = match self.model_list_state.selected() {
            Some(i) => {
                if i == 0 {
                    self.models.len() - 1
                } else {
                    i - 1
                }
            }
            None => 0,
        };
        self.model_list_state.select(Some(i));
    }

    pub fn next_setting(&mut self) {
        self.selected_setting = (self.selected_setting + 1) % 6; // 6 settings
    }

    pub fn prev_setting(&mut self) {
        if self.selected_setting == 0 {
            self.selected_setting = 5;
        } else {
            self.selected_setting -= 1;
        }
    }

    pub fn adjust_setting(&mut self, increase: bool) {
        // Determine which settings to modify based on current tab
        let settings = match self.current_tab {
            SettingsTab::TuiEngineSettings => &mut self.tui_engine_settings,
            SettingsTab::ProxyEngineSettings => &mut self.proxy_engine_settings,
            _ => return,
        };

        match self.selected_setting {
            0 => {
                // Active Model: cycle through installed models
                if self.models.is_empty() {
                    return;
                }

                let installed_models: Vec<&LiteRTModel> =
                    self.models.iter().filter(|m| m.installed).collect();

                if installed_models.is_empty() {
                    return;
                }

                let current_index = settings
                    .active_model
                    .as_ref()
                    .and_then(|name| installed_models.iter().position(|m| &m.name == name))
                    .unwrap_or(0);

                let new_index = if increase {
                    (current_index + 1) % installed_models.len()
                } else {
                    if current_index == 0 {
                        installed_models.len() - 1
                    } else {
                        current_index - 1
                    }
                };

                settings.active_model = Some(installed_models[new_index].name.clone());
            }
            1 => {
                // Backend toggle
                settings.backend = if settings.backend == "cpu" {
                    "gpu".to_string()
                } else {
                    "cpu".to_string()
                };
            }
            2 => {
                // Temperature: 0.0 - 2.0, step 0.1
                if increase {
                    settings.temperature = (settings.temperature + 0.1).min(2.0);
                } else {
                    settings.temperature = (settings.temperature - 0.1).max(0.0);
                }
            }
            3 => {
                // Max tokens: 1 - 4096, step 100
                if increase {
                    settings.max_tokens = (settings.max_tokens + 100).min(4096);
                } else {
                    settings.max_tokens = (settings.max_tokens.saturating_sub(100)).max(1);
                }
            }
            4 => {
                // Semantic routing toggle
                settings.semantic_routing = !settings.semantic_routing;
            }
            5 => {
                // Semantic model path - not adjustable via +/-, use text input
            }
            _ => {}
        }
    }

    pub fn render(&mut self, frame: &mut Frame, area: Rect, focused: bool) {
        // Render outer container with contrasting border
        let outer_block = Block::default()
            .title(" Settings (Press Esc to exit) ")
            .title_style(
                Style::default()
                    .fg(colors::ACCENT_BRIGHT)
                    .add_modifier(Modifier::BOLD),
            )
            .borders(Borders::ALL)
            .border_style(Style::default().fg(colors::SETTINGS_BORDER))
            .border_type(ratatui::widgets::BorderType::Thick)
            .style(Style::default().bg(colors::SETTINGS_BACKGROUND));

        let inner_area = outer_block.inner(area);
        frame.render_widget(outer_block, area);

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3), // Tab bar
                Constraint::Min(10),   // Content area
                Constraint::Length(3), // Status bar
            ])
            .split(inner_area);

        // Tab bar
        self.render_tab_bar(frame, chunks[0]);

        // Content based on current tab
        match self.current_tab {
            SettingsTab::Models => self.render_models_tab(frame, chunks[1], focused),
            SettingsTab::TuiEngineSettings => {
                self.render_engine_settings_tab(frame, chunks[1], focused, true)
            }
            SettingsTab::ProxyEngineSettings => {
                self.render_engine_settings_tab(frame, chunks[1], focused, false)
            }
            SettingsTab::Advanced => self.render_advanced_tab(frame, chunks[1]),
        }

        // Status bar
        self.render_status_bar(frame, chunks[2]);
    }

    fn render_tab_bar(&self, frame: &mut Frame, area: Rect) {
        let tabs = [
            SettingsTab::Models,
            SettingsTab::TuiEngineSettings,
            SettingsTab::ProxyEngineSettings,
            SettingsTab::Advanced,
        ];

        let tab_spans: Vec<Span> = tabs
            .iter()
            .map(|tab| {
                let style = if *tab == self.current_tab {
                    Style::default()
                        .fg(colors::ACCENT_BRIGHT)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(colors::TEXT_PRIMARY)
                };
                Span::styled(format!(" {} ", tab.label()), style)
            })
            .collect();

        let tabs_line = Line::from(tab_spans);
        let tabs_widget = Paragraph::new(tabs_line).block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(colors::BORDER)),
        );

        frame.render_widget(tabs_widget, area);
    }

    fn render_models_tab(&mut self, frame: &mut Frame, area: Rect, focused: bool) {
        let chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(60), Constraint::Percentage(40)])
            .split(area);

        // Model list
        let items: Vec<ListItem> = self
            .models
            .iter()
            .map(|model| {
                let status_icon = if model.installed { "✓" } else { " " };
                let line = Line::from(vec![
                    Span::styled(
                        format!(" {} ", status_icon),
                        if model.installed {
                            Style::default().fg(colors::STATUS_SUCCESS)
                        } else {
                            Style::default().fg(colors::TEXT_DIM)
                        },
                    ),
                    Span::styled(&model.name, Style::default().fg(colors::TEXT_PRIMARY)),
                    Span::styled(
                        format!(" ({})", model.size),
                        Style::default().fg(colors::TEXT_DIM),
                    ),
                ]);
                ListItem::new(line)
            })
            .collect();

        let list = List::new(items)
            .block(
                Block::default()
                    .title(" Available Models ")
                    .borders(Borders::ALL)
                    .border_style(if focused {
                        Style::default().fg(colors::ACCENT_BRIGHT)
                    } else {
                        Style::default().fg(colors::BORDER)
                    }),
            )
            .highlight_style(
                Style::default()
                    .fg(colors::ACCENT_BRIGHT)
                    .bg(colors::BORDER)
                    .add_modifier(Modifier::BOLD),
            )
            .highlight_symbol("▶ ");

        frame.render_stateful_widget(list, chunks[0], &mut self.model_list_state);

        // Model details / actions panel
        let details_text = if let Some(selected) = self.model_list_state.selected() {
            if let Some(model) = self.models.get(selected) {
                vec![
                    Line::from(vec![
                        Span::styled("Name: ", Style::default().fg(colors::TEXT_DIM)),
                        Span::styled(&model.name, Style::default().fg(colors::TEXT_PRIMARY)),
                    ]),
                    Line::from(vec![
                        Span::styled("Size: ", Style::default().fg(colors::TEXT_DIM)),
                        Span::styled(&model.size, Style::default().fg(colors::TEXT_PRIMARY)),
                    ]),
                    Line::from(vec![
                        Span::styled("Status: ", Style::default().fg(colors::TEXT_DIM)),
                        Span::styled(
                            if model.installed {
                                "Installed"
                            } else {
                                "Not installed"
                            },
                            if model.installed {
                                Style::default().fg(colors::STATUS_SUCCESS)
                            } else {
                                Style::default().fg(colors::TEXT_DIM)
                            },
                        ),
                    ]),
                    Line::from(""),
                    Line::from(Span::styled(
                        &model.description,
                        Style::default().fg(colors::TEXT_PRIMARY),
                    )),
                    Line::from(""),
                    Line::from(""),
                    Line::from(vec![Span::styled(
                        "Actions:",
                        Style::default()
                            .fg(colors::TEXT_DIM)
                            .add_modifier(Modifier::BOLD),
                    )]),
                    Line::from(vec![
                        Span::styled("  [", Style::default().fg(colors::TEXT_DIM)),
                        Span::styled("Tab", Style::default().fg(colors::ACCENT_BRIGHT)),
                        Span::styled("] Switch to ", Style::default().fg(colors::TEXT_PRIMARY)),
                        Span::styled("TUI Engine", Style::default().fg(colors::ACCENT_BRIGHT)),
                        Span::styled(" / ", Style::default().fg(colors::TEXT_DIM)),
                        Span::styled("Proxy Engine", Style::default().fg(colors::ACCENT_BRIGHT)),
                        Span::styled(" / ", Style::default().fg(colors::TEXT_DIM)),
                        Span::styled("Advanced", Style::default().fg(colors::ACCENT_BRIGHT)),
                    ]),
                    Line::from(vec![
                        Span::styled("  [", Style::default().fg(colors::TEXT_DIM)),
                        Span::styled("p", Style::default().fg(colors::ACCENT_BRIGHT)),
                        Span::styled("] Pull model", Style::default().fg(colors::TEXT_PRIMARY)),
                    ]),
                    Line::from(vec![
                        Span::styled("  [", Style::default().fg(colors::TEXT_DIM)),
                        Span::styled("r", Style::default().fg(colors::ACCENT_BRIGHT)),
                        Span::styled("] Refresh list", Style::default().fg(colors::TEXT_PRIMARY)),
                    ]),
                ]
            } else {
                vec![Line::from("No model selected")]
            }
        } else {
            vec![Line::from("No model selected")]
        };

        let details = Paragraph::new(details_text)
            .block(
                Block::default()
                    .title(" Model Details ")
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(colors::BORDER)),
            )
            .wrap(Wrap { trim: false });

        frame.render_widget(details, chunks[1]);
    }

    fn render_engine_settings_tab(
        &self,
        frame: &mut Frame,
        area: Rect,
        focused: bool,
        is_tui: bool,
    ) {
        // Select the appropriate settings based on tab
        let engine_settings = if is_tui {
            &self.tui_engine_settings
        } else {
            &self.proxy_engine_settings
        };

        let title_suffix = if is_tui { "TUI" } else { "Proxies" };

        let settings = vec![
            Line::from(vec![
                Span::styled(
                    if self.selected_setting == 0 {
                        "▶ "
                    } else {
                        "  "
                    },
                    Style::default().fg(colors::ACCENT_BRIGHT),
                ),
                Span::styled("Active Model: ", Style::default().fg(colors::TEXT_DIM)),
                Span::styled(
                    engine_settings.active_model.as_deref().unwrap_or("(none)"),
                    Style::default().fg(colors::STATUS_SUCCESS),
                ),
                Span::styled(" (← → to change)", Style::default().fg(colors::TEXT_DIM)),
            ]),
            Line::from(""),
            Line::from(vec![
                Span::styled(
                    if self.selected_setting == 1 {
                        "▶ "
                    } else {
                        "  "
                    },
                    Style::default().fg(colors::ACCENT_BRIGHT),
                ),
                Span::styled("Backend: ", Style::default().fg(colors::TEXT_DIM)),
                Span::styled(
                    &engine_settings.backend,
                    Style::default().fg(colors::TEXT_PRIMARY),
                ),
                Span::styled(" (← → to toggle)", Style::default().fg(colors::TEXT_DIM)),
            ]),
            Line::from(""),
            Line::from(vec![
                Span::styled(
                    if self.selected_setting == 2 {
                        "▶ "
                    } else {
                        "  "
                    },
                    Style::default().fg(colors::ACCENT_BRIGHT),
                ),
                Span::styled("Temperature: ", Style::default().fg(colors::TEXT_DIM)),
                Span::styled(
                    format!("{:.1}", engine_settings.temperature),
                    Style::default().fg(colors::TEXT_PRIMARY),
                ),
                Span::styled(" (← → to adjust)", Style::default().fg(colors::TEXT_DIM)),
            ]),
            Line::from(""),
            Line::from(vec![
                Span::styled(
                    if self.selected_setting == 3 {
                        "▶ "
                    } else {
                        "  "
                    },
                    Style::default().fg(colors::ACCENT_BRIGHT),
                ),
                Span::styled("Max Tokens: ", Style::default().fg(colors::TEXT_DIM)),
                Span::styled(
                    engine_settings.max_tokens.to_string(),
                    Style::default().fg(colors::TEXT_PRIMARY),
                ),
                Span::styled(" (← → to adjust)", Style::default().fg(colors::TEXT_DIM)),
            ]),
            Line::from(""),
            Line::from(vec![
                Span::styled(
                    if self.selected_setting == 4 {
                        "▶ "
                    } else {
                        "  "
                    },
                    Style::default().fg(colors::ACCENT_BRIGHT),
                ),
                Span::styled("Semantic Routing: ", Style::default().fg(colors::TEXT_DIM)),
                Span::styled(
                    if engine_settings.semantic_routing {
                        "Enabled"
                    } else {
                        "Disabled"
                    },
                    if engine_settings.semantic_routing {
                        Style::default().fg(colors::STATUS_SUCCESS)
                    } else {
                        Style::default().fg(colors::TEXT_DIM)
                    },
                ),
                Span::styled(" (← → to toggle)", Style::default().fg(colors::TEXT_DIM)),
            ]),
            Line::from(""),
            Line::from(vec![
                Span::styled(
                    if self.selected_setting == 5 {
                        "▶ "
                    } else {
                        "  "
                    },
                    Style::default().fg(colors::ACCENT_BRIGHT),
                ),
                Span::styled(
                    "Semantic Model Path: ",
                    Style::default().fg(colors::TEXT_DIM),
                ),
                Span::styled(
                    engine_settings
                        .semantic_model_path
                        .as_deref()
                        .unwrap_or("(default)"),
                    Style::default().fg(colors::TEXT_PRIMARY),
                ),
            ]),
            Line::from(""),
            Line::from(""),
            Line::from(vec![Span::styled(
                "Navigation:",
                Style::default()
                    .fg(colors::TEXT_DIM)
                    .add_modifier(Modifier::BOLD),
            )]),
            Line::from(vec![
                Span::styled("  [", Style::default().fg(colors::TEXT_DIM)),
                Span::styled("Tab", Style::default().fg(colors::ACCENT_BRIGHT)),
                Span::styled("] Switch to ", Style::default().fg(colors::TEXT_PRIMARY)),
                Span::styled("Models", Style::default().fg(colors::ACCENT_BRIGHT)),
                Span::styled(" / ", Style::default().fg(colors::TEXT_DIM)),
                Span::styled(
                    if is_tui { "Proxy Engine" } else { "TUI Engine" },
                    Style::default().fg(colors::ACCENT_BRIGHT),
                ),
                Span::styled(" / ", Style::default().fg(colors::TEXT_DIM)),
                Span::styled("Advanced", Style::default().fg(colors::ACCENT_BRIGHT)),
            ]),
            Line::from(vec![
                Span::styled("  [", Style::default().fg(colors::TEXT_DIM)),
                Span::styled("↑↓", Style::default().fg(colors::ACCENT_BRIGHT)),
                Span::styled(
                    "] Navigate settings",
                    Style::default().fg(colors::TEXT_PRIMARY),
                ),
            ]),
            Line::from(vec![
                Span::styled("  [", Style::default().fg(colors::TEXT_DIM)),
                Span::styled("←→", Style::default().fg(colors::ACCENT_BRIGHT)),
                Span::styled("] Adjust values", Style::default().fg(colors::TEXT_PRIMARY)),
            ]),
            Line::from(""),
            Line::from(vec![Span::styled(
                "Actions:",
                Style::default()
                    .fg(colors::TEXT_DIM)
                    .add_modifier(Modifier::BOLD),
            )]),
            Line::from(vec![
                Span::styled("  [", Style::default().fg(colors::TEXT_DIM)),
                Span::styled("s", Style::default().fg(colors::ACCENT_BRIGHT)),
                Span::styled("] Save settings", Style::default().fg(colors::TEXT_PRIMARY)),
            ]),
            Line::from(vec![
                Span::styled("  [", Style::default().fg(colors::TEXT_DIM)),
                Span::styled("r", Style::default().fg(colors::ACCENT_BRIGHT)),
                Span::styled(
                    "] Refresh model list",
                    Style::default().fg(colors::TEXT_PRIMARY),
                ),
            ]),
        ];

        let paragraph = Paragraph::new(settings)
            .block(
                Block::default()
                    .title(format!(" Engine Configuration ({}) ", title_suffix))
                    .borders(Borders::ALL)
                    .border_style(if focused {
                        Style::default().fg(colors::ACCENT_BRIGHT)
                    } else {
                        Style::default().fg(colors::BORDER)
                    }),
            )
            .wrap(Wrap { trim: false });

        frame.render_widget(paragraph, area);
    }

    fn render_advanced_tab(&self, frame: &mut Frame, area: Rect) {
        let text = vec![
            Line::from(vec![Span::styled(
                "Advanced Settings",
                Style::default()
                    .fg(colors::TEXT_PRIMARY)
                    .add_modifier(Modifier::BOLD),
            )]),
            Line::from(""),
            Line::from(vec![Span::styled(
                "• Database Path: ~/.config/assist-mcp/routing.db",
                Style::default().fg(colors::TEXT_PRIMARY),
            )]),
            Line::from(vec![Span::styled(
                "• Model Cache: ~/.litert-lm/models/",
                Style::default().fg(colors::TEXT_PRIMARY),
            )]),
            Line::from(vec![Span::styled(
                "• Config Path: ~/.config/assist-mcp/config.toml",
                Style::default().fg(colors::TEXT_PRIMARY),
            )]),
            Line::from(""),
            Line::from(vec![Span::styled(
                "Environment Variables:",
                Style::default()
                    .fg(colors::TEXT_DIM)
                    .add_modifier(Modifier::BOLD),
            )]),
            Line::from(vec![Span::styled(
                "• DYLD_LIBRARY_PATH=/Users/rpm/LiteRT-LM/bazel-bin/rust_api",
                Style::default().fg(colors::TEXT_PRIMARY),
            )]),
            Line::from(vec![Span::styled(
                "• RUST_LOG=debug",
                Style::default().fg(colors::TEXT_PRIMARY),
            )]),
        ];

        let paragraph = Paragraph::new(text)
            .block(
                Block::default()
                    .title(" Advanced Configuration ")
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(colors::BORDER)),
            )
            .wrap(Wrap { trim: false });

        frame.render_widget(paragraph, area);
    }

    fn render_status_bar(&self, frame: &mut Frame, area: Rect) {
        let status_text = if self.loading {
            "Loading..."
        } else if let Some(ref msg) = self.status_message {
            msg.as_str()
        } else {
            "Ready"
        };

        let status = Paragraph::new(Line::from(vec![Span::styled(
            format!(" {} ", status_text),
            Style::default().fg(colors::TEXT_PRIMARY),
        )]))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(colors::BORDER)),
        );

        frame.render_widget(status, area);
    }
}
