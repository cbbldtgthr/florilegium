# Backlog

Work specced far enough to start, not yet built. The commit that delivers an
item deletes it here; partial delivery trims it to what remains.

Terms used throughout: an **archive** is a folder of per-book JSON files in a
private git repo (never this one), a folder of its own since every `.json` in
it is read as a book. An **entry** is one highlight or orphan note in
it. An **ID** is the entry's identifier.

Extensibility: other readers (e.g. Kobo) will become sources later. Keep each
source behind its own module that turns raw input into clusters, keep
source-specific data in a per-source block on the entry (`kindle`, `calibre`,
later `kobo`), and keep `sources` an open set of names. Identity merge, the
archive writer, `check` and `resolve` must not special-case a source. Any
dedup a future source needs lives inside that source, as the Kindle replay
does.

---

## M3: derive CFIs for Kindle highlights

### Chapter titles on derived highlights
Fill `toc_family_titles` on derived blocks from the EPUB's table of contents
(NCX or nav), mapping each spine document and anchor to its chain of TOC
titles as calibre's viewer does, so derived highlights show their chapter in
"Browse annotations" like native ones.

### Synthetic EPUB and golden samples
A small synthetic EPUB (repeated phrases, curly quotes, em dashes, nested
inline markup, a cross-paragraph passage, a non-BMP character, an XHTML
self-closing tag, an `&nbsp;`). The user highlights a handful of passages in
calibre's viewer and `inspect-calibre` dumps are committed as golden samples
the generator must reproduce.

---

## M4: push to calibre

### `push-calibre`
`push-calibre --library PATH --archive DIR [--dry-run] [--include
approximate,ambiguous]`.
- Eligible by default: live highlight entries with `exact`, `manual` or
  `native` matches, on EPUB only: a `native` entry read from another format
  (AZW3, MOBI) stays archive-only. `approximate`/`ambiguous` need `--include`. Never
  `unmatched`, pending, orphan notes or tombstones.
- Build each annotation from the entry and its `calibre` block: `uuid` = entry
  ID, `timestamp` = `created`, `highlighted_text` = `text` (derived blocks do
  not repeat it), `notes` = `note`, and the block's CFIs, spine fields and
  `style`; drop the tool's own keys (`match`, `reason`, `candidates`,
  `occurrences`, `similarity`, `derived_with`, `format`, `book_id`).
- Mechanism: calibre's merge keeps the newer timestamp and, on a tie, the
  existing annotation, so a plain merge cannot update text. Instead, read the
  book/format's current viewer annotations, replace those whose `uuid` matches
  an archive entry, keep every other annotation untouched, and write the full
  list.
- The write goes through a short Python script run by `calibre-debug`, calling
  `set_annotations_for_book`, so calibre's own code handles its write
  invariants (`annotations_dirtied`, FTS) for the installed version. Rust
  passes it one JSON file.
- Back up `metadata.db` (timestamped copy) before the first write in a run.
  Refuse to write without a flag confirming calibre is closed (the GUI and
  viewer hold their own state and rewrite a book's whole annotation list when
  they save). `--dry-run` prints exactly what would change.
- Push records its own effect: every entry it writes gains `calibre` in
  `sources` in the same run, so reading it back changes nothing.
- Acceptance: pushing twice is a no-op; push then `ingest-calibre` gives a
  zero-byte archive diff; deleting calibre annotations and re-pushing restores
  them with the same IDs.

### Fields edited on both sides
Each source overwrites the fields it supplies (last ingest wins). Once an
entry lives in both Kindle and calibre, editing its note in calibre makes the
calibre ingest bring the edit in and the next Kindle ingest put the Kindle
version back: two diff hunks every cycle, not a one-off review. Decide
between: `locked_fields` on an entry (the archive keeps those fields; no
source overwrites them), or overwriting a field only when that source's own
copy changed since its last ingest (needs the last-seen value per source,
e.g. in the source block).
