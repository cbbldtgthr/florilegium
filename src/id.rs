//! Entry IDs.
//!
//! IDs share calibre's annotation ID shape: 22 characters of unpadded
//! base64url over 16 bytes. Kindle-originated IDs are uuid5 hashes of the
//! clipping's identifying fields, so the same clippings file always yields the
//! same IDs. The recipe is frozen: changing it changes the IDs of entries not
//! yet in an archive. Never edit v1.

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use uuid::Uuid;

use crate::norm::norm_v1;

/// Namespace for Kindle-originated IDs. Never change it.
const KINDLE_NAMESPACE: Uuid = Uuid::from_u128(0x0438ebe6_c7bc_4b31_9c25_d7a5d4810ae9);

const SEP: char = '\u{1F}';

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryKind {
    Highlight,
    Note,
}

impl EntryKind {
    pub fn as_str(self) -> &'static str {
        match self {
            EntryKind::Highlight => "highlight",
            EntryKind::Note => "note",
        }
    }
}

/// Where a clipping sits, as Kindle reports it. A location wins over a page.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Position {
    Location { start: u32, end: u32 },
    Page { start: u32, end: u32 },
    Unknown,
}

impl Position {
    fn token(self) -> String {
        match self {
            Position::Location { start, end } => format!("L{start}-{end}"),
            Position::Page { start, end } => format!("P{start}-{end}"),
            Position::Unknown => String::new(),
        }
    }
}

/// The identifying fields of one Kindle clipping, as read from the file.
#[derive(Debug, Clone, Copy)]
pub struct KindleClippingKey<'a> {
    pub kind: EntryKind,
    /// The title part of the title line, without the author group.
    pub title: &'a str,
    /// The last parenthesised group of the title line, if any.
    pub author: Option<&'a str>,
    /// The highlighted text, or the note body for a note.
    pub text: &'a str,
    pub position: Position,
    /// The `Added on` date string, verbatim.
    pub raw_added_on: &'a str,
}

/// Matching key for titles and authors: `norm_v1`, then lowercased. Not fuzzy.
pub fn key_v1(raw: &str) -> String {
    norm_v1(raw).to_lowercase()
}

/// The ID a Kindle clipping would get if it were new to the archive.
pub fn kindle_id(c: &KindleClippingKey) -> String {
    let fields = [
        "v1",
        c.kind.as_str(),
        &key_v1(c.title),
        &key_v1(c.author.unwrap_or("")),
        &norm_v1(c.text),
        &c.position.token(),
        c.raw_added_on.trim(),
    ];
    let name = fields.join(&SEP.to_string());
    encode(Uuid::new_v5(&KINDLE_NAMESPACE, name.as_bytes()))
}

fn encode(u: Uuid) -> String {
    URL_SAFE_NO_PAD.encode(u.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> KindleClippingKey<'static> {
        KindleClippingKey {
            kind: EntryKind::Highlight,
            title: "The Lantern Keeper's Almanac",
            author: Some("Vell, Orrin"),
            text: "The tide keeps no calendar but its own.",
            position: Position::Location {
                start: 1234,
                end: 1236,
            },
            raw_added_on: "Friday, 23 October 2026 15:50:12",
        }
    }

    // Golden vectors: these pin the ID recipe. They were cross-checked against
    // an independent Python implementation (uuid.uuid5 + urlsafe_b64encode).
    // A failure means identity has changed; fix the code, never the vector.
    #[test]
    fn golden_vectors() {
        let base = sample();
        let cases = [
            (base, "_WARV0NSXueUMf5MQBedtQ"),
            (
                KindleClippingKey {
                    kind: EntryKind::Note,
                    text: "My own aside.",
                    ..base
                },
                "0P7CLHB6V8e_SjS5xfnK4A",
            ),
            (
                KindleClippingKey {
                    author: None,
                    ..base
                },
                "4T8UhzqwUFu4setP6V5e9w",
            ),
            (
                KindleClippingKey {
                    position: Position::Page { start: 57, end: 57 },
                    ..base
                },
                "UB5JugDOVXGFIRDIffCRJg",
            ),
            (
                KindleClippingKey {
                    position: Position::Unknown,
                    ..base
                },
                "tqEwNCmAX_Czbmyb0qm3Yw",
            ),
        ];
        for (input, want) in cases {
            assert_eq!(kindle_id(&input), want, "input: {input:?}");
        }
    }

    #[test]
    fn id_has_calibre_shape() {
        let id = kindle_id(&sample());
        assert_eq!(id.len(), 22);
        assert!(
            id.chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        );
    }

    #[test]
    fn normalisation_differences_do_not_change_the_id() {
        let base = sample();
        let variant = KindleClippingKey {
            title: "\u{FEFF}the  lantern keeper\u{2019}s ALMANAC",
            author: Some(" vell,\u{00A0}orrin "),
            text: "The tide keeps\u{00A0}no calendar but its own.",
            raw_added_on: "  Friday, 23 October 2026 15:50:12\r",
            ..base
        };
        assert_eq!(kindle_id(&variant), kindle_id(&base));
    }

    #[test]
    fn each_identifying_field_changes_the_id() {
        let base = sample();
        let id = kindle_id(&base);
        let variants = [
            KindleClippingKey {
                kind: EntryKind::Note,
                ..base
            },
            KindleClippingKey {
                title: "Another Almanac",
                ..base
            },
            KindleClippingKey {
                author: Some("Someone Else"),
                ..base
            },
            KindleClippingKey {
                text: "The tide keeps no calendar.",
                ..base
            },
            KindleClippingKey {
                text: "the tide keeps no calendar but its own.",
                ..base
            }, // text case matters
            KindleClippingKey {
                position: Position::Location {
                    start: 1234,
                    end: 1237,
                },
                ..base
            },
            KindleClippingKey {
                raw_added_on: "Friday, 23 October 2026 15:50:13",
                ..base
            },
        ];
        for v in variants {
            assert_ne!(kindle_id(&v), id, "variant: {v:?}");
        }
    }

    #[test]
    fn location_and_page_tokens_differ() {
        let base = sample();
        let loc = KindleClippingKey {
            position: Position::Location { start: 57, end: 57 },
            ..base
        };
        let page = KindleClippingKey {
            position: Position::Page { start: 57, end: 57 },
            ..base
        };
        assert_ne!(kindle_id(&loc), kindle_id(&page));
    }
}
