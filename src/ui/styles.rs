use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use unicode_width::UnicodeWidthStr;

use crate::theme::Theme;

/// Badge-style key hint: e.g.  ` q ` quit
fn key_badge<'a>(theme: &Theme, key: &str, desc: &str) -> Vec<Span<'a>> {
    vec![
        Span::styled(
            format!(" {key} "),
            Style::default().fg(theme.key_fg).bg(theme.key_bg),
        ),
        Span::styled(format!(" {desc} "), Style::default().fg(theme.muted)),
    ]
}

/// Build a status bar Line with an optional prefix, badge-style shortcuts,
/// and the theme name right-aligned.
pub fn status_line<'a>(
    theme: &Theme,
    prefix: &str,
    pairs: &[(&str, &str)],
    width: u16,
) -> Line<'a> {
    let mut left_spans: Vec<Span<'a>> = Vec::new();

    if !prefix.is_empty() {
        left_spans.push(Span::styled(
            prefix.to_string(),
            Style::default().fg(theme.muted),
        ));
    }

    for (key, desc) in pairs {
        left_spans.extend(key_badge(theme, key, desc));
    }

    let theme_label = format!(" {} ", theme.name);

    // Calculate left content width
    let left_width: usize = left_spans.iter().map(|s| s.content.width()).sum();
    let right_width = theme_label.width();
    let total = width as usize;

    let mut spans = left_spans;

    // Add spacer to push theme name to the right
    let gap = total.saturating_sub(left_width + right_width);
    if gap > 0 {
        spans.push(Span::styled(
            " ".repeat(gap),
            Style::default().bg(theme.panel_bg),
        ));
    }

    spans.push(Span::styled(
        theme_label,
        Style::default().fg(theme.muted).bg(theme.panel_bg),
    ));

    Line::from(spans)
}

/// Title bar style (bg bar with bold title)
pub fn title_bar(theme: &Theme) -> Style {
    Style::default()
        .fg(theme.title)
        .bg(theme.panel_bg)
        .add_modifier(Modifier::BOLD)
}

/// Status / footer bar style
pub fn status_bar(theme: &Theme) -> Style {
    Style::default().fg(theme.fg).bg(theme.panel_bg)
}

/// Build a status bar Line with a left message and the theme name right-aligned.
pub fn status_message_line<'a>(theme: &Theme, msg: &str, width: u16) -> Line<'a> {
    let left = format!(" {msg}");
    let theme_label = format!(" {} ", theme.name);
    let left_w = UnicodeWidthStr::width(left.as_str());
    let right_w = theme_label.width();
    let total = width as usize;
    let gap = total.saturating_sub(left_w + right_w);

    Line::from(vec![
        Span::styled(left, Style::default().fg(theme.accent)),
        Span::styled(" ".repeat(gap), Style::default().bg(theme.panel_bg)),
        Span::styled(
            theme_label,
            Style::default().fg(theme.muted).bg(theme.panel_bg),
        ),
    ])
}
