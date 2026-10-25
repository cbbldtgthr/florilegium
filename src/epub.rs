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
    /// The OPF package's major version (2 or 3).
    pub version: u32,
    /// The EPUB 3 navigation document (manifest item with the `nav`
    /// property), if any.
    pub nav: Option<String>,
    /// The EPUB 2 NCX (the spine's `toc` item, else the manifest's NCX), if any.
    pub ncx: Option<String>,
}

impl Epub {
    pub fn open(path: &Path) -> Result<Epub, String> {
        let file = File::open(path).map_err(|e| format!("cannot open {}: {e}", path.display()))?;
        let mut zip =
            ZipArchive::new(file).map_err(|e| format!("{} is not a zip: {e}", path.display()))?;
        let container = read_entry(&mut zip, "META-INF/container.xml")?;
        let opf_path = rootfile(&container).ok_or("META-INF/container.xml names no rootfile")?;
        let opf = read_entry(&mut zip, &opf_path)?;
        let p = package(&opf, &opf_path)?;
        Ok(Epub {
            zip,
            spine: p.spine,
            version: p.version,
            nav: p.nav,
            ncx: p.ncx,
        })
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

pub(crate) fn local(name: &str) -> &str {
    name.rsplit(':').next().unwrap_or(name)
}

pub(crate) fn attr(e: &quick_xml::events::BytesStart, key: &str) -> Option<String> {
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

struct Package {
    spine: Vec<SpineItem>,
    version: u32,
    nav: Option<String>,
    ncx: Option<String>,
}

struct ManifestItem {
    id: String,
    name: String,
    media_type: String,
    properties: String,
}

fn package(opf: &[u8], opf_path: &str) -> Result<Package, String> {
    let base = opf_path.rsplit_once('/').map_or("", |(dir, _)| dir);
    let mut manifest: Vec<ManifestItem> = Vec::new();
    let mut idrefs: Vec<String> = Vec::new();
    let mut version = 2;
    let mut spine_toc = None;
    let mut r = Reader::from_reader(opf);
    loop {
        match r.read_event() {
            Ok(Event::Start(e) | Event::Empty(e)) => match local(e.name().as_ref()) {
                "package" => {
                    let v = attr(&e, "version").unwrap_or_default();
                    version = v
                        .split('.')
                        .next()
                        .and_then(|m| m.trim().parse().ok())
                        .unwrap_or(2);
                }
                "item" => {
                    if let (Some(id), Some(href)) = (attr(&e, "id"), attr(&e, "href")) {
                        manifest.push(ManifestItem {
                            id,
                            name: join(base, &percent_decode(&href)),
                            media_type: attr(&e, "media-type").unwrap_or_default(),
                            properties: attr(&e, "properties").unwrap_or_default(),
                        });
                    }
                }
                "spine" => spine_toc = attr(&e, "toc"),
                "itemref" => idrefs.extend(attr(&e, "idref")),
                _ => {}
            },
            Ok(Event::Eof) => break,
            Err(e) => return Err(format!("{opf_path} is not valid XML: {e}")),
            _ => {}
        }
    }
    let spine = idrefs
        .iter()
        .enumerate()
        .filter_map(|(index, idref)| {
            let item = manifest.iter().find(|m| m.id == *idref)?;
            Some(SpineItem {
                index,
                name: item.name.clone(),
                media_type: item.media_type.clone(),
            })
        })
        .collect();
    // calibre's find_existing_nav_toc and find_existing_ncx_toc.
    let nav = manifest
        .iter()
        .find(|m| m.properties.split_whitespace().any(|p| p == "nav"))
        .map(|m| m.name.clone());
    let ncx = spine_toc
        .and_then(|id| manifest.iter().find(|m| m.id == id))
        .or_else(|| {
            manifest
                .iter()
                .find(|m| m.media_type == "application/x-dtbncx+xml")
        })
        .map(|m| m.name.clone());
    Ok(Package {
        spine,
        version,
        nav,
        ncx,
    })
}

/// `base/href` with `.` and `..` resolved, as a zip entry name.
pub(crate) fn join(base: &str, href: &str) -> String {
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

pub(crate) fn percent_decode(s: &str) -> String {
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
