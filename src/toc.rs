//! An EPUB's table of contents, read the way calibre reads it
//! (`ebooks/oeb/polish/toc.py`, `get_toc`): EPUB 3 books from the navigation
//! document's `<nav epub:type="toc">`, falling back to the NCX; EPUB 2 books
//! from the NCX. Either way it is a tree of titled entries pointing at a spine
//! document and, optionally, an anchor in it. Nothing about parts, chapters or
//! sections is assumed: depth is whatever the book's TOC says.

use quick_xml::Reader;
use quick_xml::events::Event;

use crate::epub::{Epub, join, local, percent_decode};
use crate::xhtml::entity;

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TocNode {
    /// Stripped, as calibre stores it; `None` when empty.
    pub title: Option<String>,
    /// Spine document name (zip path).
    pub dest: Option<String>,
    pub frag: Option<String>,
    pub children: Vec<TocNode>,
}

/// The book's TOC: the root's children. Empty when the book has none (calibre
/// then builds one from h1–h3 headings, which is not reproduced here).
pub fn read_toc(epub: &mut Epub) -> Vec<TocNode> {
    let (nav, ncx) = (epub.nav.clone(), epub.ncx.clone());
    if epub.version >= 3
        && let Some(name) = &nav
        && let Some(t) = epub_read(epub, name).and_then(|b| parse_nav(&b, name))
        && !t.is_empty()
    {
        return t;
    }
    let Some(name) = ncx else { return Vec::new() };
    epub_read(epub, &name)
        .map(|b| parse_ncx(&b, &name))
        .unwrap_or_default()
}

fn epub_read(epub: &mut Epub, name: &str) -> Option<Vec<u8>> {
    epub.read(name).ok()
}

/// A minimal element tree, enough to walk NCX and nav documents.
#[derive(Debug, Default)]
struct Elem {
    /// Local name, lowercase.
    name: String,
    /// Attributes by local name, plus the full `prefix:name` for prefixed ones.
    attrs: Vec<(String, String)>,
    children: Vec<Child>,
}

#[derive(Debug)]
enum Child {
    Elem(Elem),
    Text(String),
}

impl Elem {
    fn attr(&self, key: &str) -> Option<&str> {
        self.attrs
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_str())
    }

    fn elems(&self) -> impl Iterator<Item = &Elem> {
        self.children.iter().filter_map(|c| match c {
            Child::Elem(e) => Some(e),
            Child::Text(_) => None,
        })
    }

    fn named<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a Elem> {
        self.elems().filter(move |e| e.name == name)
    }

    /// All descendant text, as lxml's `tostring(method='text')`.
    fn text(&self) -> String {
        let mut out = String::new();
        for c in &self.children {
            match c {
                Child::Text(t) => out.push_str(t),
                Child::Elem(e) => out.push_str(&e.text()),
            }
        }
        out
    }

    fn descendants<'a>(&'a self, out: &mut Vec<&'a Elem>) {
        for e in self.elems() {
            out.push(e);
            e.descendants(out);
        }
    }
}

fn parse_tree(bytes: &[u8]) -> Option<Elem> {
    let text = std::str::from_utf8(bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(bytes)).ok()?;
    let mut reader = Reader::from_str(text);
    let mut stack: Vec<Elem> = vec![Elem::default()];
    loop {
        match reader.read_event().ok()? {
            Event::Start(e) => stack.push(elem(&e)),
            Event::Empty(e) => {
                let el = elem(&e);
                stack.last_mut()?.children.push(Child::Elem(el));
            }
            Event::End(_) => {
                let done = stack.pop()?;
                stack.last_mut()?.children.push(Child::Elem(done));
            }
            Event::Text(t) => stack
                .last_mut()?
                .children
                .push(Child::Text(t.xml10_content().into_owned())),
            Event::CData(t) => stack
                .last_mut()?
                .children
                .push(Child::Text(t.into_inner().into_owned())),
            Event::GeneralRef(r) => {
                let s = match r.resolve_char_ref().ok()? {
                    Some(c) => c.to_string(),
                    None => entity(&r.xml10_content()).unwrap_or("").to_string(),
                };
                stack.last_mut()?.children.push(Child::Text(s));
            }
            Event::Eof => break,
            _ => {}
        }
    }
    let mut root = stack.pop()?;
    root.elems().next()?;
    Some(std::mem::take(&mut root))
}

fn elem(e: &quick_xml::events::BytesStart) -> Elem {
    let mut attrs = Vec::new();
    for a in e.attributes().flatten() {
        let key = a.key.as_ref().to_string();
        let value = a
            .normalized_value_with(quick_xml::XmlVersion::Implicit1_0, 128, entity)
            .map(|v| v.into_owned())
            .unwrap_or_default();
        if key.contains(':') {
            attrs.push((key.clone(), value.clone()));
        }
        attrs.push((local(&key).to_string(), value));
    }
    Elem {
        name: local(e.name().as_ref()).to_lowercase(),
        attrs,
        children: Vec::new(),
    }
}

/// `dest` and `frag` of an href relative to the TOC file.
fn target(href: &str, toc_name: &str) -> (Option<String>, Option<String>) {
    let (path, frag) = match href.split_once('#') {
        Some((p, f)) => (p, Some(f.to_string()).filter(|f| !f.is_empty())),
        None => (href, None),
    };
    let dest = if path.is_empty() {
        toc_name.to_string()
    } else {
        let base = toc_name.rsplit_once('/').map_or("", |(d, _)| d);
        join(base, &percent_decode(path))
    };
    (Some(dest), frag)
}

fn title(raw: String) -> Option<String> {
    let t = raw.trim();
    (!t.is_empty()).then(|| t.to_string())
}

/// calibre's `parse_ncx`: navPoints under the first navMap, the title being
/// every `navLabel/text`'s text joined.
pub fn parse_ncx(bytes: &[u8], ncx_name: &str) -> Vec<TocNode> {
    let Some(root) = parse_tree(bytes) else {
        return Vec::new();
    };
    let mut all = Vec::new();
    root.descendants(&mut all);
    let Some(navmap) = all.into_iter().find(|e| e.name == "navmap") else {
        return Vec::new();
    };
    fn walk(node: &Elem, ncx_name: &str) -> Vec<TocNode> {
        node.named("navpoint")
            .map(|np| {
                let label = np.named("navlabel").next();
                let text = label.map(|l| l.named("text").map(|t| t.text()).collect::<String>());
                let (dest, frag) = np
                    .named("content")
                    .next()
                    .and_then(|c| c.attr("src"))
                    .filter(|s| !s.is_empty())
                    .map_or((None, None), |src| target(src, ncx_name));
                TocNode {
                    title: text.and_then(title),
                    dest,
                    frag,
                    children: walk(np, ncx_name),
                }
            })
            .collect()
    }
    walk(navmap, ncx_name)
}

/// calibre's `parse_nav`: the first `<nav epub:type="toc">` with an `<ol>`,
/// each `<li>` titled by its first `<a>` or `<span>`. `None` if there is no
/// such nav.
pub fn parse_nav(bytes: &[u8], nav_name: &str) -> Option<Vec<TocNode>> {
    let root = parse_tree(bytes)?;
    let mut all = Vec::new();
    root.descendants(&mut all);
    let nav = all.into_iter().find(|e| {
        e.name == "nav" && e.attr("epub:type") == Some("toc") && e.named("ol").next().is_some()
    })?;
    fn walk(ol: &Elem, nav_name: &str) -> Vec<TocNode> {
        ol.named("li")
            .map(|li| {
                let head = li.elems().find(|e| e.name == "a" || e.name == "span");
                let text = head.map(|h| {
                    let t = h.text();
                    if t.trim().is_empty() {
                        let mut d = vec![h];
                        h.descendants(&mut d);
                        d.iter()
                            .filter_map(|e| e.attr("title"))
                            .collect::<Vec<_>>()
                            .join(" ")
                    } else {
                        t
                    }
                });
                let (dest, frag) = head
                    .and_then(|h| h.attr("href"))
                    .map_or((None, None), |href| target(href, nav_name));
                let children = li
                    .named("ol")
                    .next()
                    .map(|o| walk(o, nav_name))
                    .unwrap_or_default();
                TocNode {
                    title: text.and_then(title),
                    dest,
                    frag,
                    children,
                }
            })
            .collect()
    }
    Some(walk(nav.named("ol").next()?, nav_name))
}

/// Every node in reading order of the TOC (preorder), with the titles of the
/// node and its ancestors as calibre's viewer collects them: climbing from the
/// node, stopping at the first node without a title, outermost first.
pub fn flatten(toc: &[TocNode]) -> Vec<(&TocNode, Vec<String>)> {
    fn walk<'a>(
        nodes: &'a [TocNode],
        chain: &[Option<String>],
        out: &mut Vec<(&'a TocNode, Vec<String>)>,
    ) {
        for n in nodes {
            let mut here = chain.to_vec();
            here.push(n.title.clone());
            let family: Vec<String> = here
                .iter()
                .rev()
                .map_while(|t| t.clone())
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect();
            out.push((n, family));
            walk(&n.children, &here, out);
        }
    }
    let mut out = Vec::new();
    walk(toc, &[], &mut out);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ncx_titles_and_targets() {
        let ncx = br#"<ncx xmlns="http://www.daisy.org/z3986/2005/ncx/"><navMap>
            <navPoint><navLabel><text>  Part One </text></navLabel><content src="text/part1.xhtml"/>
              <navPoint><navLabel><text>Chapter 1</text></navLabel><content src="text/ch1.xhtml#start"/></navPoint>
            </navPoint></navMap></ncx>"#;
        let toc = parse_ncx(ncx, "OEBPS/toc.ncx");
        assert_eq!(toc[0].title.as_deref(), Some("Part One"));
        assert_eq!(toc[0].dest.as_deref(), Some("OEBPS/text/part1.xhtml"));
        let ch = &toc[0].children[0];
        assert_eq!(
            (ch.dest.as_deref(), ch.frag.as_deref()),
            (Some("OEBPS/text/ch1.xhtml"), Some("start"))
        );
        let flat = flatten(&toc);
        assert_eq!(flat[1].1, vec!["Part One", "Chapter 1"]);
    }

    #[test]
    fn nav_titles_targets_and_untitled_ancestors() {
        let nav = br##"<html xmlns="http://www.w3.org/1999/xhtml" xmlns:epub="http://www.idpf.org/2007/ops"><body>
            <nav epub:type="landmarks"><ol><li><a href="x.xhtml">Ignored</a></li></ol></nav>
            <nav epub:type="toc"><ol>
              <li><span></span><ol><li><a href="ch1.xhtml#s1">Rain &amp; <em>Shine</em></a></li></ol></li>
              <li><a href="#here">Here</a></li>
            </ol></nav></body></html>"##;
        let toc = parse_nav(nav, "OEBPS/nav.xhtml").unwrap();
        assert_eq!(toc[0].title, None);
        let child = &toc[0].children[0];
        assert_eq!(child.title.as_deref(), Some("Rain & Shine"));
        assert_eq!(
            (child.dest.as_deref(), child.frag.as_deref()),
            (Some("OEBPS/ch1.xhtml"), Some("s1"))
        );
        assert_eq!(toc[1].dest.as_deref(), Some("OEBPS/nav.xhtml"));
        // An untitled ancestor ends the family, as in calibre's viewer.
        let flat = flatten(&toc);
        assert_eq!(flat[1].1, vec!["Rain & Shine"]);
    }
}
