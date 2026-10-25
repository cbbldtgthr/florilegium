//! Ingest commands: a source read whole and merged into the archive by
//! identity, so the archive is a pure function of its sources and what was
//! already there.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use crate::archive::{Archive, Book, BookFile, SaveReport};
use crate::calibre::db::{Library, VIEWER_USER, VIEWER_USER_TYPE};
use crate::calibre::{self, Skip};
use crate::config::Config;
use crate::id::{EntryKind, key_v1};
use crate::kindle::parse::{Parsed, parse};
use crate::kindle::replay::replay;
use crate::kindle::{self};
use crate::merge::{Incoming, Outcome, merge};

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

    /// Merge one incoming entry into `file` and count the outcome. `touched`
    /// catches an entry matched twice in one run.
    fn apply(
        &mut self,
        file: &mut BookFile,
        incoming: &Incoming,
        touched: &mut BTreeSet<String>,
        warnings: &mut Vec<String>,
    ) {
        let outcome = merge(file, incoming);
        let id = match &outcome {
            Outcome::Created { id }
            | Outcome::Updated { id }
            | Outcome::Unchanged { id }
            | Outcome::TombstoneMatched { id }
            | Outcome::Merged { id, .. } => id.clone(),
        };
        if !touched.insert(id.clone()) {
            warnings.push(format!(
                "entry {id} in {} matched more than one incoming entry; the last one won",
                self.file_name
            ));
        }
        match outcome {
            Outcome::Created { .. } => self.created += 1,
            Outcome::Updated { .. } => self.updated += 1,
            Outcome::Unchanged { .. } => self.unchanged += 1,
            Outcome::TombstoneMatched { .. } => self.tombstone_matched += 1,
            Outcome::Merged { id, absorbed } => self.merges.push((id, absorbed)),
        }
    }
}

fn require_dir(archive_dir: &Path) -> Result<(), String> {
    if archive_dir.is_dir() {
        Ok(())
    } else {
        Err(format!(
            "archive directory {} does not exist; create it first",
            archive_dir.display()
        ))
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
    require_dir(archive_dir)?;
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
            report.apply(
                &mut loaded.data,
                &kindle::incoming(cluster),
                &mut touched,
                &mut warnings,
            );
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

#[derive(Debug)]
pub struct CalibreIngestReport {
    pub books: Vec<BookReport>,
    /// Viewer highlights deleted in calibre (stubs kept for syncing).
    pub removed: usize,
    /// Viewer highlights read, by book format.
    pub formats: BTreeMap<String, usize>,
    pub warnings: Vec<String>,
    pub saved: SaveReport,
}

/// Read calibre's viewer highlights, on every format, into the archive. A
/// book's file is found by its calibre book UUID, or created.
pub fn ingest_calibre(
    library: &Path,
    archive_dir: &Path,
    only_books: &[i64],
    dry_run: bool,
) -> Result<CalibreIngestReport, String> {
    require_dir(archive_dir)?;
    let lib = Library::open(library)?;
    let books = lib.books()?;
    let rows = lib.annotations(only_books)?;
    let mut archive = Archive::load(archive_dir)?;

    let mut removed = 0;
    let mut formats: BTreeMap<String, usize> = BTreeMap::new();
    let mut warnings = Vec::new();
    let mut by_book: BTreeMap<i64, Vec<(String, Incoming)>> = BTreeMap::new();
    for row in rows.iter().filter(|r| {
        r.user_type == VIEWER_USER_TYPE && r.user == VIEWER_USER && r.annot_type == "highlight"
    }) {
        match calibre::incoming(row) {
            Ok(i) => by_book
                .entry(row.book)
                .or_default()
                .push((row.format.clone(), i)),
            Err(Skip::Removed) => removed += 1,
            Err(Skip::Unreadable(w)) => warnings.push(w),
        }
    }

    let mut reports = Vec::new();
    for (book_id, incomings) in &by_book {
        let Some(book) = books.get(book_id) else {
            warnings.push(format!(
                "annotations for calibre book {book_id}, which does not exist"
            ));
            continue;
        };
        let Some(uuid) = &book.uuid else {
            warnings.push(format!(
                "calibre book {book_id} ({:?}) has no UUID; skipped",
                book.title
            ));
            continue;
        };
        let matching: Vec<usize> = archive
            .books
            .iter()
            .enumerate()
            .filter(|(_, b)| b.data.book.calibre_uuid.as_ref() == Some(uuid))
            .map(|(i, _)| i)
            .collect();
        if matching.len() > 1 {
            warnings.push(format!(
                "calibre book {uuid} ({:?}) is claimed by several archive files; used the first. Merge them with merge-books.",
                book.title
            ));
        }
        let new_file = matching.is_empty();
        let index = match matching.first() {
            Some(&i) => i,
            None => {
                let meta = Book {
                    title: book.title.clone(),
                    authors: book.authors.clone(),
                    kindle_titles: Vec::new(),
                    calibre_uuid: Some(uuid.clone()),
                    other: Default::default(),
                };
                archive.add_book(meta, &format!("calibre:{uuid}"));
                archive.books.len() - 1
            }
        };
        let loaded = &mut archive.books[index];
        let mut report = BookReport {
            file_name: loaded.file_name.clone(),
            title: loaded.data.book.title.clone(),
            new_file,
            ..BookReport::default()
        };
        let mut touched = BTreeSet::new();
        for (format, incoming) in incomings {
            *formats.entry(format.clone()).or_default() += 1;
            report.apply(&mut loaded.data, incoming, &mut touched, &mut warnings);
        }
        reports.push(report);
    }

    let saved = archive.save(dry_run)?;
    Ok(CalibreIngestReport {
        books: reports,
        removed,
        formats,
        warnings,
        saved,
    })
}
