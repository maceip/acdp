use anyhow::{Context, Result};
use crossterm::event::{
    self, Event as CrosstermEvent, KeyCode, KeyEvent, KeyEventKind, KeyModifiers,
};
use tokio::task;
use tracing::warn;

/// High level events understood by the application.
#[derive(Debug, Clone)]
pub enum Event {
    Quit,
    /// Escape key - used to exit modals/settings
    Escape,
    /// Textual input from the user.
    Input(char),
    Enter,
    Backspace,
    Tab,
    FocusNext,
    FocusPrev,
    Up,
    Down,
    Left,
    Right,
    /// Cycle through proxy selection
    CycleProxy,
    /// Start a new proxy
    StartProxy,
}

/// Blocking event reader wrapped for async callers.
pub struct EventHandler;

impl EventHandler {
    pub fn new() -> Self {
        Self
    }

    /// Try to read next event with timeout (non-blocking)
    pub async fn try_next(&mut self, timeout: std::time::Duration) -> Result<Option<Event>> {
        // Use poll with timeout to avoid blocking
        let available = match task::spawn_blocking(move || event::poll(timeout))
            .await
            .context("failed to join event poll task")?
        {
            Ok(available) => available,
            Err(err) => {
                warn!("event poll failed: {err}");
                return Ok(None);
            }
        };

        if !available {
            return Ok(None);
        }

        // Event is available, read it immediately (won't block)
        loop {
            let event = match task::spawn_blocking(event::read)
                .await
                .context("failed to join tui event reader task")?
            {
                Ok(event) => event,
                Err(err) => {
                    warn!("tui event reader unavailable: {err}");
                    return Ok(Some(Event::Quit));
                }
            };

            if let Some(app_event) = map_event(event) {
                return Ok(Some(app_event));
            }
        }
    }

    /// Blocking read of next event (for backwards compatibility)
    pub async fn next(&mut self) -> Result<Event> {
        loop {
            let event = match task::spawn_blocking(event::read)
                .await
                .context("failed to join tui event reader task")?
            {
                Ok(event) => event,
                Err(err) => {
                    warn!("tui event reader unavailable: {err}");
                    return Ok(Event::Quit);
                }
            };

            if let Some(app_event) = map_event(event) {
                return Ok(app_event);
            }
        }
    }
}

fn map_event(event: CrosstermEvent) -> Option<Event> {
    match event {
        CrosstermEvent::Key(KeyEvent {
            code,
            modifiers,
            kind,
            ..
        }) => {
            if kind != KeyEventKind::Press {
                return None;
            }
            match code {
                KeyCode::Esc => Some(Event::Escape),
                KeyCode::Enter => Some(Event::Enter),
                KeyCode::Tab => {
                    if modifiers.contains(KeyModifiers::SHIFT) {
                        Some(Event::FocusPrev)
                    } else if modifiers.contains(KeyModifiers::CONTROL) {
                        Some(Event::FocusNext)
                    } else {
                        Some(Event::Tab)
                    }
                }
                KeyCode::BackTab => Some(Event::FocusPrev),
                KeyCode::Backspace => Some(Event::Backspace),
                KeyCode::Left => Some(Event::Left),
                KeyCode::Right => Some(Event::Right),
                KeyCode::Up => Some(Event::Up),
                KeyCode::Down => Some(Event::Down),
                KeyCode::Char('c') | KeyCode::Char('q')
                    if modifiers.contains(KeyModifiers::CONTROL) =>
                {
                    Some(Event::Quit)
                }
                KeyCode::Char('p') | KeyCode::Char('P')
                    if modifiers.contains(KeyModifiers::CONTROL) =>
                {
                    // Ctrl+P triggers proxy cycling
                    Some(Event::CycleProxy)
                }
                KeyCode::Char('n') | KeyCode::Char('N')
                    if modifiers.contains(KeyModifiers::CONTROL) =>
                {
                    // Ctrl+N triggers starting a new proxy
                    Some(Event::StartProxy)
                }
                KeyCode::Char(character) => Some(Event::Input(character)),
                _ => None,
            }
        }
        _ => None,
    }
}
