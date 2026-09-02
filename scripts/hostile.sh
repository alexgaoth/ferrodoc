#!/usr/bin/env bash
# Every reader against shapes chosen to break it, not to be converted.
#
# The one gate here that does not compare against pandoc, because the
# contract is not fidelity: every conversion must write bytes or return a
# clean error, never a signal, a panic or a hang. See the header of
# `scripts/hostile.py` for why this axis is generated rather than
# collected, and what it cost to learn that.
set -euo pipefail
cd "$(dirname "$0")/.."

[ -x ./target/release/ferrodoc ] || {
    echo "build it first: cargo build --release -p ferrodoc" >&2; exit 2; }

exec python3 scripts/hostile.py "$@"
