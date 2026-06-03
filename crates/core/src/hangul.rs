//! 영타→한타 conversion: convert English QWERTY keyboard input to Hangul.
//! Implements the standard Korean 2-set (두벌식) layout.

/// Hangul Compatibility Jamo (U+3131..) for each lead-consonant index.
const LEAD_TO_COMPAT: [char; 19] = [
    'ㄱ', 'ㄲ', 'ㄴ', 'ㄷ', 'ㄸ', 'ㄹ', 'ㅁ', 'ㅂ', 'ㅃ', 'ㅅ', 'ㅆ', 'ㅇ', 'ㅈ', 'ㅉ', 'ㅊ', 'ㅋ',
    'ㅌ', 'ㅍ', 'ㅎ',
];

const HANGUL_SYLLABLES_START: u32 = 0xAC00;
const VOWELS: u32 = 21;
const TAILS: u32 = 28;

#[derive(Clone, Copy, Debug)]
enum Jamo {
    Lead(u32),
    Vowel(u32),
}

/// QWERTY key → Hangul jamo mapping for the standard Korean 2-set layout.
fn eng_to_jamo(c: char) -> Option<Jamo> {
    match c {
        'r' => Some(Jamo::Lead(0)),
        'R' => Some(Jamo::Lead(1)),
        's' => Some(Jamo::Lead(2)),
        'e' => Some(Jamo::Lead(3)),
        'E' => Some(Jamo::Lead(4)),
        'f' => Some(Jamo::Lead(5)),
        'a' => Some(Jamo::Lead(6)),
        'q' => Some(Jamo::Lead(7)),
        'Q' => Some(Jamo::Lead(8)),
        't' => Some(Jamo::Lead(9)),
        'T' => Some(Jamo::Lead(10)),
        'd' => Some(Jamo::Lead(11)),
        'w' => Some(Jamo::Lead(12)),
        'W' => Some(Jamo::Lead(13)),
        'c' => Some(Jamo::Lead(14)),
        'z' => Some(Jamo::Lead(15)),
        'x' => Some(Jamo::Lead(16)),
        'v' => Some(Jamo::Lead(17)),
        'g' => Some(Jamo::Lead(18)),
        'k' => Some(Jamo::Vowel(0)),
        'o' => Some(Jamo::Vowel(1)),
        'i' => Some(Jamo::Vowel(2)),
        'O' => Some(Jamo::Vowel(3)),
        'j' => Some(Jamo::Vowel(4)),
        'p' => Some(Jamo::Vowel(5)),
        'u' => Some(Jamo::Vowel(6)),
        'P' => Some(Jamo::Vowel(7)),
        'h' => Some(Jamo::Vowel(8)),
        'y' => Some(Jamo::Vowel(12)),
        'n' => Some(Jamo::Vowel(13)),
        'b' => Some(Jamo::Vowel(17)),
        'm' => Some(Jamo::Vowel(18)),
        'l' => Some(Jamo::Vowel(20)),
        _ => None,
    }
}

/// Map a lead-consonant index to a tail-consonant index.
fn lead_to_tail(lead: u32) -> Option<u32> {
    match lead {
        0 => Some(1),
        1 => Some(2),
        2 => Some(4),
        3 => Some(7),
        5 => Some(8),
        6 => Some(16),
        7 => Some(17),
        9 => Some(19),
        10 => Some(20),
        11 => Some(21),
        12 => Some(22),
        14 => Some(23),
        15 => Some(24),
        16 => Some(25),
        17 => Some(26),
        18 => Some(27),
        _ => None,
    }
}

/// Try to form a compound vowel from a base vowel + next vowel.
fn compound_vowel(base: u32, next: u32) -> Option<u32> {
    match (base, next) {
        (8, 0) => Some(9),
        (8, 1) => Some(10),
        (8, 20) => Some(11),
        (13, 4) => Some(14),
        (13, 5) => Some(15),
        (13, 20) => Some(16),
        (18, 20) => Some(19),
        _ => None,
    }
}

/// Try to form a compound tail from a base tail + next consonant (as lead index).
fn compound_tail(base_tail: u32, next_lead: u32) -> Option<u32> {
    match (base_tail, next_lead) {
        (1, 9) => Some(3),
        (4, 12) => Some(5),
        (4, 18) => Some(6),
        (8, 0) => Some(9),
        (8, 6) => Some(10),
        (8, 7) => Some(11),
        (8, 9) => Some(12),
        (8, 16) => Some(13),
        (8, 17) => Some(14),
        (8, 18) => Some(15),
        (17, 9) => Some(18),
        _ => None,
    }
}

/// Flush pending syllable state into the result string.
fn flush_syllable(
    result: &mut String,
    lead: &mut Option<u32>,
    vowel: &mut Option<u32>,
    tail: &mut Option<u32>,
) {
    if let (Some(l), Some(v)) = (*lead, *vowel) {
        let t = tail.unwrap_or(0);
        if let Some(c) = char::from_u32(HANGUL_SYLLABLES_START + (l * VOWELS + v) * TAILS + t) {
            result.push(c);
        }
    } else if let Some(l) = *lead {
        if let Some(&c) = LEAD_TO_COMPAT.get(l as usize) {
            result.push(c);
        }
    } else if let Some(v) = *vowel
        && let Some(c) = char::from_u32(0x314F + v)
    {
        result.push(c);
    }
    *lead = None;
    *vowel = None;
    *tail = None;
}

/// Map a tail-consonant index back to its lead-consonant index.
#[allow(clippy::match_same_arms)]
fn tail_to_lead(tail: u32) -> Option<u32> {
    match tail {
        1 => Some(0),
        2 => Some(1),
        3 => Some(9),
        4 => Some(2),
        5 => Some(12),
        6 => Some(18),
        7 => Some(3),
        8 => Some(5),
        9 => Some(0),
        10 => Some(6),
        11 => Some(7),
        12 => Some(9),
        13 => Some(16),
        14 => Some(17),
        15 => Some(18),
        16 => Some(6),
        17 => Some(7),
        18 => Some(9),
        19 => Some(9),
        20 => Some(10),
        21 => Some(11),
        22 => Some(12),
        23 => Some(14),
        24 => Some(15),
        25 => Some(16),
        26 => Some(17),
        27 => Some(18),
        _ => None,
    }
}

/// For compound tails, return the first component as a tail index.
#[allow(clippy::match_same_arms)]
fn compound_tail_first(tail: u32) -> Option<u32> {
    match tail {
        3 => Some(1),
        5 => Some(4),
        6 => Some(4),
        9 => Some(8),
        10 => Some(8),
        11 => Some(8),
        12 => Some(8),
        13 => Some(8),
        14 => Some(8),
        15 => Some(8),
        18 => Some(17),
        _ => None,
    }
}

/// Convert an English string typed on a QWERTY keyboard to Hangul using
/// the standard Korean 2-set (두벌식) layout.
///
/// Returns `None` if the input contains no mappable characters (i.e. it's
/// not plausibly mis-typed Korean).
///
/// # Panics
///
/// Panics only if a compound-tail mapping is missing — this is a logic error
/// in the conversion tables and should never happen at runtime.
#[must_use]
pub fn eng_to_hangul(input: &str) -> Option<String> {
    let mut result = String::new();
    let mut lead: Option<u32> = None;
    let mut vowel: Option<u32> = None;
    let mut tail: Option<u32> = None;
    let mut has_any = false;

    let flush = flush_syllable;

    for ch in input.chars() {
        let Some(jamo) = eng_to_jamo(ch) else {
            flush(&mut result, &mut lead, &mut vowel, &mut tail);
            result.push(ch);
            continue;
        };
        has_any = true;

        match jamo {
            Jamo::Lead(l) => {
                if lead.is_some() && vowel.is_some() {
                    if tail.is_none() {
                        if let Some(t) = lead_to_tail(l) {
                            tail = Some(t);
                            continue;
                        }
                        flush(&mut result, &mut lead, &mut vowel, &mut tail);
                        lead = Some(l);
                    } else if let Some(t) = tail {
                        if let Some(ct) = compound_tail(t, l) {
                            tail = Some(ct);
                            continue;
                        }
                        flush(&mut result, &mut lead, &mut vowel, &mut tail);
                        lead = Some(l);
                    }
                } else if lead.is_some() && vowel.is_none() {
                    flush(&mut result, &mut lead, &mut vowel, &mut tail);
                    lead = Some(l);
                } else {
                    lead = Some(l);
                }
            }
            Jamo::Vowel(v) => {
                if let Some(t) = tail {
                    if let Some(first) = compound_tail_first(t) {
                        let stolen_lead =
                            tail_to_lead(t).expect("compound tails always map to a lead");
                        tail = Some(first);
                        flush(&mut result, &mut lead, &mut vowel, &mut tail);
                        lead = Some(stolen_lead);
                        vowel = Some(v);
                    } else if let Some(sl) = tail_to_lead(t) {
                        tail = None;
                        flush(&mut result, &mut lead, &mut vowel, &mut tail);
                        lead = Some(sl);
                        vowel = Some(v);
                    } else {
                        flush(&mut result, &mut lead, &mut vowel, &mut tail);
                        vowel = Some(v);
                    }
                } else if lead.is_some() && vowel.is_some() {
                    if let Some(cv) = vowel.and_then(|vv| compound_vowel(vv, v)) {
                        vowel = Some(cv);
                    } else {
                        flush(&mut result, &mut lead, &mut vowel, &mut tail);
                        vowel = Some(v);
                    }
                } else if lead.is_some() {
                    vowel = Some(v);
                } else if let Some(cv) = vowel.and_then(|vv| compound_vowel(vv, v)) {
                    vowel = Some(cv);
                } else {
                    flush(&mut result, &mut lead, &mut vowel, &mut tail);
                    vowel = Some(v);
                }
            }
        }
    }
    flush(&mut result, &mut lead, &mut vowel, &mut tail);

    if has_any { Some(result) } else { None }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn eng_to_hangul_민법() {
        assert_eq!(eng_to_hangul("als").as_deref(), Some("민"));
        assert_eq!(eng_to_hangul("qjq").as_deref(), Some("법"));
        assert_eq!(eng_to_hangul("alsqjq").as_deref(), Some("민법"));
    }

    #[test]
    fn eng_to_hangul_한글() {
        assert_eq!(eng_to_hangul("gksrmf").as_deref(), Some("한글"));
    }

    #[test]
    fn eng_to_hangul_compound_vowel() {
        // 과 = ㄱ(r) + ㅗ(h) + ㅏ(k) → compound vowel ㅘ
        assert_eq!(eng_to_hangul("rhk").as_deref(), Some("과"));
    }

    #[test]
    fn eng_to_hangul_tail_steal() {
        // rkdtlsdnr = 강신욱 (tail ㄴ/ㅇ/ㄱ stolen as next lead)
        assert_eq!(eng_to_hangul("rkdtlsdnr").as_deref(), Some("강신욱"));
    }

    #[test]
    fn eng_to_hangul_no_mapping() {
        // "hello" maps via 2-set to Korean jamo
        assert_eq!(eng_to_hangul("hello").as_deref(), Some("되ㅣㅐ"));
        assert!(eng_to_hangul("").is_none());
        assert_eq!(eng_to_hangul("123"), None);
    }

    #[test]
    fn eng_to_hangul_mixed() {
        // Hangul chars in the middle pass through; English chars map via 2-set
        assert_eq!(eng_to_hangul("hello안녕").as_deref(), Some("되ㅣㅐ안녕"));
    }
}
