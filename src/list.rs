//! `list`: every live entry on one line, for searching and picking an ID to
//! cite (`just find` feeds it to fzf). Tab-separated: ID, book, chapter,
//! text, note. Tabs and line breaks inside fields become spaces.

use std::path::Path;

use serde_json::Value;

use crate::archive::{Archive, Entry};
use crate::calibre::WRITABLE_FORMAT;

pub fn list(dir: &Path) -> Result<Vec<String>, String> {
    let archive = Archive::load(dir)?;
    let mut books: Vec<_> = archive.books.iter().collect();
    books.sort_by_key(|b| (b.data.book.title.to_lowercase(), b.file_name.clone()));
    let mut out = Vec::new();
    for b in books {
        for e in b.data.entries.iter().filter(|e| !e.deleted) {
            let fields = [
                e.id.as_str(),
                b.data.book.title.as_str(),
                &chapter(e),
                e.text.as_deref().unwrap_or(""),
                e.note.as_deref().unwrap_or(""),
            ];
            out.push(
                fields
                    .iter()
                    .map(|f| flat(f))
                    .collect::<Vec<_>>()
                    .join("\t"),
            );
        }
    }
    Ok(out)
}

/// The chapter titles calibre knows for the entry, outermost first,
/// preferring the EPUB anchor's.
pub fn chapter_titles(e: &Entry) -> Vec<String> {
    let mut anchors: Vec<(&String, &Value)> = e.calibre.iter().collect();
    anchors.sort_by_key(|(f, _)| f.as_str() != WRITABLE_FORMAT);
    anchors
        .into_iter()
        .find_map(|(_, a)| {
            let titles: Vec<String> = a
                .get("toc_family_titles")?
                .as_array()?
                .iter()
                .filter_map(|t| t.as_str().map(String::from))
                .collect();
            (!titles.is_empty()).then_some(titles)
        })
        .unwrap_or_default()
}

/// The innermost chapter title, or empty.
pub fn chapter(e: &Entry) -> String {
    chapter_titles(e).pop().unwrap_or_default()
}

fn flat(s: &str) -> String {
    s.split(['\t', '\n', '\r'])
        .filter(|p| !p.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}
