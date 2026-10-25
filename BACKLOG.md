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

## M2: calibre read

### Read annotations through SQLite
Open `metadata.db` **read-only** (safe while calibre runs). Verified against
calibre 8.16.2 and a real library:
- Table `annotations(id, book, format, user_type, user, timestamp, annot_id,
  annot_type, annot_data, searchable_text)`; viewer highlights are
  `user_type='local'`, `user='viewer'`; `annot_id` equals the JSON `uuid`.
- Highlight JSON keys: `type, uuid, timestamp, start_cfi, end_cfi,
  highlighted_text, notes, style, spine_name, spine_index, toc_family_titles`.
  `style` is either builtin `{"kind":"color","type":"builtin","which":"yellow"}`
  or custom `{"kind":"color","type":"custom","friendly_name","light","dark"}`.
- Removed annotations are stubs `{"removed": true, "timestamp", "type",
  "uuid"}`: ignore them (no deletion propagation).
- Content-server annotations (`user_type='web'`) are ignored; the HTTP API
  cannot see viewer highlights, so it is not used.
- The book's UUID and EPUB path come from `books` and `data`.

### `ingest-calibre`
`ingest-calibre --library PATH --archive DIR [--book ID…]`. For EPUB highlight
annotations: find the book file by `calibre_uuid` (create one if absent, with
empty `kindle_titles`); match entries by ID or alias only; `text`, `note`,
`created` and the `calibre` block come from calibre with `match: "native"`;
`sources` gains `calibre`; the `kindle` block is untouched. `check` gains: a `native` entry whose text no longer matches the calibre
highlight's text (e.g. after a Kindle ingest overwrote it). Store calibre's
values verbatim so a later push → read-back produces no diff. Old CFIs that
current calibre can no longer decode (seen on 2020-era highlights) are stored
as-is.

### `inspect-calibre`
`inspect-calibre --library PATH --book ID` dumps a book's raw annotation rows
as JSON, for debugging and golden samples.

### Two files, one calibre book
If a Kindle-only file and a calibre-only file turn out to be the same book,
never merge silently: report it, and provide `merge-books A B --archive DIR`
to move entries and union keys.

---

## M3: derive CFIs for Kindle highlights

### Match archive books to calibre books
Precedence: existing `calibre_uuid`; `book-overrides.json` in the archive
(user-edited: Kindle title key → calibre book UUID, plus optional per-book
total locations); fuzzy title and
author match (normalise case, punctuation, leading articles, subtitle after a
colon, series prefix) accepted only above a threshold with a clear margin over
the runner-up; else `unmatched` with `reason: "no-calibre-book"`, logged once
per book. Only the EPUB format is used; a book without one is
`reason: "no-epub"`.

### Text locator
Parse each spine document as **XML with the HTML named-entity table
supplied** (XHTML files use `&nbsp;` and friends via their DTD). Verified on
228 real highlights: an XML parse reproduces calibre's tree on all but an
undeclared-entity case, while an HTML5 parse breaks on XHTML self-closing tags.
A document that fails to parse is `unmatched`, `reason: "parse-error"`, never
guessed. Build a normalised haystack per spine document with an index map back
to (text node, UTF-16 offset), using `norm_v1`'s mapping variant. Find every
occurrence of the normalised highlight text. One → `exact`. Several →
disambiguate by Kindle location (below). None → bounded fuzzy search above a
threshold → `approximate`, else `unmatched`, `reason: "text-not-found"`. A
match spanning two spine documents is `unmatched`, `reason: "cross-spine"`.

### Disambiguate by Kindle location
Pass 1: entries with exactly one occurrence give (Kindle location, offset in
the concatenated book text) points; fit a monotone mapping. Pass 2: for
multi-occurrence entries, predict the offset and pick the nearest candidate if
it beats the runner-up by a configured margin, else `ambiguous` with
`candidates` listed and the best guess in the CFI fields. Too few points: fall
back to `location / total_locations` (override, else the book's max location).

### CFI generator, matching calibre's viewer encoder
Rules taken from calibre's `src/pyj/read_book/cfi.pyj`:
- Path from the document: `/2` is `<html>`, `/4` is `<body>`. Children are
  indexed over the browser's `childNodes`: each element is an even step; any
  run of non-element nodes (text, comment) shares the odd step before the next
  element.
- `[id]` assertion added when the element's id is unique in the document.
- Text position: encode walks back over **all** preceding siblings, elements
  included, so the last step names the parent's **first child** (`/1` when it
  is text) and the offset sums the parent's direct text-node children up to the
  target, skipping text inside sibling elements, in UTF-16 code units.
  Verified: 92 real CFIs are `/1:N` yet land in a text node after an inline
  element. CFIs from 2020-era calibre used a different rule and are only ever
  stored, never generated.
- A selection ending at a block boundary may be an element-only CFI (no
  `:offset`); emit offsets always, but accept both when reading.
- Fill `spine_name`, `spine_index` (verified equal to the OPF spine position),
  `style` (configured default), `toc_family_titles` from the EPUB TOC where
  feasible, `match`, `reason`, `candidates`, `derived_with` (tool version).
- `derive-calibre --library PATH --archive DIR [--rederive] [--book …]`
  touches only `calibre` blocks and `book.calibre_uuid`; never `native` or
  `manual` entries.

### CFI oracle (local, dev-only)
A `calibre-debug` Python script (bundled interpreter, no dependencies) that
parses with calibre's own pipeline and resolves CFIs the way the viewer does,
used by a `just` recipe to check generated CFIs against a real local library.
Never run in CI; its inputs and outputs stay out of the repo.

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
  `native` matches. `approximate`/`ambiguous` need `--include`. Never
  `unmatched`, pending, orphan notes or tombstones.
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
- Acceptance: pushing twice is a no-op; push then `ingest-calibre` gives a
  zero-byte archive diff; deleting calibre annotations and re-pushing restores
  them with the same IDs.
