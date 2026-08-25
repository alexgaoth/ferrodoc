# ferrodoc

Convert documents — markdown (CommonMark and GFM), HTML, DOCX, ODT and
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
> every byte: **10/48 identical**, 0 refused for a missing flag. What
> stands between that and the rest is one decision, named and measured
> below. The bet here is the *library*, not the command line.

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
| **Binary / dependency on disk** | 160.4 MB | **7.1 MB** | **23× smaller** |
| **Peak memory**, 10 KB document | 115 MB | **5.2 MB** | **22× less** |
| **Malformed DOCX** (self-referential footnote) | hangs, killed at 60 s | **handled in 12 ms** | — |
| **Same document written twice** | different bytes | **identical bytes** | — |
| **Runs in a browser / edge worker** | no | **yes** (wasm32) | — |
| **Callable without a subprocess** | no | **yes** (Rust, Python) | — |

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
| `ferrodoc-markdown` GFM reader | **655/656** documents produce identical ASTs |
| `ferrodoc-markdown` pandoc-markdown reader | **3/3** on the fixtures written for it, **14/20** over every markdown document in `corpus/`, and **498/652** over the CommonMark spec — which is why `-f markdown` is still CommonMark here |
| `ferrodoc-html` | **652/652** spec examples produce identical HTML |
| `ferrodoc-docx` reader | **36/37** corpus documents produce identical ASTs, and **7/8** documents written by LibreOffice rather than pandoc |
| `ferrodoc-docx` writer | **643/652** spec examples survive a DOCX round trip identically, with embedded images and document metadata |
| `ferrodoc-odt` reader | **32/34** corpus documents produce identical ASTs, and **8/8** documents written by LibreOffice rather than pandoc |
| `ferrodoc-odt` writer | **640/652** spec examples survive an ODT round trip identically, with embedded images |
| `ferrodoc-epub` reader | **10/12** corpus documents produce identical ASTs, and **3/3** hand-authored books in layouts pandoc never emits (validated by `epubcheck`) |
| `ferrodoc-latex` writer | every corpus document **compiles with `pdflatex`** in CI |
| `ferrodoc-rst` writer | every corpus document is accepted by **`sphinx-build -W`** |
| `ferrodoc-asciidoc` writer | every corpus document is accepted by **`asciidoctor --failure-level=WARN`** |
| `ferrodoc-markdown` writer | **652/652** spec examples survive a markdown round trip identically (pandoc: 593/652) |
| `ferrodoc-markdown` GFM writer | **656/656** documents survive a GFM round trip identically (pandoc: 590/656) |
| `ferrodoc-html` reader | **635/661** HTML documents produce identical ASTs |

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
guessed the same column. Highlighting is a rendering choice this project
does not make — and unlike wrapping it is **visible**, so a reader who
checks the `652/652` with plain `pandoc -t html` sees a difference on the
first code block:

```console
$ printf '```rust\nfn main() {}\n```\n' | pandoc -f gfm -t html
<div class="sourceCode" id="cb1"><pre
class="sourceCode rust"><code class="sourceCode rust"><span id="cb1-1"><a href="#cb1-1" aria-hidden="true" tabindex="-1"></a><span class="kw">fn</span> main() <span class="op">{}</span></span></code></pre></div>

$ printf '```rust\nfn main() {}\n```\n' | ferrodoc -f gfm -t html
<pre class="rust"><code>fn main() {}</code></pre>
```

Give pandoc the flag and the same block comes back byte for byte:

```console
$ printf '```rust\nfn main() {}\n```\n' | pandoc -f gfm -t html --syntax-highlighting=none --wrap=none
<pre class="rust"><code>fn main() {}</code></pre>
```

So the difference is the highlighting and nothing else — structure,
escaping and attributes match either way. Syntax highlighting is not
implemented here, and `COMPATIBILITY.md` records it as a known loss rather
than a footnote.

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
ferrodoc = "0.2"
```

```sh
ferrodoc README.md -o readme.html        # an HTML fragment
ferrodoc README.md -s -o readme.html     # a page a browser can open        # formats inferred from extensions
ferrodoc report.docx -o copy.docx        # DOCX in, DOCX out — keeps its images
ferrodoc report.docx -t gfm             # DOCX in, GitHub markdown out — keeps tables
ferrodoc report.docx -t markdown         # DOCX in, CommonMark out — no table syntax
ferrodoc report.docx -t plain            # DOCX in, text out, to stdout
ferrodoc report.docx -t gfm --extract-media out   # ...and keep the pictures
ferrodoc minutes.odt -t gfm              # LibreOffice in, GitHub markdown out
ferrodoc README.md --defaults build.yaml    # the flags a Makefile keeps in a file
cat notes.md | ferrodoc -f markdown -t docx -o notes.docx
ferrodoc --help                          # every option and format
```

Inputs: `markdown` (`commonmark`, `md`), `gfm`, `html`, `docx`, `odt`, `epub`,
`ipynb`, `json` (the pandoc AST). Outputs: those plus `latex`, `rst`,
`asciidoc` and `plain`.

> **`markdown` here is CommonMark, which is not what `pandoc -f markdown`
> means.** Pandoc's own dialect adds YAML metadata blocks, header
> attributes (`# H {#id .class}`), definition lists and
> superscript/subscript; none of those are read here, and they come through
> as the literal text they are written with. `-f pandoc_markdown` reads
> them — and **does not yet alias `markdown`**, because measured against
> `pandoc -f markdown` over every markdown document in `corpus/` it agrees
> on **6 of 20**. Aliasing a reader that disagrees with pandoc on two
> thirds of a corpus would move the difference from a name you have to
> type to every conversion you already run. The gap is `smart` quotes,
> `implicit_figures`, and code spans inside table cells:
>
> ```sh
> cargo run -p ferrodoc-harness -- diff-pandoc-md corpus --verbose
> ``` Footnotes are read by `gfm`
> and not by `markdown` — which is how pandoc has it too. The one case
> where the output is *wrong* rather than narrower is a YAML metadata
> block, because the title and author land in the body, so ferrodoc prints
> a line to stderr when a document opens with one. `-s`/`--standalone` writes a complete page
**through pandoc's own default template**, which is vendored here under
the BSD-3 licence pandoc offers for it: byte-identical output across
`--toc`, `--toc-depth`, `--css`, `-V`, `-H`, `-B`, `-A` and a
third-party `--template`, checked by `./scripts/flags.sh`.

Prefer `-t gfm` over `-t markdown` for anything with a table: CommonMark has
no table syntax, so a table degrades to one paragraph per cell there.

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
stdout, output files and stderr. **11/48 command lines identical**, with
**0 refused** for a flag ferrodoc does not have.

`--attribute` turns that into work: it retries each miss with one of
pandoc's own features switched off at a time — on **both** sides, since
this now highlights too — and names the smallest set that makes the two
agree. Reading `-f markdown` as pandoc's dialect rather than CommonMark
accounts for **18** misses on its own and **9** more together with syntax
highlighting, which accounts for **2** by itself. One row is a difference
this project has decided to keep. That leaves **7**, and most of those
are the same dialect decision from an angle the experiment cannot model — `markdown_github` naming it, a `.pmd` document written in
it, or `-t markdown` being asked to *write* it. **The number is one
decision away.**

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
  **58%** of its gzipped size and the CLI binary to 60% of its own. The
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
./bindings/wasm/build.sh                       # every format: 1,855,728 bytes, 683,175 gzipped
./bindings/wasm/build.sh --no-default-features --features ferrodoc/markdown,ferrodoc/html
                                               # markdown + HTML: 1,176,179 bytes, 407,537 gzipped
cargo build --release -p ferrodoc --no-default-features --features markdown,html
                                               # 3,964,400 bytes, against 6,499,664
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
