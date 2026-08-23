#!/usr/bin/env bash
# Regenerate everything under samples/.
#
# Every conversion is run twice — once with ferrodoc, once with the pinned
# pandoc — and both outputs are kept side by side so the difference is
# something you can read rather than a percentage you have to believe.
#
#   ./samples/generate.sh            rewrite samples/
#   ./samples/generate.sh --check    fail if samples/ is not what a fresh
#                                    run produces, leaving the tree alone
#
# Needs pandoc 3.8.2.1 on PATH. Writes samples/RESULTS.md at the end.
set -uo pipefail

check=0
case "${1-}" in
    --check) check=1 ;;
    "")      ;;
    *) echo "usage: $0 [--check]" >&2; exit 2 ;;
esac

root=$(cd "$(dirname "$0")/.." && pwd)
cd "$root"
out=samples
# --check writes a whole second copy beside the committed one and compares
# the two at the end, so a verification run never touches the tree it is
# verifying. The directory is created inside the repo because the paths in
# a diff header are relative to it.
if [ "$check" = 1 ]; then
    out=$(mktemp -d -p . samples-check.XXXXXX)/samples
    trap 'rm -rf "$(dirname "$out")"' EXIT INT TERM
    mkdir -p "$out/inputs"
    cp samples/inputs/handbook.md "$out/inputs/handbook.md"
    # Hand-authored, unlike `page.html`: the attributes it carries
    # cannot be written in markdown at all, which is also why
    # `diff-html` cannot see them — that gate is markdown -> HTML.
    cp samples/inputs/attributes.html "$out/inputs/attributes.html"
fi
inputs=$out/inputs
bin=target/release/ferrodoc

PANDOC_PINNED=3.8.2.1
have=$(pandoc --version 2>/dev/null | head -1 | awk '{print $2}')
if [ "$have" != "$PANDOC_PINNED" ]; then
    echo "pandoc $PANDOC_PINNED is what these samples are compared against; found '${have:-none}'." >&2
    echo "The ferrodoc side will still be written; the pandoc side would not be comparable." >&2
    exit 1
fi

cargo build --release -p ferrodoc || exit 1

# Real documents, not ones written to flatter the tool: report.docx and
# contract.odt come out of LibreOffice, and the EPUB is hand-authored in a
# layout pandoc's own writer never produces.
mkdir -p "$inputs"
cp corpus/docx-libreoffice/report.docx "$inputs/report.docx"
cp corpus/odt-libreoffice/contract.odt "$inputs/contract.odt"
cp corpus/epub-handmade/cover-and-collisions.epub "$inputs/book.epub"
cp corpus/swatch.png "$inputs/logo.png"
# An HTML input another tool wrote, so the HTML reader is judged on
# someone else's markup rather than on ferrodoc's own.
pandoc "$inputs/handbook.md" -f gfm -t html -s -o "$inputs/page.html"

rows=()
wrap_used=

# One conversion, both engines, kept side by side.
#   case <folder> <input> <-f from> <-t to> <extension> [extra ferrodoc args...]
case_() {
    local name=$1 input=$2 from=$3 to=$4 ext=$5; shift 5
    local dir="$out/$name"
    mkdir -p "$dir"
    ( cd "$inputs" && "$root/$bin" "$(basename "$input")" -f "$from" -t "$to" "$@" \
        -o "$root/$dir/ferrodoc.$ext" ) 2>"$dir/ferrodoc.stderr"
    # ferrodoc has no line-wrapping option: it never reflows. Pandoc
    # reflows at column 72 by default, which rewrites nearly every line of
    # every text sample and buries the differences that are about content.
    # So pandoc is run both ways and the closer output is kept — the flag
    # that won is recorded in RESULTS.md rather than chosen quietly.
    local best= bestcount=-1 mode
    for mode in none preserve; do
        ( cd "$inputs" && pandoc "$(basename "$input")" -f "$from" -t "$to" \
            --wrap="$mode" "$@" -o "$root/$dir/pandoc.$mode.$ext" ) 2>"$dir/pandoc.stderr"
        local count
        count=$(diff "$dir/pandoc.$mode.$ext" "$dir/ferrodoc.$ext" 2>/dev/null |
            grep -cE '^[<>]')
        if [ "$bestcount" -lt 0 ] || [ "$count" -lt "$bestcount" ]; then
            bestcount=$count; best=$mode
        fi
    done
    mv "$dir/pandoc.$best.$ext" "$dir/pandoc.$ext"
    for mode in none preserve; do rm -f "$dir/pandoc.$mode.$ext"; done
    wrap_used="--wrap=$best"
    for f in "$dir/ferrodoc.stderr" "$dir/pandoc.stderr"; do
        [ -s "$f" ] || rm -f "$f"
    done
    verdict "$name" "$dir/ferrodoc.$ext" "$dir/pandoc.$ext" "$dir" "$to ($wrap_used)"
}

# A binary target cannot be read, so what is compared is what the rest of
# the world sees when it opens the file: pandoc reading ours against
# pandoc reading its own. Both binaries are kept too — open them in Word,
# LibreOffice or an e-reader.
case_binary() {
    local name=$1 input=$2 from=$3 to=$4; shift 4
    local dir="$out/$name"
    mkdir -p "$dir"
    ( cd "$inputs" && "$root/$bin" "$(basename "$input")" -f "$from" -t "$to" "$@" \
        -o "$root/$dir/ferrodoc.$to" ) 2>/dev/null
    ( cd "$inputs" && pandoc "$(basename "$input")" -f "$from" -t "$to" "$@" \
        -o "$root/$dir/pandoc.$to" ) 2>/dev/null
    pandoc "$dir/ferrodoc.$to" -f "$to" -t gfm --wrap=preserve -o "$dir/ferrodoc.readback.md" 2>/dev/null
    pandoc "$dir/pandoc.$to" -f "$to" -t gfm --wrap=preserve -o "$dir/pandoc.readback.md" 2>/dev/null
    verdict "$name" "$dir/ferrodoc.readback.md" "$dir/pandoc.readback.md" "$dir" "$to (read back)"
}

verdict() {
    local name=$1 ours=$2 theirs=$3 dir=$4 label=$5
    if [ ! -s "$ours" ]; then
        rows+=("| \`$name\` | $label | **ferrodoc produced nothing** | — |")
        return
    fi
    if diff -q "$ours" "$theirs" >/dev/null 2>&1; then
        rm -f "$dir/diff.txt"
        rows+=("| \`$name\` | $label | **identical to pandoc** | — |")
        echo "  $name: identical"
    else
        diff -u "$theirs" "$ours" > "$dir/diff.txt"
        local changed
        changed=$(grep -cE '^[+-][^+-]' "$dir/diff.txt")
        rows+=("| \`$name\` | $label | differs | $changed lines — see \`$name/diff.txt\` |")
        echo "  $name: differs ($changed lines)"
    fi
}

echo "== out of the formats people are stuck in"
case_ 01-docx-to-markdown  report.docx   docx     gfm      md
case_ 02-docx-to-html      report.docx   docx     html     html
case_ 03-odt-to-markdown   contract.odt  odt      gfm      md
case_ 04-epub-to-markdown  book.epub     epub     gfm      md
case_ 05-html-to-markdown  page.html     html     gfm      md
# HTML in, HTML out: the only sample that scores the HTML reader and
# writer together. `diff-html` runs markdown -> HTML, so an attribute
# markdown cannot express is invisible to it; this is where it shows.
case_ 14-html-to-html      attributes.html html   html     html

echo "== into the formats people need"
case_ 06-markdown-to-html     handbook.md gfm html html
# The whole page, not the fragment. `diff-html` scores the fragment and
# says nothing about the `<head>`, the default stylesheet or the contents
# — which is where `-s` output was 176 lines away from pandoc's until it
# went through pandoc's own template. This is the sample that would say
# so again.
case_ 15-markdown-to-html-page handbook.md gfm html html -s --toc
case_ 07-markdown-to-latex    handbook.md gfm latex tex -s
case_ 08-markdown-to-rst      handbook.md gfm rst rst
case_ 09-markdown-to-asciidoc handbook.md gfm asciidoc adoc
case_ 10-markdown-to-plain    handbook.md gfm plain txt

echo "== into binary formats, judged by what opens them"
case_binary 11-markdown-to-docx handbook.md gfm docx
case_binary 12-markdown-to-odt  handbook.md gfm odt
case_binary 13-markdown-to-epub handbook.md gfm epub

{
    echo "# Results"
    echo
    echo "Generated by \`samples/generate.sh\` against pandoc $PANDOC_PINNED."
    echo "\"Identical to pandoc\" means byte-for-byte."
    echo
    echo "| sample | conversion | ferrodoc vs pandoc | difference |"
    echo "|---|---|---|---|"
    printf '%s\n' "${rows[@]}"
} > "$out/RESULTS.md"

echo
if [ "$check" = 0 ]; then
    echo "wrote $out/RESULTS.md"
    exit 0
fi

# What is committed has to be what a fresh run produces, or samples/ is a
# description of some earlier tree. Two things can never compare equal and
# only two: the `---`/`+++` header of a diff carries the timestamp and the
# directory the run happened in, and `.docx`/`.odt`/`.epub` embed zip mtimes
# and pandoc-generated ids wherever they appear. Their `*.readback.md` is
# the comparable artefact and is compared. Everything else must match byte
# for byte — including `inputs/`, which is not hand-maintained: five of its
# six files are copied from `corpus/` on every run and `page.html` is
# generated by pandoc, so a drift there is as real as one anywhere else.
artefacts() {
    ( cd "$1" && find RESULTS.md inputs [0-9]*-*/ -type f \
        ! -name '*.docx' ! -name '*.odt' ! -name '*.epub' -print ) | sort
}
header='1,2{/^--- /d;/^+++ /d;}'
stale=0
while read -r f; do
    fresh=$out/$f committed=samples/$f
    if [ ! -f "$fresh" ]; then
        echo "  no longer produced, still committed: samples/$f"
        stale=1
    elif [ ! -f "$committed" ]; then
        echo "  produced, not committed: samples/$f"
        stale=1
    elif [ "$(basename "$f")" = diff.txt ]; then
        diff <(sed "$header" "$committed") <(sed "$header" "$fresh") >/dev/null ||
            { echo "  differs: samples/$f"; stale=1; }
    elif ! cmp -s "$fresh" "$committed"; then
        echo "  differs: samples/$f"
        stale=1
    fi
done < <( { artefacts "$out"; artefacts samples; } | sort -u )

if [ "$stale" = 1 ]; then
    echo "samples/ is stale: run ./samples/generate.sh and read the diffs before committing" >&2
    exit 1
fi
echo "samples/ matches a fresh run"
