pub mod help;
pub mod law_detail;
pub mod law_list;
pub mod styles;

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, Paragraph};

use crate::app::{App, Popup, View};
use crate::theme::Theme;

use legal_ko_core::AGENTS;

/// Minimum terminal size (cols, rows)
const MIN_WIDTH: u16 = 40;
const MIN_HEIGHT: u16 = 10;

/// Main render function — dispatches to the appropriate view
pub fn render(f: &mut Frame, app: &App) {
    let area = f.area();
    let theme = app.theme();

    // Paint full background
    f.render_widget(
        Block::default().style(Style::default().bg(theme.bg).fg(theme.fg)),
        area,
    );

    // Minimum terminal size guard
    if area.width < MIN_WIDTH || area.height < MIN_HEIGHT {
        let msg = Paragraph::new(format!(
            "Terminal too small\nNeed {}x{}, have {}x{}",
            MIN_WIDTH, MIN_HEIGHT, area.width, area.height
        ))
        .style(Style::default().fg(theme.error));
        f.render_widget(msg, area);
        return;
    }

    match app.view {
        View::Loading => render_loading(f, app, theme, area),
        View::List => {
            law_list::render_law_list(f, app, theme, area);
            // Render popups on top
            match app.popup {
                Popup::Help => help::render_help(f, theme, area),
                Popup::CategoryFilter => {
                    render_filter_popup(f, app, theme, area, FilterKind::Category);
                }
                Popup::DepartmentFilter => {
                    render_filter_popup(f, app, theme, area, FilterKind::Department);
                }
                Popup::AgentPicker => render_agent_picker(f, app, theme, area),
                Popup::None | Popup::ArticleList => {}
            }
        }
        View::Detail => {
            law_detail::render_law_detail(f, app, theme, area);
            match app.popup {
                Popup::Help => help::render_help(f, theme, area),
                Popup::ArticleList => law_detail::render_article_popup(f, app, theme, area),
                Popup::AgentPicker => render_agent_picker(f, app, theme, area),
                Popup::None | Popup::CategoryFilter | Popup::DepartmentFilter => {}
            }
        }
    }
}

fn render_loading(f: &mut Frame, app: &App, theme: &Theme, area: Rect) {
    let msg = if let Some(ref err) = app.status_message {
        err.clone()
    } else {
        "Loading metadata...".to_string()
    };

    let paragraph = Paragraph::new(msg)
        .style(
            Style::default()
                .fg(theme.accent)
                .add_modifier(Modifier::BOLD),
        )
        .block(
            Block::default()
                .title(" legal-ko ")
                .borders(Borders::ALL)
                .style(Style::default().fg(theme.border).bg(theme.bg)),
        );

    f.render_widget(paragraph, area);
}

#[derive(Clone, Copy)]
enum FilterKind {
    Category,
    Department,
}

fn render_filter_popup(f: &mut Frame, app: &App, theme: &Theme, area: Rect, kind: FilterKind) {
    let popup_area = styles::centered_rect(40, 60, area);

    let (title, items_source, current_filter): (&str, &[String], &Option<String>) = match kind {
        FilterKind::Category => (
            " Category — 법령구분 ",
            &app.categories,
            &app.category_filter,
        ),
        FilterKind::Department => (
            " Department — 소관부처 ",
            &app.departments,
            &app.department_filter,
        ),
    };

    let mut items: Vec<ListItem> = Vec::new();

    // "All" option
    let all_style = if app.popup_selected == 0 {
        Style::default()
            .fg(theme.highlight_fg)
            .bg(theme.highlight_bg)
            .add_modifier(Modifier::BOLD)
    } else if current_filter.is_none() {
        Style::default()
            .fg(theme.accent)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(theme.fg)
    };
    items.push(ListItem::new(Line::from(Span::styled(
        "  All (전체)".to_string(),
        all_style,
    ))));

    for (i, item) in items_source.iter().enumerate() {
        let is_selected = app.popup_selected == i + 1;
        let is_active = current_filter.as_ref() == Some(item);
        let style = if is_selected {
            Style::default()
                .fg(theme.highlight_fg)
                .bg(theme.highlight_bg)
                .add_modifier(Modifier::BOLD)
        } else if is_active {
            Style::default()
                .fg(theme.accent)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(theme.fg)
        };
        items.push(ListItem::new(Line::from(Span::styled(
            format!("  {item}"),
            style,
        ))));
    }

    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .style(Style::default().fg(theme.accent).bg(theme.panel_bg));

    let list = List::new(items).block(block);

    f.render_widget(Clear, popup_area);
    f.render_widget(list, popup_area);
}

fn render_agent_picker(f: &mut Frame, app: &App, theme: &Theme, area: Rect) {
    let popup_area = styles::centered_rect(35, 30, area);

    let last_agent_name = app.last_agent_index.map(|i| AGENTS[i].name);

    let items: Vec<ListItem> = app
        .installed_agents
        .iter()
        .enumerate()
        .map(|(i, agent)| {
            let is_selected = i == app.popup_selected;
            let is_last_used = last_agent_name == Some(agent.name);

            let style = if is_selected {
                Style::default()
                    .fg(theme.highlight_fg)
                    .bg(theme.highlight_bg)
                    .add_modifier(Modifier::BOLD)
            } else if is_last_used {
                Style::default()
                    .fg(theme.accent)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(theme.fg)
            };

            let marker = if is_last_used { " *" } else { "" };
            ListItem::new(Line::from(Span::styled(
                format!("  {}{marker}", agent.name),
                style,
            )))
        })
        .collect();

    let block = Block::default()
        .title(" AI Agent ")
        .borders(Borders::ALL)
        .style(Style::default().fg(theme.accent).bg(theme.panel_bg));

    let list = List::new(items).block(block);

    f.render_widget(Clear, popup_area);
    f.render_widget(list, popup_area);
}
