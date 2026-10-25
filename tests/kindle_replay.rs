use annot_archive::id::EntryKind;
use annot_archive::kindle::parse::{ParseConfig, RawClipping, parse};
use annot_archive::kindle::replay::{Book, Cluster, ReplayConfig, overlaps, replay, would_be_id};

const FIXTURE: &str = include_str!("fixtures/kindle/replay.txt");

fn clippings() -> Vec<RawClipping> {
    let p = parse(FIXTURE, &ParseConfig::default()).unwrap();
    assert!(p.warnings.is_empty(), "{:#?}", p.warnings);
    p.clippings
}

fn books_with(config: &ReplayConfig) -> Vec<Book> {
    replay(&clippings(), config)
}

fn books() -> Vec<Book> {
    books_with(&ReplayConfig::default())
}

fn orchard(books: &[Book]) -> &Book {
    books
        .iter()
        .find(|b| b.key.starts_with("the orchard ledger"))
        .unwrap()
}

fn cluster<'a>(book: &'a Book, latest_text_start: &str) -> &'a Cluster {
    let found: Vec<&Cluster> = book
        .clusters
        .iter()
        .filter(|c| c.latest.text.starts_with(latest_text_start))
        .collect();
    assert_eq!(
        found.len(),
        1,
        "clusters whose latest text starts {latest_text_start:?}: {found:#?}"
    );
    found[0]
}

/// The would-be IDs of the clippings at these file indexes.
fn ids(indexes: &[usize]) -> Vec<String> {
    let all = clippings();
    indexes.iter().map(|&i| would_be_id(&all[i])).collect()
}

fn assert_identity(c: &Cluster, members: &[usize]) {
    assert_eq!(c.members, members);
    let mut expected = ids(members);
    let id = expected.remove(0);
    expected.sort();
    expected.dedup();
    expected.retain(|a| *a != id);
    assert_eq!(c.id, id, "survivor must be the earliest member");
    assert_eq!(c.aliases, expected);
}

#[test]
fn extended_highlight_folds_and_keeps_its_note() {
    let books = books();
    let c = cluster(
        orchard(&books),
        "The orchard keeps two ledgers, one in ink and one in the rings of its oldest trees, and",
    );
    assert_identity(c, &[0, 1]);
    assert_eq!(c.note.as_deref(), Some("Ledger metaphor again."));
    assert_eq!(c.latest.location, Some((10, 12)));
}

#[test]
fn trimmed_highlight_folds_to_the_trim() {
    let books = books();
    let c = cluster(orchard(&books), "conducted in the language");
    assert_identity(c, &[3, 4]);
}

#[test]
fn edit_of_an_edit_folds_and_the_latest_note_wins() {
    let books = books();
    let c = cluster(orchard(&books), "Bees do not read");
    assert_identity(c, &[5, 7, 8]);
    assert!(c.latest.text.ends_with("never late for them."));
    assert_eq!(c.note.as_deref(), Some("Second, better thought."));
    assert_eq!(c.superseded_notes, vec![6]);
}

#[test]
fn rereading_the_same_passage_months_later_folds() {
    let books = books();
    let c = cluster(orchard(&books), "A windbreak");
    assert_eq!(c.members.len(), 2);
    assert_eq!(c.latest.raw_added_on, "Saturday, 5 September 2026 08:00:00");
    assert_identity(c, &c.members.clone());
}

#[test]
fn a_wide_highlight_absorbs_two() {
    let books = books();
    let c = cluster(
        orchard(&books),
        "Frost settles first in the low corners of the field, where",
    );
    assert_identity(c, &[11, 12, 13]);
}

#[test]
fn overlapping_only_a_superseded_member_does_not_absorb() {
    let books = books();
    let book = orchard(&books);
    assert_identity(
        cluster(book, "The cider press remembers every apple by its weight"),
        &[15, 16],
    );
    assert_identity(cluster(book, "every season by its silence"), &[17]);
}

#[test]
fn sharing_a_boundary_location_is_not_overlap() {
    let books = books();
    let book = orchard(&books);
    let ladders = cluster(book, "Ladders lean");
    let wasps = cluster(book, "Nobody counts");
    assert_identity(ladders, &[18]);
    assert_identity(wasps, &[19]);
    // both contain the note's location; it goes to the one made just before it
    assert_eq!(wasps.note.as_deref(), Some("Lovely image."));
    assert_eq!(ladders.note, None);
}

#[test]
fn an_edit_that_moved_both_ends_folds() {
    let books = books();
    assert_identity(
        cluster(orchard(&books), "a kind of borrowed memory"),
        &[21, 22],
    );
}

#[test]
fn an_exact_duplicate_entry_adds_no_alias() {
    let books = books();
    let c = cluster(orchard(&books), "Shade is a debt");
    assert_eq!(c.members, vec![23, 24]);
    assert!(c.aliases.is_empty());
}

#[test]
fn a_short_highlight_folds_into_its_extension() {
    let books = books();
    assert_identity(cluster(orchard(&books), "Yes, the orchard"), &[25, 26]);
}

#[test]
fn orphan_notes_become_entries_latest_wins() {
    let books = books();
    let book = orchard(&books);
    let orphans: Vec<&Cluster> = book
        .clusters
        .iter()
        .filter(|c| c.kind == EntryKind::Note)
        .collect();
    assert_eq!(orphans.len(), 2, "{orphans:#?}");
    let edited = cluster(book, "A note on nothing");
    assert_eq!(edited.kind, EntryKind::Note);
    assert_eq!(edited.latest.text, "A note on nothing at all.");
    assert_identity(edited, &[27, 28]);
    let past_frost = cluster(book, "Just past the frost");
    assert_eq!(past_frost.kind, EntryKind::Note);
}

#[test]
fn note_tolerance_widens_attachment() {
    let books = books_with(&ReplayConfig {
        note_tolerance: 1,
        ..ReplayConfig::default()
    });
    let book = orchard(&books);
    let frost = cluster(
        book,
        "Frost settles first in the low corners of the field, where",
    );
    assert_eq!(
        frost.note.as_deref(),
        Some("Just past the frost highlight.")
    );
    assert_eq!(
        book.clusters
            .iter()
            .filter(|c| c.kind == EntryKind::Note)
            .count(),
        1
    );
}

#[test]
fn page_only_highlights_fold_by_text() {
    let books = books();
    let pamphlet = books
        .iter()
        .find(|b| b.key.starts_with("field pamphlet"))
        .unwrap();
    assert_eq!(pamphlet.clusters.len(), 2);
    assert_identity(cluster(pamphlet, "Mulch is"), &[29, 30]);
    assert_identity(cluster(pamphlet, "Weeds are"), &[31]);
}

#[test]
fn every_clipping_lands_in_exactly_one_cluster() {
    let books = books();
    let mut seen: Vec<usize> = books
        .iter()
        .flat_map(|b| &b.clusters)
        .flat_map(|c| c.members.clone())
        .collect();
    seen.sort();
    // notes attached to highlights are not members; everything else is
    let attached_notes = [2, 6, 9, 20, 34, 36, 38, 40];
    let expected: Vec<usize> = (0..clippings().len())
        .filter(|i| !attached_notes.contains(i))
        .collect();
    assert_eq!(seen, expected);
}

#[test]
fn replay_is_deterministic() {
    assert_eq!(books(), books());
}

#[test]
fn overlap_needs_both_range_and_text() {
    let all = clippings();
    let config = ReplayConfig::default();
    assert!(overlaps(&all[0], &all[1], &config)); // extend
    assert!(!overlaps(&all[18], &all[19], &config)); // boundary only
    assert!(!overlaps(&all[16], &all[17], &config)); // shared text, disjoint ranges
}

#[test]
fn notes_at_different_positions_on_one_highlight_are_joined() {
    let books = books();
    let c = cluster(orchard(&books), "Count the harvest");
    assert_identity(c, &[33, 35]);
    assert_eq!(
        c.note.as_deref(),
        Some("See the glossary.\n\nSee the appendix.")
    );
    assert!(c.superseded_notes.is_empty());
}

#[test]
fn identical_notes_at_different_positions_appear_once() {
    let books = books();
    let c = cluster(orchard(&books), "The rootstock decides");
    assert_identity(c, &[37, 39]);
    assert_eq!(c.note.as_deref(), Some("Compare chapter two."));
}
