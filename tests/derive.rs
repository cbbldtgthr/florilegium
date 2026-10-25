use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::Path;

use annot_archive::archive::{Archive, Book, Entry};
use annot_archive::check::check;
use annot_archive::config::Config;
use annot_archive::derive::{BookOutcome, DeriveConfig, derive_calibre};
use annot_archive::ingest::ingest_clippings;
use rusqlite::Connection;
use serde_json::{Value, json};

const HEAD: &str = r#"<?xml version="1.0" encoding="utf-8"?><html xmlns="http://www.w3.org/1999/xhtml"><head><title>t</title></head>"#;

fn epub(path: &Path, chapters: &[&str]) {
    let mut zip = zip::ZipWriter::new(fs::File::create(path).unwrap());
    let stored =
        zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    let mut put = |name: &str, body: &str| {
        zip.start_file(name, stored).unwrap();
        zip.write_all(body.as_bytes()).unwrap();
    };
    put("mimetype", "application/epub+zip");
    put(
        "META-INF/container.xml",
        r#"<container xmlns="urn:oasis:names:tc:opendocument:xmlns:container"><rootfiles><rootfile full-path="OEBPS/content.opf"/></rootfiles></container>"#,
    );
    let items: String = (0..chapters.len())
        .map(|i| {
            format!(r#"<item id="c{i}" href="ch{i}.xhtml" media-type="application/xhtml+xml"/>"#)
        })
        .collect();
    let refs: String = (0..chapters.len())
        .map(|i| format!(r#"<itemref idref="c{i}"/>"#))
        .collect();
    put(
        "OEBPS/content.opf",
        &format!(
            r#"<package xmlns="http://www.idpf.org/2007/opf"><manifest>{items}</manifest><spine>{refs}</spine></package>"#
        ),
    );
    for (i, body) in chapters.iter().enumerate() {
        put(
            &format!("OEBPS/ch{i}.xhtml"),
            &format!("{HEAD}<body>{body}</body></html>"),
        );
    }
    zip.finish().unwrap();
}

const ORCHARD: &str = "11111111-0000-4000-8000-000000000007";
const SECOND_COPY: &str = "11111111-0000-4000-8000-000000000010";
const OTHER_NAME: &str = "11111111-0000-4000-8000-000000000011";

/// A calibre library: the Orchard Ledger (EPUB), Inkwell Notes (AZW3 only),
/// Second Copy (EPUB, already in the archive), and Totally Different Name
/// (EPUB, reached only through an override).
fn library() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let db = Connection::open(dir.path().join("metadata.db")).unwrap();
    db.execute_batch(&format!(
        "CREATE TABLE books (id INTEGER PRIMARY KEY, title TEXT, path TEXT, uuid TEXT);
         CREATE TABLE authors (id INTEGER PRIMARY KEY, name TEXT);
         CREATE TABLE books_authors_link (id INTEGER PRIMARY KEY, book INTEGER, author INTEGER);
         CREATE TABLE data (id INTEGER PRIMARY KEY, book INTEGER, format TEXT, name TEXT);
         CREATE TABLE annotations (id INTEGER PRIMARY KEY, book INTEGER, format TEXT, user_type TEXT, user TEXT,
             timestamp REAL, annot_id TEXT, annot_type TEXT, annot_data TEXT, searchable_text TEXT);
         INSERT INTO books VALUES (7, 'The Orchard Ledger', 'orchard', '{ORCHARD}');
         INSERT INTO books VALUES (9, 'Inkwell Notes', 'inkwell', '11111111-0000-4000-8000-000000000009');
         INSERT INTO books VALUES (10, 'Second Copy', 'second', '{SECOND_COPY}');
         INSERT INTO books VALUES (11, 'Totally Different Name', 'other', '{OTHER_NAME}');
         INSERT INTO authors VALUES (1, 'Mara Penn'), (2, 'Ivo Penn');
         INSERT INTO books_authors_link VALUES (1, 7, 1), (2, 9, 1), (3, 10, 2);
         INSERT INTO data VALUES (1, 7, 'EPUB', 'book'), (2, 9, 'AZW3', 'book'), (3, 10, 'EPUB', 'book'), (4, 11, 'EPUB', 'book');"
    ))
    .unwrap();
    for d in ["orchard", "inkwell", "second", "other"] {
        fs::create_dir(dir.path().join(d)).unwrap();
    }
    // Filler keeps chapter 1's repeat of "Echo of the hills." far from
    // chapter 0's, so location can tell them apart.
    let chapter1 = format!(
        "<p>{}</p><p>Echo of the hills.</p><p>Bees read the light, not the calendar.</p>{}",
        "orchard rows run on and on ".repeat(40),
        "<p>Gulls argue over a single bright thing.</p><p>This sentence ends the chapter and</p>"
    );
    epub(
        &dir.path().join("orchard/book.epub"),
        &[
            "<p>The orchard keeps two ledgers.</p><p>Frost settles first in the low corners.</p><p>Echo of the hills.</p>",
            &chapter1,
            "<p>continues in the next one.</p>",
        ],
    );
    epub(
        &dir.path().join("second/book.epub"),
        &["<p>Second copy text.</p>"],
    );
    epub(
        &dir.path().join("other/book.epub"),
        &["<p>Reached through an override.</p>"],
    );
    dir
}

fn clip(book: &str, kind: &str, loc: u32, minute: u32, text: &str) -> String {
    format!(
        "{book}\n- Your {kind} at location {loc} | Added on Sunday, 4 January 2026 10:{minute:02}:00\n\n{text}\n==========\n"
    )
}

const LEDGER: &str = "The Orchard Ledger (Penn, Mara)";

fn clippings() -> String {
    [
        clip(LEDGER, "Highlight", 10, 1, "The orchard keeps two ledgers."),
        clip(
            LEDGER,
            "Highlight",
            20,
            2,
            "Frost settles first in the low corners.",
        ),
        clip(LEDGER, "Highlight", 30, 3, "Echo of the hills."),
        clip(
            LEDGER,
            "Highlight",
            60,
            4,
            "Bees read the light, not the calendar.",
        ),
        clip(
            LEDGER,
            "Highlight",
            70,
            5,
            "Gulls argue over a single bright thingy.",
        ),
        clip(
            LEDGER,
            "Highlight",
            90,
            6,
            "This sentence ends the chapter and continues in the next one.",
        ),
        clip(
            LEDGER,
            "Highlight",
            95,
            7,
            "Words that appear nowhere in the book at all.",
        ),
        clip(LEDGER, "Note", 999, 8, "A note on nothing."),
        clip(
            "Field Pamphlet (Stone, Abel)",
            "Highlight",
            5,
            9,
            "Mulch is the soil's own patience.",
        ),
        clip(
            "Inkwell Notes (Penn, Mara)",
            "Highlight",
            5,
            10,
            "Ink remembers.",
        ),
        clip(
            "Second Copy (Penn, Ivo)",
            "Highlight",
            5,
            11,
            "Second copy text.",
        ),
        clip(
            "Odd Kindle Title (Nobody)",
            "Highlight",
            5,
            12,
            "Reached through an override.",
        ),
    ]
    .concat()
}

/// The archive after a Kindle ingest, plus a calibre file for Second Copy and
/// an override for the oddly titled book.
fn archive() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    ingest_clippings(&clippings(), dir.path(), &Config::default(), false).unwrap();
    let mut a = Archive::load(dir.path()).unwrap();
    a.add_book(
        Book {
            title: "Second Copy".into(),
            authors: vec!["Ivo Penn".into()],
            kindle_titles: Vec::new(),
            calibre_uuid: Some(SECOND_COPY.into()),
            other: BTreeMap::new(),
        },
        "calibre:second",
    );
    a.save(false).unwrap();
    fs::write(
        dir.path().join("book-overrides.json"),
        format!(r#"{{"Odd Kindle Title (Nobody)": {{"calibre_uuid": "{OTHER_NAME}"}}}}"#),
    )
    .unwrap();
    dir
}

fn derive(lib: &Path, archive: &Path, rederive: bool) -> annot_archive::derive::DeriveReport {
    derive_calibre(lib, archive, &DeriveConfig::default(), rederive, &[], false).unwrap()
}

fn entries(archive: &Path, title: &str) -> (Option<String>, Vec<Entry>) {
    let a = Archive::load(archive).unwrap();
    let b = a
        .books
        .into_iter()
        .find(|b| b.data.book.title == title && !b.data.entries.is_empty())
        .unwrap();
    (b.data.book.calibre_uuid, b.data.entries)
}

fn block<'a>(entries: &'a [Entry], text_start: &str) -> &'a Value {
    let e = entries
        .iter()
        .find(|e| {
            e.text
                .as_deref()
                .or(e.note.as_deref())
                .unwrap_or("")
                .starts_with(text_start)
        })
        .unwrap_or_else(|| panic!("no entry {text_start:?}"));
    e.calibre
        .as_ref()
        .unwrap_or_else(|| panic!("{text_start:?} has no calibre block"))
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

#[test]
fn a_matched_book_is_linked_and_its_highlights_anchored() {
    let (lib, arch) = (library(), archive());
    derive(lib.path(), arch.path(), false);
    let (uuid, e) = entries(arch.path(), "The Orchard Ledger");
    assert_eq!(uuid.as_deref(), Some(ORCHARD));
    let b = block(&e, "The orchard keeps");
    assert_eq!(b["match"], "exact");
    assert_eq!(
        (b["start_cfi"].as_str(), b["end_cfi"].as_str()),
        (Some("/2/4/2/1:0"), Some("/2/4/2/1:30"))
    );
    assert_eq!(
        (b["spine_name"].as_str(), b["spine_index"].as_u64()),
        (Some("OEBPS/ch0.xhtml"), Some(0))
    );
    assert_eq!(
        (b["format"].as_str(), b["book_id"].as_i64()),
        (Some("EPUB"), Some(7))
    );
    assert!(
        b.get("highlighted_text").is_none(),
        "the entry's own text is not repeated"
    );
    assert_eq!(b["style"]["which"], "yellow");
    assert!(
        b["derived_with"]
            .as_str()
            .unwrap()
            .starts_with("annot-archive ")
    );
    assert_eq!(check(arch.path()), vec![]);
}

#[test]
fn a_repeated_passage_is_placed_by_kindle_location() {
    let (lib, arch) = (library(), archive());
    derive(lib.path(), arch.path(), false);
    let (_, e) = entries(arch.path(), "The Orchard Ledger");
    let b = block(&e, "Echo of the hills");
    assert_eq!(b["match"], "exact");
    assert_eq!(
        b["spine_name"], "OEBPS/ch0.xhtml",
        "location 30 sits between 20 (chapter 0) and 60 (chapter 1)"
    );
    assert_eq!(b["start_cfi"], "/2/4/6/1:0");
}

#[test]
fn near_misses_crossings_and_absences_are_classified() {
    let (lib, arch) = (library(), archive());
    derive(lib.path(), arch.path(), false);
    let (_, e) = entries(arch.path(), "The Orchard Ledger");
    let gulls = block(&e, "Gulls argue");
    assert_eq!(gulls["match"], "approximate");
    assert!(gulls["similarity"].as_f64().unwrap() >= 0.9);
    assert_eq!(gulls["spine_name"], "OEBPS/ch1.xhtml");
    assert_eq!(block(&e, "This sentence ends")["reason"], "cross-spine");
    assert_eq!(block(&e, "Words that appear")["reason"], "text-not-found");
    assert_eq!(block(&e, "A note on nothing")["reason"], "orphan-note");
}

#[test]
fn books_without_a_calibre_book_or_epub_are_reported() {
    let (lib, arch) = (library(), archive());
    let r = derive(lib.path(), arch.path(), false);
    let outcome = |title: &str| &r.books.iter().find(|b| b.title == title).unwrap().outcome;
    assert!(matches!(
        outcome("Field Pamphlet"),
        BookOutcome::NoCalibreBook { .. }
    ));
    assert!(matches!(
        outcome("Inkwell Notes"),
        BookOutcome::NoEpub { .. }
    ));

    let (uuid, e) = entries(arch.path(), "Field Pamphlet");
    assert_eq!(uuid, None);
    assert_eq!(block(&e, "Mulch")["reason"], "no-calibre-book");
    let (uuid, e) = entries(arch.path(), "Inkwell Notes");
    assert!(uuid.is_some(), "the book is known even without an EPUB");
    assert_eq!(block(&e, "Ink remembers")["reason"], "no-epub");
}

#[test]
fn a_book_with_its_own_calibre_file_is_left_for_merge_books() {
    let (lib, arch) = (library(), archive());
    let r = derive(lib.path(), arch.path(), false);
    let second = r.books.iter().find(|b| b.title == "Second Copy").unwrap();
    assert!(matches!(
        &second.outcome,
        BookOutcome::ClaimedByOtherFile { .. }
    ));
    let (uuid, e) = entries(arch.path(), "Second Copy");
    assert_eq!(uuid, None, "never two files claiming one calibre book");
    assert!(e[0].calibre.is_none());
    assert_eq!(check(arch.path()), vec![]);
}

#[test]
fn an_override_names_the_calibre_book() {
    let (lib, arch) = (library(), archive());
    derive(lib.path(), arch.path(), false);
    let (uuid, e) = entries(arch.path(), "Odd Kindle Title");
    assert_eq!(uuid.as_deref(), Some(OTHER_NAME));
    assert_eq!(block(&e, "Reached")["match"], "exact");
}

#[test]
fn runs_are_deterministic_and_native_or_manual_blocks_are_never_touched() {
    let (lib, arch) = (library(), archive());
    derive(lib.path(), arch.path(), false);
    let before = snapshot(arch.path());
    derive(lib.path(), arch.path(), false);
    assert_eq!(snapshot(arch.path()), before, "second run");
    derive(lib.path(), arch.path(), true);
    assert_eq!(
        snapshot(arch.path()),
        before,
        "rederive gives the same answers"
    );

    let mut a = Archive::load(arch.path()).unwrap();
    let e = a
        .books
        .iter_mut()
        .flat_map(|b| b.data.entries.iter_mut())
        .find(|e| e.text.as_deref() == Some("Frost settles first in the low corners."))
        .unwrap();
    e.calibre =
        Some(json!({"match": "manual", "start_cfi": "/2/4/4/1:0", "end_cfi": "/2/4/4/1:5"}));
    a.save(false).unwrap();
    let pinned = snapshot(arch.path());
    derive(lib.path(), arch.path(), true);
    assert_eq!(snapshot(arch.path()), pinned);
}

#[test]
fn an_equidistant_repeat_is_ambiguous_with_candidates() {
    let lib = library();
    let book = "Echo Chamber (Penn, Mara)";
    let db = Connection::open(lib.path().join("metadata.db")).unwrap();
    db.execute_batch(
        "INSERT INTO books VALUES (12, 'Echo Chamber', 'echo', '11111111-0000-4000-8000-000000000012');
         INSERT INTO books_authors_link VALUES (4, 12, 1);
         INSERT INTO data VALUES (5, 12, 'EPUB', 'book');",
    )
    .unwrap();
    fs::create_dir(lib.path().join("echo")).unwrap();
    let filler = "word ".repeat(50);
    epub(
        &lib.path().join("echo/book.epub"),
        &[&format!(
            "<p>Echo marks.</p><p>{filler}</p><p>Echo marks.</p>"
        )],
    );
    let arch = tempfile::tempdir().unwrap();
    ingest_clippings(
        &clip(book, "Highlight", 50, 1, "Echo marks."),
        arch.path(),
        &Config::default(),
        false,
    )
    .unwrap();
    // 50 of 100 locations predicts the middle of the book: no occurrence is clearly nearer.
    fs::write(
        arch.path().join("book-overrides.json"),
        format!(r#"{{"{book}": {{"total_locations": 100}}}}"#),
    )
    .unwrap();
    derive(lib.path(), arch.path(), false);
    let (_, e) = entries(arch.path(), "Echo Chamber");
    let b = block(&e, "Echo marks");
    assert_eq!(b["match"], "ambiguous");
    assert_eq!(b["occurrences"], 2);
    assert_eq!(b["candidates"].as_array().unwrap().len(), 1);
}

#[test]
fn dry_run_writes_nothing() {
    let (lib, arch) = (library(), archive());
    let before = snapshot(arch.path());
    let r = derive_calibre(
        lib.path(),
        arch.path(),
        &DeriveConfig::default(),
        false,
        &[],
        true,
    )
    .unwrap();
    assert!(!r.saved.updated.is_empty());
    assert_eq!(snapshot(arch.path()), before);
}
