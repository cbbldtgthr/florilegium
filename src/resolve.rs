//! `resolve`: the canonical lookup behind every citation. Any ID an entry
//! has ever had finds it, so a citation written years ago still lands.
//!
//! [`Clipping`] is the public shape other tools integrate against. It is
//! deliberately not the archive entry: the archive's internals (calibre
//! anchors, sources, aliases) can change without breaking a consumer. Its
//! `version` changes only when a field changes meaning or goes away.

use std::collections::HashMap;
use std::path::Path;

use serde::Serialize;

use crate::archive::{Archive, Book, Entry, EntryType};

/// Bumped when a [`Clipping`] field changes meaning or is removed; adding a
/// field does not bump it.
pub const CLIPPING_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq)]
pub struct Resolved {
    /// The ID that was asked for.
    pub queried: String,
    pub file_name: String,
    pub book: Book,
    pub entry: Entry,
}

impl Resolved {
    /// The entry's current ID, which may differ from the one asked for.
    pub fn canonical_id(&self) -> &str {
        &self.entry.id
    }

    pub fn via_alias(&self) -> bool {
        self.queried != self.entry.id
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Lookup {
    Found(Box<Resolved>),
    /// The entry exists but is a tombstone.
    Deleted(Box<Resolved>),
    NotFound,
    /// More than one entry claims the ID; `check` reports how.
    Ambiguous(Vec<String>),
}

/// Every ID and alias in an archive, indexed once for many lookups.
pub struct Resolver {
    hits: HashMap<String, Vec<Resolved>>,
}

impl Resolver {
    pub fn new(dir: &Path) -> Result<Resolver, String> {
        let archive = Archive::load(dir)?;
        let mut hits: HashMap<String, Vec<Resolved>> = HashMap::new();
        for b in archive.books {
            for e in &b.data.entries {
                for id in std::iter::once(&e.id).chain(&e.aliases) {
                    hits.entry(id.clone()).or_default().push(Resolved {
                        queried: id.clone(),
                        file_name: b.file_name.clone(),
                        book: b.data.book.clone(),
                        entry: e.clone(),
                    });
                }
            }
        }
        Ok(Resolver { hits })
    }

    pub fn lookup(&self, queried: &str) -> Lookup {
        let Some(hits) = self.hits.get(queried) else {
            return Lookup::NotFound;
        };
        match hits.as_slice() {
            [one] => {
                let r = Box::new(one.clone());
                if r.entry.deleted {
                    Lookup::Deleted(r)
                } else {
                    Lookup::Found(r)
                }
            }
            many => Lookup::Ambiguous(
                many.iter()
                    .map(|r| format!("{} in {}", r.entry.id, r.file_name))
                    .collect(),
            ),
        }
    }
}

pub fn resolve(dir: &Path, queried: &str) -> Result<Lookup, String> {
    Ok(Resolver::new(dir)?.lookup(queried))
}

/// The public, stable shape of one looked-up clipping. Every field is always
/// present, `null` or empty when it does not apply.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Clipping {
    pub version: u32,
    /// The ID that was asked for.
    pub queried: String,
    /// `found`, `deleted`, `unknown` or `ambiguous`.
    pub status: &'static str,
    /// The entry's current ID (differs from `queried` for an old alias).
    pub id: Option<String>,
    pub book: Option<ClippingBook>,
    /// Chapter titles, outermost first, from the book's table of contents.
    pub chapter: Vec<String>,
    /// `highlight`, or `note` for a note attached to no highlight.
    #[serde(rename = "type")]
    pub kind: Option<&'static str>,
    pub text: Option<String>,
    pub note: Option<String>,
    /// UTC, `YYYY-MM-DDTHH:MM:SS.mmmZ`.
    pub created: Option<String>,
    /// Kindle location range, when known.
    pub location: Option<[u32; 2]>,
    /// Kindle page range, when known.
    pub page: Option<[u32; 2]>,
    /// For `ambiguous`: the entries claiming the ID.
    pub claimants: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ClippingBook {
    pub title: String,
    pub authors: Vec<String>,
    pub calibre_uuid: Option<String>,
}

impl Clipping {
    /// The public view of a lookup. A deleted entry's content is included only
    /// with `show_deleted`.
    pub fn of(queried: &str, lookup: &Lookup, show_deleted: bool) -> Clipping {
        let mut c = Clipping {
            version: CLIPPING_VERSION,
            queried: queried.to_string(),
            status: "unknown",
            id: None,
            book: None,
            chapter: Vec::new(),
            kind: None,
            text: None,
            note: None,
            created: None,
            location: None,
            page: None,
            claimants: Vec::new(),
        };
        let (r, with_content) = match lookup {
            Lookup::NotFound => return c,
            Lookup::Ambiguous(claimants) => {
                c.status = "ambiguous";
                c.claimants = claimants.clone();
                return c;
            }
            Lookup::Found(r) => {
                c.status = "found";
                (r, true)
            }
            Lookup::Deleted(r) => {
                c.status = "deleted";
                (r, show_deleted)
            }
        };
        c.id = Some(r.entry.id.clone());
        c.book = Some(ClippingBook {
            title: r.book.title.clone(),
            authors: r.book.authors.clone(),
            calibre_uuid: r.book.calibre_uuid.clone(),
        });
        if with_content {
            let e = &r.entry;
            c.chapter = crate::list::chapter_titles(e);
            c.kind = Some(match e.kind {
                EntryType::Highlight => "highlight",
                EntryType::Note => "note",
            });
            c.text = e.text.clone();
            c.note = e.note.clone();
            c.created = e.created.clone();
            c.location = e.kindle.as_ref().and_then(|k| k.location);
            c.page = e.kindle.as_ref().and_then(|k| k.page);
        }
        c
    }
}

/// IDs are base64url, so one can start with `-` and look like a flag. For
/// `resolve`, move every ID-shaped argument (not the value of `--archive`)
/// after a `--`, keeping their order, so flags work before or after them.
pub fn ids_after_double_dash(args: Vec<String>) -> Vec<String> {
    if args.get(1).map(String::as_str) != Some("resolve") || args.iter().any(|a| a == "--") {
        return args;
    }
    let (mut rest, mut ids) = (args[..2].to_vec(), Vec::new());
    let mut value_next = false;
    for a in &args[2..] {
        if value_next {
            rest.push(a.clone());
            value_next = false;
        } else if a == "--archive" {
            rest.push(a.clone());
            value_next = true;
        } else if crate::check::is_id_shaped(a) {
            ids.push(a.clone());
        } else {
            rest.push(a.clone());
        }
    }
    rest.push("--".into());
    rest.extend(ids);
    rest
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(a: &[&str]) -> Vec<String> {
        a.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn hyphen_led_ids_never_parse_as_flags() {
        assert_eq!(
            ids_after_double_dash(args(&[
                "aa",
                "resolve",
                "-QaKIS3PWPq1eiVpBvD80g",
                "--json",
                "LS2ZErNkVN6T3-tktmAZ6A"
            ])),
            args(&[
                "aa",
                "resolve",
                "--json",
                "--",
                "-QaKIS3PWPq1eiVpBvD80g",
                "LS2ZErNkVN6T3-tktmAZ6A"
            ])
        );
        // The value of --archive stays with it, even if ID-shaped.
        assert_eq!(
            ids_after_double_dash(args(&[
                "aa",
                "resolve",
                "--archive",
                "Abcdefghijklmnopqrstuv",
                "x"
            ])),
            args(&[
                "aa",
                "resolve",
                "--archive",
                "Abcdefghijklmnopqrstuv",
                "x",
                "--"
            ])
        );
        // An explicit -- and other commands are left alone.
        let explicit = args(&["aa", "resolve", "--", "-QaKIS3PWPq1eiVpBvD80g"]);
        assert_eq!(ids_after_double_dash(explicit.clone()), explicit);
        let other = args(&["aa", "check", "-QaKIS3PWPq1eiVpBvD80g"]);
        assert_eq!(ids_after_double_dash(other.clone()), other);
    }
}
