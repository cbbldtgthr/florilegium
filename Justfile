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

# Run annot-archive with the given arguments, e.g. `just run --help`
run *args:
    cargo run --quiet -- {{args}}
