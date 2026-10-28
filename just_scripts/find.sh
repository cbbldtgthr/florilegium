#!/bin/sh
# Fuzzy-find an archive entry with fzf, print its ID, and copy the ID to the
# clipboard (where pbcopy, wl-copy or xclip exists).
#
#     just_scripts/find.sh [QUERY...]
#
# The archive is $FLOR_ARCHIVE (just loads it from .env).
# Matching runs on book, chapter, text and note; the preview shows the whole
# entry as `resolve` prints it.
set -eu

archive="${FLOR_ARCHIVE:?set FLOR_ARCHIVE in .env (see .env.example)}"
cargo build --quiet --release --bin flor
bin="$(pwd)/target/release/flor"

id="$("$bin" list --archive "$archive" | fzf \
    --delimiter "$(printf '\t')" \
    --with-nth 2.. \
    --nth 1.. \
    --query "$*" \
    --prompt 'highlight> ' \
    --preview "\"$bin\" resolve {1} --archive \"$archive\"" \
    --preview-window 'down,45%,wrap' \
    | cut -f1)"

[ -n "$id" ] || exit 1
printf '%s\n' "$id"
for copy in pbcopy wl-copy "xclip -selection clipboard"; do
    if command -v ${copy%% *} >/dev/null 2>&1; then
        printf '%s' "$id" | $copy && echo "(copied to the clipboard)" >&2
        break
    fi
done
