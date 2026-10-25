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

# Run annot-archive with the given arguments, e.g. `just run --help`
run *args:
    cargo run --quiet -- {{args}}
