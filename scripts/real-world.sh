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

score() {
  language=$1
  shift
  same=0 total=0
  for file in "$@"; do
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
score c $(ls /usr/include/*.h 2>/dev/null | head -40)
score python $(ls /usr/lib64/python3*/*.py /usr/lib/python3*/*.py 2>/dev/null | head -40)
score bash $(grep -rl '^#!/bin/bash\|^#!/usr/bin/env bash' /usr/bin 2>/dev/null | head -40)
[ -n "$ruby_lib" ] && score ruby $(ls "$ruby_lib"/*.rb 2>/dev/null | head -40)
