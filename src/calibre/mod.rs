//! The calibre source: highlights made in calibre's viewer.

pub mod db;

use std::collections::BTreeMap;

use serde_json::{Map, Value};

use crate::archive::{Entry, EntryType};
use crate::merge::{Incoming, SourceBlock};
use db::AnnotationRow;

/// The only format the tool ever writes highlights to (or derives CFIs for).
/// Highlights on other formats are read and archived, never written back.
pub const WRITABLE_FORMAT: &str = "EPUB";

/// Key the tool adds to an anchor beside calibre's own annotation fields.
const MATCH: &str = "match";

/// Why a viewer highlight row did not become an incoming entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Skip {
    Removed,
    Unreadable(String),
}

/// A viewer highlight row as an incoming entry with one anchor, under the
/// row's format. The anchor holds the annotation verbatim (so calibre's own
/// text survives a Kindle overwrite and a push can rebuild it exactly), plus
/// `book_id` and `match`.
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
    block.insert("book_id".into(), Value::from(row.book));
    block.insert(MATCH.into(), Value::String("native".into()));
    Ok(Incoming {
        ids: vec![uuid],
        kind: EntryType::Highlight,
        text,
        note: text_of("notes").filter(|n| !n.is_empty()),
        created: text_of("timestamp"),
        block: SourceBlock::Calibre(BTreeMap::from([(row.format.clone(), block)])),
    })
}

/// One incoming entry from all of a highlight's annotations: the same ID on
/// several formats of a book (e.g. read on AZW3, pushed to EPUB) is one
/// passage. Every format's anchor is kept; text, note and date come from the
/// most recently changed annotation, the writable format winning a tie.
pub fn combine(mut parts: Vec<Incoming>) -> Incoming {
    let rank = |i: &Incoming| {
        let writable = match &i.block {
            SourceBlock::Calibre(a) => a.contains_key(WRITABLE_FORMAT),
            SourceBlock::Kindle(_) => false,
        };
        (i.created.clone(), writable)
    };
    parts.sort_by_key(|i| std::cmp::Reverse(rank(i)));
    let mut anchors = BTreeMap::new();
    for p in &parts {
        if let SourceBlock::Calibre(a) = &p.block {
            for (format, anchor) in a {
                anchors
                    .entry(format.clone())
                    .or_insert_with(|| anchor.clone());
            }
        }
    }
    let mut best = parts.swap_remove(0);
    best.block = SourceBlock::Calibre(anchors);
    best
}

/// Store a freshly read anchor for `format`. If the archive already anchored
/// the entry there at the same place (`exact` or `manual`, same CFIs), it is
/// our own highlight read back: our anchor stays exactly as it was, so a push
/// followed by a read changes nothing.
pub fn store_anchor(e: &mut Entry, format: &str, block: Map<String, Value>) {
    if let Some(Value::Object(old)) = e.calibre.get(format) {
        let ours = matches!(
            old.get(MATCH).and_then(Value::as_str),
            Some("exact" | "manual")
        );
        let same_place = ["start_cfi", "end_cfi", "spine_name"]
            .iter()
            .all(|k| old.get(*k) == block.get(*k));
        if ours && same_place {
            return;
        }
    }
    e.calibre.insert(format.to_string(), Value::Object(block));
}
