//! Fold a clippings file's edit history into clusters, one per real
//! highlight or orphan note.
//!
//! Kindle records every edit of a highlight (extend, trim, redo) as a new
//! appended entry, so overlapping highlights are replayed in file order and
//! only the latest survives. This is the only place in the system that
//! judges overlap; everything downstream matches by ID.

use std::collections::BTreeMap;

use crate::id::{EntryKind, KindleClippingKey, key_v1, kindle_id};
use crate::kindle::parse::RawClipping;
use crate::norm::norm_v1;

#[derive(Debug, Clone)]
pub struct ReplayConfig {
    /// The shared text must cover at least this fraction of the shorter text.
    pub min_shared_ratio: f64,
    /// ...and be at least this many characters, unless the shorter text is
    /// itself shorter, in which case it must be wholly contained.
    pub min_shared_chars: usize,
    /// Locations a note may sit outside its highlight's range and still attach.
    pub note_tolerance: u32,
}

impl Default for ReplayConfig {
    fn default() -> Self {
        ReplayConfig {
            min_shared_ratio: 0.5,
            min_shared_chars: 20,
            note_tolerance: 0,
        }
    }
}

/// One archive-bound entry: a highlight with its edit history, or an orphan
/// note.
#[derive(Debug, Clone, PartialEq)]
pub struct Cluster {
    /// The would-be ID of the earliest member in file order.
    pub id: String,
    /// The would-be IDs of every other member, sorted, without `id`.
    pub aliases: Vec<String>,
    /// `Highlight`, or `Note` for an orphan note.
    pub kind: EntryKind,
    /// The latest member, which supplies text, position and date.
    pub latest: RawClipping,
    /// The attached note's text (highlights only); several notes at different
    /// positions are joined with a blank line.
    pub note: Option<String>,
    /// File indexes of attached notes replaced by a later edit at the same
    /// position.
    pub superseded_notes: Vec<usize>,
    /// File indexes of every member, ascending.
    pub members: Vec<usize>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Book {
    /// `key_v1` of the title line; what the archive matches books on.
    pub key: String,
    /// Every distinct title line seen for this book, sorted.
    pub title_lines: Vec<String>,
    /// Ordered by earliest member in file order.
    pub clusters: Vec<Cluster>,
}

/// Replay every book in the file. Books are ordered by key.
pub fn replay(clippings: &[RawClipping], config: &ReplayConfig) -> Vec<Book> {
    let mut by_book: BTreeMap<String, Vec<&RawClipping>> = BTreeMap::new();
    for c in clippings {
        by_book.entry(key_v1(&c.title_line)).or_default().push(c);
    }
    by_book
        .into_iter()
        .map(|(key, clips)| {
            let mut title_lines: Vec<String> = clips.iter().map(|c| c.title_line.clone()).collect();
            title_lines.sort();
            title_lines.dedup();
            Book {
                key,
                title_lines,
                clusters: replay_book(&clips, config),
            }
        })
        .collect()
}

/// Work-in-progress cluster: member clippings in file order, the last being
/// the latest.
struct Group<'a> {
    members: Vec<&'a RawClipping>,
    notes: Vec<&'a RawClipping>,
}

impl<'a> Group<'a> {
    fn latest(&self) -> &'a RawClipping {
        self.members.last().unwrap()
    }
}

fn replay_book(clips: &[&RawClipping], config: &ReplayConfig) -> Vec<Cluster> {
    // Highlights: a new highlight absorbs every current group whose latest
    // member truly overlaps it.
    let mut current: Vec<Group> = Vec::new();
    for &h in clips.iter().filter(|c| c.kind == EntryKind::Highlight) {
        let (hits, rest): (Vec<Group>, Vec<Group>) = current
            .into_iter()
            .partition(|g| overlaps(g.latest(), h, config));
        current = rest;
        let mut members: Vec<&RawClipping> = hits.into_iter().flat_map(|g| g.members).collect();
        members.sort_by_key(|c| c.index);
        members.push(h);
        current.push(Group {
            members,
            notes: Vec::new(),
        });
    }

    // Notes: attach to the highlight containing the note's position; the rest
    // become orphan groups, latest-wins per position.
    let mut orphans: Vec<Group> = Vec::new();
    for &n in clips.iter().filter(|c| c.kind == EntryKind::Note) {
        if let Some(g) = note_target(&mut current, n, config) {
            g.notes.push(n);
        } else if let Some(g) = orphans
            .iter_mut()
            .find(|g| g.latest().position() == n.position())
        {
            g.members.push(n);
        } else {
            orphans.push(Group {
                members: vec![n],
                notes: Vec::new(),
            });
        }
    }

    let mut out: Vec<Cluster> = current
        .into_iter()
        .map(|g| finish(g, EntryKind::Highlight))
        .chain(orphans.into_iter().map(|g| finish(g, EntryKind::Note)))
        .collect();
    out.sort_by_key(|c| c.members[0]);
    out
}

fn finish(g: Group, kind: EntryKind) -> Cluster {
    let mut ids: Vec<String> = g.members.iter().map(|c| would_be_id(c)).collect();
    let id = ids.remove(0);
    ids.sort();
    ids.dedup();
    ids.retain(|a| *a != id);
    let (note, superseded_notes) = combine_notes(&g.notes);
    Cluster {
        id,
        aliases: ids,
        kind,
        latest: g.latest().clone(),
        note,
        superseded_notes,
        members: g.members.iter().map(|c| c.index).collect(),
    }
}

/// A highlight's note from the notes attached to it, in file order. Notes at
/// the same position are edits, so the latest wins; notes at different
/// positions are separate thoughts (e.g. a word highlighted and annotated,
/// then the whole sentence), joined with a blank line in order of first
/// writing, each distinct text once. Returns the note and the file indexes of
/// the edits that lost.
fn combine_notes(notes: &[&RawClipping]) -> (Option<String>, Vec<usize>) {
    let mut by_position: Vec<Vec<&RawClipping>> = Vec::new();
    for &n in notes {
        match by_position
            .iter_mut()
            .find(|g| g[0].position() == n.position())
        {
            Some(g) => g.push(n),
            None => by_position.push(vec![n]),
        }
    }
    let mut texts: Vec<&str> = Vec::new();
    for g in &by_position {
        let text = g.last().unwrap().text.as_str();
        if !texts.contains(&text) {
            texts.push(text);
        }
    }
    let superseded = by_position
        .iter()
        .flat_map(|g| g[..g.len() - 1].iter().map(|n| n.index))
        .collect();
    ((!texts.is_empty()).then(|| texts.join("\n\n")), superseded)
}

pub fn would_be_id(c: &RawClipping) -> String {
    kindle_id(&KindleClippingKey {
        kind: c.kind,
        title: &c.title,
        author: c.author.as_deref(),
        text: &c.text,
        position: c.position(),
        raw_added_on: &c.raw_added_on,
    })
}

/// Location (or, failing that, page) range of a clipping, if both sides have
/// the same kind of position.
fn comparable_ranges(a: &RawClipping, b: &RawClipping) -> Option<((u32, u32), (u32, u32))> {
    match (a.location, b.location) {
        (Some(x), Some(y)) => Some((x, y)),
        (None, None) => Some((a.page?, b.page?)),
        _ => None,
    }
}

/// True overlap: ranges that touch or intersect, confirmed by shared text.
/// Location alone is not enough, because adjacent but distinct highlights
/// routinely share a boundary location.
pub fn overlaps(a: &RawClipping, b: &RawClipping, config: &ReplayConfig) -> bool {
    let Some(((a0, a1), (b0, b1))) = comparable_ranges(a, b) else {
        return false;
    };
    if a0 > b1 || b0 > a1 {
        return false;
    }
    let ta = comparable_text(&a.text);
    let tb = comparable_text(&b.text);
    let shorter = ta.len().min(tb.len());
    if shorter == 0 {
        return false;
    }
    let shared = longest_common_substring(&ta, &tb);
    shared as f64 >= config.min_shared_ratio * shorter as f64
        && shared >= config.min_shared_chars.min(shorter)
}

/// Lowercased `norm_v1` text without punctuation at either edge: extending a
/// highlight usually turns its final `.` into `,`, which must not hide that the
/// old text is still wholly inside the new one.
fn comparable_text(text: &str) -> Vec<char> {
    norm_v1(text)
        .to_lowercase()
        .trim_matches(|c: char| !c.is_alphanumeric())
        .chars()
        .collect()
}

fn longest_common_substring(a: &[char], b: &[char]) -> usize {
    let mut prev = vec![0usize; b.len() + 1];
    let mut cur = vec![0usize; b.len() + 1];
    let mut best = 0;
    for &ca in a {
        for (j, &cb) in b.iter().enumerate() {
            cur[j + 1] = if ca == cb { prev[j] + 1 } else { 0 };
            best = best.max(cur[j + 1]);
        }
        std::mem::swap(&mut prev, &mut cur);
    }
    best
}

/// The highlight group a note belongs to: its range must contain the note's
/// position. When several do, prefer the group with a highlight made most
/// recently before the note, because a note is written straight after its
/// highlight; Kindle's note position (usually the highlight's end, often its
/// start) cannot break the tie.
fn note_target<'g, 'a>(
    groups: &'g mut [Group<'a>],
    n: &RawClipping,
    config: &ReplayConfig,
) -> Option<&'g mut Group<'a>> {
    let t = config.note_tolerance;
    let mut best: Option<(usize, (bool, isize))> = None;
    for (i, g) in groups.iter().enumerate() {
        let Some(((h0, h1), (n0, _))) = comparable_ranges(g.latest(), n) else {
            continue;
        };
        if n0 + t < h0 || n0 > h1 + t {
            continue;
        }
        let made_before = g
            .members
            .iter()
            .map(|m| m.index)
            .filter(|&m| m < n.index)
            .max();
        let rank = match made_before {
            Some(m) => (true, m as isize),
            None => (false, -(g.members[0].index as isize)),
        };
        if best.is_none_or(|(_, r)| rank > r) {
            best = Some((i, rank));
        }
    }
    best.map(|(i, _)| &mut groups[i])
}
