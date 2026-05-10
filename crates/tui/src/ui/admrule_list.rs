use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Margin, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, Paragraph};

use crate::app::{App, InputMode, View};
use crate::theme::Theme;

use super::VERSION;
use super::styles;

pub fn render_admrule_list(f: &mut Frame, app: &App, theme: &Theme, area: Rect) {
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
    let total = app.all_admrules.len();
    let title_style = styles::title_bar(theme);

    let mut parts: Vec<Span> = vec![Span::styled(
        " legal-ko \u{2014} 행정규칙 ",
        title_style.add_modifier(Modifier::BOLD),
    )];

    let filtered = app.admrule_filtered_indices.len();
    if filtered == total {
        parts.push(Span::styled(format!(" [{total}] "), title_style));
    } else {
        parts.push(Span::styled(format!(" [{filtered}/{total}] "), title_style));
    }

    // Active filters
    if let Some(ref rt) = app.admrule_type_filter {
        parts.push(Span::styled(
            format!(" type:{rt} "),
            Style::default().fg(theme.category).bg(theme.panel_bg),
        ));
    }
    if let Some(ref agency) = app.admrule_agency_filter {
        parts.push(Span::styled(
            format!(" agency:{agency} "),
            Style::default().fg(theme.department).bg(theme.panel_bg),
        ));
    }

    if !app.admrules_loaded {
        parts.push(Span::styled(
            " loading... ",
            Style::default()
                .fg(theme.accent)
                .bg(theme.panel_bg)
                .add_modifier(Modifier::ITALIC),
        ));
    }

    styles::push_version_label(
        &mut parts,
        theme,
        app.admrule_sort_order.label(),
        VERSION,
        area.width,
    );

    let line = Line::from(parts);
    let bar = Paragraph::new(line).style(title_style);
    f.render_widget(bar, area);
}

fn render_search_bar(f: &mut Frame, app: &App, theme: &Theme, area: Rect) {
    let in_search = app.input_mode == InputMode::Search && app.view == View::AdmruleList;
    let content = if in_search {
        Line::from(vec![
            Span::styled(
                " / ",
                Style::default()
                    .fg(theme.search)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                app.admrule_search_query.as_str(),
                Style::default().fg(theme.search),
            ),
            Span::styled("\u{258c}", Style::default().fg(theme.search)),
        ])
    } else if app.admrule_search_query.is_empty() {
        Line::from("")
    } else {
        Line::from(vec![
            Span::styled(" / ", Style::default().fg(theme.muted)),
            Span::styled(
                app.admrule_search_query.as_str(),
                Style::default().fg(theme.fg),
            ),
        ])
    };

    let bar = Paragraph::new(content).style(Style::default().bg(theme.bg));
    f.render_widget(bar, area);
}

fn render_list(f: &mut Frame, app: &App, theme: &Theme, area: Rect) {
    if app.admrule_filtered_indices.is_empty() {
        let msg = if app.all_admrules.is_empty() {
            if app.admrules_loaded {
                "No admrules loaded — run `legal-ko-cli zmd admrules` first"
            } else {
                "Loading admrules..."
            }
        } else {
            "No matching admrules"
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
    let type_w: usize = 8;
    let agency_w: usize = 16;
    let date_w: usize = 10;
    let gaps: usize = 3;
    let name_w = total_width.saturating_sub(type_w + agency_w + date_w + gaps);

    let offset = if app.admrule_list_selected < app.admrule_list_offset {
        app.admrule_list_selected
    } else if app.admrule_list_selected >= app.admrule_list_offset + visible_height {
        app.admrule_list_selected
            .saturating_sub(visible_height)
            .saturating_add(1)
    } else {
        app.admrule_list_offset
    };

    let items: Vec<ListItem> = app
        .admrule_filtered_indices
        .iter()
        .enumerate()
        .skip(offset)
        .take(visible_height)
        .map(|(display_idx, &idx)| {
            let entry = &app.all_admrules[idx];
            let is_selected = display_idx == app.admrule_list_selected;

            let display_name = styles::truncate_with_ellipsis(&entry.title, name_w);
            let name_col = styles::pad_to_width(&display_name, name_w);
            let type_text = format!("[{}]", entry.rule_type);
            let type_col = styles::pad_to_width(&type_text, type_w);
            let agency_col = styles::pad_to_width(&entry.agency, agency_w);
            let date_col = styles::pad_to_width(&entry.date, date_w);

            let name_style = styles::list_item_style(theme, is_selected, false);

            let spans = vec![
                Span::styled(name_col, name_style),
                Span::styled(" ", Style::default()),
                Span::styled(type_col, Style::default().fg(theme.category)),
                Span::styled(" ", Style::default()),
                Span::styled(agency_col, Style::default().fg(theme.department)),
                Span::styled(" ", Style::default()),
                Span::styled(date_col, Style::default().fg(theme.date)),
            ];

            let item = ListItem::new(Line::from(spans));
            if !is_selected && display_idx % 2 == 1 {
                item.style(Style::default().bg(theme.stripe_bg))
            } else {
                item
            }
        })
        .collect();

    let list = List::new(items);
    f.render_widget(list, area);
}

fn render_footer(f: &mut Frame, app: &App, theme: &Theme, area: Rect) {
    let content = if let Some(ref msg) = app.status_message {
        styles::status_message_line(theme, msg, area.width)
    } else {
        let filtered = app.admrule_filtered_indices.len();
        let prefix = if filtered == 0 {
            String::new()
        } else {
            format!(" {}/{filtered} ", app.admrule_list_selected + 1)
        };

        let pairs: Vec<(&str, &str)> = vec![
            ("j/k", "이동"),
            ("Enter", "열기"),
            ("/", "검색"),
            ("c", "종류"),
            ("d", "소관부처"),
            ("S", "정렬"),
            ("Tab", "다음"),
            ("t", "테마"),
            ("q", "종료"),
            ("?", "도움말"),
        ];

        styles::status_line(theme, &prefix, &pairs, area.width)
    };

    let bar = Paragraph::new(content).style(styles::status_bar(theme));
    f.render_widget(bar, area);
}
