use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Parser, Subcommand};

use annot_archive::check::check;
use annot_archive::config::Config;
use annot_archive::id::{EntryKind, key_v1};
use annot_archive::ingest::{IngestReport, ingest_clippings};
use annot_archive::kindle::parse::{ParseConfig, RawClipping, parse};
use annot_archive::kindle::replay::{ReplayConfig, replay};
use annot_archive::resolve::{Lookup, Resolved, resolve};

#[derive(Parser)]
#[command(version, about = "A permanent, citable archive of reading highlights")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Parse a My Clippings.txt and summarise what it holds, without writing anything
    InspectClippings {
        /// Path to My Clippings.txt
        clippings: PathBuf,
        /// List the entries of books whose title line contains this text
        #[arg(long)]
        book: Option<String>,
    },
    /// Replay a My Clippings.txt and merge it into the archive by identity
    IngestClippings {
        /// Path to My Clippings.txt
        clippings: PathBuf,
        /// Archive directory: a folder of its own, in a private repository
        #[arg(long)]
        archive: PathBuf,
        /// Settings file; defaults to config.toml in the archive, if present
        #[arg(long)]
        config: Option<PathBuf>,
        /// Report what would change without writing anything
        #[arg(long)]
        dry_run: bool,
    },
    /// Validate the archive; exits 1 if anything is wrong
    Check {
        /// Archive directory
        #[arg(long)]
        archive: PathBuf,
    },
    /// Look up an entry by ID or alias. Exits 0 if found, 2 if it was deleted,
    /// 1 if unknown
    Resolve {
        /// An entry ID or any alias it has had
        id: String,
        /// Archive directory
        #[arg(long)]
        archive: PathBuf,
        /// Print the result as JSON
        #[arg(long)]
        json: bool,
        /// Also print a deleted entry's content
        #[arg(long)]
        show_deleted: bool,
    },
}

fn main() -> ExitCode {
    match Cli::parse().command {
        Command::InspectClippings { clippings, book } => {
            inspect_clippings(&clippings, book.as_deref())
        }
        Command::IngestClippings {
            clippings,
            archive,
            config,
            dry_run,
        } => run_ingest(&clippings, &archive, config.as_deref(), dry_run),
        Command::Check { archive } => run_check(&archive),
        Command::Resolve {
            id,
            archive,
            json,
            show_deleted,
        } => run_resolve(&id, &archive, json, show_deleted),
    }
}

fn run_check(archive: &Path) -> ExitCode {
    let issues = check(archive);
    if issues.is_empty() {
        println!("{}: no problems found", archive.display());
        return ExitCode::SUCCESS;
    }
    for i in &issues {
        match &i.id {
            Some(id) => println!("{}: {id}: {}", i.file, i.message),
            None => println!("{}: {}", i.file, i.message),
        }
    }
    println!("{} problem(s) found", issues.len());
    ExitCode::FAILURE
}

fn run_resolve(id: &str, archive: &Path, json: bool, show_deleted: bool) -> ExitCode {
    let lookup = match resolve(archive, id) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::FAILURE;
        }
    };
    match lookup {
        Lookup::Found(r) => {
            print_resolved(&r, json, true);
            ExitCode::SUCCESS
        }
        Lookup::Deleted(r) => {
            if json {
                print_resolved(&r, true, show_deleted);
            } else {
                println!(
                    "{id} resolves to {}, which was deleted ({})",
                    r.canonical_id(),
                    r.file_name
                );
                if show_deleted {
                    print_resolved(&r, false, true);
                }
            }
            ExitCode::from(2)
        }
        Lookup::NotFound => {
            eprintln!("{id}: no entry has this ID or alias");
            ExitCode::FAILURE
        }
        Lookup::Ambiguous(holders) => {
            eprintln!(
                "{id} is claimed by several entries (run check): {}",
                holders.join("; ")
            );
            ExitCode::FAILURE
        }
    }
}

fn print_resolved(r: &Resolved, json: bool, with_content: bool) {
    if json {
        let mut out = serde_json::json!({
            "queried": r.queried,
            "id": r.canonical_id(),
            "deleted": r.entry.deleted,
            "file": r.file_name,
            "book": {"title": r.book.title, "authors": r.book.authors, "calibre_uuid": r.book.calibre_uuid},
        });
        if with_content {
            out["entry"] = serde_json::to_value(&r.entry).expect("entries serialise");
        }
        println!(
            "{}",
            serde_json::to_string_pretty(&out).expect("values serialise")
        );
        return;
    }
    let alias = if r.via_alias() {
        format!(" (alias {})", r.queried)
    } else {
        String::new()
    };
    println!("{}{alias}", r.canonical_id());
    let authors = if r.book.authors.is_empty() {
        String::new()
    } else {
        format!(" by {}", r.book.authors.join("; "))
    };
    println!("{}{authors}  [{}]", r.book.title, r.file_name);
    let e = &r.entry;
    let place = e.kindle.as_ref().and_then(|k| {
        k.location
            .map(|[a, b]| format!("loc {a}-{b}"))
            .or(k.page.map(|[a, b]| format!("p {a}-{b}")))
    });
    println!(
        "{}  {}",
        place.unwrap_or_default(),
        e.created.as_deref().unwrap_or("no date")
    );
    if let Some(t) = &e.text {
        println!("\n{t}");
    }
    if let Some(n) = &e.note {
        println!("\nNote: {n}");
    }
}

fn run_ingest(clippings: &Path, archive: &Path, config: Option<&Path>, dry_run: bool) -> ExitCode {
    let result = Config::load(config, archive).and_then(|config| {
        let input = std::fs::read_to_string(clippings)
            .map_err(|e| format!("cannot read {}: {e}", clippings.display()))?;
        ingest_clippings(&input, archive, &config, dry_run)
    });
    match result {
        Ok(report) => {
            print_ingest(&report, dry_run);
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("{e}");
            ExitCode::FAILURE
        }
    }
}

fn print_ingest(r: &IngestReport, dry_run: bool) {
    let sum =
        |f: fn(&annot_archive::ingest::BookReport) -> usize| r.books.iter().map(f).sum::<usize>();
    let merges: Vec<_> = r
        .books
        .iter()
        .flat_map(|b| b.merges.iter().map(move |m| (b, m)))
        .collect();
    println!(
        "{} books: {} entries created, {} updated, {} unchanged, {} merged, {} matched tombstones",
        r.books.len(),
        sum(|b| b.created),
        sum(|b| b.updated),
        sum(|b| b.unchanged),
        merges.len(),
        sum(|b| b.tombstone_matched),
    );
    let verb = if dry_run { "would write" } else { "wrote" };
    println!(
        "files: {verb} {} new, {} changed; {} unchanged",
        r.saved.created.len(),
        r.saved.updated.len(),
        r.saved.unchanged
    );

    let changed: Vec<_> = r.books.iter().filter(|b| b.changed()).collect();
    if !changed.is_empty() {
        println!("\nChanged books:");
        for b in changed {
            let new = if b.new_file { " (new file)" } else { "" };
            println!(
                "  +{} ~{} ={} merged {} tombstone {}  {}{new}",
                b.created,
                b.updated,
                b.unchanged,
                b.merges.len(),
                b.tombstone_matched,
                b.file_name
            );
        }
    }

    if !merges.is_empty() {
        println!("\n!! Entries merged because a clipping joined them (review these):");
        for (b, (id, absorbed)) in merges {
            println!("  {id} absorbed {} in {}", absorbed.join(", "), b.file_name);
        }
    }

    let p = &r.parsed;
    let warnings =
        p.warnings.len() + r.orphan_notes.len() + r.joined_notes.len() + r.warnings.len();
    if warnings > 0 {
        println!("\nWarnings ({warnings}):");
        for w in &p.warnings {
            println!("  clippings line {}: {}", w.line, w.message);
        }
        for (title, line, text) in &r.orphan_notes {
            let text: String = text.chars().take(60).collect();
            println!("  clippings line {line}: orphan note {text:?} in {title}");
        }
        for (title, line) in &r.joined_notes {
            println!("  clippings line {line}: several notes joined on one highlight in {title}");
        }
        for w in &r.warnings {
            println!("  {w}");
        }
    }
    println!(
        "\n(skipped {} bookmarks, {} clipping-limit stubs)",
        p.bookmarks, p.stubs
    );
}

fn inspect_clippings(path: &PathBuf, book: Option<&str>) -> ExitCode {
    let input = match std::fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("cannot read {}: {e}", path.display());
            return ExitCode::FAILURE;
        }
    };
    let parsed = match parse(&input, &ParseConfig::default()) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::FAILURE;
        }
    };

    let notes = parsed
        .clippings
        .iter()
        .filter(|c| c.kind == EntryKind::Note)
        .count();
    println!(
        "{}: {} clippings ({} highlights, {} notes), {} bookmarks, {} stubs, {} warnings",
        path.display(),
        parsed.clippings.len(),
        parsed.clippings.len() - notes,
        notes,
        parsed.bookmarks,
        parsed.stubs,
        parsed.warnings.len(),
    );

    let mut raw: BTreeMap<String, Vec<&RawClipping>> = BTreeMap::new();
    for c in &parsed.clippings {
        raw.entry(key_v1(&c.title_line)).or_default().push(c);
    }
    let books = replay(&parsed.clippings, &ReplayConfig::default());

    let highlights = parsed.clippings.len() - notes;
    let entries = books
        .iter()
        .flat_map(|b| &b.clusters)
        .filter(|c| c.kind == EntryKind::Highlight)
        .count();
    let attached = books
        .iter()
        .flat_map(|b| &b.clusters)
        .filter(|c| c.note.is_some())
        .count();
    let orphans: Vec<_> = books
        .iter()
        .flat_map(|b| b.clusters.iter().map(move |c| (b, c)))
        .filter(|(_, c)| c.kind == EntryKind::Note)
        .collect();
    println!(
        "replay: {highlights} highlights fold into {entries} entries; {attached} notes attached, {} orphan notes",
        orphans.len()
    );

    println!(
        "\nBooks ({}):\n  {:>5} {:>5} {:>7} {:>7}  {:10}  {:10}  TITLE",
        books.len(),
        "HL",
        "NOTES",
        "ENTRIES",
        "ORPHANS",
        "FIRST",
        "LAST"
    );
    for b in &books {
        let clips = &raw[&b.key];
        let notes = clips.iter().filter(|c| c.kind == EntryKind::Note).count();
        let orphans = b
            .clusters
            .iter()
            .filter(|c| c.kind == EntryKind::Note)
            .count();
        let dates: Vec<String> = clips
            .iter()
            .filter_map(|c| c.created)
            .map(|t| t.to_string()[..10].to_string())
            .collect();
        println!(
            "  {:>5} {:>5} {:>7} {:>7}  {:10}  {:10}  {}",
            clips.len() - notes,
            notes,
            b.clusters.len() - orphans,
            orphans,
            dates.iter().min().map_or("-", String::as_str),
            dates.iter().max().map_or("-", String::as_str),
            b.title_lines[0],
        );
    }

    if let Some(needle) = book {
        let needle = needle.to_lowercase();
        for b in books.iter().filter(|b| b.key.contains(&needle)) {
            println!("\n{}", b.title_lines.join(" | "));
            for c in &b.clusters {
                let members: Vec<String> = c.members.iter().map(|i| format!("#{i}")).collect();
                println!(
                    "  {} {:24} {}",
                    c.id,
                    members.join("+"),
                    describe(&c.latest)
                );
                if let Some(note) = &c.note {
                    let note: String = note.replace('\n', " / ").chars().take(90).collect();
                    println!("  {:47} note: {note}", "");
                }
            }
        }
    }

    if !orphans.is_empty() {
        println!("\nOrphan notes (no highlight contains their location):");
        for (b, c) in &orphans {
            let text: String = c
                .latest
                .text
                .replace('\n', " / ")
                .chars()
                .take(70)
                .collect();
            println!("  line {}: {} | {}", c.latest.line, text, b.title_lines[0]);
        }
    }

    if !parsed.warnings.is_empty() {
        println!("\nWarnings:");
        for w in &parsed.warnings {
            println!("  line {}: {}", w.line, w.message);
        }
    }
    ExitCode::SUCCESS
}

fn describe(c: &RawClipping) -> String {
    let range = |(a, b): (u32, u32)| {
        if a == b {
            a.to_string()
        } else {
            format!("{a}-{b}")
        }
    };
    let pos = [
        c.location.map(|l| format!("loc {}", range(l))),
        c.page.map(|p| format!("p {}", range(p))),
    ]
    .into_iter()
    .flatten()
    .collect::<Vec<_>>()
    .join(" ");
    let date = c.created.map_or("no date".to_string(), |t| {
        t.to_string()[..16].replace('T', " ")
    });
    let kind = if c.kind == EntryKind::Note {
        "NOTE "
    } else {
        ""
    };
    let text: String = c.text.replace('\n', " / ").chars().take(90).collect();
    format!("{:22} {}  {kind}{text}", pos, date)
}
