use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Margin, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, Paragraph};
use unicode_width::UnicodeWidthStr;

use crate::app::{App, InputMode};
use crate::theme::Theme;

use super::styles;

/// Pad or truncate `s` so its display width is exactly `target_width`.
fn pad_to_width(s: &str, target_width: usize) -> String {
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

pub fn render_law_list(f: &mut Frame, app: &App, theme: &Theme, area: Rect) {
    let chunks = Layout::vertical([
        Constraint::Length(1), // title bar
        Constraint::Length(1), // search / filter bar
        Constraint::Min(1),    // list
        Constraint::Length(1), // status / footer bar
    ])
    .split(area);

    render_title_bar(f, app, theme, chunks[0]);
    render_search_bar(f, app, theme, chunks[1]);
    render_list(
        f,
        app,
        theme,
        chunks[2].inner(Margin {
            vertical: 0,
            horizontal: 2,
        }),
    );
    render_footer(f, app, theme, chunks[3]);
}

fn render_title_bar(f: &mut Frame, app: &App, theme: &Theme, area: Rect) {
    let total = app.all_laws.len();
    let filtered = app.filtered_indices.len();

    let title_style = styles::title_bar(theme);

    let mut parts: Vec<Span> = vec![Span::styled(
        " legal-ko ",
        title_style.add_modifier(Modifier::BOLD),
    )];

    if filtered == total {
        parts.push(Span::styled(format!(" [{total}] "), title_style));
    } else {
        parts.push(Span::styled(format!(" [{filtered}/{total}] "), title_style));
    }

    // Active filters
    if let Some(ref cat) = app.category_filter {
        parts.push(Span::styled(
            format!(" cat:{cat} "),
            Style::default().fg(theme.category).bg(theme.panel_bg),
        ));
    }
    if let Some(ref dept) = app.department_filter {
        parts.push(Span::styled(
            format!(" dept:{dept} "),
            Style::default().fg(theme.department).bg(theme.panel_bg),
        ));
    }
    if app.bookmarks_only {
        parts.push(Span::styled(
            " \u{2605} bookmarks ",
            Style::default().fg(theme.bookmark).bg(theme.panel_bg),
        ));
    }

    let line = Line::from(parts);
    let bar = Paragraph::new(line).style(title_style);
    f.render_widget(bar, area);
}

fn render_search_bar(f: &mut Frame, app: &App, theme: &Theme, area: Rect) {
    let content = if app.input_mode == InputMode::Search {
        Line::from(vec![
            Span::styled(
                " / ",
                Style::default()
                    .fg(theme.search)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(app.search_query.clone(), Style::default().fg(theme.search)),
            Span::styled("\u{258c}", Style::default().fg(theme.search)),
        ])
    } else if app.search_query.is_empty() {
        Line::from("")
    } else {
        Line::from(vec![
            Span::styled(" / ", Style::default().fg(theme.muted)),
            Span::styled(app.search_query.clone(), Style::default().fg(theme.fg)),
        ])
    };

    let bar = Paragraph::new(content).style(Style::default().bg(theme.bg));
    f.render_widget(bar, area);
}

fn render_list(f: &mut Frame, app: &App, theme: &Theme, area: Rect) {
    if app.filtered_indices.is_empty() {
        let msg = if app.all_laws.is_empty() {
            "No laws loaded"
        } else {
            "No matching laws"
        };
        let p = Paragraph::new(msg)
            .style(Style::default().fg(theme.muted))
            .block(Block::default().borders(Borders::NONE));
        f.render_widget(p, area);
        return;
    }

    let visible_height = area.height as usize;
    let total_width = area.width as usize;

    // Adaptive departments column: only show when at least one filtered entry has data
    let show_dept = app
        .filtered_indices
        .iter()
        .any(|&idx| !app.all_laws[idx].departments.is_empty());

    let bookmark_w: usize = 2;
    let cat_w: usize = 14; // fits brackets + longest category (e.g. "[대통령령]" = 10 display width)
    let dept_w: usize = if show_dept { 16 } else { 0 };
    let gaps: usize = 1 + usize::from(show_dept); // 1 gap before cat, 1 before dept (if shown)
    let title_w = total_width.saturating_sub(bookmark_w + cat_w + dept_w + gaps);

    // Calculate the offset so the selected item is visible
    let offset = if app.list_selected < app.list_offset {
        app.list_selected
    } else if app.list_selected >= app.list_offset + visible_height {
        app.list_selected
            .saturating_sub(visible_height)
            .saturating_add(1)
    } else {
        app.list_offset
    };

    let items: Vec<ListItem> = app
        .filtered_indices
        .iter()
        .enumerate()
        .skip(offset)
        .take(visible_height)
        .map(|(display_idx, &law_idx)| {
            let entry = &app.all_laws[law_idx];
            let is_selected = display_idx == app.list_selected;
            let is_bookmarked = app.bookmarks.is_bookmarked(&entry.id);

            let bookmark_marker = if is_bookmarked { "\u{2605} " } else { "  " };

            let title_col = pad_to_width(&entry.title, title_w);
            let cat_text = format!("[{}]", entry.category);
            let cat_col = pad_to_width(&cat_text, cat_w);

            let title_style = if is_selected {
                Style::default()
                    .fg(theme.highlight_fg)
                    .bg(theme.highlight_bg)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(theme.fg)
            };

            let mut spans = vec![
                Span::styled(
                    bookmark_marker.to_string(),
                    Style::default().fg(theme.bookmark),
                ),
                Span::styled(title_col, title_style),
                Span::styled(" ", Style::default()),
                Span::styled(cat_col, Style::default().fg(theme.category)),
            ];

            if show_dept {
                let dept_text = entry.departments.join(", ");
                let dept_col = pad_to_width(&dept_text, dept_w);
                spans.push(Span::styled(" ", Style::default()));
                spans.push(Span::styled(
                    dept_col,
                    Style::default().fg(theme.department),
                ));
            }

            ListItem::new(Line::from(spans))
        })
        .collect();

    let list = List::new(items);
    f.render_widget(list, area);
}

fn render_footer(f: &mut Frame, app: &App, theme: &Theme, area: Rect) {
    let content = if let Some(ref msg) = app.status_message {
        styles::status_message_line(theme, msg, area.width)
    } else {
        let prefix = if app.filtered_indices.is_empty() {
            String::new()
        } else {
            format!(" {}/{} ", app.list_selected + 1, app.filtered_indices.len())
        };

        let pairs: Vec<(&str, &str)> = vec![
            ("j/k", "navigate"),
            ("Enter", "open"),
            ("/", "search"),
            ("c", "category"),
            ("d", "department"),
            ("b", "bookmarks"),
            ("B", "bookmark"),
            #[cfg(feature = "tts")]
            ("T", "tts profile"),
            ("t", "theme"),
            ("o", "AI agent"),
            ("q", "quit"),
            ("?", "help"),
        ];

        styles::status_line(theme, &prefix, &pairs, area.width)
    };

    let bar = Paragraph::new(content).style(styles::status_bar(theme));
    f.render_widget(bar, area);
}
