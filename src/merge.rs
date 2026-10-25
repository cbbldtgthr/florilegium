//! Merge incoming entries into an archive book by identity alone.
//!
//! No overlap, text or location judgment happens here: that belongs to each
//! source (the Kindle replay). An incoming entry matches archive entries that
//! share any ID or alias with it, and nothing else.

use std::collections::BTreeSet;

use serde_json::Value;

use crate::archive::{BookFile, Entry, EntryType, KindleBlock};

/// One entry as a source delivers it, before it meets the archive.
#[derive(Debug, Clone, PartialEq)]
pub struct Incoming {
    /// Every ID the entry has had, earliest first, without duplicates. The
    /// first is the ID a new archive entry gets.
    pub ids: Vec<String>,
    pub kind: EntryType,
    pub text: Option<String>,
    pub note: Option<String>,
    pub created: Option<String>,
    pub block: SourceBlock,
}

/// The source-specific part of an entry, stored in that source's block.
#[derive(Debug, Clone, PartialEq)]
pub enum SourceBlock {
    Kindle(KindleBlock),
}

impl SourceBlock {
    fn source(&self) -> &'static str {
        match self {
            SourceBlock::Kindle(_) => "kindle",
        }
    }

    fn store(&self, e: &mut Entry) {
        match self {
            SourceBlock::Kindle(k) => e.kindle = Some(k.clone()),
        }
    }
}

/// calibre match values whose data is derived from the text and must be
/// recomputed when the text changes. `native` and `manual` are never touched.
const DERIVED_MATCHES: [&str; 4] = ["exact", "approximate", "ambiguous", "unmatched"];

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    Created {
        id: String,
    },
    Updated {
        id: String,
    },
    Unchanged {
        id: String,
    },
    /// Matched a tombstone; only its aliases may have grown.
    TombstoneMatched {
        id: String,
    },
    /// Several archive entries were one passage; `absorbed` are the losers'
    /// IDs, now aliases of `id`.
    Merged {
        id: String,
        absorbed: Vec<String>,
    },
}

/// Merge one incoming entry into `file`.
pub fn merge(file: &mut BookFile, incoming: &Incoming) -> Outcome {
    let wanted: BTreeSet<&str> = incoming.ids.iter().map(String::as_str).collect();
    let matched: Vec<usize> = file
        .entries
        .iter()
        .enumerate()
        .filter(|(_, e)| identities(e).any(|i| wanted.contains(i)))
        .map(|(i, _)| i)
        .collect();

    match matched.as_slice() {
        [] => {
            let entry = create(incoming);
            let id = entry.id.clone();
            file.entries.push(entry);
            Outcome::Created { id }
        }
        [one] => {
            let entry = &mut file.entries[*one];
            let before = entry.clone();
            let id = entry.id.clone();
            add_aliases(entry, &incoming.ids);
            if entry.deleted {
                return Outcome::TombstoneMatched { id };
            }
            update(entry, incoming);
            if *entry == before {
                Outcome::Unchanged { id }
            } else {
                Outcome::Updated { id }
            }
        }
        many => {
            let survivor = pick_survivor(file, many, &incoming.ids);
            let mut merged = file.entries[survivor].clone();
            let mut absorbed = Vec::new();
            for &i in many.iter().filter(|&&i| i != survivor) {
                let loser = &file.entries[i];
                absorbed.push(loser.id.clone());
                let loser_ids: Vec<String> = identities(loser).map(String::from).collect();
                add_aliases(&mut merged, &loser_ids);
                merged.sources.extend(loser.sources.iter().cloned());
                merged.deleted |= loser.deleted;
            }
            add_aliases(&mut merged, &incoming.ids);
            if !merged.deleted {
                update(&mut merged, incoming);
            }
            absorbed.sort();
            let id = merged.id.clone();
            file.entries[survivor] = merged;
            let mut losers: Vec<usize> = many.iter().copied().filter(|&i| i != survivor).collect();
            losers.sort_unstable_by(|a, b| b.cmp(a));
            for i in losers {
                file.entries.remove(i);
            }
            Outcome::Merged { id, absorbed }
        }
    }
}

fn identities(e: &Entry) -> impl Iterator<Item = &str> {
    std::iter::once(e.id.as_str()).chain(e.aliases.iter().map(String::as_str))
}

/// Aliases are only ever added. The entry's own ID is never its alias.
fn add_aliases(e: &mut Entry, ids: &[String]) {
    for id in ids {
        if *id != e.id && !e.aliases.contains(id) {
            e.aliases.push(id.clone());
        }
    }
    e.aliases.sort();
}

/// The matched entry whose ID the source saw earliest; failing that, the
/// lexicographically smallest ID.
fn pick_survivor(file: &BookFile, matched: &[usize], order: &[String]) -> usize {
    let rank = |i: &usize| {
        let id = &file.entries[*i].id;
        (
            order.iter().position(|o| o == id).unwrap_or(usize::MAX),
            id.clone(),
        )
    };
    *matched.iter().min_by_key(|i| rank(i)).unwrap()
}

fn create(incoming: &Incoming) -> Entry {
    let mut e = Entry {
        id: incoming.ids[0].clone(),
        aliases: Vec::new(),
        kind: incoming.kind,
        text: None,
        note: None,
        created: None,
        deleted: false,
        sources: Vec::new(),
        kindle: None,
        calibre: None,
        other: Default::default(),
    };
    add_aliases(&mut e, &incoming.ids);
    update(&mut e, incoming);
    e
}

/// Last ingest wins per field, on purpose: the user reviews every change in
/// the diff, so there is no conflict machinery.
fn update(e: &mut Entry, incoming: &Incoming) {
    if e.text != incoming.text && has_derived_match(e) {
        e.calibre = None;
    }
    e.text = incoming.text.clone();
    e.note = incoming.note.clone();
    e.created = incoming.created.clone();
    let source = incoming.block.source().to_string();
    if !e.sources.contains(&source) {
        e.sources.push(source);
        e.sources.sort();
    }
    incoming.block.store(e);
}

fn has_derived_match(e: &Entry) -> bool {
    e.calibre
        .as_ref()
        .and_then(|c| c.get("match"))
        .and_then(Value::as_str)
        .is_some_and(|m| DERIVED_MATCHES.contains(&m))
}
