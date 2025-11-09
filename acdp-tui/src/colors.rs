//! Helix-inspired color theme for MCP TUI

use ratatui::style::Color;

// Primary theme colors
pub const WHITE: Color = Color::Rgb(255, 255, 255);
pub const LILAC: Color = Color::Rgb(219, 191, 239);
pub const LAVENDER: Color = Color::Rgb(164, 160, 232);
pub const COMET: Color = Color::Rgb(90, 89, 119);
pub const BOSSANOVA: Color = Color::Rgb(69, 40, 89);
pub const MIDNIGHT: Color = Color::Rgb(59, 34, 76);
pub const REVOLVER: Color = Color::Rgb(40, 23, 51);

// Accent colors
pub const SILVER: Color = Color::Rgb(204, 204, 204);
pub const MINT: Color = Color::Rgb(159, 242, 143);
pub const ALMOND: Color = Color::Rgb(236, 205, 186);
pub const HONEY: Color = Color::Rgb(239, 186, 93);

// Status colors
pub const APRICOT: Color = Color::Rgb(244, 120, 104);
pub const LIGHTNING: Color = Color::Rgb(255, 205, 28);
pub const DELTA: Color = Color::Rgb(111, 68, 240);
pub const ZINC: Color = Color::Rgb(161, 161, 170);

// Semantic color mappings
pub const BACKGROUND: Color = REVOLVER;
pub const BACKGROUND_LIGHT: Color = MIDNIGHT;
pub const FOREGROUND: Color = LILAC;
pub const FOREGROUND_DIM: Color = COMET;

pub const BORDER: Color = BOSSANOVA;
pub const BORDER_FOCUSED: Color = DELTA;

pub const TEXT_PRIMARY: Color = LILAC;
pub const TEXT_SECONDARY: Color = LAVENDER;
pub const TEXT_DIM: Color = ZINC;

pub const STATUS_SUCCESS: Color = MINT;
pub const STATUS_WARNING: Color = HONEY;
pub const STATUS_ERROR: Color = APRICOT;
pub const STATUS_INFO: Color = LIGHTNING;

pub const ACCENT: Color = DELTA;
pub const ACCENT_BRIGHT: Color = LAVENDER;

pub const SELECTION: Color = BOSSANOVA;
pub const SELECTION_FOCUSED: Color = MIDNIGHT;

// Settings panel
pub const SETTINGS_BACKGROUND: Color = MIDNIGHT;
pub const SETTINGS_BORDER: Color = ACCENT; // Use bright accent color for contrast
