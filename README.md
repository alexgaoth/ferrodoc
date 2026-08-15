# ferrodoc

A universal document converter in Rust — markdown (CommonMark and GFM),
HTML and DOCX — that produces the same output as pandoc, and produces it
far faster.

## ferrodoc vs pandoc

Every row measured on this machine in one sitting (Linux x86-64, pandoc
3.8.2.1, release build), reproducible with the commands below.

| | pandoc | ferrodoc | |
|---|---|---|---|
| **Convert 16 KB markdown → HTML** | 110 ms | **1.4 ms** | **77× faster** |
| **Convert 0.6 KB markdown → HTML** | 21 ms | **46 µs** | **462× faster** |
| **Startup** (one tiny document) | 13 ms | **2 ms** | **6× faster** |
| **Peak memory**, 0.6 KB document | 65 MB | **3.8 MB** | **17× less** |
| **Peak memory**, 16 KB document | 207 MB | **4.7 MB** | **44× less** |
| **Binary on disk** | 153 MB | **4.5 MB** | **34× smaller** |
| **Malformed DOCX** (self-referential footnote) | hangs, killed at 60 s | **handled in 12 ms** | — |
| **Same document written twice** | different bytes | **identical bytes** | — |
| **Runs in a browser / edge worker** | no | **yes** (wasm32) | — |

DOCX, in process: writes the 16 KB document in 1.7 ms and reads it back in
5.0 ms (0.22 ms / 0.34 ms for the small one) — a path pandoc can only offer
through a subprocess.

The throughput rows compare an in-process library call against a `pandoc`
subprocess, because that is the choice a real pipeline makes: pandoc is a
binary, so calling it from a program *means* spawning a process per document.
The small-document ratio is dominated by that startup — precisely the cost
document pipelines pay on every file today.

```sh
python3 -c "import json; print('\n\n'.join(e['markdown'] for e in json.load(open('corpus/commonmark-spec-0.31.2.json'))))" > /tmp/bigdoc.md
cargo build --release
./target/release/ferrodoc-harness bench      /tmp/bigdoc.md corpus/readme-style.md --iters 300
./target/release/ferrodoc-harness bench-docx /tmp/bigdoc.md corpus/readme-style.md --iters 200
cargo run -p ferrodoc-harness --example determinism corpus/readme-style.md
```

Absolute timings move with CPU state; the ratios hold. Compare interleaved
runs, never numbers from different sittings.

## And the output is the same

Speed is worthless if the document changes. Every claim here comes from
running the real pandoc binary side by side and comparing whole documents —
nothing is trusted because it looks right.

| crate | conformance vs pandoc 3.8.2.1 |
|---|---|
| `ferrodoc-ast` | any `pandoc -t json` document round-trips to an equal value |
| `ferrodoc-markdown` | **652/652** CommonMark spec examples produce identical ASTs |
| `ferrodoc-markdown` GFM reader | **654/655** documents produce identical ASTs |
| `ferrodoc-html` | **652/652** spec examples produce identical HTML |
| `ferrodoc-docx` reader | **36/37** corpus documents produce identical ASTs, and **7/8** documents written by LibreOffice rather than pandoc |
| `ferrodoc-docx` writer | **643/652** spec examples survive a DOCX round trip identically, with embedded images and document metadata |
| `ferrodoc-markdown` writer | **652/652** spec examples survive a markdown round trip identically (pandoc: 593/652) |
| `ferrodoc-markdown` GFM writer | **655/655** documents survive a GFM round trip identically (pandoc: 589/655) |
| `ferrodoc-html` reader | **633/659** HTML documents produce identical ASTs |

```sh
cargo run -p ferrodoc-harness -- diff-spec  corpus/commonmark-spec-0.31.2.json --fail-under 100
cargo run -p ferrodoc-harness -- diff-ast   corpus --fail-under 100
cargo run -p ferrodoc-harness -- diff-html  corpus/commonmark-spec-0.31.2.json --fail-under 100
cargo run -p ferrodoc-harness -- diff-docx  corpus/docx --fail-under 96
cargo run -p ferrodoc-harness -- diff-docx  corpus/docx-libreoffice --fail-under 87
cargo run -p ferrodoc-harness -- diff-write corpus --fail-under 90
cargo run -p ferrodoc-harness -- diff-md    corpus/commonmark-spec-0.31.2.json --fail-under 100
cargo run -p ferrodoc-harness -- diff-gfm    corpus/gfm --fail-under 100
cargo run -p ferrodoc-harness -- diff-gfm    corpus/commonmark-spec-0.31.2.json --fail-under 99.8
cargo run -p ferrodoc-harness -- diff-gfm-md corpus/gfm corpus/commonmark-spec-0.31.2.json --fail-under 100
cargo run -p ferrodoc-harness -- diff-html-read corpus/commonmark-spec-0.31.2.json corpus --fail-under 95
```

`diff-write` is the DOCX writer's oracle: both engines write the same AST to
a `.docx`, pandoc reads both back, and the two documents must match.
Comparing zip bytes would be meaningless; comparing what the format
preserves is the real contract.

`diff-md` measures the markdown writer by fidelity — write the document out,
read it back, require what returns to be what went in — and prints pandoc's
score on the same corpus beside ours. This is the one place ferrodoc is
measurably ahead rather than equal: **652/652 against pandoc's 593/652**,
with pandoc given its best setting (`--wrap=preserve`; `--wrap=none` would
cost it another 58 by flattening soft breaks). Pandoc loses escaped
punctuation, autolinks containing backslashes and code spans with edge
spaces; converting a DOCX to markdown and back changes those documents.
The remaining gaps are limits of CommonMark itself and are listed in
`crates/ferrodoc-markdown/src/write.rs`.

## Install and use

```sh
cargo install ferrodoc                   # installs the `ferrodoc` binary
```

Or take a prebuilt binary from the
[latest release](https://github.com/alexgaoth/ferrodoc/releases/latest) —
Linux (musl), macOS (Intel and Apple silicon), Windows, and a wasm32
package — or add the library to a project:

```toml
[dependencies]
ferrodoc = "0.1"
```

```sh
ferrodoc README.md -o readme.html        # an HTML fragment
ferrodoc README.md -s -o readme.html     # a page a browser can open        # formats inferred from extensions
ferrodoc report.docx -o copy.docx        # DOCX in, DOCX out — keeps its images
ferrodoc report.docx -t gfm             # DOCX in, GitHub markdown out — keeps tables
ferrodoc report.docx -t markdown         # DOCX in, CommonMark out — no table syntax
ferrodoc report.docx -t plain            # DOCX in, text out, to stdout
cat notes.md | ferrodoc -f markdown -t docx -o notes.docx
ferrodoc --help                          # every option and format
```

Inputs: `markdown` (`commonmark`, `md`), `gfm`, `html`, `docx`, `json` (the
pandoc AST). Outputs: those plus `plain`. `-s`/`--standalone` wraps HTML
output in a complete page — doctype, charset, `lang`, and the title and
authors the document carries — and `--css FILE` inlines a stylesheet into
it.

Prefer `-t gfm` over `-t markdown` for anything with a table: CommonMark has
no table syntax, so a table degrades to one paragraph per cell there.

As a library, one call converts — and the AST is right there when you want to
transform rather than convert, with no subprocess and no JSON round trip:

```rust
use ferrodoc::{Format, ast::Block, convert, parse, render};

let html = convert(b"# Title\n\nHello.\n", Format::Markdown, Format::Html)?;

let mut doc = parse(b"# Title\n\ntext\n", Format::Markdown)?;
doc.blocks.retain(|block| !matches!(block, Block::Header(..)));
let without_headings = render(&doc, Format::Html)?;
```

## Why the differences are structural, not incidental

- **No runtime.** Memory tracks the document, not an interpreter; there is no
  GC pause and nothing to page in before work starts.
- **A library, not a binary.** Callers get typed values — `Pandoc`, `Block`,
  `Inline` — to inspect and transform in memory.
- **Bounded and memory-safe.** Every recursive path is depth-limited and
  readers return `Err` rather than aborting; `unsafe` is forbidden crate-wide,
  so a malformed document cannot become a memory-safety bug. Two fuzz-found
  DOCX files that hang pandoc for 60 s are handled here in 12 ms and 3.5 s.
- **Deterministic output.** The same AST always produces the same `.docx`
  bytes, which makes content-addressed caching and "did this change?" work.
  Pandoc embeds timestamps, so its output differs run to run.
- **Portable.** All five library crates, including the DOCX reader and writer,
  compile to `wasm32-unknown-unknown` — conversion in a browser tab or an edge
  worker, with no document leaving the client.

```sh
cargo build --release --target wasm32-unknown-unknown \
  -p ferrodoc-ast -p ferrodoc-markdown -p ferrodoc-html -p ferrodoc-text -p ferrodoc-docx
```

Every gate, every known loss and every deliberate divergence is listed one
by one in [`COMPATIBILITY.md`](COMPATIBILITY.md), with the command that
produces it. CI runs all nine against a pinned pandoc, and builds and tests on
Linux, macOS and Windows, plus a wasm32 build and a 500,000-mutation fuzz
campaign.

## Where pandoc is still ahead

The table above is not the whole picture, and pretending otherwise would make
the rest less believable. Pandoc supports ~40 formats to ferrodoc's four, plus
citations, templates, Lua filters, PDF output and fifteen years of edge cases.
Our DOCX writer still drops raw blocks, which have no OOXML equivalent, and
embeds eight image formats — PNG, JPEG, GIF, WebP, TIFF, SVG, EMF and WMF —
where anything else becomes alt text. The HTML reader parses to the HTML5
spec, via `html5ever`, where pandoc uses `tagsoup`; on malformed markup — an
unclosed `<a>`, a tag with no closing `>`, a `<pre>` inside a `<tr>` — the
two build different trees, and that is what most of the 26 unmatched
documents are. Not all: `<![CDATA[…]]>` tokenizes differently, and `<a/>`
self-closing syntax sends the two down different recovery paths. The
remaining families are listed in `TODO.md`, one by one. The bet is not that
this replaces pandoc — it is that the common path, markdown/HTML/DOCX
called from a program rather than a shell, is worth doing natively.

## License notes

`corpus/commonmark-spec-0.31.2.json` is the example set from the
[CommonMark spec](https://spec.commonmark.org/0.31.2/), © John MacFarlane,
licensed CC-BY-SA 4.0.
