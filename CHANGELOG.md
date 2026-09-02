# Changelog

What changed between releases, and what a caller has to do about it. The
compatibility ledger is [`COMPATIBILITY.md`](COMPATIBILITY.md) and the
forward plan is [`ROADMAP.md`](ROADMAP.md); this file is the record of
what already happened.

Every "changed" entry below carries the signature on both sides, because a
break published without its note is a break twice.

## 1.0.1 — 2026-09-02

### Fixed

**A deeply nested TeX expression no longer exhausts the stack.** The TeX
renderer is recursive descent, and nothing bounded it: `$` + 50,000 `{` +
`x` + 50,000 `}` + `$` aborted the process with `fatal runtime error:
stack overflow` on `-t html`, `-t plain` and `-t commonmark` — every
writer that renders math — from any reader that can carry a `Math` node.
A second path did it without braces at all: `\left` repeated, which
`control()` recursed on directly.

Both are bounded at 200 levels now, the same figure the markdown and HTML
readers already refuse at. Past it the expression is declined and the
writer falls back to the TeX source, which is what it already did for a
fraction — so the document is preserved and only the rendering is lost.
`scripts/math.sh` is unchanged at 242/243.

This was reachable by any caller converting untrusted documents, so a
service accepting uploads should take this before exposing one.

## 1.0.0 — 2026-09-01

### Indistinguishable, on a scope that is written down

**The claim.** For the nine formats this supports, ferrodoc produces
byte-identical output to pandoc 3.8.2.1 or fails loudly saying what it
will not do. `README.md` names what it will not do, each entry with the
workaround where there is one; **citations are the named exception**.

Nothing in the public API changed. This release is the number behind the
sentence:

| what it says | where it is measured |
|---|---|
| every AST construct, in every writer | `scripts/ast-sweep.sh` — **286/286 on all eight**, flat and composed, with no divergence that is neither fixed nor recorded |
| every TeX expression it renders | `scripts/math.sh` — **242/243**, and the one that differs is a recorded escaping decision |
| real command lines from the wild | `dropin/` — **47/48**, none of them *fixable* |
| the markdown, GFM and HTML writers | the CommonMark spec, **652/652** and **656/656** and **652/652** |
| the resource bound | 80× input RSS, worst gated path 60× |

### What changed since 0.8.0

- **Math is rendered, not written.** `commonmark`, `html` and `plain`
  convert a TeX expression to inlines the way pandoc does — `$x^2$` is
  `x²` and `<em>x</em><sup>2</sup>` — and write the source between
  dollars only for the expressions pandoc writes that way too.
- **Every writer's composition axis is whole.** The sweep crosses
  container against content, and all eight writers now match pandoc on
  every pair: RST's inline nesting, AsciiDoc's containers, the markdown
  rule, the plain writer's first lines, LaTeX and GFM table cells.
- **`scripts/math.sh` is a new gate**, because the sweep asks about
  `$x^2$` and nothing else.

## 0.8.0 — 2026-08-30

### `markdown` is Pandoc Markdown in both directions

**This changes what `ferrodoc x.md` and `-t markdown` do.** A `.md` file with
no `-f`, `-f markdown`, and `-t markdown` now use Pandoc's own dialect —
heading identifiers, footnotes, definition lists, `{#id .class}` attributes
and a YAML metadata block — where they used CommonMark before.

> **If you want the old behaviour, pass `-f commonmark` or `-t commonmark`.**
> It is the explicit strict-CommonMark spelling.

Measured over this repository's own documents against `pandoc -t html`
with no `-f` on either side, the difference from pandoc's output falls
from **2,466 lines to 241**, and no document gets worse. The initial input
alias took the 48 real command lines in `dropin/` from **12/48 to 22/48**;
later work has the current result at 33/48 (see `dropin/README.md`).

`markdown`, `md`, and `pandoc_markdown` now name the same format in the public
API and CLI. `Format::parse_input` remains as a backwards-compatible public
API alias for callers that want to state their intent.

### The dialect writer, which `-t markdown` now reaches

`-t markdown` writes Pandoc Markdown, so the constructs CommonMark has no
spelling for survive a round trip: **definition lists**, fancy ordered-list
markers, line blocks, `{#id .class}` attributes, superscript and subscript,
raw blocks, citations written from their data rather than their rendered
text, and pandoc's three table shapes — **simple**, **multiline** and, new
in this release, **grid**.

A grid table is what a cell holding anything but a single paragraph gets: a
code block, two paragraphs, a list, a blockquote. Those used to fall back to
raw `<table>`, which is what every DOCX or HTML table with structure in a
cell became.

### Link destinations are percent-encoded, in the dialect only

`[link](/my uri)` reads as `/my%20uri`, matching `pandoc -f markdown`. The
set is whitespace plus ``<>|"{}[]^` ``. `-f commonmark` and `-f gfm` are
unchanged and hand the destination back exactly as written.

### Escaping a paragraph's later lines

`128.`, `- x` and `+ x` on a line after a **hard** break are written bare, as
pandoc writes them; they used to be escaped. After a *soft* break they are
still escaped, because a list can open there.

### Archives declare a bound

An archive is refused past **100× its compressed size or 64 MB, whichever is
larger**, so a decompression bomb is an error rather than an allocation. The
end-to-end bound is unchanged: peak resident memory stays within **80× the
source document** on every gated conversion.

### Smaller DOCX output

The DOCX writer emits one run per formatting change rather than per word:
17.7 MB became 3.3 MB on the document that showed it.

### Highlighting

Rust, Ruby and C join Python and Bash, and the Python and Bash rules were
reworked against system files rather than hand-picked lines.

### Where it stands

`ferrodoc` is byte-identical to pandoc 3.8.2.1 on **33 of the 48 real
command lines** in `dropin/`, **224/224** output-shaping flag combinations,
**652/652** CommonMark-spec documents through the CommonMark reader, and
**286** AST constructs across eight text writers. The Pandoc Markdown
*reader* is the incomplete part — **502/652** on the spec — and
`README.md` and `COMPATIBILITY.md` say where the rest of that gap is.

## 0.7.0 — 2026-08-26

### Syntax highlighting, in pandoc's shape

Code fences are coloured for **C, Python, bash and Ruby**, in the exact
markup pandoc's skylighting emits: `<div class="sourceCode" id="cbN">`
with the block's attributes on that div, `<pre>` carrying the block's
classes, `<code>` carrying the syntax's canonical name, and one
`<span id="cbN-M">` per line with an anchor in it. A language not on the
list degrades to what this writer emitted before — `<pre><code>` — so a
short list costs nothing but colour.

`--no-highlight` and `--syntax-highlighting=none` turn it off. Any other
style value is **refused by name**, because a style that silently does
nothing is worse than one that says so. It is a cargo feature
(`highlight`, on by default) worth 80.4 KB, 1.16% of the binary.

Every rule was read off the pinned `pandoc 3.8.2.1` rather than reasoned
about, and the ones that would have been guessed wrong are worth naming:
`NULL`, `printf` and `malloc` are **not** classed; adjacent operator
characters are one span; a numeric suffix is a `bu` beside the number
rather than part of it; and bash's classes are **positional** — the same
word is `fu` at the start of a command and plain text one word later.

**The gate was wrong, and this release says so.** `scripts/highlight.sh`
compares 26 files chosen from this repository and stood at 26/26 for
weeks. Pointed at code nobody wrote for this project, the same
highlighters matched pandoc on **1 system header in 40**. A gate cannot
fail on a construct its corpus lacks, and 2,650 lines of our own C,
Python and shell contain no multi-line `#define`, no blank line inside a
licence header, no `f"{x!r}"`. Eight probed rules later:

```console
$ ./scripts/real-world.sh
highlighting against code written for somebody else:
  c        23/40
  python   16/40
  bash     17/24
  ruby     9/40
```

That script reports and does not gate — its corpus is whatever the
machine holds, and the denominators drift as packages are installed.
COMPATIBILITY.md carries both columns and names what is still wrong:
bash does not tokenize an array subscript, and Ruby's `$1`, heredoc
bodies and `%w[…]` are unhandled. **Ruby ships with its number beside it
rather than with a claim.**

The highlighting stylesheet is **ours**, in
`crates/ferrodoc-html/styles/highlight.css`, written against the class
names skylighting emits rather than copied from pandoc. Nine structural
declarations coincide with pandoc's because there is one way to write
them; nothing carrying a colour is shared. Before it, `-s` output had
the spans and none of the colours.

### The text writers are pandoc's, and lines are filled to 72 columns

Five writers were rebuilt against a second oracle nobody had used:
pandoc *writes* LaTeX, RST, AsciiDoc, plain and markdown from the same
AST, so the bytes compare directly without asking its reader to survive
anything. `./scripts/writers.sh` gates every one of them:

```console
$ ./scripts/writers.sh | tail -1
byte-identical: html 38/40, commonmark 29/40, markdown 6/40, gfm 28/40, latex 36/40, rst 34/40, asciidoc 38/40, plain 38/40 on corpus/*.md, corpus/gfm/*.gfm and this repository's own prose
```

Twenty documents, each written twice — as the document falls, and filled
to a column.

> **Behaviour change.** `--wrap=auto` at **72 columns** is now the
> default, which is pandoc's. A 0.2 caller who relied on lines being
> emitted as the document fell should pass `--wrap=preserve`.

### Binary writers are judged by what pandoc reads back

`./scripts/roundtrip.sh` writes a DOCX, ODT, EPUB or notebook and hands
it to **pandoc's own reader**, comparing the AST that comes back. It
normalises only what cannot match — an EPUB's identifier and date, a
notebook cell's UUID, a media file's number — and reports a refused
conversion separately from a mismatch, because pandoc writes nothing
when it refuses and the previous document's bytes were being compared
three times before that was noticed.

```console
$ ./scripts/roundtrip.sh | tail -1
read back by pandoc: docx 14/16, odt 16/16, epub 0/16, ipynb 11/16 (2 unwritten) over corpus/*.md and this repository's own prose
```

### pandoc's markdown dialect, and the drop-in count

`smart`, `implicit_figures`, inline attributes, `^[…]` inline notes,
bracketed spans, `{#id .class k=v}` after a fence, metadata block
scalars, and raw HTML the way pandoc's markdown reads it. Scored over
the CommonMark spec's 652 examples rather than twenty documents:
**498/652**. Real command lines from `dropin/` that run byte-identically:
**11 of 48**. Flag combinations byte-identical: **224/224**.

### Retired: the `epub-spec` figure

It was the HTML reader's score printed in a table headed EPUB — each of
its 22 files bundles 30 spec examples, so any one of the reader's known
divergences fails a whole document. The corpus and its gate stay as a
regression check; the number is no longer published.


### `--reference-doc` for DOCX and ODT, and no refusals left in the corpus

The house styles live in a `.docx` somebody made in Word, and this is the
flag that reaches them. `word/styles.xml` and `word/numbering.xml` come
from the reference; nothing else does, because copying the rest would
mean shipping parts nothing declares. A reference that is not a `.docx`,
or has no styles part, is named rather than ignored.

**Every one of the 48 real command lines in `dropin/` now runs.** None is
refused for a flag this build does not have.

### `--resource-path` and `--data-dir` (card D4.7 finished)

`--resource-path` is searched after the document's own directory, so a
picture beside the document still wins. `--data-dir` supplies
`templates/default.html5` — **pandoc's file name, measured**: a data
directory holding `templates/html5.html` is ignored by pandoc, so it is
ignored here.

### `--ascii`, `--id-prefix`, `--metadata-file`

`./scripts/flags.sh` is now **184/184 byte-identical** to pandoc.

`--ascii` is HTML-only and says so: pandoc spells the escape differently
in every writer, and inventing one would be a flag that looks honoured
and writes something pandoc does not. `--id-prefix` rewrites internal
links as well as identifiers, so anchors still land. `--metadata-file`
reads the flat `key: value` subset and refuses the rest by name.

### `+ext-ext` extension syntax (card D4.5)

`-f markdown+footnotes-pipe_tables` was refused wholesale. It is now
accepted **where it asks for nothing** — the named dialect already reads
that way, so it is the same conversion — and refused by name otherwise,
saying which of the three dialects does read the extension. A name pandoc
does not have is reported as a typo rather than as a missing feature.

### `--shift-heading-level-by`, `--strip-comments`, `--eol` (card D4.8)

`./scripts/flags.sh` is the gate: **144/144 flag combinations
byte-identical** to pandoc over every document in `corpus/`, covering
these three and the page flags below. Required at 100, not gated at a
floor — a `--eol=crlf` that is 90% right is a file with mixed line
endings.

Three rules were measured rather than read, and each was wrong the first
time: the heading promoted to the title is the one the shift takes to
**exactly level 0** whatever level it started at; `--strip-comments` cuts
the comment out of the raw text and keeps the block, so the newline that
followed it survives; and an **unterminated** `<!--` is left alone rather
than swallowing the rest of the document.

### Standalone HTML is pandoc's page, byte for byte

`-s` output was one fixed page shape against pandoc's template language,
and 176 lines away from it. Pandoc's default template and default
stylesheet are now vendored in `crates/ferrodoc-html/templates/` under
the **BSD-3 option** its `COPYRIGHT` offers for `data/templates`, and
rendered through a subset of its template language — `$var$`, `$if$`,
`$for$`, `$sep$`, `$partial()$` — with everything outside that subset
refused **by name** rather than left as a hole in the page.

```console
$ ./scripts/standalone.sh
80/80 standalone command lines byte-identical
```

New flags, all of which the page needed: `--toc-depth`, `-V`/`--variable`,
`--template`, `-H`/`--include-in-header`, `-B`/`--include-before-body`,
`-A`/`--include-after-body`.

**Three behaviour changes** a 0.2 caller will notice:

- **`--css` links a stylesheet, it does not inline one.** Pandoc's flag
  takes a URL and emits `<link>`; inlining the file was this project's
  invention and made every `-s -c` command line differ.
- **`-V` is `--variable` and `-v` is `--version`**, which is pandoc's
  assignment. `ferrodoc -s -V lang=fr` used to print a version string and
  convert nothing.
- `write_html_standalone` and `render_html_standalone` are replaced by
  `write_page(doc, &Page)` and `render_page`, which carry the flags above.
  The hand-rolled page writer is gone rather than left beside the new one.

Two rules in it were measured, not assumed: `--css` turns pandoc's
default stylesheet **off**, and `--toc` on a document with no heading
writes **nothing** rather than an empty `<nav>`.

## 0.2.0

The ten library crates went to crates.io on 2026-08-23; `ferrodoc-epub`
and the `ferrodoc` facade followed the same day, after the registry's
new-crate rate limit. Everything below is in that release.

The command-line sections come last because they landed between the two
publishes — every crate already on the registry was byte-identical at
both, so the `v0.2.0` tag is what every published 0.2.0 was built from.

### `--wrap` means what pandoc means by it (roadmap card D4.3)

Three defects, found by measuring the flag rather than reading it:

- **`--wrap=none` and `--wrap=preserve` were the same value.** They are
  not the same thing — `none` joins every soft break into a space,
  `preserve` keeps them — and on the one writer that could tell them
  apart, both preserved.
- **`--wrap=auto` was a silent no-op for five of the seven text
  writers**, and dropped embedded media on the way past: the wrapped path
  called the writer that takes no media resolver, so
  `--wrap=auto -o out.docx` lost every picture.
- **ferrodoc had no single wrap default**, and `README.md` claimed it
  did. `html` and `plain` join, which is pandoc's `--wrap=none`;
  `markdown`, `gfm`, `latex`, `rst` and `asciidoc` keep the document's
  breaks, which is `--wrap=preserve`.

Now: `Format::wrapping()` states what each writer does, `Wrap` carries
pandoc's three modes, and a writer that cannot honour the mode asked for
returns `Error::NotWrappable` naming what it does instead. Breaking for
callers of `render_wrapped`, which took a bare column count:

```rust
// was
pub fn render_wrapped(doc: &Pandoc, to: Format, columns: usize) -> Result<Vec<u8>, Error>
// now
pub fn render_wrapped(doc: &Pandoc, to: Format, wrap: Wrap) -> Result<Vec<u8>, Error>
pub fn render_wrapped_with_media(doc: &Pandoc, to: Format, wrap: Wrap,
                                 media: &dyn Fn(&str) -> Option<Vec<u8>>) -> Result<Vec<u8>, Error>
```

### `-f markdown` stays CommonMark, and now says by how much (card D4.4)

`diff-pandoc-md` scored **3/3** against three fixtures written for the
pandoc-markdown reader. Run over every markdown document in `corpus/` the
same reader scores **6/20**, so `markdown` does not alias
`pandoc_markdown`: that would move the difference from a name you type to
every conversion you already run. `verify.sh` gates both — the fixtures
at 100, the wide corpus at its measured 30%.

The widened run also fixed the gate: a document the reader *refuses* now
counts as a failure instead of aborting the run, so one `abstract: |` no
longer stops the other nineteen being measured.

### `--defaults`, and `-c` for `--css` (card D4.7)

`--defaults FILE` reads a flat `key: value` file and applies it **where
the flag appeared**: `-t plain --defaults d.yaml` takes `to` from the
file and `--defaults d.yaml -t plain` takes it from the flag, which is
pandoc's rule, measured both ways round. A key this build has no flag for
is an error naming the key — a `filters:` silently dropped would convert
the document and leave out what the file was written to do.

Seven of the 48 command lines in `dropin/` were refused for this flag
alone; with `-c` that count is 15 down to 7.

### `-s` is pandoc's no-op where it is one (card D4.6)

`ferrodoc --standalone --to plain x.md` used to fail and write nothing,
where pandoc writes the document. `-s` is pandoc's no-op for a format
with no page form — **but only while the document carries no metadata**:
with any at all it writes a title block, and for `plain` that is two
blank lines even for a key no title block would show. So it is accepted
and ignored where the bytes are identical, and refused by name where they
would not be.

That, plus `--defaults`, took the drop-in corpus **off zero**: one of the
48 real command lines now produces byte-identical output, and
`scripts/dropin.sh --fail-under 1` is a gate rather than a measurement,
as its own comment had promised.

### Other

- `-t html5` and `-f html5` are accepted. Pandoc has spelled it that way
  for a decade and writes identical bytes for it; ferrodoc refused it for
  its name alone. `html4` stays refused **by name**, because pandoc's
  html4 writer differs on real constructs and answering it with html
  output would be wrong rather than absent.
- `--from markdown_github` now prints pandoc's own deprecation line,
  byte for byte: `[WARNING] Deprecated: markdown_github. Use gfm
  instead.`
- `dropin/` and `scripts/dropin.sh`: 48 real pandoc command lines, run
  through both binaries and compared byte for byte. Both changes above
  were found by it.

### Breaking changes — Rust

Four, and only the first two are likely to reach a caller. The public
surface was compared symbol by symbol:

```sh
git diff v0.1.0..HEAD -- 'crates/*/src/*.rs' |
  grep -E '^[-+].*\bpub (fn|struct|enum|trait|const|type|mod|use)\b'
```

**1. Standalone HTML takes a table-of-contents flag.**

```rust
// 0.1
pub fn render_html_standalone(doc: &Pandoc, css: Option<&str>) -> Vec<u8>
pub fn write_html_standalone(doc: &Pandoc, css: Option<&str>) -> String

// 0.2
pub fn render_html_standalone(doc: &Pandoc, css: Option<&str>, toc: bool) -> Vec<u8>
pub fn write_html_standalone(doc: &Pandoc, css: Option<&str>, toc: bool) -> String
```

Pass `false` for what 0.1 did. `ferrodoc::render_html_standalone` is the
facade's; `ferrodoc_html::write_html_standalone` is the crate's, and both
moved the same way.

**2. `Format` gained seven variants.** `PandocMarkdown`, `Odt`, `Epub`,
`Ipynb`, `Latex`, `Rst` and `Asciidoc`. A `match` over `Format` with no
wildcard arm stops compiling — which is the point of the break rather
than an accident of it; a conversion silently taking a wrong branch for
`Format::Epub` would be worse.

**3. `Error` gained `NotCompiled(Format)`.** Same shape of break, for the
same reason: a build trimmed with `--no-default-features` now says which
format it was compiled without, instead of answering wrongly.

**4. `ferrodoc_docx::xml::body_children` takes the path to the body.**

```rust
// 0.1
pub fn body_children(xml: &str) -> Result<BodyChildren<'_>, Error>
// 0.2
pub fn body_children<'a>(xml: &'a str, path: &[&'static str]) -> Result<BodyChildren<'a>, Error>
```

`xml` is `#[doc(hidden)]` — it exists so `ferrodoc-odt` and
`ferrodoc-epub` can share the DOCX crate's XML reader rather than copy it
— so this is listed for completeness, not because it is supported
surface. Pass `&["body"]` for the 0.1 behaviour; `ferrodoc-odt` passes
`&["body", "text"]`, which is why the parameter exists.

Two things that *look* like breaks in the diff and are not:
`Format::readable` and `Format::embeds_media` changed their bodies, but
their answer for every format that existed in 0.1 is unchanged. The new
arms are new formats.

### Breaking changes — C, Python, TypeScript

None. All three bindings are **new in 0.2**: `bindings/c`,
`bindings/python` and `bindings/wasm` did not exist at v0.1.0
(`git diff v0.1.0..HEAD --stat -- bindings` is 3,797 lines, all
additions). There is nothing to migrate.

### Added

**Formats.** Reading: pandoc-markdown (`pandoc_markdown`, a dialect of
its own so that `markdown` cannot silently change meaning), ODT, EPUB and
Jupyter notebooks. Writing: ODT, EPUB, Jupyter notebooks, LaTeX,
reStructuredText, AsciiDoc, and a plain-text writer rewritten to match
pandoc byte for byte.

**Bindings.** Python (`ferrodoc.convert`, abi3 wheels for 3.9 and up),
WASM/npm (browser, Node and edge), and a C ABI — one header, one
function — so Go, Java, C# and Ruby can link rather than spawn.

**Feature flags.** Every format is a cargo feature with `default =
["all"]`, so nothing changes for a caller who does not ask.
`--no-default-features --features markdown,html` takes the CLI to 60% of
its size and the wasm module to 59% of its gzipped size. A trimmed build
refuses a format it does not contain, by name.

**CLI flags.** `--extract-media`, `--wrap`, `--columns`, `--toc` /
`--table-of-contents`, `--number-sections`, `-M`/`--metadata`, and
`--opt=value` spelling everywhere. `--help` lists exactly the formats the
build contains.

**Gates.** The harness went from 9 differential comparisons to 18, and
`scripts/verify.sh` — which did not exist at 0.1.0 — now runs 23 of them
as gates plus one as a measurement, against a pinned pandoc 3.8.2.1.
`./scripts/verify.sh` is the one command that decides whether the tree is
releasable.

### Fixed

The ones that changed output a user would have seen:

- **footnotes.** The markdown reader swallowed the note that followed
  one; the markdown writer numbered nested notes so that one label could
  appear twice — which makes a document pandoc cannot read without
  exhausting memory. The HTML reader read a footnote reference as a link
  rather than a `Note`.
- **three silent losses in the writers**, found by looking at the output
  rather than at a score: they are what `samples/` exists to catch.
- **`data-` written twice** on an HTML attribute that already had it.
- **task lists**: written the way pandoc writes them, and read back with
  the right boxes ticked.
- **code fences** carry their language label the way pandoc labels it.
- **a skipped heading level** still gets its EPUB section.
- **five HTML reader divergences** no gate could reach, plus the maths a
  notebook is mostly made of.
- **RST**: a substitution definition is a block, not an inline.
- **LaTeX**: strikeout no longer needs `ulem`, which a base TeX
  installation does not have.

### Performance

Boxing the two widest `Inline` payloads took the type from 152 bytes to
48 and cut peak memory 1.7–2.0× on every path. The published bound is
80× the input for documents up to 50 MB, gated in CI.

## 0.1.0

The first release: markdown (CommonMark and GFM), HTML, DOCX and the
pandoc JSON AST, as a Rust library and a CLI.
