//! Parse `My Clippings.txt` into raw clippings, in file order.
//!
//! Nothing is skipped silently: every block becomes a clipping, a counted
//! drop (bookmark, clipping-limit stub), or a warning with its line number.

use jiff::Timestamp;
use jiff::civil::DateTime;
use jiff::tz::TimeZone;

use crate::id::{EntryKind, Position};

/// Locale-dependent words and formats. Defaults are for an English Kindle.
#[derive(Debug, Clone)]
pub struct ParseConfig {
    pub separator: String,
    pub header_prefix: String,
    pub highlight: String,
    pub note: String,
    pub bookmark: String,
    pub location: String,
    pub page: String,
    pub added_on: String,
    /// Body texts marking a clipping-limit stub, compared after trimming.
    pub stub_markers: Vec<String>,
    /// `strptime` formats for the `Added on` date, tried in order.
    pub date_formats: Vec<String>,
    /// Whole words in the date replaced before parsing, e.g. `("janvier",
    /// "January")`, since `strptime` only reads English month and day names.
    pub date_words: Vec<(String, String)>,
    /// IANA zone the Kindle's clock was set to; its dates carry no zone.
    pub timezone: String,
}

impl Default for ParseConfig {
    fn default() -> Self {
        ParseConfig {
            separator: "==========".into(),
            header_prefix: "- Your ".into(),
            highlight: "Highlight".into(),
            note: "Note".into(),
            bookmark: "Bookmark".into(),
            location: "location".into(),
            page: "page".into(),
            added_on: "Added on".into(),
            stub_markers: vec![
                "<You have reached the clipping limit for this item>".into(),
                "You have reached the clipping limit for this item".into(),
            ],
            date_formats: vec!["%A, %d %B %Y %H:%M:%S".into()],
            date_words: Vec::new(),
            timezone: "Africa/Johannesburg".into(),
        }
    }
}

/// One highlight or note as it appears in the file.
#[derive(Debug, Clone, PartialEq)]
pub struct RawClipping {
    /// Position among kept clippings, in file order. Authoritative for recency.
    pub index: usize,
    /// 1-based line number of the title line.
    pub line: usize,
    /// The title line with any BOM removed and whitespace trimmed.
    pub title_line: String,
    /// The title line without its author group.
    pub title: String,
    /// The last parenthesised group of the title line, if non-empty.
    pub author: Option<String>,
    pub kind: EntryKind,
    pub location: Option<(u32, u32)>,
    pub page: Option<(u32, u32)>,
    /// The `Added on` value, verbatim.
    pub raw_added_on: String,
    /// `raw_added_on` read in the Kindle's zone; `None` if no format matched.
    pub created: Option<Timestamp>,
    /// Highlighted text, or the note body for a note.
    pub text: String,
}

impl RawClipping {
    /// The position used for identity: a location wins over a page.
    pub fn position(&self) -> Position {
        match (self.location, self.page) {
            (Some((start, end)), _) => Position::Location { start, end },
            (None, Some((start, end))) => Position::Page { start, end },
            (None, None) => Position::Unknown,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Warning {
    pub line: usize,
    pub message: String,
    /// The offending block, truncated.
    pub raw: String,
}

#[derive(Debug, Default)]
pub struct Parsed {
    pub clippings: Vec<RawClipping>,
    pub bookmarks: usize,
    pub stubs: usize,
    pub warnings: Vec<Warning>,
}

const RAW_EXCERPT_CHARS: usize = 200;

pub fn parse(input: &str, config: &ParseConfig) -> Result<Parsed, String> {
    let tz = TimeZone::get(&config.timezone)
        .map_err(|e| format!("unknown timezone {:?}: {e}", config.timezone))?;
    let mut out = Parsed::default();
    for block in blocks(input, &config.separator) {
        parse_block(&block, config, &tz, &mut out);
    }
    Ok(out)
}

struct Block<'a> {
    /// 1-based line number of the block's first line.
    line: usize,
    lines: Vec<&'a str>,
}

/// Split on separator lines, dropping blank lines that open a block and
/// blocks that hold nothing but whitespace.
fn blocks<'a>(input: &'a str, separator: &str) -> Vec<Block<'a>> {
    let mut out = Vec::new();
    let mut current = Block {
        line: 1,
        lines: Vec::new(),
    };
    for (i, raw_line) in input.lines().enumerate() {
        let line = raw_line.strip_suffix('\r').unwrap_or(raw_line);
        if line.trim_start_matches('\u{FEFF}').trim() == separator {
            out.push(std::mem::replace(
                &mut current,
                Block {
                    line: i + 2,
                    lines: Vec::new(),
                },
            ));
            continue;
        }
        if current.lines.is_empty() && clean(line).is_empty() {
            current.line = i + 2;
            continue;
        }
        current.lines.push(line);
    }
    out.push(current);
    out.retain(|b| !b.lines.is_empty());
    out
}

fn parse_block(block: &Block, config: &ParseConfig, tz: &TimeZone, out: &mut Parsed) {
    let warn = |out: &mut Parsed, message: String| {
        let raw: String = block
            .lines
            .join("\n")
            .chars()
            .take(RAW_EXCERPT_CHARS)
            .collect();
        out.warnings.push(Warning {
            line: block.line,
            message,
            raw,
        });
    };

    let title_line = clean(block.lines[0]).to_string();
    let Some(header) = block.lines.get(1).map(|h| clean(h)) else {
        return warn(out, "block has a title line but no header line".into());
    };
    let Some(after_prefix) = strip_prefix_ci(header, &config.header_prefix) else {
        return warn(
            out,
            format!("header does not start with {:?}", config.header_prefix),
        );
    };
    let kind_word = after_prefix.split_whitespace().next().unwrap_or("");
    let kind = if kind_word.eq_ignore_ascii_case(&config.highlight) {
        EntryKind::Highlight
    } else if kind_word.eq_ignore_ascii_case(&config.note) {
        EntryKind::Note
    } else if kind_word.eq_ignore_ascii_case(&config.bookmark) {
        out.bookmarks += 1;
        return;
    } else {
        return warn(out, format!("unknown clipping type {kind_word:?}"));
    };

    let mut location = None;
    let mut page = None;
    let mut raw_added_on = None;
    for segment in after_prefix.split('|').map(str::trim) {
        if let Some(date) = strip_prefix_ci(segment, &config.added_on) {
            raw_added_on = Some(date.trim().to_string());
            continue;
        }
        if let Some(v) = value_after_keyword(segment, &config.location) {
            match parse_range(v) {
                Some(r) => location = Some(r),
                None => warn(out, format!("unreadable location {v:?}")),
            }
        }
        if let Some(v) = value_after_keyword(segment, &config.page) {
            match parse_range(v) {
                Some(r) => page = Some(r),
                None => warn(out, format!("unreadable page {v:?}")),
            }
        }
    }
    let Some(raw_added_on) = raw_added_on else {
        return warn(out, format!("header has no {:?} date", config.added_on));
    };

    let text = block.lines[2..].join("\n").trim().to_string();
    if config.stub_markers.iter().any(|m| m.trim() == text) {
        out.stubs += 1;
        return;
    }
    if text.is_empty() {
        return warn(out, format!("{} has no text", kind.as_str()));
    }

    let created = parse_date(&raw_added_on, config, tz);
    if created.is_none() {
        warn(
            out,
            format!("date {raw_added_on:?} matches no configured format"),
        );
    }
    let (title, author) = split_title(&title_line);
    out.clippings.push(RawClipping {
        index: out.clippings.len(),
        line: block.line,
        title_line,
        title,
        author,
        kind,
        location,
        page,
        raw_added_on,
        created,
        text,
    });
}

/// Trim whitespace and any BOM, which Kindle leaves mid-file as well as at
/// the start.
fn clean(line: &str) -> &str {
    line.trim_matches(|c: char| c == '\u{FEFF}' || c.is_whitespace())
}

fn strip_prefix_ci<'a>(s: &'a str, prefix: &str) -> Option<&'a str> {
    let head = s.get(..prefix.len())?;
    head.eq_ignore_ascii_case(prefix)
        .then(|| &s[prefix.len()..])
}

/// The token following `keyword` as a whole word, e.g. `"1234-1236"` from
/// `"Highlight at location 1234-1236"`.
fn value_after_keyword<'a>(segment: &'a str, keyword: &str) -> Option<&'a str> {
    let mut words = segment.split_whitespace();
    words.find(|w| w.eq_ignore_ascii_case(keyword))?;
    words.next()
}

fn parse_range(v: &str) -> Option<(u32, u32)> {
    match v.split_once('-') {
        Some((a, b)) => Some((a.parse().ok()?, b.parse().ok()?)),
        None => v.parse().ok().map(|n| (n, n)),
    }
}

fn parse_date(raw: &str, config: &ParseConfig, tz: &TimeZone) -> Option<Timestamp> {
    let translated = translate_words(raw, &config.date_words);
    config.date_formats.iter().find_map(|fmt| {
        let dt = DateTime::strptime(fmt, &translated).ok()?;
        tz.to_ambiguous_zoned(dt)
            .compatible()
            .ok()
            .map(|z| z.timestamp())
    })
}

fn translate_words(raw: &str, words: &[(String, String)]) -> String {
    if words.is_empty() {
        return raw.to_string();
    }
    let mut out = String::with_capacity(raw.len());
    let mut word = String::new();
    let flush = |word: &mut String, out: &mut String| {
        let replacement = words
            .iter()
            .find(|(from, _)| from.eq_ignore_ascii_case(word));
        out.push_str(replacement.map_or(word.as_str(), |(_, to)| to.as_str()));
        word.clear();
    };
    for c in raw.chars() {
        if c.is_alphabetic() {
            word.push(c);
        } else {
            flush(&mut word, &mut out);
            out.push(c);
        }
    }
    flush(&mut word, &mut out);
    out
}

/// Split `Title (Author)` at the last parenthesised group, which must end the
/// line. Titles may contain parentheses of their own.
fn split_title(line: &str) -> (String, Option<String>) {
    if let Some(body) = line.strip_suffix(')')
        && let Some(open) = body.rfind('(')
    {
        let author = body[open + 1..].trim();
        let title = body[..open].trim();
        if !title.is_empty() {
            let author = (!author.is_empty()).then(|| author.to_string());
            return (title.to_string(), author);
        }
    }
    (line.to_string(), None)
}
