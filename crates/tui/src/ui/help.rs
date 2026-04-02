use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};

use crate::theme::Theme;

use super::styles;

/// Render a help overlay popup showing all keybindings
pub fn render_help(f: &mut Frame, theme: &Theme, area: Rect) {
    let popup_area = styles::centered_rect(60, 80, area);

    let mut help_lines = vec![
        header_line(theme, "Navigation"),
        key_line(theme, "j / \u{2193}", "Move down"),
        key_line(theme, "k / \u{2191}", "Move up"),
        key_line(theme, "g / Home", "Go to top"),
        key_line(theme, "G / End", "Go to bottom"),
        key_line(theme, "Ctrl+d", "Page down"),
        key_line(theme, "Ctrl+u", "Page up"),
        key_line(theme, "Enter", "Open selected law"),
        key_line(theme, "Esc / q", "Back / Quit"),
        Line::from(""),
        header_line(theme, "Search & Filter"),
        key_line(theme, "/", "Search laws"),
        key_line(theme, "Esc", "Clear search"),
        key_line(theme, "c", "Filter by category"),
        key_line(theme, "d", "Filter by department"),
        Line::from(""),
        header_line(theme, "Detail View"),
        key_line(theme, "n", "Next article (\u{c81c}X\u{c870})"),
        key_line(theme, "p", "Previous article"),
        key_line(theme, "a", "Article list popup"),
        Line::from(""),
    ];

    #[cfg(feature = "tts")]
    {
        help_lines.push(header_line(theme, "Text-to-Speech"));
        help_lines.push(key_line(theme, "r", "Read current article aloud"));
        help_lines.push(key_line(theme, "R", "Read full law aloud"));
        help_lines.push(key_line(theme, "s", "Stop TTS playback"));
        help_lines.push(key_line(theme, "T", "Toggle TTS profile (Fast/Balanced)"));
        help_lines.push(Line::from(""));
    }

    help_lines.extend([
        header_line(theme, "Bookmarks"),
        key_line(theme, "B", "Toggle bookmark"),
        key_line(theme, "b", "Show bookmarks only"),
        Line::from(""),
        header_line(theme, "Other"),
        key_line(theme, "t", "Cycle theme"),
        key_line(theme, "?", "Toggle this help"),
        key_line(theme, "q", "Quit"),
    ]);

    let block = Block::default()
        .title(" Help \u{2014} Keybindings ")
        .borders(Borders::ALL)
        .style(Style::default().fg(theme.accent).bg(theme.panel_bg));

    let paragraph = Paragraph::new(help_lines).block(block);

    f.render_widget(Clear, popup_area);
    f.render_widget(paragraph, popup_area);
}

fn header_line(theme: &Theme, title: &str) -> Line<'static> {
    Line::from(vec![Span::styled(
        format!("  {title}"),
        Style::default()
            .fg(theme.accent)
            .add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
    )])
}

fn key_line(theme: &Theme, key: &str, desc: &str) -> Line<'static> {
    Line::from(vec![
        Span::styled(
            format!("    {key:<14}"),
            Style::default()
                .fg(theme.search)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(desc.to_string(), Style::default().fg(theme.fg)),
    ])
}
