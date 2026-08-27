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

# Paths arrive on stdin, one per line, rather than as arguments, so that a
# path holding a space or a glob character survives.
#
# The denominators drift. This sweep found 22 shell scripts in /usr/bin one
# hour and 24 the next, because the machine installed packages in between.
# That is the nature of the corpus and not a fault in it — but it is why
# the figures here are dated where they are quoted, and why this script
# reports rather than gates.
score() {
  language=$1
  same=0 total=0
  while IFS= read -r file; do
    [ -f "$file" ] || continue
    total=$((total + 1))
    { printf '```%s\n' "$language"; cat "$file"; printf '```\n'; } > "$work/in.md" 2>/dev/null
    ( ulimit -v 6000000; pandoc "$work/in.md" -f commonmark -t html --wrap=none ) \
      > "$work/pandoc.html" 2>/dev/null
    "$FERRODOC" "$work/in.md" -f commonmark -t html --wrap=none > "$work/ours.html" 2>/dev/null
    cmp -s "$work/pandoc.html" "$work/ours.html" && same=$((same + 1))
  done
  if [ "$total" -eq 0 ]; then
    printf '  %-8s no files found on this machine\n' "$language"
  else
    printf '  %-8s %s/%s\n' "$language" "$same" "$total"
  fi
}

ruby_lib=$(find "$HOME/.cache" -type d -name '3.4.0' -path '*ruby*' 2>/dev/null | head -1)
echo "highlighting against code written for somebody else:"
ls /usr/include/*.h 2>/dev/null | head -40 | score c
ls /usr/lib64/python3*/*.py /usr/lib/python3*/*.py 2>/dev/null | head -40 | score python
grep -rl '^#!/bin/bash\|^#!/usr/bin/env bash' /usr/bin 2>/dev/null | head -40 | score bash
[ -n "$ruby_lib" ] && ls "$ruby_lib"/*.rb 2>/dev/null | head -40 | score ruby
