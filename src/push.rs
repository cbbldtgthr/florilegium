//! `push-calibre`: write the archive's highlights into calibre's EPUBs, so
//! calibre's viewer and "Browse annotations" show them. The archive is the
//! source of truth; calibre is a reading interface onto it.
//!
//! Per EPUB book, the plan is the book's complete list of viewer annotations:
//! every annotation that is not the archive's is kept untouched, ours are
//! replaced or added. calibre's merge would keep the older text on a timestamp
//! tie, so the whole list is set instead. A book whose list would not change
//! is not written, which makes pushing twice a no-op.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::Command;

use jiff::Timestamp;
use serde_json::{Map, Value, json};

use crate::archive::{Archive, Entry, EntryType};
use crate::calibre::WRITABLE_FORMAT;
use crate::calibre::db::{Library, VIEWER_USER, VIEWER_USER_TYPE};

/// Keys the tool keeps on an anchor that are not calibre's.
const TOOL_KEYS: [&str; 7] = [
    "match",
    "reason",
    "candidates",
    "occurrences",
    "similarity",
    "derived_with",
    "book_id",
];

const SCRIPT: &str = include_str!("calibre/push.py");

#[derive(Debug, Clone, Default)]
pub struct PushOptions {
    pub include_approximate: bool,
    pub include_ambiguous: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PlannedAnnotation {
    pub annotation: Value,
    /// Seconds since the epoch, for calibre's `timestamp` column.
    pub timestamp: f64,
    /// Whether the archive anchor already records the timestamp calibre holds
    /// (anchors pushed before timestamps were recorded do not).
    pub recorded: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct BookPlan {
    pub book_id: i64,
    pub title: String,
    pub file_name: String,
    /// The complete new list of viewer annotations for the EPUB.
    pub annotations: Vec<PlannedAnnotation>,
    pub added: usize,
    pub updated: usize,
    pub unchanged: usize,
    /// Annotations already in calibre that are not the archive's.
    pub kept: usize,
    /// Archive entry ID to the `timestamp` written for it, recorded in its
    /// EPUB anchor after the push.
    pub timestamps: BTreeMap<String, String>,
}

#[derive(Debug, Default)]
pub struct PushPlan {
    pub books: Vec<BookPlan>,
    /// Entries not pushed, by reason.
    pub skipped: BTreeMap<String, usize>,
    /// Books whose calibre list already matches the archive.
    pub up_to_date: usize,
}

/// Work out what pushing would write, reading calibre only.
pub fn plan(library: &Path, archive: &Archive, options: &PushOptions) -> Result<PushPlan, String> {
    let lib = Library::open(library)?;
    let calibre_books = lib.books()?;
    let rows = lib.annotations(&[])?;
    let mut out = PushPlan::default();
    let skip = |out: &mut PushPlan, reason: &str, n: usize| {
        if n > 0 {
            *out.skipped.entry(reason.to_string()).or_default() += n;
        }
    };

    for loaded in &archive.books {
        let book = &loaded.data;
        let live: Vec<&Entry> = book.entries.iter().filter(|e| !e.deleted).collect();
        let Some(uuid) = &book.book.calibre_uuid else {
            skip(&mut out, "book not linked to calibre", live.len());
            continue;
        };
        let Some(c) = calibre_books
            .values()
            .find(|b| b.uuid.as_ref() == Some(uuid))
        else {
            skip(
                &mut out,
                "calibre book no longer in the library",
                live.len(),
            );
            continue;
        };
        if !c.formats.contains_key(WRITABLE_FORMAT) {
            skip(&mut out, "calibre book has no EPUB", live.len());
            continue;
        }

        let mut ours: BTreeMap<String, PlannedAnnotation> = BTreeMap::new();
        for e in &live {
            match eligible(e, options) {
                Ok(()) => match annotation(e) {
                    Ok(a) => {
                        ours.insert(e.id.clone(), a);
                    }
                    Err(reason) => skip(&mut out, reason, 1),
                },
                Err(reason) => skip(&mut out, reason, 1),
            }
        }
        if ours.is_empty() {
            continue;
        }

        let existing: Vec<(Value, f64)> = rows
            .iter()
            .filter(|r| {
                r.book == c.id
                    && r.format == WRITABLE_FORMAT
                    && r.user_type == VIEWER_USER_TYPE
                    && r.user == VIEWER_USER
            })
            .filter_map(|r| r.data.as_ref().ok().map(|d| (d.clone(), r.timestamp)))
            .collect();
        let mut plan = BookPlan {
            book_id: c.id,
            title: c.title.clone(),
            file_name: loaded.file_name.clone(),
            annotations: Vec::new(),
            added: 0,
            updated: 0,
            unchanged: 0,
            kept: 0,
            timestamps: BTreeMap::new(),
        };
        let mut placed: BTreeSet<String> = BTreeSet::new();
        for (data, timestamp) in &existing {
            let id = data.get("uuid").and_then(Value::as_str).unwrap_or("");
            let is_highlight = data.get("type").and_then(Value::as_str) == Some("highlight");
            match ours.get(id) {
                Some(mine) if is_highlight && placed.insert(id.to_string()) => {
                    let mut mine = mine.clone();
                    if mine.recorded && mine.annotation == *data {
                        plan.unchanged += 1;
                    } else {
                        // calibre's viewer and its syncs keep whichever copy
                        // has the newer timestamp, so a change must carry a
                        // newer one or a cached copy wins.
                        bump(&mut mine, *timestamp);
                        plan.updated += 1;
                    }
                    plan.timestamps
                        .insert(id.to_string(), timestamp_text(&mine));
                    plan.annotations.push(mine);
                }
                _ => {
                    plan.kept += 1;
                    plan.annotations.push(PlannedAnnotation {
                        annotation: data.clone(),
                        timestamp: *timestamp,
                        recorded: true,
                    });
                }
            }
        }
        let mut new: Vec<&PlannedAnnotation> = ours
            .iter()
            .filter(|(id, _)| !placed.contains(*id))
            .map(|(_, a)| a)
            .collect();
        new.sort_by(|a, b| a.timestamp.total_cmp(&b.timestamp));
        plan.added = new.len();
        for a in new {
            plan.timestamps.insert(
                a.annotation["uuid"].as_str().unwrap_or("").to_string(),
                timestamp_text(a),
            );
            plan.annotations.push(a.clone());
        }

        if plan.added + plan.updated == 0 {
            out.up_to_date += 1;
        } else {
            out.books.push(plan);
        }
    }
    Ok(out)
}

fn eligible(e: &Entry, options: &PushOptions) -> Result<(), &'static str> {
    if e.kind != EntryType::Highlight {
        return Err("orphan note");
    }
    let Some(anchor) = e.calibre.get(WRITABLE_FORMAT) else {
        return Err("no EPUB anchor yet (run derive-calibre)");
    };
    match anchor.get("match").and_then(Value::as_str) {
        Some("exact" | "manual" | "native") => Ok(()),
        Some("approximate") if options.include_approximate => Ok(()),
        Some("ambiguous") if options.include_ambiguous => Ok(()),
        Some("approximate") => Err("approximate (pass --include approximate)"),
        Some("ambiguous") => Err("ambiguous (pass --include ambiguous)"),
        _ => Err("unmatched"),
    }
}

/// calibre's annotation for an entry: the EPUB anchor's calibre fields, with
/// identity and content from the entry, which is the source of truth.
fn annotation(e: &Entry) -> Result<PlannedAnnotation, &'static str> {
    let Some(created) = &e.created else {
        return Err("no date (calibre needs one)");
    };
    let Some(text) = &e.text else {
        return Err("no text");
    };
    let Some(Value::Object(anchor)) = e.calibre.get(WRITABLE_FORMAT) else {
        return Err("no EPUB anchor yet");
    };
    // calibre's timestamp is when the annotation last changed: the anchor's,
    // once a push or read has recorded one, else the entry's date.
    let recorded = anchor
        .get("timestamp")
        .and_then(Value::as_str)
        .map(String::from);
    let stamp = recorded.clone().unwrap_or_else(|| created.clone());
    let timestamp: Timestamp = stamp.parse().map_err(|_| "unreadable date")?;
    let mut a: Map<String, Value> = anchor.clone();
    for k in TOOL_KEYS {
        a.remove(k);
    }
    a.insert("type".into(), json!("highlight"));
    a.insert("uuid".into(), json!(e.id));
    a.insert("timestamp".into(), json!(stamp));
    a.insert("highlighted_text".into(), json!(text));
    match &e.note {
        Some(n) => a.insert("notes".into(), json!(n)),
        None => a.remove("notes"),
    };
    Ok(PlannedAnnotation {
        annotation: Value::Object(a),
        timestamp: timestamp.as_millisecond() as f64 / 1000.0,
        recorded: recorded.is_some(),
    })
}

/// Give a changed annotation a timestamp newer than the copy calibre holds
/// (whose timestamp column is `existing`, in seconds): now, or a millisecond
/// later than that copy if the clock says otherwise.
fn bump(a: &mut PlannedAnnotation, existing: f64) {
    let after = Timestamp::from_millisecond((existing * 1000.0).round() as i64 + 1)
        .unwrap_or(Timestamp::now());
    let now = Timestamp::now();
    let t = if now > after { now } else { after };
    let t = Timestamp::from_millisecond(t.as_millisecond()).unwrap_or(t);
    a.annotation["timestamp"] = json!(crate::archive::format_timestamp(t));
    a.timestamp = t.as_millisecond() as f64 / 1000.0;
}

fn timestamp_text(a: &PlannedAnnotation) -> String {
    a.annotation["timestamp"].as_str().unwrap_or("").to_string()
}

#[derive(Debug)]
pub struct PushReport {
    pub plan: PushPlan,
    pub backup: Option<PathBuf>,
}

/// Plan, then (unless `dry_run`) back up `metadata.db`, apply the plan through
/// calibre's own code, and record in the archive that the pushed entries now
/// live in calibre too.
pub fn push(
    library: &Path,
    archive_dir: &Path,
    options: &PushOptions,
    calibre_debug: &Path,
    dry_run: bool,
) -> Result<PushReport, String> {
    let mut archive = Archive::load(archive_dir)?;
    let plan = plan(library, &archive, options)?;
    if dry_run || plan.books.is_empty() {
        return Ok(PushReport { plan, backup: None });
    }
    let backup = backup(library)?;

    let work = tempfile_dir()?;
    let script = work.join("push.py");
    let plan_file = work.join("plan.json");
    std::fs::write(&script, SCRIPT)
        .map_err(|e| format!("cannot write {}: {e}", script.display()))?;
    let payload = json!({
        "dbpath": library.join("metadata.db"),
        "books": plan.books.iter().map(|b| json!({
            "book_id": b.book_id,
            "format": WRITABLE_FORMAT,
            "annotations": b.annotations.iter().map(|a| json!({"annotation": a.annotation, "timestamp": a.timestamp})).collect::<Vec<_>>(),
        })).collect::<Vec<_>>(),
    });
    std::fs::write(&plan_file, payload.to_string())
        .map_err(|e| format!("cannot write {}: {e}", plan_file.display()))?;
    let output = Command::new(calibre_debug)
        .arg("-e")
        .arg(&script)
        .arg("--")
        .arg(&plan_file)
        .output()
        .map_err(|e| format!("cannot run {}: {e}", calibre_debug.display()))?;
    let _ = std::fs::remove_dir_all(&work);
    if !output.status.success() {
        return Err(format!(
            "calibre-debug failed, nothing recorded in the archive (metadata.db backup: {}):\n{}",
            backup.display(),
            String::from_utf8_lossy(&output.stderr)
        ));
    }

    // Record the push's effect, so reading these back changes nothing: the
    // entry now lives in calibre, and its anchor holds the timestamp calibre
    // holds.
    let written: BTreeMap<&str, &str> = plan
        .books
        .iter()
        .flat_map(|b| b.timestamps.iter().map(|(id, t)| (id.as_str(), t.as_str())))
        .collect();
    for b in &mut archive.books {
        for e in &mut b.data.entries {
            let Some(t) = written.get(e.id.as_str()) else {
                continue;
            };
            if !e.sources.iter().any(|s| s == "calibre") {
                e.sources.push("calibre".into());
                e.sources.sort();
            }
            if let Some(Value::Object(anchor)) = e.calibre.get_mut(WRITABLE_FORMAT) {
                anchor.insert("timestamp".into(), json!(t));
            }
        }
    }
    archive.save(false)?;
    Ok(PushReport {
        plan,
        backup: Some(backup),
    })
}

/// A timestamped copy of `metadata.db` beside it, made before any write.
fn backup(library: &Path) -> Result<PathBuf, String> {
    let stamp = Timestamp::now().strftime("%Y%m%dT%H%M%SZ").to_string();
    let to = library.join(format!("metadata.db.florilegium-{stamp}.bak"));
    std::fs::copy(library.join("metadata.db"), &to)
        .map_err(|e| format!("cannot back up metadata.db to {}: {e}", to.display()))?;
    Ok(to)
}

fn tempfile_dir() -> Result<PathBuf, String> {
    let dir = std::env::temp_dir().join(format!("florilegium-push-{}", std::process::id()));
    std::fs::create_dir_all(&dir).map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
    Ok(dir)
}

/// calibre's GUI and viewer keep their own state and rewrite a book's whole
/// annotation list when they save, so they must not run during a push.
/// Returns the names of any that are running, where `pgrep` is available.
pub fn running_calibre_processes() -> Vec<String> {
    ["calibre", "ebook-viewer", "calibre-parallel"]
        .iter()
        .filter(|name| {
            Command::new("pgrep")
                .arg("-x")
                .arg(name)
                .output()
                .is_ok_and(|o| o.status.success())
        })
        .map(|n| n.to_string())
        .collect()
}

/// Where `calibre-debug` is: the configured path, macOS's standard install,
/// or the PATH.
pub fn calibre_debug(configured: Option<&Path>) -> PathBuf {
    if let Some(p) = configured {
        return p.to_path_buf();
    }
    let mac = Path::new("/Applications/calibre.app/Contents/MacOS/calibre-debug");
    if mac.is_file() {
        mac.to_path_buf()
    } else {
        PathBuf::from("calibre-debug")
    }
}
