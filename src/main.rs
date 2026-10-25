use std::collections::BTreeMap;
use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};

use annot_archive::id::{EntryKind, key_v1};
use annot_archive::kindle::parse::{ParseConfig, RawClipping, parse};

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
        /// List the clippings of books whose title line contains this text
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

    let mut books: BTreeMap<String, Vec<&RawClipping>> = BTreeMap::new();
    for c in &parsed.clippings {
        books.entry(key_v1(&c.title_line)).or_default().push(c);
    }
    println!(
        "\nBooks ({}):\n  {:>5} {:>5}  {:10}  {:10}  TITLE",
        books.len(),
        "HL",
        "NOTES",
        "FIRST",
        "LAST"
    );
    for clips in books.values() {
        let notes = clips.iter().filter(|c| c.kind == EntryKind::Note).count();
        let dates: Vec<String> = clips
            .iter()
            .filter_map(|c| c.created)
            .map(|t| t.to_string()[..10].to_string())
            .collect();
        println!(
            "  {:>5} {:>5}  {:10}  {:10}  {}",
            clips.len() - notes,
            notes,
            dates.iter().min().map_or("-", String::as_str),
            dates.iter().max().map_or("-", String::as_str),
            clips[0].title_line,
        );
    }

    if let Some(needle) = book {
        let needle = needle.to_lowercase();
        for (key, clips) in books.iter().filter(|(k, _)| k.contains(&needle)) {
            println!("\n{} ({key})", clips[0].title_line);
            for c in clips {
                println!("  {}", describe(c));
            }
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
    format!("#{:<5} {:22} {}  {kind}{text}", c.index, pos, date)
}
