use std::fs;
use std::path::Path;

use annot_archive::archive::{Archive, Entry};
use annot_archive::check::{Issue, check};
use annot_archive::config::Config;
use annot_archive::ingest::ingest_clippings;
use annot_archive::resolve::{Lookup, resolve};

const BOOK: &str = "The Orchard Ledger (Penn, Mara)";

fn hl(book: &str, loc: &str, minute: u32, text: &str) -> String {
    format!(
        "{book}\n- Your Highlight at location {loc} | Added on Sunday, 4 January 2026 10:{minute:02}:00\n\n{text}\n==========\n"
    )
}

const LEDGER: &str =
    "The orchard keeps two ledgers, one in ink and one in the rings of its oldest trees";
const EXTENDED: &str = "The orchard keeps two ledgers, one in ink and one in the rings of its oldest trees, and only the second never lies.";

fn archive_with_an_edit() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let clippings = [
        hl(BOOK, "10-11", 1, LEDGER),
        hl(
            BOOK,
            "30-31",
            2,
            "Pruning is a conversation with next year, conducted in absence.",
        ),
        hl(
            "Field Pamphlet (Stone, Abel)",
            "5-6",
            3,
            "Mulch is the soil's own patience.",
        ),
        hl(BOOK, "10-12", 4, EXTENDED),
    ]
    .concat();
    ingest_clippings(&clippings, dir.path(), &Config::default(), false).unwrap();
    dir
}

fn edit(dir: &Path, f: impl Fn(&mut Archive)) {
    let mut a = Archive::load(dir).unwrap();
    f(&mut a);
    a.save(false).unwrap();
}

fn entry_mut<'a>(a: &'a mut Archive, text_start: &str) -> &'a mut Entry {
    a.books
        .iter_mut()
        .flat_map(|b| b.data.entries.iter_mut())
        .find(|e| e.text.as_deref().unwrap_or("").starts_with(text_start))
        .unwrap()
}

fn ids(dir: &Path, text_start: &str) -> (String, Vec<String>) {
    let mut a = Archive::load(dir).unwrap();
    let e = entry_mut(&mut a, text_start);
    (e.id.clone(), e.aliases.clone())
}

fn messages(issues: &[Issue]) -> Vec<&str> {
    issues.iter().map(|i| i.message.as_str()).collect()
}

#[test]
fn a_freshly_ingested_archive_is_clean() {
    let dir = archive_with_an_edit();
    assert_eq!(check(dir.path()), vec![]);
}

#[test]
fn duplicate_ids_are_reported() {
    let dir = archive_with_an_edit();
    let (ledger, _) = ids(dir.path(), "The orchard keeps");
    edit(dir.path(), |a| entry_mut(a, "Pruning").id = ledger.clone());
    let issues = check(dir.path());
    assert!(
        messages(&issues)
            .iter()
            .any(|m| m.starts_with("ID is used by 2 entries")),
        "{issues:#?}"
    );
}

#[test]
fn an_alias_that_is_another_entrys_id_is_reported() {
    let dir = archive_with_an_edit();
    let (pruning, _) = ids(dir.path(), "Pruning");
    edit(dir.path(), |a| {
        entry_mut(a, "The orchard keeps")
            .aliases
            .push(pruning.clone())
    });
    let issues = check(dir.path());
    assert!(
        messages(&issues)
            .iter()
            .any(|m| m.contains("is another entry's ID")),
        "{issues:#?}"
    );
}

#[test]
fn an_alias_shared_by_two_entries_is_reported() {
    let dir = archive_with_an_edit();
    let (_, aliases) = ids(dir.path(), "The orchard keeps");
    edit(dir.path(), |a| {
        entry_mut(a, "Pruning").aliases.push(aliases[0].clone())
    });
    let issues = check(dir.path());
    assert!(
        messages(&issues)
            .iter()
            .any(|m| m.contains("belongs to several entries")),
        "{issues:#?}"
    );
}

#[test]
fn schema_problems_are_reported() {
    let dir = archive_with_an_edit();
    edit(dir.path(), |a| {
        let e = entry_mut(a, "Pruning");
        e.created = Some("4 January 2026".into());
        e.kindle = None;
        e.aliases.push("short".into());
    });
    let issues = check(dir.path());
    let m = messages(&issues);
    assert!(
        m.iter()
            .any(|m| m.contains("is not YYYY-MM-DDTHH:MM:SS.mmmZ")),
        "{issues:#?}"
    );
    assert!(
        m.iter()
            .any(|m| m.contains("kindle block and kindle source disagree")),
        "{issues:#?}"
    );
    assert!(
        m.iter()
            .any(|m| m.contains("alias \"short\" is not 22 characters")),
        "{issues:#?}"
    );
}

#[test]
fn shared_calibre_books_and_title_keys_are_reported() {
    let dir = archive_with_an_edit();
    edit(dir.path(), |a| {
        for b in &mut a.books {
            b.data.book.calibre_uuid = Some("same-calibre-book".into());
            b.data.book.kindle_titles.push(BOOK.into());
        }
    });
    let issues = check(dir.path());
    let m = messages(&issues);
    assert!(
        m.iter()
            .any(|m| m.starts_with("calibre book same-calibre-book is claimed by 2 files")),
        "{issues:#?}"
    );
    assert!(
        m.iter().any(|m| m.contains("is listed in several files")),
        "{issues:#?}"
    );
}

#[test]
fn a_hand_edited_non_canonical_file_is_reported() {
    let dir = archive_with_an_edit();
    let name = Archive::load(dir.path()).unwrap().books[0]
        .file_name
        .clone();
    let path = dir.path().join(&name);
    let compact = serde_json::to_string(
        &serde_json::from_str::<serde_json::Value>(&fs::read_to_string(&path).unwrap()).unwrap(),
    )
    .unwrap();
    fs::write(&path, compact).unwrap();
    let issues = check(dir.path());
    assert_eq!(issues.len(), 1, "{issues:#?}");
    assert_eq!(issues[0].file, name);
    assert!(issues[0].message.starts_with("not in canonical form"));
}

#[test]
fn an_unloadable_file_is_the_one_issue() {
    let dir = archive_with_an_edit();
    fs::write(dir.path().join("broken--00000000.json"), "{").unwrap();
    let issues = check(dir.path());
    assert_eq!(issues.len(), 1);
    assert!(issues[0].message.contains("broken--00000000.json"));
}

#[test]
fn resolve_finds_by_id_and_by_alias() {
    let dir = archive_with_an_edit();
    let (id, aliases) = ids(dir.path(), "The orchard keeps");
    assert_eq!(aliases.len(), 1);

    let Lookup::Found(r) = resolve(dir.path(), &id).unwrap() else {
        panic!("not found by ID")
    };
    assert!(!r.via_alias());
    assert_eq!(r.entry.text.as_deref(), Some(EXTENDED));
    assert_eq!(r.book.title, "The Orchard Ledger");

    let Lookup::Found(r) = resolve(dir.path(), &aliases[0]).unwrap() else {
        panic!("not found by alias")
    };
    assert!(r.via_alias());
    assert_eq!(r.canonical_id(), id);
}

#[test]
fn resolve_reports_tombstones_and_unknown_ids() {
    let dir = archive_with_an_edit();
    let (id, _) = ids(dir.path(), "Pruning");
    edit(dir.path(), |a| entry_mut(a, "Pruning").deleted = true);
    assert!(
        matches!(resolve(dir.path(), &id).unwrap(), Lookup::Deleted(r) if r.canonical_id() == id)
    );
    assert_eq!(
        resolve(dir.path(), "AAAAAAAAAAAAAAAAAAAAAA").unwrap(),
        Lookup::NotFound
    );
}

#[test]
fn resolve_refuses_to_guess_between_claimants() {
    let dir = archive_with_an_edit();
    let (_, aliases) = ids(dir.path(), "The orchard keeps");
    edit(dir.path(), |a| {
        entry_mut(a, "Pruning").aliases.push(aliases[0].clone())
    });
    assert!(
        matches!(resolve(dir.path(), &aliases[0]).unwrap(), Lookup::Ambiguous(h) if h.len() == 2)
    );
}
