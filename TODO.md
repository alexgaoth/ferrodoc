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

## 2. Embedded images in the DOCX writer

The largest fidelity gap on a path that already works: the writer drops images
to their alt text and discards raw blocks. Needs media parts, content-type
overrides and relationship wiring.

Also here: document metadata (title/author/date) on the write side, since the
reader already understands it.

## 3. HTML reader — completes the triangle

```
Markdown ↔ AST ↔ HTML
              ↕
            DOCX
```

Structural only: headings, lists, tables, links, images, formatting. Not CSS,
not layout reproduction. `html5ever` does the parsing; the work is the mapping.

## 4. Make the promise verifiable by strangers

Trust is the product here, and none of this exists yet:

- CI on Linux, macOS and Windows, pinned to the supported pandoc version.
- A published compatibility matrix, *including the known losses* — the two open
  markdown-reader divergences, `spec-09`, the DOCX writer's dropped images.
- A regression fixture for every mismatch ever found, added when it is found.
- Fuzzing in CI for markdown, XML, ZIP and pathological nesting. Several of the
  worst bugs in this project were found this way; none of it runs automatically.
- Benchmarks at 10 KB, 1 MB and 10 MB reporting **latency and peak memory in
  absolute terms**, not only ratios against pandoc. 10 MB will expose the DOCX
  reader's full-DOM parse — that is worth knowing before a user finds it.

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
- Streaming or bounded-memory DOCX reading, if the 10 MB benchmark says so.
