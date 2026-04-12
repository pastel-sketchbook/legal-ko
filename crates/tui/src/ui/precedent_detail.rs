use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Margin, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, Paragraph, Wrap};
use unicode_width::UnicodeWidthStr;

use crate::app::App;
use crate::theme::Theme;

use super::VERSION;
use super::styles;

pub fn render_precedent_detail(f: &mut Frame, app: &App, theme: &Theme, area: Rect) {
    let chunks = Layout::vertical([
        Constraint::Length(1), // title bar
        Constraint::Min(1),    // content
        Constraint::Length(1), // status / footer bar
    ])
    .split(area);

    render_detail_title(f, app, theme, chunks[0]);
    render_detail_content(
        f,
        app,
        theme,
        chunks[1].inner(Margin {
            vertical: 0,
            horizontal: 2,
        }),
    );
    render_detail_footer(f, app, theme, chunks[2]);
}

fn render_detail_title(f: &mut Frame, app: &App, theme: &Theme, area: Rect) {
    let raw_title = app
        .precedent_detail
        .as_ref()
        .map_or("Loading...", |d| d.entry.case_name.as_str());

    // Truncate long case names to 80 display-width columns
    let title = styles::truncate_with_ellipsis(raw_title, 80);

    let title_style = styles::title_bar(theme);

    let mut parts = vec![Span::styled(format!(" {title}"), title_style)];

    // Build right-side metadata: court · case_type · date  vX.Y.Z
    let mut right_parts: Vec<Span<'static>> = Vec::new();

    if let Some(ref detail) = app.precedent_detail {
        let court = &detail.entry.court_name;
        let case_type = &detail.entry.case_type;
        let date = &detail.entry.ruling_date;

        if !court.is_empty() {
            right_parts.push(Span::styled(
                court.clone(),
                Style::default().fg(theme.department).bg(theme.panel_bg),
            ));
        }
        if !case_type.is_empty() {
            if !right_parts.is_empty() {
                right_parts.push(Span::styled(
                    " \u{b7} ",
                    Style::default().fg(theme.muted).bg(theme.panel_bg),
                ));
            }
            right_parts.push(Span::styled(
                case_type.clone(),
                Style::default().fg(theme.category).bg(theme.panel_bg),
            ));
        }
        if !date.is_empty() {
            if !right_parts.is_empty() {
                right_parts.push(Span::styled(
                    " \u{b7} ",
                    Style::default().fg(theme.muted).bg(theme.panel_bg),
                ));
            }
            right_parts.push(Span::styled(
                date.clone(),
                Style::default().fg(theme.date).bg(theme.panel_bg),
            ));
        }
    }

    let version_label = format!(" v{VERSION} ");

    // Measure widths for right-alignment
    let left_width: usize = parts.iter().map(|s| s.content.width()).sum();
    let meta_width: usize = right_parts.iter().map(|s| s.content.width()).sum();
    let version_width = UnicodeWidthStr::width(version_label.as_str());
    let right_total = meta_width + version_width + if meta_width > 0 { 2 } else { 0 };
    let total = area.width as usize;
    let gap = total.saturating_sub(left_width + right_total);

    if gap > 0 {
        parts.push(Span::styled(
            " ".repeat(gap),
            Style::default().bg(theme.panel_bg),
        ));
    }

    if !right_parts.is_empty() {
        parts.extend(right_parts);
        parts.push(Span::styled(" ", Style::default().bg(theme.panel_bg)));
    }

    parts.push(Span::styled(
        version_label,
        Style::default().fg(theme.muted).bg(theme.panel_bg),
    ));

    let line = Line::from(parts);
    let bar = Paragraph::new(line).style(title_style);
    f.render_widget(bar, area);
}

fn render_detail_content(f: &mut Frame, app: &App, theme: &Theme, area: Rect) {
    if app.precedent_detail_loading {
        let loading = Paragraph::new("Loading precedent content...")
            .style(
                Style::default()
                    .fg(theme.accent)
                    .add_modifier(Modifier::BOLD),
            )
            .block(Block::default().borders(Borders::NONE));
        f.render_widget(loading, area);
        return;
    }

    let Some(ref _detail) = app.precedent_detail else {
        let msg = Paragraph::new("No content loaded").style(Style::default().fg(theme.muted));
        f.render_widget(msg, area);
        return;
    };

    let lines: Vec<Line<'_>> = app
        .precedent_detail_rendered_lines
        .iter()
        .map(|line| {
            let spans: Vec<Span<'_>> = line
                .spans
                .iter()
                .map(|span| {
                    Span::styled(
                        std::borrow::Cow::Borrowed(span.content.as_ref()),
                        span.style,
                    )
                })
                .collect();
            Line::from(spans)
        })
        .collect();

    let scroll_y = source_line_to_wrapped_offset(&lines, app.precedent_detail_scroll, area.width);
    let paragraph = Paragraph::new(lines)
        .scroll((scroll_y, 0))
        .wrap(Wrap { trim: false });

    f.render_widget(paragraph, area);
}

fn render_detail_footer(f: &mut Frame, app: &App, theme: &Theme, area: Rect) {
    let content = if let Some(ref msg) = app.status_message {
        styles::status_message_line(theme, msg, area.width)
    } else {
        let scroll_info = if app.precedent_detail_lines_count > 0 {
            format!(
                " {}/{} ",
                app.precedent_detail_scroll + 1,
                app.precedent_detail_lines_count
            )
        } else {
            String::new()
        };

        let section_count = if app.precedent_detail_sections.is_empty() {
            String::new()
        } else {
            format!("{} sections ", app.precedent_detail_sections.len())
        };

        let prefix = format!("{scroll_info}{section_count}");

        let mut pairs: Vec<(&str, &str)> = Vec::new();
        if !app.precedent_detail_sections.is_empty() {
            pairs.push(("n/p", "section"));
            pairs.push(("a", "section list"));
        }
        pairs.push(("E", "export"));
        pairs.push(("t", "theme"));
        pairs.push(("o", "AI agent"));
        pairs.push(("Esc", "back"));
        pairs.push(("?", "help"));

        styles::status_line(theme, &prefix, &pairs, area.width)
    };

    let bar = Paragraph::new(content).style(styles::status_bar(theme));
    f.render_widget(bar, area);
}

/// Convert a source-line index into a wrapped-line offset (same logic as law_detail).
fn source_line_to_wrapped_offset(lines: &[Line<'_>], source_line: usize, width: u16) -> u16 {
    let w = width as usize;
    if w == 0 {
        return u16::try_from(source_line).unwrap_or(u16::MAX);
    }

    let mut wrapped: usize = 0;
    for line in lines.iter().take(source_line) {
        let line_width = line.width();
        if line_width <= w {
            wrapped += 1;
        } else {
            wrapped += line_width.div_ceil(w);
        }
    }

    u16::try_from(wrapped).unwrap_or(u16::MAX)
}

/// Render the section list popup
pub fn render_section_popup(f: &mut Frame, app: &App, theme: &Theme, area: Rect) {
    let popup_area = styles::centered_rect(50, 50, area);

    let items: Vec<ListItem> = app
        .precedent_detail_sections
        .iter()
        .enumerate()
        .map(|(i, sec)| {
            let style = styles::list_item_style(theme, i == app.popup_selected, false);
            ListItem::new(Line::from(Span::styled(format!("  {}", sec.label), style)))
        })
        .collect();

    let block = Block::default()
        .title(" Sections \u{2014} 섹션 목록 ")
        .borders(Borders::ALL)
        .style(Style::default().fg(theme.accent).bg(theme.panel_bg));

    let list = List::new(items).block(block);

    let clear_area = Rect {
        x: popup_area.x.saturating_sub(1),
        y: popup_area.y,
        width: popup_area.width.saturating_add(2),
        height: popup_area.height,
    };
    f.render_widget(Clear, clear_area);
    f.render_widget(list, popup_area);
}
