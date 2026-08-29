#!/usr/bin/env bash
# Ask pandoc and this binary the same question about one AST.
#
# **This is the inner loop of every rule in the repository**, and it was
# fifteen lines of inline JSON each time it was needed. The boilerplate
# was where the mistakes lived: a stray quote, a `Row` written as a
# tagged object where the schema wants an array, and — twice — a case
# that measured the surrounding batch rather than the construct.
#
#   ./scripts/probe.sh 'para(code("a`b"))'
#   ./scripts/probe.sh -t rst,latex 'para(sup(words("a b")))'
#   ./scripts/probe.sh -t markdown --columns 30 --wrap auto \
#       'table([0.5,0.5], [["h1","h2"]], [["a","b"]])'
#   ./scripts/probe.sh -f markdown 'quote(codeblock("x"))'   # and read it back
#
# The argument is a Python expression over the constructors in
# `probe.py`. `--json` prints the AST instead of converting it, and
# `-f READER` reads both outputs back — which is what says whether
# matching pandoc's bytes would cost the document.
#
# Exit status is 1 when any writer differed, so it composes.
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

( ulimit -v 6000000; python3 scripts/probe.py "$@" )
