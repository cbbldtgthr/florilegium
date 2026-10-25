//! An XHTML spine document as calibre's viewer will hold it in its DOM.
//!
//! calibre (`ebooks/oeb/polish/parsing.py`) replaces HTML named entities,
//! then parses strictly as XML, falling back to an HTML5 tag-soup parse only
//! when that fails or the root is not `<html>` in the XHTML namespace. The
//! fallback's tree can differ, so such documents are refused here rather than
//! guessed at. calibre's renderer (`srv/render_book.py`) then keeps only one
//! `<body>` and only `<head>`/`<body>` under `<html>`.

use quick_xml::Reader;
use quick_xml::events::{BytesStart, Event};

const XHTML_NS: &str = "http://www.w3.org/1999/xhtml";

pub type NodeId = usize;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Kind {
    Document,
    Element {
        name: String,
        id: Option<String>,
    },
    Text(String),
    /// Comments and processing instructions: nodes, but neither element nor text.
    Other,
}

#[derive(Debug, Clone)]
pub struct Node {
    pub kind: Kind,
    pub parent: Option<NodeId>,
    pub children: Vec<NodeId>,
}

#[derive(Debug, Clone)]
pub struct Dom {
    pub nodes: Vec<Node>,
}

pub const DOCUMENT: NodeId = 0;

impl Dom {
    pub fn parse(bytes: &[u8]) -> Result<Dom, String> {
        let text = std::str::from_utf8(bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(bytes))
            .map_err(|_| "not UTF-8".to_string())?;
        let mut dom = Dom {
            nodes: vec![Node {
                kind: Kind::Document,
                parent: None,
                children: Vec::new(),
            }],
        };
        let mut reader = Reader::from_str(text);
        reader.config_mut().check_end_names = true;
        let mut stack: Vec<NodeId> = vec![DOCUMENT];
        let mut root_checked = false;
        loop {
            let event = reader
                .read_event()
                .map_err(|e| format!("not well-formed XML: {e}"))?;
            let top = *stack.last().unwrap();
            match event {
                Event::Start(e) => {
                    if !root_checked {
                        check_root(&e)?;
                        root_checked = true;
                    }
                    let id = dom.push_element(top, &e)?;
                    stack.push(id);
                }
                Event::Empty(e) => {
                    if !root_checked {
                        check_root(&e)?;
                        root_checked = true;
                    }
                    dom.push_element(top, &e)?;
                }
                Event::End(_) => {
                    stack.pop();
                }
                Event::Text(t) if top != DOCUMENT => {
                    let s = t.xml10_content();
                    dom.push_text(top, &s);
                }
                Event::CData(t) if top != DOCUMENT => {
                    dom.push_text(top, &t.into_inner());
                }
                Event::GeneralRef(r) if top != DOCUMENT => {
                    let s = resolve_ref(&r)?;
                    dom.push_text(top, &s);
                }
                Event::Comment(_) | Event::PI(_) if top != DOCUMENT => {
                    dom.push(top, Kind::Other);
                }
                Event::Eof => break,
                _ => {}
            }
        }
        if !root_checked {
            return Err("no root element".into());
        }
        dom.as_rendered();
        Ok(dom)
    }

    fn push(&mut self, parent: NodeId, kind: Kind) -> NodeId {
        let id = self.nodes.len();
        self.nodes.push(Node {
            kind,
            parent: Some(parent),
            children: Vec::new(),
        });
        self.nodes[parent].children.push(id);
        id
    }

    fn push_element(&mut self, parent: NodeId, e: &BytesStart) -> Result<NodeId, String> {
        let name = local(e.name().as_ref()).to_string();
        let mut id = None;
        for a in e.attributes() {
            let a = a.map_err(|e| format!("bad attribute: {e}"))?;
            if a.key.as_ref() == "id" {
                let v = a
                    .normalized_value_with(quick_xml::XmlVersion::Implicit1_0, 128, entity)
                    .map_err(|e| format!("bad id attribute: {e}"))?;
                id = Some(v.into_owned());
            }
        }
        Ok(self.push(parent, Kind::Element { name, id }))
    }

    /// Text joins a directly preceding text node, as in a parsed DOM.
    fn push_text(&mut self, parent: NodeId, s: &str) {
        if let Some(&last) = self.nodes[parent].children.last()
            && let Kind::Text(t) = &mut self.nodes[last].kind
        {
            t.push_str(s);
            return;
        }
        self.push(parent, Kind::Text(s.to_string()));
    }

    /// calibre's `html_as_json`: one `<body>` (extra bodies become `<div>`s
    /// inside the first), only `<head>`/`<body>` elements under `<html>`, and no
    /// text before `<head>`.
    fn as_rendered(&mut self) {
        let html = self.nodes[DOCUMENT].children[0];
        let bodies: Vec<NodeId> = self.children_named(html, "body");
        if bodies.is_empty() {
            self.push(
                html,
                Kind::Element {
                    name: "body".into(),
                    id: None,
                },
            );
        } else {
            let first = bodies[0];
            for &b in &bodies[1..] {
                self.nodes[html].children.retain(|&c| c != b);
                if let Kind::Element { name, .. } = &mut self.nodes[b].kind {
                    *name = "div".into();
                }
                self.nodes[b].parent = Some(first);
                self.nodes[first].children.push(b);
            }
        }
        // Remove other elements under <html> together with the text that
        // follows them (lxml's tail), and the text before the first element.
        let kids = self.nodes[html].children.clone();
        let mut keep = Vec::new();
        let mut dropping_tail = false;
        for (i, &c) in kids.iter().enumerate() {
            match &self.nodes[c].kind {
                Kind::Element { name, .. } => {
                    dropping_tail = name != "head" && name != "body";
                    if !dropping_tail {
                        keep.push(c);
                    }
                }
                Kind::Text(_) if dropping_tail => {}
                Kind::Text(_) if keep.is_empty() && i == 0 => {}
                _ => keep.push(c),
            }
        }
        self.nodes[html].children = keep;
    }

    fn children_named(&self, parent: NodeId, wanted: &str) -> Vec<NodeId> {
        self.nodes[parent]
            .children
            .iter()
            .copied()
            .filter(
                |&c| matches!(&self.nodes[c].kind, Kind::Element { name, .. } if name == wanted),
            )
            .collect()
    }

    pub fn is_element(&self, n: NodeId) -> bool {
        matches!(self.nodes[n].kind, Kind::Element { .. })
    }

    pub fn element_name(&self, n: NodeId) -> Option<&str> {
        match &self.nodes[n].kind {
            Kind::Element { name, .. } => Some(name),
            _ => None,
        }
    }

    pub fn text(&self, n: NodeId) -> Option<&str> {
        match &self.nodes[n].kind {
            Kind::Text(t) => Some(t),
            _ => None,
        }
    }

    /// `<body>`, which `as_rendered` guarantees.
    pub fn body(&self) -> NodeId {
        let html = self.nodes[DOCUMENT].children[0];
        self.children_named(html, "body")[0]
    }

    /// How many elements carry this id; the viewer asserts ids only when unique.
    pub fn id_count(&self, id: &str) -> usize {
        self.nodes
            .iter()
            .filter(|n| matches!(&n.kind, Kind::Element { id: Some(i), .. } if i == id))
            .count()
    }
}

fn local(name: &str) -> &str {
    name.rsplit(':').next().unwrap_or(name)
}

fn check_root(e: &BytesStart) -> Result<(), String> {
    if e.name().as_ref() != "html" {
        return Err("root is not <html> (calibre would parse it as tag soup)".into());
    }
    let ns = e
        .attributes()
        .flatten()
        .find(|a| a.key.as_ref() == "xmlns")
        .map(|a| a.value.to_string());
    if ns.as_deref() != Some(XHTML_NS) {
        return Err(
            "root <html> is not in the XHTML namespace (calibre would parse it as tag soup)".into(),
        );
    }
    Ok(())
}

/// An entity's expansion: XML's five, then the HTML named-entity table that
/// calibre substitutes before parsing.
fn entity(name: &str) -> Option<&'static str> {
    Some(match name {
        "amp" => "&",
        "lt" => "<",
        "gt" => ">",
        "quot" => "\"",
        "apos" => "'",
        _ => {
            let key = format!("&{name};");
            let bytes = htmlize::ENTITIES.get(key.as_bytes())?;
            std::str::from_utf8(bytes).ok()?
        }
    })
}

fn resolve_ref(r: &quick_xml::events::BytesRef) -> Result<String, String> {
    if let Some(c) = r
        .resolve_char_ref()
        .map_err(|e| format!("bad character reference: {e}"))?
    {
        return Ok(c.to_string());
    }
    let name = r.xml10_content();
    entity(&name)
        .map(String::from)
        .ok_or_else(|| format!("unknown entity &{name};"))
}
