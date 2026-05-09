//! Hangul-aware text helpers for the search input.
//!
//! Korean text input has two characteristics that the default `String` ops
//! get subtly wrong:
//!
//! 1. **Backspace expectation**: Korean editors peel off the last *jamo*
//!    (component) of the trailing syllable, not the whole syllable. e.g.
//!    민 → 미 → ㅁ → ∅.
//! 2. **Normalization**: Source data and IME input may disagree on NFC vs
//!    NFD. Match-time normalization avoids silent miss-matches.
//!
//! Hangul Syllables block (U+AC00..U+D7A3) is composed via
//!   syllable = 0xAC00 + (lead * 21 + vowel) * 28 + tail
//! where `lead ∈ 0..19`, `vowel ∈ 0..21`, `tail ∈ 0..28` (0 = no tail).

use unicode_normalization::UnicodeNormalization;

/// Hangul Compatibility Jamo (U+3131..) for each lead-consonant index.
/// Used to display the lead jamo standalone after the medial vowel is removed.
const LEAD_TO_COMPAT: [char; 19] = [
    'ㄱ', 'ㄲ', 'ㄴ', 'ㄷ', 'ㄸ', 'ㄹ', 'ㅁ', 'ㅂ', 'ㅃ', 'ㅅ', 'ㅆ', 'ㅇ', 'ㅈ', 'ㅉ', 'ㅊ', 'ㅋ',
    'ㅌ', 'ㅍ', 'ㅎ',
];

const HANGUL_SYLLABLES_START: u32 = 0xAC00;
const HANGUL_SYLLABLES_END: u32 = 0xD7A3;
const VOWELS: u32 = 21;
const TAILS: u32 = 28;

/// Pop one *jamo* from the end of `s`, mutating in place.
///
/// Behavior on the last char:
/// - Hangul syllable with a tail jamo → drop the tail (민 → 미)
/// - Hangul syllable without a tail → replace with lead compat jamo (미 → ㅁ)
/// - Anything else (including bare jamo, ASCII, CJK) → plain `pop()`
pub fn pop_jamo(s: &mut String) {
    let Some(last) = s.chars().next_back() else {
        return;
    };
    let code = last as u32;

    if !(HANGUL_SYLLABLES_START..=HANGUL_SYLLABLES_END).contains(&code) {
        s.pop();
        return;
    }

    let idx = code - HANGUL_SYLLABLES_START;
    let tail = idx % TAILS;
    let vowel = (idx / TAILS) % VOWELS;
    let lead = (idx / TAILS) / VOWELS;

    s.pop();
    if tail > 0 {
        // Drop the tail, keep lead+vowel.
        let new_idx = (lead * VOWELS + vowel) * TAILS;
        if let Some(c) = char::from_u32(HANGUL_SYLLABLES_START + new_idx) {
            s.push(c);
        }
    } else if vowel > 0 || lead < LEAD_TO_COMPAT.len() as u32 {
        // Drop the vowel, leave the lead consonant as a standalone compat jamo.
        if let Some(&c) = LEAD_TO_COMPAT.get(lead as usize) {
            s.push(c);
        }
    }
}

/// Normalize to NFC for stable substring matching across data sources.
pub fn nfc(s: &str) -> String {
    s.nfc().collect()
}

// ── English-to-Hangul 2-set conversion (영타 → 한타) ──────────────────────

/// QWERTY key → Hangul jamo mapping for the standard Korean 2-set layout.
/// Returns `None` for keys that have no jamo mapping.
fn eng_to_jamo(c: char) -> Option<Jamo> {
    match c {
        // Consonants (초/종성)
        'r' => Some(Jamo::Lead(0)),   // ㄱ
        'R' => Some(Jamo::Lead(1)),   // ㄲ
        's' => Some(Jamo::Lead(2)),   // ㄴ
        'e' => Some(Jamo::Lead(3)),   // ㄷ
        'E' => Some(Jamo::Lead(4)),   // ㄸ
        'f' => Some(Jamo::Lead(5)),   // ㄹ
        'a' => Some(Jamo::Lead(6)),   // ㅁ
        'q' => Some(Jamo::Lead(7)),   // ㅂ
        'Q' => Some(Jamo::Lead(8)),   // ㅃ
        't' => Some(Jamo::Lead(9)),   // ㅅ
        'T' => Some(Jamo::Lead(10)),  // ㅆ
        'd' => Some(Jamo::Lead(11)),  // ㅇ
        'w' => Some(Jamo::Lead(12)),  // ㅈ
        'W' => Some(Jamo::Lead(13)),  // ㅉ
        'c' => Some(Jamo::Lead(14)),  // ㅊ
        'z' => Some(Jamo::Lead(15)),  // ㅋ
        'x' => Some(Jamo::Lead(16)),  // ㅌ
        'v' => Some(Jamo::Lead(17)),  // ㅍ
        'g' => Some(Jamo::Lead(18)),  // ㅎ
        // Vowels (중성)
        'k' => Some(Jamo::Vowel(0)),  // ㅏ
        'o' => Some(Jamo::Vowel(1)),  // ㅐ
        'i' => Some(Jamo::Vowel(2)),  // ㅑ
        'O' => Some(Jamo::Vowel(3)),  // ㅒ
        'j' => Some(Jamo::Vowel(4)),  // ㅓ
        'p' => Some(Jamo::Vowel(5)),  // ㅔ
        'u' => Some(Jamo::Vowel(6)),  // ㅕ
        'P' => Some(Jamo::Vowel(7)),  // ㅖ
        'h' => Some(Jamo::Vowel(8)),  // ㅗ
        // hk=ㅘ(9), ho=ㅙ(10), hl=ㅚ(11) — compound vowels handled in compose
        'y' => Some(Jamo::Vowel(12)), // ㅛ
        'n' => Some(Jamo::Vowel(13)), // ㅜ
        // nj=ㅝ(14), np=ㅞ(15), nl=ㅟ(16) — compound vowels
        'b' => Some(Jamo::Vowel(17)), // ㅠ
        'm' => Some(Jamo::Vowel(18)), // ㅡ
        // ml=ㅢ(19) — compound vowel
        'l' => Some(Jamo::Vowel(20)), // ㅣ
        _ => None,
    }
}

#[derive(Clone, Copy, Debug)]
enum Jamo {
    Lead(u32),
    Vowel(u32),
}

/// Map a lead-consonant index to all possible tail-consonant indices.
/// Returns `None` if the consonant cannot appear as a tail (e.g. ㄸ, ㅃ, ㅉ).
fn lead_to_tail(lead: u32) -> Option<u32> {
    match lead {
        0 => Some(1),   // ㄱ
        1 => Some(2),   // ㄲ
        2 => Some(4),   // ㄴ
        3 => Some(7),   // ㄷ
        5 => Some(8),   // ㄹ
        6 => Some(16),  // ㅁ
        7 => Some(17),  // ㅂ
        9 => Some(19),  // ㅅ
        10 => Some(20), // ㅆ
        11 => Some(21), // ㅇ
        12 => Some(22), // ㅈ
        14 => Some(23), // ㅊ
        15 => Some(24), // ㅋ
        16 => Some(25), // ㅌ
        17 => Some(26), // ㅍ
        18 => Some(27), // ㅎ
        _ => None,      // ㄸ(4), ㅃ(8), ㅉ(13) cannot be tails
    }
}

/// Try to form a compound vowel from a base vowel + next vowel.
fn compound_vowel(base: u32, next: u32) -> Option<u32> {
    match (base, next) {
        (8, 0) => Some(9),    // ㅗ+ㅏ=ㅘ
        (8, 1) => Some(10),   // ㅗ+ㅐ=ㅙ
        (8, 20) => Some(11),  // ㅗ+ㅣ=ㅚ
        (13, 4) => Some(14),  // ㅜ+ㅓ=ㅝ
        (13, 5) => Some(15),  // ㅜ+ㅔ=ㅞ
        (13, 20) => Some(16), // ㅜ+ㅣ=ㅟ
        (18, 20) => Some(19), // ㅡ+ㅣ=ㅢ
        _ => None,
    }
}

/// Try to form a compound tail from a base tail + next consonant (as lead index).
fn compound_tail(base_tail: u32, next_lead: u32) -> Option<u32> {
    match (base_tail, next_lead) {
        (1, 9) => Some(3),    // ㄱ+ㅅ=ㄳ
        (4, 12) => Some(5),   // ㄴ+ㅈ=ㄵ
        (4, 18) => Some(6),   // ㄴ+ㅎ=ㄶ
        (8, 0) => Some(9),    // ㄹ+ㄱ=ㄺ
        (8, 6) => Some(10),   // ㄹ+ㅁ=ㄻ
        (8, 7) => Some(11),   // ㄹ+ㅂ=ㄼ
        (8, 9) => Some(12),   // ㄹ+ㅅ=ㄽ
        (8, 16) => Some(13),  // ㄹ+ㅌ=ㄾ
        (8, 17) => Some(14),  // ㄹ+ㅍ=ㄿ
        (8, 18) => Some(15),  // ㄹ+ㅎ=ㅀ
        (17, 9) => Some(18),  // ㅂ+ㅅ=ㅄ
        _ => None,
    }
}

/// Convert an English string typed on a QWERTY keyboard to Hangul using
/// the standard Korean 2-set (두벌식) layout.
///
/// Returns `None` if the input contains no mappable characters (i.e. it's
/// not plausibly mis-typed Korean).
pub fn eng_to_hangul(input: &str) -> Option<String> {
    let mut result = String::new();

    // Automaton state: we may be building a syllable.
    let mut lead: Option<u32> = None;
    let mut vowel: Option<u32> = None;
    let mut tail: Option<u32> = None;
    let mut has_any = false;

    let flush = |result: &mut String, lead: &mut Option<u32>, vowel: &mut Option<u32>, tail: &mut Option<u32>| {
        if let (Some(l), Some(v)) = (*lead, *vowel) {
            let t = tail.unwrap_or(0);
            if let Some(c) = char::from_u32(HANGUL_SYLLABLES_START + (l * VOWELS + v) * TAILS + t) {
                result.push(c);
            }
        } else if let Some(l) = *lead {
            if let Some(&c) = LEAD_TO_COMPAT.get(l as usize) {
                result.push(c);
            }
        } else if let Some(v) = *vowel {
            // Standalone vowel (ㅏ=U+314F..)
            // Hangul Compatibility Jamo vowels start at U+314F for ㅏ (index 0).
            if let Some(c) = char::from_u32(0x314F + v) {
                result.push(c);
            }
        }
        *lead = None;
        *vowel = None;
        *tail = None;
    };

    for ch in input.chars() {
        let Some(jamo) = eng_to_jamo(ch) else {
            // Non-mappable char: flush pending syllable, pass through literally
            flush(&mut result, &mut lead, &mut vowel, &mut tail);
            result.push(ch);
            continue;
        };
        has_any = true;

        match jamo {
            Jamo::Lead(l) => {
                if lead.is_some() && vowel.is_some() {
                    // We have lead+vowel; this consonant might be a tail
                    if tail.is_none() {
                        if let Some(t) = lead_to_tail(l) {
                            tail = Some(t);
                            continue;
                        }
                        // Can't be a tail → flush, start new
                        flush(&mut result, &mut lead, &mut vowel, &mut tail);
                        lead = Some(l);
                    } else {
                        // Already have a tail; try compound tail
                        if let Some(ct) = compound_tail(tail.unwrap(), l) {
                            tail = Some(ct);
                            continue;
                        }
                        // Can't compound → flush, start new
                        flush(&mut result, &mut lead, &mut vowel, &mut tail);
                        lead = Some(l);
                    }
                } else if lead.is_some() && vowel.is_none() {
                    // Double consonant without vowel — flush first, start new
                    flush(&mut result, &mut lead, &mut vowel, &mut tail);
                    lead = Some(l);
                } else {
                    lead = Some(l);
                }
            }
            Jamo::Vowel(v) => {
                if lead.is_some() && vowel.is_some() && tail.is_some() {
                    // Tail gets "stolen" by this vowel as the lead of the next syllable.
                    let t = tail.unwrap();
                    // Check if compound tail — split it, keep first part
                    if let Some(first) = compound_tail_first(t) {
                        let stolen_lead = tail_to_lead(t).unwrap();
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
                    // Try compound vowel
                    if let Some(cv) = compound_vowel(vowel.unwrap(), v) {
                        vowel = Some(cv);
                    } else {
                        flush(&mut result, &mut lead, &mut vowel, &mut tail);
                        vowel = Some(v);
                    }
                } else if lead.is_some() {
                    vowel = Some(v);
                } else {
                    // Vowel without a lead — try compound with previous standalone vowel
                    if vowel.is_some() {
                        if let Some(cv) = compound_vowel(vowel.unwrap(), v) {
                            vowel = Some(cv);
                        } else {
                            flush(&mut result, &mut lead, &mut vowel, &mut tail);
                            vowel = Some(v);
                        }
                    } else {
                        vowel = Some(v);
                    }
                }
            }
        }
    }
    flush(&mut result, &mut lead, &mut vowel, &mut tail);

    if has_any { Some(result) } else { None }
}

/// Map a tail-consonant index back to its lead-consonant index.
/// For compound tails, returns the *last* component as a lead.
fn tail_to_lead(tail: u32) -> Option<u32> {
    match tail {
        1 => Some(0),   // ㄱ
        2 => Some(1),   // ㄲ
        3 => Some(9),   // ㄳ → ㅅ (last component)
        4 => Some(2),   // ㄴ
        5 => Some(12),  // ㄵ → ㅈ
        6 => Some(18),  // ㄶ → ㅎ
        7 => Some(3),   // ㄷ
        8 => Some(5),   // ㄹ
        9 => Some(0),   // ㄺ → ㄱ
        10 => Some(6),  // ㄻ → ㅁ
        11 => Some(7),  // ㄼ → ㅂ
        12 => Some(9),  // ㄽ → ㅅ
        13 => Some(16), // ㄾ → ㅌ
        14 => Some(17), // ㄿ → ㅍ
        15 => Some(18), // ㅀ → ㅎ
        16 => Some(6),  // ㅁ
        17 => Some(7),  // ㅂ
        18 => Some(9),  // ㅄ → ㅅ
        19 => Some(9),  // ㅅ
        20 => Some(10), // ㅆ
        21 => Some(11), // ㅇ
        22 => Some(12), // ㅈ
        23 => Some(14), // ㅊ
        24 => Some(15), // ㅋ
        25 => Some(16), // ㅌ
        26 => Some(17), // ㅍ
        27 => Some(18), // ㅎ
        _ => None,
    }
}

/// For compound tails, return the first component as a tail index so we
/// can keep it on the previous syllable when stealing the last component.
fn compound_tail_first(tail: u32) -> Option<u32> {
    match tail {
        3 => Some(1),   // ㄳ → ㄱ
        5 => Some(4),   // ㄵ → ㄴ
        6 => Some(4),   // ㄶ → ㄴ
        9 => Some(8),   // ㄺ → ㄹ
        10 => Some(8),  // ㄻ → ㄹ
        11 => Some(8),  // ㄼ → ㄹ
        12 => Some(8),  // ㄽ → ㄹ
        13 => Some(8),  // ㄾ → ㄹ
        14 => Some(8),  // ㄿ → ㄹ
        15 => Some(8),  // ㅀ → ㄹ
        18 => Some(17), // ㅄ → ㅂ
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pop_jamo_drops_tail() {
        let mut s = String::from("민");
        pop_jamo(&mut s);
        assert_eq!(s, "미");
    }

    #[test]
    fn pop_jamo_drops_vowel_to_lead() {
        let mut s = String::from("미");
        pop_jamo(&mut s);
        assert_eq!(s, "ㅁ");
    }

    #[test]
    fn pop_jamo_drops_lead_jamo() {
        let mut s = String::from("ㅁ");
        pop_jamo(&mut s);
        assert_eq!(s, "");
    }

    #[test]
    fn pop_jamo_chain_민법() {
        let mut s = String::from("민법");
        pop_jamo(&mut s); // 법 → 버
        assert_eq!(s, "민버");
        pop_jamo(&mut s); // 버 → ㅂ
        assert_eq!(s, "민ㅂ");
        pop_jamo(&mut s); // ㅂ → ∅
        assert_eq!(s, "민");
        pop_jamo(&mut s); // 민 → 미
        assert_eq!(s, "미");
    }

    #[test]
    fn pop_jamo_ascii() {
        let mut s = String::from("abc");
        pop_jamo(&mut s);
        assert_eq!(s, "ab");
    }

    #[test]
    fn pop_jamo_empty() {
        let mut s = String::new();
        pop_jamo(&mut s);
        assert_eq!(s, "");
    }

    #[test]
    fn nfc_normalizes_decomposed() {
        // ᄆ + ᅵ + ᆫ (NFD jamo) → 민 (NFC syllable)
        let nfd = "\u{1106}\u{1175}\u{11AB}";
        assert_eq!(nfc(nfd), "민");
    }

    #[test]
    fn eng_to_hangul_민법() {
        // a=ㅁ k=ㅏ f=ㄴ → 민, q=ㅂ j=ㅓ v=ㅍ → ... wait
        // 민 = ㅁ(a) + ㅣ(l) + ㄴ(s) = "als"
        // 법 = ㅂ(q) + ㅓ(j) + ㅂ(q) = "qjq"  — but ㅂ tail is index 17
        assert_eq!(eng_to_hangul("als"), Some("민".to_string()));
        assert_eq!(eng_to_hangul("alsqjq"), Some("민법".to_string()));
    }

    #[test]
    fn eng_to_hangul_한글() {
        // 한 = ㅎ(g) + ㅏ(k) + ㄴ(s) = "gks"
        // 글 = ㄱ(r) + ㅡ(m) + ㄹ(f) = "rmf"
        assert_eq!(eng_to_hangul("gksrmf"), Some("한글".to_string()));
    }

    #[test]
    fn eng_to_hangul_compound_vowel() {
        // 과 = ㄱ(r) + ㅘ(hk) = "rhk"
        assert_eq!(eng_to_hangul("rhk"), Some("과".to_string()));
    }

    #[test]
    fn eng_to_hangul_tail_steal() {
        // 가나 = ㄱ(r) + ㅏ(k) + ㄴ(s) + ㅏ(k) → tail ㄴ stolen → 가 + 나
        assert_eq!(eng_to_hangul("rksk"), Some("가나".to_string()));
    }

    #[test]
    fn eng_to_hangul_no_mapping() {
        assert_eq!(eng_to_hangul("123"), None);
    }

    #[test]
    fn eng_to_hangul_mixed() {
        // "r1k" → ㄱ + "1" + ㅏ
        let result = eng_to_hangul("r1k").unwrap();
        assert_eq!(result, "ㄱ1ㅏ");
    }
}
