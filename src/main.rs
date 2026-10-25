use std::collections::BTreeMap;
use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};

use annot_archive::id::{EntryKind, key_v1};
use annot_archive::kindle::parse::{ParseConfig, RawClipping, parse};
use annot_archive::kindle::replay::{ReplayConfig, replay};

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
}

fn main() -> ExitCode {
    match Cli::parse().command {
        Command::InspectClippings { clippings, book } => {
            inspect_clippings(&clippings, book.as_deref())
        }
    }
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
