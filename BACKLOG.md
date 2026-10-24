# Backlog

Work specced far enough to start, not yet built. The commit that delivers an
item deletes it here; partial delivery trims it to what remains.

Terms used throughout: an **archive** is a directory of per-book JSON files (a
separate, private git repo). An **entry** is one highlight or orphan note in
it. An **ID** is the entry's identifier.

Extensibility: other readers (e.g. Kobo) will become sources later. Keep each
source behind its own module that turns raw input into clusters, keep
source-specific data in a per-source block on the entry (`kindle`, `calibre`,
later `kobo`), and keep `sources` an open set of names. Identity merge, the
archive writer, `check` and `resolve` must not special-case a source. Any
dedup a future source needs lives inside that source, as the Kindle replay
does.

---

## M1: Core — clippings to archive (no calibre needed)

### Clippings parser
Parse `My Clippings.txt` into an ordered list of raw entries, each tagged with
its file index and starting line number.
- Entries are split on a line of exactly `==========`. Strip a leading BOM
  (`﻿`) from the file **and from every title line** (real files carry BOMs
  mid-file, which otherwise split one book into two). Accept `\r\n`.
- Header shapes seen in practice: `- Your Highlight on page N | location N-N |
  Added on …`, `- Your Highlight at location N-N | Added on …`, `- Your Note on
  page N | location N | …`, `- Your Highlight on page N-N | …` (page-only), and
  bookmark equivalents. Locations and pages may be single numbers or ranges.
- Title line is `Title (Author)`; the author is the **last** parenthesised
  group, may hold several names split by `;` or `,`, and may be absent.
- Drop bookmarks and "clipping limit" stubs, counting each.
- Never skip silently: an unparseable block becomes a warning with its line
  number. Exit non-zero only on I/O errors.
- All keywords (`Highlight`, `Note`, `Bookmark`, `Added on`, `location`, `page`,
  separator) and an ordered list of `strptime`-style date formats come from
  config, defaulting to English (`%A, %d %B %Y %H:%M:%S`). No fuzzy date
  parsing. An unmatched date is a warning and `created: null`.
- Dates have no timezone: attach the configured Kindle zone (default
  `Africa/Johannesburg`) and store UTC. Keep the raw string as
  `kindle.raw_added_on`.
- File order, not the parsed date, is authoritative for recency.

### Supersede replay (highlights)
Kindle records an edit (extend, trim, redo) as a new appended entry. For
overlapping highlights only the latest is kept. This logic exists **only**
here; the archive and calibre match by ID alone.

Per book, walk highlights in file order keeping a set of *current* clusters.
A new highlight absorbs every current cluster whose **latest member** truly
overlaps it; the merged cluster takes content from the new highlight.

**True overlap** (decided after measuring the real clippings file):
- *Candidates*: location ranges that intersect inclusively, i.e. touching at a
  boundary is enough to be compared. Page-only entries are candidates only
  within the same page.
- *Confirmed* only by text: the longest common substring of the two
  `norm_v1`, lowercased texts covers at least 50% of the shorter text **and**
  is at least 20 characters. Both thresholds are config, tuned with fixtures.
- Why: Kindle locations are ~150 characters, so adjacent separate highlights
  routinely share a boundary location (81 such pairs in the real file, almost
  all sharing ≤23 chars of text), while real edits that shifted both ends share
  ≥42 chars and ≥51%. Location alone would silently merge distinct highlights.

The surviving ID is the would-be ID of the **earliest** member in file order;
every other member's ID is an alias. If Kindle trimmed old lines and a
cluster's earliest member vanishes, the survivor ID may differ from run to
run; that is harmless because archive merge matches on any alias.

Tests (synthetic): extend, trim, edit of an edit, re-highlight on re-read, a
wide highlight absorbing two, a later highlight overlapping only a superseded
member (must not absorb the current one), adjacent highlights sharing a
boundary location (must **not** merge), page-only fallback, aliases hold every
member ID, earliest survives.

### Notes
After the replay, walk a book's notes in file order.
- A note attaches to the current highlight cluster whose location range
  contains the note's location (inclusive, tolerance in config, default 0).
  Latest note in file order wins. The note's own ID is not kept as an alias.
- A note that attaches to nothing becomes an orphan entry (`type: "note"`,
  `text: null`) with its own ID, latest-wins among orphans at the same
  location, and is listed in the report's warnings.

### Archive format and deterministic writer
One JSON file per book, named `<slug>--<short-id>.json`, name assigned at
creation and never changed. Files are found by the identity fields inside
them, never by name.

```json
{
  "schema_version": 1,
  "book": {
    "title": "Display Title",
    "authors": ["Author Name"],
    "kindle_titles": ["Title Of The Book (Author Name)"],
    "calibre_uuid": null
  },
  "entries": [{
    "id": "…", "aliases": [], "type": "highlight",
    "text": "…", "note": null,
    "created": "2026-10-23T13:50:12.000Z",
    "deleted": false, "sources": ["kindle"],
    "kindle": {"location": [1234, 1236], "page": 57, "raw_added_on": "…"},
    "calibre": null
  }]
}
```
- A clipping joins the file whose `kindle_titles` produce the same title key.
- `calibre_uuid` is calibre's **book** UUID (stable across library rebuilds,
  unlike numeric book IDs); null until matched.
- `created` uses calibre's exact timestamp text, `YYYY-MM-DDTHH:MM:SS.mmmZ`,
  so a calibre round trip is lossless.
- Serialisation: sorted keys, 2-space indent, UTF-8 unescaped, LF, one
  trailing newline. Entries sorted by `(created, nulls last; id)`; `aliases`
  and `sources` sorted and de-duplicated. A file is rewritten only if its bytes
  change.
- Alongside books: a user-edited `book-overrides.json` (Kindle title key →
  calibre book UUID, plus optional per-book total locations) and an optional
  `config.toml`.

### Identity merge into the archive
For each incoming cluster, `U = {id} ∪ aliases`; `M` = archive entries in the
same book (live or tombstoned) whose `{id} ∪ aliases` intersects `U`.
- `|M| = 0`: create.
- `|M| = 1`: update. The entry keeps its ID; aliases gain `U` minus that ID.
  `text`, `note`, `created`, `kindle` are replaced; `sources` unioned. If
  `text` changed and the calibre match is derived (`exact`, `approximate`,
  `ambiguous`, `unmatched`), drop the `calibre` block back to pending; keep it
  for `native` and `manual`.
- `|M| ≥ 2`: merge the entries. Survivor is the entry whose ID appears earliest
  among the cluster's members in file order (fallback: lexicographic). Union
  aliases (plus the losers' IDs) and sources. A tombstone in the group makes
  the result a tombstone. Reported prominently.
- Last ingest wins per field, on purpose: a calibre-side note edit is
  overwritten by the next Kindle ingest and the user rejects the hunk in
  review. No conflict machinery.

### Tombstones
`"deleted": true` is set by hand. Tombstones still take part in identity
matching (an incoming cluster that maps onto one updates nothing visible and
creates no live entry), but are excluded from push, export and `resolve`'s
default output.

### `ingest-clippings` command and change report
`annot-archive ingest-clippings CLIPPINGS --archive DIR [--dry-run] [--config
PATH]` re-processes the whole file each run. Prints per-book counts (created,
updated, unchanged, merged, tombstone-matched), warnings (unparsed blocks,
orphan notes, bad dates), and a prominent list of `|M| ≥ 2` merges.
Acceptance: a second run with the same file changes zero bytes; appending
clippings changes only the entries they touch; an entry later edited on the
Kindle keeps its original ID.

### `check` and `resolve`
- `check --archive DIR`: duplicate IDs or alias collisions across entries, an
  alias that is another entry's ID, schema violations, two files claiming one
  `calibre_uuid`, files that would re-serialise differently.
- `resolve ID --archive DIR`: look up by ID **or alias**; print the canonical
  ID and the entry as JSON or text; report tombstones.

### Synthetic fixtures
A synthetic clippings file (invented books and prose) covering every parser,
replay and note case above. Nothing real, ever.

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
`sources` gains `calibre`; the `kindle` block is untouched. Store calibre's
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
Precedence: existing `calibre_uuid`; `book-overrides.json`; fuzzy title and
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
