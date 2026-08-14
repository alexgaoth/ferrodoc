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
| pandoc JSON AST | yes | yes |
| plain text | — | yes |

Everything else pandoc supports — LaTeX, EPUB, RST, Org, presentations, the
other ~36 — is a declared non-goal. See `TODO.md`.

## Measured conformance

```sh
export PATH="$HOME/.cargo/bin:$HOME/.local/bin:$PATH"
cargo run -p ferrodoc-harness -- diff-spec      corpus/commonmark-spec-0.31.2.json --fail-under 100
cargo run -p ferrodoc-harness -- diff-ast       corpus --fail-under 100
cargo run -p ferrodoc-harness -- diff-html      corpus/commonmark-spec-0.31.2.json --fail-under 100
cargo run -p ferrodoc-harness -- diff-docx      corpus/docx --fail-under 96
cargo run -p ferrodoc-harness -- diff-write     corpus --fail-under 90
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
| `diff-write` | DOCX writer survives a round trip through pandoc | **10/11** |
| `diff-md` | markdown writer round-trips the document | **652/652** (pandoc: 593/652) |
| `diff-gfm` | GFM reader produces pandoc's AST | **654/655** |
| `diff-gfm-md` | GFM writer round-trips the document | **655/655** (pandoc: 589/655) |
| `diff-html-read` | HTML reader produces pandoc's AST | **631/657** |

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

### DOCX reader — 1 corpus document

`corpus/docx/spec-09.docx`: a list nested inside a table cell in a shape the
reader flattens.

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

### HTML reader — 26 of 657

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

## Resource limits worth knowing

Measured with `ferrodoc-harness bench-sizes` and `/usr/bin/time`; the full
table is in `TODO.md`. The one to plan around:

**`docx → markdown` peaks at ~1.12 GB of RSS for a 10 MB source document.**
The body is read one part at a time, so its XML tree never exists in full —
what is left is the AST, which is the answer, and the part being decompressed.
Streaming it cut peak RSS 2.7× and was ~12% *faster*, measured interleaved
against a baseline build; pandoc needs 12× more on the same input.

An image part is read only when the output can embed it, so a `.docx`
carrying a part that inflates a thousandfold costs 5 MB through
`-t markdown` and 840 MB through `-o out.docx`, which has to hold it.

## How to check any of this yourself

Everything above is reproducible from a clone with pandoc 3.8.2.1 on the
path. CI runs the same gates on every push, plus a 500,000-mutation fuzz
campaign with a fresh seed each run: `.github/workflows/ci.yml`.
