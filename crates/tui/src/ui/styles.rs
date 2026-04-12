use ratatui::layout::{Constraint, Flex, Layout, Rect};
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

/// Build a status bar Line with an optional prefix and badge-style shortcuts.
#[must_use]
pub fn status_line<'a>(
    theme: &Theme,
    prefix: &str,
    pairs: &[(&str, &str)],
    width: u16,
) -> Line<'a> {
    let mut spans: Vec<Span<'a>> = Vec::new();

    if !prefix.is_empty() {
        spans.push(Span::styled(
            prefix.to_string(),
            Style::default().fg(theme.muted),
        ));
    }

    for (key, desc) in pairs {
        spans.extend(key_badge(theme, key, desc));
    }

    // Fill remaining space with panel background
    let used: usize = spans.iter().map(|s| s.content.width()).sum();
    let total = width as usize;
    let gap = total.saturating_sub(used);
    if gap > 0 {
        spans.push(Span::styled(
            " ".repeat(gap),
            Style::default().bg(theme.panel_bg),
        ));
    }

    Line::from(spans)
}

/// Title bar style (bg bar with bold title)
#[must_use]
pub fn title_bar(theme: &Theme) -> Style {
    Style::default()
        .fg(theme.title)
        .bg(theme.panel_bg)
        .add_modifier(Modifier::BOLD)
}

/// Status / footer bar style
#[must_use]
pub fn status_bar(theme: &Theme) -> Style {
    Style::default().fg(theme.fg).bg(theme.panel_bg)
}

/// Style for a selectable list item with up to three states.
///
/// - `selected`: the cursor is on this item → highlighted background + bold.
/// - `active`: item is the current/active choice (e.g. active filter, last-used
///   agent) → accent colour + bold.
/// - Otherwise: default foreground.
#[must_use]
pub fn list_item_style(theme: &Theme, selected: bool, active: bool) -> Style {
    if selected {
        Style::default()
            .fg(theme.highlight_fg)
            .bg(theme.highlight_bg)
            .add_modifier(Modifier::BOLD)
    } else if active {
        Style::default()
            .fg(theme.accent)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(theme.fg)
    }
}

/// Pad or truncate `s` so its display width is exactly `target_width`.
///
/// Uses Unicode-aware column width measurement. Double-width CJK characters
/// that would overflow `target_width` are skipped and the remaining space is
/// filled with ASCII spaces.
#[must_use]
pub fn pad_to_width(s: &str, target_width: usize) -> String {
    let w = UnicodeWidthStr::width(s);
    if w >= target_width {
        // Truncate to fit
        let mut result = String::new();
        let mut current = 0;
        for ch in s.chars() {
            let cw = unicode_width::UnicodeWidthChar::width(ch).unwrap_or(0);
            if current + cw > target_width {
                break;
            }
            result.push(ch);
            current += cw;
        }
        // Fill remaining with spaces (e.g. if a double-width char was skipped)
        while current < target_width {
            result.push(' ');
            current += 1;
        }
        result
    } else {
        let padding = target_width - w;
        format!("{s}{}", " ".repeat(padding))
    }
}

/// Build a status bar Line with a left message.
#[must_use]
pub fn status_message_line<'a>(theme: &Theme, msg: &str, width: u16) -> Line<'a> {
    let left = format!(" {msg}");
    let left_w = UnicodeWidthStr::width(left.as_str());
    let total = width as usize;
    let gap = total.saturating_sub(left_w);

    Line::from(vec![
        Span::styled(left, Style::default().fg(theme.accent)),
        Span::styled(" ".repeat(gap), Style::default().bg(theme.panel_bg)),
    ])
}

/// Create a centered rect using percentage of the parent area.
#[must_use]
pub fn centered_rect(percent_x: u16, percent_y: u16, area: Rect) -> Rect {
    let vertical = Layout::vertical([Constraint::Percentage(percent_y)])
        .flex(Flex::Center)
        .split(area);
    Layout::horizontal([Constraint::Percentage(percent_x)])
        .flex(Flex::Center)
        .split(vertical[0])[0]
}

/// Truncate a string to fit within `max_width` display columns.
///
/// If the string exceeds `max_width`, it is cut and an ellipsis (`…`, 1 column)
/// is appended. Handles multi-byte / double-width characters (e.g. Korean)
/// correctly via `UnicodeWidthChar`.
#[must_use]
pub fn truncate_with_ellipsis(s: &str, max_width: usize) -> String {
    use unicode_width::UnicodeWidthChar;

    let full_width = UnicodeWidthStr::width(s);
    if full_width <= max_width {
        return s.to_string();
    }

    let mut result = String::new();
    let mut width = 0;
    let limit = max_width.saturating_sub(1); // reserve 1 col for "…"
    for ch in s.chars() {
        let cw = ch.width().unwrap_or(0);
        if width + cw > limit {
            break;
        }
        result.push(ch);
        width += cw;
    }
    result.push('\u{2026}'); // …
    result
}

/// Append right-aligned `[sort] [theme] vX.Y.Z` labels to `spans`.
///
/// `width` is the total available columns. The function measures the existing
/// spans, computes the gap, and pushes a spacer + label spans.
pub fn push_version_label(
    spans: &mut Vec<Span<'static>>,
    theme: &Theme,
    sort_label: &str,
    version: &str,
    width: u16,
) {
    let sort_text = format!(" {sort_label} ");
    let theme_text = format!(" {} ", theme.name);
    let version_text = format!(" v{version} ");
    let left_width: usize = spans.iter().map(|s| s.content.width()).sum();
    let right_width = sort_text.width() + theme_text.width() + version_text.width();
    let total = width as usize;
    let gap = total.saturating_sub(left_width + right_width);
    if gap > 0 {
        spans.push(Span::styled(
            " ".repeat(gap),
            Style::default().bg(theme.panel_bg),
        ));
    }
    spans.push(Span::styled(
        sort_text,
        Style::default().fg(theme.muted).bg(theme.panel_bg),
    ));
    spans.push(Span::styled(
        theme_text,
        Style::default().fg(theme.accent).bg(theme.panel_bg),
    ));
    spans.push(Span::styled(
        version_text,
        Style::default().fg(theme.muted).bg(theme.panel_bg),
    ));
}
