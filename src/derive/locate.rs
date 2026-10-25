//! Where in a book's EPUB a highlight's text sits.

use std::path::Path;

use crate::cfi::{Haystack, encode};
use crate::epub::{Epub, SpineItem};
use crate::norm::norm_v1;
use crate::xhtml::Dom;

pub struct Doc {
    pub spine: SpineItem,
    /// The parsed document, or why calibre's XML path would refuse it.
    pub parsed: Result<(Dom, Haystack), String>,
    /// Characters of normalised text in every earlier spine document.
    pub offset: usize,
}

/// A book's spine documents, parsed once, and its table of contents placed
/// in them.
pub struct BookText {
    pub docs: Vec<Doc>,
    /// TOC entries as ((document, node), TOC order, titles), for the entries
    /// whose target is a readable document.
    toc: Vec<((usize, usize), usize, Vec<String>)>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Hit {
    pub doc: usize,
    pub range: (usize, usize),
}

#[derive(Debug, Clone, PartialEq)]
pub enum Located {
    One(Hit),
    Many(Vec<Hit>),
    /// Not found verbatim; the closest window and its similarity (0 to 1).
    Approximate(Hit, f64),
    NotFound {
        cross_spine: bool,
        unreadable_docs: bool,
    },
}

impl BookText {
    pub fn open(path: &Path) -> Result<BookText, String> {
        let mut epub = Epub::open(path)?;
        let mut docs = Vec::new();
        let mut offset = 0;
        for item in epub.spine.clone() {
            if !item.media_type.contains("html") {
                continue;
            }
            let parsed = epub
                .read(&item.name)
                .and_then(|b| Dom::parse(&b))
                .map(|dom| {
                    let hay = Haystack::new(&dom);
                    (dom, hay)
                });
            let len = parsed.as_ref().map_or(0, |(_, h)| h.chars.len());
            docs.push(Doc {
                spine: item,
                parsed,
                offset,
            });
            // A separator character between documents keeps their text apart.
            offset += len + 1;
        }
        let tree = crate::toc::read_toc(&mut epub);
        let toc = crate::toc::flatten(&tree)
            .into_iter()
            .enumerate()
            .filter_map(|(order, (node, family))| {
                let dest = node.dest.as_ref()?;
                let doc = docs.iter().position(|d| d.spine.name == *dest)?;
                let (dom, _) = docs[doc].parsed.as_ref().ok()?;
                // A missing anchor counts as the start of its document.
                let at = node
                    .frag
                    .as_ref()
                    .and_then(|f| dom.element_with_id(f))
                    .unwrap_or(0);
                Some(((doc, at), order, family))
            })
            .collect();
        Ok(BookText { docs, toc })
    }

    /// The titles calibre's viewer would record for a highlight starting at
    /// `hit`: those of the last TOC entry at or before it in reading order
    /// (the later TOC entry on a tie, i.e. the deeper one), with its titled
    /// ancestors, outermost first. Empty before the first entry.
    pub fn toc_family(&self, hit: Hit) -> Vec<String> {
        let Ok((dom, hay)) = &self.docs[hit.doc].parsed else {
            return Vec::new();
        };
        let Some((start, _)) = hay.points(dom, hit.range) else {
            return Vec::new();
        };
        let here = (hit.doc, start.node);
        self.toc
            .iter()
            .filter(|(at, _, _)| *at <= here)
            .max_by_key(|(at, order, _)| (*at, *order))
            .map(|(_, _, family)| family.clone())
            .unwrap_or_default()
    }

    pub fn total_len(&self) -> usize {
        self.docs.last().map_or(0, |d| {
            d.offset + d.parsed.as_ref().map_or(0, |(_, h)| h.chars.len())
        })
    }

    pub fn global_offset(&self, hit: Hit) -> usize {
        self.docs[hit.doc].offset + hit.range.0
    }

    pub fn locate(&self, text: &str, approximate_threshold: f64) -> Located {
        let needle: Vec<char> = norm_v1(text).chars().collect();
        let hits: Vec<Hit> = self
            .readable()
            .flat_map(|(i, hay)| {
                hay.find_all(&needle)
                    .into_iter()
                    .map(move |range| Hit { doc: i, range })
            })
            .collect();
        match hits.len() {
            1 => return Located::One(hits[0]),
            n if n > 1 => return Located::Many(hits),
            _ => {}
        }
        if let Some((hit, similarity)) = self
            .closest(&needle)
            .filter(|(_, s)| *s >= approximate_threshold)
        {
            return Located::Approximate(hit, similarity);
        }
        Located::NotFound {
            cross_spine: self.crosses_documents(&needle),
            unreadable_docs: self.docs.iter().any(|d| d.parsed.is_err()),
        }
    }

    /// Start and end CFI of a hit, as the viewer would encode them.
    pub fn cfis(&self, hit: Hit) -> Option<(String, String)> {
        let (dom, hay) = self.docs[hit.doc].parsed.as_ref().ok()?;
        let (s, e) = hay.points(dom, hit.range)?;
        Some((encode(dom, s), encode(dom, e)))
    }

    fn readable(&self) -> impl Iterator<Item = (usize, &Haystack)> {
        self.docs
            .iter()
            .enumerate()
            .filter_map(|(i, d)| d.parsed.as_ref().ok().map(|(_, h)| (i, h)))
    }

    /// The window most like `needle`, found by anchoring on its first and
    /// last words. Bounded: only windows starting at an anchor are compared.
    fn closest(&self, needle: &[char]) -> Option<(Hit, f64)> {
        let n = needle.len();
        if n < 20 {
            return None;
        }
        let anchor = (n / 4).clamp(8, 40);
        let (head, tail) = (&needle[..anchor], &needle[n - anchor..]);
        let mut best: Option<(Hit, f64)> = None;
        for (doc, hay) in self.readable() {
            let starts: Vec<usize> = hay
                .find_all(head)
                .into_iter()
                .map(|(s, _)| s)
                .take(20)
                .collect();
            let ends: Vec<usize> = hay
                .find_all(tail)
                .into_iter()
                .map(|(_, e)| e)
                .take(20)
                .collect();
            let mut windows: Vec<(usize, usize)> = Vec::new();
            for &s in &starts {
                windows.extend(
                    ends.iter()
                        .filter(|&&e| e > s && within(e - s, n))
                        .map(|&e| (s, e)),
                );
                windows.push((s, (s + n).min(hay.chars.len())));
            }
            for &e in &ends {
                windows.push((e.saturating_sub(n), e));
            }
            for (s, e) in windows {
                let similarity = similarity(&hay.chars[s..e], needle);
                if best.is_none_or(|(_, b)| similarity > b) {
                    best = Some((Hit { doc, range: (s, e) }, similarity));
                }
            }
        }
        best
    }

    /// Whether the text begins at the end of one document and continues at the
    /// start of the next.
    fn crosses_documents(&self, needle: &[char]) -> bool {
        let docs: Vec<&Haystack> = self
            .docs
            .iter()
            .filter_map(|d| d.parsed.as_ref().ok().map(|(_, h)| h))
            .collect();
        docs.windows(2).any(|w| {
            let (a, b) = (&w[0].chars, &w[1].chars);
            (1..needle.len()).any(|split| {
                let (left, right) = needle.split_at(split);
                let right = if right.first() == Some(&' ') {
                    &right[1..]
                } else {
                    right
                };
                let left = if left.last() == Some(&' ') {
                    &left[..left.len() - 1]
                } else {
                    left
                };
                !left.is_empty() && !right.is_empty() && a.ends_with(left) && b.starts_with(right)
            })
        })
    }
}

fn within(len: usize, n: usize) -> bool {
    len * 4 >= n * 3 && len * 3 <= n * 4
}

/// 1 minus the edit distance over the longer length.
fn similarity(a: &[char], b: &[char]) -> f64 {
    let longest = a.len().max(b.len());
    if longest == 0 {
        return 1.0;
    }
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    let mut cur = vec![0; b.len() + 1];
    for (i, ca) in a.iter().enumerate() {
        cur[0] = i + 1;
        for (j, cb) in b.iter().enumerate() {
            cur[j + 1] = (prev[j] + usize::from(ca != cb))
                .min(prev[j + 1] + 1)
                .min(cur[j] + 1);
        }
        std::mem::swap(&mut prev, &mut cur);
    }
    1.0 - prev[b.len()] as f64 / longest as f64
}
