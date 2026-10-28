# Paths come from a gitignored .env (see .env.example); flags still override them.
set dotenv-load := true

[private]
default:
    @just --list

# Compile the flor binary in debug mode
build:
    cargo build --workspace --all-targets

# Run the full test suite (needs no calibre and no Python)
test:
    cargo test --workspace --all-targets

# Run the tests that need calibre installed (they drive calibre-debug)
test-calibre:
    cargo test --workspace --all-targets -- --ignored

# Lint with clippy, treating every warning as an error
lint:
    cargo clippy --workspace --all-targets -- --deny warnings

# Rewrite all Rust sources to rustfmt style
fmt:
    cargo fmt --all

# Fail if any Rust source is not rustfmt-formatted
fmt-check:
    cargo fmt --all -- --check

# Summarise My Clippings.txt ($FLOR_CLIPPINGS) without writing anything; add `--book TEXT` to list one book
[positional-arguments]
inspect-clippings *args:
    cargo run --quiet -- inspect-clippings "$@"

# Replay My Clippings.txt ($FLOR_CLIPPINGS) into the archive ($FLOR_ARCHIVE); add `--dry-run` to only report
[positional-arguments]
ingest-clippings *args:
    cargo run --quiet -- ingest-clippings "$@"

# Dump one calibre book's raw annotation rows as JSON, read-only (library: $FLOR_CALIBRE_LIBRARY)
inspect-calibre book:
    cargo run --quiet -- inspect-calibre --book "{{book}}"

# Read calibre's viewer highlights into the archive; add `--book ID` or `--dry-run`
[positional-arguments]
ingest-calibre *args:
    cargo run --quiet -- ingest-calibre "$@"

# Place highlights in calibre's EPUBs; add `--rederive`, `--book FILE` or `--dry-run`
[positional-arguments]
derive-calibre *args:
    cargo run --quiet --release -- derive-calibre "$@"

# Write highlights into calibre's EPUBs; needs `--calibre-closed` (or `--dry-run`), optionally `--include approximate,ambiguous`
[positional-arguments]
push-calibre *args:
    cargo run --quiet --release -- push-calibre "$@"

# Join two book files that are one book: KEEP gains ABSORB's entries; add `--dry-run` to only report
[positional-arguments]
merge-books keep absorb *args:
    cargo run --quiet -- merge-books "$@"

# Print every live entry on one tab-separated line: ID, book, chapter, text, note
list:
    cargo run --quiet -- list

# Fuzzy-find an entry with fzf; prints its ID and copies it to the clipboard
[positional-arguments]
find *query:
    ./just_scripts/find.sh "$@"

# Validate the archive; fails if anything is wrong
check:
    cargo run --quiet -- check

# Look up an entry by ID or alias; add `--json` or `--show-deleted`
[positional-arguments]
resolve id *args:
    cargo run --quiet -- resolve "$@"

# Dev-only: regenerate a real library's EPUB highlight CFIs from their text and compare with calibre's
cfi-selfcheck library=env("FLOR_CALIBRE_LIBRARY", ""):
    cargo run --quiet --release --example cfi_selfcheck -- "{{library}}"

# Dev-only: compute chapter titles of a real library's EPUB highlights and compare with calibre's
toc-selfcheck library=env("FLOR_CALIBRE_LIBRARY", ""):
    cargo run --quiet --release --example toc_selfcheck -- "{{library}}"

# Run flor with the given arguments, e.g. `just run --help`
run *args:
    cargo run --quiet -- {{args}}
