//! Which calibre book an archive book is. Book identity across systems is
//! calibre's book UUID; once set on an archive file it is never guessed again.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use serde::Deserialize;

use crate::archive::Book;
use crate::calibre::db::CalibreBook;
use crate::id::key_v1;
use crate::norm::norm_v1;

pub const OVERRIDES_FILE: &str = crate::archive::OVERRIDES_FILE;

/// `book-overrides.json`: user-edited, keyed by a Kindle title line's
/// `key_v1` (as `inspect-clippings` groups books).
#[derive(Debug, Default, Clone, Deserialize)]
#[serde(transparent)]
pub struct Overrides(pub BTreeMap<String, Override>);

#[derive(Debug, Default, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Override {
    /// The calibre book this Kindle book is; skips fuzzy matching.
    pub calibre_uuid: Option<String>,
    /// The book's total Kindle locations, when its highlights are too few to
    /// place repeated passages by themselves.
    pub total_locations: Option<u32>,
}

impl Overrides {
    pub fn load(archive: &Path) -> Result<Overrides, String> {
        let path = archive.join(OVERRIDES_FILE);
        if !path.is_file() {
            return Ok(Overrides::default());
        }
        let text = std::fs::read_to_string(&path)
            .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
        let raw: Overrides = serde_json::from_str(&text)
            .map_err(|e| format!("{} is not valid: {e}", path.display()))?;
        // Accept keys written as raw title lines too.
        Ok(Overrides(
            raw.0.into_iter().map(|(k, v)| (key_v1(&k), v)).collect(),
        ))
    }

    /// The override for any of a book's Kindle title lines.
    pub fn for_book(&self, book: &Book) -> Option<&Override> {
        book.kindle_titles
            .iter()
            .find_map(|t| self.0.get(&key_v1(t)))
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum How {
    AlreadyLinked,
    Override,
    /// Fuzzy title/author score of the chosen book.
    Fuzzy(f64),
}

#[derive(Debug, Clone, PartialEq)]
pub enum BookMatch {
    Matched {
        calibre_id: i64,
        how: How,
    },
    /// No calibre book scored well enough, or the best did not stand out;
    /// the top candidates, best first.
    NoCalibreBook {
        candidates: Vec<Candidate>,
    },
    /// The archive names a calibre UUID the library does not have.
    Gone {
        calibre_uuid: String,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct Candidate {
    pub score: f64,
    pub calibre_id: i64,
    pub uuid: Option<String>,
    pub title: String,
    pub has_epub: bool,
}

#[derive(Debug, Clone, Copy)]
pub struct MatchConfig {
    pub threshold: f64,
    pub margin: f64,
}

impl Default for MatchConfig {
    fn default() -> Self {
        MatchConfig {
            threshold: 0.85,
            margin: 0.1,
        }
    }
}

pub fn match_book(
    book: &Book,
    overrides: &Overrides,
    library: &BTreeMap<i64, CalibreBook>,
    config: MatchConfig,
) -> BookMatch {
    let by_uuid = |u: &str| {
        library
            .values()
            .find(|b| b.uuid.as_deref() == Some(u))
            .map(|b| b.id)
    };
    if let Some(u) = &book.calibre_uuid {
        return match by_uuid(u) {
            Some(calibre_id) => BookMatch::Matched {
                calibre_id,
                how: How::AlreadyLinked,
            },
            None => BookMatch::Gone {
                calibre_uuid: u.clone(),
            },
        };
    }
    if let Some(u) = overrides
        .for_book(book)
        .and_then(|o| o.calibre_uuid.as_deref())
    {
        return match by_uuid(u) {
            Some(calibre_id) => BookMatch::Matched {
                calibre_id,
                how: How::Override,
            },
            None => BookMatch::Gone {
                calibre_uuid: u.to_string(),
            },
        };
    }

    let wanted = Fingerprint::of(&book.title, &book.authors);
    let mut scored: Vec<(f64, &CalibreBook)> = library
        .values()
        .map(|c| (wanted.score(&Fingerprint::of(&c.title, &c.authors)), c))
        .collect();
    scored.sort_by(|a, b| b.0.total_cmp(&a.0).then(a.1.id.cmp(&b.1.id)));
    let candidates = || {
        scored
            .iter()
            .take(3)
            .map(|(s, c)| Candidate {
                score: *s,
                calibre_id: c.id,
                uuid: c.uuid.clone(),
                title: c.title.clone(),
                has_epub: c.formats.contains_key("EPUB"),
            })
            .collect()
    };

    let Some(&(best, _)) = scored.first() else {
        return BookMatch::NoCalibreBook {
            candidates: Vec::new(),
        };
    };
    if best < config.threshold {
        return BookMatch::NoCalibreBook {
            candidates: candidates(),
        };
    }
    // Books within the margin of the best are indistinguishable by name. If
    // exactly one of them has an EPUB, it is the only one worth matching.
    let close: Vec<&CalibreBook> = scored
        .iter()
        .take_while(|(s, _)| best - s < config.margin)
        .map(|(_, c)| *c)
        .collect();
    let chosen = match close.as_slice() {
        [only] => Some(*only),
        many => {
            let with_epub: Vec<&&CalibreBook> = many
                .iter()
                .filter(|c| c.formats.contains_key("EPUB"))
                .collect();
            (with_epub.len() == 1).then(|| *with_epub[0])
        }
    };
    match chosen {
        Some(c) => BookMatch::Matched {
            calibre_id: c.id,
            how: How::Fuzzy(best),
        },
        None => BookMatch::NoCalibreBook {
            candidates: candidates(),
        },
    }
}

/// What a title and author list look like once noise is removed.
#[derive(Debug, Clone, PartialEq)]
struct Fingerprint {
    main: BTreeSet<String>,
    full: BTreeSet<String>,
    authors: BTreeSet<String>,
}

impl Fingerprint {
    fn of(title: &str, authors: &[String]) -> Fingerprint {
        let title = norm_v1(title).to_lowercase();
        let without_groups = strip_groups(&title);
        let main = without_groups.split(':').next().unwrap_or("").to_string();
        Fingerprint {
            main: words(&main, true),
            full: words(&without_groups, true),
            authors: authors
                .iter()
                .flat_map(|a| words(&norm_v1(a).to_lowercase(), false))
                .collect(),
        }
    }

    /// 0 to 1. Main titles carry most of the weight, authors confirm.
    fn score(&self, other: &Fingerprint) -> f64 {
        let title = jaccard(&self.main, &other.main).max(jaccard(&self.full, &other.full));
        if self.authors.is_empty() || other.authors.is_empty() {
            title
        } else {
            0.75 * title + 0.25 * jaccard(&self.authors, &other.authors)
        }
    }
}

/// Remove `(...)` and `[...]` groups: series names, edition notes, publishers.
fn strip_groups(s: &str) -> String {
    let mut out = String::new();
    let mut depth = 0usize;
    for c in s.chars() {
        match c {
            '(' | '[' => depth += 1,
            ')' | ']' => depth = depth.saturating_sub(1),
            c if depth == 0 => out.push(c),
            _ => {}
        }
    }
    out
}

fn words(s: &str, drop_leading_article: bool) -> BTreeSet<String> {
    let mut tokens: Vec<String> = s
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(String::from)
        .collect();
    if drop_leading_article && tokens.len() > 1 && ["the", "a", "an"].contains(&tokens[0].as_str())
    {
        tokens.remove(0);
    }
    tokens.into_iter().collect()
}

fn jaccard(a: &BTreeSet<String>, b: &BTreeSet<String>) -> f64 {
    if a.is_empty() || b.is_empty() {
        return 0.0;
    }
    a.intersection(b).count() as f64 / a.union(b).count() as f64
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fp(title: &str, authors: &[&str]) -> Fingerprint {
        Fingerprint::of(
            title,
            &authors.iter().map(|a| a.to_string()).collect::<Vec<_>>(),
        )
    }

    #[test]
    fn noise_does_not_matter() {
        let kindle = fp(
            "The Orchard Ledger: A Year in Rings (Seasons Book 2)",
            &["Penn, Mara"],
        );
        let calibre = fp("Orchard Ledger", &["Mara Penn"]);
        assert_eq!(kindle.score(&calibre), 1.0);
    }

    #[test]
    fn different_books_score_low() {
        let a = fp("The Orchard Ledger", &["Mara Penn"]);
        assert!(a.score(&fp("Field Pamphlet", &["Abel Stone"])) < 0.2);
        // Same series word, different book.
        assert!(a.score(&fp("The Orchard Almanac", &["Mara Penn"])) < 0.85);
    }
}
