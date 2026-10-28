use std::fs;
use std::path::Path;

use florilegium::archive::Archive;
use florilegium::check::check;
use florilegium::config::Config;
use florilegium::ingest::{ingest_calibre, ingest_clippings};
use rusqlite::Connection;
use serde_json::{Value, json};

const BOOK_UUID: &str = "11111111-2222-4333-8444-555555555555";

/// A synthetic calibre library holding the columns the tool reads.
fn library(annotations: &[(i64, &str, &str, &str, Value)]) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let db = Connection::open(dir.path().join("metadata.db")).unwrap();
    db.execute_batch(
        "CREATE TABLE books (id INTEGER PRIMARY KEY, title TEXT, path TEXT, uuid TEXT);
         CREATE TABLE authors (id INTEGER PRIMARY KEY, name TEXT);
         CREATE TABLE books_authors_link (id INTEGER PRIMARY KEY, book INTEGER, author INTEGER);
         CREATE TABLE data (id INTEGER PRIMARY KEY, book INTEGER, format TEXT, name TEXT);
         CREATE TABLE annotations (id INTEGER PRIMARY KEY, book INTEGER, format TEXT, user_type TEXT, user TEXT,
             timestamp REAL, annot_id TEXT, annot_type TEXT, annot_data TEXT, searchable_text TEXT);
         INSERT INTO books VALUES (7, 'The Orchard Ledger', 'Mara Penn/The Orchard Ledger (7)', '11111111-2222-4333-8444-555555555555');
         INSERT INTO books VALUES (8, 'Field Pamphlet', 'Abel Stone/Field Pamphlet (8)', NULL);
         INSERT INTO authors VALUES (1, 'Mara Penn'), (2, 'Ivo Penn');
         INSERT INTO books_authors_link VALUES (1, 7, 1), (2, 7, 2);
         INSERT INTO data VALUES (1, 7, 'EPUB', 'The Orchard Ledger - Mara Penn');",
    )
    .unwrap();
    for (book, format, user_type, user, data) in annotations {
        db.execute(
            "INSERT INTO annotations (book, format, user_type, user, timestamp, annot_id, annot_type, annot_data, searchable_text)
             VALUES (?1, ?2, ?3, ?4, 0, ?5, 'highlight', ?6, '')",
            rusqlite::params![book, format, user_type, user, data["uuid"].as_str().unwrap(), data.to_string()],
        )
        .unwrap();
    }
    dir
}

fn highlight(uuid: &str, text: &str, note: Option<&str>) -> Value {
    let mut v = json!({
        "type": "highlight", "uuid": uuid, "timestamp": "2026-04-12T03:43:39.157Z",
        "start_cfi": "/2/4/2/12/1:0", "end_cfi": "/2/4/2/12/1:40",
        "highlighted_text": text, "spine_name": "text/ch1.xhtml", "spine_index": 2,
        "style": {"kind": "color", "type": "builtin", "which": "yellow"},
        "toc_family_titles": ["Chapter One"],
    });
    if let Some(n) = note {
        v["notes"] = json!(n);
    }
    v
}

const A: &str = "Orchard0Highlight00001";
const B: &str = "Orchard0Highlight00002";

fn standard() -> tempfile::TempDir {
    library(&[
        (
            7,
            "EPUB",
            "local",
            "viewer",
            highlight(
                A,
                "Bees do not read the calendar; they read the light.",
                Some("Seasons."),
            ),
        ),
        (
            7,
            "EPUB",
            "local",
            "viewer",
            highlight(
                B,
                "A windbreak is a promise planted for grandchildren.",
                None,
            ),
        ),
        (
            7,
            "EPUB",
            "local",
            "viewer",
            json!({"removed": true, "type": "highlight", "uuid": "Orchard0Removed0000003", "timestamp": "2021-01-05T03:53:06.374Z"}),
        ),
        (
            7,
            "AZW3",
            "local",
            "viewer",
            highlight("Xyz0123456789abcdefABC", "On another format.", None),
        ),
        (
            7,
            "EPUB",
            "web",
            "*",
            highlight("Web0123456789abcdefABC", "From the content server.", None),
        ),
        (
            8,
            "EPUB",
            "local",
            "viewer",
            highlight("NoUuid3456789abcdefABC", "A book without a UUID.", None),
        ),
    ])
}

fn snapshot(dir: &Path) -> Vec<(String, Vec<u8>)> {
    let mut v: Vec<(String, Vec<u8>)> = fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap())
        .map(|e| {
            (
                e.file_name().to_string_lossy().into_owned(),
                fs::read(e.path()).unwrap(),
            )
        })
        .collect();
    v.sort();
    v
}

#[test]
fn viewer_highlights_become_native_entries() {
    let lib = standard();
    let archive = tempfile::tempdir().unwrap();
    let r = ingest_calibre(lib.path(), archive.path(), &[], false).unwrap();
    assert_eq!(r.removed, 1);
    assert_eq!(r.formats.get("EPUB"), Some(&2));
    assert_eq!(r.formats.get("AZW3"), Some(&1));
    assert!(
        r.warnings.iter().any(|w| w.contains("has no UUID")),
        "{:?}",
        r.warnings
    );

    let a = Archive::load(archive.path()).unwrap();
    assert_eq!(a.books.len(), 1);
    let book = &a.books[0].data;
    assert_eq!(book.book.title, "The Orchard Ledger");
    assert_eq!(book.book.authors, vec!["Mara Penn", "Ivo Penn"]);
    assert_eq!(book.book.calibre_uuid.as_deref(), Some(BOOK_UUID));
    assert!(book.book.kindle_titles.is_empty());
    assert_eq!(
        book.entries.len(),
        3,
        "every format is read; removed and web annotations are not"
    );
    let azw3 = book
        .entries
        .iter()
        .find(|e| e.text.as_deref() == Some("On another format."))
        .unwrap();
    assert_eq!(azw3.calibre.keys().collect::<Vec<_>>(), vec!["AZW3"]);

    let e = book.entries.iter().find(|e| e.id == A).unwrap();
    assert_eq!(
        e.text.as_deref(),
        Some("Bees do not read the calendar; they read the light.")
    );
    assert_eq!(e.note.as_deref(), Some("Seasons."));
    assert_eq!(
        e.created.as_deref(),
        Some("2026-04-12T03:43:39.157Z"),
        "calibre's timestamp verbatim"
    );
    assert_eq!(e.sources, vec!["calibre"]);
    assert!(e.kindle.is_none());
    assert_eq!(e.calibre.keys().collect::<Vec<_>>(), vec!["EPUB"]);
    let c = &e.calibre["EPUB"];
    assert_eq!(c["match"], "native");
    assert_eq!(c["book_id"], 7);
    assert_eq!(c["start_cfi"], "/2/4/2/12/1:0");
    assert_eq!(c["highlighted_text"], e.text.clone().unwrap().as_str());
    assert_eq!(check(archive.path()), vec![]);
}

#[test]
fn a_second_run_changes_zero_bytes_and_the_library_is_untouched() {
    let lib = standard();
    let archive = tempfile::tempdir().unwrap();
    let db_before = fs::read(lib.path().join("metadata.db")).unwrap();
    ingest_calibre(lib.path(), archive.path(), &[], false).unwrap();
    let before = snapshot(archive.path());
    let r = ingest_calibre(lib.path(), archive.path(), &[], false).unwrap();
    assert_eq!((r.saved.created.len(), r.saved.updated.len()), (0, 0));
    assert_eq!(snapshot(archive.path()), before);
    assert_eq!(fs::read(lib.path().join("metadata.db")).unwrap(), db_before);
}

#[test]
fn book_filter_limits_what_is_read() {
    let lib = standard();
    let archive = tempfile::tempdir().unwrap();
    let r = ingest_calibre(lib.path(), archive.path(), &[8], false).unwrap();
    assert!(r.books.is_empty());
    assert!(snapshot(archive.path()).is_empty());
}

/// A Kindle highlight, matched to this calibre book and pushed (simulated by
/// writing its ID and our CFIs into calibre), read back.
fn pushed_kindle_entry(calibre_cfis_match: bool) -> (tempfile::TempDir, String) {
    let archive = tempfile::tempdir().unwrap();
    let text = "Pruning is a conversation with next year, conducted in absence.";
    let clippings = format!(
        "The Orchard Ledger (Penn, Mara)\n- Your Highlight at location 30-31 | Added on Sunday, 4 January 2026 10:00:00\n\n{text}\n==========\n"
    );
    ingest_clippings(&clippings, archive.path(), &Config::default(), false).unwrap();
    let mut a = Archive::load(archive.path()).unwrap();
    a.books[0].data.book.calibre_uuid = Some(BOOK_UUID.into());
    let e = &mut a.books[0].data.entries[0];
    e.calibre.insert(
        "EPUB".into(),
        json!({
            "match": "exact", "derived_with": "florilegium 0.1.0",
            "start_cfi": "/2/4/2/12/1:0", "end_cfi": "/2/4/2/12/1:40", "spine_name": "text/ch1.xhtml",
            "spine_index": 2, "book_id": 7, "style": {"kind": "color", "type": "builtin", "which": "yellow"},
        }),
    );
    let id = e.id.clone();
    let created = e.created.clone().unwrap();
    a.save(false).unwrap();

    let mut pushed = highlight(&id, text, None);
    pushed["timestamp"] = json!(created);
    if !calibre_cfis_match {
        pushed["start_cfi"] = json!("/2/4/2/14/1:0");
    }
    let lib = library(&[(7, "EPUB", "local", "viewer", pushed)]);
    ingest_calibre(lib.path(), archive.path(), &[], false).unwrap();
    (archive, id)
}

#[test]
fn a_pushed_kindle_highlight_read_back_is_the_same_entry() {
    let (archive, id) = pushed_kindle_entry(true);
    let a = Archive::load(archive.path()).unwrap();
    assert_eq!(a.books.len(), 1, "found by calibre_uuid, no new file");
    let entries = &a.books[0].data.entries;
    assert_eq!(entries.len(), 1, "no duplicate");
    let e = &entries[0];
    assert_eq!(e.id, id);
    assert_eq!(e.sources, vec!["calibre", "kindle"]);
    assert!(e.kindle.is_some(), "kindle block untouched");
    let c = &e.calibre["EPUB"];
    assert_eq!(
        c["match"], "exact",
        "same CFIs: our own anchor, not relabelled native"
    );
    assert_eq!(c["derived_with"], "florilegium 0.1.0");
    assert!(
        c.get("highlighted_text").is_none() && c.get("uuid").is_none(),
        "our anchor is kept as it was, so a push and a read change nothing"
    );
}

#[test]
fn a_re_anchored_highlight_becomes_native() {
    let (archive, _) = pushed_kindle_entry(false);
    let a = Archive::load(archive.path()).unwrap();
    let c = &a.books[0].data.entries[0].calibre["EPUB"];
    assert_eq!(c["match"], "native");
    assert!(c.get("derived_with").is_none());
}

#[test]
fn check_reports_native_text_overwritten_by_kindle() {
    let lib = standard();
    let archive = tempfile::tempdir().unwrap();
    ingest_calibre(lib.path(), archive.path(), &[], false).unwrap();
    let mut a = Archive::load(archive.path()).unwrap();
    a.books[0]
        .data
        .entries
        .iter_mut()
        .find(|e| e.id == A)
        .unwrap()
        .text = Some("Kindle's different wording.".into());
    a.save(false).unwrap();
    let issues = check(archive.path());
    assert_eq!(issues.len(), 1, "{issues:#?}");
    assert_eq!(issues[0].id.as_deref(), Some(A));
    assert!(
        issues[0]
            .message
            .starts_with("text differs from the EPUB highlight in calibre")
    );
}

#[test]
fn a_folder_without_metadata_db_is_an_error() {
    let not_a_library = tempfile::tempdir().unwrap();
    let archive = tempfile::tempdir().unwrap();
    let err = ingest_calibre(not_a_library.path(), archive.path(), &[], false).unwrap_err();
    assert!(err.contains("no metadata.db"), "{err}");
}

#[test]
fn one_highlight_on_two_formats_is_one_entry_with_two_anchors() {
    // Read on AZW3 long ago, later written to the EPUB under the same ID with
    // a newer note.
    let mut on_epub = highlight(
        A,
        "Bees do not read the calendar; they read the light.",
        Some("Newer note."),
    );
    on_epub["timestamp"] = json!("2026-05-01T00:00:00.000Z");
    on_epub["start_cfi"] = json!("/2/4/2/40/1:0");
    let lib = library(&[
        (
            7,
            "AZW3",
            "local",
            "viewer",
            highlight(
                A,
                "Bees do not read the calendar; they read the light.",
                Some("Seasons."),
            ),
        ),
        (7, "EPUB", "local", "viewer", on_epub),
    ]);
    let archive = tempfile::tempdir().unwrap();
    let r = ingest_calibre(lib.path(), archive.path(), &[], false).unwrap();
    assert!(r.warnings.is_empty(), "{:?}", r.warnings);
    let a = Archive::load(archive.path()).unwrap();
    let entries = &a.books[0].data.entries;
    assert_eq!(entries.len(), 1);
    let e = &entries[0];
    assert_eq!(e.calibre.keys().collect::<Vec<_>>(), vec!["AZW3", "EPUB"]);
    assert_eq!(e.calibre["AZW3"]["start_cfi"], "/2/4/2/12/1:0");
    assert_eq!(e.calibre["EPUB"]["start_cfi"], "/2/4/2/40/1:0");
    assert_eq!(
        e.note.as_deref(),
        Some("Newer note."),
        "the most recently changed annotation supplies the content"
    );
    assert_eq!(check(archive.path()), vec![]);
}

#[test]
fn calibres_timestamp_never_overwrites_a_creation_date() {
    // A pushed highlight later bumped in calibre (edited, or re-pushed with a
    // newer timestamp): calibre's timestamp says when it last changed, not when
    // it was made.
    let archive = tempfile::tempdir().unwrap();
    let text = "Pruning is a conversation with next year, conducted in absence.";
    let clippings = format!(
        "The Orchard Ledger (Penn, Mara)\n- Your Highlight at location 30-31 | Added on Sunday, 4 January 2026 10:00:00\n\n{text}\n==========\n"
    );
    ingest_clippings(&clippings, archive.path(), &Config::default(), false).unwrap();
    let mut a = Archive::load(archive.path()).unwrap();
    a.books[0].data.book.calibre_uuid = Some(BOOK_UUID.into());
    let id = a.books[0].data.entries[0].id.clone();
    let created = a.books[0].data.entries[0].created.clone();
    a.save(false).unwrap();

    let mut later = highlight(&id, text, Some("Added in calibre."));
    later["timestamp"] = json!("2026-09-01T00:00:00.000Z");
    let lib = library(&[(7, "EPUB", "local", "viewer", later)]);
    ingest_calibre(lib.path(), archive.path(), &[], false).unwrap();
    let e = &Archive::load(archive.path()).unwrap().books[0].data.entries[0];
    assert_eq!(e.created, created);
    assert_eq!(
        e.note.as_deref(),
        Some("Added in calibre."),
        "content still comes from calibre"
    );
}
