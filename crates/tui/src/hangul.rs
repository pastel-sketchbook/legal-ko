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
}
