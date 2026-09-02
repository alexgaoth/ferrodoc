#!/usr/bin/env bash
# Every TeX expression this renders, against pandoc's own rendering.
#
# The AST sweep asks about `$x^2$` and nothing else, so it cannot say how
# much of the language the math writer covers. This can: see the header
# of `scripts/math.py` for what it measures and why a shared fallback
# counts as agreement.
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

exec python3 scripts/math.py "$@"
