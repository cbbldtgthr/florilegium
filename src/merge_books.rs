//! `merge-books`: two archive files that turn out to be one book (say, a
//! Kindle-only file and a calibre-only one) become one file, by hand.
//!
//! Only books are joined. Entries keep their identities: a passage
//! highlighted on both devices stays two entries, which the user may merge by
//! adding one's ID to the other's aliases.

use std::collections::BTreeSet;
use std::path::Path;

use crate::archive::{Archive, SaveReport};

#[derive(Debug)]
pub struct MergeBooksReport {
    pub kept: String,
    pub absorbed: String,
    pub moved_entries: usize,
    pub added_kindle_titles: Vec<String>,
    /// The calibre book UUID the kept file gained, if it had none.
    pub took_calibre_uuid: Option<String>,
    pub saved: SaveReport,
}

/// Move every entry and key of the book file `absorb` into `keep`, then
/// delete `absorb`. Files are named as listed in the archive, with or
/// without `.json`.
pub fn merge_books(
    dir: &Path,
    keep: &str,
    absorb: &str,
    dry_run: bool,
) -> Result<MergeBooksReport, String> {
    let mut archive = Archive::load(dir)?;
    let keep_i = find(&archive, keep)?;
    let absorb_i = find(&archive, absorb)?;
    if keep_i == absorb_i {
        return Err(format!("{keep} and {absorb} are the same file"));
    }

    let (k, a) = (&archive.books[keep_i].data, &archive.books[absorb_i].data);
    if let (Some(ku), Some(au)) = (&k.book.calibre_uuid, &a.book.calibre_uuid)
        && ku != au
    {
        return Err(format!(
            "they are different calibre books ({ku} and {au}); not merging"
        ));
    }
    let keep_ids: BTreeSet<&str> = k.entries.iter().flat_map(identities).collect();
    let mut shared: Vec<&str> = a
        .entries
        .iter()
        .flat_map(identities)
        .filter(|i| keep_ids.contains(i))
        .collect();
    shared.sort();
    shared.dedup();
    if !shared.is_empty() {
        return Err(format!(
            "both files hold entries with the ID(s) {}; run check and fix those first",
            shared.join(", ")
        ));
    }

    let absorbed = archive.remove_book(absorb_i);
    let keep_i = if absorb_i < keep_i {
        keep_i - 1
    } else {
        keep_i
    };
    let kept = &mut archive.books[keep_i];
    let book = &mut kept.data.book;
    let added_kindle_titles: Vec<String> = absorbed
        .data
        .book
        .kindle_titles
        .iter()
        .filter(|t| !book.kindle_titles.contains(t))
        .cloned()
        .collect();
    book.kindle_titles
        .extend(added_kindle_titles.iter().cloned());
    let took_calibre_uuid = match (&book.calibre_uuid, &absorbed.data.book.calibre_uuid) {
        (None, Some(u)) => {
            book.calibre_uuid = Some(u.clone());
            Some(u.clone())
        }
        _ => None,
    };
    for (k, v) in &absorbed.data.book.other {
        book.other.entry(k.clone()).or_insert_with(|| v.clone());
    }
    let moved_entries = absorbed.data.entries.len();
    kept.data.entries.extend(absorbed.data.entries);
    let kept_name = kept.file_name.clone();

    let saved = archive.save(dry_run)?;
    Ok(MergeBooksReport {
        kept: kept_name,
        absorbed: absorbed.file_name,
        moved_entries,
        added_kindle_titles,
        took_calibre_uuid,
        saved,
    })
}

fn identities(e: &crate::archive::Entry) -> impl Iterator<Item = &str> {
    std::iter::once(e.id.as_str()).chain(e.aliases.iter().map(String::as_str))
}

fn find(archive: &Archive, name: &str) -> Result<usize, String> {
    let name = Path::new(name)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(name);
    let with_ext = if name.ends_with(".json") {
        name.to_string()
    } else {
        format!("{name}.json")
    };
    archive
        .books
        .iter()
        .position(|b| b.file_name == with_ext)
        .ok_or_else(|| format!("no book file {with_ext} in {}", archive.dir.display()))
}
