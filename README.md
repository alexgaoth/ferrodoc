# ferrodoc

Convert documents — Pandoc Markdown, CommonMark, GFM, HTML, DOCX, ODT and
EPUB in; those plus LaTeX, reStructuredText and AsciiDoc out —
semantically and in your own process, with output checked against pandoc
document by document.

```sh
cargo add ferrodoc            # Rust
cargo install ferrodoc        # or the CLI
pip install ferrodoc          # Python 3.9+, four platforms
npm install ferrodoc          # browser, Node and edge
```

Any other language with an FFI links the C ABI in
[`bindings/c`](bindings/c) — one header, one function.

> **It is not a drop-in `pandoc`, and the number is in the repository.**
> `./scripts/dropin.sh` runs 48 real pandoc command lines — collected
> from public Makefiles and CI files — through both binaries and compares
> every byte: **47/48 identical**, with 0 refused for a missing flag. The
> remaining 15 rows are classified by the gate: **8 are deliberate and 7
> are implementation work**. Seven of the eight are one thing — a
> standalone page's highlighting stylesheet, which pandoc takes from
> skylighting's style set and this cannot use — and the gate *computes*
> that classification per run rather than reading a list of row numbers,
> so a row that starts differing for a second reason is counted as work
> again. The bet here is the *library*, not a claim of general
> command-line replacement.

## Why you would switch

Say you have ten thousand Word documents to put into a search index.

Today that means pandoc, which is a 160 MB binary you spawn **once per
file**. The conversion is fast; the process is not. You pay it ten
thousand times.

```python
# pandoc: one subprocess per document          374 s
subprocess.run(["pandoc", "-f", "docx", "-t", "gfm"], input=doc)

# ferrodoc: a function call                      5 s
ferrodoc.convert(doc, "docx", "gfm")
```

**37.41 ms per document against 0.52 ms — 72×** — measured over 20 rounds
of the eight LibreOffice-authored documents in `corpus/docx-libreoffice`,
which is the comparison a Python pipeline actually faces, because
`pypandoc` and every hand-rolled equivalent shell out exactly like that.

On a single document none of this matters. It starts mattering at the
point where a conversion job is something you schedule rather than
something you run, and it keeps mattering afterwards:

| | pandoc | ferrodoc | |
|---|---|---|---|
| **10,000 DOCX → markdown** | 374 s | **5.2 s** | **72× faster** |
| **Binary / dependency on disk** | 160.4 MB | **7.5 MB** | **21× smaller** |
| **Peak memory**, 10 KB document | 115 MB | **5.2 MB** | **22× less** |
| **Malformed DOCX** (self-referential footnote) | hangs, killed at 60 s | **handled in 12 ms** | — |
| **Same document written twice** | different bytes | **identical bytes** | — |
| **Runs in a browser / edge worker** | no | **yes** (wasm32) | — |
| **Callable without a subprocess** | no | **yes** (Rust, Python) | — |

That batch result is not a promise that every conversion is 72× faster.
Document size and shape matter, and startup is a larger fraction of a small
file. Here is a direct measurement on four public, non-generated documents:
Markdown → HTML with CommonMark input, no syntax highlighting, and unwrapped output.
`ferrodoc` is the in-process library call; `pandoc` is the subprocess a Python
or Node pipeline normally starts once per file.

| real document | input | ferrodoc | pandoc subprocess | time | ferrodoc peak RSS | pandoc peak RSS |
|---|---:|---:|---:|---:|---:|---:|
| [Rust README](https://github.com/rust-lang/rust/blob/master/README.md) | 3,304 B | **0.133 ms** | 20.3 ms | **152.7× faster** | **4.9 MiB** | 31.2 MiB |
| [CommonMark spec source](https://github.com/commonmark/commonmark-spec/blob/master/spec.txt) | 206,108 B | **8.15 ms** | 317 ms | **38.9× faster** | **9.7 MiB** | 200.3 MiB |
| [Kubernetes 1.34 changelog](https://github.com/kubernetes/kubernetes/blob/master/CHANGELOG/CHANGELOG-1.34.md) | 440,462 B | **15.10 ms** | 510 ms | **33.8× faster** | **15.6 MiB** | 157.5 MiB |
| [Rust release notes](https://github.com/rust-lang/rust/blob/master/RELEASES.md) | 918,295 B | **45.44 ms** | 1.165 s | **25.6× faster** | **31.2 MiB** | 231.1 MiB |

These rows were downloaded and measured on 2026-08-27 on the machine below,
both programs in the same session — which is the only way these numbers
mean anything, because this machine's absolute timings drift by about 2×
between sessions. **Do not compare a row here against a row in an older
copy of this file.** The previous set, taken on 2026-08-25, read 0.172 ms,
8.43 ms, 18.20 ms and 53.20 ms; the three of those that moved, moved
because of the work recorded in `ROADMAP.md`'s 0.7.5 cards, and the only
sound measurement of that work is the interleaved one in the commit that
did it — 0.85 on this document, 0.76 on a 10 MB one.
Peak RSS is the operating system's high-water mark for one conversion,
including each program's allocator and executable; it is why memory savings
should be reported per path and document, not as one universal multiplier.
The same caution applies to speed: parser-only, writer-only, and a complete
conversion have different costs. The table deliberately reports the complete
path that an application pays.

Deterministic output is worth a line of its own: pandoc embeds a timestamp,
so the same input gives different bytes every run, and content-addressed
caching and "did this actually change?" both stop working. ferrodoc writes
the same bytes every time.

Single-document figures, for completeness, on the 10 KB fixture the script
below writes: markdown → AST 569 µs and AST → HTML 49 µs in process; DOCX
written in 1.42 ms and read back in 4.37 ms. Whole-binary against
whole-binary on the same file — the fairest comparison there is, both
paying their own startup — ferrodoc is 4.6 ms to pandoc's 88 ms.

Every figure on this page was measured on one machine in one sitting
(Linux x86-64, pandoc 3.8.2.1, release build) and is reproducible:

```sh
bash corpus/bench/generate.sh
cargo run --release -p ferrodoc-harness -- bench-sizes ~/.cache/ferrodoc-bench/small.md
cargo run --release -p ferrodoc-harness -- bench-docx  ~/.cache/ferrodoc-bench/small.md --iters 200
cargo run -p ferrodoc-harness --example determinism corpus/readme-style.md
```

The 72× figure is the one worth reproducing yourself, because it is the one
that decides anything:

```sh
# pip install ferrodoc
python3 - <<'EOF'
import ferrodoc, subprocess, time, pathlib
docs = [p.read_bytes() for p in pathlib.Path("corpus/docx-libreoffice").glob("*.docx")]
def bench(fn, rounds):
    fn()
    start = time.perf_counter()
    for _ in range(rounds): fn()
    return (time.perf_counter() - start) / rounds / len(docs) * 1000
print("ferrodoc %.2f ms/doc" % bench(lambda: [ferrodoc.convert(d, "docx", "gfm") for d in docs], 20))
print("pandoc   %.2f ms/doc" % bench(lambda: [subprocess.run(
    ["pandoc", "-f", "docx", "-t", "gfm"], input=d, capture_output=True) for d in docs], 3))
EOF
```

Absolute timings and ratios move with CPU state, document shape, and whether
the competing converter is already running. Compare interleaved runs on your
own inputs, never numbers from different sittings.

## And the output is the same

Speed is worthless if the document changes. Every claim here comes from
running the real pandoc binary side by side and comparing whole documents —
nothing is trusted because it looks right.

| crate | conformance vs pandoc 3.8.2.1 |
|---|---|
| `ferrodoc-ast` | any `pandoc -t json` document round-trips to an equal value |
| `ferrodoc-markdown` | **652/652** CommonMark spec examples produce identical ASTs |
| `ferrodoc-markdown` GFM reader | **655/656** documents produce identical ASTs |
| `ferrodoc-markdown` pandoc-markdown reader | **3/3** on the fixtures written for it, **17/20** over every markdown document in `corpus/`, and **504/652** over the CommonMark spec; `-f markdown` uses this dialect, while `-f commonmark` selects CommonMark |
| `ferrodoc-html` | **652/652** spec examples produce identical HTML |
| `ferrodoc-docx` reader | **37/37** corpus documents produce identical ASTs, and **7/8** documents written by LibreOffice rather than pandoc |
| `ferrodoc-docx` writer | **646/652** spec examples survive a DOCX round trip identically, with embedded images and document metadata |
| `ferrodoc-odt` reader | **32/34** corpus documents produce identical ASTs, and **8/8** documents written by LibreOffice rather than pandoc |
| `ferrodoc-odt` writer | **640/652** spec examples survive an ODT round trip identically, with embedded images |
| `ferrodoc-epub` reader | **11/12** corpus documents produce identical ASTs, and **3/3** hand-authored books in layouts pandoc never emits (validated by `epubcheck`) |
| `ferrodoc-latex` writer | every corpus document **compiles with `pdflatex`** in CI |
| `ferrodoc-rst` writer | every corpus document is accepted by **`sphinx-build -W`** |
| `ferrodoc-asciidoc` writer | every corpus document is accepted by **`asciidoctor --failure-level=WARN`** |
| `ferrodoc-markdown` writer | **652/652** spec examples survive a markdown round trip identically (pandoc: 593/652) |
| `ferrodoc-markdown` GFM writer | **656/656** documents survive a GFM round trip identically (pandoc: 590/656) |
| `ferrodoc-html` reader | **641/661** HTML documents produce identical ASTs |

```sh
cargo run -p ferrodoc-harness -- diff-spec  corpus/commonmark-spec-0.31.2.json --fail-under 100
cargo run -p ferrodoc-harness -- diff-ast   corpus --fail-under 100
cargo run -p ferrodoc-harness -- diff-html  corpus/commonmark-spec-0.31.2.json --fail-under 100
cargo run -p ferrodoc-harness -- diff-docx  corpus/docx --fail-under 96
cargo run -p ferrodoc-harness -- diff-docx  corpus/docx-libreoffice --fail-under 87
cargo run -p ferrodoc-harness -- diff-write corpus --fail-under 90
cargo run -p ferrodoc-harness -- diff-odt   corpus/odt --fail-under 94
cargo run -p ferrodoc-harness -- diff-odt   corpus/odt-libreoffice --fail-under 100
cargo run -p ferrodoc-harness -- diff-odt-write corpus --fail-under 100
cargo run -p ferrodoc-harness -- diff-md    corpus/commonmark-spec-0.31.2.json --fail-under 100
cargo run -p ferrodoc-harness -- diff-gfm    corpus/gfm --fail-under 100
cargo run -p ferrodoc-harness -- diff-gfm    corpus/commonmark-spec-0.31.2.json --fail-under 99.8
cargo run -p ferrodoc-harness -- diff-gfm-md corpus/gfm corpus/commonmark-spec-0.31.2.json --fail-under 100
cargo run -p ferrodoc-harness -- diff-pandoc-md corpus/pandoc-markdown --fail-under 100
cargo run -p ferrodoc-harness -- diff-html-read corpus/commonmark-spec-0.31.2.json corpus --fail-under 96
```

**What these comparisons hand pandoc, and why.** Three of them pass a flag,
and it belongs here beside the numbers rather than only in
`COMPATIBILITY.md`. `diff-html` runs `pandoc -f commonmark -t html
--syntax-highlighting=none --wrap=none`; `diff-epub-write` passes the first
of those; the LaTeX and RST round trips pass `--wrap=preserve`. Wrapping is
typesetting rather than content, so comparing against it would measure who
guessed the same column. The muted HTML gate isolates structure, escaping and
attributes from presentation. Highlighting has its own oracle:
`scripts/highlight.sh` checks **26 real C, Python, and shell-family files**
byte-for-byte against pandoc. Other languages are emitted as unhighlighted
code, an explicit visible gap against pandoc's default rather than a hidden
part of the `652/652` claim.

`diff-write` and `diff-odt-write` are the office writers' oracle: both
engines write the same AST to a `.docx` (or `.odt`), pandoc reads both back,
and the two documents must match.
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

### Python

```sh
pip install ferrodoc
```

```python
import ferrodoc

with open("report.docx", "rb") as f:
    markdown = ferrodoc.convert(f.read(), "docx", "gfm")   # str

docx = ferrodoc.convert("# Title\n\nHello.\n", "gfm", "docx")   # bytes
```

One function. `convert` takes `str` or `bytes` and returns `str` for a text
format and `bytes` for DOCX. The pandoc AST is the `json` format, so
`json.loads(ferrodoc.convert(doc, "docx", "json"))` gives you the tree
without a second API. The GIL is released around the conversion, so a
thread pool converts in parallel. Wheels are `abi3` for Python 3.9 and up,
on Linux, macOS (both architectures) and Windows. `read_formats()` and
`write_formats()` say which way each format goes — they are not the same
list, and asking `formats()` for one direction is what shipped a binding
that could not write an ODT.

Three of the four wheels are built, installed and tested on the platform
they target, on every push. The Intel macOS one is **cross-compiled and
not run**, because GitHub no longer has a machine to run it on.

### Rust and the command line

```sh
cargo install ferrodoc                   # installs the `ferrodoc` binary
```

Or take a prebuilt binary from the
[latest release](https://github.com/alexgaoth/ferrodoc/releases/latest) —
Linux (musl), macOS (Intel and Apple silicon) and Windows — or add the
library to a project:

```toml
[dependencies]
ferrodoc = "0.7"
```

```sh
ferrodoc README.md -o readme.html        # an HTML fragment
ferrodoc README.md -s -o readme.html     # a page a browser can open        # formats inferred from extensions
ferrodoc report.docx -o copy.docx        # DOCX in, DOCX out — keeps its images
ferrodoc report.docx -t gfm             # DOCX in, GitHub markdown out — keeps tables
ferrodoc report.docx -t markdown         # DOCX in, Pandoc Markdown out
ferrodoc report.docx -t commonmark       # DOCX in, strict CommonMark out
ferrodoc report.docx -t plain            # DOCX in, text out, to stdout
ferrodoc report.docx -t gfm --extract-media out   # ...and keep the pictures
ferrodoc minutes.odt -t gfm              # LibreOffice in, GitHub markdown out
ferrodoc README.md --defaults build.yaml    # the flags a Makefile keeps in a file
cat notes.md | ferrodoc -f markdown -t docx -o notes.docx
ferrodoc --help                          # every option and format
```

CLI inputs: `markdown` (pandoc's dialect; `md`), `commonmark`, `gfm`, `html`,
`docx`, `odt`, `epub`, `ipynb`, `json` (the pandoc AST). Outputs are the
same set, plus `latex`, `rst`, `asciidoc` and `plain`; `markdown` and
`pandoc_markdown` are equivalent names for Pandoc Markdown, and
`commonmark` is strict CommonMark.

> **CLI input `markdown` is pandoc's markdown dialect.** This matches
> `pandoc -f markdown` and the default inferred for `.md` files; use
> `-f commonmark` when that stricter dialect is intended. The explicit
> `pandoc_markdown` name is available on both sides. `-t markdown` also
> writes Pandoc Markdown; use `-t commonmark` for strict CommonMark. The
> Pandoc Markdown reader is not complete: it agrees on **17 of 20** markdown documents in `corpus/` and
> **504 of 652** CommonMark-spec examples. Its remaining gaps are
> parser-level, not missing extensions: of the 150 spec examples it still
> reads differently, **40 are emphasis** and **25 are links**. Pandoc's
> markdown does not apply CommonMark's flanking rules, so `*foo bar *`
> and `*(*foo)` are emphasis for pandoc and literal text here. Exactly
> one of the 652 is *refused* rather than read differently, so this is a
> difference in the tree and not a wall to hit:
>
> ```sh
> cargo run -p ferrodoc-harness -- diff-pandoc-md corpus --verbose
> ```

Pandoc Markdown and GFM both read footnotes. Pandoc Markdown also accepts
YAML metadata, header attributes, definition lists, superscript and
subscript; the reader's incomplete score above is why users with demanding
Pandoc-Markdown input should still validate their own corpus.

`-s`/`--standalone` writes a complete page
**through pandoc's own default template**, which is vendored here under
the BSD-3 licence pandoc offers for it: byte-identical output across
`--toc`, `--toc-depth`, `--css`, `-V`, `-H`, `-B`, `-A` and a
third-party `--template`, checked by `./scripts/flags.sh`.

Prefer `-t gfm` or `-t markdown` over `-t commonmark` for anything with a
table: strict CommonMark has no table syntax, so a table is written there as
raw `<table>` HTML. That keeps the document but is not pleasant source text.

**Line layout is pandoc's**, as of 2026-08-24. Every text writer fills to
`--columns` (72 by default), keeps the document's own breaks under
`--wrap=preserve`, or puts each block on one line under `--wrap=none` —
and the default is `auto`, which is pandoc's.

It was not, for a long time, and the shape of the gap is worth keeping
because it is what the drop-in number was mostly measuring. ferrodoc did
not fill at all, and what it did instead was **not the same for every
writer**: `html` and `plain` joined every soft break into a space, which
is `--wrap=none`, while the other five kept the document's own breaks,
which is `--wrap=preserve`. That was not a decision — it was seven
writers written separately — and asking for a layout a writer could not
produce was an error by name rather than a silent no-op. Filling them one
at a time is what let the default flip.

The gap is counted rather than described: `./scripts/dropin.sh` runs 48
real pandoc command lines — collected from public Makefiles, CI files and
scripts — through both binaries and compares every byte either wrote,
stdout, output files and stderr. **47/48 command lines identical**, with
**0 refused** for a flag ferrodoc does not have.

`--attribute` turns that into work: it retries each miss with one of
pandoc's own features switched off at a time — on **both** sides, since
this now highlights too — and names the smallest set that makes the two
agree. Reading `markdown` as pandoc's dialect rather than CommonMark —
on the way in, on the way out, and under the deprecated name
`markdown_github` — accounted for 23 misses on its own until
**2026-08-27, when the default became pandoc's dialect** and the number
went 12/48 to 22/48; the table of contents took it to 26/48, a figure
caption to 27/48, the dialect writer learning simple tables, six inline
spellings and the fenced div took it to 30/48, and un-smartening the
curly quotes, a `\tightlist` on a description list and a break
opportunity inside the contents took it to **33/48**. The dialect now
accounts for **1** miss, with **1** more together with syntax
highlighting, which accounts for **1** by itself.

**8 rows are deliberate**, and the gate decides that rather than being
told: seven of them differ only inside a `<style>` element, which is the
highlighting stylesheet this cannot take from skylighting, and the
eighth is an ordered list's counter the LaTeX writer places where
pandoc's own reader can still find it. Listing those seven by row number
would have been wrong twice over — two *other* rows carry the same
stylesheet difference **and** a real one underneath it, and a list would
have retired them both.

**That leaves 4, and none of them is plain implementation work any
more.** Three are dialect **reader** rules: pandoc's markdown does not
open a code block for `` ```rust ignore `` (two rows), and does not let a
nested `>` interrupt a paragraph. Neither can be reconstructed after the
parse — the marker is gone from the tree by the time this sees it — so
both are recorded rather than emulated halfway. The fourth is the only
row writing a **binary** format, where two independent writers do not
produce identical zip bytes and comparing them is arguably the wrong
question to ask. Every one is in `COMPATIBILITY.md` with a repro that
runs as printed, as are the two `markdown_github` rows above: that name
is pandoc's *old* markdown reader rather than a spelling of `gfm`, and
`gfm` is the closest of the three readers here at 3/8 documents.

`--extract-media DIR` writes the input's embedded images under `DIR` and
repoints the output at them, matching `pandoc --extract-media` file for
file. Without it a `docx → markdown` conversion names pictures nothing ever
writes — pandoc does the same, and it is the reason both tools need the
flag. Media is only read when something will hold it, so the conversions
that do not ask for pictures never pay for them.

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
- **Portable, and shipped that way.** Every library crate compiles to
  `wasm32-unknown-unknown`, and the npm package is that build
  (`npm install ferrodoc`): **0.65 MB gzipped** (683,175 bytes, measured by
  `./bindings/wasm/build.sh` today), converting in a browser tab
  with no document leaving the client. CI drives it in headless Chrome and asserts that the page
  makes no network request, because that claim is the reason it exists.

```sh
cargo build --release --target wasm32-unknown-unknown \
  -p ferrodoc-ast -p ferrodoc-markdown -p ferrodoc-html -p ferrodoc-text \
  -p ferrodoc-docx -p ferrodoc-odt
```

- **And you can pay for less.** Every format is a cargo feature, with
  `default = ["all"]`, so nothing changes for anyone who does not ask. A
  caller who converts markdown and HTML can leave the other nine out:
  `--no-default-features --features markdown,html` takes the wasm module to
  **57%** of its gzipped size and the CLI binary to 59% of its own. The
  ratio is the claim and the byte count illustrates it: the CLI is
  reproducible to the byte in one checkout with one toolchain but 0.7%
  smaller on a CI runner, and the wasm module varies by about 0.03% with
  the build path. Both are re-derived by
  `./scripts/claims.sh --sizes`, which fails when either ratio moves by a
  point or any artefact grows more than 5% — this paragraph had drifted
  1.4% before that existed. A trimmed build cannot quietly do less than it says: `--help` lists
  exactly the formats it contains, and asking for one it does not have is
  an error naming the reason, not a wrong answer.

```sh
./bindings/wasm/build.sh                       # every format: 2,068,161 bytes, 766,219 gzipped
./bindings/wasm/build.sh --no-default-features --features ferrodoc/markdown,ferrodoc/html
                                               # markdown + HTML: 1,254,392 bytes, 436,961 gzipped
cargo build --release -p ferrodoc --no-default-features --features markdown,html
                                               # 4,402,392 bytes, against 7,487,680
```

Every gate, every known loss and every deliberate divergence is listed one
by one in [`COMPATIBILITY.md`](COMPATIBILITY.md), with the command that
produces it. CI runs **every one of them** against a pinned pandoc, and builds
and tests on Linux, macOS and Windows, plus a wasm32 build and a
500,000-mutation fuzz campaign.

What changed between releases, and what a 0.1 caller has to do about it,
is in [`CHANGELOG.md`](CHANGELOG.md); how a release is cut is in
[`docs/releasing.md`](docs/releasing.md).

The forward plan is in [`ROADMAP.md`](ROADMAP.md): a version ladder to 1.0,
where the claim is that for the formats this supports, it produces
byte-identical output to pandoc or fails loudly saying what it will not do —
with a drop-in corpus of real command lines as the number that decides it.

## What this does not do

The list, with the workaround where there is one. Every flag below is
**refused by name** — `ferrodoc: unknown option --citeproc` — rather than
accepted and ignored, so a script that depends on one fails where it is
called instead of producing a document that is quietly wrong.

| not done | workaround |
|---|---|
| **Citations** — `--citeproc`, `--bibliography`, CSL | pandoc. This is the named exception to the 1.0 claim: CSL processing and five bibliography readers are the size of everything here, and shipping without them is what makes the rest checkable |
| **Lua filters** — `--lua-filter` | the JSON AST, which is what most filters walk: `ferrodoc in.md -t json \| your-filter \| ferrodoc -f json -t html` |
| **PDF output** — `-t pdf`, `--pdf-engine` | `-t latex` and then `latexmk`/`pdflatex`, which is what pandoc shells out to; CI compiles this writer's output on every push |
| **Math output modes** — `--mathjax`, `--katex`, `--mathml` | none. Math is rendered the way pandoc's default does it: to Unicode where it can, and to the TeX source between dollars where it cannot |
| **`--listings`** | the default highlighted `Shaded`/`Highlighting` environments, which is pandoc's default too |
| **The other ~30 formats** | pandoc. The nine here are in the table above; anything else is refused by name at the command line |

`--template` is supported, and so are `-V`, `--toc`, `-H`, `-B`, `-A`,
`--css` and `--data-dir`: `-s` writes a complete page through pandoc's own
default template, vendored here, byte-identical across all of them.

## Where pandoc is still ahead

The table above is not the whole picture, and pretending otherwise would make
the rest less believable. Pandoc supports ~40 formats to ferrodoc's nine, plus
citations, templates, Lua filters, PDF output and fifteen years of edge cases.
Our DOCX writer still drops raw blocks, which have no OOXML equivalent, and
embeds eight image formats — PNG, JPEG, GIF, WebP, TIFF, SVG, EMF and WMF —
where anything else becomes alt text. **Prefer DOCX to ODT** when a document
has to keep its code blocks or its merged cells: pandoc's ODT *reader* has
no construct for either, so neither converter can carry them through an
`.odt`, and `COMPATIBILITY.md` lists the rest of what that reader drops. The HTML reader parses to the HTML5
spec, via `html5ever`, where pandoc uses `tagsoup`; on malformed markup — an
unclosed `<a>`, a tag with no closing `>`, a `<pre>` inside a `<tr>` — the
two build different trees, and that is what most of the 26 unmatched
documents are. Not all: `<![CDATA[…]]>` tokenizes differently, and `<a/>`
self-closing syntax sends the two down different recovery paths. The
remaining families are listed in [`docs/divergences.md`](docs/divergences.md),
one by one. The bet is not that
this replaces pandoc — it is that the common path, markdown/HTML/DOCX
called from a program rather than a shell, is worth doing natively.

## License notes

`corpus/commonmark-spec-0.31.2.json` is the example set from the
[CommonMark spec](https://spec.commonmark.org/0.31.2/), © John MacFarlane,
licensed CC-BY-SA 4.0.
