#!/usr/bin/env bash
# Every number this project publishes, and the command that re-derives it.
#
# The most expensive defects here have not been wrong code. They have been
# claims wider than their evidence — seven in one week — and the ones that
# survive longest are the ones that were *true when written*: a gate whose
# corpus grew, a byte count from a build two months ago. Nothing was
# checking them, so nothing said when they stopped being true.
#
#   scripts/claims.sh --gates FILE   the scores `verify.sh` just produced
#   scripts/claims.sh --slow         the figures no gate produces (~2 min)
#   scripts/claims.sh --sizes        the byte counts and ratios (~4 min)
#
# `verify.sh` calls --gates itself, so the common case costs nothing. The
# other two need builds or a second pandoc run and are called by CI.
#
# A claim fails here in either direction: the derived number moved, or the
# published text no longer says it. Fixing one is not optional — the
# figure in the document is the product, and the derivation is the truth.
set -euo pipefail
cd "$(dirname "$0")/.."

failures=0
checked=0

# Report one claim. `where` is the file, `anchor` the text of the line
# that publishes the figure — matched literally, so a table row that was
# reworded fails here rather than silently stopping being checked.
published() {
    local figure="$1" where="$2" anchor="$3"
    checked=$((checked + 1))
    local line
    line=$(grep -F -- "$anchor" "$where" || true)
    if [ -z "$line" ]; then
        printf '  MISSING  %-34s %s\n' "$where" "no line matching \"$anchor\""
        failures=$((failures + 1))
        return
    fi
    if printf '%s' "$line" | grep -qF -- "$figure"; then
        return
    fi
    printf '  DRIFTED  %-34s says %s\n' "$where" \
        "$(printf '%s' "$line" | head -c 100)"
    printf '           %-34s derived %s\n' "(\"$anchor\")" "$figure"
    failures=$((failures + 1))
}

# --- the gate figures ------------------------------------------------
#
# Fields: the label `verify.sh` prints, whose score (ours|pandoc), the
# file that publishes it, and the literal text of the line that does.
# Tab-separated, because the table rows contain pipes.
gate_claims() {
    printf '%s\n' \
"markdown reader	ours	COMPATIBILITY.md	\`diff-spec\` | markdown reader" \
"markdown reader	ours	README.md	CommonMark spec examples produce identical ASTs" \
"AST round trip	ours	COMPATIBILITY.md	\`diff-ast\` | any pandoc JSON" \
"HTML writer	ours	COMPATIBILITY.md	\`diff-html\` | HTML writer" \
"HTML writer	ours	README.md	spec examples produce identical HTML" \
"HTML reader	ours	COMPATIBILITY.md	\`diff-html-read\` | HTML reader" \
"HTML reader	ours	README.md	HTML documents produce identical ASTs" \
"markdown writer	ours	COMPATIBILITY.md	\`diff-md\` | markdown writer" \
"markdown writer	pandoc	COMPATIBILITY.md	\`diff-md\` | markdown writer" \
"markdown writer	ours	README.md	spec examples survive a markdown round trip" \
"markdown writer	pandoc	README.md	spec examples survive a markdown round trip" \
"GFM writer	ours	COMPATIBILITY.md	\`diff-gfm-md\` | GFM writer" \
"GFM writer	pandoc	COMPATIBILITY.md	\`diff-gfm-md\` | GFM writer" \
"GFM writer	ours	README.md	documents survive a GFM round trip" \
"GFM writer	pandoc	README.md	documents survive a GFM round trip" \
"pandoc-markdown	ours	README.md	pandoc-markdown reader" \
"pandoc-markdown	ours	COMPATIBILITY.md	\`diff-pandoc-md\` | pandoc-markdown reader" \
"pandoc-markdown (all markdown)	ours	README.md	pandoc-markdown reader" \
"pandoc-markdown (all markdown)	ours	COMPATIBILITY.md	\`diff-pandoc-md\` | pandoc-markdown reader" \
"toc and numbering	ours	COMPATIBILITY.md	./scripts/compare-toc.sh" \
"DOCX reader	ours	COMPATIBILITY.md	\`diff-docx\` | DOCX reader" \
"DOCX reader	ours	README.md	corpus documents produce identical ASTs, and" \
"DOCX reader (LO)	ours	COMPATIBILITY.md	\`diff-docx\` (LibreOffice)" \
"DOCX writer	ours	COMPATIBILITY.md	\`diff-write\` | DOCX writer" \
"ODT reader	ours	COMPATIBILITY.md	\`diff-odt\` | ODT reader" \
"ODT reader (LO)	ours	COMPATIBILITY.md	\`diff-odt\` (LibreOffice)" \
"ODT writer	ours	COMPATIBILITY.md	\`diff-odt-write\` | ODT writer" \
"EPUB reader	ours	COMPATIBILITY.md	\`diff-epub\` | EPUB reader" \
"EPUB reader (hand-authored)	ours	COMPATIBILITY.md	\`diff-epub\` (hand-authored)" \
"EPUB reader (spec chunks)	ours	COMPATIBILITY.md	\`corpus/epub-spec\`" \
"EPUB writer	ours	COMPATIBILITY.md	\`diff-epub-write\` | EPUB writer" \
"ipynb reader (hand-authored)	ours	COMPATIBILITY.md	\`diff-ipynb\` | notebook reader" \
"ipynb writer	ours	COMPATIBILITY.md	\`diff-ipynb-write\` | notebook writer" \
"LaTeX writer (fidelity)	ours	COMPATIBILITY.md	\`diff-latex\` | LaTeX writer" \
"LaTeX writer (fidelity)	pandoc	COMPATIBILITY.md	\`diff-latex\` | LaTeX writer" \
"RST writer (fidelity)	ours	COMPATIBILITY.md	\`diff-rst\` | RST writer" \
"RST writer (fidelity)	pandoc	COMPATIBILITY.md	\`diff-rst\` | RST writer" \
"flags vs pandoc	ours	COMPATIBILITY.md	flag combinations byte-identical" \
"real command lines	ours	README.md	command lines identical"
}

# The figure in one recorded score line: `N/M` from ours, or the `N/M`
# that follows "round-trips" for pandoc's.
figure_from() {
    local scores="$1" label="$2" which="$3" line
    if [ "$which" = pandoc ]; then
        line=$(grep -P "^\Q$label\E\t.*round-trips" "$scores" || true)
        printf '%s' "$line" | grep -oE 'round-trips [0-9]+/[0-9]+' | awk '{print $2}'
    else
        line=$(grep -P "^\Q$label\E\t" "$scores" | grep -v round-trips || true)
        printf '%s' "$line" | grep -oE '[0-9]+/[0-9]+' | head -n1
    fi
}

check_gates() {
    local scores="$1"
    [ -f "$scores" ] || { echo "no scores file: $scores" >&2; exit 2; }
    local label which where anchor figure
    while IFS=$'\t' read -r label which where anchor; do
        # `|| true`, or a claim with no score kills the run under `set -e`
        # instead of being reported — which is the one case this exists
        # for. Found by breaking a figure and watching nothing be said.
        figure=$(figure_from "$scores" "$label" "$which" || true)
        if [ -z "$figure" ]; then
            printf '  MISSING  %-34s no "%s" score in this run\n' "$label" "$which"
            failures=$((failures + 1))
            checked=$((checked + 1))
            continue
        fi
        published "$figure" "$where" "$anchor"
    done < <(gate_claims)

    # `writers.sh` prints its seven scores on one line and each has a
    # table row of its own. Nothing checked them until 2026-08-23, and the
    # LaTeX row had said 0/8 for as long as it had a row.
    local writers score
    writers=$(grep -P "^\Qtext writers vs pandoc's\E\t" "$scores" || true)
    for writer in html latex plain gfm rst asciidoc markdown; do
        score=$(printf '%s' "$writers" | grep -oE "\b$writer [0-9]+/[0-9]+" | awk '{print $2}')
        if [ -z "$score" ]; then
            printf '  MISSING  %-34s no writers.sh score in this run\n' "$writer"
            failures=$((failures + 1))
            checked=$((checked + 1))
            continue
        fi
        published "$score" COMPATIBILITY.md "| \`$writer\` |"
    done

    # The GFM reader is published as one number and gated as two runs.
    local corpus spec
    corpus=$(figure_from "$scores" "GFM reader (corpus)" ours || true)
    spec=$(figure_from "$scores" "GFM reader (spec)" ours || true)
    if [ -n "$corpus" ] && [ -n "$spec" ]; then
        local sum
        sum=$(( ${corpus%%/*} + ${spec%%/*} ))/$(( ${corpus##*/} + ${spec##*/} ))
        published "$sum" COMPATIBILITY.md '`diff-gfm` | GFM reader'
        published "$sum" README.md 'GFM reader'
    fi
}

# --- the figures no gate produces -------------------------------------
#
# Both office writers are published against the *spec*, and `verify.sh`
# runs them against `corpus/` — 44 s each, which is why they are here and
# not there. A published number with no command anywhere is the defect
# this file exists for, and these two were exactly that.
check_slow() {
    local harness=./target/release/ferrodoc-harness
    [ -x "$harness" ] || cargo build --quiet --release -p ferrodoc-harness
    local spec=corpus/commonmark-spec-0.31.2.json figure
    figure=$($harness diff-write "$spec" --fail-under 0 2>&1 |
        grep -oE '[0-9]+/[0-9]+ identical' | tail -n1 | cut -d' ' -f1 || true)
    published "$figure" README.md 'spec examples survive a DOCX round trip'
    figure=$($harness diff-odt-write "$spec" --fail-under 0 2>&1 |
        grep -oE '[0-9]+/[0-9]+ identical' | tail -n1 | cut -d' ' -f1 || true)
    published "$figure" README.md 'spec examples survive an ODT round trip'
    published "$figure" COMPATIBILITY.md 'over the spec examples'
}

# --- the sizes --------------------------------------------------------
#
# The byte counts move with the compiler and with any code change, so a
# bound rather than an equality: 5% is far tighter than the 33x and 60%
# claims they support, and loose enough not to fail on a rustc release.
# The *ratios* are the actual claim and are held to a point.
# Plain `gzip -c`, the same level `bindings/wasm/build.sh` reports with:
# `-9` is 0.4% smaller and would make the two disagree about the same
# module.
gzipped() { gzip -c "$1" | wc -c; }

# 6499664 -> 6,499,664, which is how README writes it. `printf "%'d"`
# would depend on the locale, and CI runs in C.
commas() { printf '%s' "$1" | sed -e ':a' -e 's/\B[0-9]\{3\}\>/,&/;ta'; }

# A percentage README publishes, against the one just derived. One point
# of slack, which is the rounding, and no more.
point() {
    local what="$1" derived="$2" said="$3"
    checked=$((checked + 1))
    if [ -z "$said" ]; then
        printf '  MISSING  %-29s README.md publishes no percentage\n' "$what"
        failures=$((failures + 1))
        return
    fi
    local off=$(( derived > said ? derived - said : said - derived ))
    if [ "$off" -le 1 ]; then
        printf '  %-38s %11s%%  (README says %s%%)\n' "$what" "$derived" "$said"
    else
        printf '  DRIFTED  %-29s %11s%%  (README says %s%%)\n' "$what" "$derived" "$said"
        failures=$((failures + 1))
    fi
}

within() {
    local what="$1" derived="$2" published_value="$3" percent="$4"
    checked=$((checked + 1))
    local off
    off=$(awk -v d="$derived" -v p="$published_value" \
        'BEGIN { printf "%.2f", (d > p ? d - p : p - d) * 100 / p }')
    if awk -v o="$off" -v l="$percent" 'BEGIN { exit !(o <= l) }'; then
        printf '  %-38s %12s  (published %s, %s%% off)\n' \
            "$what" "$derived" "$published_value" "$off"
    else
        printf '  DRIFTED  %-29s %12s  (published %s, %s%% off, limit %s%%)\n' \
            "$what" "$derived" "$published_value" "$off" "$percent"
        failures=$((failures + 1))
    fi
}

# Report one figure that is allowed to differ by a percentage, for the
# claims that are measured rather than fixed. Fractions are compared, so
# `awk` does the arithmetic that bash cannot.
near() {
    local what="$1" derived="$2" said="$3" limit="$4"
    checked=$((checked + 1))
    if [ -z "$said" ]; then
        printf '  MISSING  %-29s README.md publishes no figure\n' "$what"
        failures=$((failures + 1))
        return
    fi
    local off
    off=$(awk -v a="$derived" -v b="$said" 'BEGIN { d = a - b; if (d < 0) d = -d; printf "%.2f", b == 0 ? 100 : d * 100 / b }')
    if awk -v o="$off" -v l="$limit" 'BEGIN { exit !(o <= l) }'; then
        printf '  %-38s %11s  (README says %s, %s%% off)\n' "$what" "$derived" "$said" "$off"
    else
        printf '  DRIFTED  %-29s %11s  (README says %s, %s%% off, limit %s%%)\n' \
            "$what" "$derived" "$said" "$off" "$limit"
        failures=$((failures + 1))
    fi
}

check_sizes() {
    echo "  building four artefacts..."
    cargo build --quiet --release -p ferrodoc
    cargo build --quiet --release -p ferrodoc --no-default-features \
        --features markdown,html --target-dir target/trimmed
    local cli trimmed_cli
    cli=$(stat -c%s target/release/ferrodoc)
    trimmed_cli=$(stat -c%s target/trimmed/release/ferrodoc)

    ./bindings/wasm/build.sh >/dev/null 2>&1
    local wasm wasm_gz
    wasm=$(stat -c%s bindings/wasm/js/ferrodoc.wasm)
    wasm_gz=$(gzipped bindings/wasm/js/ferrodoc.wasm)
    # A trimmed build deliberately stays in `target/` and never reaches
    # `js/` — that is what stops a measurement replacing what npm ships,
    # and a stub has been published once already.
    ./bindings/wasm/build.sh --no-default-features \
        --features ferrodoc/markdown,ferrodoc/html >/dev/null 2>&1
    local trimmed=bindings/wasm/target/wasm32-unknown-unknown/release/ferrodoc_wasm.wasm
    local trimmed_wasm trimmed_wasm_gz
    trimmed_wasm=$(stat -c%s "$trimmed")
    trimmed_wasm_gz=$(gzipped "$trimmed")

    within "CLI, every format"        "$cli"             6878440 5
    within "CLI, markdown + html"     "$trimmed_cli"     4126744 5
    within "wasm, every format"       "$wasm"            1907341 5
    within "wasm gzipped"             "$wasm_gz"          703010 5
    within "wasm, markdown + html"    "$trimmed_wasm"    1186745 5
    within "wasm gzipped, trimmed"    "$trimmed_wasm_gz"  410922 5

    # The headline table's own row, which nothing checked until
    # 2026-08-24 and which said **4.6 MB** while the binary was 6.9 — it
    # was written before four of the eleven format crates existed. Both
    # sides of the comparison are measured: pandoc's binary is on `PATH`
    # here, so the ratio is derived rather than remembered.
    local pandoc_bytes megabytes pandoc_mb ratio disk
    local pandoc_path
    pandoc_path=$(command -v pandoc) || {
        printf '  MISSING  %-29s pandoc is not on PATH to measure\n' "the disk row"
        failures=$((failures + 1)); checked=$((checked + 1))
        return
    }
    pandoc_bytes=$(stat -Lc%s "$pandoc_path")
    megabytes=$(awk -v b="$cli" 'BEGIN { printf "%.1f", b / 1000000 }')
    pandoc_mb=$(awk -v b="$pandoc_bytes" 'BEGIN { printf "%.1f", b / 1000000 }')
    ratio=$(awk -v a="$pandoc_bytes" -v b="$cli" 'BEGIN { printf "%.0f", a / b }')
    disk='| **Binary / dependency on disk** |'
    local row said_ours said_pandoc said_ratio
    row=$(grep -F -- "$disk" README.md || true)
    if [ -z "$row" ]; then
        printf '  MISSING  %-29s README.md has no disk row\n' "the disk row"
        failures=$((failures + 1)); checked=$((checked + 1))
        return
    fi
    said_pandoc=$(printf '%s' "$row" | grep -oE '\| [0-9]+\.[0-9] MB \|' | tr -dc '0-9.')
    said_ours=$(printf '%s' "$row" | grep -oE '\*\*[0-9]+\.[0-9] MB\*\*' | tr -dc '0-9.')
    said_ratio=$(printf '%s' "$row" | grep -oE '\*\*[0-9]+× smaller\*\*' | tr -dc '0-9')
    # A tenth of a megabyte is finer than the binary is reproducible: the
    # same checkout builds 0.7% smaller on a CI runner than here, which is
    # a fact about linkers. The same 5% bound as the byte counts.
    near "disk, ferrodoc MB"  "$megabytes"    "$said_ours"   5
    # Pandoc's binary is a **pinned release**, so its size moves only when
    # the platform does; two percent catches a figure left behind by a
    # pandoc upgrade, which five did not — 152.9 against 160.4 is 4.7%.
    near "disk, pandoc MB"    "$pandoc_mb"    "$said_pandoc" 2
    near "disk, the ratio"    "$ratio"        "$said_ratio"  5

    # No exact check on any byte count. The CLI is reproducible to the
    # byte in one checkout with one toolchain and **not across machines**:
    # this ran 0.68% smaller on a CI runner than on the machine the
    # figures were taken on, which is a fact about linkers, not a drift.
    # The counts in README illustrate; the bound above is what gates.

    # The ratios are what README claims; the byte counts illustrate them.
    local cli_ratio wasm_ratio
    cli_ratio=$(awk -v a="$trimmed_cli" -v b="$cli" 'BEGIN { printf "%.0f", a * 100 / b }')
    wasm_ratio=$(awk -v a="$trimmed_wasm_gz" -v b="$wasm_gz" 'BEGIN { printf "%.0f", a * 100 / b }')
    # The ratios are the claim, so they are compared as numbers rather
    # than as text: matching the printed string would turn 59.7% into a
    # failure or a pass depending on which side of a rounding boundary a
    # runner landed.
    local sentence='of its gzipped size and the CLI binary to' line
    line=$(grep -F -- "$sentence" README.md || true)
    if [ -z "$line" ]; then
        printf '  MISSING  %-34s no line matching "%s"\n' README.md "$sentence"
        failures=$((failures + 1)); checked=$((checked + 1))
        return
    fi
    local said_wasm said_cli
    said_wasm=$(printf '%s' "$line" | grep -oE '\*\*[0-9]+%\*\*' | tr -dc 0-9)
    said_cli=$(printf '%s' "$line" | grep -oE 'to [0-9]+% of its own' | tr -dc 0-9)
    point "wasm gzipped, trimmed / full" "$wasm_ratio" "$said_wasm"
    point "CLI trimmed / full"           "$cli_ratio"  "$said_cli"
}

case "${1-}" in
    --gates) echo "== published gate figures"; check_gates "${2?usage: --gates FILE}" ;;
    --slow)  echo "== published figures no gate produces"; check_slow ;;
    --sizes) echo "== published sizes"; check_sizes ;;
    *) echo "usage: $0 --gates FILE | --slow | --sizes" >&2; exit 2 ;;
esac

if [ "$failures" = 0 ]; then
    echo "  $checked published figures still derive"
else
    echo "  $failures of $checked published figures no longer derive"
fi
exit "$failures"
