#!/usr/bin/env bash
# Compare every XHTML file inside every corpus EPUB, `pandoc -f html`
# against `ferrodoc -f html`, and print each divergence with its file.
#
# Why this exists: `diff-html-read` walks `corpus/` for `*.html`, and there
# are eight of those. The EPUBs hold **119** XHTML files written by pandoc's
# own writer, which is a far wider vocabulary — `epub:type`, `<li id>`,
# `role="doc-noteref"` — and none of it was gate-reachable. Three rounds of
# a census guessed at the boundary instead of sweeping it and undercounted
# every time.
#
#   ./scripts/sweep-epub-xhtml.sh          # divergences only
#   ./scripts/sweep-epub-xhtml.sh -v       # every file, with its verdict
#
# Exits non-zero if any file diverges, so it can be read by a machine.
set -uo pipefail
cd "$(dirname "$0")/.."

verbose=0
[ "${1:-}" = "-v" ] && verbose=1

# Capped, because the oracle is the danger here: pandoc exhausts memory on
# a self-referential footnote, and an EPUB is exactly where one lives.
ulimit -v 6000000

work=$(mktemp -d -p . sweep-epub.XXXXXX)
trap 'rm -rf "$work"' EXIT INT TERM

total=0
differing=0
for book in corpus/epub/*.epub corpus/epub-handmade/*.epub corpus/epub-spec/*.epub; do
    [ -f "$book" ] || continue
    unpacked="$work/$(basename "$book" .epub)"
    mkdir -p "$unpacked"
    unzip -qq -o "$book" -d "$unpacked" 2>/dev/null || continue
    while IFS= read -r page; do
        total=$((total + 1))
        pandoc -f html -t json "$page" > "$work/theirs.json" 2>/dev/null
        ./target/release/ferrodoc "$page" -f html -t json > "$work/ours.json" 2>/dev/null
        # `.blocks` only: an EPUB chapter has no `<head>` worth comparing,
        # and pandoc's identifier generation differs there by design — see
        # `read_html_without_generated_identifiers`.
        if python3 - "$work/theirs.json" "$work/ours.json" <<'PY'
import json, sys
def blocks(path):
    try:
        with open(path) as f:
            return json.load(f)["blocks"]
    except Exception:
        return None
a, b = blocks(sys.argv[1]), blocks(sys.argv[2])
sys.exit(0 if a is not None and a == b else 1)
PY
        then
            [ "$verbose" = 1 ] && printf '  same  %s\n' "${page#"$work"/}"
        else
            differing=$((differing + 1))
            printf 'DIVERGES  %s\n' "${page#"$work"/}"
        fi
    done < <(find "$unpacked" \( -name '*.xhtml' -o -name '*.html' \) -type f | sort)
done

printf '\n%s XHTML files across the corpus EPUBs, %s diverging\n' "$total" "$differing"
[ "$differing" -eq 0 ]
