#!/usr/bin/env bash
# Build the wasm module the npm package ships.
#
# `js/ferrodoc.wasm` is a build artefact and is not committed: a 1.6 MB
# binary in git costs every clone forever, and it would drift from the
# source the moment someone forgot to rebuild it.
set -euo pipefail
cd "$(dirname "$0")"

# With no arguments this builds the module the npm package ships. Any
# argument is passed to cargo and the result is deliberately *not* copied
# into `js/`, so measuring a trimmed build cannot replace what is published:
#
#   ./build.sh --no-default-features --features ferrodoc/markdown,ferrodoc/html
#
cargo build --release --target wasm32-unknown-unknown "$@"
built=target/wasm32-unknown-unknown/release/ferrodoc_wasm.wasm
if [ "$#" -eq 0 ]; then
    cp "$built" js/ferrodoc.wasm
    module=js/ferrodoc.wasm
else
    module=$built
fi

size=$(wc -c < "$module")
gzipped=$(gzip -c "$module" | wc -c)
printf '%s  %s bytes (%s gzipped)\n' "$module" "$size" "$gzipped"

# Size is a published claim: this package is downloaded over a network
# into a browser tab, where every kilobyte is somebody's latency.
limit=$((3 * 1024 * 1024))
if [ "$gzipped" -gt "$limit" ]; then
    echo "gzipped module is over the 3 MB the README promises" >&2
    exit 1
fi
