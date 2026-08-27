#!/usr/bin/env bash
# Score the highlighters against code nobody wrote for this repo.
#
# `highlight.sh` compares 26 files that were chosen — by us — to exercise
# the rules we knew about. It cannot fail on a construct its corpus lacks,
# and for a long time it did not: every language stood at 26/26 there while
# C matched pandoc on 1 real header in 40. This script takes whatever the
# machine already has — system headers, the Python standard library, the
# scripts in /usr/bin — and asks the same question of files chosen by
# somebody else.
#
# It reports; it does not gate. The corpus is whatever this machine holds,
# so the numbers are not comparable between machines and there is no floor
# to fail against. Re-run it after touching a highlighter, and compare
# against the figures recorded in COMPATIBILITY.md.
set -u
cd "$(dirname "$0")/.."
FERRODOC=${FERRODOC:-target/release/ferrodoc}
[ -x "$FERRODOC" ] || { echo "build first: cargo build --release" >&2; exit 1; }
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

# A bash script is a file whose **first line** is a bash shebang.
#
# `grep -rl '^#!/bin/bash' /usr/bin` is not that test, and the difference
# is not academic: it matched `/usr/bin/gh` and `/usr/bin/podman`, which
# are ELF binaries that happen to contain those bytes after a newline
# somewhere inside. They brought 390,000 "lines" of machine code into a
# 2,900-line corpus and quietly moved every figure. It looked at first
# like the package set drifting between runs; it was not, and a wrong
# recorded reason is worse than none.
shell_scripts() {
  find /usr/bin -maxdepth 1 -type f 2>/dev/null | sort | while IFS= read -r file; do
    # 64 bytes is more than any shebang needs, and `tr` drops the nulls
    # a binary's first bytes would otherwise put through the substitution.
    case $(head -c 64 "$file" 2>/dev/null | tr -d '\000' | head -1) in
      '#!'*bash*) printf '%s\n' "$file" ;;
    esac
  done
}

# Paths arrive on stdin, one per line, rather than as arguments, so that a
# path holding a space or a glob character survives.
#
# The corpus is still whatever the machine holds, so the figures are dated
# where they are quoted and this script reports rather than gates.
# Two numbers, because one of them is misleading on its own.
#
# The file score is whole-file byte-identity, and a 3,000-line file needs
# 3,000 consecutive correct lines to earn its point — so a highlighter
# that is right about 95% of lines can score 9/40 and look broken. The
# line figure beside it says how much is actually wrong. Read the file
# score for "is this finished" and the line score for "how far off".
score() {
  language=$1
  same=0 total=0 lines=0 wrong=0
  while IFS= read -r file; do
    [ -f "$file" ] || continue
    total=$((total + 1))
    { printf '```%s\n' "$language"; cat "$file"; printf '```\n'; } > "$work/in.md" 2>/dev/null
    ( ulimit -v 6000000; pandoc "$work/in.md" -f commonmark -t html --wrap=none ) \
      > "$work/pandoc.html" 2>/dev/null
    "$FERRODOC" "$work/in.md" -f commonmark -t html --wrap=none > "$work/ours.html" 2>/dev/null
    cmp -s "$work/pandoc.html" "$work/ours.html" && same=$((same + 1))
    lines=$((lines + $(wc -l < "$file")))
    wrong=$((wrong + $(diff <(tr '>' '>\n' < "$work/pandoc.html") \
                            <(tr '>' '>\n' < "$work/ours.html") | grep -c '^<')))
  done
  if [ "$total" -eq 0 ]; then
    printf '  %-8s no files found on this machine\n' "$language"
  else
    printf '  %-8s %2s/%-3s files   %s of %s lines byte-identical\n' \
      "$language" "$same" "$total" "$((100 * (lines - wrong) / lines))%" "$lines"
  fi
}

ruby_lib=$(find "$HOME/.cache" -type d -name '3.4.0' -path '*ruby*' 2>/dev/null | head -1)
echo "highlighting against code written for somebody else:"
ls /usr/include/*.h 2>/dev/null | head -40 | score c
ls /usr/lib64/python3*/*.py /usr/lib/python3*/*.py 2>/dev/null | head -40 | score python
shell_scripts | head -40 | score bash
[ -n "$ruby_lib" ] && ls "$ruby_lib"/*.rb 2>/dev/null | head -40 | score ruby
# Rust's corpus is this repository — the one language where that is not a
# compromise, since nobody wrote these 31,000 lines to be highlighted.
#
# **`git ls-files`, not `find`.** `find crates bindings -name '*.rs'` pulls
# in `target/**`, and four copies of a *generated* 11,844-line entity table
# turned 40 files into 104,428 lines of machine output. The same mistake as
# the ELF binaries above, from the other direction: ask the tool that knows
# what is source.
git ls-files '*.rs' 2>/dev/null | head -40 | score rust
