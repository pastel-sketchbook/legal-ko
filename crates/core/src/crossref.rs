//! Cross-reference between precedents (판례) and law articles (법조문).
//!
//! Implements a 4-approach fallback strategy:
//!
//! 1. **Exact parse** — Regex extraction of `StatuteRef` from the 참조조문 section.
//!    Highest precision. Yields structured `law_name + article + paragraph`.
//!
//! 2. **Case citation parse** — Regex extraction of `CaseRef` from 참조판례.
//!    Enables precedent→precedent graph traversal and transitive statute lookup.
//!
//! 3. **Fuzzy law-name matching** — Normalise the 참조조문 law name and match
//!    against known law directory names from the metadata index to produce a
//!    concrete law ID (e.g. `kr/민법/법률`).
//!
//! 4. **Case-type affinity** — When no structured 참조조문 exists, use the
//!    precedent's `case_type` to suggest likely related statutes by topic.

use serde::Serialize;

// ── Approach 1: Exact 참조조문 parsing ────────────────────────

/// A structured reference to a statute article extracted from a precedent's
/// 참조조문 section.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct StatuteRef {
    /// Law name as written in the precedent (e.g. "민법", "형법",
    /// "집합건물의소유및관리에관한법률").
    pub law_name: String,
    /// Article number (e.g. "제840조").
    pub article: String,
    /// Optional paragraph / item detail (e.g. "제6호", "제2항 제4호").
    pub detail: Option<String>,
    /// The `[N]` group number from the 참조조문 section, if present.
    /// Corresponds to the numbered points in the 판시사항 section.
    pub group: Option<u32>,
}

/// Extract structured statute references from a precedent's raw markdown.
///
/// Looks for the `## 참조조문` section and parses entries like:
/// - `민법 제840조 제6호`
/// - `[1] 형법 제138조, 법원조직법 제56조 제2항`
/// - `구 지방세법(1998. 12. 31. 법률 제5615호로 개정되기 전의 것) 제112조 제2항`
///
/// Returns an empty `Vec` if no 참조조문 section is found.
#[must_use]
pub fn extract_statute_refs(raw: &str) -> Vec<StatuteRef> {
    let section_text = match extract_section_text(raw, "참조조문") {
        Some(t) => t,
        None => return Vec::new(),
    };

    let mut refs = Vec::new();

    // Split by `/` for grouped entries: [1] ... / [2] ...
    // Also handle ungrouped: "민법 제840조 제6호, 민법 제842조"
    for segment in section_text.split('/') {
        let segment = segment.trim();
        if segment.is_empty() {
            continue;
        }

        // Extract optional group number: [1], [2], [1][2], etc.
        let (groups, rest) = extract_groups(segment);

        // Parse comma-separated refs within this segment.
        // A ref can be: "민법 제393조" or just "제763조" (inheriting the previous law name).
        parse_statute_segment(rest.trim(), &groups, &mut refs);
    }

    refs
}

/// Extract the text content of a named `## ` section from raw markdown.
///
/// Returns the text between the target heading and the next `## ` heading
/// (or end of document), with frontmatter stripped.
fn extract_section_text<'a>(raw: &'a str, section_name: &str) -> Option<&'a str> {
    let content = crate::parser::strip_frontmatter(raw);

    let target = format!("## {section_name}");
    let start = content.find(&target)?;
    let after_heading = start + target.len();

    // Skip to the next line
    let body_start = content[after_heading..]
        .find('\n')
        .map_or(content.len(), |i| after_heading + i + 1);

    // Find the next ## heading or end of content
    let body_end = content[body_start..]
        .find("\n## ")
        .map_or(content.len(), |i| body_start + i);

    let text = content[body_start..body_end].trim();
    if text.is_empty() { None } else { Some(text) }
}

/// Extract `[N]` group markers from the front of a string.
///
/// Returns the group numbers and the remaining string.
/// Handles `[1]`, `[1][2]`, or no brackets.
fn extract_groups(s: &str) -> (Vec<u32>, &str) {
    let mut groups = Vec::new();
    let mut rest = s;

    loop {
        let trimmed = rest.trim_start();
        if !trimmed.starts_with('[') {
            return (groups, trimmed);
        }
        if let Some(close) = trimmed.find(']')
            && let Ok(n) = trimmed[1..close].parse::<u32>()
        {
            groups.push(n);
            rest = &trimmed[close + 1..];
            continue;
        }
        return (groups, trimmed);
    }
}

/// Parse a comma-separated segment of statute references.
///
/// Within a segment like "민법 제393조, 제763조", the law name carries forward:
/// "제763조" inherits "민법" from the preceding entry.
fn parse_statute_segment(segment: &str, groups: &[u32], out: &mut Vec<StatuteRef>) {
    let mut current_law: Option<String> = None;
    let mut current_article: Option<String> = None;

    for part in segment.split(',') {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }

        // Try to split into "law_name 제N조 ..." or just "제N조 ..."
        if let Some(statute_ref) = parse_single_ref(part, &current_law) {
            current_law = Some(statute_ref.law_name.clone());
            current_article = Some(statute_ref.article.clone());
            // Emit one ref per group, or one with no group
            emit_ref(statute_ref, groups, out);
        } else if let Some(detail) = try_parse_bare_detail(part) {
            // Handle bare detail like "제2항" or "제6호" — inherit law + article
            if let (Some(law), Some(article)) = (&current_law, &current_article) {
                let statute_ref = StatuteRef {
                    law_name: law.clone(),
                    article: article.clone(),
                    detail: Some(detail),
                    group: None,
                };
                emit_ref(statute_ref, groups, out);
            }
        }
    }
}

/// Emit a statute ref once per group, or once with no group.
fn emit_ref(statute_ref: StatuteRef, groups: &[u32], out: &mut Vec<StatuteRef>) {
    if groups.is_empty() {
        out.push(statute_ref);
    } else {
        for &g in groups {
            let mut r = statute_ref.clone();
            r.group = Some(g);
            out.push(r);
        }
    }
}

/// Try to parse a bare detail like "제2항" or "제6호" (no 조 present).
///
/// Returns the detail string if the part starts with "제" and contains
/// "항" or "호" but NOT "조".
fn try_parse_bare_detail(s: &str) -> Option<String> {
    let s = s.trim();
    if !s.starts_with('제') {
        return None;
    }
    // Must not contain 조 (otherwise it's a full article ref)
    if s.contains('조') {
        return None;
    }
    // Must contain 항 or 호 or 목 to be a valid detail
    if s.contains('항') || s.contains('호') || s.contains('목') {
        Some(s.to_string())
    } else {
        None
    }
}

/// Parse a single statute reference like "민법 제393조" or "제763조".
///
/// If the string doesn't start with "제", everything before the first "제"
/// (outside parentheses) is treated as the law name. If it starts with "제",
/// the `inherited_law` is used.
fn parse_single_ref(s: &str, inherited_law: &Option<String>) -> Option<StatuteRef> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }

    // Find the first "제" that is NOT inside parentheses.
    let article_start = find_je_outside_parens(s)?;

    // Everything before "제" is the law name (possibly with parenthetical notes)
    let raw_law_name = s[..article_start].trim();
    let law_name = if raw_law_name.is_empty() {
        inherited_law.clone()?
    } else {
        normalise_law_name(raw_law_name)
    };

    // Extract "제N조" and any trailing detail like "제2항", "제6호"
    let article_part = &s[article_start..];
    let (article, detail) = parse_article_detail(article_part);

    if article.is_empty() {
        return None;
    }

    Some(StatuteRef {
        law_name,
        article,
        detail,
        group: None,
    })
}

/// Find the byte offset of the first '제' character that is not inside parentheses.
fn find_je_outside_parens(s: &str) -> Option<usize> {
    let mut depth = 0u32;
    for (i, ch) in s.char_indices() {
        match ch {
            '(' => depth += 1,
            ')' => depth = depth.saturating_sub(1),
            '제' if depth == 0 => return Some(i),
            _ => {}
        }
    }
    None
}

/// Remove parenthetical annotations from a law name.
///
/// E.g. `구 지방세법(1998. 12. 31. 법률 제5615호로 개정되기 전의 것)` → `구 지방세법`
fn normalise_law_name(s: &str) -> String {
    // Strip everything from the first `(` to its matching `)`.
    let mut result = String::new();
    let mut depth = 0u32;
    for ch in s.chars() {
        match ch {
            '(' => depth += 1,
            ')' => depth = depth.saturating_sub(1),
            _ if depth == 0 => result.push(ch),
            _ => {}
        }
    }
    result.trim().to_string()
}

/// Parse "제N조 제M항 제K호" into (article, detail).
///
/// Returns `("제N조", Some("제M항 제K호"))` or `("제N조", None)`.
fn parse_article_detail(s: &str) -> (String, Option<String>) {
    // Find "제...조" — the article number
    let Some(jo_pos) = s.find('조') else {
        return (String::new(), None);
    };

    let jo_end = jo_pos + '조'.len_utf8();
    let article = s[..jo_end].trim().to_string();
    let rest = s[jo_end..].trim();

    if rest.is_empty() {
        (article, None)
    } else {
        // The rest is detail like "제2항", "제6호", "제2항 제4호", etc.
        // Also strip trailing annotations like "(현행 삭제)"
        let detail = normalise_law_name(rest); // reuse paren stripping
        if detail.is_empty() {
            (article, None)
        } else {
            (article, Some(detail))
        }
    }
}

// ── Approach 2: 참조판례 parsing ─────────────────────────────

/// A structured reference to another precedent extracted from the 참조판례
/// section.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CaseRef {
    /// Court name (e.g. "대법원", "헌법재판소").
    pub court: String,
    /// Case number (e.g. "2000다24061").
    pub case_number: String,
    /// Ruling date as written (e.g. "2000-11-10" or "2000. 11. 10.").
    pub ruling_date: String,
    /// The `[N]` group number(s), if present.
    pub groups: Vec<u32>,
}

/// Extract structured case references from a precedent's 참조판례 section.
///
/// Parses entries like:
/// - `대법원 2000. 11. 10. 선고 2000다24061 판결(공2001상, 12)`
/// - `[1] 대법원 1988. 10. 11. 선고 87다카1130 판결(공1988, 1402)`
/// - `헌법재판소 2000. 2. 24. 98헌바94 결정`
#[must_use]
pub fn extract_case_refs(raw: &str) -> Vec<CaseRef> {
    let section_text = match extract_section_text(raw, "참조판례") {
        Some(t) => t,
        None => return Vec::new(),
    };

    let mut refs = Vec::new();

    // Split by `/` for grouped entries, then by `,` for multiple within a group
    for segment in section_text.split('/') {
        let segment = segment.trim();
        if segment.is_empty() {
            continue;
        }

        let (groups, rest) = extract_groups(segment);

        // Within a segment, entries are separated by `,` followed by a court name
        // (대법원, 헌법재판소, etc.) or start of string.
        // We split on `, 대법원` / `, 헌법재판소` boundaries.
        for sub in split_case_entries(rest.trim()) {
            if let Some(case_ref) = parse_single_case_ref(sub.trim(), &groups) {
                refs.push(case_ref);
            }
        }
    }

    refs
}

/// Split a string at boundaries where a new case citation starts.
///
/// Case citations start with court names: 대법원, 헌법재판소.
fn split_case_entries(s: &str) -> Vec<&str> {
    let mut entries = Vec::new();
    let mut last = 0;

    let courts = ["대법원", "헌법재판소"];
    let bytes = s.as_bytes();
    let mut i = 0;

    while i < s.len() {
        // Check if we're at a `, 법원명` boundary (but not at the very start)
        if i > 0 {
            for court in &courts {
                // Check for ", {court}" pattern
                if s[i..].starts_with(court) {
                    // Look back for ", " separator
                    let prefix = &s[last..i];
                    let prefix_trimmed = prefix.trim_end();
                    if prefix_trimmed.ends_with(',') {
                        // Split here: everything before the comma is the previous entry
                        entries.push(
                            s[last..i].trim_end_matches(|c: char| c == ',' || c.is_whitespace()),
                        );
                        last = i;
                        break;
                    }
                }
            }
        }

        // Advance by one character (handle multi-byte Korean)
        if bytes[i] < 0x80 {
            i += 1;
        } else if bytes[i] < 0xE0 {
            i += 2;
        } else if bytes[i] < 0xF0 {
            i += 3;
        } else {
            i += 4;
        }
    }

    if last < s.len() {
        entries.push(s[last..].trim());
    }

    entries
}

/// Parse a single case citation like "대법원 2000. 11. 10. 선고 2000다24061 판결(...)".
fn parse_single_case_ref(s: &str, groups: &[u32]) -> Option<CaseRef> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }

    // Extract court name
    let court;
    let rest;
    if let Some(stripped) = s.strip_prefix("대법원") {
        court = "대법원".to_string();
        rest = stripped.trim_start();
    } else if let Some(stripped) = s.strip_prefix("헌법재판소") {
        court = "헌법재판소".to_string();
        rest = stripped.trim_start();
    } else {
        return None;
    }

    // Extract date: "2000. 11. 10." pattern
    let date = extract_date_from_citation(rest);

    // Extract case number: pattern like "2000다24061", "98헌바94", "87다카1130"
    let case_number = extract_case_number(rest)?;

    Some(CaseRef {
        court,
        case_number,
        ruling_date: date.unwrap_or_default(),
        groups: groups.to_vec(),
    })
}

/// Extract a date in "YYYY. M. D." format from a citation string.
fn extract_date_from_citation(s: &str) -> Option<String> {
    // Match pattern: N. N. N. (Korean date format in citations)
    let mut nums = Vec::new();
    let mut current = String::new();
    let mut in_date = false;

    for ch in s.chars() {
        if ch.is_ascii_digit() {
            current.push(ch);
            in_date = true;
        } else if ch == '.' && in_date && !current.is_empty() {
            nums.push(current.clone());
            current.clear();
            if nums.len() == 3 {
                break;
            }
        } else if ch == ' ' && in_date {
            // Skip spaces within date
        } else {
            if !current.is_empty() {
                // Reset — not a valid date pattern
                nums.clear();
                current.clear();
            }
            in_date = false;
        }
    }

    if nums.len() == 3 {
        let year = &nums[0];
        let month = format!("{:0>2}", nums[1]);
        let day = format!("{:0>2}", nums[2]);
        Some(format!("{year}-{month}-{day}"))
    } else {
        None
    }
}

/// Extract a case number from a citation string.
///
/// Korean case numbers follow patterns like: `2000다24061`, `98헌바94`, `87다카1130`.
/// They consist of 2-4 digit year + Korean syllable(s) + number(s).
fn extract_case_number(s: &str) -> Option<String> {
    // Look for "선고" marker — case number follows it
    // Or look for a pattern: digits + Korean + digits
    let search_after = s.find("선고").map_or(0, |i| i + "선고".len());
    let rest = s[search_after..].trim();

    // Find first digit-Korean-digit sequence
    let mut start = None;
    let mut end = 0;
    let mut saw_digit = false;
    let mut saw_korean = false;
    let mut saw_digit_after_korean = false;

    for (i, ch) in rest.char_indices() {
        if ch.is_ascii_digit() {
            if start.is_none() {
                start = Some(i);
                saw_digit = true;
            } else if saw_korean {
                saw_digit_after_korean = true;
            }
            end = i + ch.len_utf8();
        } else if is_korean_syllable(ch) {
            if saw_digit {
                saw_korean = true;
                end = i + ch.len_utf8();
            } else if start.is_some() {
                // Korean after start but no digits first — reset
                break;
            }
        } else if ch == '_' && saw_digit_after_korean {
            // Handle merged case numbers like "2000다11065_11072"
            end = i + ch.len_utf8();
        } else {
            if saw_digit_after_korean {
                break;
            }
            // Continue if we haven't completed a full pattern
            if saw_korean {
                break;
            }
            start = None;
            saw_digit = false;
            saw_korean = false;
        }
    }

    let start = start?;
    if !saw_digit_after_korean {
        return None;
    }

    Some(rest[start..end].to_string())
}

/// Check if a character is a Korean syllable (Hangul Syllables block).
fn is_korean_syllable(ch: char) -> bool {
    ('\u{AC00}'..='\u{D7A3}').contains(&ch)
}

// ── Approach 3: Fuzzy law-name matching ──────────────────────

/// Result of matching a 참조조문 law name to a known law ID.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LawMatch {
    /// The statute reference from the precedent.
    pub statute_ref: StatuteRef,
    /// Matched law ID (e.g. "kr/민법/법률"), if found.
    pub law_id: Option<String>,
    /// Match quality.
    pub match_type: MatchType,
}

/// How a statute reference was matched to a law ID.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum MatchType {
    /// Exact law name match against a known law directory name.
    Exact,
    /// Matched after stripping `구 ` prefix (old law version).
    OldVersion,
    /// Matched via substring/contains heuristic.
    Substring,
    /// No match found — only case-type affinity available.
    Unmatched,
}

/// Attempt to match statute references against a set of known law names.
///
/// `known_laws` should be the set of law directory names from the legalize-kr
/// metadata index (e.g. `["민법", "형법", "주택임대차보호법", ...]`).
///
/// Uses a 3-tier matching strategy:
/// 1. Exact match: `statute_ref.law_name == known_law`
/// 2. Old-version match: strip `구 ` prefix, then exact match
/// 3. Substring match: `known_law.contains(normalised_name)` or vice versa
#[must_use]
pub fn match_statute_refs(statute_refs: &[StatuteRef], known_laws: &[String]) -> Vec<LawMatch> {
    statute_refs
        .iter()
        .map(|sr| {
            let name = &sr.law_name;

            // Tier 1: Exact match
            if let Some(law) = known_laws.iter().find(|l| *l == name) {
                return LawMatch {
                    statute_ref: sr.clone(),
                    law_id: Some(format!("kr/{law}/법률")),
                    match_type: MatchType::Exact,
                };
            }

            // Tier 2: Strip "구 " prefix (refers to an older version of the law)
            let stripped = name.strip_prefix("구 ").unwrap_or(name);
            if stripped != name
                && let Some(law) = known_laws.iter().find(|l| *l == stripped)
            {
                return LawMatch {
                    statute_ref: sr.clone(),
                    law_id: Some(format!("kr/{law}/법률")),
                    match_type: MatchType::OldVersion,
                };
            }

            // Tier 3: Substring matching (either direction)
            // This handles cases where the 참조조문 uses a slightly different
            // name than the directory (e.g. spaces removed, or an abbreviation).
            let normalised = stripped.replace(' ', "");
            if let Some(law) = known_laws.iter().find(|l| {
                let l_normalised = l.replace(' ', "");
                l_normalised.contains(&normalised) || normalised.contains(&l_normalised)
            }) {
                return LawMatch {
                    statute_ref: sr.clone(),
                    law_id: Some(format!("kr/{law}/법률")),
                    match_type: MatchType::Substring,
                };
            }

            // No match
            LawMatch {
                statute_ref: sr.clone(),
                law_id: None,
                match_type: MatchType::Unmatched,
            }
        })
        .collect()
}

// ── Approach 4: Case-type affinity ───────────────────────────

/// A suggested law that is commonly relevant to a given case type.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AffinityLaw {
    /// Search term to use with `legal-ko-cli search`.
    pub search_term: &'static str,
    /// Human-readable description of why this law is relevant.
    pub reason: &'static str,
}

/// Suggest related laws based on the precedent's case type.
///
/// This is the lowest-precision fallback, used when the precedent has no
/// 참조조문 section or when all statute references failed to match.
///
/// Returns a list of search terms and reasons, ordered by relevance.
#[must_use]
pub fn affinity_laws(case_type: &str) -> Vec<AffinityLaw> {
    match case_type {
        "민사" => vec![
            AffinityLaw {
                search_term: "민법",
                reason: "General civil law — contracts, torts, property",
            },
            AffinityLaw {
                search_term: "상법",
                reason: "Commercial law — business disputes",
            },
            AffinityLaw {
                search_term: "민사소송",
                reason: "Civil procedure",
            },
            AffinityLaw {
                search_term: "임대차",
                reason: "Lease / housing disputes",
            },
        ],
        "형사" => vec![
            AffinityLaw {
                search_term: "형법",
                reason: "Criminal code — offenses and penalties",
            },
            AffinityLaw {
                search_term: "형사소송",
                reason: "Criminal procedure",
            },
            AffinityLaw {
                search_term: "특정범죄",
                reason: "Special criminal statutes",
            },
        ],
        "가사" => vec![
            AffinityLaw {
                search_term: "민법",
                reason: "Civil code — family law provisions (제4편)",
            },
            AffinityLaw {
                search_term: "가사소송",
                reason: "Family litigation procedure",
            },
            AffinityLaw {
                search_term: "가정폭력",
                reason: "Domestic violence prevention",
            },
        ],
        "세무" => vec![
            AffinityLaw {
                search_term: "국세기본",
                reason: "Basic tax framework",
            },
            AffinityLaw {
                search_term: "소득세",
                reason: "Income tax",
            },
            AffinityLaw {
                search_term: "법인세",
                reason: "Corporate tax",
            },
            AffinityLaw {
                search_term: "부가가치세",
                reason: "VAT",
            },
            AffinityLaw {
                search_term: "지방세",
                reason: "Local tax",
            },
        ],
        "일반행정" => vec![
            AffinityLaw {
                search_term: "행정소송",
                reason: "Administrative litigation",
            },
            AffinityLaw {
                search_term: "행정절차",
                reason: "Administrative procedure",
            },
            AffinityLaw {
                search_term: "행정기본",
                reason: "Basic administrative law",
            },
        ],
        "특허" => vec![
            AffinityLaw {
                search_term: "특허법",
                reason: "Patent law",
            },
            AffinityLaw {
                search_term: "상표",
                reason: "Trademark law",
            },
            AffinityLaw {
                search_term: "디자인보호",
                reason: "Design protection",
            },
            AffinityLaw {
                search_term: "부정경쟁",
                reason: "Unfair competition prevention",
            },
        ],
        _ => vec![
            AffinityLaw {
                search_term: "헌법",
                reason: "Constitution — fundamental rights",
            },
            AffinityLaw {
                search_term: "민법",
                reason: "Civil code — general provisions",
            },
        ],
    }
}

// ── Orchestrator: 4-approach fallback chain ───────────────────

/// Complete cross-reference result for a single precedent.
#[derive(Debug, Clone, Serialize)]
pub struct CrossReference {
    /// Structured statute references extracted from 참조조문 (Approach 1).
    pub statute_refs: Vec<StatuteRef>,
    /// Case references extracted from 참조판례 (Approach 2).
    pub case_refs: Vec<CaseRef>,
    /// Statute refs matched to law IDs (Approach 3).
    pub law_matches: Vec<LawMatch>,
    /// Fallback affinity suggestions (Approach 4).
    pub affinity: Vec<AffinityLaw>,
    /// Which approach yielded usable results.
    pub resolution: Resolution,
}

/// Which approach in the fallback chain produced the final result.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum Resolution {
    /// Approach 1+3: statute refs extracted and matched to law IDs.
    ExactStatuteMatch,
    /// Approach 1: statute refs extracted but not all matched to IDs.
    PartialStatuteMatch,
    /// Approach 2: no statute refs, but case refs available for transitive lookup.
    CaseRefsOnly,
    /// Approach 4: no structured refs, fell back to case-type affinity.
    AffinityFallback,
}

/// Build a complete cross-reference for a precedent's raw markdown.
///
/// Runs all 4 approaches and determines which level of the fallback chain
/// produced usable results.
///
/// `known_laws` is the set of law directory names for Approach 3 matching.
/// Pass an empty slice to skip Approach 3 (the `law_matches` field will
/// contain only `Unmatched` entries).
#[must_use]
pub fn cross_reference(raw: &str, case_type: &str, known_laws: &[String]) -> CrossReference {
    // Approach 1: Extract statute refs
    let statute_refs = extract_statute_refs(raw);

    // Approach 2: Extract case refs
    let case_refs = extract_case_refs(raw);

    // Approach 3: Match statute refs to law IDs
    let law_matches = if !statute_refs.is_empty() && !known_laws.is_empty() {
        match_statute_refs(&statute_refs, known_laws)
    } else {
        statute_refs
            .iter()
            .map(|sr| LawMatch {
                statute_ref: sr.clone(),
                law_id: None,
                match_type: MatchType::Unmatched,
            })
            .collect()
    };

    // Approach 4: Affinity fallback
    let affinity = affinity_laws(case_type);

    // Determine resolution level
    let resolution = if !law_matches.is_empty() && law_matches.iter().all(|m| m.law_id.is_some()) {
        Resolution::ExactStatuteMatch
    } else if !law_matches.is_empty() && law_matches.iter().any(|m| m.law_id.is_some()) {
        Resolution::PartialStatuteMatch
    } else if !case_refs.is_empty() {
        Resolution::CaseRefsOnly
    } else {
        Resolution::AffinityFallback
    };

    CrossReference {
        statute_refs,
        case_refs,
        law_matches,
        affinity,
        resolution,
    }
}

// ── Tests ────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── Approach 1: statute ref extraction ───────────────────

    #[test]
    fn test_extract_statute_refs_simple() {
        let raw = "---\n사건명: test\n---\n# Test\n\n## 참조조문\n\n민법 제840조 제6호, 민법 제842조\n\n## 판례내용\n\nBody";
        let refs = extract_statute_refs(raw);
        assert_eq!(refs.len(), 2);
        assert_eq!(refs[0].law_name, "민법");
        assert_eq!(refs[0].article, "제840조");
        assert_eq!(refs[0].detail.as_deref(), Some("제6호"));
        assert_eq!(refs[1].law_name, "민법");
        assert_eq!(refs[1].article, "제842조");
        assert!(refs[1].detail.is_none());
    }

    #[test]
    fn test_extract_statute_refs_grouped() {
        let raw = "## 참조조문\n\n[1] 민법 제393조, 제763조 / [2] 민법 제393조, 제763조";
        let refs = extract_statute_refs(raw);
        assert_eq!(refs.len(), 4);
        assert_eq!(refs[0].group, Some(1));
        assert_eq!(refs[0].article, "제393조");
        assert_eq!(refs[1].group, Some(1));
        assert_eq!(refs[1].article, "제763조");
        assert_eq!(refs[2].group, Some(2));
        assert_eq!(refs[3].group, Some(2));
    }

    #[test]
    fn test_extract_statute_refs_inherited_law_name() {
        let raw = "## 참조조문\n\n민법 제580조 제1항, 제664조, 제667조, 제670조";
        let refs = extract_statute_refs(raw);
        assert_eq!(refs.len(), 4);
        for r in &refs {
            assert_eq!(r.law_name, "민법");
        }
        assert_eq!(refs[0].article, "제580조");
        assert_eq!(refs[0].detail.as_deref(), Some("제1항"));
        assert_eq!(refs[1].article, "제664조");
        assert_eq!(refs[2].article, "제667조");
        assert_eq!(refs[3].article, "제670조");
    }

    #[test]
    fn test_extract_statute_refs_multiple_laws() {
        let raw = "## 참조조문\n\n헌법 제12조 제1항, 제27조 제1항, 제101조 제1항, 형법 제1조 제1항, 제138조, 법원조직법 제56조 제2항, 헌법재판소법 제35조";
        let refs = extract_statute_refs(raw);
        assert_eq!(refs.len(), 7);
        assert_eq!(refs[0].law_name, "헌법");
        assert_eq!(refs[0].article, "제12조");
        assert_eq!(refs[3].law_name, "형법");
        assert_eq!(refs[3].article, "제1조");
        assert_eq!(refs[4].law_name, "형법"); // inherited
        assert_eq!(refs[4].article, "제138조");
        assert_eq!(refs[5].law_name, "법원조직법");
        assert_eq!(refs[6].law_name, "헌법재판소법");
    }

    #[test]
    fn test_extract_statute_refs_old_law_with_parens() {
        let raw = "## 참조조문\n\n구 지방세법(1998. 12. 31. 법률 제5615호로 개정되기 전의 것) 제112조 제2항";
        let refs = extract_statute_refs(raw);
        assert_eq!(refs.len(), 1);
        assert_eq!(refs[0].law_name, "구 지방세법");
        assert_eq!(refs[0].article, "제112조");
        assert_eq!(refs[0].detail.as_deref(), Some("제2항"));
    }

    #[test]
    fn test_extract_statute_refs_no_section() {
        let raw = "## 판시사항\n\nSome text\n\n## 판례내용\n\nBody";
        let refs = extract_statute_refs(raw);
        assert!(refs.is_empty());
    }

    #[test]
    fn test_extract_statute_refs_bare_article() {
        // Edge case: 참조조문 starts with article number, no law name
        // e.g. "제840조 제6호, 민법 제842조" — first one has no explicit law
        let raw = "## 참조조문\n\n제840조 제6호, 민법 제842조";
        let refs = extract_statute_refs(raw);
        // First ref has no inherited law name and no explicit name — should be skipped
        // Only the second one (민법 제842조) should parse
        assert_eq!(refs.len(), 1);
        assert_eq!(refs[0].law_name, "민법");
        assert_eq!(refs[0].article, "제842조");
    }

    #[test]
    fn test_extract_statute_refs_complex_grouped() {
        let raw = "## 참조조문\n\n[1] 집합건물의소유및관리에관한법률 제47조 제1항, 제2항, 제48조 / [2] 집합건물의소유및관리에관한법률 제48조 제2항, 제4항";
        let refs = extract_statute_refs(raw);
        // Group 1: 제47조 제1항, (제47조) 제2항, 제48조 = 3 refs
        // Group 2: 제48조 제2항, (제48조) 제4항 = 2 refs
        assert!(refs.len() >= 4);
        assert_eq!(refs[0].law_name, "집합건물의소유및관리에관한법률");
        assert_eq!(refs[0].group, Some(1));
    }

    // ── Approach 2: case ref extraction ──────────────────────

    #[test]
    fn test_extract_case_refs_simple() {
        let raw = "## 참조판례\n\n대법원 2000. 6. 27. 선고 2000다11621 판결\n\n## 판례내용";
        let refs = extract_case_refs(raw);
        assert_eq!(refs.len(), 1);
        assert_eq!(refs[0].court, "대법원");
        assert_eq!(refs[0].case_number, "2000다11621");
        assert_eq!(refs[0].ruling_date, "2000-06-27");
    }

    #[test]
    fn test_extract_case_refs_grouped() {
        let raw = "## 참조판례\n\n[1] 대법원 1988. 10. 11. 선고 87다카1130 판결(공1988, 1402) / [2] 대법원 1998. 2. 10. 선고 97다35894 판결(공1998상, 683)";
        let refs = extract_case_refs(raw);
        assert_eq!(refs.len(), 2);
        assert_eq!(refs[0].groups, vec![1]);
        assert_eq!(refs[0].case_number, "87다카1130");
        assert_eq!(refs[1].groups, vec![2]);
        assert_eq!(refs[1].case_number, "97다35894");
    }

    #[test]
    fn test_extract_case_refs_multiple_in_group() {
        let raw = "## 참조판례\n\n[1] 대법원 2000. 11. 10. 선고 2000다24061 판결, 대법원 2000. 6. 27. 선고 2000다11621 판결";
        let refs = extract_case_refs(raw);
        assert_eq!(refs.len(), 2);
        assert_eq!(refs[0].case_number, "2000다24061");
        assert_eq!(refs[1].case_number, "2000다11621");
    }

    #[test]
    fn test_extract_case_refs_no_section() {
        let raw = "## 판시사항\n\nText\n\n## 판례내용\n\nBody";
        let refs = extract_case_refs(raw);
        assert!(refs.is_empty());
    }

    // ── Approach 3: fuzzy law-name matching ──────────────────

    #[test]
    fn test_match_statute_refs_exact() {
        let refs = vec![StatuteRef {
            law_name: "민법".to_string(),
            article: "제840조".to_string(),
            detail: None,
            group: None,
        }];
        let known = vec!["민법".to_string(), "형법".to_string()];
        let matches = match_statute_refs(&refs, &known);
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].law_id.as_deref(), Some("kr/민법/법률"));
        assert_eq!(matches[0].match_type, MatchType::Exact);
    }

    #[test]
    fn test_match_statute_refs_old_version() {
        let refs = vec![StatuteRef {
            law_name: "구 지방세법".to_string(),
            article: "제112조".to_string(),
            detail: None,
            group: None,
        }];
        let known = vec!["지방세법".to_string()];
        let matches = match_statute_refs(&refs, &known);
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].law_id.as_deref(), Some("kr/지방세법/법률"));
        assert_eq!(matches[0].match_type, MatchType::OldVersion);
    }

    #[test]
    fn test_match_statute_refs_substring() {
        let refs = vec![StatuteRef {
            law_name: "집합건물의소유및관리에관한법률".to_string(),
            article: "제47조".to_string(),
            detail: None,
            group: None,
        }];
        // Real name might have slight differences
        let known = vec!["집합건물의소유및관리에관한법률".to_string()];
        let matches = match_statute_refs(&refs, &known);
        assert_eq!(matches[0].match_type, MatchType::Exact);
    }

    #[test]
    fn test_match_statute_refs_unmatched() {
        let refs = vec![StatuteRef {
            law_name: "완전히없는법률".to_string(),
            article: "제1조".to_string(),
            detail: None,
            group: None,
        }];
        let known = vec!["민법".to_string()];
        let matches = match_statute_refs(&refs, &known);
        assert_eq!(matches[0].match_type, MatchType::Unmatched);
        assert!(matches[0].law_id.is_none());
    }

    // ── Approach 4: affinity ─────────────────────────────────

    #[test]
    fn test_affinity_civil() {
        let laws = affinity_laws("민사");
        assert!(!laws.is_empty());
        assert!(laws.iter().any(|l| l.search_term == "민법"));
    }

    #[test]
    fn test_affinity_criminal() {
        let laws = affinity_laws("형사");
        assert!(laws.iter().any(|l| l.search_term == "형법"));
    }

    #[test]
    fn test_affinity_unknown() {
        let laws = affinity_laws("알수없는종류");
        assert!(!laws.is_empty()); // Falls back to 헌법 + 민법
    }

    // ── Orchestrator ─────────────────────────────────────────

    #[test]
    fn test_cross_reference_full_match() {
        let raw = "## 참조조문\n\n민법 제393조\n\n## 참조판례\n\n대법원 2000. 6. 27. 선고 2000다11621 판결\n\n## 판례내용\n\nBody";
        let known = vec!["민법".to_string()];
        let xref = cross_reference(raw, "민사", &known);
        assert_eq!(xref.resolution, Resolution::ExactStatuteMatch);
        assert_eq!(xref.statute_refs.len(), 1);
        assert_eq!(xref.case_refs.len(), 1);
        assert_eq!(xref.law_matches.len(), 1);
        assert!(xref.law_matches[0].law_id.is_some());
    }

    #[test]
    fn test_cross_reference_partial_match() {
        let raw = "## 참조조문\n\n민법 제393조, 완전히없는법 제1조\n\n## 판례내용\n\nBody";
        let known = vec!["민법".to_string()];
        let xref = cross_reference(raw, "민사", &known);
        assert_eq!(xref.resolution, Resolution::PartialStatuteMatch);
    }

    #[test]
    fn test_cross_reference_case_refs_only() {
        let raw = "## 참조판례\n\n대법원 2000. 6. 27. 선고 2000다11621 판결\n\n## 판례내용\n\nBody";
        let known = vec!["민법".to_string()];
        let xref = cross_reference(raw, "민사", &known);
        assert_eq!(xref.resolution, Resolution::CaseRefsOnly);
        assert!(xref.statute_refs.is_empty());
        assert!(!xref.case_refs.is_empty());
    }

    #[test]
    fn test_cross_reference_affinity_fallback() {
        let raw = "## 판시사항\n\nSome text\n\n## 판례내용\n\nBody";
        let known = vec!["민법".to_string()];
        let xref = cross_reference(raw, "형사", &known);
        assert_eq!(xref.resolution, Resolution::AffinityFallback);
        assert!(!xref.affinity.is_empty());
        assert!(xref.affinity.iter().any(|a| a.search_term == "형법"));
    }

    // ── Helper function tests ────────────────────────────────

    #[test]
    fn test_normalise_law_name() {
        assert_eq!(
            normalise_law_name("구 지방세법(1998. 12. 31. 법률 제5615호로 개정되기 전의 것)"),
            "구 지방세법"
        );
        assert_eq!(normalise_law_name("민법"), "민법");
        assert_eq!(
            normalise_law_name("자동차손해배상보장법"),
            "자동차손해배상보장법"
        );
    }

    #[test]
    fn test_extract_groups() {
        let (g, r) = extract_groups("[1] 민법 제1조");
        assert_eq!(g, vec![1]);
        assert_eq!(r, "민법 제1조");

        let (g, r) = extract_groups("[1][2] 대법원");
        assert_eq!(g, vec![1, 2]);
        assert_eq!(r, "대법원");

        let (g, r) = extract_groups("민법 제1조");
        assert!(g.is_empty());
        assert_eq!(r, "민법 제1조");
    }

    #[test]
    fn test_extract_date_from_citation() {
        assert_eq!(
            extract_date_from_citation("2000. 11. 10. 선고"),
            Some("2000-11-10".to_string())
        );
        assert_eq!(
            extract_date_from_citation("1988. 10. 11. 선고"),
            Some("1988-10-11".to_string())
        );
    }

    #[test]
    fn test_extract_case_number() {
        assert_eq!(
            extract_case_number("2000. 11. 10. 선고 2000다24061 판결"),
            Some("2000다24061".to_string())
        );
        assert_eq!(
            extract_case_number("1988. 10. 11. 선고 87다카1130 판결"),
            Some("87다카1130".to_string())
        );
    }
}
