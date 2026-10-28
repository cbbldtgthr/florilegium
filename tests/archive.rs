use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use florilegium::archive::{
    Archive, Book, BookFile, Entry, EntryType, KindleBlock, OVERRIDES_FILE,
};
use serde_json::json;

fn book() -> Book {
    Book {
        title: "The Lantern Keeper\u{2019}s Almanac".into(),
        authors: vec!["Orrin Vell".into()],
        kindle_titles: vec!["Z title".into(), "A title".into(), "A title".into()],
        calibre_uuid: None,
        other: BTreeMap::new(),
    }
}

fn entry(id: &str, created: Option<&str>) -> Entry {
    Entry {
        id: id.into(),
        aliases: Vec::new(),
        kind: EntryType::Highlight,
        text: Some("Text.".into()),
        note: None,
        created: created.map(String::from),
        deleted: false,
        sources: vec!["kindle".into()],
        kindle: Some(KindleBlock {
            location: Some([10, 12]),
            page: None,
            raw_added_on: "Sunday, 4 January 2026 10:00:00".into(),
        }),
        calibre: BTreeMap::new(),
        other: BTreeMap::new(),
    }
}

fn sample() -> BookFile {
    let mut f = BookFile::new(book());
    let mut tide = entry("bbbb", Some("2026-10-23T13:50:12.000Z"));
    tide.aliases = vec!["y".into(), "x".into(), "y".into()];
    tide.text = Some("Tide\u{2019}s \u{201C}own\u{201D} \u{2014} \u{2713}".into());
    let mut orphan = entry("aaaa", None);
    orphan.kind = EntryType::Note;
    orphan.text = None;
    orphan.note = Some("Orphan.".into());
    orphan.kindle = Some(KindleBlock {
        location: None,
        page: Some([4, 4]),
        raw_added_on: "bad date".into(),
    });
    let mut first = entry("cccc", Some("2026-01-01T00:00:00.000Z"));
    first.other.insert("locked_fields".into(), json!(["note"]));
    f.entries = vec![tide, orphan, first];
    f
}

// Golden bytes: pins the on-disk format. Sorted keys, two-space indent,
// unescaped UTF-8, entries by (created, nulls last; id), lists sorted and
// de-duplicated, unknown fields kept, one trailing newline.
const GOLDEN: &str = r#"{
  "book": {
    "authors": [
      "Orrin Vell"
    ],
    "calibre_uuid": null,
    "kindle_titles": [
      "A title",
      "Z title"
    ],
    "title": "The Lantern Keeper’s Almanac"
  },
  "entries": [
    {
      "aliases": [],
      "calibre": {},
      "created": "2026-01-01T00:00:00.000Z",
      "deleted": false,
      "id": "cccc",
      "kindle": {
        "location": [
          10,
          12
        ],
        "page": null,
        "raw_added_on": "Sunday, 4 January 2026 10:00:00"
      },
      "locked_fields": [
        "note"
      ],
      "note": null,
      "sources": [
        "kindle"
      ],
      "text": "Text.",
      "type": "highlight"
    },
    {
      "aliases": [
        "x",
        "y"
      ],
      "calibre": {},
      "created": "2026-10-23T13:50:12.000Z",
      "deleted": false,
      "id": "bbbb",
      "kindle": {
        "location": [
          10,
          12
        ],
        "page": null,
        "raw_added_on": "Sunday, 4 January 2026 10:00:00"
      },
      "note": null,
      "sources": [
        "kindle"
      ],
      "text": "Tide’s “own” — ✓",
      "type": "highlight"
    },
    {
      "aliases": [],
      "calibre": {},
      "created": null,
      "deleted": false,
      "id": "aaaa",
      "kindle": {
        "location": null,
        "page": [
          4,
          4
        ],
        "raw_added_on": "bad date"
      },
      "note": "Orphan.",
      "sources": [
        "kindle"
      ],
      "text": null,
      "type": "note"
    }
  ],
  "schema_version": 2
}
"#;

#[test]
fn canonical_bytes_are_pinned() {
    assert_eq!(String::from_utf8(sample().to_bytes()).unwrap(), GOLDEN);
}

#[test]
fn input_order_does_not_matter() {
    let mut shuffled = sample();
    shuffled.entries.reverse();
    shuffled.book.kindle_titles.reverse();
    assert_eq!(shuffled.to_bytes(), sample().to_bytes());
}

#[test]
fn parsing_canonical_bytes_round_trips() {
    let parsed: BookFile = serde_json::from_str(GOLDEN).unwrap();
    assert_eq!(parsed.to_bytes(), GOLDEN.as_bytes());
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
fn new_books_get_a_stable_name_and_are_written_once() {
    let dir = tempfile::tempdir().unwrap();
    let mut a = Archive::load(dir.path()).unwrap();
    let name = a
        .add_book(book(), "the lantern keeper's almanac (vell, orrin)")
        .file_name
        .clone();
    assert!(
        name.starts_with("the-lantern-keeper-s-almanac-orrin-vell--"),
        "{name}"
    );
    assert_eq!(
        name.len(),
        "the-lantern-keeper-s-almanac-orrin-vell--".len() + 8 + ".json".len()
    );

    let r = a.save(false).unwrap();
    assert_eq!(
        (r.created.as_slice(), r.updated.len(), r.unchanged),
        (std::slice::from_ref(&name), 0, 0)
    );
    assert_eq!(names(dir.path()), vec![name.clone()]);

    // A fresh load of the same directory gives the same name for the same key.
    let mut b = Archive::load(dir.path()).unwrap();
    assert_eq!(b.books[0].file_name, name);
    assert_eq!(b.save(false).unwrap().unchanged, 1);
}

#[test]
fn saving_unchanged_files_touches_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let mut a = Archive::load(dir.path()).unwrap();
    a.add_book(book(), "key").data.entries = sample().entries;
    a.save(false).unwrap();
    let path = dir.path().join(&a.books[0].file_name);
    let before = fs::metadata(&path).unwrap().modified().unwrap();

    std::thread::sleep(std::time::Duration::from_millis(20));
    let mut again = Archive::load(dir.path()).unwrap();
    let r = again.save(false).unwrap();
    assert_eq!((r.created.len(), r.updated.len(), r.unchanged), (0, 0, 1));
    assert_eq!(fs::metadata(&path).unwrap().modified().unwrap(), before);
}

#[test]
fn only_changed_files_are_rewritten() {
    let dir = tempfile::tempdir().unwrap();
    let mut a = Archive::load(dir.path()).unwrap();
    a.add_book(book(), "one");
    a.add_book(
        Book {
            title: "Second".into(),
            ..book()
        },
        "two",
    );
    a.save(false).unwrap();

    let mut b = Archive::load(dir.path()).unwrap();
    b.books[1].data.entries.push(entry("dddd", None));
    let changed = b.books[1].file_name.clone();
    let r = b.save(false).unwrap();
    assert_eq!(
        (r.created.len(), r.updated, r.unchanged),
        (0, vec![changed], 1)
    );
}

#[test]
fn a_non_canonical_file_on_disk_is_rewritten_canonically() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("hand-edited--00000000.json");
    let compact =
        serde_json::to_string(&serde_json::from_str::<serde_json::Value>(GOLDEN).unwrap()).unwrap();
    fs::write(&path, compact).unwrap();
    let mut a = Archive::load(dir.path()).unwrap();
    assert_eq!(
        a.save(false).unwrap().updated,
        vec!["hand-edited--00000000.json".to_string()]
    );
    assert_eq!(fs::read_to_string(&path).unwrap(), GOLDEN);
}

#[test]
fn dry_run_writes_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let mut a = Archive::load(dir.path()).unwrap();
    a.add_book(book(), "key");
    assert_eq!(a.save(true).unwrap().created.len(), 1);
    assert!(names(dir.path()).is_empty());
}

#[test]
fn same_slug_and_key_get_a_longer_id() {
    let dir = tempfile::tempdir().unwrap();
    let mut a = Archive::load(dir.path()).unwrap();
    let first = a.add_book(book(), "key").file_name.clone();
    let second = a.add_book(book(), "key").file_name.clone();
    assert_ne!(first, second);
    assert!(second.len() == first.len() + 1);
}

#[test]
fn overrides_and_other_files_are_not_books() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join(OVERRIDES_FILE), "{}").unwrap();
    fs::write(dir.path().join("config.toml"), "").unwrap();
    fs::write(dir.path().join("README.md"), "").unwrap();
    fs::create_dir(dir.path().join(".git")).unwrap();
    assert!(Archive::load(dir.path()).unwrap().books.is_empty());
}

#[test]
fn an_unreadable_book_file_is_an_error_naming_it() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("broken--12345678.json"), "{ not json").unwrap();
    let err = Archive::load(dir.path()).unwrap_err();
    assert!(err.contains("broken--12345678.json"), "{err}");
}

#[test]
fn a_newer_schema_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let newer = GOLDEN.replace("\"schema_version\": 2", "\"schema_version\": 3");
    fs::write(dir.path().join("future--12345678.json"), newer).unwrap();
    let err = Archive::load(dir.path()).unwrap_err();
    assert!(err.contains("schema_version 3"), "{err}");
}

#[test]
fn version_1_files_migrate_to_anchors_by_format() {
    let dir = tempfile::tempdir().unwrap();
    let v1 = json!({
        "schema_version": 1,
        "book": {"title": "T", "authors": [], "kindle_titles": [], "calibre_uuid": null},
        "entries": [
            {"id": "aaaa", "aliases": [], "type": "highlight", "text": "x", "note": null, "created": null,
             "deleted": false, "sources": ["calibre"], "kindle": null,
             "calibre": {"match": "native", "format": "AZW3", "start_cfi": "/2"}},
            {"id": "bbbb", "aliases": [], "type": "highlight", "text": "y", "note": null, "created": null,
             "deleted": false, "sources": ["kindle"], "kindle": null,
             "calibre": {"match": "unmatched", "reason": "no-epub"}},
            {"id": "cccc", "aliases": [], "type": "highlight", "text": "z", "note": null, "created": null,
             "deleted": false, "sources": ["kindle"], "kindle": null, "calibre": null}
        ]
    });
    fs::write(dir.path().join("old--00000000.json"), v1.to_string()).unwrap();
    let mut a = Archive::load(dir.path()).unwrap();
    let e = &a.books[0].data.entries;
    assert_eq!(
        e[0].calibre["AZW3"],
        json!({"match": "native", "start_cfi": "/2"})
    );
    assert_eq!(
        e[1].calibre["EPUB"]["reason"], "no-epub",
        "a v1 block without a format was the EPUB's"
    );
    assert!(e[2].calibre.is_empty());
    a.save(false).unwrap();
    let rewritten: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(dir.path().join("old--00000000.json")).unwrap())
            .unwrap();
    assert_eq!(rewritten["schema_version"], 2);
}
