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

### Synthetic EPUB and golden samples
A small synthetic EPUB (repeated phrases, curly quotes, em dashes, nested
inline markup, a cross-paragraph passage, a non-BMP character, an XHTML
self-closing tag, an `&nbsp;`). The user highlights a handful of passages in
calibre's viewer and `inspect-calibre` dumps are committed as golden samples
the generator must reproduce.

---

## Later: two-way edits

### Fields edited on both sides
Each source overwrites the fields it supplies (last ingest wins). Once an
entry lives in both Kindle and calibre, editing its note in calibre makes the
calibre ingest bring the edit in and the next Kindle ingest put the Kindle
version back: two diff hunks every cycle, not a one-off review. Decide
between: `locked_fields` on an entry (the archive keeps those fields; no
source overwrites them), or overwriting a field only when that source's own
copy changed since its last ingest (needs the last-seen value per source,
e.g. in the source block). The same applies to a pushed highlight's colour: a read-back of
our own anchor keeps the archive's `style`, so re-colouring it in calibre is
undone by the next push.
