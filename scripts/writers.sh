#!/usr/bin/env bash
# Each text writer against pandoc's own, byte for byte, on the same AST.
#
# The gates for these formats are *fidelity* runs: write the document,
# read it back through pandoc, require what returns to be what went in.
# That is the right shape where pandoc's reader is good, and it says
# nothing at all where pandoc cannot read the format back — pandoc writes
# AsciiDoc and does not read it, and its LaTeX reader round-trips 1 of 13
# of this corpus, which is our score exactly.
#
# But pandoc *writes* all of these, from the same AST. So there is an
# oracle after all, and it is the strictest one available: the bytes.
# This prints one number per writer and cannot fail the run — no floor
# has been chosen, and a floor chosen after seeing a score is not a
# floor. It is here to be watched, and to say which writer is close.
#
# Three flags go to pandoc, each for a reason already recorded in
# README.md and COMPATIBILITY.md:
#
#   -f commonmark            ferrodoc reads `.md` as CommonMark; pandoc's
#                            own `markdown` is a different dialect and
#                            adds heading identifiers (ROADMAP card D4.4)
#   --wrap=...               handed to **both** binaries. Every writer
#                            lays lines out all three ways since
#                            2026-08-24, and the default on both sides is
#                            now pandoc's `auto` at 72 — so this is a
#                            choice of what to compare rather than a
#                            workaround: `none` and `preserve` are the
#                            modes these numbers have always been taken at
#   --syntax-highlighting=none  pandoc colours code by default, and so
#                               does this — both sides are muted (0.7)
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

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

verbose=0
floors=0
case "${1-}" in
    --verbose) verbose=1 ;;
    --floors)  floors=1 ;;
    "") ;;
    *) echo "usage: $0 [--verbose|--floors]" >&2; exit 2 ;;
esac

# The floor for each writer is **the score it reached**, because every
# point below one is a document that used to be byte-identical and is not
# any more. That is a regression, not a range.
#
# A floor chosen after seeing a score is not a floor — which is why this
# printed a number and gated nothing for as long as the numbers were low.
# It is a contract now because five of the seven are at or within one of
# the whole corpus, and the two that are not are held to what they have.
floor_for() {
    case "$1" in
        html)     echo 38 ;;
        rst)      echo 36 ;;
        plain)    echo 38 ;;
        latex)    echo 36 ;;
        asciidoc) echo 38 ;;
        gfm)      echo 28 ;;
        # The like-for-like row: `-t markdown` here **is** CommonMark, so
        # pandoc's `commonmark` writer is the writer to compare it with.
        # It went 3 to 8 the day it was asked separately, and all four
        # misses left are pandoc losing information — a code block that
        # opens a blockquote or a list item comes back from its own round
        # trip as a paragraph, and its `<!-- -->` list separator comes
        # back as a `RawBlock` that was never in the document.
        commonmark) echo 29 ;;
        # `-t markdown` against `pandoc -t markdown`: the same question
        # asked of both binaries, which is what it always should have
        # been. The **CommonMark** writer scored 6 here while it was the
        # one `-t markdown` reached; the dialect writer scores 23, and
        # took the name on 2026-08-28.
        #
        # There is no `pandoc_markdown` row beside this one. It is an
        # accepted spelling of the same `Format`, so it cannot diverge
        # from this row, and two identical numbers in a published table
        # read as two pieces of evidence.
        markdown) echo 29 ;;
        *)        echo 0 ;;
    esac
}
below=0

# The last line is a one-line summary, because `verify.sh` reports a
# measurement by its final line and seven of these would otherwise be
# six numbers nobody sees.
summary=""

# Two source dialects, because one of them cannot express the constructs
# the writers are worst at. `corpus/*.md` is read as CommonMark, which has
# **no table and no task list** — so a score over it alone said `latex 7/8`
# while `samples/07-markdown-to-latex` showed the LaTeX *table* writer
# diverging on every row it wrote. That is this repository's most expensive
# bug class: a gate cannot fail on a construct its corpus lacks. The `gfm`
# pass is the same comparison over `corpus/gfm/*.gfm`, which holds tables
# with every alignment, task lists, and footnotes.
#
# **The third pass is this repository's own prose**, and it is here for
# the same reason `highlight.sh` runs on real source files: the twelve
# above are fixtures, written to be converted, and 4,440 lines of README,
# ROADMAP, COMPATIBILITY and `docs/` were not. Adding them on 2026-08-25
# scored **asciidoc 0/8 and rst 1/8** on writers that were at 11/12 and
# 12/12 here, and every one of the five bugs behind that was real.
# `commonmark` and `markdown` are the same ferrodoc writer measured
# against two different pandoc writers — see `floor_for` above.
for format in html commonmark markdown gfm latex rst asciidoc plain; do
    case "$format" in
        html|plain) wrap=none ;;
        *) wrap=preserve ;;
    esac
    mine=$format
    # Both sides name their dialect. `-t markdown` means pandoc's dialect
    # in both binaries since 2026-08-28, so nothing here may inherit a
    # default: the row that compares the **CommonMark** writer has to say
    # `commonmark` on this side too, and it said `markdown` until the day
    # that stopped being true.
    theirs=$format
    [ "$format" != pandoc_markdown ] || theirs=markdown
    same=0 total=0
    # **Every document twice: as it falls, and filled.** The second mode
    # is pandoc's own default, and nothing measured it until 2026-08-26 —
    # the RST writer treated an inline span as one unbreakable word, so
    # `**a long run**` was pushed whole onto the next line and overran
    # the column, on four of the eight prose documents. `--wrap=preserve`
    # never runs a fill, so no score here could move.
    for mode in "$wrap" auto; do
        for source in "commonmark corpus/*.md" "gfm corpus/gfm/*.gfm" \
                      "commonmark README.md COMPATIBILITY.md ROADMAP.md docs/*.md samples/README.md"; do
            read -r from pattern <<<"$source"
            for doc in $pattern; do
                total=$((total + 1))
                ( ulimit -v 6000000
                  pandoc "$doc" -f "$from" -t "$theirs" --wrap="$mode" --columns=72 \
                      --syntax-highlighting=none ) > "$work/p" 2>/dev/null
                "$FERRODOC" "$doc" -f "$from" -t "$mine" --wrap="$mode" --columns=72 \
                      --no-highlight > "$work/f" 2>/dev/null
                if diff -q "$work/p" "$work/f" >/dev/null; then
                    same=$((same + 1))
                elif [ "$verbose" = 1 ]; then
                    printf '  %-9s %-6s %-22s %s lines\n' "$format" "$mode" \
                        "$(basename "$doc")" "$(diff "$work/p" "$work/f" | grep -c '^[<>]')"
                fi
            done
        done
    done
    floor=$(floor_for "$format")
    if [ "$same" -lt "$floor" ]; then
        printf '%-10s %d/%d — BELOW ITS FLOOR OF %d\n' "$format" "$same" "$total" "$floor"
        below=1
    elif [ "$floors" = 0 ]; then
        printf '%-10s %d/%d byte-identical to pandoc (--wrap=%s and --wrap=auto)\n' \
            "$format" "$same" "$total" "$wrap"
    fi
    summary="$summary $format $same/$total,"
done

printf 'byte-identical:%s on corpus/*.md, corpus/gfm/*.gfm and this repository'"'"'s own prose\n' "${summary%,}"
exit "$below"
