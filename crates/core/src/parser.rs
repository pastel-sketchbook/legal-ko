use std::collections::HashMap;

use crate::models::ArticleRef;
use crate::models::PrecedentSectionRef;

/// Strip YAML frontmatter delimited by --- ... ---
#[must_use]
pub fn strip_frontmatter(raw: &str) -> &str {
    if !raw.starts_with("---") {
        return raw;
    }
    // Find the closing ---
    if let Some(end) = raw[3..].find("\n---") {
        let after = end + 3 + 4; // skip past \n---
        if after < raw.len() {
            let rest = &raw[after..];
            return rest.strip_prefix('\n').unwrap_or(rest);
        }
    }
    raw
}

/// Extract the raw YAML frontmatter block (without the `---` delimiters).
///
/// Returns `None` if no valid frontmatter is found.
#[must_use]
fn extract_frontmatter_block(raw: &str) -> Option<&str> {
    if !raw.starts_with("---") {
        return None;
    }
    let after_open = &raw[3..];
    let end = after_open.find("\n---")?;
    Some(&after_open[..end])
}

/// Parse YAML frontmatter into a flat key→value map.
///
/// Handles:
/// - Simple scalars: `key: value` (with optional single-quote stripping)
/// - Lists: lines starting with `- ` under a `key:` with no inline value
///
/// This is intentionally minimal — **no `serde_yaml` dependency**.
#[must_use]
pub fn parse_frontmatter(raw: &str) -> HashMap<String, FrontmatterValue> {
    let Some(block) = extract_frontmatter_block(raw) else {
        return HashMap::new();
    };

    let mut map = HashMap::new();
    let mut current_key: Option<String> = None;
    let mut current_list: Vec<String> = Vec::new();

    for line in block.lines() {
        // List continuation: "- value"
        let line = line.trim_end_matches('\r');
        if let Some(item) = line
            .strip_prefix("- ")
            .or_else(|| line.strip_prefix("  - "))
        {
            if current_key.is_some() {
                current_list.push(strip_yaml_quotes(item.trim()));
            }
            continue;
        }

        // New key: "key: value" or "key:"
        if let Some((key, value)) = line.split_once(':') {
            let key = key.trim();
            if key.is_empty() || key.starts_with('#') {
                continue;
            }

            // Flush previous list key
            if let Some(prev_key) = current_key.take() {
                if current_list.is_empty() {
                    // Was a scalar with no value — store empty string
                    map.insert(prev_key, FrontmatterValue::Scalar(String::new()));
                } else {
                    map.insert(
                        prev_key,
                        FrontmatterValue::List(std::mem::take(&mut current_list)),
                    );
                }
            }

            let value = value.trim();
            if value.is_empty() {
                // This key has a list or empty value on subsequent lines
                current_key = Some(key.to_string());
                current_list.clear();
            } else {
                map.insert(
                    key.to_string(),
                    FrontmatterValue::Scalar(strip_yaml_quotes(value)),
                );
            }
        }
    }

    // Flush final key
    if let Some(key) = current_key {
        if current_list.is_empty() {
            map.insert(key, FrontmatterValue::Scalar(String::new()));
        } else {
            map.insert(key, FrontmatterValue::List(current_list));
        }
    }

    map
}

/// A value from YAML frontmatter — either a simple string or a list of strings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FrontmatterValue {
    Scalar(String),
    List(Vec<String>),
}

impl FrontmatterValue {
    /// Get as a scalar string, returning an empty string for lists.
    #[must_use]
    pub fn as_str(&self) -> &str {
        match self {
            FrontmatterValue::Scalar(s) => s,
            FrontmatterValue::List(_) => "",
        }
    }

    /// Get as a list of strings. Scalars are returned as a single-element list.
    #[must_use]
    pub fn as_list(&self) -> Vec<String> {
        match self {
            FrontmatterValue::Scalar(s) => {
                if s.is_empty() {
                    Vec::new()
                } else {
                    vec![s.clone()]
                }
            }
            FrontmatterValue::List(v) => v.clone(),
        }
    }
}

/// Strip surrounding single or double quotes from a YAML value.
fn strip_yaml_quotes(s: &str) -> String {
    let s = s.trim();
    if s.len() >= 2
        && ((s.starts_with('\'') && s.ends_with('\'')) || (s.starts_with('"') && s.ends_with('"')))
    {
        s[1..s.len() - 1].to_string()
    } else {
        s.to_string()
    }
}

/// Enrich a `LawEntry` with metadata extracted from the YAML frontmatter of the
/// raw markdown content.
///
/// Fields that are non-empty in the frontmatter overwrite the entry's defaults.
/// This is called after fetching the full law content so that the list view can
/// display accurate departments, dates, category, and status.
pub fn enrich_entry_from_frontmatter(entry: &mut crate::models::LawEntry, raw: &str) {
    let fm = parse_frontmatter(raw);

    if let Some(v) = fm.get("제목") {
        let s = v.as_str();
        if !s.is_empty() {
            entry.title = s.to_string();
        }
    }
    if let Some(v) = fm.get("법령구분") {
        let s = v.as_str();
        if !s.is_empty() {
            entry.category = s.to_string();
        }
    }
    if let Some(v) = fm.get("소관부처") {
        let list = v.as_list();
        if !list.is_empty() {
            entry.departments = list;
        }
    }
    if let Some(v) = fm.get("공포일자") {
        let s = v.as_str();
        if !s.is_empty() {
            entry.promulgation_date = s.to_string();
        }
    }
    if let Some(v) = fm.get("시행일자") {
        let s = v.as_str();
        if !s.is_empty() {
            entry.enforcement_date = s.to_string();
        }
    }
    if let Some(v) = fm.get("상태") {
        let s = v.as_str();
        if !s.is_empty() {
            entry.status = s.to_string();
        }
    }
}

/// Extract article references (제X조) from raw markdown content.
///
/// Returns a list of `ArticleRef` with the label and the line index
/// within the stripped content (matching the line ordering that a
/// renderer would produce).
#[must_use]
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
/// article heading, stripped of markdown formatting.
///
/// The `article_index` corresponds to the index into the list returned
/// by [`extract_articles`].
#[must_use]
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
        .map_or(lines.len(), |next| next.line_index);

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
#[must_use]
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

// ── Precedent (판례) parsing ────────────────────────────────

/// Enrich a `PrecedentEntry` with metadata extracted from the YAML frontmatter.
///
/// Precedent frontmatter fields:
/// - `사건명` → `case_name`
/// - `사건번호` → `case_number`
/// - `선고일자` → `ruling_date`
/// - `법원명` → `court_name`
/// - `사건종류` → `case_type`
/// - `판결유형` → `ruling_type`
pub fn enrich_precedent_from_frontmatter(entry: &mut crate::models::PrecedentEntry, raw: &str) {
    let fm = parse_frontmatter(raw);

    if let Some(v) = fm.get("사건명") {
        let s = v.as_str();
        if !s.is_empty() {
            entry.case_name = s.to_string();
        }
    }
    if let Some(v) = fm.get("사건번호") {
        let s = v.as_str();
        if !s.is_empty() {
            entry.case_number = s.to_string();
        }
    }
    if let Some(v) = fm.get("선고일자") {
        let s = v.as_str();
        if !s.is_empty() {
            entry.ruling_date = s.to_string();
        }
    }
    if let Some(v) = fm.get("법원명") {
        let s = v.as_str().trim();
        if !s.is_empty() {
            entry.court_name = s.to_string();
        }
    }
    if let Some(v) = fm.get("사건종류") {
        let s = v.as_str();
        if !s.is_empty() {
            entry.case_type = s.to_string();
        }
    }
    if let Some(v) = fm.get("판결유형") {
        let s = v.as_str();
        if !s.is_empty() {
            entry.ruling_type = s.to_string();
        }
    }
}

/// Extract section references from a precedent markdown document.
///
/// Precedent documents use `## heading` for major sections such as
/// 판시사항, 판결요지, 참조조문, 참조판례, 판례내용.
#[must_use]
pub fn extract_precedent_sections(raw: &str) -> Vec<PrecedentSectionRef> {
    let content = strip_frontmatter(raw);
    let mut sections = Vec::new();

    for (line_index, line) in content.lines().enumerate() {
        if let Some(heading) = line.strip_prefix("## ") {
            let label = heading.trim().to_string();
            if !label.is_empty() {
                sections.push(PrecedentSectionRef { label, line_index });
            }
        }
    }

    sections
}

/// Extract plain text for a specific section of a precedent by index.
///
/// Returns the section heading and all subsequent lines up to the next
/// `## ` heading, stripped of markdown formatting.
#[must_use]
pub fn extract_precedent_section_text(raw: &str, section_index: usize) -> Option<String> {
    let sections = extract_precedent_sections(raw);
    let section = sections.get(section_index)?;

    let content = strip_frontmatter(raw);
    let lines: Vec<&str> = content.lines().collect();

    let start = section.line_index;
    if start >= lines.len() {
        return None;
    }

    let end = sections
        .get(section_index + 1)
        .map_or(lines.len(), |next| next.line_index);

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_strip_frontmatter() {
        let input = "---\ntitle: test\n---\n# Hello";
        assert_eq!(strip_frontmatter(input), "# Hello");
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
        let input = "---\ntitle: test\n---\n# 법률\n##### 제1조 (목적)\n**①** 이 법은 목적이다.";
        let text = extract_full_text(input);
        assert!(text.contains("법률"));
        assert!(text.contains("제1조 (목적)"));
        assert!(text.contains("① 이 법은 목적이다."));
        // No markdown remains
        assert!(!text.contains('#'));
        assert!(!text.contains("**"));
    }

    #[test]
    fn test_parse_frontmatter_scalars() {
        let input = "---\n제목: 민법\n법령구분: 법률\n상태: 시행\n---\n# Content";
        let fm = parse_frontmatter(input);
        assert_eq!(fm.get("제목").unwrap().as_str(), "민법");
        assert_eq!(fm.get("법령구분").unwrap().as_str(), "법률");
        assert_eq!(fm.get("상태").unwrap().as_str(), "시행");
    }

    #[test]
    fn test_parse_frontmatter_quoted_values() {
        let input = "---\n법령ID: '001706'\n공포일자: '2026-03-17'\n---\n# Content";
        let fm = parse_frontmatter(input);
        assert_eq!(fm.get("법령ID").unwrap().as_str(), "001706");
        assert_eq!(fm.get("공포일자").unwrap().as_str(), "2026-03-17");
    }

    #[test]
    fn test_parse_frontmatter_list() {
        let input = "---\n소관부처:\n- 법무부\n- 행정안전부\n---\n# Content";
        let fm = parse_frontmatter(input);
        let depts = fm.get("소관부처").unwrap().as_list();
        assert_eq!(depts, vec!["법무부", "행정안전부"]);
    }

    #[test]
    fn test_parse_frontmatter_indented_list() {
        let input = "---\n소관부처:\n  - 법무부\n  - 행정안전부\n---\n# Content";
        let fm = parse_frontmatter(input);
        let depts = fm.get("소관부처").unwrap().as_list();
        assert_eq!(depts, vec!["법무부", "행정안전부"]);
    }

    #[test]
    fn test_parse_frontmatter_empty() {
        let input = "# No frontmatter";
        let fm = parse_frontmatter(input);
        assert!(fm.is_empty());
    }

    #[test]
    fn test_strip_frontmatter_no_trailing_newline() {
        // Frontmatter immediately followed by content without newline
        let input = "---\ntitle: test\n---\nHello";
        assert_eq!(strip_frontmatter(input), "Hello");
    }

    #[test]
    fn test_strip_frontmatter_missing_close() {
        let input = "---\ntitle: test\n# Hello";
        // No closing ---, returns original
        assert_eq!(strip_frontmatter(input), input);
    }

    #[test]
    fn test_extract_articles_with_frontmatter() {
        let input = "---\ntitle: test\n---\n##### 제1조 (목적)\nSome text";
        let articles = extract_articles(input);
        assert_eq!(articles.len(), 1);
        assert_eq!(articles[0].label, "제1조 (목적)");
        assert_eq!(articles[0].line_index, 0);
    }

    #[test]
    fn test_extract_article_text_single_article() {
        // Single article, text extends to end of document
        let input = "##### 제1조 (목적)\n**①** 이 법은 목적을 정한다.";
        let text = extract_article_text(input, 0).unwrap();
        assert!(text.contains("제1조 (목적)"));
        assert!(text.contains("① 이 법은 목적을 정한다."));
    }

    #[test]
    fn test_enrich_entry_from_frontmatter() {
        let mut entry = crate::models::LawEntry {
            id: "kr/민법/법률".to_string(),
            title: "민법".to_string(),
            category: "법률".to_string(),
            departments: Vec::new(),
            promulgation_date: String::new(),
            enforcement_date: String::new(),
            status: "시행".to_string(),
            path: "kr/민법/법률.md".to_string(),
        };
        let raw = "---\n제목: 민법\n법령구분: 법률\n소관부처:\n- 법무부\n공포일자: '2026-03-17'\n시행일자: '2026-03-17'\n상태: 시행\n---\n# 민법";
        enrich_entry_from_frontmatter(&mut entry, raw);
        assert_eq!(entry.departments, vec!["법무부"]);
        assert_eq!(entry.promulgation_date, "2026-03-17");
        assert_eq!(entry.enforcement_date, "2026-03-17");
    }

    #[test]
    fn test_extract_full_text_no_frontmatter() {
        let input = "# 법률\n##### 제1조 (목적)\n**①** 텍스트";
        let text = extract_full_text(input);
        assert!(text.contains("법률"));
        assert!(text.contains("제1조 (목적)"));
        assert!(text.contains("① 텍스트"));
    }

    #[test]
    fn test_parse_frontmatter_realistic() {
        let input = "---\n제목: 민법\n법령MST: 284415\n법령ID: '001706'\n법령구분: 법률\n소관부처:\n- 법무부\n공포일자: '2026-03-17'\n시행일자: '2026-03-17'\n상태: 시행\n출처: https://www.law.go.kr/법령/민법\n---\n# 민법";
        let fm = parse_frontmatter(input);
        assert_eq!(fm.get("제목").unwrap().as_str(), "민법");
        assert_eq!(fm.get("법령구분").unwrap().as_str(), "법률");
        assert_eq!(fm.get("소관부처").unwrap().as_list(), vec!["법무부"]);
        assert_eq!(fm.get("공포일자").unwrap().as_str(), "2026-03-17");
        assert_eq!(fm.get("시행일자").unwrap().as_str(), "2026-03-17");
        assert_eq!(fm.get("상태").unwrap().as_str(), "시행");
    }

    // ── Precedent parser tests ──────────────────────────────

    #[test]
    fn test_extract_precedent_sections() {
        let input = "---\n사건명: 소유권이전등기등\n---\n# 소유권이전등기등\n\n## 판시사항\n\n[1] Some text\n\n## 판결요지\n\n[1] More text\n\n## 판례내용\n\nFull text here";
        let sections = extract_precedent_sections(input);
        assert_eq!(sections.len(), 3);
        assert_eq!(sections[0].label, "판시사항");
        assert_eq!(sections[1].label, "판결요지");
        assert_eq!(sections[2].label, "판례내용");
    }

    #[test]
    fn test_extract_precedent_sections_empty() {
        let input = "# Just a title\nSome body text";
        let sections = extract_precedent_sections(input);
        assert!(sections.is_empty());
    }

    #[test]
    fn test_extract_precedent_section_text() {
        let input = "## 판시사항\n\n[1] First point\n\n## 판결요지\n\n[1] Second point";
        let text = extract_precedent_section_text(input, 0).unwrap();
        assert!(text.contains("판시사항"));
        assert!(text.contains("[1] First point"));
        assert!(!text.contains("판결요지"));

        let text2 = extract_precedent_section_text(input, 1).unwrap();
        assert!(text2.contains("판결요지"));
        assert!(text2.contains("[1] Second point"));
    }

    #[test]
    fn test_extract_precedent_section_text_out_of_range() {
        let input = "## 판시사항\nSome text";
        assert!(extract_precedent_section_text(input, 5).is_none());
    }

    #[test]
    fn test_enrich_precedent_from_frontmatter() {
        let mut entry = crate::models::PrecedentEntry {
            id: "민사/대법원/2000다10048".to_string(),
            case_name: String::new(),
            case_number: "2000다10048".to_string(),
            ruling_date: String::new(),
            court_name: "대법원".to_string(),
            case_type: "민사".to_string(),
            ruling_type: String::new(),
            path: "민사/대법원/2000다10048.md".to_string(),
        };
        let raw = "---\n판례일련번호: '81927'\n사건번호: 2000다10048\n사건명: 소유권이전등기등\n법원명: 대법원\n법원등급: 대법원\n사건종류: 민사\n선고일자: '2002-09-27'\n---\n# 소유권이전등기등";
        enrich_precedent_from_frontmatter(&mut entry, raw);
        assert_eq!(entry.case_name, "소유권이전등기등");
        assert_eq!(entry.ruling_date, "2002-09-27");
        assert_eq!(entry.court_name, "대법원");
        assert_eq!(entry.case_type, "민사");
    }

    #[test]
    fn test_parse_precedent_frontmatter_realistic() {
        let input = "---\n판례일련번호: '81927'\n사건번호: 2000다10048\n사건명: 소유권이전등기등\n법원명: 대법원\n법원등급: 대법원\n사건종류: 민사\n출처: https://www.law.go.kr/판례/81927\n선고일자: '2002-09-27'\n---\n# Content";
        let fm = parse_frontmatter(input);
        assert_eq!(fm.get("판례일련번호").unwrap().as_str(), "81927");
        assert_eq!(fm.get("사건번호").unwrap().as_str(), "2000다10048");
        assert_eq!(fm.get("사건명").unwrap().as_str(), "소유권이전등기등");
        assert_eq!(fm.get("선고일자").unwrap().as_str(), "2002-09-27");
    }
}
