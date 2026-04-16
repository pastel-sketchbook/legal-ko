use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Margin, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, Paragraph, Wrap};
use unicode_width::UnicodeWidthStr;

#[cfg(feature = "tts")]
use legal_ko_core::tts::TtsState;

use crate::app::App;
use crate::theme::Theme;

use super::VERSION;
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
        .map_or("Loading...", |d| d.entry.title.as_str());

    let bookmark_marker = app.detail.as_ref().map_or("", |d| {
        if app.bookmarks.is_bookmarked(&d.entry.id) {
            " \u{2605}"
        } else {
            ""
        }
    });

    let title_style = styles::title_bar(theme);

    let mut parts = vec![
        Span::styled(format!(" {title}"), title_style),
        Span::styled(
            bookmark_marker.to_string(),
            Style::default()
                .fg(theme.bookmark)
                .bg(theme.panel_bg)
                .add_modifier(Modifier::BOLD),
        ),
    ];

    // Build right-side metadata: department · date · 판례 count  vX.Y.Z
    let mut right_parts: Vec<Span<'static>> = Vec::new();

    if let Some(ref detail) = app.detail {
        // Precedent count badge
        if let Some(ref map) = app.precedent_map {
            let count = map.law_count(&detail.entry.title);
            if count > 0 {
                right_parts.push(Span::styled(
                    format!("{count}판"),
                    Style::default().fg(theme.accent).bg(theme.panel_bg),
                ));
            }
        }

        let dept = detail.entry.departments.join(", ");
        let date = &detail.entry.promulgation_date;

        if !dept.is_empty() {
            if !right_parts.is_empty() {
                right_parts.push(Span::styled(
                    " · ",
                    Style::default().fg(theme.muted).bg(theme.panel_bg),
                ));
            }
            right_parts.push(Span::styled(
                dept,
                Style::default().fg(theme.department).bg(theme.panel_bg),
            ));
        }
        if !date.is_empty() {
            if !right_parts.is_empty() {
                right_parts.push(Span::styled(
                    " · ",
                    Style::default().fg(theme.muted).bg(theme.panel_bg),
                ));
            }
            right_parts.push(Span::styled(
                date.clone(),
                Style::default().fg(theme.date).bg(theme.panel_bg),
            ));
        }
    }

    let sort_label = format!(" {} ", app.sort_order.label());
    let theme_label = format!(" {} ", theme.name);
    let version_label = format!(" v{VERSION} ");

    // Measure widths for right-alignment
    let left_width: usize = parts.iter().map(|s| s.content.width()).sum();
    let meta_width: usize = right_parts.iter().map(|s| s.content.width()).sum();
    let sort_width = UnicodeWidthStr::width(sort_label.as_str());
    let theme_width = UnicodeWidthStr::width(theme_label.as_str());
    let version_width = UnicodeWidthStr::width(version_label.as_str());
    // Add 2 for the space before metadata and space before sort/theme/version
    let right_total =
        meta_width + sort_width + theme_width + version_width + if meta_width > 0 { 2 } else { 0 };
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
        sort_label,
        Style::default().fg(theme.muted).bg(theme.panel_bg),
    ));
    parts.push(Span::styled(
        theme_label,
        Style::default().fg(theme.accent).bg(theme.panel_bg),
    ));
    parts.push(Span::styled(
        version_label,
        Style::default().fg(theme.muted).bg(theme.panel_bg),
    ));

    let line = Line::from(parts);
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

    let Some(ref _detail) = app.detail else {
        let msg = Paragraph::new("No content loaded").style(Style::default().fg(theme.muted));
        f.render_widget(msg, area);
        return;
    };

    // Build lines for the paragraph by borrowing span content from the
    // cached `detail_rendered_lines`.  `Paragraph` requires owned `Text`,
    // but individual spans can use `Cow::Borrowed` to avoid deep-copying
    // the string data every frame.
    //
    // When TTS highlighting is active, highlighted spans get a bg override
    // (still borrowed content, only the style changes).
    #[cfg(feature = "tts")]
    let highlight_range = app.tts_highlight_lines();
    #[cfg(not(feature = "tts"))]
    let highlight_range: Option<(usize, usize)> = None;

    let lines: Vec<Line<'_>> = if let Some((hl_start, hl_end)) = highlight_range {
        let hl_bg = theme.highlight_bg;
        app.detail_rendered_lines
            .iter()
            .enumerate()
            .map(|(i, line)| {
                let spans: Vec<Span<'_>> = line
                    .spans
                    .iter()
                    .map(|span| {
                        let style = if i >= hl_start && i < hl_end {
                            span.style.bg(hl_bg)
                        } else {
                            span.style
                        };
                        Span::styled(std::borrow::Cow::Borrowed(span.content.as_ref()), style)
                    })
                    .collect();
                Line::from(spans)
            })
            .collect()
    } else {
        app.detail_rendered_lines
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
            .collect()
    };

    let scroll_y = source_line_to_wrapped_offset(&lines, app.detail_scroll, area.width);
    let paragraph = Paragraph::new(lines)
        .scroll((scroll_y, 0))
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

        let article_count = if app.detail_articles.is_empty() {
            String::new()
        } else {
            format!("{} articles ", app.detail_articles.len())
        };

        #[cfg(feature = "tts")]
        let tts_indicator = match app.tts_state {
            TtsState::Loading => {
                // Animated loading bar: a sliding highlight in a dot/pipe pattern
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
        #[cfg(not(feature = "tts"))]
        let tts_indicator = String::new();

        let prefix = format!("{tts_indicator}{scroll_info}{article_count}");

        let mut pairs: Vec<(&str, &str)> = Vec::new();
        if !app.detail_articles.is_empty() {
            pairs.push(("n/p", "조문"));
            pairs.push(("a", "조문 목록"));
        }
        pairs.push(("P", "판례"));
        // TTS key hints
        #[cfg(feature = "tts")]
        match app.tts_state {
            TtsState::Playing | TtsState::Synthesizing => {
                pairs.push(("s", "정지"));
            }
            _ => {
                pairs.push(("r", "조문 읽기"));
                pairs.push(("R", "전체 읽기"));
            }
        }
        pairs.push(("B", "북마크"));
        pairs.push(("t", "테마"));
        pairs.push(("o", "AI 에이전트"));
        pairs.push(("Esc", "뒤로"));
        pairs.push(("?", "도움말"));

        styles::status_line(theme, &prefix, &pairs, area.width)
    };

    let bar = Paragraph::new(content).style(styles::status_bar(theme));
    f.render_widget(bar, area);
}

/// Convert a source-line index into a wrapped-line offset.
///
/// `Paragraph::scroll((y, 0))` with `Wrap` counts **wrapped** lines, not source
/// lines.  When the terminal is narrow (e.g. tmux split), long lines wrap into
/// multiple rendered lines, so source-line N can be much further down than
/// wrapped-line N.  This function sums the wrapped line counts for all source
/// lines before `source_line` to produce the correct scroll offset.
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
            // Ceiling division: number of visual rows this source line occupies.
            wrapped += line_width.div_ceil(w);
        }
    }

    u16::try_from(wrapped).unwrap_or(u16::MAX)
}

/// Render the article list popup
pub fn render_article_popup(f: &mut Frame, app: &App, theme: &Theme, area: Rect) {
    let popup_area = styles::centered_rect(50, 70, area);

    let law_title = app.detail.as_ref().map(|d| d.entry.title.as_str());

    let items: Vec<ListItem> = app
        .detail_articles
        .iter()
        .enumerate()
        .map(|(i, art)| {
            let style = styles::list_item_style(theme, i == app.popup_selected, false);

            // Show precedent count if map is loaded
            let count_suffix = if let (Some(title), Some(map)) = (law_title, &app.precedent_map) {
                let article_id = art.label.split_whitespace().next().unwrap_or(&art.label);
                let count = map.article_count(title, article_id);
                if count > 0 {
                    format!("  ({count}판)")
                } else {
                    String::new()
                }
            } else {
                String::new()
            };

            let mut spans = vec![Span::styled(format!("  {}", art.label), style)];
            if !count_suffix.is_empty() {
                spans.push(Span::styled(
                    count_suffix,
                    Style::default().fg(theme.accent),
                ));
            }
            ListItem::new(Line::from(spans))
        })
        .collect();

    let block = Block::default()
        .title(" Articles \u{2014} \u{c870}\u{d56d} \u{baa9}\u{b85d} ")
        .borders(Borders::ALL)
        .style(Style::default().fg(theme.accent).bg(theme.panel_bg));

    let list = List::new(items).block(block);

    f.render_widget(Clear, styles::clear_area_for_popup(popup_area));
    f.render_widget(list, popup_area);
}
