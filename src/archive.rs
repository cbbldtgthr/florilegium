//! The archive: a directory of per-book JSON files, the source of truth.
//!
//! Every file is written canonically (sorted keys, two-space indent, UTF-8,
//! LF, one trailing newline; entries, aliases and sources in a fixed order),
//! and only when its bytes change, so re-running a command with no new input
//! leaves `git status` clean.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use jiff::Timestamp;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use uuid::Uuid;

pub const SCHEMA_VERSION: u32 = 1;

/// User-edited, not a book file.
pub const OVERRIDES_FILE: &str = "book-overrides.json";

/// Namespace for the short ID in a book file's name. Never change it: names
/// are assigned once, but a changed namespace would rename every new file.
const FILE_NAMESPACE: Uuid = Uuid::from_u128(0x052b9797_b328_4724_85ff_b79801282e54);

const SLUG_MAX_CHARS: usize = 60;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BookFile {
    pub schema_version: u32,
    pub book: Book,
    pub entries: Vec<Entry>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Book {
    pub title: String,
    pub authors: Vec<String>,
    /// Raw Kindle title lines seen for this book; clippings join the file
    /// whose title lines share their `key_v1`.
    pub kindle_titles: Vec<String>,
    /// calibre's book UUID, stable across library rebuilds; null until matched.
    pub calibre_uuid: Option<String>,
    /// Fields this version does not know, kept so a rewrite never drops them.
    #[serde(flatten)]
    pub other: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EntryType {
    Highlight,
    Note,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Entry {
    pub id: String,
    pub aliases: Vec<String>,
    #[serde(rename = "type")]
    pub kind: EntryType,
    /// The highlighted text as received; null for an orphan note.
    pub text: Option<String>,
    pub note: Option<String>,
    /// UTC, in calibre's timestamp format (see `format_timestamp`).
    pub created: Option<String>,
    /// A tombstone, set by hand.
    pub deleted: bool,
    /// Sources this entry has been seen in, e.g. `kindle`, `calibre`.
    pub sources: Vec<String>,
    pub kindle: Option<KindleBlock>,
    /// Derived or native calibre data; typed when the calibre side lands.
    pub calibre: Option<Value>,
    /// Fields this version does not know (a future source's block, a field the
    /// user added), kept so a rewrite never drops them.
    #[serde(flatten)]
    pub other: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct KindleBlock {
    pub location: Option<[u32; 2]>,
    pub page: Option<[u32; 2]>,
    pub raw_added_on: String,
}

/// calibre's annotation timestamp text, e.g. `2026-04-12T03:43:39.157Z`.
pub fn format_timestamp(t: Timestamp) -> String {
    t.strftime("%Y-%m-%dT%H:%M:%S%.3fZ").to_string()
}

impl BookFile {
    pub fn new(book: Book) -> BookFile {
        BookFile {
            schema_version: SCHEMA_VERSION,
            book,
            entries: Vec::new(),
        }
    }

    /// Put every list in its canonical order.
    pub fn canonicalize(&mut self) {
        sort_dedup(&mut self.book.kindle_titles);
        for e in &mut self.entries {
            sort_dedup(&mut e.aliases);
            sort_dedup(&mut e.sources);
        }
        // Created ascending with nulls last, then ID: stable as derived data
        // changes. Exporters sort by position for display.
        self.entries.sort_by(|a, b| {
            (a.created.is_none(), &a.created, &a.id).cmp(&(b.created.is_none(), &b.created, &b.id))
        });
    }

    /// The canonical bytes of this file.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut canonical = self.clone();
        canonical.canonicalize();
        let value =
            sort_keys(serde_json::to_value(&canonical).expect("archive types always serialise"));
        let mut out = serde_json::to_string_pretty(&value).expect("values always serialise");
        out.push('\n');
        out.into_bytes()
    }
}

fn sort_dedup(v: &mut Vec<String>) {
    v.sort();
    v.dedup();
}

/// Rebuild every object with its keys inserted in sorted order, so output is
/// sorted whether or not serde_json preserves insertion order.
fn sort_keys(v: Value) -> Value {
    match v {
        Value::Object(map) => {
            let mut entries: Vec<(String, Value)> = map.into_iter().collect();
            entries.sort_by(|a, b| a.0.cmp(&b.0));
            Value::Object(
                entries
                    .into_iter()
                    .map(|(k, v)| (k, sort_keys(v)))
                    .collect::<Map<_, _>>(),
            )
        }
        Value::Array(items) => Value::Array(items.into_iter().map(sort_keys).collect()),
        other => other,
    }
}

/// A book file as loaded: its data and, for files already on disk, the bytes
/// they held, so unchanged files are never rewritten.
#[derive(Debug, Clone)]
pub struct LoadedBook {
    pub file_name: String,
    pub data: BookFile,
    on_disk: Option<Vec<u8>>,
}

impl LoadedBook {
    /// Whether the file on disk already holds exactly its canonical bytes.
    pub fn is_canonical_on_disk(&self) -> bool {
        self.on_disk.as_deref() == Some(self.data.to_bytes().as_slice())
    }
}

#[derive(Debug)]
pub struct Archive {
    pub dir: PathBuf,
    pub books: Vec<LoadedBook>,
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct SaveReport {
    pub created: Vec<String>,
    pub updated: Vec<String>,
    pub unchanged: usize,
}

impl Archive {
    /// Load every book file in `dir`. Fails on any file that does not parse,
    /// naming it, rather than skipping it.
    pub fn load(dir: &Path) -> Result<Archive, String> {
        let mut books = Vec::new();
        let entries =
            fs::read_dir(dir).map_err(|e| format!("cannot read archive {}: {e}", dir.display()))?;
        let mut names: Vec<String> = Vec::new();
        for entry in entries {
            let entry = entry.map_err(|e| format!("cannot read archive {}: {e}", dir.display()))?;
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.ends_with(".json") && name != OVERRIDES_FILE && entry.path().is_file() {
                names.push(name);
            }
        }
        names.sort();
        for name in names {
            let path = dir.join(&name);
            let bytes =
                fs::read(&path).map_err(|e| format!("cannot read {}: {e}", path.display()))?;
            let data: BookFile = serde_json::from_slice(&bytes)
                .map_err(|e| format!("{} is not a valid book file: {e}", path.display()))?;
            if data.schema_version != SCHEMA_VERSION {
                return Err(format!(
                    "{} has schema_version {}, this tool reads {SCHEMA_VERSION}",
                    path.display(),
                    data.schema_version
                ));
            }
            books.push(LoadedBook {
                file_name: name,
                data,
                on_disk: Some(bytes),
            });
        }
        Ok(Archive {
            dir: dir.to_path_buf(),
            books,
        })
    }

    /// Add a book, assigning its file name once. `name_key` should identify
    /// the book where it was first seen (e.g. its Kindle title key).
    pub fn add_book(&mut self, book: Book, name_key: &str) -> &mut LoadedBook {
        let file_name = self.unused_file_name(&book, name_key);
        self.books.push(LoadedBook {
            file_name,
            data: BookFile::new(book),
            on_disk: None,
        });
        self.books.last_mut().unwrap()
    }

    fn unused_file_name(&self, book: &Book, name_key: &str) -> String {
        let slug = slug(&format!("{} {}", book.title, book.authors.join(" ")));
        let hex = Uuid::new_v5(&FILE_NAMESPACE, name_key.as_bytes())
            .simple()
            .to_string();
        // Names compare case-insensitively on macOS, so the ID is lowercase hex.
        let taken = |n: &str| {
            self.books
                .iter()
                .any(|b| b.file_name.eq_ignore_ascii_case(n))
                || self.dir.join(n).exists()
        };
        (8..=hex.len())
            .map(|len| format!("{slug}--{}.json", &hex[..len]))
            .find(|n| !taken(n))
            .expect("a full uuid never collides")
    }

    /// Write every new or changed book file; leave the rest untouched.
    pub fn save(&mut self, dry_run: bool) -> Result<SaveReport, String> {
        let mut report = SaveReport::default();
        for b in &mut self.books {
            let bytes = b.data.to_bytes();
            match &b.on_disk {
                Some(old) if *old == bytes => {
                    report.unchanged += 1;
                    continue;
                }
                Some(_) => report.updated.push(b.file_name.clone()),
                None => report.created.push(b.file_name.clone()),
            }
            if !dry_run {
                write_atomically(&self.dir.join(&b.file_name), &bytes)?;
                b.on_disk = Some(bytes);
            }
        }
        Ok(report)
    }
}

fn write_atomically(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, bytes).map_err(|e| format!("cannot write {}: {e}", tmp.display()))?;
    fs::rename(&tmp, path).map_err(|e| format!("cannot replace {}: {e}", path.display()))
}

/// Lowercase ASCII letters and digits joined by single hyphens; accents are
/// dropped, other characters become separators.
fn slug(s: &str) -> String {
    use unicode_normalization::UnicodeNormalization;
    use unicode_normalization::char::is_combining_mark;
    let mut out = String::new();
    for c in crate::norm::norm_v1(s)
        .nfd()
        .filter(|c| !is_combining_mark(*c))
    {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
        } else if !out.is_empty() && !out.ends_with('-') {
            out.push('-');
        }
        if out.len() >= SLUG_MAX_CHARS {
            break;
        }
    }
    let out = out.trim_end_matches('-');
    if out.is_empty() {
        "book".to_string()
    } else {
        out.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slugs() {
        assert_eq!(
            slug("The Lantern Keeper\u{2019}s Almanac Vell, Orrin"),
            "the-lantern-keeper-s-almanac-vell-orrin"
        );
        assert_eq!(slug("  ***  "), "book");
        assert_eq!(slug("Caf\u{00E9} Na\u{00EF}ve"), "cafe-naive");
        assert!(slug(&"word ".repeat(40)).len() <= SLUG_MAX_CHARS);
        assert!(!slug(&"word ".repeat(40)).ends_with('-'));
    }

    #[test]
    fn timestamps_use_calibre_format() {
        let t: Timestamp = "2026-10-23T13:50:12Z".parse().unwrap();
        assert_eq!(format_timestamp(t), "2026-10-23T13:50:12.000Z");
        let t: Timestamp = "2026-04-12T03:43:39.157Z".parse().unwrap();
        assert_eq!(format_timestamp(t), "2026-04-12T03:43:39.157Z");
    }
}
