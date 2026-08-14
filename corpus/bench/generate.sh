#!/usr/bin/env bash
# Write the three benchmark fixtures the performance figures are measured
# on, at 10 KB, 1 MB and 10 MB. Run from anywhere; they land in
# ~/.cache/ferrodoc-bench and are not committed — they are 11 MB of
# generated prose, and generating them is cheaper than storing them.
#
#   bash corpus/bench/generate.sh
#   cargo run -q --release -p ferrodoc-harness -- bench-sizes \
#       ~/.cache/ferrodoc-bench/{small,medium,large}.md --iters 200
#
# Two things this exists to prevent. Fixtures do NOT go in /tmp, which is
# tmpfs on some machines, so a 10 MB fixture and its derived forms are
# resident memory rather than disk. And the content is ordinary prose
# rather than tiled corpus documents: tiling `corpus/truncation-cases.md`
# produces unterminated HTML comments that swallow the closing tags of
# every repetition after them, so the document nests past the HTML
# reader's 200-level bound and is *refused*. `bench-sizes` used to time
# that refusal and report it as throughput.
set -euo pipefail

python3 - <<'PY'
import os, random

directory = os.path.expanduser('~/.cache/ferrodoc-bench')
os.makedirs(directory, exist_ok=True)
words = "the quick brown fox jumps over a lazy dog while parsing documents at speed and scale".split()

def block(i):
    # Seeded per block, so a fixture is identical on every machine.
    source = random.Random(i)
    line = lambda n: ' '.join(source.choice(words) for _ in range(n))
    kind = i % 7
    if kind == 0: return "## " + line(6).title() + "\n"
    if kind == 1: return f"{line(40)} *{line(3)}* {line(30)} [{line(2)}](https://example.com/{i}) {line(20)}.\n"
    if kind == 2: return "- " + "\n- ".join(line(8) for _ in range(4)) + "\n"
    if kind == 3: return "> " + line(25) + "\n"
    if kind == 4: return "```rust\nfn f%d() -> u32 { %d }\n```\n" % (i, i)
    if kind == 5: return f"{line(35)} `{line(2)}` **{line(3)}** {line(25)}.\n"
    return f"| a | b |\n|---|---|\n| {line(3)} | {line(3)} |\n"

for target, name in [(10 * 1024, 'small.md'), (1024 * 1024, 'medium.md'), (10 * 1024 * 1024, 'large.md')]:
    blocks, i, written = [], 0, 0
    while written < target:
        text = block(i)
        blocks.append(text)
        written += len(text) + 1
        i += 1
    path = os.path.join(directory, name)
    with open(path, 'w') as out:
        out.write('\n'.join(blocks))
    print(f'{path}: {os.path.getsize(path)} bytes')
PY
