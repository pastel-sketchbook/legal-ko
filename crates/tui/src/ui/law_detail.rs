use ratatui::Frame;
use ratatui::layout::{Constraint, Flex, Layout, Margin, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, Paragraph, Wrap};

use legal_ko_core::tts::TtsState;

use crate::app::App;
use crate::parser;
use crate::theme::Theme;

use super::styles;

pub fn render_law_detail(f: &mut Frame, app: &App, theme: &Theme, area: Rect) {
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
    let title = app
        .detail
        .as_ref()
        .map(|d| d.entry.title.as_str())
        .unwrap_or("Loading...");

    let bookmark_marker = app
        .detail
        .as_ref()
        .map(|d| {
            if app.bookmarks.is_bookmarked(&d.entry.id) {
                " \u{2605}"
            } else {
                ""
            }
        })
        .unwrap_or("");

    let title_style = styles::title_bar(theme);

    let line = Line::from(vec![
        Span::styled(format!(" {title}"), title_style),
        Span::styled(
            bookmark_marker.to_string(),
            Style::default()
                .fg(theme.bookmark)
                .bg(theme.panel_bg)
                .add_modifier(Modifier::BOLD),
        ),
    ]);

    let bar = Paragraph::new(line).style(title_style);
    f.render_widget(bar, area);
}

fn render_detail_content(f: &mut Frame, app: &App, theme: &Theme, area: Rect) {
    if app.detail_loading {
        let loading = Paragraph::new("Loading law content...")
            .style(
                Style::default()
                    .fg(theme.accent)
                    .add_modifier(Modifier::BOLD),
            )
            .block(Block::default().borders(Borders::NONE));
        f.render_widget(loading, area);
        return;
    }

    let Some(ref detail) = app.detail else {
        let msg = Paragraph::new("No content loaded").style(Style::default().fg(theme.muted));
        f.render_widget(msg, area);
        return;
    };

    let (mut lines, _) = parser::parse_law_markdown(&detail.raw_markdown, theme);

    // Apply TTS highlight to lines of the currently-playing article
    if let Some((hl_start, hl_end)) = app.tts_highlight_lines() {
        let hl_bg = theme.highlight_bg;
        for (i, line) in lines.iter_mut().enumerate() {
            if i >= hl_start && i < hl_end {
                // Re-style each span to add the highlight background
                let new_spans: Vec<Span<'static>> = line
                    .spans
                    .iter()
                    .map(|span| Span::styled(span.content.to_string(), span.style.bg(hl_bg)))
                    .collect();
                *line = Line::from(new_spans);
            }
        }
    }

    let paragraph = Paragraph::new(lines)
        .scroll((app.detail_scroll as u16, 0))
        .wrap(Wrap { trim: false });

    f.render_widget(paragraph, area);
}

fn render_detail_footer(f: &mut Frame, app: &App, theme: &Theme, area: Rect) {
    let content = if let Some(ref msg) = app.status_message {
        styles::status_message_line(theme, msg, area.width)
    } else {
        let scroll_info = if app.detail_lines_count > 0 {
            format!(" {}/{} ", app.detail_scroll + 1, app.detail_lines_count)
        } else {
            String::new()
        };

        let article_count = if !app.detail_articles.is_empty() {
            format!("{} articles ", app.detail_articles.len())
        } else {
            String::new()
        };

        let tts_indicator = match app.tts_state {
            TtsState::Loading => {
                // Animated loading bar: a sliding highlight in a dot/pipe pattern
                // Pattern cycles: ·· shifts a bright segment across ··|··|··|··
                let frames = [
                    "\u{2590}··|··|··|··\u{258c}",
                    "·\u{2590}·|··|··|··\u{258c}",
                    "··\u{2590}··|··|··|\u{258c}",
                    "··|\u{2590}·|··|··|\u{258c}",
                    "··|·\u{2590}|··|··|\u{258c}",
                    "··|··\u{2590}··|··|\u{258c}",
                    "··|··|\u{2590}·|··|\u{258c}",
                    "··|··|·\u{2590}|··|\u{258c}",
                    "··|··|··\u{2590}··|\u{258c}",
                    "··|··|··|\u{2590}·|\u{258c}",
                    "··|··|··|·\u{2590}|\u{258c}",
                    "··|··|··|··\u{2590}\u{258c}",
                ];
                // Slow down: advance frame every ~3 ticks (~150ms per frame)
                let frame = (app.tick / 3) % frames.len();
                format!("{} ", frames[frame])
            }
            TtsState::Synthesizing => "\u{1f50a}\u{2026} ".to_string(),
            TtsState::Playing => "\u{25b6}\u{fe0f} ".to_string(),
            _ => String::new(),
        };

        let prefix = format!("{tts_indicator}{scroll_info}{article_count}");

        let mut pairs: Vec<(&str, &str)> = Vec::new();
        if !app.detail_articles.is_empty() {
            pairs.push(("n/p", "article"));
            pairs.push(("a", "article list"));
        }
        // TTS key hints
        match app.tts_state {
            TtsState::Playing | TtsState::Synthesizing => {
                pairs.push(("s", "stop"));
            }
            _ => {
                pairs.push(("r", "read article"));
                pairs.push(("R", "read all"));
            }
        }
        pairs.push(("B", "bookmark"));
        pairs.push(("t", "theme"));
        pairs.push(("Esc", "back"));
        pairs.push(("?", "help"));

        styles::status_line(theme, &prefix, &pairs, area.width)
    };

    let bar = Paragraph::new(content).style(styles::status_bar(theme));
    f.render_widget(bar, area);
}

/// Render the article list popup
pub fn render_article_popup(f: &mut Frame, app: &App, theme: &Theme, area: Rect) {
    let popup_area = centered_rect(50, 70, area);

    let items: Vec<ListItem> = app
        .detail_articles
        .iter()
        .enumerate()
        .map(|(i, art)| {
            let style = if i == app.popup_selected {
                Style::default()
                    .fg(theme.highlight_fg)
                    .bg(theme.highlight_bg)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(theme.fg)
            };
            ListItem::new(Line::from(Span::styled(format!("  {}", art.label), style)))
        })
        .collect();

    let block = Block::default()
        .title(" Articles \u{2014} \u{c870}\u{d56d} \u{baa9}\u{b85d} ")
        .borders(Borders::ALL)
        .style(Style::default().fg(theme.accent).bg(theme.panel_bg));

    let list = List::new(items).block(block);

    f.render_widget(Clear, popup_area);
    f.render_widget(list, popup_area);
}

/// Create a centered rect
fn centered_rect(percent_x: u16, percent_y: u16, area: Rect) -> Rect {
    let vertical = Layout::vertical([Constraint::Percentage(percent_y)])
        .flex(Flex::Center)
        .split(area);
    Layout::horizontal([Constraint::Percentage(percent_x)])
        .flex(Flex::Center)
        .split(vertical[0])[0]
}
