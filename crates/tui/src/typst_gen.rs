use anyhow::Context;
use fontdb::{Database, Source as FontSource};
use typst::diag::{FileError, FileResult};
use typst::foundations::{Bytes, Datetime, Dict, Duration, IntoValue, Str};
use typst::syntax::{FileId, Source, VirtualPath, VirtualRoot};
use typst::text::{Font, FontBook};
use typst::utils::LazyHash;
use typst::{Library, LibraryExt, World};

static TEMPLATE: &str = include_str!("templates/legal.typ");

struct PdfWorld {
    library: LazyHash<Library>,
    book: LazyHash<FontBook>,
    main_source: Source,
    fonts: Vec<Font>,
}

impl PdfWorld {
    fn new(template: &str, inputs: Dict, book: FontBook, fonts: Vec<Font>) -> Self {
        let library = LazyHash::new(Library::builder().with_inputs(inputs).build());
        let main_source = Source::new(
            FileId::new(typst::syntax::RootedPath::new(
                VirtualRoot::Project,
                VirtualPath::new("main.typ").expect("static virtual path: main.typ"),
            )),
            template.to_string(),
        );
        Self {
            library,
            book: LazyHash::new(book),
            main_source,
            fonts,
        }
    }
}

impl World for PdfWorld {
    fn library(&self) -> &LazyHash<Library> {
        &self.library
    }

    fn book(&self) -> &LazyHash<FontBook> {
        &self.book
    }

    fn main(&self) -> FileId {
        self.main_source.id()
    }

    fn source(&self, id: FileId) -> FileResult<Source> {
        if id == self.main_source.id() {
            Ok(self.main_source.clone())
        } else {
            Err(FileError::NotFound(id.vpath().get_with_slash().into()))
        }
    }

    fn file(&self, id: FileId) -> FileResult<Bytes> {
        if id == self.main_source.id() {
            Ok(Bytes::from_string(self.main_source.text().to_string()))
        } else {
            Err(FileError::NotFound(id.vpath().get_with_slash().into()))
        }
    }

    fn font(&self, index: usize) -> Option<Font> {
        self.fonts.get(index).cloned()
    }

    fn today(&self, offset: Option<Duration>) -> Option<Datetime> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .ok()?;

        let time_dur = time::Duration::seconds(now.as_secs().cast_signed())
            + time::Duration::nanoseconds(i64::from(now.subsec_nanos()));

        let total: time::Duration = if let Some(off) = offset {
            let off_dur: time::Duration = off.into();
            time_dur + off_dur
        } else {
            time_dur
        };

        let epoch = time::Date::from_calendar_date(1970, time::Month::January, 1).ok()?;
        let date_time = time::PrimitiveDateTime::new(epoch, time::Time::MIDNIGHT) + total;

        Datetime::from_ymd(date_time.year(), date_time.month() as u8, date_time.day())
    }
}

fn load_fonts() -> (FontBook, Vec<Font>) {
    let mut db = Database::new();
    db.load_system_fonts();

    let mut book = FontBook::new();
    let mut fonts = Vec::new();

    for face in db.faces() {
        if let Some((source, _face_idx)) = db.face_source(face.id) {
            let Some(data) = source_to_bytes(&source) else {
                continue;
            };
            if let Some(font) = Font::new(data, face.index) {
                book.push(font.info().clone());
                fonts.push(font);
            }
        }
    }

    (book, fonts)
}

fn source_to_bytes(source: &FontSource) -> Option<Bytes> {
    match source {
        FontSource::Binary(d) => {
            let bytes: &[u8] = d.as_ref().as_ref();
            Some(Bytes::new(bytes.to_vec()))
        }
        FontSource::File(path) => {
            let data = std::fs::read(path).ok()?;
            Some(Bytes::new(data))
        }
        FontSource::SharedFile(_path, d) => {
            let bytes: &[u8] = d.as_ref().as_ref();
            Some(Bytes::new(bytes.to_vec()))
        }
    }
}

fn s(key: &str) -> Str {
    key.into()
}

pub fn render_pdf(
    doc_type: &str,
    title: &str,
    raw_markdown: &str,
    metadata: &[(&str, &str)],
) -> anyhow::Result<Vec<u8>> {
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

    let (book, fonts) = load_fonts();
    let world = PdfWorld::new(TEMPLATE, outer, book, fonts);

    let warned = typst::compile::<typst_layout::PagedDocument>(&world);
    let doc = warned
        .output
        .map_err(|e| anyhow::anyhow!("Typst compilation failed: {e:?}"))?;

    let options = typst_pdf::PdfOptions::default();
    let pdf = typst_pdf::pdf(&doc, &options)
        .map_err(|errs| anyhow::anyhow!("PDF export failed: {errs:?}"))
        .context("generating PDF from Typst document")?;

    Ok(pdf)
}

fn markdown_to_typst(md: &str) -> String {
    let mut out = String::with_capacity(md.len());
    let mut in_clause = false;

    for line in md.lines() {
        let trimmed = strip_backslash_escapes(line.trim());

        if trimmed == "---" || trimmed == "***" || trimmed == "___" {
            in_clause = false;
            out.push_str("#line(length: 100%)\n");
            continue;
        }

        if let Some(rest) = trimmed.strip_prefix("######") {
            in_clause = false;
            out.push_str("====== ");
            out.push_str(&escape_typst(rest.trim_start_matches(' ')));
            out.push('\n');
        } else if let Some(rest) = trimmed.strip_prefix("#####") {
            in_clause = false;
            out.push_str("===== ");
            out.push_str(&escape_typst(rest.trim_start_matches(' ')));
            out.push('\n');
        } else if let Some(rest) = trimmed.strip_prefix("####") {
            in_clause = false;
            out.push_str("==== ");
            out.push_str(&escape_typst(rest.trim_start_matches(' ')));
            out.push('\n');
        } else if let Some(rest) = trimmed.strip_prefix("###") {
            if !rest.starts_with('#') {
                in_clause = false;
                out.push_str("=== ");
                out.push_str(&escape_typst(rest.trim_start_matches(' ')));
                out.push('\n');
            }
        } else if let Some(rest) = trimmed.strip_prefix("##") {
            if !rest.starts_with('#') {
                in_clause = false;
                out.push_str("== ");
                out.push_str(&escape_typst(rest.trim_start_matches(' ')));
                out.push('\n');
            }
        } else if let Some(rest) = trimmed.strip_prefix('#') {
            if !rest.starts_with('#') {
                in_clause = false;
                out.push_str("= ");
                out.push_str(&escape_typst(rest.trim_start_matches(' ')));
                out.push('\n');
            }
        } else if trimmed.is_empty() {
            out.push('\n');
        } else {
            let escaped = escape_typst(&trimmed);
            let converted = convert_bold(&escaped);

            if starts_with_circled_number(&trimmed) {
                in_clause = true;
                if !out.ends_with("\n\n") {
                    out.push('\n');
                }
                push_indented(&mut out, &converted, "1em");
            } else if starts_with_numbered_item(&trimmed) {
                let items = split_numbered_line(&trimmed);
                for item in &items {
                    let escaped = escape_typst(item);
                    let converted = convert_bold(&escaped);
                    if in_clause {
                        push_indented(&mut out, &converted, "2.5em");
                    } else {
                        if !out.ends_with("\n\n") {
                            out.push('\n');
                        }
                        out.push_str(&converted);
                        out.push('\n');
                    }
                }
            } else {
                in_clause = false;
                out.push_str(&converted);
                out.push('\n');
            }
        }
    }

    out
}

fn push_indented(out: &mut String, content: &str, indent: &str) {
    out.push_str("#pad(left: ");
    out.push_str(indent);
    out.push_str(")[");
    out.push_str(content);
    out.push_str("]\n");
}

/// Strip backslash escapes from markdown source lines.
/// Korean legal documents use `1\. ` (backslash-period-space) and
/// `1.\\ ` (period-backslash-space) to format numbered items; this
/// causes backslashes to appear in the Typst output unless we remove
/// them first.
fn strip_backslash_escapes(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(ch) = chars.next() {
        if ch == '\\' {
            if let Some(next) = chars.clone().next() {
                // Strip `\.` → `.` and `\ ` → ` `
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

fn starts_with_numbered_item(s: &str) -> bool {
    let bytes = s.as_bytes();
    let len = bytes.len();
    let mut i = 0;
    while i < len && bytes[i].is_ascii_digit() {
        i += 1;
    }
    i > 0 && i < len && bytes[i] == b'.' && (i + 1 >= len || bytes[i + 1] == b' ')
}

/// Split a line into multiple numbered items when multiple `N.` markers
/// appear on the same line (e.g. `1. 강사료와 2. 수당` → `["1. 강사료와", "2. 수당"]`).
fn split_numbered_line(line: &str) -> Vec<String> {
    let bytes = line.as_bytes();
    let len = bytes.len();
    let mut parts: Vec<String> = Vec::new();
    // Start of the current segment being built
    let mut seg_start = 0;

    // Scan for boundary patterns: whitespace, digit(s), '.', then whitespace
    // This detects subsequent numbered item starts like " 2." or " 12."
    let mut i = 1;
    while i < len {
        if bytes[i - 1] == b' ' && bytes[i].is_ascii_digit() {
            let mut j = i + 1;
            while j < len && bytes[j].is_ascii_digit() {
                j += 1;
            }
            if j < len && bytes[j] == b'.' && j + 1 < len && bytes[j + 1] == b' ' {
                // Found boundary at position i — split
                if seg_start < i.saturating_sub(1) {
                    parts.push(line[seg_start..i].trim().to_string());
                }
                seg_start = i;
                i = j;
            }
        }
        i += 1;
    }

    // Don't forget the tail (or the whole line if no split)
    if seg_start < len {
        // Trim trailing whitespace from the last segment
        let trailing = line[seg_start..].trim_end().to_string();
        if !trailing.is_empty() {
            // If it starts with a digit+period, keep it; otherwise it's orphan text
            if starts_with_numbered_item(&trailing) || parts.is_empty() {
                parts.push(trailing);
            }
        }
    }

    if parts.is_empty() {
        // No split happened — return the original as a single segment
        vec![line.to_string()]
    } else {
        parts
    }
}

fn starts_with_circled_number(s: &str) -> bool {
    let Some(ch) = s.chars().next() else {
        return false;
    };
    matches!(ch, '①'..='⑳' | '㉑'..='㉟' | '㊱'..='㊿')
}

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

fn convert_bold(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let chars: Vec<char> = s.chars().collect();
    let len = chars.len();
    let mut i = 0;

    while i < len {
        if i + 1 < len
            && chars[i] == '*'
            && chars[i + 1] == '*'
            && let Some(end) = find_double_star(&chars, i + 2)
        {
            out.push('*');
            for &ch in &chars[i + 2..end] {
                out.push(ch);
            }
            out.push('*');
            i = end + 2;
            continue;
        }
        out.push(chars[i]);
        i += 1;
    }

    out
}

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

    #[test]
    fn numbered_items_after_text_get_paragraph_break() {
        let input = "본인은 다음 사항을 확인합니다.\n1. 이름\n2. 주소";
        let output = markdown_to_typst(input);
        assert!(
            output.contains("확인합니다.\n\n1. 이름"),
            "numbered item after text should get paragraph break: {output:?}"
        );
        assert!(
            output.contains("\n\n2. 주소"),
            "consecutive numbered items should each get paragraph break: {output:?}"
        );
    }

    #[test]
    fn circled_numbers_get_indented() {
        let input = "제4조 (출연금)\n① 출연금은 사용할 수 없다.\n1. 유치위원회의 경비보조\n2. 유치활동에 필요한 조사\n② 유치위원회는 결산서를 제출하여야 한다.";
        let output = markdown_to_typst(input);
        assert!(
            output.contains("#pad(left: 1em)["),
            "expected 1em indent for ①: {output}"
        );
        assert!(
            output.contains("#pad(left: 2.5em)["),
            "expected 2.5em indent for 1.: {output}"
        );
        assert!(
            output.starts_with("제4조"),
            "article title should not be indented: {output}"
        );
    }

    #[test]
    fn numbered_items_outside_clause_get_paragraph_break() {
        let input = "1. 첫째\n2. 둘째";
        let output = markdown_to_typst(input);
        assert!(
            !output.contains("#pad"),
            "numbered items outside clause should not be indented: {output}"
        );
        assert_eq!(
            output, "\n1. 첫째\n\n2. 둘째\n",
            "numbered items should each be on their own paragraph"
        );
    }

    #[test]
    fn multi_numbered_items_on_one_line_inside_clause() {
        let input = "⑤ 항목\n1. 강사료와 2. 수당\n3. 교육교재";
        let output = markdown_to_typst(input);
        assert!(
            output.contains("#pad(left: 2.5em)[1. 강사료와]"),
            "first item should be separate indented block: {output:?}"
        );
        assert!(
            output.contains("#pad(left: 2.5em)[2. 수당]"),
            "second item on same line should be separate indented block: {output:?}"
        );
        assert!(
            output.contains("#pad(left: 2.5em)[3. 교육교재]"),
            "third item on its own line should also be indented: {output:?}"
        );
    }

    #[test]
    fn multi_numbered_items_on_one_line_outside_clause() {
        let input = "앞말:\n1. 강사료와 2. 수당\n3. 교육교재";
        let output = markdown_to_typst(input);
        assert!(
            !output.contains("#pad"),
            "outside clause, no indentation: {output:?}"
        );
        assert!(
            output.contains("앞말:\n\n1. 강사료와"),
            "first item after text paragraph break: {output:?}"
        );
        assert!(
            output.contains("\n\n2. 수당"),
            "second item on same line paragraph break: {output:?}"
        );
        assert!(
            output.contains("\n\n3. 교육교재"),
            "third item on its own line also paragraph break: {output:?}"
        );
    }
    #[test]
    fn escaped_period_in_numbered_item_is_stripped() {
        let input = "① 내용\n1\\. 강사료와\n2\\. 수당";
        let output = markdown_to_typst(input);
        assert!(
            output.contains("#pad(left: 2.5em)[1. 강사료와]"),
            "escaped period should be stripped: {output:?}"
        );
        assert!(
            !output.contains("\\"),
            "no backslashes should remain: {output:?}"
        );
    }

    #[test]
    fn escaped_period_in_numbered_item_outside_clause() {
        let input = "1\\. 이름\n2\\. 주소";
        let output = markdown_to_typst(input);
        assert!(
            !output.contains("\\"),
            "no backslashes should remain: {output:?}"
        );
        assert_eq!(
            output, "\n1. 이름\n\n2. 주소\n",
            "items should be on separate paragraphs"
        );
    }

    #[test]
    fn escaped_period_from_legalize_kr_source() {
        // Actual content from legalize-kr/시행령.md
        let input = "  1\\. 「상법」에 따른 회사로서\n  2\\. 청년이 「소득세법」 제168조";
        let output = markdown_to_typst(input);
        // Without preceding circled number → outside clause → paragraph breaks
        assert!(
            output.contains("1. 「상법」에 따른 회사로서"),
            "escaped period stripped: {output:?}"
        );
        assert!(
            output.contains("2. 청년이 「소득세법」 제168조"),
            "second item also stripped: {output:?}"
        );
        assert!(
            !output.contains("\\"),
            "no backslashes in output: {output:?}"
        );
    }

    #[test]
    fn numbered_item_detection() {
        assert!(starts_with_numbered_item("1. 내용"));
        assert!(starts_with_numbered_item("12. 내용"));
        assert!(starts_with_numbered_item("3."));
        assert!(!starts_with_numbered_item("내용"));
        assert!(!starts_with_numbered_item(". 내용"));
        assert!(!starts_with_numbered_item(""));
    }

    #[test]
    fn circled_number_detection() {
        assert!(starts_with_circled_number("① 내용"));
        assert!(starts_with_circled_number("⑳ 내용"));
        assert!(starts_with_circled_number("㉑ 내용"));
        assert!(!starts_with_circled_number("1. 내용"));
        assert!(!starts_with_circled_number("내용"));
        assert!(!starts_with_circled_number(""));
    }
}
