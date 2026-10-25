use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Parser, Subcommand};

use annot_archive::calibre::db::Library;
use annot_archive::check::check;
use annot_archive::config::Config;
use annot_archive::derive::books::How;
use annot_archive::derive::{BookOutcome, DeriveReport, derive_calibre};
use annot_archive::id::{EntryKind, key_v1};
use annot_archive::ingest::{
    BookReport, CalibreIngestReport, IngestReport, ingest_calibre, ingest_clippings,
};
use annot_archive::kindle::parse::{ParseConfig, RawClipping, parse};
use annot_archive::kindle::replay::{ReplayConfig, replay};
use annot_archive::merge_books::merge_books;
use annot_archive::push::{
    PushOptions, PushReport, calibre_debug, push, running_calibre_processes,
};
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
    /// Dump a calibre book's raw annotation rows as JSON, without writing anything
    InspectCalibre {
        /// calibre library root (the folder holding metadata.db)
        #[arg(long)]
        library: PathBuf,
        /// calibre's numeric book ID
        #[arg(long)]
        book: i64,
    },
    /// Read calibre's viewer highlights (EPUB) into the archive by identity
    IngestCalibre {
        /// Archive directory: a folder of its own, in a private repository
        #[arg(long)]
        archive: PathBuf,
        /// calibre library root; defaults to [calibre] library in config.toml
        #[arg(long)]
        library: Option<PathBuf>,
        /// Settings file; defaults to config.toml in the archive, if present
        #[arg(long)]
        config: Option<PathBuf>,
        /// Only these calibre book IDs (repeatable)
        #[arg(long)]
        book: Vec<i64>,
        /// Report what would change without writing anything
        #[arg(long)]
        dry_run: bool,
    },
    /// Work out where each Kindle highlight sits in calibre's EPUB and record
    /// it in the archive (derived data only; never touches native entries)
    DeriveCalibre {
        /// Archive directory
        #[arg(long)]
        archive: PathBuf,
        /// calibre library root; defaults to [calibre] library in config.toml
        #[arg(long)]
        library: Option<PathBuf>,
        /// Settings file; defaults to config.toml in the archive, if present
        #[arg(long)]
        config: Option<PathBuf>,
        /// Also recompute exact, approximate and ambiguous entries
        #[arg(long)]
        rederive: bool,
        /// Only these archive book files (repeatable)
        #[arg(long)]
        book: Vec<String>,
        /// Report what would change without writing anything
        #[arg(long)]
        dry_run: bool,
    },
    /// Write the archive's highlights into calibre's EPUBs (backs up
    /// metadata.db first; calibre and its viewer must be closed)
    PushCalibre {
        /// Archive directory
        #[arg(long)]
        archive: PathBuf,
        /// calibre library root; defaults to [calibre] library in config.toml
        #[arg(long)]
        library: Option<PathBuf>,
        /// Settings file; defaults to config.toml in the archive, if present
        #[arg(long)]
        config: Option<PathBuf>,
        /// Also push these match kinds: approximate, ambiguous
        #[arg(long, value_delimiter = ',')]
        include: Vec<String>,
        /// Confirm that calibre and its viewer are closed; required to write
        #[arg(long)]
        calibre_closed: bool,
        /// Show what would be written without writing anything
        #[arg(long)]
        dry_run: bool,
    },
    /// Join two book files that are one book: KEEP gains ABSORB's entries and
    /// keys, and ABSORB's file is deleted
    MergeBooks {
        /// Book file that survives (keeps its name, title and authors)
        keep: String,
        /// Book file whose entries and keys move into KEEP
        absorb: String,
        /// Archive directory
        #[arg(long)]
        archive: PathBuf,
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
        Command::InspectCalibre { library, book } => run_inspect_calibre(&library, book),
        Command::IngestCalibre {
            archive,
            library,
            config,
            book,
            dry_run,
        } => run_ingest_calibre(
            &archive,
            library.as_deref(),
            config.as_deref(),
            &book,
            dry_run,
        ),
        Command::DeriveCalibre {
            archive,
            library,
            config,
            rederive,
            book,
            dry_run,
        } => run_derive(
            &archive,
            library.as_deref(),
            config.as_deref(),
            rederive,
            &book,
            dry_run,
        ),
        Command::PushCalibre {
            archive,
            library,
            config,
            include,
            calibre_closed,
            dry_run,
        } => run_push(
            &archive,
            library.as_deref(),
            config.as_deref(),
            &include,
            calibre_closed,
            dry_run,
        ),
        Command::MergeBooks {
            keep,
            absorb,
            archive,
            dry_run,
        } => run_merge_books(&archive, &keep, &absorb, dry_run),
        Command::Check { archive } => run_check(&archive),
        Command::Resolve {
            id,
            archive,
            json,
            show_deleted,
        } => run_resolve(&id, &archive, json, show_deleted),
    }
}

fn run_inspect_calibre(library: &Path, book: i64) -> ExitCode {
    let rows = Library::open(library).and_then(|lib| lib.annotations(&[book]));
    match rows {
        Ok(rows) => {
            let out: Vec<serde_json::Value> = rows
                .iter()
                .map(|r| {
                    serde_json::json!({
                        "id": r.id, "book": r.book, "format": r.format,
                        "user_type": r.user_type, "user": r.user,
                        "annot_type": r.annot_type, "annot_id": r.annot_id,
                        "annot_data": match &r.data { Ok(v) => v.clone(), Err(e) => serde_json::json!({"unparseable": e}) },
                    })
                })
                .collect();
            println!(
                "{}",
                serde_json::to_string_pretty(&out).expect("values serialise")
            );
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("{e}");
            ExitCode::FAILURE
        }
    }
}

fn run_ingest_calibre(
    archive: &Path,
    library: Option<&Path>,
    config: Option<&Path>,
    books: &[i64],
    dry_run: bool,
) -> ExitCode {
    let result = Config::load(config, archive).and_then(|config| {
        let library = library
            .map(Path::to_path_buf)
            .or(config.calibre.library)
            .ok_or("no calibre library: pass --library or set [calibre] library in config.toml")?;
        ingest_calibre(&library, archive, books, dry_run)
    });
    match result {
        Ok(r) => {
            print_calibre_ingest(&r, dry_run);
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("{e}");
            ExitCode::FAILURE
        }
    }
}

fn print_calibre_ingest(r: &CalibreIngestReport, dry_run: bool) {
    print_book_reports(&r.books, &r.saved, dry_run);
    let read: Vec<String> = r.formats.iter().map(|(f, n)| format!("{n} {f}")).collect();
    if !read.is_empty() {
        println!("\nRead viewer highlights: {}", read.join(", "));
    }
    let not_writable: usize = r
        .formats
        .iter()
        .filter(|(f, _)| *f != annot_archive::calibre::WRITABLE_FORMAT)
        .map(|(_, n)| n)
        .sum();
    if not_writable > 0 {
        println!(
            "  {not_writable} are on formats other than {0}; derive-calibre places them in the book's {0} too.",
            annot_archive::calibre::WRITABLE_FORMAT
        );
    }
    if !r.warnings.is_empty() {
        println!("\nWarnings ({}):", r.warnings.len());
        for w in &r.warnings {
            println!("  {w}");
        }
    }
    if !r.possible_duplicates.is_empty() {
        println!("\nPossibly the same book (same title); merge each pair with merge-books if so:");
        for (calibre, kindle) in &r.possible_duplicates {
            println!("  just merge-books {calibre} {kindle} <archive>");
        }
    }
    println!("\n(skipped {} highlights deleted in calibre)", r.removed);
}

fn run_derive(
    archive: &Path,
    library: Option<&Path>,
    config: Option<&Path>,
    rederive: bool,
    books: &[String],
    dry_run: bool,
) -> ExitCode {
    let result = Config::load(config, archive).and_then(|config| {
        let library = library
            .map(Path::to_path_buf)
            .or(config.calibre.library.clone())
            .ok_or("no calibre library: pass --library or set [calibre] library in config.toml")?;
        derive_calibre(
            &library,
            archive,
            &config.derive_config(),
            rederive,
            books,
            dry_run,
        )
    });
    match result {
        Ok(r) => {
            print_derive(&r, dry_run);
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("{e}");
            ExitCode::FAILURE
        }
    }
}

fn print_derive(r: &DeriveReport, dry_run: bool) {
    let mut totals: BTreeMap<&str, usize> = BTreeMap::new();
    for b in &r.books {
        for (k, n) in &b.counts {
            *totals.entry(k.as_str()).or_default() += n;
        }
    }
    let summary: Vec<String> = totals.iter().map(|(k, n)| format!("{n} {k}")).collect();
    println!(
        "{} books derived: {}",
        r.books.len(),
        if summary.is_empty() {
            "nothing to do".into()
        } else {
            summary.join(", ")
        }
    );
    let verb = if dry_run { "would write" } else { "wrote" };
    println!(
        "files: {verb} {} changed; {} unchanged",
        r.saved.updated.len(),
        r.saved.unchanged
    );

    let mut sections: BTreeMap<&str, Vec<String>> = BTreeMap::new();
    for b in &r.books {
        let counts: Vec<String> = b.counts.iter().map(|(k, n)| format!("{n} {k}")).collect();
        let (section, line) = match &b.outcome {
            BookOutcome::Matched { calibre_title, how } => {
                let how = match how {
                    How::AlreadyLinked => "linked".to_string(),
                    How::Override => "override".to_string(),
                    How::Fuzzy(s) => format!("matched {s:.2}"),
                };
                (
                    "1 Matched",
                    format!(
                        "{}  ->  {calibre_title} ({how}): {}",
                        b.file_name,
                        counts.join(", ")
                    ),
                )
            }
            BookOutcome::NoEpub { calibre_title } => (
                "2 No EPUB in calibre (convert it to EPUB)",
                format!("{}  ->  {calibre_title}", b.file_name),
            ),
            BookOutcome::NoCalibreBook { candidates } => {
                let mut line = format!("{}  ({})", b.file_name, b.title);
                for c in candidates.iter().filter(|c| c.score > 0.0) {
                    let epub = if c.has_epub { "" } else { ", no EPUB" };
                    line.push_str(&format!(
                        "\n      {:.2} {} (#{}{epub})",
                        c.score, c.title, c.calibre_id
                    ));
                    if let (Some(u), Some(t), true) = (&c.uuid, &b.kindle_title, c.score >= 0.25) {
                        line.push_str(&format!(
                            "\n           override: {t:?}: {{\"calibre_uuid\": {u:?}}}"
                        ));
                    }
                }
                (
                    "3 No calibre book (copy an override line into book-overrides.json if one is right)",
                    line,
                )
            }
            BookOutcome::Gone { calibre_uuid } => (
                "4 calibre book no longer in the library",
                format!("{}  ({calibre_uuid})", b.file_name),
            ),
            BookOutcome::UnreadableEpub { error } => {
                ("5 Unreadable EPUB", format!("{}: {error}", b.file_name))
            }
            BookOutcome::ClaimedByOtherFile {
                calibre_title,
                other_file,
            } => (
                "6 Already has its own archive file (merge first, then derive again)",
                format!(
                    "just merge-books {other_file} {} <archive>   # {calibre_title}",
                    b.file_name
                ),
            ),
        };
        sections.entry(section).or_default().push(line);
    }
    for (section, lines) in sections {
        println!("\n{}:", &section[2..]);
        for l in lines {
            println!("  {l}");
        }
    }
}

fn run_push(
    archive: &Path,
    library: Option<&Path>,
    config: Option<&Path>,
    include: &[String],
    calibre_closed: bool,
    dry_run: bool,
) -> ExitCode {
    let mut options = PushOptions::default();
    for kind in include {
        match kind.as_str() {
            "approximate" => options.include_approximate = true,
            "ambiguous" => options.include_ambiguous = true,
            other => {
                eprintln!("--include takes approximate and/or ambiguous, not {other:?}");
                return ExitCode::FAILURE;
            }
        }
    }
    if !dry_run {
        if !calibre_closed {
            eprintln!(
                "push writes to calibre's metadata.db: close calibre and its viewer, then pass --calibre-closed (or use --dry-run)"
            );
            return ExitCode::FAILURE;
        }
        let running = running_calibre_processes();
        if !running.is_empty() {
            eprintln!(
                "calibre is still running ({}); close it and try again",
                running.join(", ")
            );
            return ExitCode::FAILURE;
        }
    }
    let result = Config::load(config, archive).and_then(|config| {
        let library = library
            .map(Path::to_path_buf)
            .or(config.calibre.library.clone())
            .ok_or("no calibre library: pass --library or set [calibre] library in config.toml")?;
        let debug = calibre_debug(config.calibre.debug_executable.as_deref());
        push(&library, archive, &options, &debug, dry_run)
    });
    match result {
        Ok(r) => {
            print_push(&r, dry_run);
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("{e}");
            ExitCode::FAILURE
        }
    }
}

fn print_push(r: &PushReport, dry_run: bool) {
    let p = &r.plan;
    let (added, updated): (usize, usize) = p
        .books
        .iter()
        .map(|b| (b.added, b.updated))
        .fold((0, 0), |a, b| (a.0 + b.0, a.1 + b.1));
    let verb = if dry_run { "would write" } else { "wrote" };
    println!(
        "{verb} {} books: {added} highlights added, {updated} updated; {} books already up to date",
        p.books.len(),
        p.up_to_date
    );
    if let Some(b) = &r.backup {
        println!("metadata.db backed up to {}", b.display());
    }
    if !p.books.is_empty() {
        println!();
        for b in &p.books {
            println!(
                "  +{} ~{} ={} kept {}  {} (#{})",
                b.added, b.updated, b.unchanged, b.kept, b.title, b.book_id
            );
        }
    }
    if !p.skipped.is_empty() {
        println!("\nNot pushed:");
        for (reason, n) in &p.skipped {
            println!("  {n:5}  {reason}");
        }
    }
}

fn run_merge_books(archive: &Path, keep: &str, absorb: &str, dry_run: bool) -> ExitCode {
    match merge_books(archive, keep, absorb, dry_run) {
        Ok(r) => {
            let verb = if dry_run { "would move" } else { "moved" };
            println!(
                "{verb} {} entries from {} into {}",
                r.moved_entries, r.absorbed, r.kept
            );
            for t in &r.added_kindle_titles {
                println!("  kindle title added: {t}");
            }
            if let Some(u) = &r.took_calibre_uuid {
                println!("  calibre book taken over: {u}");
            }
            let verb = if dry_run { "would delete" } else { "deleted" };
            println!("  {verb} {}", r.saved.removed.join(", "));
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("{e}");
            ExitCode::FAILURE
        }
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
    print_book_reports(&r.books, &r.saved, dry_run);
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

fn print_book_reports(
    books: &[BookReport],
    saved: &annot_archive::archive::SaveReport,
    dry_run: bool,
) {
    let sum = |f: fn(&BookReport) -> usize| books.iter().map(f).sum::<usize>();
    let merges: Vec<_> = books
        .iter()
        .flat_map(|b| b.merges.iter().map(move |m| (b, m)))
        .collect();
    println!(
        "{} books: {} entries created, {} updated, {} unchanged, {} merged, {} matched tombstones",
        books.len(),
        sum(|b| b.created),
        sum(|b| b.updated),
        sum(|b| b.unchanged),
        merges.len(),
        sum(|b| b.tombstone_matched),
    );
    let verb = if dry_run { "would write" } else { "wrote" };
    println!(
        "files: {verb} {} new, {} changed; {} unchanged",
        saved.created.len(),
        saved.updated.len(),
        saved.unchanged
    );

    let changed: Vec<_> = books.iter().filter(|b| b.changed()).collect();
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
        println!("\n!! Entries merged because an incoming entry joined them (review these):");
        for (b, (id, absorbed)) in merges {
            println!("  {id} absorbed {} in {}", absorbed.join(", "), b.file_name);
        }
    }
}
