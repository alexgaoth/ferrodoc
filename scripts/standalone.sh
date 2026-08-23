#!/usr/bin/env bash
# `-s` output against pandoc's, byte for byte, over a matrix of the flags
# that shape a page.
#
# The fragment writer had matched pandoc for a long time; the page around
# it was 176 lines away, and no gate could see that — `diff-html` scores
# fragments. This runs every document in `corpus/` through ten flag
# combinations and requires **every one** to be identical, because
# reproducing pandoc's own template is not a thing one gets 90% right:
# a page that is nearly pandoc's is a page every diff of a migrated site
# reports.
#
#   scripts/standalone.sh              the score
#   scripts/standalone.sh --verbose    and the first lines of each diff
set -uo pipefail
cd "$(dirname "$0")/.."

PANDOC_PINNED=3.8.2.1
FERRODOC=${FERRODOC:-./target/release/ferrodoc}

have=$(pandoc --version 2>/dev/null | head -1 | awk '{print $2}')
[ "$have" = "$PANDOC_PINNED" ] || {
    echo "pandoc $PANDOC_PINNED is what this compares against; found '${have:-none}'" >&2
    exit 2
}
[ -x "$FERRODOC" ] || { echo "build it first: cargo build --release -p ferrodoc" >&2; exit 2; }

verbose=0
[ "${1-}" != "--verbose" ] || verbose=1

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
A=dropin/assets

# The three flags handed to pandoc are the ones README already records:
# ferrodoc's HTML writer joins soft breaks (`--wrap=none`), does not
# highlight code (0.7), and reads `.md` as CommonMark rather than pandoc's
# dialect (card D4.4). Everything else is the page, which is what this
# measures.
HANDED="--wrap=none --syntax-highlighting=none -f commonmark"

same=0 total=0
while IFS= read -r flags; do
    for doc in corpus/*.md; do
        total=$((total + 1))
        # shellcheck disable=SC2086
        ( ulimit -v 6000000; pandoc "$doc" -t html $HANDED $flags ) > "$work/p" 2>/dev/null
        # shellcheck disable=SC2086
        "$FERRODOC" "$doc" -t html -f commonmark $flags > "$work/f" 2>/dev/null
        if diff -q "$work/p" "$work/f" >/dev/null; then
            same=$((same + 1))
            continue
        fi
        printf '  %-28s %s\n' "$(basename "$doc")" "$flags"
        [ "$verbose" = 0 ] || diff "$work/p" "$work/f" | head -n 8 | sed 's/^/      /'
    done
done <<EOF
-s
-s --toc
-s --toc --toc-depth=2
-s -c $A/style.css
-s -V lang=fr
-s -V title-prefix=Docs --toc
-s -H $A/header.html
-s -B $A/before.html -A $A/after.html
-s --template $A/template.html
-s --template $A/template.html -c $A/style.css --toc
EOF

echo
echo "$same/$total standalone command lines byte-identical"
[ "$same" = "$total" ] || exit 1
