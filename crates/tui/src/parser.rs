use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};

use crate::theme::Theme;
use legal_ko_core::models::ArticleRef;

/// Parse markdown content into styled ratatui Lines and extract article references.
///
/// Returns (`rendered_lines`, articles).
#[must_use]
pub fn parse_law_markdown(raw: &str, theme: &Theme) -> (Vec<Line<'static>>, Vec<ArticleRef>) {
    let mut lines: Vec<Line<'static>> = Vec::new();
    let mut articles: Vec<ArticleRef> = Vec::new();

    // Strip YAML frontmatter if present
    let content = legal_ko_core::parser::strip_frontmatter(raw);

    for text_line in content.lines() {
        let line_index = lines.len();

        if let Some(heading) = text_line.strip_prefix("##### ") {
            // Article heading: ##### 제X조 (Title)
            if heading.contains("제") && heading.contains("조") {
                articles.push(ArticleRef {
                    label: heading.trim().to_string(),
                    line_index,
                });
            }
            lines.push(Line::from(vec![Span::styled(
                heading.to_string(),
                Style::default()
                    .fg(theme.heading_article)
                    .add_modifier(Modifier::BOLD),
            )]));
        } else if let Some(heading) = text_line.strip_prefix("#### ") {
            lines.push(Line::from(vec![Span::styled(
                heading.to_string(),
                Style::default()
                    .fg(theme.heading_article)
                    .add_modifier(Modifier::BOLD),
            )]));
        } else if let Some(heading) = text_line.strip_prefix("### ") {
            // Section (절)
            lines.push(Line::from(vec![Span::styled(
                format!("  {heading}"),
                Style::default()
                    .fg(theme.heading_section)
                    .add_modifier(Modifier::BOLD),
            )]));
        } else if let Some(heading) = text_line.strip_prefix("## ") {
            // Chapter (장)
            lines.push(Line::from(vec![Span::styled(
                heading.to_string(),
                Style::default()
                    .fg(theme.heading_chapter)
                    .add_modifier(Modifier::BOLD),
            )]));
        } else if let Some(heading) = text_line.strip_prefix("# ") {
            // Major heading (편 or law title)
            lines.push(Line::from(vec![Span::styled(
                heading.to_string(),
                Style::default()
                    .fg(theme.heading_major)
                    .add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
            )]));
        } else if let Some(inner) = text_line
            .strip_prefix("**")
            .and_then(|s| s.strip_suffix("**"))
        {
            // Bold paragraph markers like **①**
            lines.push(Line::from(vec![Span::styled(
                inner.to_string(),
                Style::default().fg(theme.fg).add_modifier(Modifier::BOLD),
            )]));
        } else if text_line.starts_with("**") {
            // Partial bold (e.g., **①** some text)
            lines.push(parse_inline_bold(text_line, theme));
        } else if text_line.trim().is_empty() {
            lines.push(Line::from(""));
        } else {
            // Regular text
            lines.push(Line::from(Span::styled(
                text_line.to_string(),
                Style::default().fg(theme.fg),
            )));
        }
    }

    (lines, articles)
}

/// Parse a line with inline **bold** markers into styled spans
fn parse_inline_bold(line: &str, theme: &Theme) -> Line<'static> {
    let mut spans: Vec<Span<'static>> = Vec::new();
    let mut rest = line;

    while let Some(start) = rest.find("**") {
        // Text before bold
        if start > 0 {
            spans.push(Span::styled(
                rest[..start].to_string(),
                Style::default().fg(theme.fg),
            ));
        }
        rest = &rest[start + 2..];

        // Find closing **
        if let Some(end) = rest.find("**") {
            spans.push(Span::styled(
                rest[..end].to_string(),
                Style::default().fg(theme.fg).add_modifier(Modifier::BOLD),
            ));
            rest = &rest[end + 2..];
        } else {
            // No closing **, treat rest as plain
            spans.push(Span::styled(
                format!("**{rest}"),
                Style::default().fg(theme.fg),
            ));
            rest = "";
            break;
        }
    }

    if !rest.is_empty() {
        spans.push(Span::styled(
            rest.to_string(),
            Style::default().fg(theme.fg),
        ));
    }

    Line::from(spans)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme;

    fn default_theme() -> &'static Theme {
        &theme::THEMES[0]
    }

    #[test]
    fn test_article_extraction() {
        let input = "##### 제1조 (목적)\nSome text\n##### 제2조 (정의)\nMore text";
        let (lines, articles) = parse_law_markdown(input, default_theme());
        assert_eq!(articles.len(), 2);
        assert_eq!(articles[0].label, "제1조 (목적)");
        assert_eq!(articles[0].line_index, 0);
        assert_eq!(articles[1].label, "제2조 (정의)");
        assert_eq!(articles[1].line_index, 2);
        assert_eq!(lines.len(), 4);
    }
}
