use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};

use crate::theme::Theme;
use legal_ko_core::models::{ArticleRef, PrecedentSectionRef};

/// Strip backslash escapes from markdown source lines.
/// Korean legal documents use `1\. ` (backslash-period-space) and
/// `1.\\ ` (period-backslash-space) to format numbered items.
fn strip_backslash_escapes(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(ch) = chars.next() {
        if ch == '\\' {
            if let Some(next) = chars.clone().next() {
                if next == '.' || next == ' ' {
                    chars.next();
                    out.push(next);
                } else {
                    out.push(ch);
                }
            } else {
                out.push(ch);
            }
        } else {
            out.push(ch);
        }
    }
    out
}

/// Check if a trimmed line starts with a circled number like `①`, `⑫`.
fn starts_with_circled_number(s: &str) -> bool {
    let first = s.chars().next();
    matches!(first, Some('①'..='⑳'))
}

/// Check if a trimmed line starts with a numbered item like `1.`, `12.`.
fn starts_with_numbered_item(s: &str) -> bool {
    let bytes = s.as_bytes();
    if bytes.is_empty() || !bytes[0].is_ascii_digit() {
        return false;
    }
    let mut i = 1;
    while i < bytes.len() && bytes[i].is_ascii_digit() {
        i += 1;
    }
    i < bytes.len() && bytes[i] == b'.'
}

/// Parse markdown content into styled ratatui Lines and extract article references.
///
/// Returns (`rendered_lines`, articles).
#[must_use]
pub fn parse_law_markdown(raw: &str, theme: &Theme) -> (Vec<Line<'static>>, Vec<ArticleRef>) {
    let mut lines: Vec<Line<'static>> = Vec::new();
    let mut articles: Vec<ArticleRef> = Vec::new();

    // Strip YAML frontmatter if present
    let content = legal_ko_core::parser::strip_frontmatter(raw);

    let mut in_clause = false;

    for text_line in content.lines() {
        let line_index = lines.len();

        if let Some(heading) = text_line.strip_prefix("##### ") {
            in_clause = false;
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
            in_clause = false;
            lines.push(Line::from(vec![Span::styled(
                heading.to_string(),
                Style::default()
                    .fg(theme.heading_article)
                    .add_modifier(Modifier::BOLD),
            )]));
        } else if let Some(heading) = text_line.strip_prefix("### ") {
            in_clause = false;
            // Section (절)
            lines.push(Line::from(vec![Span::styled(
                format!("  {heading}"),
                Style::default()
                    .fg(theme.heading_section)
                    .add_modifier(Modifier::BOLD),
            )]));
        } else if let Some(heading) = text_line.strip_prefix("## ") {
            in_clause = false;
            // Chapter (장)
            lines.push(Line::from(vec![Span::styled(
                heading.to_string(),
                Style::default()
                    .fg(theme.heading_chapter)
                    .add_modifier(Modifier::BOLD),
            )]));
        } else if let Some(heading) = text_line.strip_prefix("# ") {
            in_clause = false;
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
            in_clause = starts_with_circled_number(inner);
            lines.push(Line::from(vec![Span::styled(
                inner.to_string(),
                Style::default().fg(theme.fg).add_modifier(Modifier::BOLD),
            )]));
        } else if text_line.starts_with("**") {
            // Partial bold (e.g., **①** some text)
            let trimmed = text_line.trim_start();
            in_clause = trimmed.starts_with("**①")
                || trimmed.starts_with("**②")
                || trimmed.starts_with("**③")
                || trimmed.starts_with("**④")
                || trimmed.starts_with("**⑤")
                || trimmed.starts_with("**⑥")
                || trimmed.starts_with("**⑦")
                || trimmed.starts_with("**⑧")
                || trimmed.starts_with("**⑨")
                || trimmed.starts_with("**⑩");
            lines.push(parse_inline_bold(text_line, theme));
        } else if text_line.trim().is_empty() {
            in_clause = false;
            lines.push(Line::from(""));
        } else {
            // Regular text — strip escapes and conditionally indent numbered items
            let stripped = strip_backslash_escapes(text_line);
            let trimmed = stripped.trim_start();
            if in_clause && starts_with_numbered_item(trimmed) {
                lines.push(Line::from(Span::styled(
                    format!("    {trimmed}"),
                    Style::default().fg(theme.fg),
                )));
            } else if starts_with_numbered_item(trimmed) {
                // Outside clause — strip leading whitespace, no extra indent
                lines.push(Line::from(Span::styled(
                    trimmed.to_string(),
                    Style::default().fg(theme.fg),
                )));
            } else {
                lines.push(Line::from(Span::styled(
                    stripped,
                    Style::default().fg(theme.fg),
                )));
            }
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

/// Parse precedent markdown content into styled ratatui Lines and extract section references.
///
/// Returns (`rendered_lines`, sections).
#[must_use]
pub fn parse_precedent_markdown(
    raw: &str,
    theme: &Theme,
) -> (Vec<Line<'static>>, Vec<PrecedentSectionRef>) {
    let mut lines: Vec<Line<'static>> = Vec::new();
    let mut sections: Vec<PrecedentSectionRef> = Vec::new();

    // Strip YAML frontmatter if present
    let content = legal_ko_core::parser::strip_frontmatter(raw);

    for text_line in content.lines() {
        let line_index = lines.len();

        if let Some(heading) = text_line.strip_prefix("## ") {
            // Section headings: ## 판시사항, ## 판결요지, etc.
            sections.push(PrecedentSectionRef {
                label: heading.trim().to_string(),
                line_index,
            });
            lines.push(Line::from(vec![Span::styled(
                heading.to_string(),
                Style::default()
                    .fg(theme.heading_chapter)
                    .add_modifier(Modifier::BOLD),
            )]));
        } else if let Some(heading) = text_line.strip_prefix("# ") {
            // Major heading (title)
            lines.push(Line::from(vec![Span::styled(
                heading.to_string(),
                Style::default()
                    .fg(theme.heading_major)
                    .add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
            )]));
        } else if let Some(heading) = text_line.strip_prefix("### ") {
            lines.push(Line::from(vec![Span::styled(
                format!("  {heading}"),
                Style::default()
                    .fg(theme.heading_section)
                    .add_modifier(Modifier::BOLD),
            )]));
        } else if let Some(inner) = text_line
            .strip_prefix("**")
            .and_then(|s| s.strip_suffix("**"))
        {
            lines.push(Line::from(vec![Span::styled(
                inner.to_string(),
                Style::default().fg(theme.fg).add_modifier(Modifier::BOLD),
            )]));
        } else if text_line.starts_with("**") {
            lines.push(parse_inline_bold(text_line, theme));
        } else if text_line.trim().is_empty() {
            lines.push(Line::from(""));
        } else {
            // Regular text — strip escapes and indent numbered items
            let stripped = strip_backslash_escapes(text_line);
            let trimmed = stripped.trim_start();
            if starts_with_numbered_item(trimmed) {
                lines.push(Line::from(Span::styled(
                    format!("  {trimmed}"),
                    Style::default().fg(theme.fg),
                )));
            } else {
                lines.push(Line::from(Span::styled(
                    stripped,
                    Style::default().fg(theme.fg),
                )));
            }
        }
    }

    (lines, sections)
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

    #[test]
    fn escaped_period_is_stripped() {
        let input = "1\\. 내용입니다";
        let (lines, _) = parse_law_markdown(input, default_theme());
        assert_eq!(lines.len(), 1);
        // Outside clause → no extra indent
        assert_eq!(lines[0].spans[0].content, "1. 내용입니다");
    }

    #[test]
    fn escaped_space_is_stripped() {
        let input = "1.\\ 강사료와";
        let (lines, _) = parse_law_markdown(input, default_theme());
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].spans[0].content, "1. 강사료와");
    }

    #[test]
    fn numbered_item_under_circled_gets_indent() {
        let input = "**①** 내용\n1\\. 항목1\n2\\. 항목2";
        let (lines, _) = parse_law_markdown(input, default_theme());
        assert_eq!(lines.len(), 3);
        // Under ① → 4 spaces indent
        assert_eq!(lines[1].spans[0].content, "    1. 항목1");
        assert_eq!(lines[2].spans[0].content, "    2. 항목2");
    }

    #[test]
    fn numbered_item_outside_clause_no_indent() {
        let input = "  1\\. 「상법」에 따른 회사";
        let (lines, _) = parse_law_markdown(input, default_theme());
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].spans[0].content, "1. 「상법」에 따른 회사");
    }

    #[test]
    fn clause_state_resets_on_heading() {
        let input = "**①** 내용\n1\\. 항목\n##### 제1조 (목적)\n2\\. 다른 항목";
        let (lines, _) = parse_law_markdown(input, default_theme());
        // 항목 under ① gets indent, 항목 after heading does not
        assert_eq!(lines[1].spans[0].content, "    1. 항목");
        assert_eq!(lines[3].spans[0].content, "2. 다른 항목");
    }

    #[test]
    fn non_numbered_text_not_indented() {
        let input = "일반 텍스트입니다";
        let (lines, _) = parse_law_markdown(input, default_theme());
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].spans[0].content, "일반 텍스트입니다");
    }
}
