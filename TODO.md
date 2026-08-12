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

### CI, fuzzing and the compatibility matrix — done

- `.github/workflows/ci.yml`: build, test and clippy on Linux, macOS and
  Windows; a wasm32 build, because the browser story is a README claim; the
  seven conformance gates against a **pinned** pandoc 3.8.2.1, installed from
  its release asset rather than whatever the runner has; and a fuzz campaign.
- `COMPATIBILITY.md`: every gate with the command that produces it, and every
  known loss named one by one — the two markdown-reader divergences,
  `spec-09`, `nested-structures.md`, the DOCX writer's raw blocks and image
  formats, the four CommonMark limits, the HTML reader's 26, the two
  deliberate divergences, and the 3.5 GB memory ceiling.
- `ferrodoc-harness fuzz` mutates the corpus — byte flips, truncation,
  repeated regions, spliced structural tokens, deletions — and requires every
  reader to refuse rather than panic. Not coverage-guided, because that needs
  nightly and a contributor may not have it; this is the part that pays.
  **1.1 million mutations across five seeds found nothing**, which is the
  first time that has been true rather than assumed. A short fixed-seed run
  is a `cargo test`, so it runs on every change; CI runs 500,000 with a fresh
  seed each time so the search keeps moving.

Still missing: a regression fixture for *every* mismatch ever found. The
recent ones have them; the older ones are described in `.iterate/` verdicts
rather than pinned in the corpus.

## 5. Ship it — prepared, not shipped

Everything reversible is done; everything irreversible is waiting on a
decision that is not mine to make.

Done:

- **Licences.** `LICENSE-MIT` and `LICENSE-APACHE` exist. `license = "MIT OR
  Apache-2.0"` was declared for months with no licence text in the tree,
  which would have blocked publishing and left users guessing.
- **Crate metadata.** `rust-version`, `homepage`, `keywords`, `categories`
  and `readme` on all six crates, inherited from the workspace.
  `cargo publish --dry-run -p ferrodoc-ast` packages clean.
- **A deliberately small public API**, unchanged and worth keeping small:
  `Format`, `Error`, `parse`, `render`, `render_with_media`, `convert`, and
  the `ast` module. Six items and one module.
- **`.github/workflows/release.yml`.** A tag builds static binaries for
  linux-musl (so it runs on any distribution, not just new-enough glibc),
  macOS on both architectures, and Windows, and attaches them to the
  release, along with a wasm32 artefact.

Deliberately **not** done, because it is outward-facing and irreversible:

- **`cargo publish`.** A published version cannot be unpublished; yanking
  leaves it visible. Publish order is `ferrodoc-ast`, then `-markdown`,
  `-html`, `-text`, `-docx`, then `ferrodoc`. Needs someone to own the name.
- **Tagging 0.1.** Same reason: the tag is what triggers the release build.
- **Python and Node bindings**, still waiting on evidence of where users
  are. Guessing the wrong one costs months.

## 6. PDF output via embedded Typst — measured, and the premise does not hold

The case for this item was: *"A single ~3 MB binary that renders markdown to
PDF with no system dependencies is a differentiating capability."* That is
the right idea and the wrong arithmetic, and the measurement kills it in its
current form.

Adding `typst` and `typst-pdf` takes the dependency tree from **63 crates to
283** — a 4.5× increase, before counting the embedded fonts and ICU data
Typst needs to lay text out. The binary would not be ~3 MB. It would be an
order of magnitude larger than the 4.6 MB one whose smallness is a headline
claim, measured and published in the README as 35× smaller than pandoc.

So the item as written trades the project's clearest advantage for a
capability, which is the opposite of the bet. Two ways it could still
happen, neither of them "add Typst to the CLI":

- **A separate crate**, `ferrodoc-pdf`, that nobody pays for unless they
  want it. The core binary and the wasm build stay as they are.
- **A cargo feature**, off by default, with the release workflow shipping
  both a lean binary and a `ferrodoc-pdf` one, so the 35× claim keeps a
  binary it honestly describes.

Either way it is a large piece of work and it is not the next one. Filed as
open, with the number attached, rather than as a plan.

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
