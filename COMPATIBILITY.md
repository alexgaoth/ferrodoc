# Compatibility with pandoc 3.8.2.1

What ferrodoc converts identically to pandoc, what it converts differently,
and what it does not convert at all. Every number here is produced by a
command in this file; none of it is an estimate.

Pinned to **pandoc 3.8.2.1**. A different pandoc will produce different
numbers — some of these behaviours changed between releases, and pandoc's
published sources describe a later pandoc than this binary.

## Formats

| | read | write |
|---|---|---|
| CommonMark | yes | yes (no tables — see below) |
| GFM | yes (the five spec extensions) | yes |
| HTML | yes | yes (fragment, or `-s` for a whole page) |
| DOCX | yes | yes |
| ODT | yes | yes |
| EPUB | yes | yes |
| Jupyter notebook (ipynb) | yes | yes (nbformat 4.5) |
| LaTeX | — (never: a `.tex` expands macros) | yes |
| reStructuredText | — | yes |
| AsciiDoc | — (pandoc cannot read it either) | yes |
| pandoc JSON AST | yes | yes |
| plain text | — | yes |

Reachable from Rust, Python (`pip install ferrodoc`), JavaScript
(`npm install ferrodoc` — browser, Node and edge, 0.6 MB gzipped), any
language with an FFI (a C ABI in `bindings/c`), and the command line. Every binding converts through the same crates and is held
to the numbers below.

Everything else pandoc supports — LaTeX, EPUB, RST, Org, presentations, the
rest of its ~40 — is not converted today.

## Measured conformance

```sh
export PATH="$HOME/.cargo/bin:$HOME/.local/bin:$PATH"
cargo run -p ferrodoc-harness -- diff-spec      corpus/commonmark-spec-0.31.2.json --fail-under 100
cargo run -p ferrodoc-harness -- diff-ast       corpus --fail-under 100
cargo run -p ferrodoc-harness -- diff-html      corpus/commonmark-spec-0.31.2.json --fail-under 100
cargo run -p ferrodoc-harness -- diff-docx      corpus/docx --fail-under 96
cargo run -p ferrodoc-harness -- diff-write     corpus --fail-under 90
cargo run -p ferrodoc-harness -- diff-odt       corpus/odt --fail-under 94
cargo run -p ferrodoc-harness -- diff-odt       corpus/odt-libreoffice --fail-under 100
cargo run -p ferrodoc-harness -- diff-odt-write corpus --fail-under 100
cargo run -p ferrodoc-harness -- diff-epub-write corpus --fail-under 72
cargo run -p ferrodoc-harness -- diff-ipynb      corpus/ipynb-handmade --fail-under 100
cargo run -p ferrodoc-harness -- diff-ipynb-write corpus/ipynb-handmade --fail-under 100
cargo run -p ferrodoc-harness -- diff-md        corpus/commonmark-spec-0.31.2.json --fail-under 100
cargo run -p ferrodoc-harness -- diff-gfm       corpus/gfm --fail-under 100
cargo run -p ferrodoc-harness -- diff-gfm       corpus/commonmark-spec-0.31.2.json --fail-under 99.8
cargo run -p ferrodoc-harness -- diff-gfm-md    corpus/gfm corpus/commonmark-spec-0.31.2.json --fail-under 100
cargo run -p ferrodoc-harness -- diff-html-read corpus/commonmark-spec-0.31.2.json corpus --fail-under 96
```

| gate | what it proves | result |
|---|---|---|
| `diff-spec` | markdown reader produces pandoc's AST | **652/652** |
| `diff-ast` | any pandoc JSON round-trips to an equal value | **11/11** |
| `diff-html` | HTML writer produces pandoc's HTML | **652/652** |
| `diff-docx` | DOCX reader produces pandoc's AST | **36/37** |
| `diff-docx` (LibreOffice) | ...on documents *another* writer produced | **7/8** |
| `diff-write` | DOCX writer survives a round trip through pandoc | **10/11** |
| `diff-odt` | ODT reader produces pandoc's AST | **32/34** |
| `diff-odt` (LibreOffice) | ...on documents *another* writer produced | **8/8** |
| `diff-odt-write` | ODT writer survives a round trip through pandoc | **11/11** |
| `diff-epub` | EPUB reader produces pandoc's AST | **10/12** |
| `diff-epub` (hand-authored) | ...on books in shapes pandoc's writer never emits | **3/3** |
| `diff-epub-write` | EPUB writer survives a round trip through pandoc | **8/11** |
| `diff-ipynb` | notebook reader produces pandoc's AST | **8/8** |
| `diff-ipynb-write` | notebook writer survives a round trip through pandoc | **8/8** |
| `diff-latex` | LaTeX writer round-trips the document | **0/11** (pandoc: 0/11) — **reported, not gated**; see below |
| `diff-rst` | RST writer round-trips the document | **2/11** (pandoc: 3/11) |
| `diff-md` | markdown writer round-trips the document | **652/652** (pandoc: 593/652) |
| `diff-gfm` | GFM reader produces pandoc's AST | **655/656** |
| `diff-gfm-md` | GFM writer round-trips the document | **656/656** (pandoc: 590/656) |
| `diff-html-read` | HTML reader produces pandoc's AST | **633/659** |

The two round-trip gates are where ferrodoc is measurably *ahead*: pandoc's
own writers lose 59 of the same 652 documents in `commonmark` and 66 of 655
in `gfm`, at their best setting.

## Known losses, one by one

Nothing here is a surprise waiting to happen; each is a decision or a
measured gap with a reason.

### Markdown reader — 2 open divergences

- Entity-encoded spaces (`&#32;`) should stay inside a `Str` and do not.
- Link reference definitions followed by a run of dashes, in corner cases.

Repros: `.iterate/20260810-markdown-reader/round-3-verdict.md`.

### DOCX reader — 1 corpus document, and 1 deliberate divergence

`corpus/docx/spec-09.docx`: a list nested inside a table cell in a shape the
reader flattens.

**The DOCX corpus is pandoc's own output**, which means `diff-docx` over it
proves "ferrodoc reads what pandoc writes the way pandoc reads it" and
cannot fail on a structure pandoc's writer never emits — `Heading1` and
`TableContents` styles, LibreOffice's `numbering.xml`, `w:tblLayout`. So
there is a second corpus, `corpus/docx-libreoffice`, written by LibreOffice
Writer: eight documents covering headings, nested and mixed lists, tables
with merged cells, an embedded image, hard breaks, inline runs split at
arbitrary points, and entities.

Seven of the eight produce an AST **identical to pandoc's**. The eighth
differs because ferrodoc keeps something pandoc drops: LibreOffice writes a
horizontal rule as a paragraph that is nothing but a bottom border, and
ferrodoc reads it as `HorizontalRule` where pandoc reads nothing at all.
The rule is narrow — the paragraph must have no content beyond that single
border — so a paragraph merely styled with an underline is not affected.

### DOCX writer — 1 corpus document, and two categories

- `corpus/nested-structures.md`: a quotation nested in a way the round trip
  does not preserve.
- **Raw blocks** are dropped: OOXML has no equivalent.
- **Images** embed as PNG, JPEG, GIF, WebP, TIFF, SVG, EMF and WMF. A
  format not in that list — BMP among them — falls back to alt text rather
  than produce a package Word would reject. Every one of those eight opens
  in LibreOffice with its picture intact, which is the check no gate here
  makes.

An image is placed at the size the file itself states, resolution
included: a 300-dpi PNG is a quarter the width of a 72-dpi one with the
same pixel count. A JPEG states its resolution in two places that do not
tie — JFIF in APP0 and Exif in APP1 — and the rule, pandoc's, is Exif
where it names a resolution *and* the unit it is in, JFIF otherwise. A
scanner writes Exif and frequently no JFIF density; most Exif segments
name no resolution at all. Reading either one alone is a four-fold error
on exactly the files that bothered to record one.

Ten places diverge from pandoc deliberately, every row below measured
against the 3.8.2.1 binary on a real file of that format:

| | pandoc | ferrodoc |
|---|---|---|
| lossless WebP, 7x11 | 1x6 — it mis-reads the VP8L header | 7x11 |
| big-endian TIFF or Exif, 300 dpi | 44 dpi — it reads the rationals in the wrong byte order | 300 dpi |
| JPEG with neither JFIF nor Exif | 300 x 200 pt — no size at all | the frame header's 7x11 |
| EMF, frame 185x291 | 4.5 x 8.25 pt | 5.244 x 8.248 pt |
| WMF, any | 300 x 200 pt | the placeable header's own size |
| SVG behind a comment or doctype with no XML declaration | 300 x 200 pt | the root's own 7x11 |
| SVG, `width="50%"` with a `viewBox` | width 0 — an invisible picture | the view box, 40 x 20 px |
| SVG, only percentage lengths, no `viewBox` | 0 x 0 — invisible | 300 x 200 pt |
| SVG, `viewBox="0 0 40.9 20.5"` or `viewBox="0,0,40,20"` | 300 x 200 pt | 40 x 20 px |
| SVG, `width="10em"` | 123.75 x 247.5 pt | 300 x 200 pt |

They fall into three kinds. **Six** — rows 3 and 5 through 9 — are pandoc
producing a size nobody chose: a zero extent, or the 300 x 200 point
fallback it uses for an image it cannot size at all. (An `<svg>` that
states no size *and* no view box is not among them: pandoc's fallback is
what ferrodoc writes too, so the two agree.) **Two** — the lossless WebP
and the byte-swapped rationals — are pandoc reading a header wrongly.
The remaining **two**, the EMF and the `em` length, are pandoc computing
a size correctly from a rule this declines to follow.

The EMF row is the substantive divergence, because it is the one where
pandoc parses everything correctly and still disagrees. Pandoc quantises
the frame onto the pixel grid of the machine that *recorded* the
metafile, taking a resolution from the header's device fields — so the
same drawing is 4.5 pt wide when they claim a 96-dpi monitor and 4.875 pt
when they claim 192. Reproduce any row with
`printf '![](x.svg)' | pandoc -f markdown -t docx -o o.docx` and read
`wp:extent` out of `word/document.xml`.

An image whose stated or intrinsic size falls outside OOXML's coordinate
range — a header claiming four billion pixels, a resolution so large the
extent rounds to nothing — is alt text rather than a drawing. Word rejects
such a package and LibreOffice opens it with the picture silently gone,
which is worse than the alt text, because nothing is left to read.

Two limits in the same area that are *not* deliberate, both measured:
a stated `{width=100px}` is 100 points here and 100 pixels at 96 dpi
(75 points) for pandoc; and giving only one of width and height leaves the
other at the image's own size, where pandoc scales it to keep the aspect
ratio.

`docx → docx` keeps its pictures. The bytes go through unchanged; only the
part name is renumbered. Media is read **only when the output can embed it**,
because a `.docx` can hold a part that inflates a thousandfold and
`docx → markdown` has no use for it: on a 400 MB image part that is 5 MB of
peak RSS rather than 840 MB.

Where it stops:

- an image in a format the writer cannot embed (above) is alt text on the
  way out, so it is not in the new package either;
- a part the archive does not actually hold is skipped, not an error;
- a relationship that is external (`TargetMode="External"`) is a link, not
  an embedded part, and stays one.

Two things worth knowing about the other direction. A picture inside a
footnote is declared by that note's own relationship table, so it is read
from `word/_rels/footnotes.xml.rels` and only falls back to the document's
when the note declares none — which is the shape **pandoc writes**, and why
`pandoc a.docx -t markdown` drops a footnote image from pandoc's own file
while ferrodoc keeps it. And a figure is written with pandoc's
`CaptionedFigure` style: written any other way pandoc's reader drops the
picture, so a round trip through this writer would lose it one hop later.

### ODT reader — 2 corpus documents, and what pandoc's own reader cannot hold

`corpus/odt` is pandoc's own output and `corpus/odt-libreoffice` is
LibreOffice's — the same split, and for the same reason, as the two DOCX
corpora. **All eight LibreOffice documents produce an AST identical to
pandoc's.** Two of the 34 pandoc-written ones do not, and both for one
reason:

**Pandoc reads every list twice** — and 2^n times at n levels of nesting.
The only observable effect is on identifiers: a heading or bookmark inside
a list is allocated once per pass, so pandoc's suffix is one higher than
ferrodoc's (`foo-1` where ferrodoc says `foo`). The identifiers are unique
and internally consistent either way. This is deliberately not reproduced:
copying an exponential blowup into a converter whose promise is that it
cannot be made to hang is the worse trade. `corpus/odt/spec-03.odt` and
`corpus/odt/spec-09.odt`.

Everything else in this section is pandoc's ODT reader, not ferrodoc's, and
ferrodoc matches it exactly. It is a much plainer reader than pandoc's docx
one, and it is worth knowing before choosing ODT as an interchange format:

- **no metadata.** `meta.xml` is not read at all. A title, author or date
  comes back as an ordinary paragraph, not as document metadata;
- **no code blocks.** A code block written by pandoc's *own* ODT writer
  does not survive its own reader: it comes back as one paragraph per line,
  with the indentation intact and the language gone;
- **no table spans, widths or alignments.** A merged cell becomes a
  single-column cell and the row is padded at the end; every column is
  `ColWidthDefault` and `AlignDefault`;
- a horizontal rule is dropped, and so are annotations, tables of contents,
  text boxes and the field elements (`text:date`, `text:page-number`);
- a multi-paragraph block quote comes back as one quote per paragraph;
- a `text:a` pointing at `#name` is not rewritten to the identifier that
  bookmark was actually given, so an internal link lands nowhere.

ferrodoc drops one thing pandoc keeps: `text:bibliography-mark` becomes a
`Cite` for pandoc and nothing here, citations being a declared non-goal.

### ODT writer — everything the reader above cannot hold

`diff-odt-write` compares this writer's output *read back by pandoc*
against pandoc's own output read back the same way, which is what isolates
the writer from the format: a loss both writers share does not count
against either. On that measure it is **11/11 on the corpus**, and
**640/652** over the spec examples
(`diff-odt-write corpus/commonmark-spec-0.31.2.json`).

What the format loses on the way through is the list above, and it applies
to both writers equally: a code block becomes paragraphs, a horizontal rule
disappears, a table's merged cells flatten, and metadata does not survive.
Use DOCX rather than ODT when a document has to keep its code blocks.

One divergence is ferrodoc's own: a `Div`'s attributes are dropped rather
than written as a `text:section`, because pandoc's writer drops them too
and reading one back would produce a `Div` pandoc's own round trip does
not.

### EPUB reader — 2 corpus documents, and a corpus that measures something else

An EPUB's content documents are XHTML, so this reader's fidelity is the
HTML reader's fidelity plus the spine. That has a consequence worth stating
before the numbers: **the two documents it misses are both HTML reader
divergences** (an unterminated comment, and a line break inside code), not
EPUB ones. They are in the 26 listed under the HTML reader below.

Three corpora, because they measure three different things:

| corpus | what it is | score |
|---|---|---|
| `corpus/epub` | pandoc's own output, 12 documents | **10/12** |
| `corpus/epub-handmade` | books in shapes pandoc's writer never emits | **3/3** |
| `corpus/epub-spec` | 22 files of 30 spec examples each | 8/22 |

The last is not a fidelity claim and is not averaged into the others. Each
of its files bundles 30 spec examples, so **one** of the HTML reader's 26
known divergences fails a whole document — and at 30 examples per file most
files contain one. Reporting it as "the EPUB reader scores 36%" would be
reporting the HTML reader's score under another name. It is still gated,
because a *drop* there is a real regression.

`corpus/epub-handmade` is the one that found things. It is hand-authored
rather than produced by another program — EPUB 2 with a `toc.ncx`, an
`OEBPS/` layout, a package document at the archive root, a spine whose
order is not the file order, a `linear="no"` cover, a percent-encoded
href — and it is validated by **epubcheck** in CI. Two rules came out of it
failing: pandoc's EPUB reader generates no heading identifiers at all, and
the anchor it emits per file is named for the *raw* href, not the decoded
one.

What pandoc's EPUB reader does that is worth knowing:

- **a `linear="no"` item contributes nothing**, not even its anchor, and a
  title page contributes its anchor and nothing else — both are furniture
  a reading system shows around the text;
- **every identifier is prefixed with the file it came from**
  (`ch001.xhtml_intro`), because the spine makes one document out of many
  files and two chapters may each define `#intro`. Links are rewritten to
  match;
- **footnotes are put back together**: an EPUB keeps a note in an `<aside>`
  at the end of the file with a link to it, and read literally that is a
  link to the bottom of the chapter;
- an **image** src is resolved against its chapter's directory, a **link**
  href is left exactly as written.

### EPUB writer — 3 corpus documents, all one deliberate rule

`diff-epub-write` scores **8/11**, and the three that differ differ for the
same reason: **this writer does not emit a reference the book cannot
satisfy.**

- an image whose bytes cannot be found becomes its alt text
  (`corpus/images.md`, `corpus/readme-style.md`);
- a relative link naming a file that is not in the book becomes its text
  (`corpus/nested-structures.md`). A fragment (`#section`) is a link *into*
  the book and is kept, as is anything with a scheme.

Pandoc writes both references, and `epubcheck` rejects the resulting book
for exactly them (`RSC-007`). Raising the number would mean copying that,
so the check that settles it runs beside the gate: **`epubcheck` accepts
every book this writer produces — 0 fatals, 0 errors, 0 warnings — and
does not accept pandoc's** on the same corpus.

Three more differences are not scored at all, because no writer could
match them and matching would cost something worth more:

| | pandoc | ferrodoc |
|---|---|---|
| `dc:title`, document untitled | omitted — `epubcheck` errors, EPUB 3 requires one | `Untitled` |
| `dc:identifier` | a fresh random UUID every run | derived from the content, so a book is reproducible |
| `dcterms:modified` | the current time | fixed, for the same reason |
| `dc:language`, unspecified | the machine's locale (`de-DE` under `de_DE.UTF-8`) | `en` |

The gate drops those four fields rather than report a clock, a dice roll
or a locale — and only in their exact unmatchable form, so a book that
lost its identifier or invented a title still fails.

An unterminated HTML comment is **closed**, not dropped: XML has none of
HTML's tolerance, and left as written the book will not open at all.
Pandoc does the same. A well-formed comment is untouched.

Syntax highlighting is not done, so a code block carries no `cbN`
identifier and no per-token markup; the gate passes
`--syntax-highlighting=none`, exactly as the HTML gate does.

### Jupyter notebooks — both gates at 8/8, and 6 divergences the corpus avoids

Both notebook gates are at **100%**: `diff-ipynb` reads all 8 hand-authored
notebooks to exactly pandoc's AST, and `diff-ipynb-write` writes all 8 to a
notebook that comes back through pandoc's reader identical to pandoc's own.

```sh
cargo build --release -p ferrodoc-harness
./target/release/ferrodoc-harness diff-ipynb       corpus/ipynb-handmade --fail-under 100
./target/release/ferrodoc-harness diff-ipynb-write corpus/ipynb-handmade --fail-under 100
```

**The writer gate drops one thing, and only in the form nothing can
match.** nbformat 4.5 requires an `id` on every cell, so a cell whose AST
carries none forces both writers to invent one: pandoc draws a random
UUID, this derives a UUID-*shaped* string from the cell's own content so a
notebook written twice is byte-identical. The gate clears a cell `Div`
identifier on both sides **only when it is the 8-4-4-4-12 hex shape**, so a
cell that loses a real Jupyter id — which is 8 hex characters, `3a7f1c2d`
— still fails. Confirmed by mutation: making the writer discard every
identifier drops the gate from 8/8 to **0/8**. On this corpus the drop
never fires, because every cell carries a real id; it fires on a notebook
written before nbformat 4.5, where both sides invent and the gate still
reports 1/1. The guarantee is therefore scoped to Jupyter's actual 8-hex
form, not to "a real id": a notebook whose cells genuinely carry
8-4-4-4-12 identifiers would have them cleared on both sides and could lose
them unnoticed. No such notebook is in the corpus, and Jupyter does not
write that shape — but the limit is the id's *shape*, not its realness.

**The judge that is not us** is `nbformat.validate` — Jupyter's own schema
validator, `nbformat 5.11.1`, installed with `pip install nbformat` and run
in CI over a notebook written from each corpus document plus one written
from markdown: **9/9 accepted**, all at nbformat 4.5 with an `id` on every
cell.

```sh
pip install nbformat && cargo build --release -p ferrodoc
python3 scripts/nbformat-check.py corpus/ipynb-handmade/*.ipynb
```

**Six divergences in a markdown cell, none of them reached by the corpus.**
A markdown cell is markdown, and pandoc parses it with an extension set
that is neither CommonMark nor GFM: it has pipe tables, task lists,
strikeout, `$…$` math and raw HTML, and it has neither bare-URI autolinks
nor footnotes nor escaped line breaks nor `fancy_lists`. `read_gfm` is the
closest reader this project has, and these are what remains. Each command
below prints what it claims:

```sh
cargo build --release -p ferrodoc
python3 - <<'EOF'
import json; json.dump({"cells":[{"cell_type":"markdown","id":"c1","metadata":{},
 "source":["See www.example.org for more.\n"]}],"metadata":{},"nbformat":4,
 "nbformat_minor":5}, open('/tmp/div.ipynb','w'))
EOF
pandoc -f ipynb -t json /tmp/div.ipynb
./target/release/ferrodoc /tmp/div.ipynb -f ipynb -t json
```

| in a markdown cell | pandoc | ferrodoc |
|---|---|---|
| `See www.example.org for more.` | `Str "www.example.org"` — no autolink | `Link` to `http://www.example.org` |
| `text[^1]` with `[^1]: the note` | `Link ["^1"] ("the%20note","")` | two `Para`s, the syntax left literal |
| `one\` at end of line | `Str "one\\"` then `SoftBreak` | `LineBreak` |
| `<div class="note">\nhi\n</div>` | `RawBlock "html"` **without** the final newline | with it |
| `# Done 😀` | identifier `done-grinning` | identifier `done-` |
| `[https://x](https://x)` written out | `Link ("",[],[])` | `Link ("",["uri"],[])` |

The first three and the fifth are the *flavour*: pandoc's ipynb markdown and
GFM genuinely disagree, and matching them would mean a fifth markdown
reader. The fourth is ferrodoc's own gap — the trailing newline of a raw
HTML block differs between pandoc's markdown reader and its CommonMark one.

The sixth is this reader's own cost of a fix. Comrak does not record whether
a link was written as an autolink, so `autolink_class` uses the test that
distinguishes them in the source — an autolink's text *is* its target — and
an explicit `[url](url)` therefore gets the class pandoc leaves off. Named
here rather than papered over.

**Two that used to be on this list are now fixed, and the corpus carries
them**: `$…$` math and bare-URI autolinks. Both were absent from the corpus
when these gates first read 100%, and a review measured that adding nothing
but inline math dropped the reader to 5/8 and the writer to 6/8. The corpus
was widened rather than the claim narrowed; `read_gfm` now reads `$x$` and
`$$x$$` as pandoc's `gfm` does, the markdown writer emits math verbatim
instead of escaping it into `\\sum\_i`, and `<url>` autolinks survive a
round trip with their `uri`/`email` class. Both gates are 100% on the wider
corpus, which is the number that means something.

**Three things pandoc's notebook writer loses, which this writer copies
deliberately** — the gate compares the two readbacks, so copying them is
what keeps it measuring the writer:

- an image output's `width`/`height` are written into the output's
  `metadata` **unkeyed**, where pandoc's own reader looks for them under
  the mime type — so they do not survive `ipynb → ipynb`;
- `nbformat_minor` is forced to 5, whatever the document said;
- a raw cell's `format` metadata is rewritten to `raw_mimetype`.

### Markdown writer — 4 limits of CommonMark itself

**Use `-t gfm`, not `-t markdown`, for anything with a table.** CommonMark
has no table syntax, so a table becomes one paragraph per cell there and
the row/column relationship is gone — not recoverable afterwards. GFM
output keeps it.

Superscript, subscript, underline, small caps and a span carrying
attributes have no markdown syntax, and are written as raw HTML —
`<sup>`, `<sub>`, `<u>`, `<span class="smallcaps">` — which is what pandoc
writes, byte for byte. They used to degrade to their content, which lost
meaning rather than styling: `H~2~O` became `H2O` and an anchor a link
pointed at disappeared.

The other four, listed in `crates/ferrodoc-markdown/src/write.rs`: footnotes
and definition lists degrade to their content; emphasis directly inside
emphasis inside a word; two ordered lists in a row sharing a delimiter; an
unterminated raw HTML block swallowing the blank line after it.

One divergence beside those limits, pre-existing and deliberate to record
rather than to fix: a `CodeBlock` whose `Attr` is **entirely empty** is
written as a bare ```` ``` ```` fence here and as an *indented* block by
pandoc. It is the last class-related shape still spelled differently, and it
is three lines of `samples/05-html-to-markdown/diff.txt`, which is where a
reader meets it. Any non-empty `Attr` agrees: `["sourceCode"]` alone, an id
alone, or key-values alone all fence in both. No round trip can see it,
because the two spellings read back to the same `CodeBlock`, and pandoc
indents unconditionally — a blank line or a backtick run in the content does
not make it fence. Measured on `-t markdown`, `-t gfm` and `-t commonmark`:

    block='{"pandoc-api-version":[1,23,1],"meta":{},"blocks":[{"t":"CodeBlock","c":[["",[],[]],"x"]}]}'
    printf '%s' "$block" | pandoc   -f json -t gfm
    printf '%s' "$block" | ferrodoc -f json -t gfm

### GFM — a chosen subset, and what a pipe table cannot hold

ferrodoc reads and writes the five extensions the **GFM specification**
defines: pipe tables, task list items, strikethrough, extended autolinks,
and tag filtering (which is off, because pandoc does not apply it either).
Heading identifiers are derived too, since pandoc's `gfm` always does.

Pandoc's `gfm` additionally bundles *pandoc* extensions the GFM
specification does not define. Two of them **are** read, because a
document that has them is wrong without them: `$math$` and **footnotes**.
Emoji shortcodes, alerts and YAML metadata blocks are not — that last is
the single `diff-gfm` mismatch, since pandoc reads `---\n---\n` as an
empty metadata block where we read two thematic breaks.

**Footnotes** agree with `pandoc -f gfm` on all fifteen shapes probed,
including the two quirks: a reference inside a footnote body resolves to
nothing (`[^1]: outer[^2]` gives `Note [Para [Str "outer", Str ""]]` —
`[^2]`'s body is never reached), and a reference with no definition stays
literal text. `read_commonmark` reads none of them, as `pandoc -f
commonmark` does not. `corpus/gfm/footnotes.gfm` covers twelve.

> **Pandoc exhausts memory on a self-referential footnote.**
> `printf 'a[^1]\n\n[^1]: see [^1]\n' | pandoc -f gfm -t json` took
> **1.4 GB in 5 s** and was still climbing when a 2 GB cap stopped it;
> ferrodoc reads the same file in **4.6 MB and 0.00 s**. Bodies here never
> resolve references — which is pandoc's own semantics, above — so the
> conversion is not recursive and terminates on every input.

Six further reader divergences, all the same shape: ferrodoc parses with
comrak, a port of GitHub's own **`cmark-gfm`**, and pandoc parses with
`commonmark-hs`, which is stricter than the implementation the format is
named after. Where the two disagree on well-formed input this reader
follows GitHub — a GFM document should convert the way GitHub reads it.
These are not in the corpus, because the corpus is scored against pandoc;
each is pinned by
`deliberate_divergences_from_pandoc_hold` in `crates/ferrodoc-markdown/src/lib.rs`.

| input | here (and on GitHub) | pandoc |
|---|---|---|
| `~one tilde~` | `Strikeout` | literal tildes |
| `Text:` then a pipe table | `Para` + `Table` | one `Para` of literal pipes |
| a plain line after a table row | another row | a new `Para` |
| `[http://e.com](http://e.com)` | link text stays text | `Link` inside a `Link` |
| `mailto:x@e.com` | link text keeps the scheme | `Str "mailto:"` + `Link` |
| `a.www.e.com` | no autolink — `www.` needs a word boundary | autolinked |
| `- [x]` then a lazy line | the line continues the item | the list closes, new `Para` |

The first two are the ones an ordinary GitHub README hits.

A pipe table always keeps its grid; everything else about a table degrades
in a stated way, because GFM's table syntax is one header row of one-line
cells:

- extra head rows and the whole foot become body rows;
- a cell spanning columns becomes that many cells, content in the first;
  a cell spanning rows leaves the rows below it short, which pads them;
- a cell's blocks are flattened onto one line, joined by spaces — a list
  in a cell loses its markers;
- the caption follows the table as an ordinary paragraph;
- column widths are dropped, and a table with no head gains an empty
  header row;
- a `Cell`'s own alignment is dropped — only the column's survives, since
  a pipe table states alignment once per column in the delimiter row.

Pandoc demotes extra head and foot rows exactly as this does, but for a
table with merged or nested cells it falls back to a raw HTML `<table>`
instead. That renders on GitHub too, and it keeps the merge — but it
re-reads as a `RawBlock` rather than a table, so the document stops being
a table at all. Keeping the grid is the smaller loss.

Strikeout degrades in three stated ways, because `~~` is a flanking
delimiter and two runs that meet make a tilde code fence:

- a space just inside the `~~` moves outside it (`~~a~~ b`), as pandoc
  does too;
- content that is nothing but whitespace loses the markup entirely;
- strikeout whose content starts or ends with strikeout loses one of the
  two levels — the one at the edge keeps its `~~` and the other does not,
  so `<del>a<del>b</del></del>` comes back as plain `a` plus struck `b`.
  Nesting with text on both sides (`~~outer ~~inner~~ tail~~`) is exact.
  Two adjacent `Strikeout` nodes arrive back as one, which is what
  pandoc's *reader* makes of adjacent `<del>` anyway. Pandoc's writer
  emits the four tildes instead, and its own output then re-reads as a
  code block that swallows the rest of the document.

### HTML writer — 652/652, and the shape that gate cannot see

`diff-html` scores against the CommonMark specification, which has no task
lists in it, and no round trip can see one either, because `- ☐ a` and
`- [ ] a` are one AST. So the writer's task lists are held by literal-output
tests in `crates/ferrodoc-html/src/lib.rs` instead, measured against pandoc
3.8.2.1 for the same AST. The rule, as probed: a list item whose first
`Plain`/`Para` opens with a `Str "☒"`/`Str "☐"` **immediately followed by a
`Space`** becomes `<label><input type="checkbox" checked="" />…</label>` —
`checked` present and empty when ticked, absent when not — and a `<ul>`
takes `class="task-list"` only when *every* item is one. A mixed list keeps
the boxes and loses the class; an `<ol>` never takes the class and still
gets the boxes. Anything narrower stays literal text: no `Space` after the
box, a `SoftBreak` in its place, the box and its space in one `Str`, or the
box inside an `Emph`.

    printf -- '- [x] a\n- [ ] b\n' | ferrodoc -f gfm -t html
    printf -- '- [x] a\n- [ ] b\n' | pandoc -f gfm -t html --wrap=none

Pandoc's LaTeX writer makes something else of the same AST (`\item[$\boxtimes$]`,
and it leaves ordered lists alone); that is a different writer and ferrodoc's
LaTeX writer is gated on fidelity, not on matching it.

### HTML reader — 26 of 659

Most are one cause: **ferrodoc parses to the HTML5 spec via `html5ever`,
pandoc parses with `tagsoup`, which does not.** On malformed markup the two
build different trees and no mapping reconciles them — a tag with no closing
`>`, an unclosed `<a>` whose formatting element is reconstructed after the
paragraph, a `<pre>` or `<div>` opened inside a `<tr>` and never closed.

Not all of them are that, and assuming so hid real bugs for a review round:
the `<![CDATA[…]]>` and `<style>` raw-text boundaries are a tokenizer
disagreement on input that is merely unusual, and `<a/>` self-closing syntax
sends the two parsers down different recovery paths.

Four deliberate divergences, all chosen on the same principle — *match
pandoc wherever pandoc has a describable rule on well-formed input; diverge
only where matching would mean reproducing a parse failure*:

- An `<a href="…"></a>` with no text is **kept**. Dropping it would match
  pandoc on unclosed `<a>` tags but delete a well-formed empty anchor, which
  real pages use as jump targets.
- A newline immediately after `<pre>` is **kept**, matching pandoc rather
  than the HTML spec, because a code block silently losing its first line is
  worse than disagreeing about an invisible character.
- `<output>`, `<canvas>` and `<textarea>` stay **inline**. Pandoc counts
  them block-level and splits the paragraph around them into `Plain`
  fragments; all three are phrasing content, so a paragraph that mentions
  one is one paragraph. Measured: `<p>x <canvas>t</canvas> y</p>` is three
  blocks to pandoc and one `Para` here.
- An `<input type="checkbox">` **outside a list item** is dropped and
  nothing else. Inside one it is a task list's box and is read as pandoc
  reads it — `Str "☒"` or `Str "☐"` then `Space`, written where the element
  stands — which is what makes a task list survive a round trip through
  HTML, because `<li><label><input type="checkbox" checked="" />done</label>`
  is what pandoc's own HTML writer emits for one. What pandoc does with the
  element *outside* a list item is not one rule but three, measured one
  context at a time, and only the first is a divergence:
  - In a `<p>` pandoc drops the element **and breaks the block around it**,
    so `<p>loose <input type="checkbox" /> in a paragraph</p>` is two blocks
    to pandoc and one `Para` here. Matching that would mean reproducing a
    parse failure. Reproduce with
    `printf '<p>loose <input type="checkbox" /> in a paragraph</p>' | pandoc -f html -t json`
  - In a `<td>` pandoc drops the element and leaves the block whole, and
    ferrodoc matches it byte for byte — one `Plain` either way. Reproduce with
    `printf '<table><tr><td><input type="checkbox" checked="" />plain cell text</td></tr></table>' | pandoc -f html -t json`
  - In an `<h2>` pandoc loses the `Header` **entirely**, emitting two
    `Plain`s rather than a broken heading, where ferrodoc keeps `Header 2`.
    Reproduce with
    `printf '<h2>head <input type="checkbox" /> box</h2>' | pandoc -f html -t json`

  One more consequence of `tagsoup`: pandoc reads the box only
  when the tag closes itself. Written `<input type="checkbox" checked="">`,
  with no `/`, the tag stays open for pandoc and takes the rest of the list
  with it — `<ol><li><label><input type="checkbox" checked="">a</label></li></ol>`
  loses its list entirely, where `html5ever` knows the element is void.

Two more, measured, where ferrodoc keeps *more* than pandoc: a `<tr>`
inside a `<template>` stays a table row here (tagsoup has no notion of a
template, so pandoc flattens the table to a `Plain`), and a `<bdo>` keeps
the `dir` attribute that is its whole point.

An **inline `<svg>`** is carried as its own bytes, in a
`data:image/svg+xml;base64,…` URL, the same shape pandoc uses — and a
`data:` URL now reaches the DOCX writer, so a chart written into a page
converts all the way to a `.docx` that opens with the picture. Two details
of the serialization differ, both measured:

- **Case is kept, in element and attribute names alike.** Pandoc
  lowercases both, and SVG is case-sensitive, so its payload for anything
  with a capital in it is not the picture that went in: `viewBox` becomes
  `viewbox` and is ignored, and `<linearGradient>`, `<clipPath>`,
  `<textPath>`, `<foreignObject>` and the whole `<feGaussianBlur>` filter
  family become elements that do not exist. A gradient or a clip path is
  in most real charts. Measured: the same drawing renders 61x41 pixels
  through LibreOffice with `viewBox` and **31x31** with `viewbox`.
  Matching pandoc would mean shipping a broken picture.
- **`<rect>`, `<path>` and `<use>`** are written `<rect></rect>` here and
  `<rect />` by pandoc, which treats those three as void. Every other SVG
  element whose name is already lowercase agrees exactly — `circle`,
  `ellipse`, `line`, `polygon`, `polyline`, `g`, `text`, `title`, `desc`,
  `defs`, `image` among them — and both spellings render identically.
- **A wrapped `data:` URL is decoded**, not refused: ASCII whitespace is
  stripped first, the way the WHATWG's "forgiving base64" and RFC 2397
  both specify, because a URL long enough to hold a picture is routinely
  broken across lines.

Also `<xmp>`, an obsolete raw-text element, is read as raw text here (which
is what the HTML spec says it is) and as markup by pandoc.

One case is neither: a `<template>` that is the **first thing in a
document** with no `<body>` around it. A conforming parser puts it in the
head, which this reader does not read, and tagsoup has no head to put it
in — so pandoc sees the content and ferrodoc does not. Anywhere else, a
template's content is read and spliced where the element stood.

Content a browser may never display is content, and is read: a
`<template>`'s (which `html5ever` parses into a fragment of its own rather
than into the element's children) and a `<noscript>`'s (which the reader
asks for as markup, by parsing with scripting disabled, because pandoc has
no notion of scripting at all). Both were silently returning nothing.

### LaTeX fidelity — reported, and deliberately not a gate

`diff-latex` prints a number and cannot fail `verify.sh`. It ran at a 9%
floor while exactly one of eleven corpus documents round-tripped;
`corpus/docx/src/anchors-and-notes.md` was that one, and it stopped
passing when its footnotes started being read, because **a footnote
containing a list does not survive pandoc's LaTeX reader**:

```console
$ printf 'a[^1]\n\n[^1]: intro\n\n    - one\n    - two\n' > n.md
$ ferrodoc n.md -f gfm -t json | pandoc -f json -t latex | pandoc -f latex -t json
… "BulletList", "c": [[{"t": "Para", …    # Plain went in; Para comes back
```

That is **pandoc's own LaTeX round-tripped by pandoc**, and it loses the
tightness identically. Adding `\tightlist`, removing it, and putting the
items on one line all give the same result. The remaining divergences —
`DefaultStyle` where the document said `Decimal`, a dropped code-block
language — fail pandoc's own output the same way, which is why the
harness prints `pandoc round-trips 0/11` beside our score.

An oracle that scores zero cannot set a floor for us, so there is none.
What decides this writer instead: CI compiles every corpus document with
`pdflatex -halt-on-error`, and each rule a round trip cannot observe has a
literal-output test — `\tightlist`, the `\verb` delimiter search, and the
`enumerate` styles.

### The write-only formats — judged by their toolchains, not by pandoc

LaTeX, RST and AsciiDoc are written and never read, deliberately: people
author them by hand and convert *out of* them far more often than in, and
a `.tex` file expands user-defined macros, which is a language rather than
a format.

That changes how they are gated, and the numbers above need reading with
care:

- **a LaTeX round trip is lossy for everybody.** Pandoc's own scores
  **0/11** on this corpus — its reader turns a code block with a language
  into two empty divs, drops link titles, and invents a heading identifier
  where the document had none. The 1/11 is a measure of the *format*
  through pandoc's reader, not of the writer. What is checked instead is
  that **every corpus document compiles**, in CI, which is what anyone
  actually does with LaTeX. One caveat, and it is the engine's rather than
  the writer's: **`pdflatex` cannot set a character `inputenc` has no
  declaration for**, so a document containing an emoji — `edge-cases.md`
  has a ☕ — needs `lualatex` or `xelatex`. The character is emitted raw,
  which is both what pandoc does and what `diff-latex` requires; CI tries
  `pdflatex` first and names any document that needed a Unicode engine;
- **RST cannot nest inline markup at all**, and has no link title and no
  strikeout, so a document using any of them cannot return unchanged.
  `sphinx-build -W` — warnings as errors — is the real check, because a
  short title underline or a misaligned grid table is a warning and both
  mean the document is wrong;
- **AsciiDoc has no gate at all, and cannot have one.** Pandoc writes
  AsciiDoc and does not read it, so there is no oracle to compare against.
  `asciidoctor --failure-level=WARN` accepts every corpus document in CI,
  and the writer's own tests hold the shapes a toolchain accepts and then
  silently mis-renders — markers the opposite way round from markdown, a
  fence that must clear any run inside it, headings starting at `==`.

## Resource limits worth knowing

Memory is a product feature here, not hygiene: the reason to link this
rather than spawn pandoc is that it fits inside your process, and a browser
tab or a 128 MB edge worker has a hard ceiling. So the bound is measured,
published, and gated in CI.

```sh
./scripts/verify.sh --limits      # fails if any path exceeds the bound
```

Peak resident memory, as a multiple of the input, on 10 MB of generated
prose (`bash corpus/bench/generate.sh`):

| path | peak RSS | ratio |
|---|---|---|
| markdown → AST | 359 MB | 35.9× |
| markdown → HTML | 359 MB | 35.9× |
| markdown → ODT | 359 MB | 35.9× |
| ODT → markdown | 367 MB | 36.7× |
| HTML → AST | 379 MB | 37.8× |
| markdown → DOCX | 654 MB | 65.4× |
| **DOCX → markdown** | **738 MB** | **73.8×** |

CI holds the worst path at **80×**, which is a regression bound rather than
an aspiration: nothing may quietly get hungrier.

**What this means in practice.** A 1 MB document needs roughly 75 MB and
fits anywhere. A 10 MB document needs about 750 MB and does not fit a small
edge worker; convert it in a process with room, or split it. The ratio is
stable across sizes, so it multiplies out honestly.

**Why the floor is around 36×.** Holding a pandoc AST costs what it costs:
every word is a separate `Str` with its own heap allocation, and every
element of a `Vec<Inline>` is as wide as the widest variant. Boxing every
wide payload took `Inline` from **152 to 48 bytes** and cut peak memory
**1.7–2.0× across every path**. What remains is one allocation per word,
which no amount of boxing reaches — and the three ways to fix *that* each
cost something this project values more (raw pointers, a dependency in the
AST crate, or a public type that cannot serialize alone).

Two further limits worth planning around. The DOCX body is read one part at
a time, so its XML tree never exists in full — streaming it cut peak RSS
2.7× and was ~12% *faster*, measured interleaved against a baseline build.
And an image part is read only when the output can embed it, so a `.docx`
carrying a part that inflates a thousandfold costs 5 MB through
`-t markdown` and 840 MB through `-o out.docx`, which has to hold it.

**Time is linear per document but not per byte.** Reading eight 1 MB DOCX
files takes 4.74 s; reading one 8 MB file takes 8.40 s — the same content,
1.77× the time. That is not an algorithmic defect in the mapping (the
per-document cost is constant) and it is not byte volume either (cutting
memory 1.6× left the curve unchanged). It is the *number* of live
allocations. `docx → AST` therefore grows about 20× for 10× the input, and
that is the one path where size costs more than proportionally.

### `markdown` means CommonMark, not pandoc's markdown

The flag spelling is the same and the dialects are not. `pandoc -f
markdown` is pandoc's own dialect; `ferrodoc -f markdown` is CommonMark,
and extension inference picks it for a `.md` file. Five things a pandoc
document may carry are read as the literal text they are written with:

```console
$ cat sample.md
---
title: A report
---

# Heading {#custom-id .fancy}

Text with a footnote.[^1]

[^1]: The note body.

Term
:   Definition of the term.

H~2~O and E=mc^2^.

$ ferrodoc -f markdown -t html sample.md
<hr />
<h2>title: A report</h2>
<h1>Heading {#custom-id .fancy}</h1>
<p>Text with a footnote.[^1]</p>
<p>[^1]: The note body.</p>
<p>Term : Definition of the term.</p>
<p>H~2~O and E=mc^2^.</p>
```

Footnotes are the exception that moved: `-f gfm` reads them, matching
`pandoc -f gfm`, and `-f markdown` reads none, matching `pandoc -f
commonmark`.

**Only the metadata block makes the output wrong rather than narrower** —
the title and author appear in the body — so that one case warns on
stderr. The test is pandoc's own, probed: the first line is exactly `---`,
the line after it is not blank, and a later line is exactly `---` or
`...`. `stdout` and the exit code are untouched.

### `--wrap` — the default differs from pandoc's, deliberately

**Pandoc fills text output to 72 columns by default; ferrodoc leaves every
line where the document put it.** That is `--wrap=preserve` against
pandoc's `--wrap=auto`, and it is the one CLI default that differs.

The reason is what a migration looks like. Converting a corpus with both
tools and diffing is how anyone checks a swap, and filling by default makes
*every paragraph of every document* differ on line breaks alone — burying
whatever real difference the diff was run to find. Leaving the text alone
makes the diff readable, and the fill is one flag away.

`--wrap=auto` (with `--columns N`, default 72) matches pandoc. Measured in
isolation, because scoring it over whole documents would score this
project's other divergences at the same time: of the 79 DOCX and ODT corpus
documents, **10** already produce byte-identical GFM to
`pandoc --wrap=none`. On those 10 — the subset where wrapping is the only
variable — `ferrodoc --wrap=auto --columns 72` is **10/10 identical** to
`pandoc -t gfm` at its default.

A line breaks only where a `Space` or `SoftBreak` stood in the tree, never
inside a code span, a link destination or a link title, which are written
rather than read from the document. A heading is never filled (pandoc
leaves a 151-column heading at 151), a pipe table row is never filled, and
a word wider than the column overruns rather than being cut. A list marker
and a quote's `> ` count toward the width, as they do for pandoc.

### `--extract-media` — file for file with pandoc

`ferrodoc <doc> --extract-media DIR` writes each embedded image to
`DIR/<the path the AST names>` and rewrites the reference to that joined
path, which is what `pandoc --extract-media` does. Verified against 3.8.2.1
on every reader that carries media, `cmp` per file:

| document | files | byte-identical |
|---|---|---|
| `corpus/docx/notes-and-images.docx` | 1 | 1 |
| `corpus/odt/notes-and-images.odt` | 3 | 3 |
| `corpus/epub/notes-and-images.epub` | 1 | 1 |
| `corpus/ipynb-handmade/03-plot-display-data.ipynb` | 2 | 2 |

One deliberate difference: **a key that escapes `DIR` is refused, not
sanitized.** The key comes out of somebody's zip, so a `..` component would
place a file anywhere the process can write. Renaming it instead invites a
second question about what the new name now collides with.

## Where ferrodoc behaves differently on purpose

- **Deterministic output.** The same AST always produces the same `.docx`
  bytes. Pandoc embeds timestamps, so its output differs run to run.
- **Bounded recursion.** Every recursive path is depth-limited and readers
  return `Err`; `unsafe` is forbidden workspace-wide. Two fuzz-found DOCX
  files that hang pandoc for 60 s are handled here in 12 ms and 3.5 s.
- **`data-` attribute symmetry.** A name the AST does not recognize is read
  without its `data-` prefix and written back with it, so a round trip
  cannot turn `data-onclick` into an event handler that runs. Pandoc's
  writer does the same.

## How to check any of this yourself

Everything above is reproducible from a clone with pandoc 3.8.2.1 on the
path. CI runs the same gates on every push, plus a 500,000-mutation fuzz
campaign with a fresh seed each run: `.github/workflows/ci.yml`.
