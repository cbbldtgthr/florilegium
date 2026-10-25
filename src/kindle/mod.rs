//! The Kindle source: `My Clippings.txt`.

pub mod parse;
pub mod replay;

use crate::archive::{EntryType, KindleBlock, format_timestamp};
use crate::id::EntryKind;
use crate::merge::{Incoming, SourceBlock};
use replay::Cluster;

/// A replayed cluster as a source-neutral incoming entry.
pub fn incoming(c: &Cluster) -> Incoming {
    let latest = &c.latest;
    let (kind, text, note) = match c.kind {
        EntryKind::Highlight => (
            EntryType::Highlight,
            Some(latest.text.clone()),
            c.note.clone(),
        ),
        // An orphan note's content is its own text; it highlights nothing.
        EntryKind::Note => (EntryType::Note, None, Some(latest.text.clone())),
    };
    Incoming {
        ids: c.member_ids.clone(),
        kind,
        text,
        note,
        created: latest.created.map(format_timestamp),
        block: SourceBlock::Kindle(KindleBlock {
            location: latest.location.map(|(a, b)| [a, b]),
            page: latest.page.map(|(a, b)| [a, b]),
            raw_added_on: latest.raw_added_on.clone(),
        }),
    }
}
