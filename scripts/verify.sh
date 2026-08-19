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
#   scripts/verify.sh --samples    only the samples/ freshness check
#   scripts/verify.sh --quick      only tests, clippy and wasm — no pandoc
#   scripts/verify.sh --fuzz-only  only the fuzz run — no pandoc
#   scripts/verify.sh --limits     only the resource bound — no pandoc
#   scripts/verify.sh --wasm       only the npm package — no pandoc
#   scripts/verify.sh --c          only the C ABI — no pandoc
#
# Only the gates need pandoc, and they need exactly 3.8.2.1: a different
# one produces spurious diffs, so this refuses to score against it rather
# than publish a number that means something else.
set -euo pipefail
cd "$(dirname "$0")/.."

PANDOC_PINNED=3.8.2.1
HARNESS=./target/release/ferrodoc-harness

# Peak resident memory, as a multiple of the input, on a 10 MB document.
# Set from the measured worst path (docx -> markdown, 73.8x) with room to
# move. It is a *regression* bound: nothing here may quietly get hungrier,
# and it follows real improvements *down* — a bound left slack after a win
# has stopped gating.
MAX_RSS_RATIO=80

want_gates=1 want_checks=1 want_fuzz=0 want_limits=1 want_wasm=0 want_c=0
want_samples=1
case "${1-}" in
    --fuzz)       want_fuzz=1 ;;
    --gates)      want_checks=0 want_limits=0 want_samples=0 ;;
    --samples)    want_checks=0 want_gates=0 want_limits=0 ;;
    --quick)      want_gates=0 want_limits=0 want_samples=0 ;;
    --fuzz-only)  want_checks=0 want_gates=0 want_limits=0 want_samples=0 want_fuzz=1 ;;
    --limits)     want_checks=0 want_gates=0 want_samples=0 ;;
    --wasm)       want_checks=0 want_gates=0 want_limits=0 want_samples=0 want_wasm=1 ;;
    --c)          want_checks=0 want_gates=0 want_limits=0 want_samples=0 want_c=1 ;;
    "")           ;;
    *) echo "usage: $0 [--fuzz|--gates|--samples|--quick|--fuzz-only|--limits|--wasm|--c]" >&2
       exit 2 ;;
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
# A number worth printing that cannot fail the run. Use it only where an
# oracle scores 0 on the same corpus, so any threshold would be a number
# picked after seeing the score — a thermometer, in this file's language.
# Everything else is a `gate`.
measure() {
    local name="$1"; shift
    printf '%-46s ' "$name"
    printf '%s\n' "$("$@" 2>&1 | tail -n1)"
}

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

if [ "$want_gates" = 1 ] || [ "$want_samples" = 1 ]; then
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
    gate "HTML reader"          $HARNESS diff-html-read  $SPEC corpus --fail-under 96
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
    gate "EPUB reader"          $HARNESS diff-epub       corpus/epub --fail-under 83
    # Books in shapes pandoc's own writer never emits: EPUB 2, an OEBPS
    # layout, a spine that is not the file order, a non-linear cover, a
    # percent-encoded href. Gated at 100 because it is small and every one
    # of its rules was found by it failing.
    gate "EPUB reader (hand-authored)" $HARNESS diff-epub corpus/epub-handmade --fail-under 100
    # The spec chunks measure the HTML reader compounding, not the EPUB
    # layer — see corpus/epub-spec/generate.sh. A *drop* is still a
    # regression, so the level is held.
    gate "EPUB reader (spec chunks)"   $HARNESS diff-epub corpus/epub-spec --fail-under 36
    # Notebooks in the shape Jupyter and Colab write them — nbformat
    # 4.5 with cell ids, streams, execute_results, a base64 `image/png`
    # display_data and an ANSI-coloured traceback — none of which
    # pandoc's own ipynb *writer* emits. Gated at 100 because it is
    # small and hand-authored, and because ipynb is JSON: there is no
    # parse ambiguity to hide behind.
    gate "ipynb reader (hand-authored)" $HARNESS diff-ipynb corpus/ipynb-handmade --fail-under 100
    # Ours through pandoc against pandoc's through pandoc, from the AST
    # pandoc itself read out of the notebook — so the reader cannot
    # flatter the writer. 8/8: nothing in this corpus is unmatchable,
    # because every cell carries a real id. The gate still drops a
    # UUID-shaped id from both sides, for a document whose cells have
    # none, and only that shape — a cell that loses `3a7f1c2d` fails.
    gate "ipynb writer"                $HARNESS diff-ipynb-write corpus/ipynb-handmade --fail-under 100
    # 8/11, and the three that differ all differ the same way: **this
    # writer does not emit a reference the book cannot satisfy.** An
    # image whose bytes are missing becomes its alt text and a relative
    # link naming no file in the book becomes its text; pandoc writes
    # both, and `epubcheck` rejects pandoc's book for exactly them
    # (RSC-007). Raising this number means writing invalid books, so the
    # bar that matters is beside it: CI runs `epubcheck` over every
    # written book and requires 0 fatals and 0 errors, which pandoc's own
    # output does not reach. Every remaining case is listed in
    # COMPATIBILITY.md.
    gate "EPUB writer"                 $HARNESS diff-epub-write corpus --fail-under 72
    # **Reported, not gated, and that is a demotion made on evidence.**
    # This ran at `--fail-under 9` because exactly one of eleven documents
    # round-tripped. Reading footnotes correctly cost that one: its note
    # holds a list, and a footnote containing a list does not survive
    # pandoc's LaTeX reader — measured with `\tightlist`, without it, with
    # one-line items, and with **pandoc's own LaTeX round-tripped by
    # pandoc**, which loses it identically. The harness prints
    # `pandoc round-trips 0/11` beside our number: the oracle scores zero,
    # so any floor here is a number chosen after seeing the score.
    # The checks that decide this writer are elsewhere and are real: CI
    # compiles every corpus document with `pdflatex`, and the rules a
    # round trip cannot see (`\tightlist`, `\verb` delimiters, enumerate
    # styles) have literal-output tests. See COMPATIBILITY.md.
    measure "LaTeX writer (fidelity)"  $HARNESS diff-latex corpus --fail-under 0
    # Same framing as LaTeX: pandoc round-trips 3/11 of this corpus, so
    # the ceiling is the format, not the writer. There is deliberately no
    # `diff-asciidoc` — pandoc writes AsciiDoc and cannot read it, so
    # there is no oracle; `asciidoctor` judges that one in CI.
    gate "RST writer (fidelity)"       $HARNESS diff-rst corpus --fail-under 18
fi

if [ "$want_samples" = 1 ]; then
    echo "== samples"
    # The gates above are blind in two places — `diff-html` scores against
    # the CommonMark specification, which has no tables, and the markdown
    # gates round-trip through this project's own reader, which never
    # produces an inline CommonMark cannot spell. Three silent data losses
    # lived there with every gate green, and `samples/` is what found them.
    # This says the committed artefacts are still what this tree produces,
    # so a fourth cannot land unnoticed. ~4 s: it regenerates every sample
    # into a scratch directory and compares, leaving the tree untouched.
    step "samples/ matches a fresh generate.sh" ok ./samples/generate.sh --check
fi

if [ "$want_fuzz" = 1 ]; then
    echo "== fuzz"
    # A reader's contract is that it refuses hostile input rather than
    # panicking. The seed varies so the search keeps moving.
    gate "500k mutations" env FERRODOC_FUZZ_SEED="${FERRODOC_FUZZ_SEED-$$}" \
        $HARNESS fuzz corpus --iters 500000
fi

if [ "$want_wasm" = 1 ]; then
    echo "== npm package"
    # The binding is outside the workspace, so none of the checks above
    # reach it. Each of these has caught something the others could not.
    ( cd bindings/wasm && ./build.sh >/dev/null 2>&1 )
    step "cargo test (handle table)" ok \
        env -C bindings/wasm cargo test --quiet
    step "cargo clippy -D warnings" ok \
        env -C bindings/wasm cargo clippy --all-targets -- -D warnings
    step "node --test" ok \
        env -C bindings/wasm node --test test/ferrodoc.test.mjs
    # A browser is the only judge of the claim this package exists for.
    if command -v google-chrome >/dev/null 2>&1 || [ -n "${CHROME-}" ]; then
        step "headless Chrome, no network request" ok \
            env -C bindings/wasm node test/browser/run.mjs
    else
        printf '%-46s %s\n' "headless Chrome" "skipped (no browser)"
    fi
    if [ -d bindings/wasm/node_modules/typescript ]; then
        step "tsc --noEmit" ok env -C bindings/wasm npx --no-install tsc --noEmit
    else
        printf '%-46s %s\n' "tsc --noEmit" "skipped (npm i typescript)"
    fi
fi

if [ "$want_c" = 1 ]; then
    echo "== C ABI"
    step "cargo test (safety rules)" ok env -C bindings/c cargo test --quiet
    step "cargo clippy -D warnings" ok \
        env -C bindings/c cargo clippy --all-targets -- -D warnings
    # A header nobody has called through is a guess, not an interface.
    step "the C example compiles and runs" ok env -C bindings/c ./build.sh
    if command -v valgrind >/dev/null 2>&1; then
        step "no leaks under valgrind" ok env -C bindings/c env VALGRIND=1 ./build.sh
    else
        printf '%-46s %s\n' "valgrind" "skipped (not installed)"
    fi
fi

echo
if [ "$failures" = 0 ]; then
    echo "all checks passed"
else
    echo "$failures check(s) FAILED"
fi
exit "$failures"
