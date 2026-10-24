# annot-archive

A permanent, plain-text archive of every highlight and note you make while
reading, whether on a Kindle or in calibre's viewer, where every highlight has
an identifier you can cite for years.

## Why

Reading leaves highlights scattered across devices. The Kindle appends them to
`My Clippings.txt`, a noisy, append-only file full of duplicates from every
edit. calibre keeps its own in a database. Neither is a record you can build on.

This tool consolidates both into one **archive**: a directory of per-book JSON
files kept in a private git repository. The point is the identifier. Each
highlight gets an ID once, and that ID never changes, so a note in Obsidian or a
blog post can cite the passage instead of copy-pasting it, and the citation
still resolves after re-reads, edits, device resets and library rebuilds. The
archive also feeds highlights back into calibre, so its "Browse annotations"
becomes one place to see everything.

## Principles

- **The archive is the source of truth.** The Kindle file and calibre feed it;
  calibre and everything downstream read from it.
- **Identity is minted once and frozen.** Text, positions and match quality may
  change; an ID may not. When two entries turn out to be the same passage, the
  loser's ID is kept as an alias, so no citation ever breaks.
- **Review by diff.** Same inputs give byte-identical output, and re-running a
  command with nothing new changes nothing. The tool can afford to be
  opinionated because every change it makes is visible in `git diff`. There are
  no prompts and no conflict-resolution UI.
- **Derived data never touches identity.** Anything recomputable (calibre book
  match, CFIs) can be regenerated without moving an ID.
- **Judgment lives in one place.** Only the Kindle file needs overlap-based
  deduplication, because only it records edits as new entries. Everywhere else
  matches by ID alone.
- **Careful with calibre's library.** Read liberally; write rarely, with a
  backup, and only what the tool is confident about. Anything uncertain is
  flagged, never guessed.
- **Public tool, private data.** This repository never contains real clippings,
  book text or notes, including in test fixtures. Your archive lives in its own
  private repository.

## Scope

The Kindle and calibre come first. Other readers (Kobo, for example) are meant
to plug in later as further sources feeding the same archive, so nothing about
identity or the archive format may assume there are only two.

## Constraints

- Books are EPUB. Convert anything else to EPUB in calibre yourself before
  importing highlights; the tool never converts or modifies a book.
- No GUI, no syncing back to the Kindle, and no automatic detection of deleted
  highlights. You mark a deletion by hand in the archive and review it in the
  diff.

## Development

Everything runs through `just`; run `just` to list the recipes. The tool is
written in Rust. calibre is needed only for the commands that touch a calibre
library.
