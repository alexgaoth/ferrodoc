#!/usr/bin/env bash
# Every threshold the prose asserts must be a threshold a gate has.
#
# `verify.sh` is the single source of every threshold and that rule has
# held; what drifts is the prose *about* them. See the header of
# `scripts/drift.py` for the day three files explained a floor the gate
# had left behind, and why ROADMAP.md and CHANGELOG.md are exempt.
set -euo pipefail
cd "$(dirname "$0")/.."

exec python3 scripts/drift.py "$@"
