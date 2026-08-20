#!/usr/bin/env bash
# `--toc` and `--number-sections`, compared against pandoc document by
# document. There is no differential gate in the harness for either: the
# rest of a standalone page is deliberately not pandoc's (pandoc's default
# template carries a ~170-line stylesheet, an `xmlns`, a `generator` meta
# and a title taken from the *file name*), so what is compared here is the
# part this project claims to match — the `<nav id="TOC">` block, and the
# heading lines.
#
#   ./scripts/compare-toc.sh            # every document, prints a score
#   ./scripts/compare-toc.sh -v         # and the first differing document
#
# Pandoc is given `--wrap=none` because its default reflows at column 72,
# and comparing against that would measure who guessed the same column.
set -u
cd "$(dirname "$0")/.."

FERRODOC=${FERRODOC:-./target/release/ferrodoc}
verbose=${1:-}
[ -x "$FERRODOC" ] || { echo "build it first: cargo build --release -p ferrodoc" >&2; exit 2; }

nav() { sed -n '/<nav id="TOC"/,/<\/nav>/p'; }
headings() { grep -E '^<h[1-6][ >]' || true; }

total=0
matched=0
failures=()
while IFS= read -r doc; do
    # Two headings, because a document with one cannot show nesting and a
    # document with none produces no nav at all.
    [ "$(grep -cE '^#{1,6} ' "$doc")" -ge 2 ] || continue
    total=$((total + 1))

    ours_toc=$("$FERRODOC" -f gfm -t html -s --toc "$doc" | nav)
    theirs_toc=$( ( ulimit -v 6000000; pandoc -f gfm -t html -s --toc --wrap=none "$doc" ) | nav)
    ours_num=$("$FERRODOC" -f gfm -t html --number-sections "$doc" | headings)
    theirs_num=$( ( ulimit -v 6000000; pandoc -f gfm -t html --number-sections --wrap=none \
        --syntax-highlighting=none "$doc" ) | headings)

    if [ "$ours_toc" = "$theirs_toc" ] && [ "$ours_num" = "$theirs_num" ]; then
        matched=$((matched + 1))
    else
        failures+=("$doc")
        if [ -n "$verbose" ] && [ "${#failures[@]}" = 1 ]; then
            diff <(printf '%s\n' "$theirs_toc") <(printf '%s\n' "$ours_toc") | head -20
            diff <(printf '%s\n' "$theirs_num") <(printf '%s\n' "$ours_num") | head -20
        fi
    fi
done < <(find corpus samples/inputs -name '*.md' -not -path '*/node_modules/*' | sort)

printf '%s/%s documents identical (toc and numbering)\n' "$matched" "$total"
if [ "${#failures[@]}" -gt 0 ]; then
    printf '  %s\n' "${failures[@]}"
    exit 1
fi
