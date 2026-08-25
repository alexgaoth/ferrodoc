#!/usr/bin/env bash
# Would a user notice? Run real pandoc command lines through both
# binaries and compare the bytes.
#
# Every other gate in this repository scores an *AST* or a single
# conversion with the flags chosen by the gate. None of them can answer
# the question a person actually asks before replacing `pandoc` with
# `ferrodoc` in a Makefile. This can, and its answer is one number:
#
#   N/M command lines identical
#
# with every miss classified:
#
#   fixable        both produced output and the bytes differ
#   out of surface ferrodoc refused, naming a flag or format it does not have
#   deliberate     a difference this project has decided to keep, listed below
#   pandoc refused pandoc could not run it here; the row is not counted
#
# The corpus is `dropin/commands.tsv` — real invocations from
# public Makefiles, CI files and scripts, each with the repository it came
# from. See `dropin/README.md`.
#
#   scripts/dropin.sh              the number, and one line per miss
#   scripts/dropin.sh --fail-under N   ...and exit non-zero below N rows
#   scripts/dropin.sh --verbose    and the first lines of each diff
#   scripts/dropin.sh --attribute  and what one change would fix each miss
#   scripts/dropin.sh dropin-013   one row, with its diff
#
# `--attribute` is what turns the number into work. It retries every miss
# with one hypothesis at a time — pandoc without its default syntax
# highlighting, ferrodoc filling to 72 columns as pandoc does — and says
# which single decision would make each row identical. A row that still
# differs after both is the actual remainder.
set -uo pipefail
cd "$(dirname "$0")/.."

PANDOC_PINNED=3.8.2.1
FERRODOC=${FERRODOC:-./target/release/ferrodoc}
CORPUS=dropin/commands.tsv

verbose=0
attribute=0
only=""
floor=0
case "${1-}" in
    --verbose) verbose=1 ;;
    --attribute) attribute=1 ;;
    --fail-under) floor=${2:?--fail-under needs a count} ;;
    "") ;;
    dropin-*) only="$1"; verbose=1 ;;
    *) echo "usage: $0 [--verbose|--attribute|--fail-under N|dropin-NNN]" >&2; exit 2 ;;
esac

have=$(pandoc --version 2>/dev/null | head -1 | awk '{print $2}')
if [ "$have" != "$PANDOC_PINNED" ]; then
    echo "pandoc $PANDOC_PINNED is what this compares against; found '${have:-none}'" >&2
    exit 2
fi
[ -x "$FERRODOC" ] || { echo "build it first: cargo build --release -p ferrodoc" >&2; exit 2; }

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

# A difference this project has decided to keep. Each entry is an id and
# the reason, and each reason must also be a row in COMPATIBILITY.md —
# this list is a pointer, not a place to retire a failure.
deliberate() {
    case "$1" in
        # The LaTeX writer sets an ordered list's counter **before** its
        # label, and pandoc writes them the other way round. Pandoc's own
        # reader takes the start value from the first directive it meets,
        # so pandoc's order loses it — a list that asked to begin at 3
        # comes back beginning at 1. COMPATIBILITY.md records the
        # measurement. The row needs the dialect as well, and is listed
        # here because it cannot become identical even with it: that is
        # what this list means, and why it is not counted as work.
        dropin-006) return 0 ;;
        *) return 1 ;;
    esac
}

# Retry one miss with pandoc's own features switched off, one
# combination at a time, and name the smallest set that makes the two
# agree. Switching them off on **pandoc's** side is the right way round:
# the user's command line is fixed, so the question is what ferrodoc
# would have to gain, and neutralising the feature in pandoc models
# exactly that.
#
# Three decisions, each of which touches every line of a conversion:
#
#   highlighting  pandoc colours code by default; this does not (0.7)
#   wrap          pandoc fills to 72 columns; this preserves (card D4.3)
#   dialect       `-f markdown` is pandoc's dialect there and CommonMark
#                 here, and pandoc's gives every heading an identifier
#                 (card D4.4)
#
# Anything still differing after all three is a real remainder and worth
# a bug. A fourth entry here would be a claim that a fourth global
# decision exists — measure before adding one.
attribute_miss() {
    local id="$1" p_args="$2" f_args="$3"
    local dir="$work/$id.try" combination try_p try_f mine
    # The hypothesis is about pandoc's `markdown`, so it applies wherever
    # *that* is the input format however it was named — on the command
    # line, in a defaults file, or inferred from the extension. Skipping
    # every row that spelled `-f markdown` out loud put nine of them in
    # the remainder bucket and kept them there.
    local dialect_applies=0 from=
    from=$(printf '%s\n' $args | awk '
        /^--from=/ { sub(/^--from=/, ""); f = $0; next }
        take       { f = $0; take = 0; next }
        $0 == "-f" || $0 == "--from" { take = 1 }
        END { print f }')
    if [ -z "$from" ]; then
        case " $args " in *--defaults*)
            local defaults
            defaults=$(printf '%s\n' $args | sed -n 's/^--defaults=//p; /^--defaults$/{n;p;}')
            [ -z "$defaults" ] || from=$(sed -n 's/^from:[[:space:]]*//p' "$defaults")
        esac
    fi
    if [ -z "$from" ]; then
        case "$input" in *.md|*.markdown|*.pmd) from=markdown ;; esac
    fi
    case "$from" in markdown|markdown[+-]*) dialect_applies=1 ;; esac
    for combination in \
        "highlighting" "wrap=none" "wrap=preserve" "dialect" \
        "highlighting+wrap=none" "highlighting+wrap=preserve" \
        "highlighting+dialect" "wrap=none+dialect" "wrap=preserve+dialect" \
        "highlighting+wrap=none+dialect" "highlighting+wrap=preserve+dialect"
    do
        case "$combination" in *dialect*) [ "$dialect_applies" = 1 ] || continue ;; esac
        case "$combination" in *wrap*) case " $args " in *--wrap*) continue ;; esac ;; esac
        try_p="$p_args"
        # Highlighting is switched off on **both** sides. Muting pandoc
        # alone was right while ferrodoc highlighted nothing; once it
        # highlighted C, Python and bash the one-sided version blamed
        # eight rows on a difference that no longer existed.
        try_f="$f_args"
        case "$combination" in *highlighting*)
            try_p="$try_p --syntax-highlighting=none"
            try_f="$try_f --no-highlight" ;;
        esac
        case "$combination" in *wrap=none*) try_p="$try_p --wrap=none" ;;
                               *wrap=preserve*) try_p="$try_p --wrap=preserve" ;; esac
        case "$combination" in *dialect*) try_p="$try_p -f commonmark" ;; esac
        rm -rf "$dir"; mkdir -p "$dir/p"
        mine="$f_out"
        case "$combination" in *highlighting*)
            mkdir -p "$dir/f"
            mine="$dir/f"
            local ff=${try_f//$f_out/$dir\/f}
            eval "$FERRODOC $ff" < /dev/null > "$dir/f/stdout" 2> "$dir/f/stderr" || true ;;
        esac
        local pp=${try_p//$p_out/$dir\/p}
        # stderr too: the main comparison includes it, so leaving it out
        # here made every file list differ and every row "remains".
        eval "( ulimit -v 6000000; pandoc $pp )" \
            < /dev/null > "$dir/p/stdout" 2> "$dir/p/stderr" || continue
        if diff -rq "$dir/p" "$mine" >/dev/null 2>&1; then
            printf 'identical once pandoc drops: %s' "$combination"
            return
        fi
    done
    printf 'remains after highlighting, wrap and dialect'
}

total=0 identical=0
declare -a misses=()

while IFS=$'\t' read -r id source input args changed verbatim; do
    case "$id" in ''|'#'*) continue ;; esac
    [ -z "$only" ] || [ "$id" = "$only" ] || continue
    total=$((total + 1))

    p_out="$work/$id.pandoc"
    f_out="$work/$id.ferrodoc"
    mkdir -p "$p_out" "$f_out"

    # `%OUT%` is a path stem, so a row can name several artefacts (an
    # `--extract-media` directory beside the document it repoints).
    p_args=${args//%IN%/$input}; p_args=${p_args//%OUT%/$p_out\/out}
    f_args=${args//%IN%/$input}; f_args=${f_args//%OUT%/$f_out\/out}

    # `eval` because the corpus stores the arguments as a shell would see
    # them, quotes included: `--metadata title="The Super Programmer"` is
    # one argument in the Makefile it came from, and splitting on spaces
    # would make it three.
    # `</dev/null`, because a row with no input file is a filter reading
    # stdin — and stdin here is the corpus file this loop is reading.
    # Without it the first such row ate the rest of the corpus and the
    # run reported 0/5.
    p_status=0
    eval "( ulimit -v 6000000; pandoc $p_args )" \
        < /dev/null > "$p_out/stdout" 2> "$p_out/stderr" || p_status=$?
    f_status=0
    eval "$FERRODOC $f_args" \
        < /dev/null > "$f_out/stdout" 2> "$f_out/stderr" || f_status=$?

    if [ "$p_status" != 0 ]; then
        total=$((total - 1))
        misses+=("$id	pandoc refused	$(head -c 90 "$p_out/stderr" | tr '\n' ' ')")
        continue
    fi

    # Compare every byte either side produced: stdout, and every file
    # under the output directory. A conversion that writes the right
    # document to the wrong place is not identical.
    ( cd "$p_out" && find . -type f | sort ) > "$work/$id.plist"
    ( cd "$f_out" && find . -type f | sort ) > "$work/$id.flist"
    same=1
    if ! diff -q "$work/$id.plist" "$work/$id.flist" >/dev/null; then
        same=0
        detail="different files written"
    elif ! diff -rq "$p_out" "$f_out" >/dev/null 2>&1; then
        same=0
        detail=$(diff -rq "$p_out" "$f_out" 2>&1 | head -n1 | sed "s|$work/||g")
    elif [ "$f_status" != "$p_status" ]; then
        same=0
        detail="exit $f_status against pandoc's $p_status"
    fi

    if [ "$same" = 1 ]; then
        identical=$((identical + 1))
        [ "$verbose" = 0 ] || printf '  %-12s identical\n' "$id"
        continue
    fi

    if [ "$f_status" != 0 ] && grep -qiE 'unknown|not compiled|cannot|unsupported|no such (format|option)' "$f_out/stderr"; then
        class="out of surface"
        detail=$(head -c 90 "$f_out/stderr" | tr '\n' ' ')
    elif deliberate "$id"; then
        class="deliberate"
    else
        class="fixable"
    fi
    if [ "$attribute" = 1 ] && [ "$class" = fixable ]; then
        detail=$(attribute_miss "$id" "$p_args" "$f_args")
    fi
    misses+=("$id	$class	$detail")
    if [ "$verbose" = 1 ]; then
        printf '  %-12s %s\n' "$id" "$class"
        diff -r "$p_out" "$f_out" 2>&1 | head -n 12 | sed 's/^/      /'
    fi
done < "$CORPUS"

echo
if [ "${#misses[@]}" -gt 0 ]; then
    printf '%s\n' "${misses[@]}" | sort -t$'\t' -k2,2 |
        while IFS=$'\t' read -r id class detail; do
            printf '  %-12s %-15s %s\n' "$id" "$class" "$detail"
        done
    echo
fi
percent=$(awk -v a="$identical" -v b="$total" 'BEGIN { printf "%.1f", b ? a * 100 / b : 0 }')
# One line, because `verify.sh` reports a gate by its last: the count of
# refusals belongs beside the score rather than under it.
refused=$(printf '%s\n' "${misses[@]}" | grep -c 'out of surface' || true)
echo "$identical/$total command lines identical ($percent%), $refused refused for a missing flag"
# A count rather than a percentage: the corpus is 48 rows and a
# percentage floor over a corpus that small tolerates a whole row going
# backwards. It is a count of rows that must keep working.
if [ "$identical" -lt "$floor" ]; then
    echo "below the floor of $floor" >&2
    exit 1
fi
