//! Auburn University brand colors and shared styles.
//! Per the lab's color directory — Auburn Orange #E86100, Auburn Blue #0B2341.

use ratatui::style::{Color, Modifier, Style};

pub const ORANGE: Color = Color::Rgb(0xE8, 0x61, 0x00);
pub const NAVY: Color = Color::Rgb(0x0B, 0x23, 0x41);

pub fn navy() -> Style {
    Style::new().fg(NAVY).add_modifier(Modifier::BOLD)
}
/// Lighter navy-tinted gray for secondary text (pure navy is unreadable on dark terminals).
pub const SLATE: Color = Color::Rgb(0x8A, 0x9B, 0xB0);
pub const GOOD: Color = Color::Rgb(0x4C, 0xAF, 0x50);
pub const BAD: Color = Color::Rgb(0xE5, 0x53, 0x4B);

pub fn title() -> Style {
    Style::new().fg(ORANGE).add_modifier(Modifier::BOLD)
}

pub fn border() -> Style {
    Style::new().fg(ORANGE)
}

pub fn dim() -> Style {
    Style::new().fg(SLATE)
}

/// Highlighted/selected element: navy text on an orange block.
pub fn highlight() -> Style {
    Style::new().fg(NAVY).bg(ORANGE).add_modifier(Modifier::BOLD)
}

pub fn good() -> Style {
    Style::new().fg(GOOD)
}

pub fn bad() -> Style {
    Style::new().fg(BAD)
}
