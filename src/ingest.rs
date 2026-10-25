//! `ingest-clippings`: the whole clippings file, replayed and merged into the
//! archive by identity. The archive ends up a pure function of the file and
//! what was already there.

use std::collections::BTreeSet;
use std::path::Path;

use crate::archive::{Archive, Book, SaveReport};
use crate::config::Config;
use crate::id::{EntryKind, key_v1};
use crate::kindle::parse::{Parsed, parse};
use crate::kindle::replay::replay;
use crate::kindle::{self};
use crate::merge::{Outcome, merge};

#[derive(Debug, Default)]
pub struct BookReport {
    pub file_name: String,
    pub title: String,
    pub new_file: bool,
    pub created: usize,
    pub updated: usize,
    pub unchanged: usize,
    pub tombstone_matched: usize,
    /// Surviving ID and the IDs it absorbed.
    pub merges: Vec<(String, Vec<String>)>,
}

impl BookReport {
    pub fn changed(&self) -> bool {
        self.new_file || self.created + self.updated + self.merges.len() > 0
    }
}

#[derive(Debug)]
pub struct IngestReport {
    pub parsed: Parsed,
    pub books: Vec<BookReport>,
    /// (book title, line, note text) for notes no highlight contains.
    pub orphan_notes: Vec<(String, usize, String)>,
    /// (book title, line of the highlight) where several notes were joined.
    pub joined_notes: Vec<(String, usize)>,
    pub warnings: Vec<String>,
    pub saved: SaveReport,
}

pub fn ingest_clippings(
    input: &str,
    archive_dir: &Path,
    config: &Config,
    dry_run: bool,
) -> Result<IngestReport, String> {
    if !archive_dir.is_dir() {
        return Err(format!(
            "archive directory {} does not exist; create it first",
            archive_dir.display()
        ));
    }
    let parsed = parse(input, &config.parse_config())?;
    let books = replay(&parsed.clippings, &config.replay_config());
    let mut archive = Archive::load(archive_dir)?;

    let mut reports = Vec::new();
    let mut orphan_notes = Vec::new();
    let mut joined_notes = Vec::new();
    let mut warnings = Vec::new();

    for book in &books {
        let first = &book.clusters[0].latest;
        let matching: Vec<usize> = archive
            .books
            .iter()
            .enumerate()
            .filter(|(_, b)| {
                b.data
                    .book
                    .kindle_titles
                    .iter()
                    .any(|t| key_v1(t) == book.key)
            })
            .map(|(i, _)| i)
            .collect();
        if matching.len() > 1 {
            let files: Vec<&str> = matching
                .iter()
                .map(|&i| archive.books[i].file_name.as_str())
                .collect();
            warnings.push(format!(
                "{:?} matches several archive files ({}); used the first. Merge them with merge-books.",
                book.title_lines[0],
                files.join(", ")
            ));
        }
        let new_file = matching.is_empty();
        let index = match matching.first() {
            Some(&i) => i,
            None => {
                let authors = first
                    .author
                    .as_deref()
                    .map(|a| {
                        a.split(';')
                            .map(str::trim)
                            .filter(|a| !a.is_empty())
                            .map(String::from)
                            .collect()
                    })
                    .unwrap_or_default();
                let meta = Book {
                    title: first.title.clone(),
                    authors,
                    kindle_titles: Vec::new(),
                    calibre_uuid: None,
                    other: Default::default(),
                };
                archive.add_book(meta, &book.key);
                archive.books.len() - 1
            }
        };
        let loaded = &mut archive.books[index];
        loaded
            .data
            .book
            .kindle_titles
            .extend(book.title_lines.iter().cloned());

        let mut report = BookReport {
            file_name: loaded.file_name.clone(),
            title: loaded.data.book.title.clone(),
            new_file,
            ..BookReport::default()
        };
        let mut touched = BTreeSet::new();
        for cluster in &book.clusters {
            if cluster.kind == EntryKind::Note {
                orphan_notes.push((
                    report.title.clone(),
                    cluster.latest.line,
                    cluster.latest.text.clone(),
                ));
            }
            if cluster.note_parts > 1 {
                joined_notes.push((report.title.clone(), cluster.latest.line));
            }
            let outcome = merge(&mut loaded.data, &kindle::incoming(cluster));
            let id = match &outcome {
                Outcome::Created { id }
                | Outcome::Updated { id }
                | Outcome::Unchanged { id }
                | Outcome::TombstoneMatched { id }
                | Outcome::Merged { id, .. } => id.clone(),
            };
            if !touched.insert(id.clone()) {
                warnings.push(format!(
                    "entry {id} in {} matched more than one clipping cluster; the last one won",
                    report.file_name
                ));
            }
            match outcome {
                Outcome::Created { .. } => report.created += 1,
                Outcome::Updated { .. } => report.updated += 1,
                Outcome::Unchanged { .. } => report.unchanged += 1,
                Outcome::TombstoneMatched { .. } => report.tombstone_matched += 1,
                Outcome::Merged { id, absorbed } => report.merges.push((id, absorbed)),
            }
        }
        reports.push(report);
    }

    let saved = archive.save(dry_run)?;
    Ok(IngestReport {
        parsed,
        books: reports,
        orphan_notes,
        joined_notes,
        warnings,
        saved,
    })
}
