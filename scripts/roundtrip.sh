#!/usr/bin/env bash
# Every **binary** writer, judged by what pandoc reads back out of it.
#
# The text writers have `writers.sh`, which compares bytes. A DOCX, an
# ODT, an EPUB and a notebook have no bytes worth comparing — the two
# tools zip different files in a different order — so the judge here is
# pandoc's own reader: write the same AST with both, read both back, and
# require the two ASTs to agree.
#
# Why this exists: `diff-docx` and friends score **pandoc's own output**,
# and pandoc does not write a code block inside a list item, so the DOCX
# writer split one into a paragraph per line for as long as anyone had
# looked. In one sitting this found that, `--wrap` reaching none of the
# notebook writer, the EPUB writer emitting no accessibility metadata,
# and its `dc:language` defaulting to `en` where pandoc's is `en-US`.
#
# Two fields are normalised away, and only two. Both are cases where the
# two tools are **documented** as differing and neither is wrong:
#
#   * a notebook cell's `id`, which pandoc makes random and this derives;
#   * an EPUB's `dc:identifier` and `dc:date`, for the same reason — this
#     writes a stable identifier and a fixed timestamp so the same
#     document produces the same book, which pandoc does not do.
#
# Nothing else is filtered. A miss here is a real difference.
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

cat > "$work/norm.py" <<'PY'
import json, re, sys

UUID = re.compile(r"^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$")
d = json.load(sys.stdin)
# An EPUB's identifier and date: pandoc stamps a fresh UUID and the clock,
# this derives one from the content and fixes the other, so that the same
# document produces the same book.
for key in ("date", "identifier"):
    d.get("meta", {}).pop(key, None)

# A notebook cell's id, which pandoc's reader puts on the cell's `Div`.
# Same reason, and it is the identifier of the triple rather than a field,
# which is why a pass looking for an `id` key found nothing to strip and
# every notebook read as differing.
def strip(x):
    if isinstance(x, dict):
        if x.get("t") == "Div" and isinstance(x.get("c"), list) and UUID.match(x["c"][0][0] or ""):
            x["c"][0][0] = ""
        for v in x.values():
            strip(v)
    elif isinstance(x, list):
        for v in x:
            strip(v)

# The name an embedded picture is filed under — `media/rId9.png` for
# pandoc, `media/image1.png` here. `samples/README.md` has counted that
# as immaterial since the writers were written. Numbered by first
# appearance rather than blanked, so two *different* pictures still read
# as different.
seen = {}
def rename(x):
    if isinstance(x, dict):
        if x.get("t") in ("Image", "Link") and isinstance(x.get("c"), list):
            url = x["c"][2][0]
            if url.startswith("media/"):
                x["c"][2][0] = "media/%d" % seen.setdefault(url, len(seen))
        for v in x.values():
            rename(v)
    elif isinstance(x, list):
        for v in x:
            rename(v)

strip(d)
rename(d)
json.dump(d, sys.stdout, indent=1, sort_keys=True)
PY

# The same documents `writers.sh` uses: fixtures written to be converted,
# and this repository's own prose, which was not.
docs=(corpus/*.md README.md COMPATIBILITY.md ROADMAP.md docs/*.md samples/README.md)

floor_for() {
    case "$1" in
        odt)   echo 16 ;;
        docx)  echo 14 ;;
        ipynb) echo 11 ;;
        # Every book differs on `dc:title`, which this writes always and
        # pandoc omits — `epubcheck` rejects pandoc's book for exactly
        # that, so the divergence is decided and the row stays at zero
        # until the decision changes. Gated so a *second* difference
        # cannot hide behind the first: `--verbose` prints the line count.
        #
        # **Two are known, and both are deliberate.** Nine lines is the
        # `dc:title` baseline. Above it means the document has a relative
        # link to a file the book does not contain: this drops the dead
        # href and keeps the text, pandoc keeps the href, and `epubcheck`
        # answers the question —
        #
        #   ERROR(RSC-007): Referenced resource "EPUB/text/x.html"
        #   could not be found in the EPUB.
        #
        # measured 2026-08-27 by injecting one into an otherwise valid
        # book. `docs/gates.md` has no relative links and sits at nine;
        # `README.md` has six and sits at 152. **A count above nine that
        # is not explained by those links is the third difference this
        # line exists to catch.**
        epub)  echo 0 ;;
        *)     echo 0 ;;
    esac
}

below=0
summary=""
for format in docx odt epub ipynb; do
    same=0 total=0 refused=0
    for doc in "${docs[@]}"; do
        [ -f "$doc" ] || continue
        total=$((total + 1))
        # **Every artefact is removed first.** Pandoc writes nothing when
        # it refuses a document — `corpus/images.md` names a picture that
        # is deliberately absent, and `-o out.ipynb` then leaves whatever
        # was there — so without this the comparison silently ran against
        # the *previous* document's bytes and three rows agreed by
        # accident.
        rm -f "$work/p.$format" "$work/f.$format"
        # **Pandoc looks for media in the working directory; this looks
        # beside the document** (`COMPATIBILITY.md`, `--resource-path`).
        # Without saying so, every document with a picture measured that
        # difference instead of the writer: pandoc found none of
        # `corpus/images.md`'s and wrote a book with no frames in it.
        ( ulimit -v 6000000
          pandoc "$doc" -f commonmark --resource-path="$(dirname "$doc")" \
              -o "$work/p.$format" ) 2>/dev/null
        "$FERRODOC" "$doc" -f markdown -o "$work/f.$format" 2>/dev/null
        # A document only one of them will write is not a difference in
        # the writer, and saying so beats scoring it either way.
        if [ ! -s "$work/p.$format" ] || [ ! -s "$work/f.$format" ]; then
            [ "$verbose" = 0 ] || printf '  %-6s %-24s not written by %s\n' \
                "$format" "$(basename "$doc")" \
                "$([ -s "$work/p.$format" ] && echo ferrodoc || echo pandoc)"
            refused=$((refused + 1))
            continue
        fi
        for side in p f; do
            ( ulimit -v 6000000; pandoc "$work/$side.$format" -t json ) 2>/dev/null \
                | python3 "$work/norm.py" > "$work/$side.json" 2>/dev/null
        done
        if [ -s "$work/p.json" ] && diff -q "$work/p.json" "$work/f.json" >/dev/null 2>&1; then
            same=$((same + 1))
        elif [ "$verbose" = 1 ]; then
            printf '  %-6s %-24s %s lines\n' "$format" "$(basename "$doc")" \
                "$(diff "$work/p.json" "$work/f.json" 2>/dev/null | grep -c '^[<>]')"
        fi
    done
    floor=$(floor_for "$format")
    if [ "$same" -lt "$floor" ]; then
        printf '%-6s %d/%d — BELOW ITS FLOOR OF %d\n' "$format" "$same" "$total" "$floor"
        below=1
    fi
    if [ "$refused" -gt 0 ]; then
        summary="$summary $format $same/$total ($refused unwritten),"
    else
        summary="$summary $format $same/$total,"
    fi
done

printf 'read back by pandoc:%s over corpus/*.md and this repository'"'"'s own prose\n' "${summary%,}"
exit "$below"
