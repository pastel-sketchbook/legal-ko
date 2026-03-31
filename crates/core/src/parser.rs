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
}
