//! CFIs worked out by hand from the rules in calibre's `cfi.pyj` for small
//! invented documents. The real-library check (`just cfi-selfcheck`) is what
//! ties these rules to calibre's actual output.

use std::io::Write;

use florilegium::cfi::{Haystack, encode};
use florilegium::epub::Epub;
use florilegium::norm::norm_v1;
use florilegium::xhtml::Dom;

const HEAD: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<!DOCTYPE html PUBLIC "-//W3C//DTD XHTML 1.1//EN" "http://www.w3.org/TR/xhtml11/DTD/xhtml11.dtd">
<html xmlns="http://www.w3.org/1999/xhtml"><head><title>Unseen title</title><style>p { color: red }</style></head>"#;

fn doc(body: &str) -> Dom {
    Dom::parse(format!("{HEAD}{body}</html>").as_bytes()).unwrap()
}

/// CFIs of every occurrence of `needle` (normalised like a highlight).
fn cfis(dom: &Dom, needle: &str) -> Vec<(String, String)> {
    let hay = Haystack::new(dom);
    let needle: Vec<char> = norm_v1(needle).chars().collect();
    hay.find_all(&needle)
        .into_iter()
        .map(|r| {
            let (s, e) = hay.points(dom, r).unwrap();
            (encode(dom, s), encode(dom, e))
        })
        .collect()
}

fn one(dom: &Dom, needle: &str) -> (String, String) {
    let all = cfis(dom, needle);
    assert_eq!(all.len(), 1, "{needle:?}: {all:?}");
    all.into_iter().next().unwrap()
}

const BODY: &str = concat!(
    r#"<body><p id="a">Alpha <em>beta</em> gamma&nbsp;delta.</p>"#,
    "<p>Second &#x1F9ED; para.</p><!-- a comment -->",
    r#"<p id="dup">x one</p><p id="dup">y two</p></body>"#
);

#[test]
fn text_in_a_paragraph() {
    let d = doc(BODY);
    assert_eq!(
        one(&d, "Alpha"),
        ("/2/4/2[a]/1:0".into(), "/2/4/2[a]/1:5".into())
    );
}

#[test]
fn text_after_an_inline_element_counts_only_direct_text() {
    let d = doc(BODY);
    // "Alpha " (6) precedes; "beta" inside <em> is not counted.
    assert_eq!(
        one(&d, "gamma delta."),
        ("/2/4/2[a]/1:7".into(), "/2/4/2[a]/1:19".into())
    );
}

#[test]
fn text_inside_an_inline_element() {
    let d = doc(BODY);
    assert_eq!(
        one(&d, "beta"),
        ("/2/4/2[a]/2/1:0".into(), "/2/4/2[a]/2/1:4".into())
    );
}

#[test]
fn offsets_are_utf16_code_units() {
    let d = doc(BODY);
    // "Second " is 7 units and the compass emoji 2.
    assert_eq!(
        one(&d, "para."),
        ("/2/4/4/1:10".into(), "/2/4/4/1:15".into())
    );
}

#[test]
fn comments_take_an_odd_step_and_duplicate_ids_are_not_asserted() {
    let d = doc(BODY);
    assert_eq!(one(&d, "x one"), ("/2/4/6/1:0".into(), "/2/4/6/1:5".into()));
    assert_eq!(one(&d, "y two"), ("/2/4/8/1:0".into(), "/2/4/8/1:5".into()));
}

#[test]
fn a_passage_can_cross_paragraphs() {
    let d = doc(BODY);
    assert_eq!(
        one(&d, "delta. Second"),
        ("/2/4/2[a]/1:13".into(), "/2/4/4/1:6".into())
    );
}

#[test]
fn a_parent_starting_with_an_element_names_that_element() {
    let d = doc("<body><p><b>Bold</b> after</p></body>");
    assert_eq!(one(&d, "after"), ("/2/4/2/2:1".into(), "/2/4/2/2:6".into()));
}

#[test]
fn whitespace_text_nodes_count_like_any_other() {
    let d = doc("<body>\n  <p>One</p>\n  <p>Two</p>\n</body>");
    assert_eq!(one(&d, "Two"), ("/2/4/4/1:0".into(), "/2/4/4/1:3".into()));
}

#[test]
fn head_and_style_text_is_never_found() {
    let d = doc(BODY);
    assert!(cfis(&d, "Unseen").is_empty());
    assert!(cfis(&d, "color").is_empty());
}

#[test]
fn repeated_text_yields_every_occurrence() {
    let d = doc("<body><p>echo</p><p>echo</p></body>");
    assert_eq!(cfis(&d, "echo").len(), 2);
}

#[test]
fn ids_are_escaped_like_the_viewer() {
    let d = doc(r#"<body><p id="a(b)[c]">Text</p></body>"#);
    assert_eq!(one(&d, "Text").0, "/2/4/2[a^(b^)^[c^]]/1:0");
}

#[test]
fn rendering_keeps_one_body_and_only_head_and_body() {
    let d = Dom::parse(
        concat!(
            r#"<html xmlns="http://www.w3.org/1999/xhtml">lead<head/><body><p>One</p></body>"#,
            "<foo>dropped</foo>tail<body><p>Two</p></body></html>"
        )
        .as_bytes(),
    )
    .unwrap();
    // The second <body> becomes a <div> after the first body's <p>.
    assert_eq!(one(&d, "Two").0, "/2/4/4/2/1:0");
    assert!(cfis(&d, "dropped").is_empty());
    assert!(cfis(&d, "tail").is_empty());
}

#[test]
fn documents_calibre_would_not_parse_as_xml_are_refused() {
    let cases = [
        ("<html><body/></html>", "XHTML namespace"),
        (
            r#"<svg xmlns="http://www.w3.org/2000/svg"/>"#,
            "root is not <html>",
        ),
        (
            r#"<html xmlns="http://www.w3.org/1999/xhtml"><body><p>open</body></html>"#,
            "not well-formed",
        ),
        (
            r#"<html xmlns="http://www.w3.org/1999/xhtml"><body>&bogus;</body></html>"#,
            "unknown entity",
        ),
    ];
    for (xhtml, want) in cases {
        let err = Dom::parse(xhtml.as_bytes()).unwrap_err();
        assert!(err.contains(want), "{xhtml}: {err}");
    }
}

#[test]
fn epub_spine_names_match_zip_paths() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("book.epub");
    let mut zip = zip::ZipWriter::new(std::fs::File::create(&path).unwrap());
    let stored =
        zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    let mut put = |name: &str, body: &str| {
        zip.start_file(name, stored).unwrap();
        zip.write_all(body.as_bytes()).unwrap();
    };
    put("mimetype", "application/epub+zip");
    put(
        "META-INF/container.xml",
        r#"<container xmlns="urn:oasis:names:tc:opendocument:xmlns:container"><rootfiles><rootfile full-path="OEBPS/content.opf"/></rootfiles></container>"#,
    );
    put(
        "OEBPS/content.opf",
        r#"<package xmlns="http://www.idpf.org/2007/opf"><manifest>
            <item id="c1" href="text/chapter%20one.xhtml" media-type="application/xhtml+xml"/>
            <item id="c2" href="text/../text/two.xhtml" media-type="application/xhtml+xml"/>
           </manifest><spine><itemref idref="c2"/><itemref idref="c1"/></spine></package>"#,
    );
    put(
        "OEBPS/text/chapter one.xhtml",
        &format!("{HEAD}<body><p>One</p></body></html>"),
    );
    put(
        "OEBPS/text/two.xhtml",
        &format!("{HEAD}<body><p>Two</p></body></html>"),
    );
    zip.finish().unwrap();

    let mut epub = Epub::open(&path).unwrap();
    let names: Vec<(usize, &str)> = epub
        .spine
        .iter()
        .map(|s| (s.index, s.name.as_str()))
        .collect();
    assert_eq!(
        names,
        vec![
            (0, "OEBPS/text/two.xhtml"),
            (1, "OEBPS/text/chapter one.xhtml")
        ]
    );
    let dom = Dom::parse(&epub.read("OEBPS/text/chapter one.xhtml").unwrap()).unwrap();
    assert_eq!(one(&dom, "One").0, "/2/4/2/1:0");
}
