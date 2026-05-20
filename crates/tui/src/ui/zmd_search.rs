use crate::app::{App, InputMode};
use crate::theme::Theme;
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Margin, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, Paragraph};

use super::styles;

pub fn render_zmd_search(f: &mut Frame, app: &App, theme: &Theme, area: Rect) {
    let chunks = Layout::vertical([
        Constraint::Length(1), // title bar
        Constraint::Length(1), // search input
        Constraint::Min(1),    // results list
        Constraint::Length(1), // footer
    ])
    .split(area);

    render_title_bar(f, app, theme, chunks[0]);
    render_search_input(f, app, theme, chunks[1]);
    render_results(
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
    let count = app.zmd_search_results.len();
    let title_style = styles::title_bar(theme);

    let parts = vec![
        Span::styled(" zmd search ", title_style.add_modifier(Modifier::BOLD)),
        Span::styled(format!(" [{count} results] "), title_style),
    ];

    let bar = Paragraph::new(Line::from(parts)).style(title_style);
    f.render_widget(bar, area);
}

fn render_search_input(f: &mut Frame, app: &App, theme: &Theme, area: Rect) {
    let is_searching = app.input_mode == InputMode::Search;
    let prefix = if is_searching { "/ " } else { "  " };
    let query = &app.zmd_search_query;

    let style = if is_searching {
        Style::default().fg(theme.search).bg(theme.panel_bg)
    } else {
        Style::default().fg(theme.muted).bg(theme.panel_bg)
    };

    let text = format!("{prefix}{query}");
    let bar = Paragraph::new(text).style(style);
    f.render_widget(bar, area);

    if is_searching {
        // Place cursor after the query text
        let cursor_x = area.x
            + prefix.len() as u16
            + unicode_width::UnicodeWidthStr::width(query.as_str()) as u16;
        f.set_cursor_position((cursor_x, area.y));
    }
}

fn render_results(f: &mut Frame, app: &App, theme: &Theme, area: Rect) {
    if app.zmd_search_results.is_empty() {
        let msg = if app.zmd_search_query.is_empty() {
            "Type to search across all indexed documents (laws, precedents, admrules, ordinances)"
        } else {
            "No results"
        };
        let p = Paragraph::new(msg)
            .style(Style::default().fg(theme.muted))
            .block(Block::default().borders(Borders::NONE));
        f.render_widget(p, area);
        return;
    }

    let visible_height = area.height as usize;
    // Adjust offset so selected item is visible
    let selected = app.zmd_search_selected;
    let offset = if selected < app.zmd_search_offset {
        selected
    } else if selected >= app.zmd_search_offset + visible_height {
        selected.saturating_sub(visible_height - 1)
    } else {
        app.zmd_search_offset
    };

    let items: Vec<ListItem> = app
        .zmd_search_results
        .iter()
        .skip(offset)
        .take(visible_height)
        .enumerate()
        .map(|(i, hit)| {
            let global_idx = offset + i;
            let is_selected = global_idx == selected;

            let collection_badge = match hit.collection.as_str() {
                "laws" => "[법률]",
                "precedents" => "[판례]",
                "admrules" => "[행정규칙]",
                "ordinances" => "[자치법규]",
                _ => "[?]",
            };

            let title = if hit.title.is_empty() {
                &hit.path
            } else {
                &hit.title
            };

            // Truncate title to fit
            let max_title = area.width.saturating_sub(14) as usize;
            let display_title: String = title.chars().take(max_title).collect();

            let style = if is_selected {
                Style::default()
                    .fg(theme.highlight_fg)
                    .bg(theme.highlight_bg)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(theme.fg).bg(theme.panel_bg)
            };

            let line = Line::from(vec![
                Span::styled(
                    format!("{collection_badge:<10}"),
                    if is_selected {
                        style
                    } else {
                        Style::default().fg(theme.category).bg(theme.panel_bg)
                    },
                ),
                Span::styled(display_title, style),
            ]);

            ListItem::new(line)
        })
        .collect();

    let list = List::new(items).block(Block::default().borders(Borders::NONE));
    f.render_widget(list, area);
}

fn render_footer(f: &mut Frame, app: &App, theme: &Theme, area: Rect) {
    let hints = if app.input_mode == InputMode::Search {
        "Esc:cancel  Enter:browse  Ctrl+J/K:navigate"
    } else {
        "/:search  Enter:open  q/Esc:back  j/k:navigate"
    };

    let footer = Paragraph::new(hints).style(Style::default().fg(theme.muted).bg(theme.panel_bg));
    f.render_widget(footer, area);
}
