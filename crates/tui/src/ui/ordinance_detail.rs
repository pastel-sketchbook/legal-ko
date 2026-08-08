use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Wrap};

use crate::app::App;
use crate::theme::Theme;

use super::VERSION;
use super::styles;

pub fn render_ordinance_detail(f: &mut Frame, app: &App, theme: &Theme, area: Rect) {
    super::render_detail_shell(
        f,
        app,
        theme,
        area,
        render_detail_title,
        render_detail_content,
        render_detail_footer,
    );
}

fn render_detail_title(f: &mut Frame, app: &App, theme: &Theme, area: Rect) {
    let raw_title = app
        .ordinance_detail
        .as_ref()
        .map_or("Loading...", |d| d.entry.title.as_str());

    let title = styles::truncate_with_ellipsis(raw_title, 80);
    let title_style = styles::title_bar(theme);

    let mut parts = vec![Span::styled(format!(" {title}"), title_style)];

    if let Some(ref detail) = app.ordinance_detail {
        let meta = format!(
            " {} · {} · {} · {} ",
            detail.entry.rule_type, detail.entry.region, detail.entry.government, detail.entry.date
        );
        styles::push_version_label(&mut parts, theme, &meta, VERSION, area.width);
    }

    let line = Line::from(parts);
    let bar = Paragraph::new(line).style(title_style);
    f.render_widget(bar, area);
}

fn render_detail_content(f: &mut Frame, app: &App, theme: &Theme, area: Rect) {
    if app.ordinance_detail_loading {
        let p = Paragraph::new("Loading...")
            .style(Style::default().fg(theme.accent))
            .wrap(Wrap { trim: false });
        f.render_widget(p, area);
        return;
    }

    let lines = &app.ordinance_detail_rendered_lines;
    if lines.is_empty() {
        return;
    }

    let visible_height = area.height as usize;
    let scroll = app.ordinance_detail_scroll;

    let visible_lines: Vec<Line<'static>> = lines
        .iter()
        .skip(scroll)
        .take(visible_height)
        .cloned()
        .collect();

    let paragraph = Paragraph::new(visible_lines)
        .style(Style::default().fg(theme.fg).bg(theme.bg))
        .wrap(Wrap { trim: false });

    f.render_widget(paragraph, area);
}

fn render_detail_footer(f: &mut Frame, app: &App, theme: &Theme, area: Rect) {
    let content = if let Some(ref msg) = app.status_message {
        styles::status_message_line(theme, msg, area.width)
    } else {
        let scroll_info = if app.ordinance_detail_lines_count > 0 {
            let pct = if app.ordinance_detail_lines_count <= 1 {
                100
            } else {
                (app.ordinance_detail_scroll * 100)
                    / app.ordinance_detail_lines_count.saturating_sub(1)
            };
            format!(" {pct}% ")
        } else {
            String::new()
        };

        let pairs: Vec<(&str, &str)> = vec![
            ("j/k", "스크롤"),
            ("g/G", "처음/끝"),
            ("E", "내보내기"),
            ("t", "테마"),
            ("q/Esc", "돌아가기"),
            ("?", "도움말"),
        ];

        styles::status_line(theme, &scroll_info, &pairs, area.width)
    };

    let bar = Paragraph::new(content).style(styles::status_bar(theme));
    f.render_widget(bar, area);
}
