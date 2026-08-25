#!/usr/bin/env bash
# Syntax highlighting against pandoc, over **real source files**.
#
# The trap this exists to avoid: only three of the CommonMark spec's 652
# examples hold a fence in a language pandoc knows, and they are nine
# lines of code between them. A highlighter written to pass those would
# be fitted to the fixtures rather than to the language — which is the
# corpus blind spot the root `CLAUDE.md` counts bugs against.
#
# So the inputs here are files that already exist in this repository for
# other reasons: the C binding's example and header, and every Python
# and shell file in the tree — this script among them. Nobody wrote them
# to be highlighted, and neither did the highlighter's author, which is
# what makes 2,650 lines of them a measurement rather than a rehearsal.
#
#   scripts/highlight.sh            the score, and one line per miss
#   scripts/highlight.sh --verbose  and the first lines of each diff
#
# A language is listed in COMPATIBILITY.md only while it is at 100 here.
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

same=0 total=0
# language<TAB>file
while IFS=$'\t' read -r language file; do
    case "$language" in ''|'#'*) continue ;; esac
    [ -f "$file" ] || { echo "missing: $file" >&2; exit 2; }
    total=$((total + 1))
    # A fence around the file, so this measures the writer's whole
    # output — the wrapper, the line anchors and the tokens together.
    {
        printf '```%s\n' "$language"
        cat "$file"
        printf '```\n'
    } > "$work/in.md"
    ( ulimit -v 6000000; pandoc "$work/in.md" -f commonmark -t html --wrap=none ) \
        > "$work/p" 2>/dev/null
    "$FERRODOC" "$work/in.md" -f commonmark -t html --wrap=none > "$work/f" 2>/dev/null
    if diff -q "$work/p" "$work/f" > /dev/null; then
        same=$((same + 1))
        continue
    fi
    printf '  %-10s %s\n' "$language" "$file"
    [ "$verbose" = 0 ] || diff "$work/p" "$work/f" | head -n 6 | sed 's/^/      /'
done <<EOF
c	bindings/c/example/convert.c
c	bindings/c/include/ferrodoc.h
python	scripts/nbformat-check.py
python	corpus/epub-handmade/generate.py
python	bindings/python/tests/test_ferrodoc.py
python	bindings/python/python/ferrodoc/__init__.py
bash	bindings/c/build.sh
bash	bindings/wasm/build.sh
bash	corpus/bench/generate.sh
bash	corpus/docx/generate.sh
bash	corpus/docx-libreoffice/generate.sh
bash	corpus/epub/generate.sh
bash	corpus/epub-handmade/generate.sh
bash	corpus/epub-spec/generate.sh
bash	corpus/odt/generate.sh
bash	corpus/odt-libreoffice/generate.sh
bash	crates/ferrodoc-ast/tests/fixtures/generate.sh
bash	samples/generate.sh
bash	scripts/claims.sh
bash	scripts/compare-toc.sh
bash	scripts/dropin.sh
bash	scripts/flags.sh
bash	scripts/highlight.sh
bash	scripts/sweep-epub-xhtml.sh
bash	scripts/verify.sh
bash	scripts/writers.sh
EOF

echo
echo "$same/$total highlighted files byte-identical"
[ "$same" = "$total" ] || exit 1
