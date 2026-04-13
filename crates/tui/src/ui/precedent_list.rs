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

    let title_style = styles::title_bar(theme);

    let mut parts: Vec<Span> = vec![Span::styled(
        " legal-ko \u{2014} 판례 ",
        title_style.add_modifier(Modifier::BOLD),
    )];

    if app.in_person_search_mode() {
        let found = app.person_search_results.len();
        let suffix = if app.person_search_active { "..." } else { "" };
        parts.push(Span::styled(
            format!(" [법조인 {found}{suffix}] "),
            title_style,
        ));
    } else {
        let filtered = app.precedent_filtered_indices.len();
        if filtered == total {
            parts.push(Span::styled(format!(" [{total}] "), title_style));
        } else {
            parts.push(Span::styled(format!(" [{filtered}/{total}] "), title_style));
        }
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

    // Right-align sort + theme + version labels
    styles::push_version_label(
        &mut parts,
        theme,
        app.precedent_sort_order.label(),
        VERSION,
        area.width,
    );

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
    // ── Person (법조인) search mode ──────────────────────────
    if app.person_search_active || !app.person_search_results.is_empty() {
        render_person_search_results(f, app, theme, area);
        return;
    }

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
    let court_w: usize = 14; // 대법원, XX지방법원, XX고등법원 etc.
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

            let display_name = styles::truncate_with_ellipsis(&entry.case_name, name_w);
            let name_col = styles::pad_to_width(&display_name, name_w);
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
        let visible = app.precedent_visible_count();
        let prefix = if visible == 0 {
            String::new()
        } else {
            format!(" {}/{visible} ", app.precedent_cursor() + 1)
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

/// Render person search results with an animated progress indicator.
fn render_person_search_results(f: &mut Frame, app: &App, theme: &Theme, area: Rect) {
    let results = &app.person_search_results;

    if results.is_empty() && app.person_search_active {
        // Show animated searching indicator
        let frames = ["..", "...", "....", ".....", "......"];
        let frame = (app.tick / 3) % frames.len();
        let msg = format!("법조인 검색 중{}", frames[frame]);
        let p = Paragraph::new(msg)
            .style(Style::default().fg(theme.accent))
            .block(Block::default().borders(Borders::NONE));
        f.render_widget(p, area);
        return;
    }

    let total_width = area.width as usize;

    // Column widths (same as normal list)
    let court_w: usize = 14;
    let case_type_w: usize = 10;
    let date_w: usize = 10;
    let gaps: usize = 3;
    let name_w = total_width.saturating_sub(court_w + case_type_w + date_w + gaps);

    // Header: searching indicator if still active
    let mut start_row = 0u16;
    if app.person_search_active {
        let frames = ["..", "...", "....", ".....", "......"];
        let frame = (app.tick / 3) % frames.len();
        let header = Line::from(vec![
            Span::styled(
                format!(" 법조인 검색 중{} ", frames[frame]),
                Style::default()
                    .fg(theme.accent)
                    .add_modifier(Modifier::ITALIC),
            ),
            Span::styled(
                format!("({} found) ", results.len()),
                Style::default().fg(theme.muted),
            ),
        ]);
        let header_p = Paragraph::new(header);
        if area.height > 0 {
            f.render_widget(header_p, Rect { height: 1, ..area });
            start_row = 1;
        }
    }

    let list_area = Rect {
        y: area.y + start_row,
        height: area.height.saturating_sub(start_row),
        ..area
    };
    let visible_height = list_area.height as usize;
    let selected = app.person_search_selected;

    // Calculate scroll offset so the selected item is visible
    let offset = if selected < app.person_search_offset {
        selected
    } else if selected >= app.person_search_offset + visible_height {
        selected.saturating_sub(visible_height).saturating_add(1)
    } else {
        app.person_search_offset
    };

    let items: Vec<ListItem> = results
        .iter()
        .enumerate()
        .skip(offset)
        .take(visible_height)
        .map(|(display_idx, entry)| {
            let is_selected = display_idx == selected;

            let display_name = styles::truncate_with_ellipsis(&entry.case_name, name_w);
            let name_col = styles::pad_to_width(&display_name, name_w);
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
    f.render_widget(list, list_area);
}
