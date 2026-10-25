[private]
default:
    @just --list

# Compile the annot-archive binary in debug mode
build:
    cargo build --workspace --all-targets

# Run the full test suite (needs no calibre and no Python)
test:
    cargo test --workspace --all-targets

# Lint with clippy, treating every warning as an error
lint:
    cargo clippy --workspace --all-targets -- --deny warnings

# Rewrite all Rust sources to rustfmt style
fmt:
    cargo fmt --all

# Fail if any Rust source is not rustfmt-formatted
fmt-check:
    cargo fmt --all -- --check

# Summarise a My Clippings.txt without writing anything; add `--book TEXT` to list one book
[positional-arguments]
inspect-clippings clippings *args:
    cargo run --quiet -- inspect-clippings "$@"

# Replay a My Clippings.txt into an archive directory; add `--dry-run` to only report
[positional-arguments]
ingest-clippings clippings archive *args:
    c="$1"; a="$2"; shift 2; cargo run --quiet -- ingest-clippings "$c" --archive "$a" "$@"

# Dump one calibre book's raw annotation rows as JSON, read-only
inspect-calibre library book:
    cargo run --quiet -- inspect-calibre --library "{{library}}" --book "{{book}}"

# Read calibre's viewer highlights into an archive; add `--library PATH`, `--book ID` or `--dry-run`
[positional-arguments]
ingest-calibre archive *args:
    a="$1"; shift 1; cargo run --quiet -- ingest-calibre --archive "$a" "$@"

# Validate an archive directory; fails if anything is wrong
check archive:
    cargo run --quiet -- check --archive "{{archive}}"

# Look up an entry by ID or alias; add `--json` or `--show-deleted`
[positional-arguments]
resolve id archive *args:
    i="$1"; a="$2"; shift 2; cargo run --quiet -- resolve "$i" --archive "$a" "$@"

# Run annot-archive with the given arguments, e.g. `just run --help`
run *args:
    cargo run --quiet -- {{args}}
