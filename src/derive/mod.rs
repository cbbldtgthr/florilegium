//! `derive-calibre`: work out where in calibre's EPUB each Kindle highlight
//! sits, and record it as the entry's derived `calibre` block. Derived data
//! only: it never touches identity, and never a `native` or `manual` block.

pub mod books;
pub mod locate;

use std::collections::BTreeMap;
use std::path::Path;

use serde_json::{Map, Value, json};

use crate::archive::{Archive, Entry, EntryType, SaveReport};
use crate::calibre::WRITABLE_FORMAT;
use crate::calibre::db::Library;
use books::{BookMatch, Candidate, How, MatchConfig, Overrides, match_book};
use locate::{BookText, Hit, Located};

#[derive(Debug, Clone)]
pub struct DeriveConfig {
    pub book_match: MatchConfig,
    /// Similarity (0 to 1) a fuzzy window needs to count as `approximate`.
    pub approximate_threshold: f64,
    /// A repeated passage is placed by Kindle location only if the nearest
    /// occurrence is at most this fraction of the runner-up's distance.
    pub disambiguation_ratio: f64,
    pub style: Value,
}

impl Default for DeriveConfig {
    fn default() -> Self {
        DeriveConfig {
            book_match: MatchConfig::default(),
            approximate_threshold: 0.9,
            disambiguation_ratio: 0.5,
            style: json!({"kind": "color", "type": "builtin", "which": "yellow"}),
        }
    }
}

/// Runner-up locations kept on an ambiguous entry, nearest first; a short
/// highlight can occur hundreds of times, which is noise in every diff.
const MAX_CANDIDATES: usize = 10;

pub fn derived_with() -> String {
    format!("annot-archive {}", env!("CARGO_PKG_VERSION"))
}

#[derive(Debug, Clone, PartialEq)]
pub enum BookOutcome {
    Matched {
        calibre_title: String,
        how: How,
    },
    NoCalibreBook {
        candidates: Vec<Candidate>,
    },
    /// The archive names a calibre book the library no longer has.
    Gone {
        calibre_uuid: String,
    },
    NoEpub {
        calibre_title: String,
    },
    UnreadableEpub {
        error: String,
    },
    /// The calibre book already has its own archive file; merge them first.
    ClaimedByOtherFile {
        calibre_title: String,
        other_file: String,
    },
}

#[derive(Debug)]
pub struct BookDerive {
    pub file_name: String,
    pub title: String,
    /// A Kindle title line of the book, for writing an override.
    pub kindle_title: Option<String>,
    pub outcome: BookOutcome,
    /// Entries derived, by `match` (and `reason` for unmatched).
    pub counts: BTreeMap<String, usize>,
}

#[derive(Debug)]
pub struct DeriveReport {
    pub books: Vec<BookDerive>,
    pub saved: SaveReport,
}

/// Which entries a run derives: those with no calibre block yet, and those
/// left `unmatched` (their cause, a missing book or EPUB, may have gone away).
/// `rederive` also refreshes `exact`, `approximate` and `ambiguous`.
fn wants_derive(e: &Entry, rederive: bool) -> bool {
    if e.deleted {
        return false;
    }
    match e
        .calibre
        .as_ref()
        .and_then(|c| c.get("match"))
        .and_then(Value::as_str)
    {
        None => e.calibre.is_none(),
        Some("unmatched") => true,
        Some("exact" | "approximate" | "ambiguous") => rederive,
        Some(_) => false,
    }
}

pub fn derive_calibre(
    library: &Path,
    archive_dir: &Path,
    config: &DeriveConfig,
    rederive: bool,
    only_files: &[String],
    dry_run: bool,
) -> Result<DeriveReport, String> {
    let lib = Library::open(library)?;
    let calibre_books = lib.books()?;
    let overrides = Overrides::load(archive_dir)?;
    let mut archive = Archive::load(archive_dir)?;
    let claimed: BTreeMap<String, String> = archive
        .books
        .iter()
        .filter_map(|b| {
            b.data
                .book
                .calibre_uuid
                .clone()
                .map(|u| (u, b.file_name.clone()))
        })
        .collect();

    let mut reports = Vec::new();
    for loaded in &mut archive.books {
        let wanted = only_files.is_empty()
            || only_files
                .iter()
                .any(|f| f.trim_end_matches(".json") == loaded.file_name.trim_end_matches(".json"));
        if !wanted
            || !loaded
                .data
                .entries
                .iter()
                .any(|e| wants_derive(e, rederive))
        {
            continue;
        }
        let book = &mut loaded.data;
        let total_locations = overrides
            .for_book(&book.book)
            .and_then(|o| o.total_locations);

        let (outcome, block_base, text) =
            match match_book(&book.book, &overrides, &calibre_books, config.book_match) {
                BookMatch::Gone { calibre_uuid } => (
                    BookOutcome::Gone { calibre_uuid },
                    Err("no-calibre-book"),
                    None,
                ),
                BookMatch::NoCalibreBook { candidates } => (
                    BookOutcome::NoCalibreBook { candidates },
                    Err("no-calibre-book"),
                    None,
                ),
                BookMatch::Matched { calibre_id, how } => {
                    let c = &calibre_books[&calibre_id];
                    let uuid = c.uuid.clone().unwrap_or_default();
                    let calibre_title = c.title.clone();
                    match claimed.get(&uuid) {
                        Some(other) if *other != loaded.file_name => {
                            reports.push(BookDerive {
                                file_name: loaded.file_name.clone(),
                                title: book.book.title.clone(),
                                kindle_title: book.book.kindle_titles.first().cloned(),
                                outcome: BookOutcome::ClaimedByOtherFile {
                                    calibre_title,
                                    other_file: other.clone(),
                                },
                                counts: BTreeMap::new(),
                            });
                            continue;
                        }
                        _ => {}
                    }
                    if book.book.calibre_uuid.is_none() {
                        book.book.calibre_uuid = Some(uuid);
                    }
                    match c.formats.get(WRITABLE_FORMAT) {
                        None => (BookOutcome::NoEpub { calibre_title }, Err("no-epub"), None),
                        Some(rel) => match BookText::open(&lib.root.join(rel)) {
                            Ok(t) => (
                                BookOutcome::Matched { calibre_title, how },
                                Ok(calibre_id),
                                Some(t),
                            ),
                            Err(error) => (
                                BookOutcome::UnreadableEpub { error },
                                Err("unreadable-epub"),
                                None,
                            ),
                        },
                    }
                }
            };

        let mut counts: BTreeMap<String, usize> = BTreeMap::new();
        let targets: Vec<usize> = (0..book.entries.len())
            .filter(|&i| wants_derive(&book.entries[i], rederive))
            .collect();
        let blocks: Vec<(usize, Map<String, Value>)> = match (&block_base, &text) {
            (Ok(book_id), Some(text)) => derive_book(
                &book.entries,
                &targets,
                text,
                *book_id,
                total_locations,
                config,
            ),
            (Err(reason), _) => targets.iter().map(|&i| (i, unmatched(reason))).collect(),
            _ => unreachable!("a matched book always has its text"),
        };
        for (i, block) in blocks {
            let m = block["match"].as_str().unwrap_or("").to_string();
            let key = match block.get("reason").and_then(Value::as_str) {
                Some(r) => format!("{m}: {r}"),
                None => m,
            };
            *counts.entry(key).or_default() += 1;
            book.entries[i].calibre = Some(Value::Object(block));
        }
        reports.push(BookDerive {
            file_name: loaded.file_name.clone(),
            title: book.book.title.clone(),
            kindle_title: book.book.kindle_titles.first().cloned(),
            outcome,
            counts,
        });
    }

    let saved = archive.save(dry_run)?;
    Ok(DeriveReport {
        books: reports,
        saved,
    })
}

fn unmatched(reason: &str) -> Map<String, Value> {
    let mut m = Map::new();
    m.insert("match".into(), json!("unmatched"));
    m.insert("reason".into(), json!(reason));
    m.insert("derived_with".into(), json!(derived_with()));
    m
}

/// Kindle position of an entry: location if it has one, else page, tagged
/// so the two scales are never mixed.
fn position(e: &Entry) -> Option<(bool, f64)> {
    let k = e.kindle.as_ref()?;
    k.location
        .map(|[s, _]| (true, f64::from(s)))
        .or(k.page.map(|[s, _]| (false, f64::from(s))))
}

fn derive_book(
    entries: &[Entry],
    targets: &[usize],
    text: &BookText,
    book_id: i64,
    total_locations: Option<u32>,
    config: &DeriveConfig,
) -> Vec<(usize, Map<String, Value>)> {
    let locate = |e: &Entry| match (e.kind, &e.text) {
        (EntryType::Highlight, Some(t)) => Some(text.locate(t, config.approximate_threshold)),
        _ => None,
    };

    // Pass 1: every highlight of the book found exactly once, whether or not
    // it is being derived now, pins a Kindle position to an offset in the
    // book's text, so a result never depends on what else happened to be
    // pending.
    let all: Vec<Option<Located>> = entries
        .iter()
        .enumerate()
        .map(|(i, e)| {
            (targets.contains(&i) || (e.kindle.is_some() && !e.deleted))
                .then(|| locate(e))
                .flatten()
        })
        .collect();
    let mut pins: Vec<(bool, f64, f64)> = Vec::new();
    for (e, l) in entries.iter().zip(&all) {
        if let (Some(Located::One(hit)), Some((is_loc, pos)), false) = (l, position(e), e.deleted) {
            pins.push((is_loc, pos, text.global_offset(*hit) as f64));
        }
    }
    let located: Vec<(usize, Option<Located>)> =
        targets.iter().map(|&i| (i, all[i].clone())).collect();
    let max_position = |is_loc: bool| {
        entries
            .iter()
            .filter_map(position)
            .filter(|(l, _)| *l == is_loc)
            .map(|(_, p)| p)
            .fold(0.0, f64::max)
    };
    let predict = |is_loc: bool, pos: f64| -> f64 {
        let points: Vec<(f64, f64)> = pins
            .iter()
            .filter(|p| p.0 == is_loc)
            .map(|p| (p.1, p.2))
            .collect();
        if let Some((a, b)) = fit(&points) {
            return a + b * pos;
        }
        let total = match (is_loc, total_locations) {
            (true, Some(t)) => f64::from(t),
            _ => max_position(is_loc),
        };
        if total > 0.0 {
            pos / total * text.total_len() as f64
        } else {
            0.0
        }
    };

    located
        .into_iter()
        .map(|(i, l)| {
            let block = match l {
                None => unmatched("orphan-note"),
                Some(Located::One(hit)) => anchored(text, hit, book_id, "exact", config),
                Some(Located::Approximate(hit, similarity)) => {
                    let mut b = anchored(text, hit, book_id, "approximate", config);
                    b.insert("similarity".into(), json!((similarity * 1000.0).round() / 1000.0));
                    b
                }
                Some(Located::Many(hits)) => {
                    let mut by_distance: Vec<(f64, Hit)> = match position(&entries[i]) {
                        Some((is_loc, pos)) => {
                            let p = predict(is_loc, pos);
                            hits.iter().map(|h| ((text.global_offset(*h) as f64 - p).abs(), *h)).collect()
                        }
                        None => hits.iter().map(|h| (f64::INFINITY, *h)).collect(),
                    };
                    by_distance.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.doc.cmp(&b.1.doc)).then(a.1.range.cmp(&b.1.range)));
                    let clear = by_distance[0].0.is_finite() && by_distance[0].0 <= config.disambiguation_ratio * by_distance[1].0;
                    if clear {
                        anchored(text, by_distance[0].1, book_id, "exact", config)
                    } else {
                        let mut b = anchored(text, by_distance[0].1, book_id, "ambiguous", config);
                        let candidates: Vec<Value> = by_distance[1..]
                            .iter()
                            .take(MAX_CANDIDATES)
                            .filter_map(|(_, h)| {
                                let (s, e) = text.cfis(*h)?;
                                let d = &text.docs[h.doc].spine;
                                Some(json!({"start_cfi": s, "end_cfi": e, "spine_name": d.name, "spine_index": d.index}))
                            })
                            .collect();
                        b.insert("candidates".into(), Value::Array(candidates));
                        b.insert("occurrences".into(), json!(hits.len()));
                        b
                    }
                }
                Some(Located::NotFound { cross_spine, unreadable_docs }) => unmatched(if cross_spine {
                    "cross-spine"
                } else if unreadable_docs {
                    "parse-error"
                } else {
                    "text-not-found"
                }),
            };
            (i, block)
        })
        .collect()
}

/// Least-squares line through (position, offset) points; needs three points
/// and some spread in position.
fn fit(points: &[(f64, f64)]) -> Option<(f64, f64)> {
    if points.len() < 3 {
        return None;
    }
    let n = points.len() as f64;
    let (mx, my) = (
        points.iter().map(|p| p.0).sum::<f64>() / n,
        points.iter().map(|p| p.1).sum::<f64>() / n,
    );
    let sxx: f64 = points.iter().map(|p| (p.0 - mx).powi(2)).sum();
    if sxx == 0.0 {
        return None;
    }
    let sxy: f64 = points.iter().map(|p| (p.0 - mx) * (p.1 - my)).sum();
    let b = sxy / sxx;
    Some((my - b * mx, b))
}

fn anchored(
    text: &BookText,
    hit: Hit,
    book_id: i64,
    matched: &str,
    config: &DeriveConfig,
) -> Map<String, Value> {
    let Some((start, end)) = text.cfis(hit) else {
        return unmatched("text-not-found");
    };
    let spine = &text.docs[hit.doc].spine;
    let mut m = Map::new();
    m.insert("match".into(), json!(matched));
    m.insert("format".into(), json!(WRITABLE_FORMAT));
    m.insert("book_id".into(), json!(book_id));
    m.insert("spine_name".into(), json!(spine.name));
    m.insert("spine_index".into(), json!(spine.index));
    m.insert("start_cfi".into(), json!(start));
    m.insert("end_cfi".into(), json!(end));
    m.insert("style".into(), config.style.clone());
    m.insert("derived_with".into(), json!(derived_with()));
    m
}
