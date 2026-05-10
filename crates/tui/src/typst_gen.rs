//! Typst-based PDF generation for Korean legal documents.
//!
//! Uses `typst-as-lib` to compile an embedded `.typ` template with document
//! data passed via `sys.inputs`. The body content is converted from markdown
//! to Typst markup before compilation.

use anyhow::Context;
use typst::foundations::{Dict, IntoValue, Str};
use typst::layout::PagedDocument;
use typst_as_lib::typst_kit_options::TypstKitFontOptions;
use typst_as_lib::{TypstEngine, TypstTemplateMainFile};

static TEMPLATE: &str = include_str!("templates/legal.typ");

/// Build the Typst engine with embedded fonts and the legal template.
fn build_engine() -> TypstEngine<TypstTemplateMainFile> {
    TypstEngine::builder()
        .main_file(TEMPLATE)
        .search_fonts_with(TypstKitFontOptions::default())
        .build()
}

/// Render a legal document to PDF bytes.
///
/// `metadata` is a list of `(key, value)` pairs to insert into the input dict
/// alongside `doc_type`, `title`, and `body`.
pub fn render_pdf(
    doc_type: &str,
    title: &str,
    raw_markdown: &str,
    metadata: &[(&str, &str)],
) -> anyhow::Result<Vec<u8>> {
    let engine = build_engine();

    let body = markdown_to_typst(raw_markdown);

    let mut data = Dict::new();
    data.insert(s("doc_type"), doc_type.into_value());
    data.insert(s("title"), title.into_value());
    data.insert(s("body"), body.into_value());

    for &(key, value) in metadata {
        data.insert(s(key), value.into_value());
    }

    let mut outer = Dict::new();
    outer.insert(s("v"), data.into_value());

    let doc: PagedDocument = engine
        .compile_with_input(outer)
        .output
        .map_err(|e| anyhow::anyhow!("Typst compilation failed: {e}"))?;

    let options = typst_pdf::PdfOptions::default();
    let pdf = typst_pdf::pdf(&doc, &options)
        .map_err(|errs| anyhow::anyhow!("PDF export failed: {errs:?}"))
        .context("generating PDF from Typst document")?;

    Ok(pdf)
}

/// Helper to create Typst `Str` keys.
fn s(key: &str) -> Str {
    key.into()
}

/// Convert markdown content to Typst markup.
///
/// Handles the subset of markdown commonly found in Korean legal documents:
/// - Headings (`#` → `=`)
/// - Bold (`**text**` → `*text*`)
/// - Horizontal rules (`---` → `#line(length: 100%)`)
/// - Preserves paragraph breaks
///
/// Characters that are special in Typst (`#`, `*`, `_`, `@`, `<`, `>`, `$`)
/// are escaped in body text to prevent accidental interpretation.
fn markdown_to_typst(md: &str) -> String {
    let mut out = String::with_capacity(md.len());

    for line in md.lines() {
        let trimmed = line.trim();

        // Horizontal rule
        if trimmed == "---" || trimmed == "***" || trimmed == "___" {
            out.push_str("#line(length: 100%)\n");
            continue;
        }

        // Headings: count leading `#` chars
        if let Some(rest) = trimmed.strip_prefix("######") {
            out.push_str("====== ");
            out.push_str(&escape_typst(rest.trim_start_matches(' ')));
            out.push('\n');
        } else if let Some(rest) = trimmed.strip_prefix("#####") {
            out.push_str("===== ");
            out.push_str(&escape_typst(rest.trim_start_matches(' ')));
            out.push('\n');
        } else if let Some(rest) = trimmed.strip_prefix("####") {
            out.push_str("==== ");
            out.push_str(&escape_typst(rest.trim_start_matches(' ')));
            out.push('\n');
        } else if let Some(rest) = trimmed.strip_prefix("###") {
            // Guard against `####` — already handled above
            if !rest.starts_with('#') {
                out.push_str("=== ");
                out.push_str(&escape_typst(rest.trim_start_matches(' ')));
                out.push('\n');
            }
        } else if let Some(rest) = trimmed.strip_prefix("##") {
            if !rest.starts_with('#') {
                out.push_str("== ");
                out.push_str(&escape_typst(rest.trim_start_matches(' ')));
                out.push('\n');
            }
        } else if let Some(rest) = trimmed.strip_prefix('#') {
            if !rest.starts_with('#') {
                out.push_str("= ");
                out.push_str(&escape_typst(rest.trim_start_matches(' ')));
                out.push('\n');
            }
        } else if trimmed.is_empty() {
            out.push('\n');
        } else {
            // Body line: escape special chars, then convert bold
            let escaped = escape_typst(line);
            let converted = convert_bold(&escaped);
            out.push_str(&converted);
            out.push('\n');
        }
    }

    out
}

/// Escape Typst-special characters in body text.
///
/// We escape `#`, `@`, `$`, `<`, `>` with backslash.
/// We do NOT escape `*` and `_` here because `convert_bold` needs to
/// process markdown `**bold**` patterns first — but since we call
/// escape first, we handle `*` specially: lone `*` that aren't part of
/// `**...**` pairs get escaped after bold conversion.
fn escape_typst(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '#' | '@' | '$' => {
                out.push('\\');
                out.push(ch);
            }
            '<' => out.push_str("\\<"),
            '>' => out.push_str("\\>"),
            _ => out.push(ch),
        }
    }
    out
}

/// Convert markdown bold `**text**` to Typst bold `*text*`.
fn convert_bold(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let chars: Vec<char> = s.chars().collect();
    let len = chars.len();
    let mut i = 0;

    while i < len {
        if i + 1 < len && chars[i] == '*' && chars[i + 1] == '*' {
            // Find closing **
            if let Some(end) = find_double_star(&chars, i + 2) {
                out.push('*');
                for &ch in &chars[i + 2..end] {
                    out.push(ch);
                }
                out.push('*');
                i = end + 2;
                continue;
            }
        }
        out.push(chars[i]);
        i += 1;
    }

    out
}

/// Find the position of the next `**` in chars starting from `start`.
fn find_double_star(chars: &[char], start: usize) -> Option<usize> {
    let len = chars.len();
    let mut i = start;
    while i + 1 < len {
        if chars[i] == '*' && chars[i + 1] == '*' {
            return Some(i);
        }
        i += 1;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn heading_conversion() {
        assert_eq!(markdown_to_typst("# Title"), "= Title\n");
        assert_eq!(markdown_to_typst("## Sub"), "== Sub\n");
        assert_eq!(markdown_to_typst("### H3"), "=== H3\n");
    }

    #[test]
    fn bold_conversion() {
        assert_eq!(convert_bold("hello **world**"), "hello *world*");
        assert_eq!(convert_bold("**a** and **b**"), "*a* and *b*");
    }

    #[test]
    fn escape_special() {
        assert_eq!(escape_typst("제1조 #목적"), "제1조 \\#목적");
        assert_eq!(escape_typst("a@b $c"), "a\\@b \\$c");
    }

    #[test]
    fn horizontal_rule() {
        assert_eq!(markdown_to_typst("---"), "#line(length: 100%)\n");
    }

    #[test]
    fn paragraph_preserved() {
        let input = "Line one\n\nLine two";
        let output = markdown_to_typst(input);
        assert!(output.contains("\n\n"));
    }
}
