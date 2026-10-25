use annot_archive::archive::Archive;
use annot_archive::config::Config;
use annot_archive::ingest::ingest_clippings;
use annot_archive::list::list;
use serde_json::json;

fn clip(book: &str, kind: &str, loc: &str, minute: u32, text: &str) -> String {
    format!(
        "{book}\n- Your {kind} at location {loc} | Added on Sunday, 4 January 2026 10:{minute:02}:00\n\n{text}\n==========\n"
    )
}

#[test]
fn every_live_entry_is_one_searchable_line() {
    let dir = tempfile::tempdir().unwrap();
    let clippings = [
        clip(
            "Zebra Field Notes (Stone, Abel)",
            "Highlight",
            "5-6",
            1,
            "Stripes are a\tcrowd's camouflage.",
        ),
        clip(
            "The Orchard Ledger (Penn, Mara)",
            "Highlight",
            "10-11",
            2,
            "The orchard keeps two ledgers.",
        ),
        clip(
            "The Orchard Ledger (Penn, Mara)",
            "Note",
            "11",
            3,
            "Ledger metaphor,\nagain.",
        ),
        clip(
            "The Orchard Ledger (Penn, Mara)",
            "Highlight",
            "30-31",
            4,
            "Pruning is a conversation with next year.",
        ),
        clip(
            "The Orchard Ledger (Penn, Mara)",
            "Note",
            "999",
            5,
            "An orphan thought.",
        ),
    ]
    .concat();
    ingest_clippings(&clippings, dir.path(), &Config::default(), false).unwrap();
    let mut a = Archive::load(dir.path()).unwrap();
    for b in &mut a.books {
        for e in &mut b.data.entries {
            match e.text.as_deref() {
                Some(t) if t.starts_with("Pruning") => e.deleted = true,
                Some(t) if t.starts_with("The orchard") => {
                    e.calibre.insert(
                        "EPUB".into(),
                        json!({"match": "exact", "toc_family_titles": ["Part One", "Rings"]}),
                    );
                }
                _ => {}
            }
        }
    }
    a.save(false).unwrap();

    let lines = list(dir.path()).unwrap();
    let fields: Vec<Vec<&str>> = lines.iter().map(|l| l.split('\t').collect()).collect();
    assert_eq!(lines.len(), 3, "the tombstone is left out: {lines:#?}");
    assert!(fields.iter().all(|f| f.len() == 5 && f[0].len() == 22));

    // Books in title order; tabs and line breaks inside fields become spaces.
    assert_eq!(
        fields[0][1..],
        [
            "The Orchard Ledger",
            "Rings",
            "The orchard keeps two ledgers.",
            "Ledger metaphor, again."
        ]
    );
    assert_eq!(
        fields[1][1..],
        ["The Orchard Ledger", "", "", "An orphan thought."]
    );
    assert_eq!(
        fields[2][1..],
        [
            "Zebra Field Notes",
            "",
            "Stripes are a crowd's camouflage.",
            ""
        ]
    );
}
