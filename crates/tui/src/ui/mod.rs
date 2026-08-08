pub mod admrule_detail;
pub mod admrule_list;
pub mod help;
pub mod law_detail;
pub mod law_list;
pub mod ordinance_detail;
pub mod ordinance_list;
pub mod precedent_detail;
pub mod precedent_list;
pub mod styles;
pub mod zmd_search;

use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Margin, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, Paragraph};

use crate::app::{App, Popup, View};
use crate::theme::Theme;

use legal_ko_core::AGENTS;

/// Application version, embedded at compile time from the workspace VERSION file.
const VERSION: &str = include_str!("../../../../VERSION").trim_ascii();

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

    render_view(f, app, theme, area);
    render_popup(f, app, theme, area);
}

/// Render the main content area based on the current view.
fn render_view(f: &mut Frame, app: &App, theme: &Theme, area: Rect) {
    match app.view {
        View::Loading => render_loading(f, app, theme, area),
        View::List => {
            if app.split_open {
                render_split_view(f, app, theme, area);
            } else {
                law_list::render_law_list(f, app, theme, area);
            }
        }
        View::Detail => law_detail::render_law_detail(f, app, theme, area),
        View::PrecedentList => precedent_list::render_precedent_list(f, app, theme, area),
        View::PrecedentDetail => precedent_detail::render_precedent_detail(f, app, theme, area),
        View::AdmruleList => admrule_list::render_admrule_list(f, app, theme, area),
        View::AdmruleDetail => admrule_detail::render_admrule_detail(f, app, theme, area),
        View::OrdinanceList => ordinance_list::render_ordinance_list(f, app, theme, area),
        View::OrdinanceDetail => ordinance_detail::render_ordinance_detail(f, app, theme, area),
        View::ZmdSearch => zmd_search::render_zmd_search(f, app, theme, area),
    }
}

/// Shared 4-row list layout (title / search / list / footer).
///
/// The list views (law, precedent, admrule, ordinance, zmd search) all use the
/// same vertical shell; only the four render callbacks differ.
#[allow(clippy::too_many_arguments)] // one argument per render callback
pub(crate) fn render_list_shell(
    f: &mut Frame,
    app: &App,
    theme: &Theme,
    area: Rect,
    title: fn(&mut Frame, &App, &Theme, Rect),
    search: fn(&mut Frame, &App, &Theme, Rect),
    list: fn(&mut Frame, &App, &Theme, Rect),
    footer: fn(&mut Frame, &App, &Theme, Rect),
) {
    let chunks = Layout::vertical([
        Constraint::Length(1), // title bar
        Constraint::Length(1), // search / filter bar
        Constraint::Min(1),    // list
        Constraint::Length(1), // status / footer bar
    ])
    .split(area);

    title(f, app, theme, chunks[0]);
    search(f, app, theme, chunks[1]);
    list(
        f,
        app,
        theme,
        chunks[2].inner(Margin {
            vertical: 0,
            horizontal: 2,
        }),
    );
    footer(f, app, theme, chunks[3]);
}

/// Shared 3-row detail layout (title / content / footer).
///
/// The detail views (law, precedent, admrule, ordinance) all use the same
/// vertical shell; only the three render callbacks differ.
pub(crate) fn render_detail_shell(
    f: &mut Frame,
    app: &App,
    theme: &Theme,
    area: Rect,
    title: fn(&mut Frame, &App, &Theme, Rect),
    content: fn(&mut Frame, &App, &Theme, Rect),
    footer: fn(&mut Frame, &App, &Theme, Rect),
) {
    let chunks = Layout::vertical([
        Constraint::Length(1), // title bar
        Constraint::Min(1),    // content
        Constraint::Length(1), // status / footer bar
    ])
    .split(area);

    title(f, app, theme, chunks[0]);
    content(
        f,
        app,
        theme,
        chunks[1].inner(Margin {
            vertical: 0,
            horizontal: 2,
        }),
    );
    footer(f, app, theme, chunks[2]);
}

/// Render popup overlays on top of the main view.
fn render_popup(f: &mut Frame, app: &App, theme: &Theme, area: Rect) {
    match app.popup {
        Popup::None => {}
        Popup::Help => help::render_help(f, theme, area),
        Popup::AgentPicker => render_agent_picker(f, app, theme, area),
        Popup::ExportFormat => render_export_format(f, app, theme, area),
        Popup::CategoryFilter => render_filter_popup(f, app, theme, area, FilterKind::Category),
        Popup::DepartmentFilter => render_filter_popup(f, app, theme, area, FilterKind::Department),
        Popup::CaseTypeFilter => render_filter_popup(f, app, theme, area, FilterKind::CaseType),
        Popup::CourtFilter => render_filter_popup(f, app, theme, area, FilterKind::Court),
        Popup::AdmruleTypeFilter => {
            render_filter_popup(f, app, theme, area, FilterKind::AdmruleType);
        }
        Popup::AdmruleAgencyFilter => {
            render_filter_popup(f, app, theme, area, FilterKind::AdmruleAgency);
        }
        Popup::OrdinanceTypeFilter => {
            render_filter_popup(f, app, theme, area, FilterKind::OrdinanceType);
        }
        Popup::OrdinanceRegionFilter => {
            render_filter_popup(f, app, theme, area, FilterKind::OrdinanceRegion);
        }
        Popup::ArticleList => law_detail::render_article_popup(f, app, theme, area),
        Popup::SectionList => precedent_detail::render_section_popup(f, app, theme, area),
        Popup::CrossRefList => precedent_detail::render_crossref_popup(f, app, theme, area),
    }
}

fn render_loading(f: &mut Frame, app: &App, theme: &Theme, area: Rect) {
    let msg = match app.status_message {
        Some(ref err) => err.as_str(),
        None => "Loading metadata...",
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
    CaseType,
    Court,
    AdmruleType,
    AdmruleAgency,
    OrdinanceType,
    OrdinanceRegion,
}

fn render_filter_popup(f: &mut Frame, app: &App, theme: &Theme, area: Rect, kind: FilterKind) {
    let popup_area = styles::centered_rect(40, 60, area);

    let (title, items_source, current_filter): (&str, &[String], Option<&String>) = match kind {
        FilterKind::Category => (
            " Category \u{2014} 법령구분 ",
            &app.categories,
            app.category_filter.as_ref(),
        ),
        FilterKind::Department => (
            " Department \u{2014} 소관부처 ",
            &app.departments,
            app.department_filter.as_ref(),
        ),
        FilterKind::CaseType => (
            " Case \u{2014} 사건종류 ",
            &app.precedent_case_types,
            app.precedent_case_type_filter.as_ref(),
        ),
        FilterKind::Court => (
            " Court \u{2014} 법원 ",
            &app.precedent_courts,
            app.precedent_court_filter.as_ref(),
        ),
        FilterKind::AdmruleType => (
            " Type \u{2014} 행정규칙종류 ",
            &app.admrule_types,
            app.admrule_type_filter.as_ref(),
        ),
        FilterKind::AdmruleAgency => (
            " Agency \u{2014} 소관부처 ",
            &app.admrule_agencies,
            app.admrule_agency_filter.as_ref(),
        ),
        FilterKind::OrdinanceType => (
            " Type \u{2014} 자치법규종류 ",
            &app.ordinance_types,
            app.ordinance_type_filter.as_ref(),
        ),
        FilterKind::OrdinanceRegion => (
            " Region \u{2014} 광역 ",
            &app.ordinance_regions,
            app.ordinance_region_filter.as_ref(),
        ),
    };

    let mut items: Vec<ListItem> = Vec::new();

    // "All" option
    let all_style =
        styles::list_item_style(theme, app.popup_selected == 0, current_filter.is_none());
    items.push(ListItem::new(Line::from(Span::styled(
        "  All (전체)".to_string(),
        all_style,
    ))));

    for (i, item) in items_source.iter().enumerate() {
        let is_selected = app.popup_selected == i + 1;
        let is_active = current_filter == Some(item);
        let style = styles::list_item_style(theme, is_selected, is_active);
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
    let mut state = ratatui::widgets::ListState::default().with_selected(Some(app.popup_selected));

    f.render_widget(Clear, styles::clear_area_for_popup(popup_area));
    f.render_stateful_widget(list, popup_area, &mut state);
}

/// Render list + detail side-by-side with a draggable split.
fn render_split_view(f: &mut Frame, app: &App, theme: &Theme, area: Rect) {
    // Value is clamped to 20.0..=80.0, safe to cast to u16.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let left_pct = (app.split_ratio * 100.0).round().clamp(20.0, 80.0) as u16;
    let right_pct = 100 - left_pct;
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(left_pct),
            Constraint::Percentage(right_pct),
        ])
        .split(area);

    law_list::render_law_list(f, app, theme, cols[0]);
    law_detail::render_law_detail(f, app, theme, cols[1]);
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

            let style = styles::list_item_style(theme, is_selected, is_last_used);

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

    f.render_widget(Clear, styles::clear_area_for_popup(popup_area));
    f.render_widget(list, popup_area);
}

fn render_export_format(f: &mut Frame, app: &App, theme: &Theme, area: Rect) {
    let popup_area = styles::centered_rect(25, 15, area);

    let labels = App::export_format_labels();
    let items: Vec<ListItem> = labels
        .iter()
        .enumerate()
        .map(|(i, label)| {
            let is_selected = i == app.popup_selected;
            let style = styles::list_item_style(theme, is_selected, false);
            ListItem::new(Line::from(Span::styled(format!("  {label}"), style)))
        })
        .collect();

    let block = Block::default()
        .title(" Export Format ")
        .borders(Borders::ALL)
        .style(Style::default().fg(theme.accent).bg(theme.panel_bg));

    let list = List::new(items).block(block);

    f.render_widget(Clear, styles::clear_area_for_popup(popup_area));
    f.render_widget(list, popup_area);
}

/// Render 법조인 search results (precedent entries) in place of the normal
/// list content. Used by `law_list`, `admrule_list`, `ordinance_list`, and
/// `zmd_search` when person search is active.
pub fn render_person_search_results(f: &mut Frame, app: &App, theme: &Theme, area: Rect) {
    use ratatui::widgets::{List, ListItem};

    let results = &app.person_search_results;

    if results.is_empty() && app.person_search_active {
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
    let court_w: usize = 14;
    let case_type_w: usize = 10;
    let date_w: usize = 10;
    let gaps: usize = 3;
    let name_w = total_width.saturating_sub(court_w + case_type_w + date_w + gaps);

    let mut start_row = 0u16;
    if !results.is_empty() {
        let mut header_spans: Vec<Span> = if app.person_search_active {
            let frames = ["..", "...", "....", ".....", "......"];
            let frame = (app.tick / 3) % frames.len();
            vec![
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
            ]
        } else {
            vec![Span::styled(
                format!(" 법조인 ({}개) ", results.len()),
                Style::default().fg(theme.accent),
            )]
        };
        header_spans.push(Span::styled(
            format!("정렬:{} ", app.person_search_sort_order.label()),
            Style::default().fg(theme.tag),
        ));
        let header = Line::from(header_spans);
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

            let item = ListItem::new(Line::from(spans));
            if !is_selected && display_idx % 2 == 1 {
                item.style(Style::default().bg(theme.stripe_bg))
            } else {
                item
            }
        })
        .collect();

    let list = List::new(items);
    f.render_widget(list, list_area);
}
