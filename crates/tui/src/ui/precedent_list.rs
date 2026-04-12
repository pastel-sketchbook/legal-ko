use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Margin, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, Paragraph};

use crate::app::{App, InputMode, View};
use crate::theme::Theme;

use super::VERSION;
use super::styles;

pub fn render_precedent_list(f: &mut Frame, app: &App, theme: &Theme, area: Rect) {
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
    let total = app.all_precedents.len();
    let filtered = app.precedent_filtered_indices.len();

    let title_style = styles::title_bar(theme);

    let mut parts: Vec<Span> = vec![Span::styled(
        " legal-ko \u{2014} 판례 ",
        title_style.add_modifier(Modifier::BOLD),
    )];

    if filtered == total {
        parts.push(Span::styled(format!(" [{total}] "), title_style));
    } else {
        parts.push(Span::styled(format!(" [{filtered}/{total}] "), title_style));
    }

    // Active filters
    if let Some(ref ct) = app.precedent_case_type_filter {
        parts.push(Span::styled(
            format!(" type:{ct} "),
            Style::default().fg(theme.category).bg(theme.panel_bg),
        ));
    }
    if let Some(ref court) = app.precedent_court_filter {
        parts.push(Span::styled(
            format!(" court:{court} "),
            Style::default().fg(theme.department).bg(theme.panel_bg),
        ));
    }

    if !app.precedents_loaded {
        parts.push(Span::styled(
            " loading... ",
            Style::default()
                .fg(theme.accent)
                .bg(theme.panel_bg)
                .add_modifier(Modifier::ITALIC),
        ));
    }

    // Right-align version label
    styles::push_version_label(&mut parts, theme, VERSION, area.width);

    let line = Line::from(parts);
    let bar = Paragraph::new(line).style(title_style);
    f.render_widget(bar, area);
}

fn render_search_bar(f: &mut Frame, app: &App, theme: &Theme, area: Rect) {
    let in_precedent_search =
        app.input_mode == InputMode::Search && app.view == View::PrecedentList;
    let content = if in_precedent_search {
        Line::from(vec![
            Span::styled(
                " / ",
                Style::default()
                    .fg(theme.search)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                app.precedent_search_query.as_str(),
                Style::default().fg(theme.search),
            ),
            Span::styled("\u{258c}", Style::default().fg(theme.search)),
        ])
    } else if app.precedent_search_query.is_empty() {
        Line::from("")
    } else {
        Line::from(vec![
            Span::styled(" / ", Style::default().fg(theme.muted)),
            Span::styled(
                app.precedent_search_query.as_str(),
                Style::default().fg(theme.fg),
            ),
        ])
    };

    let bar = Paragraph::new(content).style(Style::default().bg(theme.bg));
    f.render_widget(bar, area);
}

fn render_list(f: &mut Frame, app: &App, theme: &Theme, area: Rect) {
    if app.precedent_filtered_indices.is_empty() {
        let msg = if app.all_precedents.is_empty() {
            if app.precedents_loaded {
                "No precedents loaded"
            } else {
                "Loading precedents..."
            }
        } else {
            "No matching precedents"
        };
        let p = Paragraph::new(msg)
            .style(Style::default().fg(theme.muted))
            .block(Block::default().borders(Borders::NONE));
        f.render_widget(p, area);
        return;
    }

    let visible_height = area.height as usize;
    let total_width = area.width as usize;

    // Column widths
    let court_w: usize = 6; // 대법원/하급심
    let case_type_w: usize = 10;
    let date_w: usize = 10; // YYYY-MM-DD
    let gaps: usize = 3; // spaces between columns
    let name_w = total_width.saturating_sub(court_w + case_type_w + date_w + gaps);

    // Calculate the offset so the selected item is visible
    let offset = if app.precedent_list_selected < app.precedent_list_offset {
        app.precedent_list_selected
    } else if app.precedent_list_selected >= app.precedent_list_offset + visible_height {
        app.precedent_list_selected
            .saturating_sub(visible_height)
            .saturating_add(1)
    } else {
        app.precedent_list_offset
    };

    let items: Vec<ListItem> = app
        .precedent_filtered_indices
        .iter()
        .enumerate()
        .skip(offset)
        .take(visible_height)
        .map(|(display_idx, &prec_idx)| {
            let entry = &app.all_precedents[prec_idx];
            let is_selected = display_idx == app.precedent_list_selected;

            let name_col = styles::pad_to_width(&entry.case_name, name_w);
            let court_col = styles::pad_to_width(&entry.court_name, court_w);
            let type_text = format!("[{}]", entry.case_type);
            let type_col = styles::pad_to_width(&type_text, case_type_w);
            let date_col = styles::pad_to_width(&entry.ruling_date, date_w);

            let name_style = styles::list_item_style(theme, is_selected, false);

            let spans = vec![
                Span::styled(name_col, name_style),
                Span::styled(" ", Style::default()),
                Span::styled(court_col, Style::default().fg(theme.department)),
                Span::styled(" ", Style::default()),
                Span::styled(type_col, Style::default().fg(theme.category)),
                Span::styled(" ", Style::default()),
                Span::styled(date_col, Style::default().fg(theme.date)),
            ];

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
        let prefix = if app.precedent_filtered_indices.is_empty() {
            String::new()
        } else {
            format!(
                " {}/{} ",
                app.precedent_list_selected + 1,
                app.precedent_filtered_indices.len()
            )
        };

        let pairs: Vec<(&str, &str)> = vec![
            ("j/k", "navigate"),
            ("Enter", "open"),
            ("/", "search"),
            ("c", "case"),
            ("d", "court"),
            ("S", "sort"),
            ("Tab", "laws"),
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
