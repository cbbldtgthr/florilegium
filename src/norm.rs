//! The frozen text normaliser `norm_v1`.
//!
//! Its output feeds ID hashing, so any change to it silently changes the IDs
//! of future entries. Never edit v1: add a `norm_v2` beside it instead.

use unicode_normalization::UnicodeNormalization;
use unicode_normalization::char::canonical_combining_class;

/// Normalised text plus, for every output `char`, the byte offset in the raw
/// input of the segment it came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Normalized {
    pub text: String,
    pub raw_offsets: Vec<usize>,
}

/// `norm_v1` as a plain string.
pub fn norm_v1(raw: &str) -> String {
    norm_v1_mapped(raw).text
}

/// `norm_v1`, recording where each output character came from.
///
/// NFKC runs per segment (a base character plus its trailing combining marks)
/// rather than per character, so decomposed accents still compose, while every
/// output character can be traced to a raw offset.
pub fn norm_v1_mapped(raw: &str) -> Normalized {
    let mut text = String::with_capacity(raw.len());
    let mut raw_offsets = Vec::with_capacity(raw.len());
    let mut pending_space: Option<usize> = None;
    let mut push = |c: char, offset: usize| {
        if c == ' ' {
            pending_space.get_or_insert(offset);
            return;
        }
        if let Some(space_offset) = pending_space.take()
            && !text.is_empty()
        {
            text.push(' ');
            raw_offsets.push(space_offset);
        }
        text.push(c);
        raw_offsets.push(offset);
    };

    for (start, segment) in segments(raw) {
        for c in segment.nfkc() {
            match fold(c) {
                Fold::Drop => {}
                Fold::Char(f) => push(f, start),
                Fold::Str(s) => s.chars().for_each(|f| push(f, start)),
            }
        }
    }
    Normalized { text, raw_offsets }
}

/// Split into segments, each starting at a character of combining class 0.
fn segments(raw: &str) -> Vec<(usize, &str)> {
    let mut starts: Vec<usize> = raw
        .char_indices()
        .filter(|&(i, c)| i == 0 || canonical_combining_class(c) == 0)
        .map(|(i, _)| i)
        .collect();
    starts.push(raw.len());
    starts
        .windows(2)
        .map(|w| (w[0], &raw[w[0]..w[1]]))
        .collect()
}

enum Fold {
    Drop,
    Char(char),
    Str(&'static str),
}

/// Fold one NFKC output character. Any whitespace becomes a plain space,
/// collapsed and trimmed by the caller.
fn fold(c: char) -> Fold {
    match c {
        '\u{2018}' | '\u{2019}' | '\u{201A}' | '\u{201B}' => Fold::Char('\''),
        '\u{201C}' | '\u{201D}' | '\u{201E}' | '\u{201F}' => Fold::Char('"'),
        '\u{2012}' | '\u{2013}' | '\u{2014}' | '\u{2015}' => Fold::Char('-'),
        '\u{2026}' => Fold::Str("..."),
        '\u{00AD}' | '\u{200B}' | '\u{200C}' | '\u{200D}' | '\u{2060}' | '\u{FEFF}' => Fold::Drop,
        c if c.is_whitespace() => Fold::Char(' '),
        c => Fold::Char(c),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Golden vectors: these pin norm_v1. A failure here means identity has
    // changed; fix the code, never the vector.
    #[test]
    fn golden_vectors() {
        let cases = [
            ("plain text", "plain text"),
            ("  leading\tand\ntrailing  ", "leading and trailing"),
            ("Case Is Kept", "Case Is Kept"),
            (
                "\u{2018}single\u{2019} \u{201C}double\u{201D} \u{201E}low\u{201F}",
                "'single' \"double\" \"low\"",
            ),
            ("a\u{2013}b\u{2014}c\u{2012}d\u{2015}e", "a-b-c-d-e"),
            ("wait\u{2026}", "wait..."),
            (
                "non\u{00A0}breaking\u{2009}thin\u{3000}ideographic",
                "non breaking thin ideographic",
            ),
            (
                "soft\u{00AD}hyphen zero\u{200B}width\u{200C}\u{200D}\u{2060}\u{FEFF}",
                "softhyphen zerowidth",
            ),
            ("\u{FEFF}Title With BOM", "Title With BOM"),
            ("\u{FB01}ne \u{FB02}ow", "fine flow"), // ligatures expand under NFKC
            ("caf\u{0065}\u{0301}", "caf\u{00E9}"), // decomposed accent composes
            ("x\u{00B2}", "x2"),
            ("emoji \u{1F600} stays", "emoji \u{1F600} stays"),
            ("", ""),
            (" \u{00AD} ", ""),
        ];
        for (raw, want) in cases {
            assert_eq!(norm_v1(raw), want, "raw: {raw:?}");
        }
    }

    #[test]
    fn offsets_point_back_into_raw() {
        let raw = "\u{201C}Hi\u{201D}\u{00A0}\u{00A0}\u{FB01}x";
        let n = norm_v1_mapped(raw);
        assert_eq!(n.text, "\"Hi\" fix");
        assert_eq!(n.raw_offsets.len(), n.text.chars().count());
        let starts: Vec<&str> = n.raw_offsets.iter().map(|&o| &raw[o..]).collect();
        assert!(starts[0].starts_with('\u{201C}'));
        assert!(starts[1].starts_with('H'));
        assert!(starts[4].starts_with('\u{00A0}')); // collapsed space maps to the first space
        assert!(starts[5].starts_with('\u{FB01}') && starts[6].starts_with('\u{FB01}'));
        assert!(starts[7].starts_with('x'));
    }

    #[test]
    fn decomposed_accent_maps_to_its_base() {
        let raw = "e\u{0301}x";
        let n = norm_v1_mapped(raw);
        assert_eq!(n.text, "\u{00E9}x");
        assert_eq!(n.raw_offsets, vec![0, 3]);
    }
}
