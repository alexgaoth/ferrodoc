#!/usr/bin/env bash
# Build the wasm module the npm package ships.
#
# `js/ferrodoc.wasm` is a build artefact and is not committed: a 1.6 MB
# binary in git costs every clone forever, and it would drift from the
# source the moment someone forgot to rebuild it.
set -euo pipefail
cd "$(dirname "$0")"

cargo build --release --target wasm32-unknown-unknown
cp target/wasm32-unknown-unknown/release/ferrodoc_wasm.wasm js/ferrodoc.wasm

size=$(wc -c < js/ferrodoc.wasm)
gzipped=$(gzip -c js/ferrodoc.wasm | wc -c)
printf 'js/ferrodoc.wasm  %s bytes (%s gzipped)\n' "$size" "$gzipped"

# Size is a published claim: this package is downloaded over a network
# into a browser tab, where every kilobyte is somebody's latency.
limit=$((3 * 1024 * 1024))
if [ "$gzipped" -gt "$limit" ]; then
    echo "gzipped module is over the 3 MB the README promises" >&2
    exit 1
fi
