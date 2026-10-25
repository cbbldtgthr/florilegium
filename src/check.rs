//! `check`: everything about an archive that must hold for its IDs to be
//! trusted. It reads only; fixing is the user's call, made in the editor.

use std::collections::BTreeMap;
use std::path::Path;

use crate::archive::{Archive, Entry, EntryType};
use crate::id::key_v1;
use crate::norm::norm_v1;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Issue {
    pub file: String,
    /// The entry concerned, if the issue is about one.
    pub id: Option<String>,
    pub message: String,
}

/// Check the archive in `dir`. A file that cannot be loaded at all is the one
/// issue reported, since nothing else can be checked without it.
pub fn check(dir: &Path) -> Vec<Issue> {
    let archive = match Archive::load(dir) {
        Ok(a) => a,
        Err(message) => {
            return vec![Issue {
                file: dir.display().to_string(),
                id: None,
                message,
            }];
        }
    };
    let mut issues = Vec::new();
    let mut issue = |file: &str, id: Option<&str>, message: String| {
        issues.push(Issue {
            file: file.to_string(),
            id: id.map(String::from),
            message,
        });
    };

    // Every ID and alias, with the (file, entry ID) holding it.
    let mut primary: BTreeMap<&str, Vec<(&str, &str)>> = BTreeMap::new();
    let mut alias: BTreeMap<&str, Vec<(&str, &str)>> = BTreeMap::new();
    let mut calibre_books: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    let mut title_keys: BTreeMap<String, Vec<&str>> = BTreeMap::new();

    for b in &archive.books {
        let file = b.file_name.as_str();
        if !b.is_canonical_on_disk() {
            issue(
                file,
                None,
                "not in canonical form; the next ingest will rewrite it".into(),
            );
        }
        if let Some(u) = &b.data.book.calibre_uuid {
            calibre_books.entry(u).or_default().push(file);
        }
        let mut keys: Vec<String> = b
            .data
            .book
            .kindle_titles
            .iter()
            .map(|t| key_v1(t))
            .collect();
        keys.sort();
        keys.dedup();
        for k in keys {
            title_keys.entry(k).or_default().push(file);
        }

        for e in &b.data.entries {
            let id = e.id.as_str();
            primary.entry(id).or_default().push((file, id));
            for a in &e.aliases {
                alias.entry(a).or_default().push((file, id));
                if a == &e.id {
                    issue(file, Some(id), "lists its own ID as an alias".into());
                }
            }
            if !is_id_shaped(&e.id) {
                issue(
                    file,
                    Some(id),
                    "ID is not 22 characters of base64url".into(),
                );
            }
            for a in e.aliases.iter().filter(|a| !is_id_shaped(a)) {
                issue(
                    file,
                    Some(id),
                    format!("alias {a:?} is not 22 characters of base64url"),
                );
            }
            match e.kind {
                EntryType::Highlight if e.text.as_deref().is_none_or(str::is_empty) => {
                    issue(file, Some(id), "highlight has no text".into())
                }
                EntryType::Note if e.text.is_some() => issue(
                    file,
                    Some(id),
                    "orphan note has text; it should be null".into(),
                ),
                EntryType::Note if e.note.as_deref().is_none_or(str::is_empty) => {
                    issue(file, Some(id), "orphan note has no note".into())
                }
                _ => {}
            }
            if let Some(c) = &e.created
                && !is_timestamp_shaped(c)
            {
                issue(
                    file,
                    Some(id),
                    format!("created {c:?} is not YYYY-MM-DDTHH:MM:SS.mmmZ"),
                );
            }
            if e.sources.is_empty() {
                issue(file, Some(id), "has no sources".into());
            }
            if e.sources.iter().any(|s| s == "kindle") != e.kindle.is_some() {
                issue(
                    file,
                    Some(id),
                    "kindle block and kindle source disagree: each needs the other".into(),
                );
            }
            if let Some(calibre_text) = native_text(e)
                && Some(norm_v1(calibre_text)) != e.text.as_deref().map(norm_v1)
            {
                issue(
                    file,
                    Some(id),
                    "text differs from the calibre highlight it is anchored to (did a Kindle ingest overwrite it?)".into(),
                );
            }
        }
    }

    for (id, holders) in &primary {
        if holders.len() > 1 {
            issue(
                holders[0].0,
                Some(id),
                format!(
                    "ID is used by {} entries ({})",
                    holders.len(),
                    files(holders)
                ),
            );
        }
        if let Some(by) = alias.get(id) {
            for (file, owner) in by.iter().filter(|(_, owner)| owner != id) {
                issue(
                    file,
                    Some(owner),
                    format!("alias {id} is another entry's ID ({})", files(holders)),
                );
            }
        }
    }
    for (a, holders) in &alias {
        let owners: std::collections::BTreeSet<&str> =
            holders.iter().map(|(_, owner)| *owner).collect();
        if owners.len() > 1 {
            let ids: Vec<&str> = owners.into_iter().collect();
            issue(
                holders[0].0,
                None,
                format!("alias {a} belongs to several entries: {}", ids.join(", ")),
            );
        }
    }
    for (u, holders) in &calibre_books {
        if holders.len() > 1 {
            issue(
                holders[0],
                None,
                format!(
                    "calibre book {u} is claimed by {} files: {}",
                    holders.len(),
                    holders.join(", ")
                ),
            );
        }
    }
    for (k, holders) in &title_keys {
        if holders.len() > 1 {
            issue(
                holders[0],
                None,
                format!(
                    "Kindle title {k:?} is listed in several files: {}",
                    holders.join(", ")
                ),
            );
        }
    }

    issues.sort_by(|a, b| (&a.file, &a.id, &a.message).cmp(&(&b.file, &b.id, &b.message)));
    issues.dedup();
    issues
}

fn files(holders: &[(&str, &str)]) -> String {
    let mut f: Vec<&str> = holders.iter().map(|(file, _)| *file).collect();
    f.dedup();
    f.join(", ")
}

/// calibre's own highlighted text, for an entry whose CFIs are calibre's.
fn native_text(e: &Entry) -> Option<&str> {
    let c = e.calibre.as_ref()?;
    (c.get("match")?.as_str()? == "native").then_some(())?;
    c.get("highlighted_text")?.as_str()
}

/// calibre's annotation ID shape, which minted IDs share.
pub fn is_id_shaped(id: &str) -> bool {
    id.len() == 22
        && id
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_')
}

fn is_timestamp_shaped(t: &str) -> bool {
    let b = t.as_bytes();
    let digits = |r: std::ops::Range<usize>| r.into_iter().all(|i| b[i].is_ascii_digit());
    b.len() == 24
        && digits(0..4)
        && b[4] == b'-'
        && digits(5..7)
        && b[7] == b'-'
        && digits(8..10)
        && b[10] == b'T'
        && digits(11..13)
        && b[13] == b':'
        && digits(14..16)
        && b[16] == b':'
        && digits(17..19)
        && b[19] == b'.'
        && digits(20..23)
        && b[23] == b'Z'
}
