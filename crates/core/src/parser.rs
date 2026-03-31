use crate::models::ArticleRef;

/// Strip YAML frontmatter delimited by --- ... ---
pub fn strip_frontmatter(raw: &str) -> &str {
    if !raw.starts_with("---") {
        return raw;
    }
    // Find the closing ---
    if let Some(end) = raw[3..].find("\n---") {
        let after = end + 3 + 4; // skip past \n---
        if after < raw.len() {
            return &raw[after..];
        }
    }
    raw
}

/// Extract article references (제X조) from raw markdown content.
///
/// Returns a list of `ArticleRef` with the label and the line index
/// within the stripped content (matching the line ordering that a
/// renderer would produce).
pub fn extract_articles(raw: &str) -> Vec<ArticleRef> {
    let content = strip_frontmatter(raw);
    let mut articles = Vec::new();

    for (line_index, line) in content.lines().enumerate() {
        if let Some(heading) = line.strip_prefix("##### ")
            && heading.contains("제")
            && heading.contains("조")
        {
            articles.push(ArticleRef {
                label: heading.trim().to_string(),
                line_index,
            });
        }
    }

    articles
}

/// Strip markdown formatting from a single line to produce plain text.
///
/// Removes heading markers (`#`), bold markers (`**`), and leading whitespace.
fn strip_markdown_line(line: &str) -> String {
    let trimmed = line.trim();

    // Strip heading prefixes (##### → plain text)
    let without_heading = if let Some(rest) = trimmed.strip_prefix('#') {
        rest.trim_start_matches('#').trim()
    } else {
        trimmed
    };

    // Strip bold markers
    without_heading.replace("**", "")
}

/// Extract plain text for a specific article by index.
///
/// Returns the article heading and all subsequent lines up to the next
/// article heading (or the next major heading `#`..`####`), stripped of
/// markdown formatting.
///
/// The `article_index` corresponds to the index into the list returned
/// by [`extract_articles`].
pub fn extract_article_text(raw: &str, article_index: usize) -> Option<String> {
    let articles = extract_articles(raw);
    let article = articles.get(article_index)?;

    let content = strip_frontmatter(raw);
    let lines: Vec<&str> = content.lines().collect();

    let start = article.line_index;
    if start >= lines.len() {
        return None;
    }

    // Find the end: next article heading, or next major heading, or end of content
    let end = articles
        .get(article_index + 1)
        .map(|next| next.line_index)
        .unwrap_or(lines.len());

    let mut result = String::new();
    for &line in &lines[start..end] {
        let plain = strip_markdown_line(line);
        if !plain.is_empty() {
            if !result.is_empty() {
                result.push('\n');
            }
            result.push_str(&plain);
        }
    }

    Some(result)
}

/// Extract plain text for the entire law content, stripped of markdown formatting.
///
/// Empty lines are collapsed; frontmatter is removed.
pub fn extract_full_text(raw: &str) -> String {
    let content = strip_frontmatter(raw);
    let mut result = String::new();

    for line in content.lines() {
        let plain = strip_markdown_line(line);
        if !plain.is_empty() {
            if !result.is_empty() {
                result.push('\n');
            }
            result.push_str(&plain);
        }
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_strip_frontmatter() {
        let input = "---\ntitle: test\n---\n# Hello";
        assert_eq!(strip_frontmatter(input), "\n# Hello");
    }

    #[test]
    fn test_strip_frontmatter_none() {
        let input = "# Hello\nworld";
        assert_eq!(strip_frontmatter(input), input);
    }

    #[test]
    fn test_article_extraction() {
        let input = "##### 제1조 (목적)\nSome text\n##### 제2조 (정의)\nMore text";
        let articles = extract_articles(input);
        assert_eq!(articles.len(), 2);
        assert_eq!(articles[0].label, "제1조 (목적)");
        assert_eq!(articles[0].line_index, 0);
        assert_eq!(articles[1].label, "제2조 (정의)");
        assert_eq!(articles[1].line_index, 2);
    }

    #[test]
    fn test_strip_markdown_line() {
        assert_eq!(strip_markdown_line("##### 제1조 (목적)"), "제1조 (목적)");
        assert_eq!(strip_markdown_line("# 대한민국헌법"), "대한민국헌법");
        assert_eq!(strip_markdown_line("**①** 본문"), "① 본문");
        assert_eq!(strip_markdown_line("plain text"), "plain text");
        assert_eq!(strip_markdown_line(""), "");
    }

    #[test]
    fn test_extract_article_text() {
        let input = "##### 제1조 (목적)\n**①** 이 법은 목적을 정한다.\n**②** 시행한다.\n##### 제2조 (정의)\n이 법에서 사용하는 용어";
        let text = extract_article_text(input, 0).unwrap();
        assert!(text.contains("제1조 (목적)"));
        assert!(text.contains("① 이 법은 목적을 정한다."));
        assert!(text.contains("② 시행한다."));
        // Should NOT contain article 2
        assert!(!text.contains("제2조"));

        let text2 = extract_article_text(input, 1).unwrap();
        assert!(text2.contains("제2조 (정의)"));
        assert!(text2.contains("이 법에서 사용하는 용어"));
    }

    #[test]
    fn test_extract_article_text_out_of_range() {
        let input = "##### 제1조 (목적)\nSome text";
        assert!(extract_article_text(input, 5).is_none());
    }

    #[test]
    fn test_extract_full_text() {
        let input =
            "---\ntitle: test\n---\n# 법률\n##### 제1조 (목적)\n**①** 이 법은 목적이다.";
        let text = extract_full_text(input);
        assert!(text.contains("법률"));
        assert!(text.contains("제1조 (목적)"));
        assert!(text.contains("① 이 법은 목적이다."));
        // No markdown remains
        assert!(!text.contains('#'));
        assert!(!text.contains("**"));
    }
}
