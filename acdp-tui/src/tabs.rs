/// Tab management for the main TUI interface
use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Tabs as RatatuiTabs},
    Frame,
};

use crate::colors;

/// Main tabs in the TUI
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MainTab {
    Activity,
    Servers,
    Settings,
}

impl MainTab {
    /// Get all tabs in order
    pub fn all() -> &'static [MainTab] {
        &[MainTab::Activity, MainTab::Servers, MainTab::Settings]
    }

    /// Get tab title
    pub fn title(&self) -> &'static str {
        match self {
            MainTab::Activity => "Activity",
            MainTab::Servers => "Servers",
            MainTab::Settings => "Settings",
        }
    }

    /// Get tab index
    pub fn index(&self) -> usize {
        match self {
            MainTab::Activity => 0,
            MainTab::Servers => 1,
            MainTab::Settings => 2,
        }
    }

    /// Get next tab
    pub fn next(&self) -> MainTab {
        match self {
            MainTab::Activity => MainTab::Servers,
            MainTab::Servers => MainTab::Settings,
            MainTab::Settings => MainTab::Activity,
        }
    }

    /// Get previous tab
    pub fn prev(&self) -> MainTab {
        match self {
            MainTab::Activity => MainTab::Settings,
            MainTab::Servers => MainTab::Activity,
            MainTab::Settings => MainTab::Servers,
        }
    }

    /// Get tab from index
    pub fn from_index(index: usize) -> MainTab {
        match index {
            0 => MainTab::Activity,
            1 => MainTab::Servers,
            2 => MainTab::Settings,
            _ => MainTab::Activity,
        }
    }
}

/// Render tab bar
pub fn render_tab_bar(frame: &mut Frame, area: Rect, current_tab: MainTab) {
    let titles: Vec<&str> = MainTab::all().iter().map(|t| t.title()).collect();

    let tabs = RatatuiTabs::new(titles)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(colors::BORDER)),
        )
        .select(current_tab.index())
        .style(Style::default().fg(colors::TEXT_DIM))
        .highlight_style(
            Style::default()
                .fg(colors::ACCENT_BRIGHT)
                .add_modifier(Modifier::BOLD),
        );

    frame.render_widget(tabs, area);
}

/// Render contextual help for current tab
pub fn render_tab_help(current_tab: MainTab) -> String {
    match current_tab {
        MainTab::Activity => "↑/↓: scroll | Enter: details | c: clear | f: follow | /: search",
        MainTab::Servers => "↑/↓: select | Enter: details | r: routing mode | Space: start/stop",
        MainTab::Settings => "↑/↓: navigate | ←/→: adjust | Enter: edit | r: reset",
    }
    .to_string()
}
