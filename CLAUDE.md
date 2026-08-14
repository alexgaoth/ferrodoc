# CLAUDE.md

Pandoc-compatible document converter, built to be adopted outside this repo.
Two contracts: output must be value-identical to pandoc's, proven
differentially and never assumed; and every advantage claimed to a reader
must be a number someone else can reproduce.

Per-crate gotchas live in `crates/*/CLAUDE.md`; this file is what holds
repo-wide.

## Commands

- Toolchain lives off default PATH on this machine:
  `export PATH="$HOME/.cargo/bin:$HOME/.local/bin:$PATH"` (cargo + pandoc 3.8.2.1).
  Conformance claims are pinned to pandoc 3.8.2.1; a different pandoc will
  produce spurious diffs.
- Verify any reader/writer change with every gate below before claiming done.
  They are the conformance job of `.github/workflows/ci.yml`; keep the two in
  step when a threshold moves.
  `cargo run -q -p ferrodoc-harness -- diff-spec corpus/commonmark-spec-0.31.2.json --fail-under 100`
  `cargo run -q -p ferrodoc-harness -- diff-ast corpus --fail-under 100`
  `cargo run -q -p ferrodoc-harness -- diff-html corpus/commonmark-spec-0.31.2.json --fail-under 100`
  `cargo run -q -p ferrodoc-harness -- diff-docx corpus/docx --fail-under 96`
  `cargo run -q -p ferrodoc-harness -- diff-write corpus --fail-under 90`
  `cargo run -q -p ferrodoc-harness -- diff-md corpus/commonmark-spec-0.31.2.json --fail-under 100`
  `cargo run -q -p ferrodoc-harness -- diff-gfm corpus/gfm --fail-under 100`
  `cargo run -q -p ferrodoc-harness -- diff-gfm corpus/commonmark-spec-0.31.2.json --fail-under 99.8`
  `cargo run -q -p ferrodoc-harness -- diff-gfm-md corpus/gfm corpus/commonmark-spec-0.31.2.json --fail-under 100`
  `cargo run -q -p ferrodoc-harness -- diff-html-read corpus/commonmark-spec-0.31.2.json corpus --fail-under 95`
  plus `cargo test --workspace` and `cargo clippy --workspace --all-targets -- -D warnings`
  (pedantic + missing_docs are deny-level).
- Never run `cargo fmt`: the repo is not fmt-clean (128 files differ), so it
  buries a surgical change in unrelated reformatting. Match surrounding style
  by hand.
- After touching a reader also run `ferrodoc-harness fuzz corpus --iters
  500000` (`FERRODOC_FUZZ_SEED` varies the search); it requires every reader
  to refuse rather than panic. A short fixed-seed run is in `cargo test`.
- `crates/ferrodoc` is the entry point (facade + `ferrodoc` binary): a new
  reader or writer is unreachable by users until added to its `Format` enum,
  `parse`/`render`, and the `--help` text in `main.rs`.

## Rules

- Never guess pandoc behavior — probe it first
  (`printf '...' | pandoc -f commonmark -t json`), then encode the probed rule
  with a comment. Every quirk in ferrodoc-markdown was derived this way.
- **Pandoc's GitHub sources describe a later pandoc than the 3.8.2.1 binary**
  and disagree with it (numbering keyed on numId vs abstractNumId, ColWidth 0,
  `isRestart`). Read them for algorithm shape only; the binary decides.
- A behavior with no corpus document that fails without it is not covered.
  Mutation-test a new rule by breaking it and confirming the corpus drops.
  Restore with a `cp` of a copy taken first, never `git checkout` — work in
  progress here is usually uncommitted and that deletes it.
- A round-trip gate cannot see a rule whose two spellings read back the same
  (`- ☐ a` and `- [ ] a` are one AST). Those need a test on the literal
  output, or they ship broken with CI green.
- A percentage threshold over a large corpus is not a regression gate: 99%
  of 654 tolerates five failures. Gate a small hand-written corpus at 100
  separately from the spec run.
- Code no input has ever reached is not code that works, and every gate is
  green while it stays unreached. Making a dormant path live — `docx → docx`
  first embedding a picture, a fixture stopping being skipped — is a change
  to verify end to end, not by its diff. Both shipped breakage that way.
- Never compare builds by absolute timings: this machine drifts ~2× within a
  session. Interleave against a baseline worktree and report the ratio.
  (`bench-sizes` prints absolute per-path latency on purpose — for users
  sizing a pipeline, not for judging a change.)
- Regenerate benchmark inputs in `/tmp` first: a missing one fails *quietly*
  (`bench` prints nothing, `/usr/bin/time` reports pandoc's ~12 MB error path
  as a conversion). `/tmp` is **tmpfs**, so fixtures and any baseline
  `target/` are RAM — delete them as soon as the number is taken.
- A README claim needs its reproducing command, figures from one sitting, and
  pandoc's advantages beside it — selling wins without limits is what makes a
  reader stop trusting the wins.
- Measure every optimization against the code it replaces, and *ablate first*
  to find where the cost is: interning names in the DOCX tree would have won
  8% where deleting the tree won 5×. Slice-scanning escapers lost to a
  per-character loop and `with_capacity` lost to growing. Revert what does
  not measure.

## Gotchas

- Every crate must keep compiling for `wasm32-unknown-unknown`, so no crate
  below the facade may do IO or pull in a C library.
- All readers bound their recursion and must return `Err`, never abort or
  truncate. Keep bounds low: a test thread gets 2 MiB, so 500 overflows the
  suite the bound exists to protect; 200 works.
- A depth bound protects nothing a walk can bypass — a helper that never
  calls it, or dropping the tree, which `Rc` does recursively (hence
  `flatten()`). Walk iteratively wherever the bound is not consulted.
- `Vec::remove`/`insert(0, …)` inside a scan is quadratic and has shipped four
  times here; ordinary pages hit it (200k `x<br>`+newline never finished).
  Build a new `Vec` in one pass, or use `retain`/`drain`. Same trap in
  identifier uniquing: resume the `-N` suffix search, never restart it.
- Known gaps live in each crate's docs; `.iterate/*/` holds the critic verdicts
  behind them, including fixes made after a run hit its round cap. `TODO.md`
  has the roadmap and the deliberate non-goals, `COMPATIBILITY.md` every known
  loss with its reproducing command — update both when a number moves.
