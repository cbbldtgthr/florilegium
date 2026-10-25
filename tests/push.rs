use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use annot_archive::archive::{Archive, Book, Entry, EntryType};
use annot_archive::ingest::ingest_calibre;
use annot_archive::push::{PushOptions, calibre_debug, plan, push};
use rusqlite::Connection;
use serde_json::{Value, json};

const BOOK_UUID: &str = "11111111-0000-4000-8000-000000000007";

/// A synthetic calibre library with one EPUB book and these existing viewer
/// annotations on it.
fn library(existing: &[Value]) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let db = Connection::open(dir.path().join("metadata.db")).unwrap();
    db.execute_batch(&format!(
        "CREATE TABLE books (id INTEGER PRIMARY KEY, title TEXT, path TEXT, uuid TEXT);
         CREATE TABLE authors (id INTEGER PRIMARY KEY, name TEXT);
         CREATE TABLE books_authors_link (id INTEGER PRIMARY KEY, book INTEGER, author INTEGER);
         CREATE TABLE data (id INTEGER PRIMARY KEY, book INTEGER, format TEXT, name TEXT);
         CREATE TABLE annotations (id INTEGER PRIMARY KEY, book INTEGER NOT NULL, format TEXT NOT NULL, user_type TEXT NOT NULL,
             user TEXT NOT NULL, timestamp REAL NOT NULL, annot_id TEXT NOT NULL, annot_type TEXT NOT NULL,
             annot_data TEXT NOT NULL, searchable_text TEXT NOT NULL DEFAULT '',
             UNIQUE(book, user_type, user, format, annot_type, annot_id));
         CREATE TABLE annotations_dirtied (id INTEGER PRIMARY KEY, book INTEGER NOT NULL, UNIQUE(book));
         INSERT INTO books VALUES (7, 'The Orchard Ledger', 'orchard', '{BOOK_UUID}');
         INSERT INTO data VALUES (1, 7, 'EPUB', 'book');"
    ))
    .unwrap();
    for a in existing {
        db.execute(
            "INSERT INTO annotations (book, format, user_type, user, timestamp, annot_id, annot_type, annot_data)
             VALUES (7, 'EPUB', 'local', 'viewer', 1.5, ?1, ?2, ?3)",
            rusqlite::params![a["uuid"].as_str().or(a["title"].as_str()).unwrap(), a["type"].as_str().unwrap(), a.to_string()],
        )
        .unwrap();
    }
    dir
}

fn entry(id: &str, text: &str, anchor: Option<Value>) -> Entry {
    Entry {
        id: id.into(),
        aliases: Vec::new(),
        kind: EntryType::Highlight,
        text: Some(text.into()),
        note: None,
        created: Some("2026-01-04T08:00:00.000Z".into()),
        deleted: false,
        sources: vec!["kindle".into()],
        kindle: None,
        calibre: anchor
            .map(|a| BTreeMap::from([("EPUB".to_string(), a)]))
            .unwrap_or_default(),
        other: BTreeMap::new(),
    }
}

fn derived(m: &str, start: &str) -> Value {
    json!({
        "match": m, "derived_with": "annot-archive 0.1.0", "book_id": 7,
        "spine_name": "OEBPS/ch0.xhtml", "spine_index": 0, "start_cfi": start, "end_cfi": "/2/4/2/1:30",
        "style": {"kind": "color", "type": "builtin", "which": "yellow"},
    })
}

fn archive(entries: Vec<Entry>) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let mut a = Archive::load(dir.path()).unwrap();
    let b = a.add_book(
        Book {
            title: "The Orchard Ledger".into(),
            authors: vec!["Mara Penn".into()],
            kindle_titles: vec!["The Orchard Ledger (Penn, Mara)".into()],
            calibre_uuid: Some(BOOK_UUID.into()),
            other: BTreeMap::new(),
        },
        "key",
    );
    b.data.entries = entries;
    a.save(false).unwrap();
    dir
}

const MINE: &str = "Mine0Highlight00000001";

fn mine() -> Entry {
    entry(
        MINE,
        "The orchard keeps two ledgers.",
        Some(derived("exact", "/2/4/2/1:0")),
    )
}

#[test]
fn an_exact_highlight_becomes_a_calibre_annotation() {
    let lib = library(&[]);
    let arch = archive(vec![mine()]);
    let p = plan(
        lib.path(),
        &Archive::load(arch.path()).unwrap(),
        &PushOptions::default(),
    )
    .unwrap();
    assert_eq!(p.books.len(), 1);
    let b = &p.books[0];
    assert_eq!((b.added, b.updated, b.kept), (1, 0, 0));
    let a = &b.annotations[0].annotation;
    assert_eq!(
        *a,
        json!({
            "type": "highlight", "uuid": MINE, "timestamp": "2026-01-04T08:00:00.000Z",
            "highlighted_text": "The orchard keeps two ledgers.",
            "start_cfi": "/2/4/2/1:0", "end_cfi": "/2/4/2/1:30", "spine_name": "OEBPS/ch0.xhtml", "spine_index": 0,
            "style": {"kind": "color", "type": "builtin", "which": "yellow"},
        }),
        "the tool's own keys are dropped"
    );
    assert_eq!(b.annotations[0].timestamp, 1767513600.0);
}

#[test]
fn only_confident_anchors_are_pushed_by_default() {
    let lib = library(&[]);
    let mut orphan = entry("Orphan0Note00000000001", "x", None);
    orphan.kind = EntryType::Note;
    let mut undated = entry(
        "Undated0Highlight00001",
        "u",
        Some(derived("exact", "/2/4/6/1:0")),
    );
    undated.created = None;
    let arch = archive(vec![
        mine(),
        entry(
            "Approx0Highlight000001",
            "a",
            Some(derived("approximate", "/2/4/8/1:0")),
        ),
        entry(
            "Ambig0Highlight0000001",
            "b",
            Some(derived("ambiguous", "/2/4/10/1:0")),
        ),
        entry(
            "Unmatched0Highlight001",
            "c",
            Some(json!({"match": "unmatched", "reason": "text-not-found"})),
        ),
        entry("Pending0Highlight00001", "d", None),
        orphan,
        undated,
    ]);
    let a = Archive::load(arch.path()).unwrap();
    let p = plan(lib.path(), &a, &PushOptions::default()).unwrap();
    assert_eq!(p.books[0].added, 1);
    assert_eq!(
        p.skipped.get("approximate (pass --include approximate)"),
        Some(&1)
    );
    assert_eq!(
        p.skipped.get("ambiguous (pass --include ambiguous)"),
        Some(&1)
    );
    assert_eq!(p.skipped.get("unmatched"), Some(&1));
    assert_eq!(
        p.skipped.get("no EPUB anchor yet (run derive-calibre)"),
        Some(&1)
    );
    assert_eq!(p.skipped.get("orphan note"), Some(&1));
    assert_eq!(p.skipped.get("no date (calibre needs one)"), Some(&1));

    let all = PushOptions {
        include_approximate: true,
        include_ambiguous: true,
    };
    assert_eq!(plan(lib.path(), &a, &all).unwrap().books[0].added, 3);
}

#[test]
fn everything_else_in_calibre_is_kept_and_ours_replaced_in_place() {
    let other = json!({"type": "highlight", "uuid": "Other0Highlight0000001", "timestamp": "2025-01-01T00:00:00.000Z",
        "start_cfi": "/2/4/4/1:0", "end_cfi": "/2/4/4/1:3", "highlighted_text": "Not ours."});
    let bookmark = json!({"type": "bookmark", "title": "Chapter mark", "timestamp": "2025-01-01T00:00:00.000Z", "pos": "epubcfi(/6/2)", "pos_type": "epubcfi"});
    let removed = json!({"type": "highlight", "uuid": "Gone0Highlight00000001", "removed": true, "timestamp": "2024-01-01T00:00:00.000Z"});
    let stale_mine = json!({"type": "highlight", "uuid": MINE, "timestamp": "2026-01-04T08:00:00.000Z",
        "start_cfi": "/2/4/2/1:0", "end_cfi": "/2/4/2/1:30", "highlighted_text": "Old wording."});
    let lib = library(&[other.clone(), bookmark.clone(), stale_mine, removed.clone()]);
    let arch = archive(vec![mine()]);
    let p = plan(
        lib.path(),
        &Archive::load(arch.path()).unwrap(),
        &PushOptions::default(),
    )
    .unwrap();
    let b = &p.books[0];
    assert_eq!((b.added, b.updated, b.kept), (0, 1, 3));
    let list: Vec<&Value> = b.annotations.iter().map(|a| &a.annotation).collect();
    assert_eq!(list[0], &other);
    assert_eq!(list[1], &bookmark);
    assert_eq!(list[2]["uuid"], MINE);
    assert_eq!(
        list[2]["highlighted_text"], "The orchard keeps two ledgers.",
        "the archive is the source of truth"
    );
    assert_eq!(list[3], &removed);
    assert_eq!(
        b.annotations[0].timestamp, 1.5,
        "kept rows keep calibre's own timestamp"
    );
}

#[test]
fn a_book_already_matching_the_archive_is_not_written() {
    let arch = archive(vec![mine()]);
    let a = Archive::load(arch.path()).unwrap();
    let first = plan(library(&[]).path(), &a, &PushOptions::default()).unwrap();
    let pushed = first.books[0].annotations[0].annotation.clone();
    let p = plan(library(&[pushed]).path(), &a, &PushOptions::default()).unwrap();
    assert!(p.books.is_empty());
    assert_eq!(p.up_to_date, 1);
}

#[test]
fn a_highlight_deleted_in_calibre_is_restored_with_its_id() {
    let removed = json!({"type": "highlight", "uuid": MINE, "removed": true, "timestamp": "2026-02-01T00:00:00.000Z"});
    let lib = library(&[removed]);
    let arch = archive(vec![mine()]);
    let p = plan(
        lib.path(),
        &Archive::load(arch.path()).unwrap(),
        &PushOptions::default(),
    )
    .unwrap();
    let b = &p.books[0];
    assert_eq!((b.added, b.updated, b.kept), (0, 1, 0));
    assert_eq!(b.annotations[0].annotation["uuid"], MINE);
    assert!(b.annotations[0].annotation.get("removed").is_none());
}

#[test]
fn unlinked_and_epub_less_books_are_skipped() {
    let lib = library(&[]);
    let arch = archive(vec![mine()]);
    let mut a = Archive::load(arch.path()).unwrap();
    a.books[0].data.book.calibre_uuid = None;
    let p = plan(lib.path(), &a, &PushOptions::default()).unwrap();
    assert!(p.books.is_empty());
    assert_eq!(p.skipped.get("book not linked to calibre"), Some(&1));
}

#[test]
fn dry_run_touches_neither_calibre_nor_the_archive() {
    let lib = library(&[]);
    let arch = archive(vec![mine()]);
    let db = fs::read(lib.path().join("metadata.db")).unwrap();
    let before = fs::read_dir(arch.path()).unwrap().count();
    let r = push(
        lib.path(),
        arch.path(),
        &PushOptions::default(),
        Path::new("/nonexistent/calibre-debug"),
        true,
    )
    .unwrap();
    assert_eq!(r.plan.books.len(), 1);
    assert!(r.backup.is_none());
    assert_eq!(fs::read(lib.path().join("metadata.db")).unwrap(), db);
    assert_eq!(
        fs::read_dir(lib.path()).unwrap().count(),
        1,
        "no backup made"
    );
    assert_eq!(fs::read_dir(arch.path()).unwrap().count(), before);
}

fn snapshot(dir: &Path) -> Vec<(String, Vec<u8>)> {
    let mut v: Vec<_> = fs::read_dir(dir)
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

/// Runs calibre's own writer, so only where calibre is installed:
///     cargo test --test push -- --ignored
#[test]
#[ignore]
fn push_then_read_back_is_a_zero_diff_round_trip_with_calibre() {
    let debug = calibre_debug(None);
    let other = json!({"type": "highlight", "uuid": "Other0Highlight0000001", "timestamp": "2025-01-01T00:00:00.000Z",
        "start_cfi": "/2/4/4/1:0", "end_cfi": "/2/4/4/1:3", "highlighted_text": "Not ours.", "spine_name": "OEBPS/ch0.xhtml", "spine_index": 0});
    let lib = library(&[other]);
    let arch = archive(vec![mine()]);
    // As in real use, calibre's own highlights are ingested before a push.
    ingest_calibre(lib.path(), arch.path(), &[], false).unwrap();

    let r = push(
        lib.path(),
        arch.path(),
        &PushOptions::default(),
        &debug,
        false,
    )
    .unwrap();
    assert_eq!(r.plan.books.len(), 1);
    let b = &r.plan.books[0];
    assert_eq!(
        (b.added, b.updated, b.unchanged),
        (1, 0, 1),
        "the native highlight is pushed back unchanged"
    );
    assert!(r.backup.as_ref().unwrap().is_file());
    let a = Archive::load(arch.path()).unwrap();
    let mine_now = a.books[0]
        .data
        .entries
        .iter()
        .find(|e| e.id == MINE)
        .unwrap();
    assert_eq!(
        mine_now.sources,
        vec!["calibre", "kindle"],
        "push records its effect"
    );

    let db = Connection::open(lib.path().join("metadata.db")).unwrap();
    let count: i64 = db
        .query_row("SELECT count(*) FROM annotations WHERE book = 7", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(count, 2, "ours added, the other kept");
    let dirtied: i64 = db
        .query_row(
            "SELECT count(*) FROM annotations_dirtied WHERE book = 7",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(dirtied, 1, "calibre's own code marked the book for backup");

    let before = snapshot(arch.path());
    ingest_calibre(lib.path(), arch.path(), &[], false).unwrap();
    let text = |s: Vec<(String, Vec<u8>)>| -> Vec<(String, String)> {
        s.into_iter()
            .map(|(n, b)| (n, String::from_utf8(b).unwrap()))
            .collect()
    };
    assert_eq!(
        text(snapshot(arch.path())),
        text(before),
        "reading the push back changes nothing"
    );

    let again = push(
        lib.path(),
        arch.path(),
        &PushOptions::default(),
        &debug,
        false,
    )
    .unwrap();
    assert!(again.plan.books.is_empty(), "pushing twice is a no-op");
}
