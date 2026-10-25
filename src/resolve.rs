//! `resolve`: the canonical lookup behind every citation. Any ID an entry
//! has ever had finds it, so a citation written years ago still lands.

use std::path::Path;

use crate::archive::{Archive, Book, Entry};

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

pub fn resolve(dir: &Path, queried: &str) -> Result<Lookup, String> {
    let archive = Archive::load(dir)?;
    let mut hits: Vec<Resolved> = Vec::new();
    for b in archive.books {
        for e in &b.data.entries {
            if e.id == queried || e.aliases.iter().any(|a| a == queried) {
                hits.push(Resolved {
                    queried: queried.to_string(),
                    file_name: b.file_name.clone(),
                    book: b.data.book.clone(),
                    entry: e.clone(),
                });
            }
        }
    }
    Ok(match hits.len() {
        0 => Lookup::NotFound,
        1 => {
            let r = Box::new(hits.pop().unwrap());
            if r.entry.deleted {
                Lookup::Deleted(r)
            } else {
                Lookup::Found(r)
            }
        }
        _ => Lookup::Ambiguous(
            hits.into_iter()
                .map(|r| format!("{} in {}", r.entry.id, r.file_name))
                .collect(),
        ),
    })
}
