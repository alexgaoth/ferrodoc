# Changelog

What changed between releases, and what a caller has to do about it. The
compatibility ledger is [`COMPATIBILITY.md`](COMPATIBILITY.md) and the
forward plan is [`ROADMAP.md`](ROADMAP.md); this file is the record of
what already happened.

Every "changed" entry below carries the signature on both sides, because a
break published without its note is a break twice.

## Unreleased

### `--reference-doc` for DOCX, and no refusals left in the corpus

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

### Standalone HTML is pandoc's page, byte for byte (0.5)

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
