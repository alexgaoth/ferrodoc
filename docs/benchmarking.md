# Benchmarking ferrodoc

Read this before publishing any number. Every rule here exists because a
figure that was wrong got published, or nearly did.

## Where fixtures live

`bash corpus/bench/generate.sh` writes 10 KB / 1 MB / 10 MB of generated
prose to `~/.cache/ferrodoc-bench`. **Not `/tmp`**, which is tmpfs on this
machine: a 10 MB fixture and everything derived from it would be resident
memory rather than disk, and so would a baseline `target/`. Delete any
baseline tree as soon as the number is taken.

Generated prose, not a tiled corpus document. Tiling changes what a
document *is*: `corpus/truncation-cases.md` holds unterminated HTML
comments, and past the first repetition each one swallows the closing tags
of everything after it, so the document nests without bound and the HTML
reader refuses it.

## The failure that motivates all of this

**The fastest way to read a document is to refuse it.** `HTML -> AST` was
published at 4.12 s for 10 MB, and it was not a conversion — it was the
cost of refusing a document that nested past the 200-level bound, timed by
a loop that discarded the `Result`. The reader is linear.

A missing fixture fails just as quietly: `bench` prints nothing, and
`/usr/bin/time` reports pandoc's ~12 MB error path as if it were a
conversion.

`bench-sizes` now runs every read once for real and fails loudly. Do not
reintroduce a timing loop that discards a `Result`.

## Comparing

- **Never compare builds by absolute timings.** This machine drifts ~2×
  within a session. Interleave against a baseline worktree and report the
  ratio. (`bench-sizes` prints absolute per-path latency on purpose — for
  users sizing a pipeline, not for judging a change.)
- **Ablate before optimizing**, to find where the cost actually is:
  interning names in the DOCX tree would have won 8% where deleting the
  tree won 5×. Slice-scanning escapers lost to a per-character loop, and
  `with_capacity` lost to growing. Revert what does not measure.
- A README claim needs its reproducing command, figures from one sitting,
  and pandoc's advantages beside it. Selling wins without limits is what
  makes a reader stop trusting the wins.
- A timing *threshold* in a test must scale to the machine: a bound read
  off a 16-core box fails on a 4-core CI runner while the code is correct.

## The published figures

`README.md` holds the published figures and the command that reproduces
each. `docx → AST` is the one superlinear path — 16.9× the time for 10× the
input — and the only one worth re-measuring when a large document feels
slow.
