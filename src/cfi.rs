//! Find a passage in a spine document and name its ends with CFIs exactly as
//! calibre's viewer would (`src/pyj/read_book/cfi.pyj`, `encode`).

use unicode_normalization::char::canonical_combining_class;

use crate::norm::norm_v1_mapped;
use crate::xhtml::{DOCUMENT, Dom, Kind, NodeId};

/// Elements whose boundaries separate words in rendered text, so text on
/// either side never runs together in the haystack.
const BLOCKS: &[&str] = &[
    "address",
    "article",
    "aside",
    "blockquote",
    "body",
    "br",
    "caption",
    "dd",
    "div",
    "dl",
    "dt",
    "fieldset",
    "figcaption",
    "figure",
    "footer",
    "form",
    "h1",
    "h2",
    "h3",
    "h4",
    "h5",
    "h6",
    "header",
    "hr",
    "li",
    "main",
    "nav",
    "ol",
    "p",
    "pre",
    "section",
    "table",
    "tbody",
    "td",
    "tfoot",
    "th",
    "thead",
    "tr",
    "ul",
];

/// Elements whose text is never rendered, so never highlighted.
const HIDDEN: &[&str] = &["head", "script", "style", "template"];

/// A point inside a text node, the offset in UTF-16 code units as a browser
/// counts them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Point {
    pub node: NodeId,
    pub offset: usize,
}

struct Segment {
    raw_start: usize,
    raw_end: usize,
    /// `None` for a separator standing in for a block boundary.
    node: Option<NodeId>,
}

/// A spine document's rendered text, normalised with `norm_v1`, mapped back
/// to the text nodes it came from.
pub struct Haystack {
    raw: String,
    segments: Vec<Segment>,
    /// The normalised text, as characters.
    pub chars: Vec<char>,
    /// Raw byte offset of each normalised character's source.
    char_raw: Vec<usize>,
}

impl Haystack {
    pub fn new(dom: &Dom) -> Haystack {
        let mut raw = String::new();
        let mut segments = Vec::new();
        walk(dom, dom.body(), &mut raw, &mut segments);
        let n = norm_v1_mapped(&raw);
        Haystack {
            raw,
            segments,
            chars: n.text.chars().collect(),
            char_raw: n.raw_offsets,
        }
    }

    /// Every place the normalised `needle` occurs, as character ranges.
    pub fn find_all(&self, needle: &[char]) -> Vec<(usize, usize)> {
        if needle.is_empty() || needle.len() > self.chars.len() {
            return Vec::new();
        }
        (0..=self.chars.len() - needle.len())
            .filter(|&i| self.chars[i..i + needle.len()] == *needle)
            .map(|i| (i, i + needle.len()))
            .collect()
    }

    /// The text-node points bounding a character range.
    pub fn points(&self, dom: &Dom, (start, end): (usize, usize)) -> Option<(Point, Point)> {
        let raw_start = self.char_raw[start];
        let raw_end = self.raw_segment_end(self.char_raw[end - 1]);
        Some((
            self.start_point(dom, raw_start)?,
            self.end_point(dom, raw_end)?,
        ))
    }

    /// End of the raw normalisation segment (a character plus its combining
    /// marks) beginning at `raw`.
    fn raw_segment_end(&self, raw: usize) -> usize {
        let mut chars = self.raw[raw..].char_indices();
        let Some((_, first)) = chars.next() else {
            return raw;
        };
        let mut end = raw + first.len_utf8();
        for (i, c) in chars {
            if canonical_combining_class(c) == 0 {
                break;
            }
            end = raw + i + c.len_utf8();
        }
        end
    }

    fn start_point(&self, dom: &Dom, raw: usize) -> Option<Point> {
        // The text segment holding `raw`, or the next one if `raw` falls on a
        // block separator.
        let s = self
            .segments
            .iter()
            .find(|s| s.node.is_some() && raw < s.raw_end)?;
        let node = s.node?;
        let within = raw.saturating_sub(s.raw_start);
        Some(Point {
            node,
            offset: utf16_len(&dom.text(node)?[..within]),
        })
    }

    fn end_point(&self, dom: &Dom, raw: usize) -> Option<Point> {
        let s = self
            .segments
            .iter()
            .rev()
            .find(|s| s.node.is_some() && s.raw_start < raw)?;
        let node = s.node?;
        let within = raw.min(s.raw_end) - s.raw_start;
        Some(Point {
            node,
            offset: utf16_len(&dom.text(node)?[..within]),
        })
    }
}

fn walk(dom: &Dom, n: NodeId, raw: &mut String, segments: &mut Vec<Segment>) {
    for &c in &dom.nodes[n].children {
        match &dom.nodes[c].kind {
            Kind::Text(t) => {
                let start = raw.len();
                raw.push_str(t);
                segments.push(Segment {
                    raw_start: start,
                    raw_end: raw.len(),
                    node: Some(c),
                });
            }
            Kind::Element { name, .. } if HIDDEN.contains(&name.as_str()) => {}
            Kind::Element { name, .. } => {
                let block = BLOCKS.contains(&name.as_str());
                if block {
                    separate(raw, segments);
                }
                walk(dom, c, raw, segments);
                if block {
                    separate(raw, segments);
                }
            }
            _ => {}
        }
    }
}

fn separate(raw: &mut String, segments: &mut Vec<Segment>) {
    if segments.last().is_some_and(|s| s.node.is_none()) {
        return;
    }
    let start = raw.len();
    raw.push('\n');
    segments.push(Segment {
        raw_start: start,
        raw_end: raw.len(),
        node: None,
    });
}

pub fn utf16_len(s: &str) -> usize {
    s.chars().map(char::len_utf16).sum()
}

/// The CFI of a point, relative to its spine document, as the viewer encodes
/// it: the offset counts from the parent's first child, summing the parent's
/// direct text nodes before the target (text inside sibling elements is not
/// counted); each element is an even step, a run of other nodes the odd step
/// before it; ids are asserted only when unique.
pub fn encode(dom: &Dom, p: Point) -> String {
    let parent = dom.nodes[p.node].parent.expect("text nodes have parents");
    let siblings = &dom.nodes[parent].children;
    let before = siblings
        .iter()
        .position(|&s| s == p.node)
        .expect("a child of its parent");
    let offset = p.offset
        + siblings[..before]
            .iter()
            .filter_map(|&s| dom.text(s))
            .map(utf16_len)
            .sum::<usize>();

    let mut cfi = format!(":{offset}");
    let mut n = siblings[0];
    while n != DOCUMENT {
        let up = dom.nodes[n]
            .parent
            .expect("only the document has no parent");
        let mut index = 0;
        for &c in &dom.nodes[up].children {
            index |= 1;
            if dom.is_element(c) {
                index += 1;
            }
            if c == n {
                break;
            }
        }
        let assertion = match &dom.nodes[n].kind {
            Kind::Element { id: Some(id), .. } if dom.id_count(id) == 1 => {
                format!("[{}]", escape(id))
            }
            _ => String::new(),
        };
        cfi = format!("/{index}{assertion}{cfi}");
        n = up;
    }
    cfi
}

/// The viewer's `escape_for_cfi`.
fn escape(raw: &str) -> String {
    raw.chars().fold(String::new(), |mut out, c| {
        if "[],^();~@!".contains(c) {
            out.push('^');
        }
        out.push(c);
        out
    })
}
