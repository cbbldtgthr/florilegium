//! The calibre source: highlights made in calibre's viewer.

pub mod db;

use serde_json::{Map, Value};

use crate::archive::{Entry, EntryType};
use crate::merge::{Incoming, SourceBlock};
use db::AnnotationRow;

/// The only format the tool ever writes highlights to (or derives CFIs for).
/// Highlights on other formats are read and archived, never written back.
pub const WRITABLE_FORMAT: &str = "EPUB";

/// Keys the tool adds to the calibre block beside calibre's own annotation.
const MATCH: &str = "match";
/// Derived-match bookkeeping kept when a read-back confirms our own CFIs.
const DERIVED_KEYS: [&str; 4] = ["match", "reason", "candidates", "derived_with"];

/// Why a viewer highlight row did not become an incoming entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Skip {
    Removed,
    Unreadable(String),
}

/// A viewer highlight row as an incoming entry. The calibre block holds the
/// annotation verbatim (so calibre's own text survives a Kindle overwrite and
/// a push can rebuild it exactly), plus `format`, `book_id` and `match`.
pub fn incoming(row: &AnnotationRow) -> Result<Incoming, Skip> {
    let data = row
        .data
        .as_ref()
        .map_err(|e| Skip::Unreadable(format!("annotation {} is not JSON: {e}", row.id)))?;
    let Value::Object(annotation) = data else {
        return Err(Skip::Unreadable(format!(
            "annotation {} is not a JSON object",
            row.id
        )));
    };
    if annotation.get("removed").and_then(Value::as_bool) == Some(true) {
        return Err(Skip::Removed);
    }
    let text_of = |k: &str| annotation.get(k).and_then(Value::as_str).map(String::from);
    let uuid = text_of("uuid")
        .ok_or_else(|| Skip::Unreadable(format!("annotation {} has no uuid", row.id)))?;
    let text = text_of("highlighted_text").filter(|t| !t.is_empty());
    if text.is_none() {
        return Err(Skip::Unreadable(format!(
            "annotation {} ({uuid}) has no highlighted text",
            row.id
        )));
    }
    let mut block = annotation.clone();
    block.insert("format".into(), Value::String(row.format.clone()));
    block.insert("book_id".into(), Value::from(row.book));
    block.insert(MATCH.into(), Value::String("native".into()));
    Ok(Incoming {
        ids: vec![uuid],
        kind: EntryType::Highlight,
        text,
        note: text_of("notes").filter(|n| !n.is_empty()),
        created: text_of("timestamp"),
        block: SourceBlock::Calibre(block),
    })
}

/// Store a freshly read calibre block. If the archive already anchored this
/// entry at the same place (`exact` or `manual`, same CFIs), it is our own
/// highlight read back: keep that match and its bookkeeping rather than
/// relabelling it `native`.
pub fn store_block(e: &mut Entry, mut block: Map<String, Value>) {
    if let Some(Value::Object(old)) = &e.calibre {
        let old_match = old.get(MATCH).and_then(Value::as_str);
        let same_place = ["start_cfi", "end_cfi", "spine_name"]
            .iter()
            .all(|k| old.get(*k) == block.get(*k));
        if matches!(old_match, Some("exact" | "manual")) && same_place {
            for k in DERIVED_KEYS {
                match old.get(k) {
                    Some(v) => block.insert(k.into(), v.clone()),
                    None => block.remove(k),
                };
            }
        }
    }
    e.calibre = Some(Value::Object(block));
}
