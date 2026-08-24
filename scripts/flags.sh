#!/usr/bin/env bash
# Every CLI flag that shapes output, against pandoc, byte for byte.
#
# The gates score *conversions*; this scores **flags**. `diff-html` runs
# markdown -> HTML with the flags it chose, so it could not see that `-s`
# output was 176 lines away from pandoc's, and it cannot see what
# `--shift-heading-level-by` or `--strip-comments` do either.
#
# Required at **100**, not gated at a floor: these are flags whose whole
# job is to produce particular bytes. A `--eol=crlf` that is 90% right is
# a file with mixed line endings.
#
#   scripts/flags.sh              the score
#   scripts/flags.sh --verbose    and the first lines of each diff
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

# The two flags handed to pandoc are the ones README already records:
# ferrodoc does not highlight code (0.7) and reads `.md` as CommonMark
# rather than pandoc's dialect (card D4.4). Everything else is the page,
# which is what this measures.
#
# **`--wrap=none` used to be a third**, because ferrodoc's HTML writer
# joined soft breaks and pandoc filled to 72. Both default to `auto` at
# 72 since 2026-08-24, so the comparison is now made at the layout a user
# actually gets rather than at one chosen to make the two agree.
HANDED="--syntax-highlighting=none -f commonmark"

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
--shift-heading-level-by=1
--shift-heading-level-by=-1
--shift-heading-level-by=-2
--shift-heading-level-by=-3
--strip-comments
--strip-comments --shift-heading-level-by=1
--eol=crlf
--eol=lf
--ascii
--id-prefix=p-
--ascii --id-prefix=q-
-s --toc --id-prefix=p-
-s --metadata-file $A/meta.yaml
EOF

echo
echo "$same/$total flag combinations byte-identical"
[ "$same" = "$total" ] || exit 1
