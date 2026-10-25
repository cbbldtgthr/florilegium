use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use annot_archive::archive::{Archive, Entry};
use annot_archive::config::Config;
use annot_archive::ingest::{IngestReport, ingest_clippings};
use serde_json::{Value, json};

const BOOK: &str = "The Orchard Ledger (Penn, Mara)";

fn clip(book: &str, kind: &str, loc: &str, minute: u32, text: &str) -> String {
    format!(
        "{book}\n- Your {kind} at location {loc} | Added on Sunday, 4 January 2026 10:{minute:02}:00\n\n{text}\n==========\n"
    )
}

fn hl(loc: &str, minute: u32, text: &str) -> String {
    clip(BOOK, "Highlight", loc, minute, text)
}

fn base() -> String {
    [
        hl(
            "10-11",
            1,
            "The orchard keeps two ledgers, one in ink and one in the rings of its oldest trees",
        ),
        clip(BOOK, "Note", "11", 2, "Ledger metaphor."),
        hl(
            "30-31",
            3,
            "Pruning is a conversation with next year, conducted in absence.",
        ),
        clip(
            "Field Pamphlet (Stone, Abel; Reed, Ivy)",
            "Highlight",
            "5-6",
            4,
            "Mulch is the soil's own patience.",
        ),
    ]
    .concat()
}

fn ingest(input: &str, dir: &Path) -> IngestReport {
    ingest_clippings(input, dir, &Config::default(), false).unwrap()
}

fn snapshot(dir: &Path) -> Vec<(String, String)> {
    let mut v: Vec<(String, String)> = fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap())
        .map(|e| {
            (
                e.file_name().to_string_lossy().into_owned(),
                fs::read_to_string(e.path()).unwrap(),
            )
        })
        .collect();
    v.sort();
    v
}

fn entries(dir: &Path, title_starts: &str) -> Vec<Entry> {
    let a = Archive::load(dir).unwrap();
    a.books
        .into_iter()
        .find(|b| b.data.book.title.starts_with(title_starts))
        .unwrap()
        .data
        .entries
}

fn find<'a>(entries: &'a [Entry], text_start: &str) -> &'a Entry {
    entries
        .iter()
        .find(|e| e.text.as_deref().unwrap_or("").starts_with(text_start))
        .unwrap()
}

/// Set fields on the stored entry whose text starts with `text_start`.
fn edit_entry(dir: &Path, text_start: &str, f: impl Fn(&mut Entry)) {
    let mut a = Archive::load(dir).unwrap();
    for b in &mut a.books {
        for e in &mut b.data.entries {
            if e.text.as_deref().unwrap_or("").starts_with(text_start) {
                f(e);
            }
        }
    }
    a.save(false).unwrap();
}

#[test]
fn first_ingest_creates_one_file_per_book() {
    let dir = tempfile::tempdir().unwrap();
    let r = ingest(&base(), dir.path());
    assert_eq!(r.saved.created.len(), 2);
    let orchard = entries(dir.path(), "The Orchard Ledger");
    assert_eq!(orchard.len(), 2);
    let ledger = find(&orchard, "The orchard keeps");
    assert_eq!(ledger.note.as_deref(), Some("Ledger metaphor."));
    assert_eq!(ledger.sources, vec!["kindle"]);
    assert_eq!(ledger.created.as_deref(), Some("2026-01-04T08:01:00.000Z"));

    let a = Archive::load(dir.path()).unwrap();
    let pamphlet = a
        .books
        .iter()
        .find(|b| b.data.book.title == "Field Pamphlet")
        .unwrap();
    assert_eq!(pamphlet.data.book.authors, vec!["Stone, Abel", "Reed, Ivy"]);
    assert_eq!(
        pamphlet.data.book.kindle_titles,
        vec!["Field Pamphlet (Stone, Abel; Reed, Ivy)"]
    );
}

#[test]
fn a_second_run_changes_zero_bytes() {
    let dir = tempfile::tempdir().unwrap();
    ingest(&base(), dir.path());
    let before = snapshot(dir.path());
    let r = ingest(&base(), dir.path());
    assert_eq!(
        (
            r.saved.created.len(),
            r.saved.updated.len(),
            r.saved.unchanged
        ),
        (0, 0, 2)
    );
    assert!(r.books.iter().all(|b| !b.changed()));
    assert_eq!(snapshot(dir.path()), before);
}

#[test]
fn appending_changes_only_what_it_touches() {
    let dir = tempfile::tempdir().unwrap();
    ingest(&base(), dir.path());
    let before = snapshot(dir.path());
    let more = base()
        + &hl(
            "50-51",
            9,
            "Bees do not read the calendar; they read the light.",
        );
    let r = ingest(&more, dir.path());
    assert_eq!(r.saved.updated.len(), 1);
    assert_eq!(r.saved.unchanged, 1);
    let orchard = r
        .books
        .iter()
        .find(|b| b.title == "The Orchard Ledger")
        .unwrap();
    assert_eq!(
        (orchard.created, orchard.updated, orchard.unchanged),
        (1, 0, 2)
    );
    // the other book's file is byte-identical
    let after = snapshot(dir.path());
    let pamphlet = |s: &[(String, String)]| {
        s.iter()
            .find(|(n, _)| n.starts_with("field-pamphlet"))
            .unwrap()
            .1
            .clone()
    };
    assert_eq!(pamphlet(&after), pamphlet(&before));
}

#[test]
fn an_edited_highlight_keeps_its_original_id() {
    let dir = tempfile::tempdir().unwrap();
    ingest(&base(), dir.path());
    let original = find(&entries(dir.path(), "The Orchard"), "The orchard keeps")
        .id
        .clone();

    let extended = base()
        + &hl(
            "10-12",
            9,
            "The orchard keeps two ledgers, one in ink and one in the rings of its oldest trees, and only the second never lies.",
        );
    let r = ingest(&extended, dir.path());
    let e = entries(dir.path(), "The Orchard");
    let ledger = find(&e, "The orchard keeps");
    assert_eq!(ledger.id, original);
    assert_eq!(ledger.aliases.len(), 1);
    assert!(ledger.text.as_deref().unwrap().ends_with("never lies."));
    assert_eq!(e.len(), 2);
    assert_eq!(r.books.iter().map(|b| b.updated).sum::<usize>(), 1);
}

#[test]
fn trimmed_clippings_file_keeps_ids_through_aliases() {
    let dir = tempfile::tempdir().unwrap();
    let extended = base()
        + &hl(
            "10-12",
            9,
            "The orchard keeps two ledgers, one in ink and one in the rings of its oldest trees, and more.",
        );
    ingest(&extended, dir.path());
    let id = find(&entries(dir.path(), "The Orchard"), "The orchard keeps")
        .id
        .clone();

    // Kindle drops the oldest entries: the earliest member is gone, so the
    // replay's survivor is now the extension, already an alias in the archive.
    let trimmed: String = extended.split_inclusive("==========\n").skip(1).collect();
    ingest(&trimmed, dir.path());
    let e = entries(dir.path(), "The Orchard");
    let ledger = find(&e, "The orchard keeps");
    assert_eq!(ledger.id, id);
    assert_eq!(
        e.iter()
            .filter(|x| x
                .text
                .as_deref()
                .unwrap_or("")
                .starts_with("The orchard keeps"))
            .count(),
        1
    );
}

#[test]
fn tombstones_absorb_matches_without_changing() {
    let dir = tempfile::tempdir().unwrap();
    ingest(&base(), dir.path());
    edit_entry(dir.path(), "Pruning is", |e| e.deleted = true);
    let before = find(&entries(dir.path(), "The Orchard"), "Pruning is").clone();

    let extended = base()
        + &hl(
            "30-32",
            9,
            "Pruning is a conversation with next year, conducted in absence and patience.",
        );
    let r = ingest(&extended, dir.path());
    let e = entries(dir.path(), "The Orchard");
    let pruning: Vec<&Entry> = e
        .iter()
        .filter(|x| x.text.as_deref().unwrap_or("").starts_with("Pruning"))
        .collect();
    assert_eq!(pruning.len(), 1, "no live entry is resurrected");
    assert!(pruning[0].deleted);
    assert_eq!(pruning[0].text, before.text, "text untouched");
    assert_eq!(pruning[0].aliases.len(), 1, "identity still grows");
    assert_eq!(
        r.books.iter().map(|b| b.tombstone_matched).sum::<usize>(),
        1
    );
}

#[test]
fn a_wide_highlight_merges_archive_entries() {
    let dir = tempfile::tempdir().unwrap();
    let two = [
        hl(
            "90-91",
            1,
            "Frost settles first in the low corners of the field,",
        ),
        hl("92-93", 2, "where the cold air pools like water in a bowl."),
    ]
    .concat();
    ingest(&two, dir.path());
    let ids: Vec<String> = entries(dir.path(), "The Orchard")
        .iter()
        .map(|e| e.id.clone())
        .collect();
    assert_eq!(ids.len(), 2);
    let first = find(&entries(dir.path(), "The Orchard"), "Frost")
        .id
        .clone();
    let second = find(&entries(dir.path(), "The Orchard"), "where the cold")
        .id
        .clone();

    let wide = two
        + &hl(
            "90-93",
            3,
            "Frost settles first in the low corners of the field, where the cold air pools like water in a bowl.",
        );
    let r = ingest(&wide, dir.path());
    let e = entries(dir.path(), "The Orchard");
    assert_eq!(e.len(), 1);
    assert_eq!(e[0].id, first, "survivor is the earliest");
    assert!(e[0].aliases.contains(&second));
    assert_eq!(e[0].aliases.len(), 2);
    let merges: Vec<_> = r.books.iter().flat_map(|b| &b.merges).collect();
    assert_eq!(merges, vec![&(first, vec![second])]);
}

fn anchors(pairs: &[(&str, Value)]) -> BTreeMap<String, Value> {
    pairs
        .iter()
        .map(|(f, v)| (f.to_string(), v.clone()))
        .collect()
}

#[test]
fn text_changes_drop_derived_calibre_anchors_only() {
    let dir = tempfile::tempdir().unwrap();
    ingest(&base(), dir.path());
    edit_entry(dir.path(), "The orchard keeps", |e| {
        e.calibre = anchors(&[("EPUB", json!({"match": "exact", "start_cfi": "/2/4"}))])
    });
    edit_entry(dir.path(), "Pruning is", |e| {
        e.calibre = anchors(&[
            ("AZW3", json!({"match": "native", "start_cfi": "/2/6"})),
            ("EPUB", json!({"match": "approximate", "start_cfi": "/2/8"})),
        ])
    });

    let edited = base()
        + &hl(
            "10-12",
            9,
            "The orchard keeps two ledgers, one in ink and one in the rings of its oldest trees, and more.",
        )
        + &hl(
            "30-32",
            10,
            "Pruning is a conversation with next year, conducted in absence and silence.",
        );
    ingest(&edited, dir.path());
    let e = entries(dir.path(), "The Orchard");
    assert!(find(&e, "The orchard keeps").calibre.is_empty());
    assert_eq!(
        find(&e, "Pruning is").calibre,
        anchors(&[("AZW3", json!({"match": "native", "start_cfi": "/2/6"}))]),
        "the native anchor stays, the derived one goes"
    );
}

#[test]
fn unchanged_text_keeps_derived_calibre_anchors() {
    let dir = tempfile::tempdir().unwrap();
    ingest(&base(), dir.path());
    edit_entry(dir.path(), "The orchard keeps", |e| {
        e.calibre = anchors(&[("EPUB", json!({"match": "exact"}))])
    });
    ingest(&base(), dir.path());
    assert_eq!(
        find(&entries(dir.path(), "The Orchard"), "The orchard keeps").calibre,
        anchors(&[("EPUB", json!({"match": "exact"}))])
    );
}

#[test]
fn a_drifted_title_line_joins_the_same_book() {
    let dir = tempfile::tempdir().unwrap();
    ingest(&base(), dir.path());
    let drifted = base()
        + &clip(
            "THE ORCHARD  LEDGER (Penn, Mara)",
            "Highlight",
            "70",
            9,
            "A windbreak is a promise.",
        );
    let r = ingest(&drifted, dir.path());
    assert_eq!(r.saved.created.len(), 0);
    let a = Archive::load(dir.path()).unwrap();
    let orchard = a
        .books
        .iter()
        .find(|b| b.data.book.title == "The Orchard Ledger")
        .unwrap();
    assert_eq!(
        orchard.data.book.kindle_titles,
        vec!["THE ORCHARD  LEDGER (Penn, Mara)", BOOK]
    );
    assert_eq!(orchard.data.entries.len(), 3);
}

#[test]
fn hand_added_fields_survive_ingest() {
    let dir = tempfile::tempdir().unwrap();
    ingest(&base(), dir.path());
    edit_entry(dir.path(), "The orchard keeps", |e| {
        e.other.insert("locked_fields".into(), json!(["note"]));
    });
    let before = snapshot(dir.path());
    ingest(&base(), dir.path());
    assert_eq!(snapshot(dir.path()), before);
}

#[test]
fn dry_run_writes_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let r = ingest_clippings(&base(), dir.path(), &Config::default(), true).unwrap();
    assert_eq!(r.saved.created.len(), 2);
    assert!(snapshot(dir.path()).is_empty());
}

#[test]
fn a_missing_archive_directory_is_an_error() {
    let dir = tempfile::tempdir().unwrap();
    let err =
        ingest_clippings(&base(), &dir.path().join("nope"), &Config::default(), false).unwrap_err();
    assert!(err.contains("does not exist"), "{err}");
}

#[test]
fn config_in_the_archive_is_used_and_typos_are_errors() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(
        dir.path().join("config.toml"),
        "[kindle]\ntimezone = \"UTC\"\n",
    )
    .unwrap();
    let config = Config::load(None, dir.path()).unwrap();
    ingest_clippings(&base(), dir.path(), &config, false).unwrap();
    assert_eq!(
        find(&entries(dir.path(), "The Orchard"), "The orchard keeps")
            .created
            .as_deref(),
        Some("2026-01-04T10:01:00.000Z")
    );

    fs::write(
        dir.path().join("config.toml"),
        "[kindle]\ntimezon = \"UTC\"\n",
    )
    .unwrap();
    let err = Config::load(None, dir.path()).unwrap_err();
    assert!(err.contains("timezon"), "{err}");
}
