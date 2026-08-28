#!/usr/bin/env bash
# Ask both writers about every construct the AST can hold.
#
# **A corpus can only fail on what somebody wrote into it.** That blind
# spot has cost this project eight bugs, and every answer to it so far
# has been "add another document" — which fixes the instance and leaves
# the class. The AST is a finite set of variants, so walking it has no
# blind spot by construction: it names every construct where this writer
# and pandoc's disagree, in one pass, by name rather than by line number.
#
# What that is worth, measured the day it was written: the `plain` writer
# scored 38/40 on documents and 104/137 on constructs, and the markdown
# dialect writer 106/136 — 19 of those one family (ordered list markers)
# that no document in the corpus contained.
#
# Discovery is **batched**: every case goes into one document separated
# by a sentinel paragraph, which is 2 process spawns per writer instead
# of 300. A batch can misalign against its answers — that has happened
# here before, with 200 bash words — so the sentinels are counted and the
# run refuses rather than guesses. **Confirm anything it reports one case
# per invocation before acting on it.**
#
#   ./scripts/ast-sweep.sh                 every writer, with the diffs
#   ./scripts/ast-sweep.sh markdown plain  only those
#   ./scripts/ast-sweep.sh --floors        quiet unless something fell
set -euo pipefail
cd "$(dirname "$0")/.."

PANDOC_PINNED=3.8.2.1
have=$(pandoc --version 2>/dev/null | head -n1 | awk '{print $2}')
if [ "$have" != "$PANDOC_PINNED" ]; then
    echo "pandoc $PANDOC_PINNED is what this compares against; found '${have:-none}'" >&2
    exit 2
fi
[ -x ./target/release/ferrodoc ] || {
    echo "build it first: cargo build --release -p ferrodoc" >&2; exit 2; }

# The cap is for pandoc, which this spawns once per writer; python fits
# inside it comfortably.
( ulimit -v 6000000; python3 scripts/ast-sweep.py "$@" )
