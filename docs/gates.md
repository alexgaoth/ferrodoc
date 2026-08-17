# The gates

The differential gates, with the thresholds they are held to. **Run every
one of them before claiming a reader or writer change is done** — each is
cheap, and between them they are the only reason to believe any number this
project publishes.

These are also the `conformance` job of `.github/workflows/ci.yml`, and the
table in `COMPATIBILITY.md`. **Keep all three in step when a threshold
moves**; a threshold changed here and nowhere else is a gate that has quietly
stopped gating.

Conformance is pinned to **pandoc 3.8.2.1**. A different pandoc produces
spurious diffs, so a green run means "identical to this version", not
"identical to whatever pandoc was on the machine".

```sh
export PATH="$HOME/.cargo/bin:$HOME/.local/bin:$PATH"
H="cargo run -q -p ferrodoc-harness --"

$H diff-spec       corpus/commonmark-spec-0.31.2.json --fail-under 100
$H diff-ast        corpus --fail-under 100
$H diff-html       corpus/commonmark-spec-0.31.2.json --fail-under 100
$H diff-docx       corpus/docx --fail-under 96
$H diff-docx       corpus/docx-libreoffice --fail-under 87
$H diff-write      corpus --fail-under 90
$H diff-odt        corpus/odt --fail-under 94
$H diff-odt        corpus/odt-libreoffice --fail-under 100
$H diff-odt-write  corpus --fail-under 100
$H diff-md         corpus/commonmark-spec-0.31.2.json --fail-under 100
$H diff-gfm        corpus/gfm --fail-under 100
$H diff-gfm        corpus/commonmark-spec-0.31.2.json --fail-under 99.8
$H diff-gfm-md     corpus/gfm corpus/commonmark-spec-0.31.2.json --fail-under 100
$H diff-html-read  corpus/commonmark-spec-0.31.2.json corpus --fail-under 95
```

Plus, always:

```sh
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings   # pedantic + missing_docs are deny-level
```

And after touching a **reader**, because a reader's contract is that it
refuses hostile input rather than panicking:

```sh
cargo run -q --release -p ferrodoc-harness -- fuzz corpus --iters 500000
```

`FERRODOC_FUZZ_SEED` varies the search; CI sets it from the run id so the
search keeps moving instead of re-checking the same inputs forever. A short
fixed-seed run is already in `cargo test`.

## Reading a failure

Each gate prints `MISMATCH <case> at <json-pointer>: ours=… theirs=…`,
pointing at the **first** divergence in document order. That pointer is a
path into the pandoc JSON AST, so `/blocks/4/c/0/c` is
`blocks[4].c[0].c` — usually enough to identify the rule without dumping
either tree. `--verbose` prints the whole case.

A gate that *rises* deserves as much suspicion as one that falls: widening
what a gate collects can raise the score while covering less. That happened
here — eight DOCX corpus sources landed in the HTML gate, passed, and pushed
the number up.
