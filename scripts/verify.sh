#!/usr/bin/env bash
# The one command that decides whether this tree is releasable.
#
# Every threshold in the project lives here and nowhere else. CI calls this
# script rather than repeating the list, so a threshold cannot be lowered in
# one place and left standing in another — which is the failure this file
# exists to prevent, and which had already happened twice.
#
#   scripts/verify.sh              everything but the long fuzz run (~2 min)
#   scripts/verify.sh --fuzz       and 500k mutations on top (~1 min more)
#   scripts/verify.sh --gates      only the differential gates
#   scripts/verify.sh --quick      only tests, clippy and wasm — no pandoc
#   scripts/verify.sh --fuzz-only  only the fuzz run — no pandoc
#   scripts/verify.sh --limits     only the resource bound — no pandoc
#
# Only the gates need pandoc, and they need exactly 3.8.2.1: a different
# one produces spurious diffs, so this refuses to score against it rather
# than publish a number that means something else.
set -euo pipefail
cd "$(dirname "$0")/.."

PANDOC_PINNED=3.8.2.1
HARNESS=./target/release/ferrodoc-harness

# Peak resident memory, as a multiple of the input, on a 10 MB document.
# Set from the measured worst path (docx -> markdown, 77.2x) with room to
# move. It is a *regression* bound: nothing here may quietly get hungrier.
MAX_RSS_RATIO=85

want_gates=1 want_checks=1 want_fuzz=0 want_limits=1
case "${1-}" in
    --fuzz)       want_fuzz=1 ;;
    --gates)      want_checks=0 want_limits=0 ;;
    --quick)      want_gates=0 want_limits=0 ;;
    --fuzz-only)  want_checks=0 want_gates=0 want_limits=0 want_fuzz=1 ;;
    --limits)     want_checks=0 want_gates=0 ;;
    "")           ;;
    *) echo "usage: $0 [--fuzz|--gates|--quick|--fuzz-only|--limits]" >&2; exit 2 ;;
esac

failures=0

# Run a command, reporting only whether it succeeded.
step() {
    local name="$1" summary="${2-ok}"; shift 2
    printf '%-46s ' "$name"
    if output=$("$@" 2>&1); then
        printf '%s\n' "$summary"
    else
        printf 'FAILED\n'
        printf '%s\n' "$output" | tail -n 20 | sed 's/^/    /'
        failures=$((failures + 1))
    fi
}

# Run a gate, reporting the score it printed. Never "ok": a number that
# moved is worth seeing even when it passes, and a summary that says less
# than the tool did is how a threshold gets quietly lowered.
gate() {
    local name="$1"; shift
    printf '%-46s ' "$name"
    if output=$("$@" 2>&1); then
        printf '%s\n' "$(printf '%s' "$output" | tail -n1)"
    else
        printf 'FAILED  %s\n' "$(printf '%s' "$output" | tail -n1)"
        printf '%s\n' "$output" | grep -m5 '^MISMATCH' | sed 's/^/    /' || true
        failures=$((failures + 1))
    fi
}

if [ "$want_checks" = 1 ]; then
    echo "== build, test, lint"
    step "cargo test --workspace" ok cargo test --workspace --quiet
    step "cargo clippy -D warnings" ok cargo clippy --workspace --all-targets -- -D warnings
    # Every library crate must keep working where there is no operating
    # system: that is the browser and edge-worker claim in the README.
    step "wasm32 build" ok cargo build --quiet --workspace \
        --target wasm32-unknown-unknown --exclude ferrodoc-harness
fi

if [ "$want_gates" = 1 ]; then
    have=$(pandoc --version | head -1 | awk '{print $2}')
    if [ "$have" != "$PANDOC_PINNED" ]; then
        echo "pandoc $have is on PATH; conformance is pinned to $PANDOC_PINNED" >&2
        echo "a different pandoc produces spurious diffs — refusing to score" >&2
        exit 2
    fi
fi
if [ "$want_gates" = 1 ] || [ "$want_fuzz" = 1 ] || [ "$want_limits" = 1 ]; then
    cargo build --quiet --release -p ferrodoc-harness
fi

if [ "$want_limits" = 1 ]; then
    echo "== resource limits"
    # The fixture is 10 MB of generated prose; below about 1 MB the binary
    # itself is most of the resident set and the ratio measures nothing.
    fixture="$HOME/.cache/ferrodoc-bench/large.md"
    if [ ! -f "$fixture" ]; then
        bash corpus/bench/generate.sh >/dev/null
    fi
    gate "peak RSS <= ${MAX_RSS_RATIO}x input" \
        $HARNESS bench-rss "$fixture" --max-rss-ratio "$MAX_RSS_RATIO"
fi

if [ "$want_gates" = 1 ]; then
    echo "== differential gates vs pandoc $PANDOC_PINNED"
    SPEC=corpus/commonmark-spec-0.31.2.json
    gate "markdown reader"      $HARNESS diff-spec       $SPEC --fail-under 100
    gate "AST round trip"       $HARNESS diff-ast        corpus --fail-under 100
    gate "HTML writer"          $HARNESS diff-html       $SPEC --fail-under 100
    gate "HTML reader"          $HARNESS diff-html-read  $SPEC corpus --fail-under 95
    gate "markdown writer"      $HARNESS diff-md         $SPEC --fail-under 100
    gate "GFM reader (corpus)"  $HARNESS diff-gfm        corpus/gfm --fail-under 100
    gate "GFM reader (spec)"    $HARNESS diff-gfm        $SPEC --fail-under 99.8
    gate "GFM writer"           $HARNESS diff-gfm-md     corpus/gfm $SPEC --fail-under 100
    gate "DOCX reader"          $HARNESS diff-docx       corpus/docx --fail-under 96
    gate "DOCX reader (LO)"     $HARNESS diff-docx       corpus/docx-libreoffice --fail-under 87
    gate "DOCX writer"          $HARNESS diff-write      corpus --fail-under 90
    gate "ODT reader"           $HARNESS diff-odt        corpus/odt --fail-under 94
    gate "ODT reader (LO)"      $HARNESS diff-odt        corpus/odt-libreoffice --fail-under 100
    gate "ODT writer"           $HARNESS diff-odt-write  corpus --fail-under 100
fi

if [ "$want_fuzz" = 1 ]; then
    echo "== fuzz"
    # A reader's contract is that it refuses hostile input rather than
    # panicking. The seed varies so the search keeps moving.
    gate "500k mutations" env FERRODOC_FUZZ_SEED="${FERRODOC_FUZZ_SEED-$$}" \
        $HARNESS fuzz corpus --iters 500000
fi

echo
if [ "$failures" = 0 ]; then
    echo "all checks passed"
else
    echo "$failures check(s) FAILED"
fi
exit "$failures"
