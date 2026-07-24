//! TUI theme. Auburn Orange (#E86100) is the single accent color; everything
//! else is neutral gray for secondary text plus green/red for status.
//!
//! Auburn navy was dropped on purpose: at #0B2341 it renders as near-black in a
//! terminal and just read as muddy, so it added no signal.

use ratatui::style::{Color, Modifier, Style};

/// The one accent color.
pub const ORANGE: Color = Color::Rgb(0xE8, 0x61, 0x00);
/// Neutral gray for secondary / dim text (no blue tint).
pub const GRAY: Color = Color::Rgb(0x9E, 0x9E, 0x9E);
/// Status colors.
pub const GOOD: Color = Color::Rgb(0x4C, 0xAF, 0x50);
pub const BAD: Color = Color::Rgb(0xE5, 0x53, 0x4B);

/// Titles and headings — the accent.
pub fn title() -> Style {
    Style::new().fg(ORANGE).add_modifier(Modifier::BOLD)
}

/// Box borders — the accent, unbolded.
pub fn border() -> Style {
    Style::new().fg(ORANGE)
}

/// Secondary / helper text.
pub fn dim() -> Style {
    Style::new().fg(GRAY)
}

/// Selected element: bold black text on an orange block (max contrast, works on
/// both light and dark terminals).
pub fn highlight() -> Style {
    Style::new().fg(Color::Black).bg(ORANGE).add_modifier(Modifier::BOLD)
}

pub fn good() -> Style {
    Style::new().fg(GOOD)
}

pub fn bad() -> Style {
    Style::new().fg(BAD)
}
