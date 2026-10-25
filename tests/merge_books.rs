use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use annot_archive::archive::{Archive, Book, Entry, EntryType, KindleBlock};
use annot_archive::check::check;
use annot_archive::merge_books::merge_books;
use serde_json::json;

fn entry(id: &str, source: &str) -> Entry {
    Entry {
        id: id.into(),
        aliases: Vec::new(),
        kind: EntryType::Highlight,
        text: Some(format!("Text of {id}.")),
        note: None,
        created: Some("2026-01-04T08:00:00.000Z".into()),
        deleted: false,
        sources: vec![source.into()],
        kindle: (source == "kindle").then(|| KindleBlock {
            location: Some([1, 2]),
            page: None,
            raw_added_on: "Sunday, 4 January 2026 10:00:00".into(),
        }),
        calibre: (source == "calibre")
            .then(|| json!({"match": "native", "highlighted_text": format!("Text of {id}.")})),
        other: BTreeMap::new(),
    }
}

fn book(title: &str, kindle_titles: &[&str], calibre_uuid: Option<&str>) -> Book {
    Book {
        title: title.into(),
        authors: vec!["Mara Penn".into()],
        kindle_titles: kindle_titles.iter().map(|t| t.to_string()).collect(),
        calibre_uuid: calibre_uuid.map(String::from),
        other: BTreeMap::new(),
    }
}

/// A Kindle-only file and a calibre-only file for the same book.
fn two_files() -> (tempfile::TempDir, String, String) {
    let dir = tempfile::tempdir().unwrap();
    let mut a = Archive::load(dir.path()).unwrap();
    let kindle = a.add_book(
        book(
            "The Orchard Ledger",
            &["The Orchard Ledger (Penn, Mara)"],
            None,
        ),
        "kindle key",
    );
    kindle.data.entries = vec![
        entry("Kindle0Highlight000001", "kindle"),
        entry("Kindle0Highlight000002", "kindle"),
    ];
    let kindle_name = kindle.file_name.clone();
    let calibre = a.add_book(
        book(
            "The Orchard Ledger: A Year in Rings",
            &[],
            Some("book-uuid"),
        ),
        "calibre:book-uuid",
    );
    calibre.data.entries = vec![entry("Calibre0Highlight00001", "calibre")];
    let calibre_name = calibre.file_name.clone();
    a.save(false).unwrap();
    (dir, kindle_name, calibre_name)
}

fn names(dir: &Path) -> Vec<String> {
    let mut v: Vec<String> = fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    v.sort();
    v
}

#[test]
fn absorbing_moves_entries_and_keys_and_deletes_the_file() {
    let (dir, kindle, calibre) = two_files();
    let r = merge_books(dir.path(), &calibre, &kindle, false).unwrap();
    assert_eq!(
        (r.kept.as_str(), r.absorbed.as_str(), r.moved_entries),
        (calibre.as_str(), kindle.as_str(), 2)
    );
    assert_eq!(
        r.added_kindle_titles,
        vec!["The Orchard Ledger (Penn, Mara)"]
    );
    assert_eq!(r.took_calibre_uuid, None);
    assert_eq!(names(dir.path()), vec![calibre.clone()]);

    let a = Archive::load(dir.path()).unwrap();
    let b = &a.books[0].data;
    assert_eq!(
        b.book.title, "The Orchard Ledger: A Year in Rings",
        "the kept file's title wins"
    );
    assert_eq!(b.book.calibre_uuid.as_deref(), Some("book-uuid"));
    assert_eq!(
        b.book.kindle_titles,
        vec!["The Orchard Ledger (Penn, Mara)"]
    );
    assert_eq!(
        b.entries.len(),
        3,
        "entries keep their identities; none are combined"
    );
    assert_eq!(check(dir.path()), vec![]);
}

#[test]
fn keeping_the_kindle_file_takes_over_the_calibre_uuid() {
    let (dir, kindle, calibre) = two_files();
    let r = merge_books(dir.path(), &kindle, &calibre, false).unwrap();
    assert_eq!(r.took_calibre_uuid.as_deref(), Some("book-uuid"));
    let a = Archive::load(dir.path()).unwrap();
    assert_eq!(
        a.books[0].data.book.calibre_uuid.as_deref(),
        Some("book-uuid")
    );
    assert_eq!(a.books[0].file_name, kindle);
}

#[test]
fn names_work_with_or_without_the_extension_or_a_path() {
    let (dir, kindle, calibre) = two_files();
    let bare = calibre.trim_end_matches(".json");
    let path = dir.path().join(&kindle);
    merge_books(dir.path(), bare, path.to_str().unwrap(), false).unwrap();
    assert_eq!(names(dir.path()), vec![calibre]);
}

#[test]
fn dry_run_changes_nothing() {
    let (dir, kindle, calibre) = two_files();
    let before: Vec<Vec<u8>> = names(dir.path())
        .iter()
        .map(|n| fs::read(dir.path().join(n)).unwrap())
        .collect();
    let r = merge_books(dir.path(), &calibre, &kindle, true).unwrap();
    assert_eq!(r.saved.removed, vec![kindle]);
    let after: Vec<Vec<u8>> = names(dir.path())
        .iter()
        .map(|n| fs::read(dir.path().join(n)).unwrap())
        .collect();
    assert_eq!(after, before);
}

#[test]
fn different_calibre_books_are_refused() {
    let (dir, kindle, calibre) = two_files();
    let mut a = Archive::load(dir.path()).unwrap();
    a.books
        .iter_mut()
        .find(|b| b.file_name == kindle)
        .unwrap()
        .data
        .book
        .calibre_uuid = Some("other-book".into());
    a.save(false).unwrap();
    let err = merge_books(dir.path(), &calibre, &kindle, false).unwrap_err();
    assert!(err.contains("different calibre books"), "{err}");
    assert_eq!(names(dir.path()).len(), 2);
}

#[test]
fn shared_entry_ids_are_refused() {
    let (dir, kindle, calibre) = two_files();
    let mut a = Archive::load(dir.path()).unwrap();
    let k = a.books.iter_mut().find(|b| b.file_name == kindle).unwrap();
    k.data.entries[0]
        .aliases
        .push("Calibre0Highlight00001".into());
    a.save(false).unwrap();
    let err = merge_books(dir.path(), &calibre, &kindle, false).unwrap_err();
    assert!(err.contains("Calibre0Highlight00001"), "{err}");
}

#[test]
fn unknown_and_identical_files_are_refused() {
    let (dir, _, calibre) = two_files();
    assert!(
        merge_books(dir.path(), &calibre, "nope", false)
            .unwrap_err()
            .contains("no book file nope.json")
    );
    assert!(
        merge_books(dir.path(), &calibre, &calibre, false)
            .unwrap_err()
            .contains("same file")
    );
}
