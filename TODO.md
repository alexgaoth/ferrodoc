# Roadmap

## What this project is

Convert ordinary editorial documents — Markdown, HTML and DOCX — semantically,
quickly, locally and predictably, from inside a program rather than by shelling
out.

The boundary is deliberate. Pandoc's long tail (presentations, LaTeX, citation
processing, template languages, ~36 further formats) is not a goal, and
matching it is not the measure of success. Semantic conversion of the common
path is.

## Ordering principle

Rank by *what a user cannot do today*, not by what is closest to finished. A
missing capability that blocks a whole workflow outranks a fidelity gap in a
workflow that already works.

---

## 1. Markdown writer — done

`docx → markdown` and `html → markdown` were impossible; `ferrodoc report.docx
-t markdown` now works. Measured by `diff-md` as fidelity — write the document
out, read it back, require what returns to be what went in: **652/652 spec
examples, against pandoc's 593/652 at its best setting.** This is the first
place ferrodoc is ahead of pandoc on output quality rather than level with it.

What is left is an opt-in **GFM mode** (tables, task lists, strikethrough),
which is what makes the tables currently flattened to paragraphs survive. It
is a new capability, not a repair, so it ranks against the items below rather
than inside this one.

The four documented losses in `write.rs` are limits of CommonMark, not bugs;
GFM removes none of them.

## 2. Embedded images in the DOCX writer — done

PNG, JPEG and GIF images are embedded as real pictures: media parts, content
types, relationship wiring, one part per source image however often it
appears, and sizing from the document's own `width`/`height` or else the
image header. Document metadata (title, subtitle, author, date) is written as
the styled leading paragraphs the reader recovers, and matches pandoc's.

Byte resolution is the caller's job — `write_docx_with_media` takes a
resolver — because the crate must stay pure enough to compile for `wasm32`.
The CLI resolves relative to the input document.

Still open, and now the DOCX writer's only real losses:

- Raw blocks, which have no OOXML equivalent at all.
- Formats beyond PNG/JPEG/GIF (SVG, WebP, TIFF, EMF) fall back to alt text.
- The reader has no media bag, so `docx → docx` still loses images: the AST
  records the part path, not the bytes. Exposing them is what would close it.

## 3. HTML reader — done

The triangle closes:

```
Markdown ↔ AST ↔ HTML
              ↕
            DOCX
```

`ferrodoc page.html -t markdown` works. `html5ever` parses; the work was the
mapping. **631/657 documents produce an AST identical to pandoc's** — the 651
HTML fragments the `CommonMark` spec ships as expected output, plus five
hand-written files (and one pre-existing `corpus/docx/src/tables.html`)
covering tables, column widths and alignment, attributes, the sectioning
elements, the `<main>` rule, the semantic inline elements and verbatim
content, none of which the spec's HTML exercises.

Worth knowing when reading real pages: a document with a `<main>` element
(or `role="main"`) is read as **that element alone** — pandoc does the same,
and it is what keeps navigation menus and cookie notices out of the output.

Most of the 26 remaining mismatches are one cause: **ferrodoc parses to the
HTML5 spec and pandoc parses with `tagsoup`, which does not.** On malformed
markup the two build different trees, and no mapping reconciles them:

- A tag with no closing `>` (`<div id="foo"` at end of input). HTML5 discards
  it; tagsoup keeps it and invents attributes from the following text.
- An unclosed `<a>` inside a paragraph. The tree builder closes it and its
  formatting element is re-opened after the paragraph, so we produce an empty
  link and a second block; tagsoup produces neither.
- A `<pre>` or a `<div>` opened inside a `<tr>` or an `<li>` and never
  closed, which HTML5 foster-parents out of the table.

But **not all of them**, and assuming so hid real bugs for a round: the
`<![CDATA[…]]>` and `<style>` raw-text boundaries are a tokenizer
disagreement on input that is merely unusual, and `<a/>` self-closing syntax
sends the two parsers down different recovery paths.

One judgement call worth recording: dropping links with empty content would
have scored two examples higher by matching tagsoup on unclosed `<a>` tags,
but it also deletes `<a href="./target.md"></a>` — a well-formed empty anchor
that pandoc keeps and that real pages use as jump targets. A reviewer
suggested keying it on `html5ever`'s parse-error list instead; that was
measured (+2) and rejected, because the error list is document-scoped, so a
page whose only flaw is an unclosed `<b>` somewhere else loses its anchors —
a divergence from pandoc that the simpler rule does not have.

The principle that settled it, and the `<pre>` newline with it: **match
pandoc wherever pandoc has a describable rule on well-formed input; diverge
only where matching would mean reproducing a parse failure.**

Still open, and stated so nobody has to rediscover them:

- The reader does not produce `RawBlock`/`RawInline`; unknown elements
  contribute their children and lose their own tag.
- A block element reached through an inline one — `<a href="#"><div>…</div>
  <p>…</p></a>`, the "card link" pattern HTML5 allows — keeps its text
  intact and separated, but stays one block where pandoc produces several.
- `RESERVED_ATTRIBUTES` is 260 names derived from the binary, and the reader
  and writer share it: a name it does not recognize is read without its
  `data-` prefix and written back with it. That symmetry is what stops a
  round trip turning `data-onclick` into an event handler that runs, and it
  is what pandoc's writer does too — an earlier attempt to solve this in the
  reader alone was wrong, and left the hole open on the way out.
- `restore_verbatim_newline` scans the source for `<pre>` and does not know
  whether that `<` is itself inside another tag's attribute value, so
  `<div title="<pre>` followed by a newline gains one it should not.

## 4. Make the promise verifiable by strangers

Trust is the product here.

### Absolute benchmarks — done, and they found something

`ferrodoc-harness bench-sizes` reports latency and throughput per path at any
size; peak memory is measured from outside with `/usr/bin/time`, because an
in-process figure would need a custom global allocator and `unsafe` is
forbidden across this workspace for a better reason than a benchmark.

Release build, this machine, one sitting. Treat as an order of magnitude:

| path | 10 KB | 1 MB | 10 MB |
|---|---|---|---|
| markdown → AST | 0.39 ms | 59 ms | 780 ms |
| AST → HTML | 12 µs | 4.0 ms | 43 ms |
| AST → markdown | 42 µs | 7.6 ms | 71 ms |
| AST → docx | 0.52 ms | 56 ms | 645 ms |
| docx → AST | 2.2 ms | 334 ms | 9.8 s |
| HTML → AST | 0.64 ms | 93 ms | 2.3 s |

Peak RSS: markdown → HTML is 4 MB / 77 MB / 734 MB; **docx → markdown is
8 MB / 365 MB / 3.5 GB.**

Two findings, both of which the benchmark existed to surface:

- **A quadratic, now fixed.** Uniquing a heading identifier restarted its
  search at `-1` every time, so documents whose headings share a name —
  "Summary" once per section, which is what an ordinary sectioned document
  looks like — cost O(n²). 1 MB of them took **72 seconds**; it now takes
  0.34 s, and 10 MB of mixed content went 40 s → 9.8 s. The same bug was in
  the HTML reader, copied along with the algorithm. Regression test in
  `crates/ferrodoc-docx/src/write.rs`.
- **The DOCX reader's full-DOM parse is real and expensive.** 3.5 GB of peak
  RSS for a 4.3 MB `.docx` — roughly 800× the input — because the whole XML
  tree and the whole AST are live at once. Fine on a laptop, fatal in a
  256 MB container. This is the number that makes streaming a priority
  rather than a maybe; see the last section.

### Still missing

- CI on Linux, macOS and Windows, pinned to the supported pandoc version.
- A published compatibility matrix, *including the known losses* — the two open
  markdown-reader divergences, `spec-09`, the DOCX writer's dropped images.
- A regression fixture for every mismatch ever found, added when it is found.
- Fuzzing in CI for markdown, XML, ZIP and pathological nesting. Several of the
  worst bugs in this project were found this way; none of it runs automatically.

## 5. Ship it

The CLI and facade crate already exist; this is packaging, not building.

- Tag a 0.1 with a documented, deliberately small public API.
- Static CLI binaries per platform.
- Publish the crates.
- A WASM package — the browser story is real and already compiles.
- Python and Node bindings **only once there is evidence of where users are**.
  Bindings are the adoption vector, but guessing the wrong one costs months.

## 6. Later, and only later: PDF output via embedded Typst

Filed as a *later phase*, not a non-goal, because it is the one place where
this project can beat pandoc outright rather than merely match it: pandoc
cannot produce a PDF without a LaTeX or Typst installation, often gigabytes of
it. A single ~3 MB binary that renders markdown to PDF with no system
dependencies is a differentiating capability, not a completeness checkbox.

Not before items 1–5. It is a large piece of work and it is worthless on top of
an unstable core.

---

## Explicit non-goals

Declared, not deferred — so nobody has to re-litigate them:

- Tracked changes, comments, reviewer workflows.
- Forms and content controls (beyond unwrapping `w:sdt` to reach the content).
- Page geometry, headers/footers, complex section breaks.
- SmartArt, charts, macros, embedded spreadsheets.
- Pixel-perfect layout preservation. The goal is semantic conversion, not
  "open any Word file and reproduce every visual detail".
- **PDF *reading***. That is an ML problem (layout analysis, OCR), owned by
  Docling and Marker. Interoperate with them; do not compete.
- Citations, templates, Lua filters, presentation formats.

## Smaller things worth doing when they block someone

- `--standalone` HTML output. The writer emits fragments; anyone converting a
  document for the web wants `<html>`, a title and optional CSS.
- The two open markdown-reader divergences (entity-encoded spaces inside `Str`,
  refdef-plus-dash-run) — see `.iterate/20260810-markdown-reader/`.
- **Streaming or bounded-memory DOCX reading.** The 10 MB benchmark said so:
  3.5 GB peak for a 4.3 MB file. No longer conditional.
