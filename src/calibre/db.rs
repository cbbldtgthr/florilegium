//! Read-only access to a calibre library's `metadata.db`.
//!
//! Opened read-only, so it is safe while calibre runs. Writing goes through
//! calibre's own code (see the push backlog item), never through here.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use rusqlite::{Connection, OpenFlags};
use serde_json::Value;

/// calibre's viewer stores its highlights under this user.
pub const VIEWER_USER_TYPE: &str = "local";
pub const VIEWER_USER: &str = "viewer";

#[derive(Debug, Clone, PartialEq)]
pub struct CalibreBook {
    pub id: i64,
    /// calibre's book UUID; the archive's cross-library book identity.
    pub uuid: Option<String>,
    pub title: String,
    pub authors: Vec<String>,
    /// Format (e.g. `EPUB`) to the file path relative to the library root.
    pub formats: BTreeMap<String, PathBuf>,
}

/// One row of the `annotations` table.
#[derive(Debug, Clone, PartialEq)]
pub struct AnnotationRow {
    pub id: i64,
    pub book: i64,
    pub format: String,
    pub user_type: String,
    pub user: String,
    pub annot_type: String,
    pub annot_id: String,
    /// `annot_data` parsed, or the parse error.
    pub data: Result<Value, String>,
    /// The row's `timestamp` column: seconds since the epoch, as calibre
    /// stores it beside the JSON.
    pub timestamp: f64,
}

pub struct Library {
    pub root: PathBuf,
    conn: Connection,
}

impl Library {
    pub fn open(root: &Path) -> Result<Library, String> {
        let db = root.join("metadata.db");
        if !db.is_file() {
            return Err(format!(
                "{} has no metadata.db; is it a calibre library?",
                root.display()
            ));
        }
        let conn = Connection::open_with_flags(
            &db,
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )
        .map_err(|e| format!("cannot open {}: {e}", db.display()))?;
        // calibre may be mid-write; wait for its lock rather than failing.
        conn.busy_timeout(Duration::from_secs(10))
            .map_err(|e| e.to_string())?;
        Ok(Library {
            root: root.to_path_buf(),
            conn,
        })
    }

    pub fn books(&self) -> Result<BTreeMap<i64, CalibreBook>, String> {
        let q = |e: rusqlite::Error| format!("cannot read calibre books: {e}");
        let mut books = BTreeMap::new();
        let mut stmt = self
            .conn
            .prepare("SELECT id, uuid, title, path FROM books")
            .map_err(q)?;
        let rows = stmt
            .query_map([], |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, Option<String>>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, String>(3)?,
                ))
            })
            .map_err(q)?;
        let mut paths = BTreeMap::new();
        for row in rows {
            let (id, uuid, title, path) = row.map_err(q)?;
            paths.insert(id, path);
            books.insert(
                id,
                CalibreBook {
                    id,
                    uuid,
                    title,
                    authors: Vec::new(),
                    formats: BTreeMap::new(),
                },
            );
        }

        let mut stmt = self
            .conn
            .prepare("SELECT l.book, a.name FROM books_authors_link l JOIN authors a ON a.id = l.author ORDER BY l.book, l.id")
            .map_err(q)?;
        for row in stmt
            .query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)))
            .map_err(q)?
        {
            let (book, name) = row.map_err(q)?;
            if let Some(b) = books.get_mut(&book) {
                b.authors.push(name);
            }
        }

        let mut stmt = self
            .conn
            .prepare("SELECT book, format, name FROM data")
            .map_err(q)?;
        for row in stmt
            .query_map([], |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                ))
            })
            .map_err(q)?
        {
            let (book, format, name) = row.map_err(q)?;
            if let (Some(b), Some(dir)) = (books.get_mut(&book), paths.get(&book)) {
                let file = PathBuf::from(dir).join(format!("{name}.{}", format.to_lowercase()));
                b.formats.insert(format.to_uppercase(), file);
            }
        }
        Ok(books)
    }

    /// Annotation rows, optionally only for some books, in row order.
    pub fn annotations(&self, only_books: &[i64]) -> Result<Vec<AnnotationRow>, String> {
        let q = |e: rusqlite::Error| format!("cannot read calibre annotations: {e}");
        let mut stmt = self
            .conn
            .prepare("SELECT id, book, format, user_type, user, annot_type, annot_id, annot_data, timestamp FROM annotations ORDER BY id")
            .map_err(q)?;
        let rows = stmt
            .query_map([], |r| {
                let data: String = r.get(7)?;
                Ok(AnnotationRow {
                    id: r.get(0)?,
                    book: r.get(1)?,
                    format: r.get::<_, String>(2)?.to_uppercase(),
                    user_type: r.get(3)?,
                    user: r.get(4)?,
                    annot_type: r.get(5)?,
                    annot_id: r.get(6)?,
                    data: serde_json::from_str(&data).map_err(|e| e.to_string()),
                    timestamp: r.get(8)?,
                })
            })
            .map_err(q)?;
        let mut out = Vec::new();
        for row in rows {
            let row = row.map_err(q)?;
            if only_books.is_empty() || only_books.contains(&row.book) {
                out.push(row);
            }
        }
        Ok(out)
    }
}
