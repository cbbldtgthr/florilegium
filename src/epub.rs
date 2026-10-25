//! Just enough EPUB to find a book's spine documents, named the way calibre
//! names them in an annotation's `spine_name` (the path inside the zip).

use std::fs::File;
use std::io::Read;
use std::path::Path;

use quick_xml::Reader;
use quick_xml::events::Event;
use zip::ZipArchive;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpineItem {
    /// Position in the OPF spine, counting every itemref.
    pub index: usize,
    /// Path inside the zip, e.g. `OEBPS/xhtml/ch01.xhtml`.
    pub name: String,
    pub media_type: String,
}

pub struct Epub {
    zip: ZipArchive<File>,
    pub spine: Vec<SpineItem>,
}

impl Epub {
    pub fn open(path: &Path) -> Result<Epub, String> {
        let file = File::open(path).map_err(|e| format!("cannot open {}: {e}", path.display()))?;
        let mut zip =
            ZipArchive::new(file).map_err(|e| format!("{} is not a zip: {e}", path.display()))?;
        let container = read_entry(&mut zip, "META-INF/container.xml")?;
        let opf_path = rootfile(&container).ok_or("META-INF/container.xml names no rootfile")?;
        let opf = read_entry(&mut zip, &opf_path)?;
        let spine = spine(&opf, &opf_path)?;
        Ok(Epub { zip, spine })
    }

    pub fn read(&mut self, name: &str) -> Result<Vec<u8>, String> {
        read_entry(&mut self.zip, name)
    }
}

fn read_entry(zip: &mut ZipArchive<File>, name: &str) -> Result<Vec<u8>, String> {
    let mut entry = zip.by_name(name).map_err(|e| format!("{name}: {e}"))?;
    let mut out = Vec::new();
    entry
        .read_to_end(&mut out)
        .map_err(|e| format!("{name}: {e}"))?;
    Ok(out)
}

fn local(name: &str) -> &str {
    name.rsplit(':').next().unwrap_or(name)
}

fn attr(e: &quick_xml::events::BytesStart, key: &str) -> Option<String> {
    e.attributes()
        .flatten()
        .find(|a| local(a.key.as_ref()) == key)
        .and_then(|a| {
            a.normalized_value(quick_xml::XmlVersion::Implicit1_0)
                .ok()
                .map(|v| v.into_owned())
        })
}

fn rootfile(container: &[u8]) -> Option<String> {
    let mut r = Reader::from_reader(container);
    loop {
        match r.read_event() {
            Ok(Event::Start(e) | Event::Empty(e)) if local(e.name().as_ref()) == "rootfile" => {
                return attr(&e, "full-path");
            }
            Ok(Event::Eof) | Err(_) => return None,
            _ => {}
        }
    }
}

fn spine(opf: &[u8], opf_path: &str) -> Result<Vec<SpineItem>, String> {
    let base = opf_path.rsplit_once('/').map_or("", |(dir, _)| dir);
    let mut manifest: Vec<(String, String, String)> = Vec::new();
    let mut idrefs: Vec<String> = Vec::new();
    let mut r = Reader::from_reader(opf);
    loop {
        match r.read_event() {
            Ok(Event::Start(e) | Event::Empty(e)) => match local(e.name().as_ref()) {
                "item" => {
                    if let (Some(id), Some(href)) = (attr(&e, "id"), attr(&e, "href")) {
                        manifest.push((id, href, attr(&e, "media-type").unwrap_or_default()));
                    }
                }
                "itemref" => idrefs.extend(attr(&e, "idref")),
                _ => {}
            },
            Ok(Event::Eof) => break,
            Err(e) => return Err(format!("{opf_path} is not valid XML: {e}")),
            _ => {}
        }
    }
    Ok(idrefs
        .iter()
        .enumerate()
        .filter_map(|(index, idref)| {
            let (_, href, media_type) = manifest.iter().find(|(id, _, _)| id == idref)?;
            Some(SpineItem {
                index,
                name: join(base, &percent_decode(href)),
                media_type: media_type.clone(),
            })
        })
        .collect())
}

/// `base/href` with `.` and `..` resolved, as a zip entry name.
fn join(base: &str, href: &str) -> String {
    let href = href.split(['#', '?']).next().unwrap_or(href);
    let mut parts: Vec<&str> = base.split('/').filter(|p| !p.is_empty()).collect();
    for p in href.split('/') {
        match p {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            p => parts.push(p),
        }
    }
    parts.join("/")
}

fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && let Some(h) = s
                .get(i + 1..i + 3)
                .and_then(|h| u8::from_str_radix(h, 16).ok())
        {
            out.push(h);
            i += 3;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8(out).unwrap_or_else(|_| s.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hrefs_resolve_like_zip_names() {
        assert_eq!(join("OEBPS", "xhtml/ch01.xhtml"), "OEBPS/xhtml/ch01.xhtml");
        assert_eq!(
            join("OEBPS/content", "../text/a%20b.html"),
            "OEBPS/text/a%20b.html"
        );
        assert_eq!(join("", "text/part0003.html#start"), "text/part0003.html");
        assert_eq!(percent_decode("a%20b%C3%A9.html"), "a b\u{e9}.html");
    }
}
