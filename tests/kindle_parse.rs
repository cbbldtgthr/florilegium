use annot_archive::id::{EntryKind, Position};
use annot_archive::kindle::parse::{ParseConfig, Parsed, RawClipping, parse};

const FIXTURE: &str = include_str!("fixtures/kindle/synthetic.txt");

fn parsed() -> Parsed {
    parse(FIXTURE, &ParseConfig::default()).unwrap()
}

fn find<'a>(p: &'a Parsed, text_start: &str) -> &'a RawClipping {
    p.clippings
        .iter()
        .find(|c| c.text.starts_with(text_start))
        .unwrap_or_else(|| panic!("no clipping starting {text_start:?}"))
}

#[test]
fn counts_every_block() {
    let p = parsed();
    assert_eq!(p.clippings.len(), 12);
    assert_eq!(p.bookmarks, 1);
    assert_eq!(p.stubs, 1);
    // unparseable header, empty highlight, and the bad date (which still keeps its clipping)
    assert_eq!(p.warnings.len(), 3, "{:#?}", p.warnings);
}

#[test]
fn file_order_is_kept() {
    let p = parsed();
    let indexes: Vec<usize> = p.clippings.iter().map(|c| c.index).collect();
    assert_eq!(indexes, (0..12).collect::<Vec<_>>());
    assert!(p.clippings.windows(2).all(|w| w[0].line < w[1].line));
}

#[test]
fn page_and_location_header() {
    let p = parsed();
    let c = find(&p, "The tide keeps");
    assert_eq!(c.line, 1);
    assert_eq!(c.title_line, "The Lantern Keeper's Almanac (Vell, Orrin)");
    assert_eq!(c.title, "The Lantern Keeper's Almanac");
    assert_eq!(c.author.as_deref(), Some("Vell, Orrin"));
    assert_eq!(c.kind, EntryKind::Highlight);
    assert_eq!(c.page, Some((57, 57)));
    assert_eq!(c.location, Some((1234, 1236)));
    assert_eq!(
        c.position(),
        Position::Location {
            start: 1234,
            end: 1236
        }
    );
    assert_eq!(c.raw_added_on, "Friday, 23 October 2026 15:50:12");
    // 15:50:12 in Johannesburg (UTC+2)
    assert_eq!(c.created.unwrap().to_string(), "2026-10-23T13:50:12Z");
}

#[test]
fn single_digit_day_parses() {
    let p = parsed();
    let c = find(&p, "A lamp is a promise");
    assert_eq!(c.location, Some((1300, 1302)));
    assert_eq!(c.page, None);
    assert_eq!(c.created.unwrap().to_string(), "2026-10-05T05:03:09Z");
}

#[test]
fn title_with_parentheses_keeps_them() {
    let p = parsed();
    let c = find(&p, "Salt remembers");
    assert_eq!(c.title, "Tidewater Sketches (and Other Shoreline Notes)");
    assert_eq!(c.author.as_deref(), Some("Marsh, Ilse"));
}

#[test]
fn several_authors_stay_one_raw_string() {
    let p = parsed();
    let c = find(&p, "Every map is");
    assert_eq!(c.author.as_deref(), Some("Doe, Jane; Roe, Rick"));
}

#[test]
fn missing_and_empty_author() {
    let p = parsed();
    let c = find(&p, "The hill was smaller");
    assert_eq!(
        (c.title.as_str(), c.author.as_deref()),
        ("Untitled Field Notes", None)
    );
    let c = find(&p, "Dig where the reeds");
    assert_eq!(
        (c.title.as_str(), c.author.as_deref()),
        ("Pamphlet On Wells", None)
    );
}

#[test]
fn page_only_range() {
    let p = parsed();
    let c = find(&p, "Gulls argue");
    assert_eq!(c.page, Some((12, 13)));
    assert_eq!(c.location, None);
    assert_eq!(c.position(), Position::Page { start: 12, end: 13 });
}

#[test]
fn notes_and_multiline_bodies() {
    let p = parsed();
    let c = find(&p, "Compare with the harbour");
    assert_eq!(c.kind, EntryKind::Note);
    assert_eq!(c.location, Some((1236, 1236)));
    let c = find(&p, "First thought.");
    assert_eq!(c.text, "First thought.\nSecond thought, on its own line.");
}

#[test]
fn mid_file_bom_does_not_split_the_book() {
    let p = parsed();
    let c = find(&p, "Night is only");
    assert_eq!(c.title_line, find(&p, "The tide keeps").title_line);
}

#[test]
fn capitalised_keyword_and_non_bmp_text() {
    let p = parsed();
    let c = find(&p, "The compass rose");
    assert_eq!(c.location, Some((30, 31)));
    assert_eq!(c.page, Some((4, 4)));
    assert!(c.text.contains('\u{1F9ED}'));
}

#[test]
fn bad_date_is_a_warning_and_null_created() {
    let p = parsed();
    let c = find(&p, "A date written");
    assert_eq!(c.created, None);
    assert_eq!(c.raw_added_on, "2026-01-04 10:15");
    assert!(
        p.warnings
            .iter()
            .any(|w| w.line == c.line && w.message.contains("matches no configured format"))
    );
}

#[test]
fn unparseable_and_empty_blocks_are_warnings_with_lines() {
    let p = parsed();
    let header = p
        .warnings
        .iter()
        .find(|w| w.message.contains("header does not start"))
        .unwrap();
    assert!(
        header
            .raw
            .starts_with("Untitled Field Notes\nthis line is not")
    );
    assert_eq!(
        FIXTURE.lines().nth(header.line - 1),
        Some("Untitled Field Notes")
    );
    let empty = p
        .warnings
        .iter()
        .find(|w| w.message.contains("has no text"))
        .unwrap();
    assert!(
        FIXTURE
            .lines()
            .nth(empty.line)
            .unwrap()
            .contains("location 20-21")
    );
}

#[test]
fn crlf_parses_identically() {
    let crlf = FIXTURE.replace('\n', "\r\n");
    let a = parsed();
    let b = parse(&crlf, &ParseConfig::default()).unwrap();
    assert_eq!(a.clippings, b.clippings);
    assert_eq!((a.bookmarks, a.stubs), (b.bookmarks, b.stubs));
    assert_eq!(a.warnings.len(), b.warnings.len());
}

#[test]
fn keywords_and_formats_come_from_config() {
    let input = "Livre (Auteur)\n- Votre surlignement à l'emplacement 10-12 | Ajouté le dimanche 4 janvier 2026 10:00:00\n\nTexte.\n==========\n";
    let config = ParseConfig {
        header_prefix: "- Votre ".into(),
        highlight: "surlignement".into(),
        location: "l'emplacement".into(),
        added_on: "Ajouté le".into(),
        date_formats: vec!["%A %d %B %Y %H:%M:%S".into()],
        date_words: vec![
            ("dimanche".into(), "Sunday".into()),
            ("janvier".into(), "January".into()),
        ],
        timezone: "Europe/Paris".into(),
        ..ParseConfig::default()
    };
    let p = parse(input, &config).unwrap();
    assert!(p.warnings.is_empty(), "{:#?}", p.warnings);
    let c = &p.clippings[0];
    assert_eq!(c.location, Some((10, 12)));
    assert_eq!(c.created.unwrap().to_string(), "2026-01-04T09:00:00Z");
}

#[test]
fn unknown_timezone_is_an_error() {
    let config = ParseConfig {
        timezone: "Nowhere/Atlantis".into(),
        ..ParseConfig::default()
    };
    assert!(parse(FIXTURE, &config).is_err());
}
