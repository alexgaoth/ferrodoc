# ferrodoc

A universal document converter in Rust, pandoc-compatible at the AST level.

| crate | what it does | conformance vs pandoc 3.8.2.1 |
|---|---|---|
| `ferrodoc-ast` | the pandoc-types 1.23 AST | any `pandoc -t json` document round-trips to an equal value |
| `ferrodoc-markdown` | CommonMark reader | **652/652** spec examples produce identical ASTs |
| `ferrodoc-html` | HTML writer | **652/652** spec examples produce identical HTML |
| `ferrodoc-docx` | DOCX reader | **36/37** corpus documents produce identical ASTs |
| `ferrodoc-docx` | DOCX writer | **643/652** spec examples survive a docx round trip identically |
| `ferrodoc-text` | plain-text extraction | best-effort by design, not pandoc-`plain` parity |
| `ferrodoc-harness` | the differential oracle | `diff-ast`, `diff-spec`, `diff-html`, `diff-docx`, `diff-write`, `bench` |

Nothing here is trusted because it looks right: every claim above is produced
by running the real pandoc binary side by side and comparing full documents.

## Differential testing

```sh
cargo run -p ferrodoc-harness -- diff-spec  corpus/commonmark-spec-0.31.2.json --fail-under 100
cargo run -p ferrodoc-harness -- diff-ast   corpus --fail-under 100
cargo run -p ferrodoc-harness -- diff-html  corpus/commonmark-spec-0.31.2.json --fail-under 100
cargo run -p ferrodoc-harness -- diff-docx  corpus/docx --fail-under 96
cargo run -p ferrodoc-harness -- diff-write corpus
```

`diff-write` is the writer's oracle: both engines write the same AST to a
`.docx`, pandoc reads both back, and the two documents must match. Comparing
zip bytes would be meaningless; comparing what the format preserves is the
real contract.

## What the Rust rewrite actually buys

Measured on this machine (Linux x86-64, pandoc 3.8.2.1, release build). Every
number below is reproducible with the commands in this repo — none is an
estimate.

### 1. Throughput: 83×–536× faster in the shape pipelines actually use

| input | ferrodoc (in-process) | pandoc (subprocess) | ratio |
|---|---|---|---|
| 16 KB (concatenated spec examples) | 1.23 ms | 102.6 ms | **83×** |
| 0.6 KB README | 34 µs | 18.0 ms | **536×** |

The comparison is in-process library call vs `pandoc` subprocess, because that
is the choice a real pipeline makes: pandoc is a binary, so using it from
another program *means* spawning a process and serializing through it. The
small-document ratio is dominated by that startup, which is exactly the cost
document pipelines pay per file today.

`cargo run -p ferrodoc-harness -- bench <file>`

### 2. Startup: 13 ms → 2 ms per invocation

Even as a plain CLI, converting a one-line document takes pandoc ~13 ms and
ferrodoc ~2 ms — a Haskell runtime plus a 153 MB binary has to be paged in
before any work happens.

### 3. Memory: 12.8 MB → 3.4 MB peak RSS

Peak resident set converting the 16 KB document to HTML (`/usr/bin/time -v`).
No GC, no runtime: memory tracks the document, not the interpreter.

### 4. Distribution: 153 MB → 3.1 MB

`pandoc` is 152.9 MB on disk. The entire ferrodoc harness — every reader,
every writer and the test tooling — is 3.1 MB, and a library user links only
what they call. This is the difference between a container image you notice
and one you don't.

### 5. It runs where pandoc cannot: the browser

All five crates, including the DOCX reader and writer, compile to
`wasm32-unknown-unknown`:

```sh
cargo build --release --target wasm32-unknown-unknown \
  -p ferrodoc-ast -p ferrodoc-markdown -p ferrodoc-html -p ferrodoc-text -p ferrodoc-docx
```

Document conversion in a browser tab or an edge worker, with no server round
trip and no document leaving the client, is not something a GHC-compiled
pandoc offers today.

### 6. It survives hostile input that hangs pandoc

Documents are attacker-controlled in any upload pipeline. Two of them, found
by fuzzing during review:

| malformed `.docx` | pandoc | ferrodoc |
|---|---|---|
| footnote that references itself | **hung** (killed at 60 s) | error in 14 ms |
| 20,000 increasingly-nested list items | **hung** (killed at 60 s) | handled in 3.5 s |

Every recursive path is depth-bounded and the reader returns `Err` instead of
aborting; `unsafe` is forbidden crate-wide, so a malformed document cannot
become a memory-safety bug.

### 7. Reproducible output

Writing the same document twice produces byte-identical `.docx` files.
Pandoc's differ (embedded timestamps), which defeats content-addressed caching
and makes "did this document change?" un-answerable without parsing.

```sh
cargo run -p ferrodoc-harness --example determinism corpus/readme-style.md
```

### 8. A library, not a subprocess

Callers get typed values — `Pandoc`, `Block`, `Inline` with named fields —
that they can inspect and transform in memory. Using pandoc programmatically
means shelling out, or serializing to JSON and back, per document.

### Where pandoc is still ahead

Honesty matters more than the table above. Pandoc supports ~40 formats to
ferrodoc's four; it has citations, templates, Lua filters, PDF output and
fifteen years of edge cases. It also *preserves* things this writer does not
yet: images need media parts, raw blocks have nowhere to go in OOXML. The bet
is not that this replaces pandoc, but that the common path — markdown, HTML
and DOCX, called from a program rather than a shell — is worth doing natively.

## License notes

`corpus/commonmark-spec-0.31.2.json` is the example set from the
[CommonMark spec](https://spec.commonmark.org/0.31.2/), © John MacFarlane,
licensed CC-BY-SA 4.0.
