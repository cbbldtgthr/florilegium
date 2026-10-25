//! Dev-only: regenerate the CFIs of a real library's EPUB viewer highlights
//! from their text alone and compare them with what calibre stored. The
//! library is opened read-only; output is counts and CFI strings, never book
//! text, and nothing is written anywhere.
//!
//!     just cfi-selfcheck /path/to/calibre/library

use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;

use annot_archive::calibre::db::Library;
use annot_archive::cfi::{Haystack, encode};
use annot_archive::epub::Epub;
use annot_archive::norm::norm_v1;
use annot_archive::xhtml::Dom;
use serde_json::Value;

/// An opened EPUB and its spine as (index, name).
type OpenedEpub = Option<(Epub, Vec<(usize, String)>)>;

fn main() {
    let root = PathBuf::from(
        std::env::args()
            .nth(1)
            .expect("usage: cfi_selfcheck LIBRARY"),
    );
    let lib = Library::open(&root).unwrap();
    let books = lib.books().unwrap();
    let mut tally: BTreeMap<&str, usize> = BTreeMap::new();
    let mut details: Vec<String> = Vec::new();
    let mut docs: HashMap<(i64, String), Result<(Dom, Haystack), String>> = HashMap::new();
    let mut epubs: HashMap<i64, OpenedEpub> = HashMap::new();

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
        if a.get("removed").and_then(Value::as_bool) == Some(true) {
            continue;
        }
        let get = |k: &str| a.get(k).and_then(Value::as_str).unwrap_or("").to_string();
        let (spine_name, start, end, text) = (
            get("spine_name"),
            get("start_cfi"),
            get("end_cfi"),
            get("highlighted_text"),
        );

        let epub = epubs.entry(row.book).or_insert_with(|| {
            let path = root.join(books.get(&row.book)?.formats.get("EPUB")?);
            let e = Epub::open(&path).ok()?;
            let spine = e.spine.iter().map(|s| (s.index, s.name.clone())).collect();
            Some((e, spine))
        });
        let Some((epub, spine)) = epub else {
            *tally.entry("no readable epub").or_default() += 1;
            continue;
        };
        let stored_index = a
            .get("spine_index")
            .and_then(Value::as_u64)
            .unwrap_or(u64::MAX) as usize;
        if !spine
            .iter()
            .any(|(i, n)| *i == stored_index && *n == spine_name)
        {
            *tally.entry("spine index/name disagree").or_default() += 1;
        }
        let doc = docs
            .entry((row.book, spine_name.clone()))
            .or_insert_with(|| {
                let bytes = epub.read(&spine_name)?;
                let dom = Dom::parse(&bytes)?;
                let hay = Haystack::new(&dom);
                Ok((dom, hay))
            });
        let (dom, hay) = match doc {
            Ok(d) => d,
            Err(e) => {
                *tally.entry("parse error").or_default() += 1;
                details.push(format!(
                    "parse error in book {} {spine_name}: {e}",
                    row.book
                ));
                continue;
            }
        };

        let needle: Vec<char> = norm_v1(&text).chars().collect();
        let found = hay.find_all(&needle);
        let generated: Vec<(String, String)> = found
            .iter()
            .filter_map(|&r| hay.points(dom, r))
            .map(|(s, e)| (encode(dom, s), encode(dom, e)))
            .collect();
        let outcome = if found.is_empty() {
            "text not found"
        } else if generated.iter().any(|(s, e)| *s == start && *e == end) {
            if found.len() == 1 {
                "exact: both CFIs identical"
            } else {
                "exact among several occurrences"
            }
        } else if generated.iter().any(|(s, _)| *s == start) && !end.contains(':') {
            "start identical; stored end is element-only"
        } else if text != text.trim()
            && generated
                .iter()
                .any(|(s, e)| off_by_one(s, &start) && off_by_one(e, &end))
        {
            "selection took in edge whitespace (one character)"
        } else if start.contains("^-") || legacy_step(&start) {
            "2020-era encoding (escaped '-' in ids or offset step after an inline element)"
        } else {
            "CFIs differ"
        };
        *tally.entry(outcome).or_default() += 1;
        if outcome == "CFIs differ" || outcome == "text not found" {
            details.push(format!(
                "{outcome}: book {} {spine_name}\n    stored    {start} .. {end}\n    generated {}",
                row.book,
                generated
                    .iter()
                    .map(|(s, e)| format!("{s} .. {e}"))
                    .collect::<Vec<_>>()
                    .join(" | ")
            ));
        }
    }

    println!("{} EPUB viewer highlights", tally.values().sum::<usize>());
    for (k, v) in &tally {
        println!("{v:5}  {k}");
    }
    for d in &details {
        println!("{d}");
    }
}

/// Same path, offsets at most one apart.
fn off_by_one(a: &str, b: &str) -> bool {
    let split = |c: &str| {
        c.rsplit_once(':')
            .map(|(p, o)| (p.to_string(), o.parse::<i64>().unwrap_or(-9)))
    };
    matches!((split(a), split(b)), (Some((pa, oa)), Some((pb, ob))) if pa == pb && (oa - ob).abs() <= 1)
}

/// A text step other than /1 before the offset: current calibre always walks
/// back to the first child.
fn legacy_step(cfi: &str) -> bool {
    cfi.rsplit_once(':')
        .and_then(|(p, _)| p.rsplit('/').next())
        .is_some_and(|s| s.parse::<u32>().is_ok_and(|n| n % 2 == 1 && n > 1))
}
