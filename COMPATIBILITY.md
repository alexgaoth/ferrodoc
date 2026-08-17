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
| pandoc JSON AST | yes | yes |
| plain text | — | yes |

Everything else pandoc supports — LaTeX, EPUB, RST, Org, presentations, the
rest of its ~40 — is not converted today. `TODO.md` says which are planned
and which are declared non-goals.

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
cargo run -p ferrodoc-harness -- diff-md        corpus/commonmark-spec-0.31.2.json --fail-under 100
cargo run -p ferrodoc-harness -- diff-gfm       corpus/gfm --fail-under 100
cargo run -p ferrodoc-harness -- diff-gfm       corpus/commonmark-spec-0.31.2.json --fail-under 99.8
cargo run -p ferrodoc-harness -- diff-gfm-md    corpus/gfm corpus/commonmark-spec-0.31.2.json --fail-under 100
cargo run -p ferrodoc-harness -- diff-html-read corpus/commonmark-spec-0.31.2.json corpus --fail-under 95
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
| `diff-md` | markdown writer round-trips the document | **652/652** (pandoc: 593/652) |
| `diff-gfm` | GFM reader produces pandoc's AST | **654/655** |
| `diff-gfm-md` | GFM writer round-trips the document | **655/655** (pandoc: 589/655) |
| `diff-html-read` | HTML reader produces pandoc's AST | **632/658** |

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

### Markdown writer — 4 limits of CommonMark itself

**Use `-t gfm`, not `-t markdown`, for anything with a table.** CommonMark
has no table syntax, so a table becomes one paragraph per cell there and
the row/column relationship is gone — not recoverable afterwards. GFM
output keeps it.

The other four, listed in `crates/ferrodoc-markdown/src/write.rs`: footnotes
and definition lists degrade to their content; emphasis directly inside
emphasis inside a word; two ordered lists in a row sharing a delimiter; an
unterminated raw HTML block swallowing the blank line after it.

### GFM — a chosen subset, and what a pipe table cannot hold

ferrodoc reads and writes the five extensions the **GFM specification**
defines: pipe tables, task list items, strikethrough, extended autolinks,
and tag filtering (which is off, because pandoc does not apply it either).
Heading identifiers are derived too, since pandoc's `gfm` always does.

Pandoc's `gfm` additionally bundles *pandoc* extensions the GFM
specification does not define, and those are **not** read: emoji
shortcodes, footnotes, alerts, `$math$`, and YAML metadata blocks. That
last one is the single `diff-gfm` mismatch — pandoc reads `---\n---\n` as
an empty metadata block where we read two thematic breaks.

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

### HTML reader — 26 of 658

Most are one cause: **ferrodoc parses to the HTML5 spec via `html5ever`,
pandoc parses with `tagsoup`, which does not.** On malformed markup the two
build different trees and no mapping reconciles them — a tag with no closing
`>`, an unclosed `<a>` whose formatting element is reconstructed after the
paragraph, a `<pre>` or `<div>` opened inside a `<tr>` and never closed.

Not all of them are that, and assuming so hid real bugs for a review round:
the `<![CDATA[…]]>` and `<style>` raw-text boundaries are a tokenizer
disagreement on input that is merely unusual, and `<a/>` self-closing syntax
sends the two parsers down different recovery paths.

Two deliberate divergences, both chosen on the same principle — *match
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
AST crate, or a public type that cannot serialize alone). `TODO.md` records
the measurement.

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
