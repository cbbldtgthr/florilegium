//! Dev-only: for a real library's EPUB viewer highlights that calibre stored
//! chapter titles for (`toc_family_titles`), compute the titles from the
//! EPUB's table of contents and compare. Read-only; prints counts, and for
//! mismatches the two title chains.
//!
//!     just toc-selfcheck /path/to/calibre/library

use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;

use annot_archive::calibre::db::Library;
use annot_archive::derive::locate::{BookText, Located};
use serde_json::Value;

fn main() {
    let root = PathBuf::from(
        std::env::args()
            .nth(1)
            .expect("usage: toc_selfcheck LIBRARY"),
    );
    let lib = Library::open(&root).unwrap();
    let books = lib.books().unwrap();
    let mut texts: HashMap<i64, Option<BookText>> = HashMap::new();
    let mut tally: BTreeMap<&str, usize> = BTreeMap::new();
    let mut details = Vec::new();

    for row in lib.annotations(&[]).unwrap() {
        if row.format != "EPUB"
            || row.user_type != "local"
            || row.user != "viewer"
            || row.annot_type != "highlight"
        {
            continue;
        }
        let Ok(Value::Object(a)) = &row.data else {
            continue;
        };
        let Some(Value::Array(stored)) = a.get("toc_family_titles") else {
            continue;
        };
        let stored: Vec<String> = stored
            .iter()
            .filter_map(|t| t.as_str().map(String::from))
            .collect();
        let get = |k: &str| a.get(k).and_then(Value::as_str).unwrap_or("").to_string();
        let text = texts.entry(row.book).or_insert_with(|| {
            let path = root.join(books.get(&row.book)?.formats.get("EPUB")?);
            BookText::open(&path).ok()
        });
        let Some(text) = text else {
            *tally.entry("no readable EPUB").or_default() += 1;
            continue;
        };
        let hits = match text.locate(&get("highlighted_text"), 0.9) {
            Located::One(h) => vec![h],
            Located::Many(hs) => hs,
            _ => {
                *tally.entry("text not located").or_default() += 1;
                continue;
            }
        };
        // The occurrence calibre anchored, when its CFIs say which.
        let hit = hits
            .iter()
            .find(|h| text.cfis(**h).is_some_and(|(s, _)| s == get("start_cfi")))
            .or(hits.first())
            .copied()
            .unwrap();
        let ours = text.toc_family(hit);
        let outcome = if ours == stored {
            "same titles"
        } else if stored.starts_with(&ours) || ours.starts_with(&stored) {
            "one is a prefix of the other"
        } else {
            "different titles"
        };
        *tally.entry(outcome).or_default() += 1;
        if outcome != "same titles" {
            details.push(format!(
                "book {} {}\n    calibre {stored:?}\n    ours    {ours:?}",
                row.book,
                get("spine_name")
            ));
        }
    }
    println!(
        "{} EPUB viewer highlights with stored chapter titles",
        tally.values().sum::<usize>()
    );
    for (k, v) in &tally {
        println!("{v:5}  {k}");
    }
    for d in &details {
        println!("{d}");
    }
}
