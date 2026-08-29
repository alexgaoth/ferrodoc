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
| `diff-ast` | any pandoc JSON round-trips to an equal value | **13/13** |
| `diff-html` | HTML writer produces pandoc's HTML | **652/652** |
| `diff-docx` | DOCX reader produces pandoc's AST | **37/37** |
| `diff-docx` (LibreOffice) | ...on documents *another* writer produced | **7/8** |
| `diff-write` | DOCX writer survives a round trip through pandoc | **12/13** |
| `diff-odt` | ODT reader produces pandoc's AST | **32/34** |
| `diff-odt` (LibreOffice) | ...on documents *another* writer produced | **8/8** |
| `diff-odt-write` | ODT writer survives a round trip through pandoc | **13/13** |
| `diff-epub` | EPUB reader produces pandoc's AST | **11/12** |
| `diff-epub` (hand-authored) | ...on books in shapes pandoc's writer never emits | **3/3** |
| `diff-epub-write` | EPUB writer survives a round trip through pandoc | **10/13** |
| `diff-ipynb` | notebook reader produces pandoc's AST | **8/8** |
| `diff-ipynb-write` | notebook writer survives a round trip through pandoc | **8/8** |
| `diff-latex` | LaTeX writer round-trips the document | **1/13** (pandoc: 1/13) — **reported, not gated**; see below |
| `diff-rst` | RST writer round-trips the document | **3/13** (pandoc: 4/13) |
| `diff-md` | markdown writer round-trips the document | **652/652** (pandoc: 593/652) |
| `diff-gfm` | GFM reader produces pandoc's AST | **655/656** |
| `diff-gfm-md` | GFM writer round-trips the document | **656/656** (pandoc: 590/656) |
| `diff-pandoc-md` | pandoc-markdown reader produces pandoc's AST | **3/3** on its own fixtures, **14/20** over every markdown document, **498/652** over the spec |
| `diff-html-read` | HTML reader produces pandoc's AST | **641/661** |

The two round-trip gates are where ferrodoc is measurably *ahead*: pandoc's
own writers lose 59 of the same 652 documents in `commonmark` and 66 of 655
in `gfm`, at their best setting.

## Six places pandoc loses what this keeps

Every other page here measures how close this comes to pandoc. These six
go the other way, and they are collected because they were found one at a
time over two days of pointing the writers at documents nobody wrote to
be converted — and because "byte-identical to pandoc" would have meant
adopting each of them.

Every command below runs as printed, from the repository root, against
the pinned pandoc 3.8.2.1.

**1. Two adjacent bullet lists.** They need something between them or
they merge. Pandoc writes an HTML comment, and its own reader gives that
comment back as a `RawBlock` that was never in the document. This
switches the bullet from `-` to `*`, which adds no block.

    printf -- '- a\n\n* b\n' > /tmp/t.md
    pandoc /tmp/t.md -f commonmark -t commonmark | pandoc -f commonmark -t json |
      python3 -c 'import json,sys; print([b["t"] for b in json.load(sys.stdin)["blocks"]])'
    # ['BulletList', 'RawBlock', 'BulletList'] — two blocks in, three out

**2 and 3. A code block that opens a blockquote or a list item.** It
comes back a paragraph, and **both** of pandoc's markdown writers lose
it — the dialect one as well as `commonmark`, so taking the name
`markdown` did not change the answer.

    printf '> ```\n> code\n> ```\n' > /tmp/t.md
    for w in commonmark markdown; do
      pandoc /tmp/t.md -f commonmark -t $w | pandoc -f $w -t json |
        python3 -c 'import json,sys; print(json.load(sys.stdin)["blocks"][0]["c"][0]["t"])'
    done
    # Para
    # Para — a CodeBlock went in both times

This writer indents such a block instead, which costs a byte comparison
and keeps the document: `ferrodoc -t markdown` on the same input gives
`>     code`, and **pandoc's own reader** reads that back as the
`CodeBlock` that went in.

**5. A code block whose content ends in a newline.** Pandoc writes the
content `x\n` as plain `x` and reads it back as `x`, so the newline does
not survive its own round trip. This writes the blank line that carries
it, which is why the two differ on any document holding a fence left
unclosed at EOF.

    printf '{"pandoc-api-version":[1,23,1],"meta":{},"blocks":[{"t":"CodeBlock","c":[["",["text"],[]],"x\\n"]}]}' > /tmp/t.json
    pandoc /tmp/t.json -f json -t markdown | pandoc -f markdown -t json |
      python3 -c 'import json,sys; print(repr(json.load(sys.stdin)["blocks"][0]["c"][1]))'
    # 'x' — 'x\n' went in

**6. A task list item with nothing after the marker.** Pandoc writes a
bare `- ☐`, and **its own reader ends the list there**: a three-item
list comes back as a list of one and a list of two. This writes
`- [ ] `, which comes back whole, and takes the byte difference.
`corpus/gfm/task-list-runs.gfm` is where the two spellings meet, and
`diff-gfm-md` falls off 100 the moment this writer copies pandoc's.

    printf -- '- [ ] outer\n  - [x] inner\n- [ ] \n- [x] \n' > /tmp/a.gfm
    printf -- '- [ ] outer\n  - [x] inner\n- \342\230\220\n- \342\230\222\n' > /tmp/b.gfm
    for f in /tmp/a.gfm /tmp/b.gfm; do
      printf '%s: ' "$f"
      pandoc "$f" -f gfm -t json |
        python3 -c 'import json,sys; b=json.load(sys.stdin)["blocks"]; print(len(b), "list(s),", [len(x["c"]) for x in b], "items")'
    done
    # /tmp/a.gfm: 1 list(s), [3] items      <- this writer's spelling
    # /tmp/b.gfm: 2 list(s), [1, 2] items   <- pandoc's

The scale of it is on the gate's own first line: **pandoc round-trips
593 of the 652 CommonMark spec documents, and this writer round-trips
652.** `./target/release/ferrodoc-harness diff-md
corpus/commonmark-spec-0.31.2.json` prints both numbers together, which
is the only honest way to read either.

**4. A pipe inside a table cell.** Pandoc writes `` `x|y` `` unescaped,
and its own reader then splits the row at that pipe, taking the code span
with it.

    printf '| h |\n|---|\n| `x\\|y` |\n' > /tmp/t.md
    pandoc /tmp/t.md -f gfm -t gfm | pandoc -f gfm -t json | grep -c '"Code"'
    # 0 — the code span is gone

**5. Emphasis nested in strong, written to RST.** RST cannot nest inline
markup. Pandoc keeps the outer marker and drops the emphasis; this closes
the outer marker and reopens it, keeping all three marks.

    printf 'a **bold *emph* inside** b\n' > /tmp/t.md
    pandoc /tmp/t.md -f commonmark -t rst | pandoc -f rst -t json | grep -c Emph
    # 0

**6. A code span with a space at each end.** A reader strips one from
each, so the bare form pandoc writes gives back neither.

    printf '{"pandoc-api-version":[1,23,1],"meta":{},"blocks":[{"t":"Para","c":[{"t":"Code","c":[["",[],[]]," both "]}]}]}' > /tmp/t.json
    pandoc /tmp/t.json -f json -t gfm | pandoc -f gfm -t json | grep -o '" both "\|"both"'
    # "both" — the spaces are gone

**And one that is pandoc's reader rather than its writer.** The
`CommonMark` specification says a list is loose "if any of its
constituent list items directly contain two block-level elements with a
blank line between them". Pandoc reads the list below as **tight**, and
reads the same list loose once the indented code block is removed.

    printf -- '- first\n- second\n  - nested item:\n\n        code line\n\n  trailing para\n' > /tmp/t.md
    pandoc /tmp/t.md -f commonmark -t json |
      python3 -c 'import json,sys; b=json.load(sys.stdin)["blocks"][0]; print([[k["t"] for k in i] for i in b["c"]])'
    # [['Plain'], ['Plain', 'BulletList', 'Plain']] — Para here, in all three

None of these is a reason to prefer one tool: pandoc reads forty formats
and this reads eleven. They are here because a compatibility page that
only ever measured one direction would be quietly misleading about what
matching costs.

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
against either. On that measure it is **13/13 on the corpus**, and
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
| `corpus/epub` | pandoc's own output, 12 documents | **11/12** |
| `corpus/epub-handmade` | books in shapes pandoc's writer never emits | **3/3** |

There was a third row here, `corpus/epub-spec`, and **it has been
retired.** Each of its 22 files bundles 30 spec examples, so any one of
the HTML reader's 26 known divergences fails a whole document, and at 30
examples per file most files contain one. Its number moved when the HTML
reader moved and at no other time: it was the HTML reader's score under
another name, and printing it in a table headed *EPUB* invited exactly
the reading it did not support. A figure that needs a paragraph
explaining what it does not mean is better removed than explained.

The corpus stays and `verify.sh` still runs it, held at its level as a
**regression check** — a drop there is real even though the level is not
a claim. It is not a score, so it is not published as one.

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

`diff-epub-write` scores **10/13**, and the three that differ differ for the
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

### Markdown writer — the limits of CommonMark itself

**A construct CommonMark cannot spell is written as the raw HTML pandoc
writes for it**, and that now covers every one of them. A table used to
become **one paragraph per cell**, which destroyed the row-and-column
relationship the document was about; a `Div` used to lose its identifier,
its classes and every key-value silently. Both are raw HTML since
2026-08-25, byte-identical to pandoc's, and both round-trip. `-t gfm` is
still the better answer for a table — a real pipe table beats a `<table>`
in a markdown file — but the choice is about legibility now rather than
about losing the document.

One div is pandoc's own and is not written out: classes exactly
`sourceCode` with a single code block inside is the wrapper pandoc's
highlighter puts there, and it is unwrapped.

A **footnote** has no CommonMark spelling either, and degrades the way
pandoc degrades it: `[1]` where the reference stood, the body as ordinary
blocks after the document. Writing GFM's `[^1]:` into CommonMark output
produced a file this reader would not read back as a footnote.

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

**The first one costs something in the writer**, and it is worth naming
because the price is not obvious from the row. A reader that strikes on
one tilde means the writer has to escape a `~` that could pair with a
later one — pandoc, whose reader needs two, escapes only a doubled
tilde. The escape used to be every `~` in the document, so `~/path` and
`2 ~ 3` carried a backslash they did not need; it is now only a tilde
with another one after it in the same run, which is as narrow as the
decision allows. Reversing the decision would make `-t gfm` byte-
identical to pandoc on one more corpus document.

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

**The gate hands pandoc two flags, and one of them is visible.**
`diff-html` runs `pandoc -f commonmark -t html --syntax-highlighting=none
--wrap=none`, so the `652/652` is measured against pandoc with its
highlighter off. That is the right way to gate a writer — highlighting is a
rendering choice this project has not made, and wrapping is typesetting
rather than content — but it means the score does not cover a code block
with a language, and a reader running plain `pandoc -t html` will see the
difference immediately:

    printf '```rust\nfn main() {}\n```\n' | pandoc -f gfm -t html
    printf '```rust\nfn main() {}\n```\n' | pandoc -f gfm -t html --syntax-highlighting=none --wrap=none
    printf '```rust\nfn main() {}\n```\n' | ferrodoc -f gfm -t html

The first prints `<div class="sourceCode" id="cb1">` with a `kw`/`op` span
per token; the second and third both print
`<pre class="rust"><code>fn main() {}</code></pre>`, identical. The same
flag is passed by `diff-epub-write`, because pandoc's EPUB writer runs the
same highlighter. `README.md` states this beside the number as well.

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

**An EPUB title page is dropped whole**, element and content, wherever
`epub:type` contains `titlepage` — matched as a **substring**, so
`halftitlepage` counts too, and case-sensitively, so `Titlepage` does not.
**The element decides as much as the value**: eleven of them drop it
(`blockquote div dl figure hr main ol p pre section ul`) and everything
else keeps it, which was probed one element name at a time over the whole
HTML vocabulary because the set is not nameable — `<table>` and `<h1>`
keep it, `<hr>` loses it, and `<li>`, `<dd>`, `<dt>` and `<figcaption>`
only look dropped standing alone. A title page is the book's metadata set
as a page and pandoc declines to read the title twice.

**The wider count is `scripts/sweep-epub-xhtml.sh`, and it is 12 of 128.**
`diff-html-read` walks eight `corpus/*.html`; the corpus EPUBs hold 128
XHTML files written by pandoc's own writer, which is a far wider
vocabulary. That sweep stood at 77, then 46, and is 12 since 2026-08-25.
What is left is **two families, in both of which this reader is the one
following the standard**: an unclosed `<a>` with no slash, which HTML5
reconstructs into every following block and TagSoup does not, and the
`doc-noteref` divergence this reader keeps on purpose — pandoc answers an
unresolvable reference with a warning and an empty `Note`.

Four deliberate divergences, all chosen on the same principle — *match
pandoc wherever pandoc has a describable rule on well-formed input; diverge
only where matching would mean reproducing a parse failure*:

- An `<a href="…"></a>` with no text is **kept**. Dropping it would match
  pandoc on unclosed `<a>` tags but delete a well-formed empty anchor, which
  real pages use as jump targets.
- **An HTML comment is kept when reading an EPUB and dropped when
  reading a page.** That is where pandoc puts the line — its EPUB reader
  runs the HTML reader with `raw_html` enabled and `-f html` does not, so
  a comment survives one and not the other — and it is the only part of
  that extension this reader can reach, a comment being a real DOM node
  where a stray `</div>` is discarded before the tree exists. It is worth
  `diff-epub` 10/12 → **11/12**; `docs/divergences.md` has what the rest
  of `raw_html` would cost.
- **`<![CDATA[ … ]]>` is read as text and `<? … ?>` is dropped**, which is
  what pandoc does and what the XML says. An HTML5 tokenizer reads both as
  a bogus comment ending at the **first `>`** — inside the content whenever
  the content is code, so `<?php echo '>'; ?>` left `';` and `?>` behind as
  paragraphs and a block CDATA vanished entirely. Normalised in the source
  before parsing, beside the self-closing rewrite; a comment is copied
  through first so that one merely mentioning `<![CDATA[` is left alone.
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
    parse failure. Both halves, `Plain`+`Plain` against `Para`:

        html='<p>loose <input type="checkbox" /> in a paragraph</p>'
        printf '%s' "$html" | pandoc   -f html -t json
        printf '%s' "$html" | ferrodoc -f html -t json
  - In a `<td>` pandoc drops the element and leaves the block whole, and
    ferrodoc matches it byte for byte — one `Plain` either way, inside one
    `Table` on both sides:

        html='<table><tr><td><input type="checkbox" checked="" />plain cell text</td></tr></table>'
        printf '%s' "$html" | pandoc   -f html -t json
        printf '%s' "$html" | ferrodoc -f html -t json
  - In an `<h2>` pandoc loses the `Header` **entirely**, emitting two
    `Plain`s rather than a broken heading, where ferrodoc keeps `Header 2`:

        html='<h2>head <input type="checkbox" /> box</h2>'
        printf '%s' "$html" | pandoc   -f html -t json
        printf '%s' "$html" | ferrodoc -f html -t json

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
floor while exactly one corpus document round-tripped;
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
harness prints `pandoc round-trips 1/13` beside our score.

### `--reference-doc` — the styles, and only the styles

The single most common reason a team cannot switch converters: the house
styles live in a `.docx` somebody made in Word.

**For ODT it is `styles.xml`, the one part that is what a style is
there**; for DOCX it is two, `word/styles.xml` and `word/numbering.xml`,
which is what a list style is.
Copying the rest of the reference package — theme, fonts, settings —
would mean either declaring those parts in a `[Content_Types].xml` this
writer did not build, or shipping parts nothing declares, and both are
ways to write a `.docx` Word offers to repair on open.

```console
$ ferrodoc report.md -t docx --reference-doc=house.docx -o out.docx
$ unzip -p out.docx word/styles.xml | cmp - <(unzip -p house.docx word/styles.xml)
```

A reference that is not a `.docx`, or has no styles part, is **named**
rather than quietly falling back to the built-in styles: a team whose
branding vanished would find out downstream.

With this, **every one of the 48 command lines in `dropin/` runs** —
none is refused for a flag this build does not have.

### `--resource-path` and `--data-dir`

`--resource-path DIR:DIR` is searched **after** the document's own
directory, which is pandoc's order — a picture beside the document still
wins. Without it neither binary embeds a picture the document names but
does not sit beside; with it both do.

`--data-dir DIR` is where `templates/default.html5` overrides the default
template, and where a `--template` named rather than pathed is found.
**That file name is pandoc's and was measured**: a data directory holding
`templates/html5.html` is ignored by pandoc, so it is ignored here too.

### `--ascii`, `--id-prefix`, `--metadata-file`

All three in `./scripts/flags.sh`, byte for byte.

**`--ascii` is HTML's here.** Pandoc escapes a non-ASCII character
differently in every writer — `&#xE9;` in HTML, `&eacute;` in markdown
and plain, `\'{e}` in LaTeX, and not at all in RST — so this build has
the HTML one and **refuses the flag by name** for the others rather than
inventing a spelling. Where it applies it is a whole-output pass, which
is what pandoc does: text, attributes, URLs, identifiers and raw HTML
alike, measured on a document with a `café` in each.

**`--id-prefix` rewrites internal links too.** `[to A](#a)` becomes
`href="#p-a"` and still points at the heading it named; prefixing the
targets alone would break every anchor in the document, which is the
opposite of what the flag is for. The contents' entry ids carry the
prefix **before** the `toc-` — `id="p-toc-x"` beside `href="#p-x"` — and
the `<nav>` gets it too.

**`--metadata-file`** reads the same flat `key: value` subset a
`--defaults` file does, and refuses a key it cannot read by name.
Metadata quietly dropped is a title that never appears in the output to
be noticed.

### Extension syntax — accepted where it asks for nothing

`-f markdown+footnotes-pipe_tables` is pandoc's spelling, and what is
accepted here is what the named dialect **already does**: a request that
asks for nothing new is the same conversion. Anything else names what it
cannot do, and where the extension does exist:

```console
$ ferrodoc x.md -f gfm+footnotes -t html          # accepted: gfm reads them
$ ferrodoc x.md -f markdown+footnotes -t html
ferrodoc: `markdown` does not read `footnotes` here, and this build cannot turn one on: gfm and pandoc_markdown read it
$ ferrodoc x.md -f gfm-pipe_tables -t html
ferrodoc: `gfm` reads `pipe_tables` here and this build cannot turn it off: pandoc_markdown reads it
$ ferrodoc x.md -f gfm+fotnotes -t html
ferrodoc: no extension named "fotnotes"; `pandoc --list-extensions` has the names
```

The name is checked **before** asking whether the request is a no-op:
`-nothing` is a typo rather than "an extension this dialect already
lacks", and treating it as the latter accepted it silently.

What each dialect implements is in `Format::extensions`, under pandoc's
names, and it is deliberately short — claiming one that is not
implemented would make `+ext` a silent no-op, which is the failure the
list exists to prevent.

### Text shaping — `--shift-heading-level-by`, `--strip-comments`, `--eol`

All three in `./scripts/flags.sh`, byte for byte over every document in
`corpus/`. Three rules were measured rather than assumed, and each was
wrong the first time:

- **the heading that becomes the title is the one the shift takes to
  exactly level 0**, whatever level it started at. `corpus/headings-deep.md`
  opens at `##`, so `--shift-heading-level-by=-2` makes *that* the title —
  and it **overwrites** a title the document already had. A heading pushed
  below level 0 becomes a paragraph;
- **`--strip-comments` cuts the comment out of the text and keeps the
  block.** A raw block whose source was `<!-- c -->\n` comes back as
  `"\n"`; removing the block instead loses a line of output. Inline
  comments go the same way;
- **an unterminated `<!--` is left alone.** A browser swallows the rest of
  the document with one, and assuming that dropped two list items in
  `corpus/truncation-cases.md` that pandoc keeps.

`--eol` rewrites text output only: a `.docx` is a zip, and rewriting bytes
inside one would corrupt it.

### Diagnostics — `--quiet`, `--fail-if-warnings`, `--verbose`

Matched against the binary, exit codes and stderr included:

```console
$ ferrodoc x.md -f markdown_github -t html --fail-if-warnings > /dev/null
[WARNING] Deprecated: markdown_github. Use gfm instead.
Failing because there were warnings.
$ echo $?
3
```

`--quiet` silences every warning and `--fail-if-warnings` exits **3**
with pandoc's own line, both position-independent: `-f markdown_github
--quiet` warns while the *first* flag is parsed, so the flags are read
before anything else is.

**`--verbose` is accepted and adds nothing.** Pandoc's writes `[INFO]`
lines about what it is doing; there is nothing here that would say one.
Refusing the flag would fail a command line pandoc runs happily, so the
missing lines are a difference recorded here rather than an error — the
one place in the CLI where accepting-and-doing-less is the better answer,
because no document byte depends on it.

### Standalone pages — pandoc's own template, byte for byte

`-s` output used to be one fixed page shape against pandoc's template
language, and it was where "indistinguishable" was furthest away: **176
lines differed** on a document whose fragment matched exactly. 174 of
them were pandoc's default template and its default stylesheet.

Both are now vendored in `crates/ferrodoc-html/templates/`, under the
**BSD-3 option** pandoc's `COPYRIGHT` offers for everything in
`data/templates`, and rendered through a subset of pandoc's template
language: `$var$`, `$if$`/`$else$`/`$endif$`, `$for$`/`$sep$`/`$endfor$`
and `$partial()$`. Anything outside it — pipes, nested fields — is
**refused by name** rather than left as a hole in the page.

`./scripts/flags.sh` runs every document in `corpus/` through every
flag combination that shapes output and requires all of them:

```console
$ ./scripts/flags.sh
224/224 flag combinations byte-identical
```

That covers `-s`, `--toc`, `--toc-depth`, `--css`, `-V`, `-M`,
`--metadata-file`, `--title-prefix`, `-H`, `-B`, `-A` and a third-party
`--template`. Four rules in it were measured rather than assumed:
**`--css` turns pandoc's default stylesheet off** (145 lines of every
`-s -c` comparison); **`--toc` on a document with no heading writes
nothing at all**, not an empty `<nav>`; **`-H`, `-B` and `-A` imply
`--standalone`** and nothing else here does; and **a template reads the
document's metadata as variables**, so `-M linkcolor="#007bff"` colours
the links, with `-V` beating `-M` where both name one.

What `-s` still differs on is the **highlighting stylesheet**, and it is
now a difference in colours rather than an absence of them. A page with a
highlighted code block makes pandoc add 65 lines of CSS from
skylighting's style set — not from `data/templates`, so the BSD carve-out
this repository vendors the template under does not reach it. Since
2026-08-26 this writes **its own** 47 lines instead, under this project's
licence, in the same place: appended to the same `<style>` element, after
the default stylesheet, and only when the document holds a code block in
a language the highlighter knows. That is exactly pandoc's condition,
probed one shape at a time.

The trade is stated rather than buried. A standalone page **with no
highlighted code is byte-identical to pandoc's**, as it always was. One
*with* highlighted code differed by 65 lines when the colours were
missing and differs by **112** now that both sides have their own — and
renders as highlighted code instead of as uncoloured spans.
`samples/15-markdown-to-html-page` is that difference.

Nine structural declarations coincide with pandoc's — `white-space:
pre`, `display: inline-block`, the `:empty` height, `color: inherit`,
`overflow: auto` and the two print rules. A `<span>` per line renders as
lines only one way. Nothing with a colour in it is shared, and
`crates/ferrodoc-html/styles/highlight.css` says so at the top.

### Syntax highlighting — C, Python, bash, Ruby, Rust

**Code is highlighted, in pandoc's shape, for the languages named here
and no others.** A language not on the list degrades to exactly what this
writer emitted before there was a highlighter — `<pre class="whatever">
<code>` — so a short list costs nothing but colour.

| language | names accepted | curated gate | files written for somebody else | lines of those files |
|---|---|---|---|---|
| C | `c` | **2/2** | 28/40 system headers | **98%** of 11,154 |
| Python | `python`, `python3`, `py` | **4/4** | 21/40 standard library | **98%** of 28,813 |
| bash | `bash`, `sh`, `shell`, `zsh`, `ksh` | **20/20**, 2,065 lines | 7/40 scripts in `/usr/bin` | **96%** of 17,018 |
| Ruby | `ruby`, `rb` | — | 16/40 standard library | **96%** of 26,777 |
| Rust | `rust`, `rs` | — | 15/40 this repository | **98%** of 31,463 |

**The last two columns measure the same thing and disagree, and the
disagreement is the useful part.** The file column is whole-file byte
identity: a 3,000-line file earns its point only with 3,000 consecutive
correct lines, so a highlighter that is right about 95% of lines can score
3/40 and read as broken. The line column says how far off it actually is.
Read the file score for *is this finished* — nothing here is — and the
line score for *how far*.

**The two columns are the point of this section.** For weeks only the
first existed, and every language stood at 26/26 in it. Then the same
highlighters were pointed at files nobody wrote for this repository —
`/usr/include`, the Python standard library, the scripts in `/usr/bin` —
and C matched pandoc on **1 header in 40**. Python managed 6, bash 12.
The gate could not have said so: a gate cannot fail on a construct its
corpus lacks, and this repository's 2,650 lines of C, Python and shell
simply do not contain a multi-line `#define`, or a blank line inside a
licence header, or `f"{x!r}"`.

That blind spot is named in three other places in this file. It was
sitting under the strongest claim the project makes, and it took eight
rules to close most of it — fourteen now, every one read back off the
pinned binary rather than reasoned about:

- an empty line inside a block comment carries **no span**; we emitted
  `<span class="co"></span>`, which is every licence header in `/usr/include`
- **on a directive line nothing is plain**: the spacing before a trailing
  comment, a macro's parameter names, the body it expands to, all `pp`.
  This one rule took C from 7/40 to 23/40
- a directive continued with `\` runs onto the next line, and that line
  is tokenized **like a `#define`'s value** rather than taken flat:
  operators are `op` and numbers `dv`, with everything else `pp`. Reading
  it as one `pp` run was the first attempt and was measurably wrong —
  `#define A(x) \` then `((x) ? 1 : 0)` carries five operator spans. The
  `\` itself is an `op`; `##` inside a directive is an `op` too, though
  the `#` that opened it is a `pp`
- bash's **doubled** brackets are `kw` and its single ones `bu`
- python's dunders: of 96 probed, 68 are `fu` wherever they stand —
  `x.__init__` too — while `__name__` and `__file__` are `va`, and 29
  including `__dict__` and `__doc__` carry no class at all
- `match` and `case` are **soft** keywords, `cf` only when a space and an
  operand follow; `f(match="a")` is an ordinary name. This repository's
  own test file caught the first attempt, which is the curated gate doing
  the one job it is still good at
- an f-string's `!r}` and `:>3}` belong to the placeholder, not to the
  expression inside it
- the `%…` conversion letters **differ by language**: C takes `%a` and
  `%b`, python takes `%r` and `%(name)s`, and each set was probed `a` to
  `z` and `A` to `Z`
- **a comment carries alert words, in every language alike**: `TODO`,
  `FIXME`, `NOTE`, `BUG`, `HACK`, `WARNING`, `NOTICE`, `DEPRECATED`,
  `ALERT`, `ATTENTION`, `CAUTION`, `DANGER`, `SECURITY` and `###` — but
  **not** `XXX`, `REVIEW`, `OPTIMIZE`, `IMPORTANT`, `TIP` or `ERROR`,
  which is why the list was probed word by word. They match on word
  boundaries where `#` and `_` count as *word* characters, so `# TODO` is
  an alert and `#TODO` is not, and `# ###` is one where `# ####` is not
- ruby's `..` and `...` are ranges, not two attribute dots
- **a ruby percent literal** — `%w[a b]`, `%q(x)`, `%r{re}` — is `ot`
  around a body whose class the letter decides: `q`/`w`/`i` give a `vs`,
  their capitals and `r` an `st`, `%s` a `wa`, `%x` an `in`. A `%`
  followed by a letter or a space is the modulo operator
- the inside of ruby's `#{ … }` is **code**: `"#{@addr}"` carries an `ot`
- **a ruby `def` signature suppresses exactly one symbol** and then
  stops. `def cp(a, b: 1, c: 2)` gives `b` an `op` and `c` a `wa`;
  `def f; g(a: 1); end` gives `a` a `wa` because the `;` ended the
  signature; `def self.cp(a, noop: nil)` gives `noop` a `wa` because
  `self` is not a method name. The simpler rule — *a `def` line has no
  symbols* — was written first, measured, and was worse
- **Rust's `::` binds to whichever side is unknown.** A word in the
  vocabulary keeps its own class and only the `::` is `pp` —
  `dt|Vec` then `pp|::` — while an unknown one joins the run, so
  `std::io::Result` is `pp|std::io::` then `dt|Result`. Every probe
  written by hand had used a made-up name and missed this; a real file
  found it. `dt` and `cn` are **lists**, not shapes: `MyOwnType` and
  `Trait` carry no class at all.
- Rust's block comments **nest** (`/* a /* b */ c */` is one), `'a'` is a
  `ch` and `'a` an `ot` told apart only by the closing quote, and `#`,
  `(`, `)`, `[`, `]` are **not** operators — which no other language here
  agrees with.
- **`sourceCode` is written once.** Reading pandoc's own HTML back gives a
  code block whose classes contain `sourceCode` — both readers agree, the
  ASTs are identical — and this writer added another, so `html -> html`
  emitted `class="sourceCode sourceCode bash"`. Only that class
  deduplicates: a `numberSource` in the block's classes really is written
  twice, which was probed rather than assumed.
- **a bash `$` that names nothing is not a variable**: `$1`, `$a`, `$?`,
  `$$` and `$-` are, and a `$` before a space, a `"` or the end of the
  line is an ordinary character — `echo "a$"` is one `st` run, not a
  string broken around a bare sigil
- Rust's fourteen `c_*` FFI types are `dt`; `CStr`, `CString`, `OsStr`
  and `OsString` carry no class, which is not the grouping anyone would
  guess
- **a bash array subscript is an expression**: `${a[$i]}` and `a[$i]=1`
  put a `va` between two `op`, a numeric index is a `dv`, a name or a sum
  carries nothing, and only `[@]` and `[*]` are a single operator
- **a backtick substitution was not read at all.** `` x=`date` `` lost
  the whole run; both ticks are `kw` with a command between them, and a
  backtick ends the word it meets
- **`function f()` names a function, and the space belongs to the name**:
  `kw|function` then `fu| f()`, parens and all — only at command
  position, so `echo function` stays plain
- **a bare `(( … ))` evaluates arithmetic**, on the same rules as
  `$(( … ))` but closing on a `kw`. Without it `if (( verbose >= t ))`
  called its variables `ex`
- **`&>` and `&>>` redirect and do not end the command.** Reading the `&`
  as the separator put the scanner back at command position and made
  `/dev/null` an `ex`
- **`cat << EOF` opens a heredoc as surely as `cat <<EOF`**, and the space
  between belongs to the operator — `op|<< EOF`. Without it the heredoc
  never opened and its prose was read as commands
- **inside a `name=( … )` array, `[key]=` is a subscript and not a glob**:
  the brackets are `op`, a quoted key keeps its `st`, a bare one carries
  nothing, and the `=` after the `]` is a **`va`**, not the `op` an
  assignment gets anywhere else
- `.` — the source builtin — is a `bu`
- **a ruby here-document**: `<<~`, `<<-` and `<<` are `op`, the tag —
  quotes and all — is a `cf`, the body is a `do`, and the indentation
  before a closing tag is `do` while the tag itself is `cf`. `#{ … }`
  interpolates unless the tag was quoted. **`<<TAG` opens one only where
  a value is expected**: `x = <<eos` does and `a << b` shifts, which is
  the same question the regexp asks
- a leading `0` before more digits is **octal** and a `bn`: `0700` is a
  file mode, not seven hundred
- **a `:` that begins a symbol ends an operator run**: `&:to_s` is `op|&`
  then `wa|:to_s`, not one `op|&:`

`./scripts/real-world.sh` is the second column, and the figures in it were
taken on 2026-08-26. It **reports and does not gate**: its corpus is
whatever the machine happens to hold, so the numbers do not compare
between machines — and the denominators drift even on one machine, as
this sweep found 22 shell scripts in `/usr/bin` one hour and 24 the next
because packages had been installed in between. What it is, is
re-checkable, which is more than the claim it replaces.

`./scripts/highlight.sh` is the first column, and the inputs are the C
binding's own example and header and every Python and shell file in the
tree — 2,650 lines that exist in this repository for other reasons and
that nobody wrote to be highlighted, this project's own harness among
them. That was chosen over the spec because only three of its 652
examples hold a fence in a language pandoc knows, nine lines between
them, and a highlighter written to pass those would be fitted to the
fixtures. The lesson of the second column is that *any* fixed corpus is
fitted to eventually; the defence is to keep pointing the thing at code
it has never seen.

**A python raw string is a regular expression to pandoc**, and this
highlighter now reads it as one. It was the largest gap on the list; it
took twenty-one probed constructs and four more rules that only real
files could have found.

The sub-language, construct by construct: the prefix and quotes are `vs`;
`\d` and its letter friends are `dv` while `\1` and `\.` are `ch`; a
character class is `pp` with its escapes keeping their own classes, and a
`]` immediately after `[` or `[^` is literal; `.`, `^` and `$` are `dv`;
`|` is `cf`; `+`, `*`, `?` and a **numeric** `{2,3}` are `op` while `{a}`
is three ordinary characters; and a group's parentheses take their class
from what opens it — `kw` plainly, **nothing at all** for `(?: … )`, `ex`
for a lookahead, `kw` with a `fu` name for `(?P<n> … )`, and one span of
its own for `(?i)`, `(?#…)` and `(?P=n)`.

Four rules beside it, each found by a real file rather than by reasoning:

- **a lowercase `r` reads the body; a capital `R` does not.** `r"\d"`
  carries a `dv` and `R"\d"` is one flat `vs`
- **a triple-quoted raw string is a *verbose* regexp**, where `#`
  comments to the end of the line — and a single-quoted one is not.
  `r"a#b"` is flat and `r'''a#b'''` carries a `co`. `doctest.py`'s
  `_EXAMPLE_RE` is twenty lines of one, and carrying that state across
  lines is what the rule costs
- **a raw string can still be a docstring**, and then it is prose rather
  than a pattern — `doctest.py` opens with one
- **a docstring holds no conversions, no placeholders and no alert
  words.** The first line of one already knew about conversions, because
  the scanner asks whether the class is `st`; every line after it did
  not, and `difflib.py` is full of `%d` inside a triple-quoted run. A
  docstring now carries a class of its own for exactly this reason: it
  renders `co` and it is not a comment

**Python's escapes are exact, and an unknown one is an `er`.** Probed `a`
to `z`, `A` to `Z` and `0` to `9`, then form by form: `\a \b \f \n \r \t
\v` are escapes and every other bare letter is not; one to three octal
digits are; `\x` needs exactly two hex digits, `\u` four and `\U` eight,
and each is an error without them; `\N{…}` is one whole escape; and of
the punctuation only `\\`, `\'` and `\"` count. `"\d"` is an `er` for the
**backslash alone** and then string — calling it a `ch` was 89 wrong
spans in forty standard-library files.

**What else is still wrong, measured rather than supposed.** Nothing on
the list that opened this cycle is left: the python regexps, the bash
array subscripts and backticks, and the here-documents in both languages
are all read now. What remains is a long tail with no single large item
in it — the figures above are what is left of it, and the sweep that
produced them is the thing to run next. Every language here is listed with its numbers beside it rather
than with a claim, because not one of them is finished — and listing them
is still right, because dropping a language colours nothing where keeping
it colours 94 to 98 percent of every line correctly.

Every rule was read off `pandoc -f commonmark -t html`, and the ones that
would have been guessed wrong are worth naming: `NULL`, `printf` and
`malloc` are **not** classed; adjacent operator characters are one span
(`);` and `};`); a numeric suffix is a `bu` beside the number rather than
part of it; a `printf` conversion inside a string is a `sc`, as an escape
is, and the two merge when adjacent; `#include` splits into a `pp` and an
`im`; and a comment ends a directive rather than belonging to it.

The wrapper is pandoc's too: `<div class="sourceCode" id="cbN">` — with
the block's key-value attributes on **that div**, and `class` before `id`,
which is not the order the writer uses anywhere else — then `<pre>`
carrying the block's classes untouched, `<code>` carrying the syntax's
**canonical** name, and one `<span id="cbN-M">` per line with an anchor
in it. `cbN` numbers every code block in the document, highlighted or
not; an explicit identifier replaces it; `.numberLines` adds
`numberSource` and takes `aria-hidden`/`tabindex` off the anchors.

**`--no-highlight` and `--syntax-highlighting=none` turn it off.** Any
other style value is refused by name — a style that silently does nothing
is worse than one that says so. Every gate that mutes pandoc's
highlighting mutes this one too, because muting one side would compare
two different questions.

Python's rules are a different order of hair from C's, and every one was
measured: a string that opens a line **at bracket depth zero** is a
docstring and is coloured as a comment, so `__all__ = [` followed by
strings is not one; `{…}` in an ordinary string is one `sc` while an
f-string's braces are `sc` with **code** between them; an attribute dot
inside those braces is an `sc` but only at their top level, so `{a.b}`
has one and `{len(a.b)}` does not; `{x:.2f}`'s conversion belongs to the
brace that closes it; an escape is a `ch` where C's is an `sc`; and a
backslash ending a line inside a string is an `op`. The keyword table was
probed over **python's own vocabulary** — `dir(builtins)` plus
`keyword.kwlist`, 211 names — because choosing the probe set by hand is
how `file` came to be missing on the first attempt, and only a real file
caught it.

All four cost **80.4 KB, 1.16% of the binary** — most of it bash's table
of command names — and it is a cargo feature (`highlight`, on by default)
so a trimmed build can drop it.

**bash is a different kind of job, and that is what made it worth
doing.** Its classes are **positional** rather than lexical: the same
word is `fu` at the start of a command and plain text one word later, so
the scanner tracks *where in a command it stands* rather than what it is
reading. What that costs is a list of rules no amount of reasoning would
have produced, every one of them read off the pinned binary:

- The first word is `fu` if pandoc knows the command, `bu` for a shell
  builtin, `ex` otherwise; `;`, `|`, `&&`, `{` and `}` put the scanner
  back at command position, and so do `if`, `then`, `do` and `!` — but
  **not `for`**.
- A bare number is plain text. `exit 1` leaves the `1` uncoloured; only
  `return`'s number and a number beside a redirection (`2>`, `>&2`) are
  `dv`, and only inside `$(( … ))` do the ordinary rules for numbers and
  operators apply.
- `LANG=C sort file` names a variable, gives its value as plain text, and
  is **still at command position** for `sort`. `env FOO=1 ./x` on a
  continuation line is plain throughout, because a line ending in `\`
  does not start a command.
- A `case` label is a pattern: `--verbose` is an `ss` there and an `ex`
  everywhere else, `*` in it is a `pp`, and the `)` closing it is a `kw`
  while the `)` closing a `$( … )` is a `va`. Telling those apart across
  lines needs the open-parenthesis and open-substitution counts kept in
  the scanner's state.
- `${name}` is five different things depending on its punctuation, and
  `${x/pat/sub}` colours the pattern `ss` and the replacement plain.
  `${2:?message}`'s operator is `:?` and stops there; the message is not
  part of it.
- A here-document is `st` until its delimiter returns, and expands
  variables only when the delimiter was unquoted. `$'…'` is a string
  whose escapes are `dt`; inside `"…"` only `\"`, `\$`, `\\` and
  `` \` `` are escapes, so `"\t"` is all string.

The command table was first probed **one word at a time** — 204 rows —
after a batched probe came back misaligned and would have coloured 69
words wrongly. It has since been re-probed against an external list
rather than a remembered one: **every name in `/usr/bin`, 3,130 of
them**, read back 200 to a document with one word per line, which the
misalignment cannot survive because each line is read back by its own
number. 279 came back `fu`, 21 `bu`, and the other 2,830 carry no class
at all. `samples/06-markdown-to-html` is one bash block and nothing
else, and it is now byte-identical to pandoc's.

**There is a second oracle, and it had never been used.** Pandoc *writes*
LaTeX, RST and AsciiDoc from the same AST, so the bytes can be compared
directly without asking its reader to survive anything.
`./scripts/writers.sh` does that for every text writer, in a second, and
`verify.sh` **gates it**:

| writer | byte-identical to pandoc | floor |
|---|---|---|
| `html` | **38/40** | 38 |
| `rst` | 34/40 | 34 |
| `plain` | **38/40** | 38 |
| `latex` | 36/40 | 36 |
| `asciidoc` | **38/40** | 38 |
| `gfm` | 28/40 | 28 |
| `commonmark` | 29/40 | 29 |
| `markdown` | **29/40** | 29 |

**A fill must not write a document that reads back as another one**, and
until 2026-08-27 these writers did. Given a paragraph whose greedy fill
left a bare `+` at the start of a line, the markdown writers wrote a
document that came back as a `Para` **and a `BulletList`** — because
`+ 8 = 40, …` opening a line is a bullet item.

Pandoc breaks one word early to avoid it, and the set it avoids was
measured character by character rather than read off the spec: `+`, `-`,
`*` and `>`; `1.` and `1)` but **not `2.`, `12.` or `2)`**, because an
ordered list may only interrupt a paragraph when it starts at one. `#`,
`=`, `~` and `%` are allowed, which is not what the spec would suggest.
The rule applies to `plain`, `commonmark`, `gfm` and `markdown` and
**not** to `html`, `latex` or `rst`, where pandoc fills to the full
column — those outputs are never read back as markdown.

It was found by prose written into `ROADMAP.md`: this repository's own
files are part of the writer corpus, so a paragraph about the size of an
`Inline` took `plain` from 38/40 to 37/40 the moment it was committed.

**Whether it was one bug or a class was then checked, and it is one bug.**
Every document in the corpus and every `.md` in this repository was
written to `commonmark` and read back, and the resulting AST compared
with the original:

```
self-round-trip at --wrap=none:  pandoc 2/19   ours 2/19
```

and **on the same documents, one for one**. A markdown round trip is
lossy for reasons that have nothing to do with filling — a heading gains
an identifier, raw HTML is re-read as something else — and this writer is
lossy in exactly the places pandoc is. The stranded bullet was the
outlier, not the first of many. Recorded because a negative result that
closes a line of enquiry is worth as much as the fix that opened it.

Twenty documents, **each written twice** — as the document falls, and
filled to 72 columns, which is pandoc's own default. Nothing measured the
fill until 2026-08-26, and the RST writer had been treating an inline
span as one unbreakable word: `**a long run**` was pushed whole onto the
next line and overran the column on four of the eight prose documents.
The twenty are the eight in `corpus/` read as CommonMark, the four in
`corpus/gfm/` read as GFM, and **this repository's own eight** — README,
ROADMAP, COMPATIBILITY, `docs/` and `samples/README.md`, 4,440 lines that
exist to be read rather than to be converted. Those eight were added on
2026-08-25 and scored **asciidoc 0/8 and rst 1/8** against writers that
were at 11/12 and 12/12 on the fixtures; five real bugs came out of it,
and one of them was a broken fence in `README.md` itself. **The second four are there because the
first eight cannot express what the writers are worst at** — CommonMark
has no table, no task list and no footnote — and the first thing the GFM
pass found was that the HTML writer, at `diff-html` 652/652, **dropped
every footnote**: the reference, the body and the whole `<section
id="footnotes">`, silently, on every document that had one.

**Each floor is the score that writer reached**, because every point
below one is a document that used to be byte-identical and is not any
more. A floor chosen after seeing a score is not a floor, which is why
this printed a number and gated nothing while the numbers were low; it is
a contract now that five of the seven are at the whole corpus or one
document from it.

**The dialect writer landed 2026-08-27 and took the name `markdown` on
2026-08-28**, scoring **23/40** against `pandoc -t markdown` where the
`CommonMark` writer scored 6. It exists because `-f markdown` began
reading pandoc's dialect and `-t markdown` kept writing `CommonMark` —
an asymmetry worth closing rather than explaining, and it is closed in
both directions now. `-t commonmark` is how you ask for the older
meaning, and `-t pandoc_markdown` still spells the dialect explicitly.

Its text rules, each probed by handing pandoc a JSON AST and reading the
bytes back: `—` `–` `…` are written **unescaped** as `---` `--` `...`,
while a literal `'`, `"`, `~` or `|` is escaped, and a `-` is escaped
whenever another follows it. Those last two decisions have to be made in
**one pass** — substituting the em-dash first and escaping afterwards
turned every one of them into `\-\--`, which is what the first attempt
did and what took the score from 13 to 9 before the diff said why. A
`LineBreak` is a trailing `\` rather than two spaces, and raw inline
content is `` `<em>`{=html} ``, which `CommonMark` has no way to say.

**It scored 15 until `--wrap=auto` was found not to reach it at all.**
`Format::wrapping` still called the dialect `NotText`, left over from
when it was read-only, so no column count was ever passed and the writer
never filled. What made that hard to see is that the output *looked*
filled: with no fill the source's own soft breaks come through, and every
document in this corpus is already wrapped at 72. The tell was a line of
**77 columns** in a 72-column run — a fill that overshoots is not a fill.

**Four constructs it writes that no gate can reach.** A heading's
attributes, a footnote, a definition list and a fenced div are what
`CommonMark` cannot express — so `writers.sh`, which reads its corpus as
`CommonMark`, gives this writer no `Note` to get wrong. That is the
corpus blind spot in the one place it is structural rather than
accidental, and the four are held by unit tests instead. The definition
list carries the **loose/tight** distinction a bullet list does: a
definition whose first block is a `Para` takes a blank line and a `Plain`
does not.

`commonmark` and `markdown` are **the same ferrodoc writer** measured
against two different pandoc writers, and the pair is the honest way to
report it. `-t markdown` is CommonMark here and pandoc's own dialect
there, so the `markdown` row measures the dialect gap on the writer side
and moves when `pandoc_markdown` does rather than when the writer does —
ROADMAP card D4.4. The `commonmark` row asks the writer's own question,
and it went 3 to 8 the day it was asked separately: tables, divs and
footnotes were being lost or mis-spelled where no gate was looking.

**All four misses left are pandoc losing information, each checked by
round-tripping pandoc's own output through pandoc:**

- A code block that opens a blockquote or a list item comes back from
  pandoc as a **paragraph**. The list-item half was this writer's bug
  too until 2026-08-25 — four spaces past a marker padded to `3.  ` read
  back one space wider than they were written, and
  `corpus/truncation-cases.md` is the document that showed it.
- Two adjacent bullet lists need something between them or they merge.
  Pandoc writes `<!-- -->`; this switches the bullet from `-` to `*`.
  Pandoc's separator **is a `RawBlock` its own reader gives back**, so
  `pandoc -t commonmark` on `- a` / `* b` returns a three-block document
  where a two-block one went in. The bullet switch returns the two.

**The EPUB writer colours code as of 2026-08-27.** It did not while
there was no highlighter — pandoc's does, and scoring against skylighting
would have measured skylighting — and `diff-epub-write` muted pandoc's
side to match. **That reasoning expired the moment this project had a
highlighter of its own**, and muting a feature you *have* is the trap
named further up this page. The gate no longer mutes it, `epubcheck`
accepts every book, and the round trip moved sharply: eight of sixteen
documents are now at the `dc:title` baseline where four were, and
`docs/divergences.md` went from **193 differing lines to 9**.

Turning it on took the EPUB writer gate from 10/13 to 9/13 before it
took it back, and the reason is worth keeping: **the harness was linking
a `ferrodoc-html` with the highlighter compiled out.** The workspace
dependency is `default-features = false`, nothing in the harness's graph
turned `highlight` back on, and so a gate that had always agreed with the
CLI was quietly measuring a different program. A trimmed build is a
different program — this page says so about the *shipped* trimmed build,
and it is just as true of a test harness.

**The binary writers have a judge of their own since 2026-08-26.** A
DOCX, an ODT, an EPUB and a notebook have no bytes worth comparing — the
two tools zip different files in a different order — so
`scripts/roundtrip.sh` writes the same AST with both and requires what
pandoc reads back out of them to agree. It stands at **odt 16/16, docx
14/16, ipynb 11/16 and epub 0/16**, over `corpus/*.md` and this
repository's own prose, and **every one of the misses is a divergence
recorded on this page**: the loose/tight list reading, the code block
pandoc loses when it opens a container, a quotation the DOCX round trip
renests, and `dc:title`.

**Two differences sit behind the epub row, and both are decided.** Nine
differing lines is the `dc:title` baseline; a document above it has a
relative link to a file the book does not contain, which this writer
drops the href of and pandoc keeps. `epubcheck` settles that one:
injecting such a link into an otherwise valid book gives
`ERROR(RSC-007): Referenced resource "EPUB/text/x.html" could not be
found in the EPUB`. `docs/gates.md` has no relative links and sits at
nine; `README.md` has six and sits at 152. A count above nine that those
links do not explain would be a third difference, and there is not one
today.

Two documents are counted as *unwritten* rather than as misses, both for
`ipynb`: `corpus/images.md` and `corpus/readme-style.md` name a picture
that is deliberately absent, and pandoc refuses the conversion where this
falls back to the image's alt text. A document only one of them will
write is not a difference in the writer. Every artefact is deleted before
each pair is built, because pandoc writes *nothing* when it refuses and
`-o out.ipynb` then leaves the previous document's bytes in place — which
made three rows agree by accident until 2026-08-26.

**It is also the only gate that sees the fill.** `writers.sh` compares
at `--wrap=preserve`, and a notebook's markdown cell is written at
`--wrap=auto`, so the two together are what caught a nested list item
filling to 73 columns against 72: the item's content was laid out
knowing its own indent and not the enclosing list's.

Pandoc is given `--resource-path` pointing at the document's directory,
because **pandoc looks for media in the working directory and this looks
beside the document** — see `--resource-path` below. Without that the
comparison measures the difference in path resolution rather than the
writer: pandoc found none of `corpus/images.md`'s pictures and wrote a
book with no frames in it, which read back as a missing image on every
row. That was the ODT row's only miss, and it was the gate's own.

It exists because `diff-docx` and its siblings score **pandoc's own
output**, and a gate cannot fail on a construct pandoc's writer never
emits. In one sitting it found a code block inside a list item split into
a paragraph per line, `--wrap` reaching none of the notebook writer, the
EPUB writer emitting no accessibility metadata, and its `dc:language`
defaulting to `en`. Two fields are normalised away and no others: a
notebook cell's `id` and an EPUB's `dc:identifier`/`dc:date`, each a
documented difference where this writer is deterministic and pandoc is
not. The `epub` row is zero because **every** book differs on `dc:title`
— written always here, omitted by pandoc, and `epubcheck` rejects
pandoc's book for exactly that — so it is gated at zero until that
decision changes, with `--verbose` printing the line count so a second
difference cannot hide behind the first.

**A list is loose when an item holds two blocks with a blank line
between them — and pandoc reads one such list as tight.** The
`CommonMark` specification says a list is loose "if any of its
constituent list items directly contain two block-level elements with a
blank line between them", and

    - first
    - second
      - nested item:

            code line

      trailing para

is exactly that: the second item holds a `Plain`, a nested list and a
paragraph, blank-line separated. This reads it loose. `pandoc -f
commonmark` reads it **tight** and writes `trailing para` with no `<p>`
around it — and reads the same list **loose** the moment the indented
code block inside the nested item is removed, which is what makes it a
slip rather than a rule. `diff-html` is 652/652, so the spec's own
examples do not contain the shape; `COMPATIBILITY.md` does, six lines of
it, and it is the whole of what the `commonmark` and `gfm` writer rows
still differ on in this file. It reaches the **DOCX** writer too, and is
the whole of that row's remainder: the same list written to a `.docx` and
read back is `Plain` for pandoc and `Para` here.

**RST cannot nest inline markup, and the two answers lose different
things.** `**bold with *emph* inside**` has no RST spelling: pandoc keeps
the outer marker and **drops the emphasis** — its own round trip returns
`Strong ["bold with emph inside"]` with no `Emph` in it — while this
closes the outer marker and reopens it, `**bold with** *emph* **inside**`,
which returns all three marks and not the nesting. Neither is exact; one
keeps the words' emphasis and one does not. `sphinx-build -W` accepts
both.

**The EPUB writer emits no syntax highlighting**, where pandoc's book
carries both the spans and 66 lines of CSS for them. That is not an
oversight to fix in passing: the CSS is skylighting's style rather than
`data/templates`, so the BSD carve-out pandoc's template is vendored
under does not reach it, and it is the same owner's decision that holds
`-s` output back — ROADMAP 0.7. Emitting the spans without the CSS is
reachable and would close the round-trip gap; it is recorded here rather
than done, because it is half of a decision that has not been made.

**The `gfm` row's five are the same story**, checked the same way:
`code-and-raw.md` and `truncation-cases.md` are the code block pandoc
loses; `extensions.gfm` and `task-list-runs.gfm` are the `<!-- -->`
separator; and `tables.gfm` is a **pipe inside a cell**. Pandoc writes
`` `x|y` `` and `[t](u|v)` unescaped, which its own reader then splits at
the pipe — round-trip `corpus/gfm/tables.gfm` through
`pandoc -f gfm -t gfm -f gfm` and the code span and the link
**disappear**. This escapes them, and round-trips.

One `gfm` difference is neither: `[www.example.com](…)` is written with
the dot escaped here and bare by pandoc. That escape is deliberate and
load-bearing — **pandoc's own reader linkifies inside link text**, so
pandoc reads its own output back as a `Link` wrapped around a `Link`,
and the escape is what holds `diff-gfm-md` at 656/656.

Six writers went from 2/12–7/12 to where they are on **fifty-one measured
spellings**, each probed against the pinned binary a character or a
construct at a time rather than read out of the manual. The commit
messages carry them one by one; the shape of them is worth stating,
because it is the same three shapes every time:

- **an escape that is wider than pandoc's.** Markdown escaped `a_b`,
  every `&` and every tab; RST escaped every `*` and `|` wherever it
  stood; AsciiDoc reached for a backslash where AsciiDoc has no general
  escape at all and `++*++` is the only spelling that works. Each is
  safe, and each made a README converted here differ from the same README
  converted there on nearly every line;
- **a spelling no round trip can see.** An indented code block and a
  fenced one are the same `CodeBlock`; a padded pipe table and an
  unpadded one are the same `Table`; `.. code::` and `.. code-block::`
  are the same directive. The fidelity gates read 100% through all of
  them;
- **a construct the corpus could not reach.** Tables, task lists and
  footnotes, in every writer at once.

Four divergences stay, each because pandoc's own output does not read
back as what went in. They are named where they arise, and the LaTeX one
is the clearest:

```sh
printf '3) a\n\n4) b\n' > /tmp/l.md
pandoc /tmp/l.md -f commonmark -t latex | pandoc -f latex -t json | grep -o '"c":\[\[1'   # start lost
ferrodoc /tmp/l.md -f commonmark -t latex | pandoc -f latex -t json | grep -o '"c":\[\[3' # start kept
```

An ordered list starting at 3 is `\setcounter` then `\def\labelenumi`
here and the other way round in pandoc, because **pandoc's reader takes
the start value from the first directive it meets and stops looking**.
Both typeset identically; only the byte order differs, and it is the one
document the LaTeX row is short of the corpus. The other three: a
backslash before ASCII punctuation and an entity-shaped `&` in markdown,
where pandoc writes `\\` and drops the character after it; the GFM
autolink triggers, which pandoc leaves bare so its own reader turns
literal text into a link; and a multi-block footnote in AsciiDoc, where
pandoc writes `[multiblock footnote omitted]` and loses the body.

**LaTeX escaping is byte-identical to pandoc's**, checkable directly
because pandoc writes LaTeX from the same AST:

```sh
printf "A \`a  b\`, \`<p>\`, \`a|b\` and a < b with a 'quote'.\n" > /tmp/e.md
diff <(pandoc /tmp/e.md -f commonmark -t latex --wrap=none) \
     <(ferrodoc /tmp/e.md -f commonmark -t latex)   # no output
```

Inline code was `\verb` until 2026-08-22, which is illegal inside a
command argument and stopped `pdflatex` on any heading containing a code
span.

### The write-only formats — judged by their toolchains, not by pandoc

LaTeX, RST and AsciiDoc are written and never read, deliberately: people
author them by hand and convert *out of* them far more often than in, and
a `.tex` file expands user-defined macros, which is a language rather than
a format.

That changes how they are gated, and the numbers above need reading with
care:

- **a LaTeX round trip is lossy for everybody.** Pandoc's own scores
  **1/13** on this corpus — its reader turns a code block with a language
  into two empty divs, drops link titles, and invents a heading identifier
  where the document had none. That number is a measure of the *format*
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
  mean the document is wrong. **One document it refuses, and no writer
  can help it:** `corpus/headings-deep.md` goes from a second-level
  heading straight to a fourth, and RST has no heading levels — the
  underline character acquires one from where it first appears, so a skip
  is "Inconsistent title style: skip from level 1 to 3". Pandoc's RST for
  that document is byte-identical to this writer's, which is what settles
  it as the format's limit rather than ours, and CI makes exactly that
  comparison before accepting the refusal:

  ```sh
  diff <(pandoc corpus/headings-deep.md -f commonmark -t rst --wrap=preserve) \
       <(ferrodoc corpus/headings-deep.md -t rst)   # no output
  ```

- **AsciiDoc refuses the same document, for the same reason.**
  `asciidoctor --failure-level=WARN` says "section title out of
  sequence: expected level 3, got level 4" on `corpus/headings-deep.md`,
  because `=` count is a level there too. Pandoc's AsciiDoc for that
  document is byte-identical to this writer's, and CI compares them
  before accepting the refusal:

  ```sh
  diff <(pandoc corpus/headings-deep.md -f commonmark -t asciidoc --wrap=preserve) \
       <(ferrodoc corpus/headings-deep.md -t asciidoc)   # no output
  ```

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

CI holds the worst path at **80×** on this 10 MB fixture, which is a
regression bound rather than an aspiration: nothing may quietly get
hungrier. What that bound covers by size is the paragraph below.

**What this means in practice.** A 1 MB document needs roughly 75 MB and
fits anywhere. A 10 MB document needs about 750 MB and does not fit a small
edge worker; convert it in a process with room, or split it. A 50 MB
document needs about 3.9 GB.

**The ratio is not constant, and the sentence here used to say it was.**
That claim was measured on a single 10 MB fixture, where the only path
near the bound could not contradict it. Measured across the range, on the
same generated prose (`bash corpus/bench/generate.sh --range`, then
`ferrodoc-harness bench-rss` on each):

| path | 10 MB | 25 MB | 50 MB |
|---|---|---|---|
| markdown → AST | 35.9× | 35.6× | 35.5× |
| markdown → HTML | 35.9× | 35.6× | 35.5× |
| markdown → ODT | 35.9× | 35.6× | 35.5× |
| ODT → markdown | 36.7× | 35.6× | 35.5× |
| HTML → AST | 37.9× | 37.4× | 37.3× |
| markdown → DOCX | 65.4× | 66.2× | 65.2× |
| **DOCX → markdown** | **73.8×** | **77.7×** | **78.1×** |

Six of the seven paths are flat, and they are the six that were never the
constraint. The worst path **rises** — and then flattens: +3.9× over the
first interval and +0.4× over the second, which is a curve approaching
something near 78×, not a line. Continuing the first interval's slope
linearly would have crossed the 80× gate at about 33 MB; that prediction
was written down before the 50 MB measurement and the measurement refuted
it.

**So the published bound is 80× for documents up to 50 MB**, with about 2%
headroom at the top of that range, and nothing is claimed above it. The
gate still runs on the 10 MB fixture, because a 50 MB run costs 3.9 GB and
that is not something to spend on every `verify.sh`; the range above it is
re-measured by hand with the command above.

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

### The `plain` writer — identical to pandoc on the sample, with two stated gaps

`ferrodoc x.md -t plain` now matches `pandoc -t plain --wrap=none` byte for
byte on `samples/inputs/handbook.md`, which is the document written to
carry the awkward cases: `samples/10-markdown-to-plain` went from **47
differing lines to none**. The rules, each probed and each asserted on
literal output because pandoc cannot read plain text back and no
differential gate exists:

| construct | rule |
|---|---|
| block quote | indented 2 spaces, compounding per level |
| code block | indented 4 spaces at the top level, and not inside a list item or a quote, where the container's own indentation sets it apart |
| ordered list | marker column as wide as the widest marker plus a space, never under 4 — `1.  ` and `10. ` |
| list spacing | a `Para` in any item makes the whole list loose; otherwise no blank lines |
| table | 2-space margin, each column the widest cell plus 2, a dashed rule under the head, `AlignRight` padded left |
| footnote | `[N]` at the reference, bodies as `[N] …` at the end |
| strikeout | keeps `~~`, because without them the text says the opposite |
| image | alt text in `[brackets]`, or it reads as prose the document never had |
| horizontal rule | dashes to `--columns` |

One thing is **not** matched, stated rather than hidden: pandoc renders
`Math` as Unicode — `$x^2$` becomes `x²` — where this writes the TeX.

**Two RST spellings this writer keeps and pandoc loses.** A level-6
heading exhausts pandoc's underline characters and it writes a line of
**spaces**, which is not an underline at all - its own reader gives the
heading back as a paragraph, where `'''''` gives back a heading. And a
backtick inside `:literal:` ends the role in pandoc's output, so a code
span comes back as three inlines; escaping it keeps one `Code`, with a
backslash in the content, which is closer and still not exact.

    printf '{"pandoc-api-version":[1,23,1],"meta":{},"blocks":[{"t":"Header","c":[6,["",[],[]],[{"t":"Str","c":"T"}]]}]}' > /tmp/t.json
    pandoc /tmp/t.json -f json -t rst | pandoc -f rst -t json |
      python3 -c 'import json,sys; print([b["t"] for b in json.load(sys.stdin)["blocks"]])'
    # ['Para'] - a Header went in

**Pandoc's renderer is bounded, and this writer matches its fallback.**
It converts what it can to Unicode and gives up on the rest, warning as
it goes, and the two agree from there on:

    printf '{"pandoc-api-version":[1,23,1],"meta":{},"blocks":[{"t":"Para","c":[{"t":"Math","c":[{"t":"InlineMath"},"\\\\frac{a}{b}"]}]}]}' > /tmp/t.json
    pandoc /tmp/t.json -f json -t plain
    # [WARNING] Could not convert TeX math \frac{a}{b}, rendering as TeX
    # $\frac{a}{b}$

So `x^2` differs and `\frac{a}{b}` does not, which is most real
mathematics. The **delimiters** of that fallback were wrong here until
2026-08-28 — HTML wrote `\(…\)`, which is pandoc's `--mathjax` form
and not its default, and the plain writer stripped the dollars
altogether, leaving `\frac{a}{b}` reading as prose with nothing to say
it had ever been an expression.

Three writers spell math three ways and one function served all of
them: `gfm` takes GitHub's `` $`x`$ `` and a ```` ```math ```` fence,
`ipynb` takes `$x$`, and RST takes a `.. math::` directive for display
where `:math:` is only the inline role.

The second used to be the fill, and is not any more: this writer is
byte-identical to pandoc's `--wrap=auto` on all twelve documents at 20,
40, 72 and 100 columns. Four rules were needed and none of them is the
prefix: a **list marker** and a **footnote label** each shorten the line,
the second only its first line; a **table cell** is not filled at all,
though a footnote referenced from one is; and a **heading** is never
filled.

### The HTML reader's edge cases — five fixed, and a sweep that found more

Five divergences no gate could reach, each probed against 3.8.2.1 and now
carried by `corpus/inline-edges.html`, which is identical to pandoc:

| shape | was | now |
|---|---|---|
| `a<em>b </em>c` | the space **dropped** — read as `abc` | hoisted out of the element, as pandoc does |
| `a <br />b` | `Space` kept before the break | dropped |
| `<li id="x">a` | the identifier **lost** | a `Span` around the item's inlines |
| `<li id="x"><p>a</p>` | the identifier **lost** | a `Div` around the blocks — one `<p>` is already this case |
| `<span epub:type="a b">` | no classes | `a` and `b` as classes, attribute kept |
| `<head>` | `meta` left empty | `title`, `lang` and every `<meta name>`; repeats make a `MetaList` |

Three of those lost information rather than misplacing it. The `<head>` one
is the other half of `write_html_standalone`: `-s` writes the title and
authors into the head, and reading that page back dropped them.

**`scripts/sweep-epub-xhtml.sh` is how the remainder is counted**, and it
contradicts the census. It compares every XHTML file inside every corpus
EPUB — the vocabulary pandoc's own writer emits, which is far wider than
the eight `corpus/*.html` the gate walks — and reported **128 files, 77
diverging** where `docs/divergences.md` named six. Two families it never
mentioned dominated it, both **fixed on 2026-08-25**, taking the sweep to
**12** and `diff-html-read` to 638/661 — and three more documents followed
from reading `<![CDATA[ … ]]>` and `<? … ?>` as pandoc reads them, for
641/661:

- a **self-closing tag**. Pandoc's parser honours the slash on every
  element; an HTML5 tree builder ignores it on a non-void start tag, so
  `<a href="…" />` — which pandoc's *own* EPUB writer emits for a
  navigation entry with no text — stayed open here and swallowed the rest
  of the document. The source is now rewritten `<x … ></x>` before
  parsing, skipping void elements and raw-text ones, because
  `ElementFlags::self_closing` does not survive `RcDom`;
- an **`<li id>` with a sub-list**. The identifier becomes a `Span` when
  the item opens with text or an inline element and a `Div` when it opens
  with a block one — the *first child* decides, not the number of blocks,
  so `<li id>text<p>p</p>` is the `Span` case with two paragraphs and
  `<li id><p>p</p>` is the `Div` case with one. Requiring a single `Plain`
  gave a `Div` to every contents entry that had children.

The title-page family was the third, and it was **over-applied**: the drop
ran on any element, which deleted the `<a epub:type="titlepage">` in every
EPUB's landmarks nav. Re-run the sweep before believing any count of what
this reader diverges on.

### Footnotes in HTML — resolved, with two stated divergences

A `role="doc-noteref"` link becomes a `Note` whose body comes from the
container carrying `role="doc-endnotes"`, and that container then
contributes no block — which is what pandoc does. Probed against 3.8.2.1,
and the rule is narrower than it looks: `class="footnotes"` alone does not
do it, `epub:type="footnotes"` does not, and on the reference side
`epub:type="noteref"` alone leaves a plain `Link`. The backlink
(`class="footnote-back"` / `role="doc-backlink"`) is dropped from the body,
as pandoc drops it. `corpus/footnotes.html` covers it, and
`diff-html-read` reads **634/660** where it read 633/659 — the gate fails
at its unchanged 96% floor if the resolution is switched off (95.9%).

Two divergences, both deliberate:

- **`epub:type="noteref"` gives pandoc the class `["noteref"]`** and gives
  us none. That is the general `epub:type`-to-class divergence, not a
  footnote rule.
- **A reference this document cannot answer keeps its `Link`**, where
  pandoc emits `Note []` and warns `Reference not found`. An EPUB holds its
  notes in a *different* XHTML file, and `ferrodoc-epub` resolves them
  across files by matching exactly that link; discarding the target to
  reproduce a warning costs two books on `diff-epub` and buys nothing.

      html='<p>T<a href="#absent" role="doc-noteref">1</a></p>'
      printf '%s' "$html" | pandoc   -f html -t json
      printf '%s' "$html" | ferrodoc -f html -t json

### `pandoc_markdown` — pandoc's dialect, and the shapes it still misses

`-f pandoc_markdown` and `-f markdown` read pandoc's markdown dialect, as
does inferred input for a `.md` file. `-f commonmark` selects CommonMark.
The dialect includes a **YAML metadata block**, **header attributes**
(`# H {#id .cls k=v}`), **definition lists**, and **superscript/subscript**
(`H~2~O`, `E=mc^2^`). The explicit `pandoc_markdown` spelling remains useful
where a command should state its dialect rather than rely on the alias.

```sh
cargo run --release -p ferrodoc-harness -- diff-pandoc-md corpus/pandoc-markdown --fail-under 100
```

**3/3 on its own corpus**, which has its own extension (`.pmd`) for the
same reason the GFM corpus uses `.gfm`: a `.md` file under `corpus/` is in
the denominators of `diff-rst` and `diff-latex`, and a document written to
exercise a dialect those writers do not have is not a fair input to them.
Every rule is mutation-tested — turning off any one of header attributes,
definition lists, superscript, subscript or the metadata block takes the
gate from 3/3 to 1/3.

It is also writable, and since 2026-08-28 it is what `-t markdown`
means — the same name pandoc gives it. `scripts/writers.sh` measures it
at **23/40** byte-identical documents (both preserved and filled
wrapping); the CommonMark writer, still reachable as `-t commonmark`, is
a different output dialect and scored 6/40 against that particular
pandoc writer while it held the name.

**Four divergences, each probed and each left rather than guessed at.**
They were found by sweeping 42 shapes against `pandoc -f markdown -t json`;
38 agree.

| shape | pandoc | here |
|---|---|---|
| `a^b c^` — a space inside superscript | literal text: pandoc requires the space escaped | `Superscript` |
| `H~2 O~` — a space inside subscript | literal text | `Subscript` |
| `a^^b` — an empty superscript | `Superscript []` | literal text |
| a metadata block that is **not** the first thing in the file | read as metadata | read as a thematic break and a heading, as `CommonMark` does |

The first three are comrak's rules for `^…^` and `~…~` rather than
pandoc's, and reconstructing the literal text for them would mean
re-deriving the source from the AST. The fourth is a second metadata block
mid-document, which pandoc supports for concatenated files.

One more, narrower: a hand-written `[http://x](http://x)` — a link whose
text is exactly its target — takes the `uri` class here, where pandoc
gives it only to the `<http://x>` form. Telling them apart means reading
source positions for two identical ASTs, and the class is what pandoc's
own markdown reader puts on autolinks:

    printf 'see <http://x.example> and <a@b.example>\n' | pandoc -f markdown -t json
    printf 'see <http://x.example> and <a@b.example>\n' | pandoc -f gfm -t json

The first classes them `uri` and `email`; the second classes neither, which
is why this belongs to one dialect and stays out of `gfm`.

**A YAML block outside the subset is refused, not guessed.** `key: scalar`,
`key:` with `- item` lines, **block scalars** (`|`, `>`, with any
chomping indicator), `#` comments and blank lines are read; nested maps,
flow collections and anchors are an error naming the line. A metadata block is the one construct where reading it
nearly right is worse than refusing it — the values become the document's
title and authors, and a wrong one is invisible in the output. Pandoc's
value semantics are matched: a scalar is parsed **as markdown inlines**
(`title: A *report*` is `MetaInlines [Str "A", Space, Emph …]`), `true` and
`false` are `MetaBool`, and a number is `MetaInlines`, not a number.

**Extension syntax is refused by name.** `-f markdown+footnotes` is an
error listing what the three dialects read, rather than a flag that looks
accepted and changes nothing.

**`smart` is read, and it is this dialect's alone** — probed, `-f gfm`
and `-f commonmark` both leave `it's` as it stands. `--` is an en dash,
`---` an em dash, `...` an ellipsis, an apostrophe is `’`, and a *pair*
of quotes is a `Quoted` element rather than two characters. The pairing
is the half comrak does not do, and three of its rules were measured
rather than assumed:

* a pair does not cross a container — `"opens *and closes" inside*` is
  two literal marks in pandoc, so the pairing runs over one sibling list;
* an opening mark with a space after it is a **closing** one: `a " b " c`
  is two closing marks and no quotation. comrak decides from the
  character before, pandoc from the character after;
* inside an open quotation the next mark of that kind ends it whichever
  way it leans, and the quotation does not keep the space before it:
  `"b and then "c"` is one quotation of `b and then` and a stray mark.

**`implicit_figures` is read too**: a paragraph that is nothing but one
image *with alt text* is a `Figure`, and the alt text is its caption. The
image keeps its classes and attributes and gives up only its identifier,
which moves to the figure. It happens where a paragraph is built, which
is why a table cell is unaffected and a tight list item is not: the cell
never goes through a paragraph, and the item's is already a figure by the
time the list is tightened. All five shapes measured.

Measured over the CommonMark spec, `445/652` examples now read exactly as
`pandoc -f markdown` reads them, up from 417 — **twenty-eight gained and
none lost**, which is what a 652-example denominator is for. Over the
corpus's own twenty markdown documents it is 10/20, up from 6/20.

Three more constructs are read because comrak parses them and the
attributes it hangs on the node are now picked up: `link_attributes`
(`[a](b){#i .c k=v}`, and the same after an image), `inline_code_attributes`
(`` `code`{.rust} ``) and `inline_footnotes` (`a^[the note]`). An
autolink's `uri`/`email` class shares that field, so it is given only
where the source wrote no attributes of its own.

**Raw HTML is read the way pandoc reads it** — the one rule that
accounted for 43 of the spec's 44 HTML-block examples. Pandoc's markdown
writes **one `RawBlock` per block-level tag** and reads what lies between
two of them as markdown, where CommonMark keeps the whole run as a single
opaque chunk:

    printf '<table>\n  <tr><td>\n hi\n  </td></tr>\n</table>\n' |
      pandoc -f markdown -t json

gives `RawBlock "<table>"`, `RawBlock "<tr>"`, … with `Plain [Str "hi"]`
in the middle. There is no other shape to write: that is what pandoc's
tree holds, and the gate compares trees.

Six rules under it, each measured rather than assumed:

* **which tags are block-level is a list, and it is not CommonMark's.**
  `<embed>`, `<meta>`, `<title>`, `<track>` and `<source>` are; `<a>`,
  `<img>`, `<input>`, `<label>` and `<span>` are not. **Thirty-nine of
  the 107 are not HTML at all** — pandoc also knows DocBook's block
  elements, so `<warning>`, `<note>`, `<tip>` and `<itemizedlist>` open a
  block while `<danger>` and `<foo>` do not.
* `<pre>`, `<script>`, `<style>` and `<textarea>` hold **no markdown**:
  the whole element is one raw block, and it keeps no trailing newline.
* a run with no block-level tag in it is a **paragraph**, tags raw and
  the markdown between them read — `<foo>\n*bar*\n</foo>` has an `Emph`
  in it. A paragraph that *begins* with a block-level tag is split the
  same way, which is the case CommonMark never calls an HTML block at
  all (`<del>*foo*</del>`).
* `native_divs` and `native_spans`: a matched `<div>` pair is a `Div`
  carrying the element's attributes (`id` the identifier, `class` split
  on whitespace, the rest in order, names matched without case), and
  `<span>` a `Span`. An unclosed `<div>` takes everything after it; an
  unmatched `</div>` stays raw.
* of the `<!` forms only a **comment** is raw: `<!DOCTYPE html>` and
  `<![CDATA[ … ]]>` are the literal text they are written with, block or
  inline, which is the opposite of what CommonMark does with them.
* and a paragraph is a `Plain` unless a blank line closes it — pandoc's
  `para` falls back to `plain`, so `a\n<p>x</p>` opens with a `Plain`
  while `a\n\nb` is two `Para`s. A fenced code block closes one as a
  blank line does; a `</div>` does too, because `native_divs` takes it
  rather than leaving it on the next line, which is the whole of why
  `<div>\nx\n</div>` ends in a `Para` and `<td>\nx\n</td>` in a `Plain`.

**HTML blocks went 43/44 failing to 7/44, and the reader 445/652 to
488/652 on that card alone — 43 examples gained and none lost.** What is
left there is
narrow: a `<pre>` whose content holds a blank line (so CommonMark ends
the block before the closing tag), a comment indented one space, and
malformed tags pandoc's inline scanner reads differently.

**Heading identifiers are pandoc's, not GitHub's.** Three differences,
each probed: a **run** of whitespace is one hyphen rather than one per
character (`# foo ### b` is `foo-b`, where `gfm` says `foo--b`), `.` is
kept (`# a.b` is `a.b`, where `gfm` says `ab`), and everything before
the first letter is dropped — `# 1. x` is `x` and `# 123` is `section`.
No gate had seen it because no corpus heading held punctuation between
two words, and every HTML conversion of a pandoc-markdown document
carries these.

**A block scalar in a metadata block is read** — `abstract: |` and the
folded `>`, with any chomping indicator. Pandoc gives one `MetaBlocks`,
the text read as markdown rather than as inlines, *unless* the indicator
strips the trailing newline (`|-`), where it gives `MetaInlines`; all
four measured. This used to be a refusal, which is the worst outcome
available: the whole document stopped converting over one metadata key.

**Bracketed spans are read**: `[text]{#id .cls k=v}` is a `Span`, and
`[text]{.smallcaps}` a `SmallCaps` — that class alone, and among others
a `SmallCaps` inside the span. An attribute list with anything malformed
in it is not a span at all, so `[t]{foo}` stays `[t]{foo}`. The pairing
runs **before** the quote pairing, because `smart` has already made the
quotes in `[t]{k="a b"}` curly and turning those into a `Quoted` first
would eat the value.

**Two unclosed constructs are text, not blocks.** A fence that never
closes is the literal `` ``` `` and the lines under it (`CommonMark` runs
the block to the end of its container), and a `<!--` with no `-->` is the
text it is written with — which `smart` then reaches, so it comes out
`<!–`. The fence's opening line is taken from the *source*, cut at the
column the block starts in, so a `> ` or a `3. ` in front of it is not
part of the text.

**Task items are opposite in the two dialects**, on both counts and each
measured against its own reader: an **ordered** list has task items in
pandoc's markdown (`1. [ ] a` is a box) and none in `gfm`, while an
**empty** marker is a box in `gfm` and the literal `[ ]` in pandoc's
markdown. A plain `- [ ] a` is a box in both.

**A code span is trimmed**, on both sides and of ASCII whitespace only:
`` ` a` `` is `a` where CommonMark says ` a`, and `` ` ` `` is empty. A
non-breaking space inside one is content, not padding.

**What the 154 remaining spec examples are**, by the spec's own sections,
because the shape of that list is the finding:

| failing | section |
|---|---|
| 40/132 | Emphasis and strong emphasis |
| 26/90 | Links |
| 15/27 | Setext headings |
| 9/26 | Lists |
| 8/27 | link reference definitions |
| 7/20, 7/25 | Raw HTML, block quotes |
| 6/18, 6/19, 6/44, 5/29 | ATX headings, autolinks, HTML blocks, fenced code |

**Nothing left in that table is one rule.** Each bucket was sampled
example by example, and the causes are distinct: the largest coherent one
is pandoc's setext heading, which wants a **one-line** paragraph and an
unindented underline (`Foo\nBar\n---` is a paragraph there and a heading
here) and is worth about five; the blank-line-before-a-block rule — a
heading, list or quote cannot interrupt a paragraph — is worth about
four; `***x***` nests `Strong` outside `Emph` where CommonMark nests the
other way, and telling that from `*__x__*` needs the source rather than
the tree, for two. The rest are one apiece.

**This is the parser rather than the dialect, and no card closes it.**
Emphasis flanking, link destinations with spaces or newlines, setext
underlines, and pandoc's rule that a heading, a list or a block quote
needs a blank line in front of it — `a\n# H` is one paragraph there and a
paragraph plus a heading here. Pandoc's markdown is not CommonMark plus a
feature list, and those sections are where the difference stops being a
feature at all. That is the ceiling this reader is approaching, and it is
why the number to watch is the shape of the table rather than the total.

**What the remaining ten corpus documents need**, each named rather
than described as "the dialect": a
fence info string of several words (pandoc does not read one as a fence
at all), `***x***` nesting `Strong` outside `Emph` where CommonMark
nests the other way round, and the four `.gfm` documents, which are read
by a dialect that is not GFM and disagree about pipes in code spans and
about how a run of task items is cut into lists.

### `markdown` means pandoc's markdown on the way in, CommonMark on the way out

**Reversed on 2026-08-27.** It used to mean CommonMark in both
directions, and the paragraphs below describe what that cost; they are
kept because the cost is what decided it.

`ferrodoc -f markdown` and a `.md` file with no `-f` now read pandoc's
own dialect, as they do in pandoc. `-f commonmark` names CommonMark.
**`-t markdown` still writes CommonMark**; the dialect writer is
`-t pandoc_markdown`. Keeping those output names distinct preserves the
original writer contract while making pandoc's spelling available.

What decided it, measured over this repository's own documents against
`pandoc -t html` with no `-f` on either side:

| read as | differing lines |
|---|---:|
| CommonMark | **2,466** |
| pandoc's dialect | **241** |

Ten times closer, and **not one document worse**. Drop-in went 12/48 to
**22/48** in the same commit. Twenty-eight of the 48 real command lines
in `dropin/` name no input format at all, so the extension decided it for
them, and it decided wrongly.

The old behaviour is below, as the record of what the flag used to do.
Five things a pandoc document may carry were read as the literal text
they are written with:

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

**What the dialect costs is measured, and it is most of the drop-in
number.** `scripts/dropin.sh --attribute` retries every miss with one of
pandoc's own features neutralised at a time and names the smallest set
that makes the two agree. On the 38 misses in 48 real command lines:

| what one change would fix the row | rows |
|---|---|
| reading `-f markdown` as pandoc's dialect | 4 |
| that, and pandoc's default syntax highlighting | 1 |
| highlighting alone | **8** |
| a difference this project keeps (`dropin-006`) | 1 |
| neither | **12** |

A **table of contents** took it from 22/48 to 26/48 on 2026-08-27: the
`<a>` of every TOC entry was written with a literal space between its
attributes where the rest of this writer marks a break opportunity, so
pandoc filled those lines and this did not. Four `--toc` command lines
were byte-identical the moment it did.

Those first two rows read **17** and **10** before the default changed.
What is left is the honest remainder: twelve rows are gaps in this
project's own reading of pandoc's dialect, which the old default kept out
of sight behind a decision.

**This was the pre-alias analysis.** The input decision is now implemented
and the corpus is 22/48, not "one decision from the high thirties". The
remaining rows are tracked as parser and command-surface work; the
`pandoc_markdown` writer supplies the dialect output separately from the
longstanding CommonMark `-t markdown` name.

### `--wrap` — pandoc's, since 2026-08-24

**Every text writer lays lines out all three ways, and the default is
`auto` at 72 columns, which is pandoc's.** It was `preserve` for five
writers and `none` for two until then — not a decision but seven writers
written separately — and the flip is what card D4.3 was waiting for.

The gap it closed was the largest single cause in the drop-in corpus:
`scripts/dropin.sh --attribute` put **23 of 44 misses** on the fill alone,
and the number doubled from 4/48 to 8/48 the day the default changed.
Eight of the fifteen `samples/` are byte-identical to pandoc now, at both
binaries' defaults, where four were before — and `samples/generate.sh` no
longer runs pandoc twice and keeps the closer output, which is a
workaround it needed for exactly this.

Each writer is byte-identical to pandoc's fill over the twelve documents
in `corpus/` and `corpus/gfm/`, at 20, 40, 72 and 100 columns:

| writer | identical, four widths | what still differs |
|---|---|---|
| `html` | **60/60** (five widths) | — |
| `plain` | **48/48** | — |
| `rst` | 44/48 | four documents at 20 columns |
| `asciidoc` | 43/48 | the deliberate multiblock footnote, and one table at 20 |
| `latex` | 40/48 | the deliberate `\setcounter` order, and footnote nesting below 72 |

A line breaks only where a `Space` or `SoftBreak` stood in the tree, and
each writer adds the places its own syntax allows: HTML breaks **between
a tag's attributes**, LaTeX inside a `\footnote{…}` but not inside
`alt={…}`, RST between the pieces of a split emphasis but never inside
one. A heading is never filled in `plain`, `rst` or `asciidoc` — it is in
HTML and LaTeX, which is measured rather than assumed — a word wider than
the column overruns rather than being cut, and a list marker or a quote's
prefix counts toward the width.

**Columns are display columns.** A CJK ideograph and an emoji take two,
and `\u{200b}`, `\u{200d}` and `\u{fe0f}` take none. The table in the
HTML writer was measured, not transcribed: every codepoint in the blocks
that could plausibly be wide went through `pandoc --wrap=auto
--columns=13` alone in a paragraph.

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

### `--toc`, `--number-sections` and `-M` — matched on the parts that are claims

Three flags a pandoc command line reaches for, and the first of them used
to be an unknown-option error, which fails a Makefile at the swap rather
than converting slightly differently.

**What is compared, and what deliberately is not.** The rest of a
standalone page is *not* pandoc's and is not meant to be: pandoc's default
template carries a ~170-line stylesheet, an `xmlns`, a `generator` meta and
a title taken from the **file name**, where `-s` here writes a minimal page
from what the document actually knows. So the gate compares fragments —
the `<nav id="TOC">…</nav>` block, and the `^<h[1-6]` lines — and claims
nothing about template equality:

```sh
./scripts/compare-toc.sh          # 6/6 documents identical
```

Six is the whole denominator: every markdown document under `corpus/` and
`samples/inputs/` with two or more headings, two of them added with this
work. Both mutations were checked — a `TOC_DEPTH` of 4 drops it to 4/6, and
numbering from level 1 instead of the document's shallowest heading drops
it to 5/6.

The rules, each probed against pandoc 3.8.2.1 with `--wrap=none`:

| | |
|---|---|
| depth | three levels, which is **pandoc's `--toc-depth` default**, not a property of the format — `pandoc --toc-depth=4` disagrees |
| nesting | relative, not absolute: `#`, `###`, `##` puts the last two as siblings one level in |
| no identifier | no link — the entry is bare text, which is every heading in a `-f commonmark` document |
| numbering base | one component per level from the document's **shallowest** heading, so an all-`##` document numbers `1`, `2` and a mixed one numbers the `##` as `0.1` |
| `unnumbered` | takes no number and consumes none: the heading after it continues the sequence |
| containers | a heading inside a `Div` is numbered and listed; one inside a `BlockQuote` is neither |
| no headings | **no** `<nav>` element at all, rather than an empty one |
| `-M` | `-M k=v` is a `MetaString`, a bare `-M k` is `MetaBool true`, and `-M title=…` overrides a title the document carried |

`--toc` without `--standalone` is accepted and emits nothing, because there
is no page to put the contents in — pandoc does the same, and erroring
would fail a build pandoc runs happily. `--toc` or `--number-sections`
against a non-HTML output warns on stderr rather than silently doing
nothing; pandoc numbers LaTeX and DOCX too, and this does not.

**A heading's attributes are written in a different order from every other
element's**, which this work found and fixed:

    printf '# H {#i .foo data-k=v}\n' | pandoc -f markdown -t html --wrap=none
    <h1 class="foo" data-k="v" id="i">H</h1>

Class first, then the key-values with `data-number` among them, and the
identifier **last** — where a `Div` with the same attributes gets `id`
first. No gate reached it: the `CommonMark` spec's headings carry no
attributes at all, so `diff-html` reads 652/652 either way.

### Trimmed builds — what a feature subset drops, and how it says so

Every format is a cargo feature of `crates/ferrodoc`, and `default =
["all"]`: a caller who does nothing gets what this document measures
everywhere else. Three features imply another — `odt` implies `docx`,
`epub` implies `html` and `docx`, `ipynb` implies `markdown` — because each
of those crates already depends on the one it pulls in, so the implication
costs no bytes.

What a trimmed build does with a format it does not contain:

| | |
|---|---|
| `ferrodoc -f docx` | `docx support was not compiled into this build; known formats: …`, exit 1 |
| `ferrodoc --help` | the `FORMATS:` block lists exactly what is compiled in; the default build's block is byte-identical to before features existed |
| `ferrodoc::parse` / `render` | `Error::NotCompiled(Format::Docx)` — the name is real, the code is absent, which is a different answer from `unknown format` |
| `Format::parse("docx")` | still `Some(Format::Docx)`: the enum keeps every variant in every configuration, so the three bindings compile unchanged |

Measured on this machine, at the commit that added the features:

    ./bindings/wasm/build.sh
    target/wasm32-unknown-unknown/release/ferrodoc_wasm.wasm  1846226 bytes (679019 gzipped)

    ./bindings/wasm/build.sh --no-default-features --features ferrodoc/markdown,ferrodoc/html
    target/wasm32-unknown-unknown/release/ferrodoc_wasm.wasm  1162137 bytes (402019 gzipped)

That is **58%** of the gzipped module for markdown plus HTML, and 60% of
the CLI binary (6,408,248 → 3,871,424). comrak and html5ever are what
remains and neither can be dropped while those two formats are wanted, so
this is close to the floor for a build that still converts anything.

**Both ratios were measured twice, in two checkouts, and one of the two
figures does not reproduce exactly.** The CLI binary is byte-identical
across build directories; the wasm module is not — the same source at a
different path differs by about 0.03% (1,846,226 against 1,846,003 for the
all-formats module), so the module embeds something path-dependent that the
binary does not. The ratio is stable and is what the claim rests on; an
exact byte count for the module is only reproducible from the same
directory.

CI builds and tests one subset on all three platforms; a `#[cfg]` that was
never added shows up nowhere else, because the default build compiles
whatever the features forgot.

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
