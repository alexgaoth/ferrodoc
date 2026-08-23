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
#   --wrap=...               ferrodoc leaves lines where the document put
#                            them; pandoc fills to 72 (card D4.3). Which
#                            setting matches depends on the writer, and
#                            the choice here is the one `samples/` already
#                            makes for the same format: `none` for HTML
#                            and plain, where ferrodoc emits no breaks of
#                            its own, `preserve` for the writers that
#                            carry the document's own lines through
#   --syntax-highlighting=none  pandoc colours code by default (0.7)
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
[ "${1-}" != "--verbose" ] || verbose=1

# The last line is a one-line summary, because `verify.sh` reports a
# measurement by its final line and seven of these would otherwise be
# six numbers nobody sees.
summary=""

for format in html markdown gfm latex rst asciidoc plain; do
    case "$format" in
        html|plain) wrap=none ;;
        *) wrap=preserve ;;
    esac
    same=0 total=0
    for doc in corpus/*.md; do
        total=$((total + 1))
        ( ulimit -v 6000000
          pandoc "$doc" -f commonmark -t "$format" --wrap="$wrap" \
              --syntax-highlighting=none ) > "$work/p" 2>/dev/null
        "$FERRODOC" "$doc" -f commonmark -t "$format" > "$work/f" 2>/dev/null
        if diff -q "$work/p" "$work/f" >/dev/null; then
            same=$((same + 1))
        elif [ "$verbose" = 1 ]; then
            printf '  %-9s %-22s %s lines\n' "$format" "$(basename "$doc")" \
                "$(diff "$work/p" "$work/f" | grep -c '^[<>]')"
        fi
    done
    printf '%-10s %d/%d byte-identical to pandoc (--wrap=%s)\n' \
        "$format" "$same" "$total" "$wrap"
    summary="$summary $format $same/$total,"
done

printf 'byte-identical:%s all on corpus/*.md\n' "${summary%,}"
