#!/usr/bin/env bash
# Regenerate the JSON fixtures from real pandoc output.
# Requires pandoc 3.8.x on PATH; run from anywhere.
set -euo pipefail
cd "$(dirname "$0")"

pandoc -f markdown -t json -o kitchen-sink.json src/kitchen-sink.md
pandoc -f native -t json -o exhaustive.json src/exhaustive.native
