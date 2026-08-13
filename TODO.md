# Roadmap

## What this project is

Convert ordinary editorial documents — Markdown, HTML and DOCX — semantically,
quickly, locally and predictably, from inside a program rather than by shelling
out.

The boundary is deliberate. Pandoc's long tail (presentations, LaTeX, citation
processing, template languages, ~36 further formats) is not a goal, and
matching it is not the measure of success. Semantic conversion of the common
path is.

## Two principles that keep settling arguments

**Rank by what a user cannot do today**, not by what is closest to finished. A
missing capability that blocks a whole workflow outranks a fidelity gap in a
workflow that already works.

**Match pandoc wherever pandoc has a describable rule on well-formed input;
diverge only where matching would mean reproducing a parse failure.** This
settled the empty-anchor and `<pre>`-newline questions in the HTML reader, and
it is the reason a heuristic keyed on parse errors was measured and rejected.

---

## Done

Detail lives where it is useful rather than here: `COMPATIBILITY.md` has every
gate with its reproducing command and every known loss by name; `CLAUDE.md` has
the rules that are easy to get wrong; `.iterate/*/` has the critic verdicts
behind them.

| | state |
|---|---|
| Markdown reader | 652/652 spec examples identical to pandoc |
| HTML writer | 652/652 identical |
| Markdown writer | 652/652 round-trip fidelity — **pandoc manages 593/652** |
| DOCX reader | 36/37 corpus documents identical |
| DOCX writer | 643/652, with embedded PNG/JPEG/GIF and document metadata |
| HTML reader | 631/657 identical; closes the Markdown ↔ AST ↔ HTML ↔ DOCX square |
| GFM | reader 654/655 identical; writer 655/655 round-trip fidelity — **pandoc manages 589/655**. `docx → gfm` keeps its tables |
| Media | `docx → docx` keeps its images, byte for byte; `read_docx_with_media` and `parse_with_media` expose the bag |
| Verifiability | CI on three platforms against pinned pandoc, wasm32 build, 500k-mutation fuzz per run, `COMPATIBILITY.md` |
| Packaging | licences, crate metadata, tag-driven static binaries — everything except the irreversible steps |

Performance, measured with `bench-sizes` at 10 KB / 1 MB / 10 MB: markdown →
AST 0.39 ms / 59 ms / 780 ms; AST → HTML 12 µs / 4.0 ms / 43 ms; AST → docx
0.52 ms / 56 ms / 645 ms; docx → AST 2.2 ms / 334 ms / 9.8 s. Peak RSS for
docx → markdown: 8 MB / 365 MB / **3.5 GB**.

---

## Next, in order

### 1. Bounded-memory DOCX reading

3.5 GB of peak RSS for a 4.3 MB `.docx` — roughly 800× the input, because the
whole XML tree and the whole AST are live at once. Fine on a laptop, fatal in
a 256 MB container, and the one number that makes ferrodoc unusable somewhere
pandoc is usable.

- Map paragraphs as they stream out of `quick-xml` instead of materializing
  the tree first, or drop each subtree once its blocks are built.
- Target: peak proportional to the largest *paragraph*, not the document.
- `bench-sizes` plus `/usr/bin/time` is the check; it must not cost more than
  ~10% latency.

Second-order, same area: `HTML → AST` is also superlinear (93 ms → 2.3 s
across a 10× input). Worth a look once the DOCX path is done.

### 2. `--standalone` HTML output

The writer emits fragments. Anyone converting a document *for the web* — the
obvious reason to want HTML — has to hand-write `<html>`, `<head>`, a charset
and a title around it. Pandoc's `--standalone` is one flag.

- `<!doctype>`, `<html lang>`, charset, `<title>` from document metadata.
- Optional CSS by path; no template language (that is a declared non-goal).

### 3. Publish 0.1 — blocked on a decision, not on work

Everything reversible is done. Publishing cannot be undone and a yanked
version is still visible, so it needs an owner's go-ahead.

- Publish order: `ferrodoc-ast`, `-markdown`, `-html`, `-text`, `-docx`, then
  `ferrodoc`.
- Tag `v0.1.0` — the tag is what triggers the release build.

### 4. PDF output — as a separate crate, not in the binary

The original case was "a single ~3 MB binary that renders markdown to PDF with
no system dependencies". Right idea, wrong arithmetic: `typst` + `typst-pdf`
takes the dependency tree from **63 crates to 283**, before the fonts and ICU
data Typst needs, and the binary whose 34× smallness is a published claim
would grow by an order of magnitude.

So: a separate `ferrodoc-pdf` crate, or a default-off feature shipping a
second binary. Nobody who wants a small converter pays for a typesetter. Large
work; not next.

### 5. Python and Node bindings — blocked on evidence

Bindings are the adoption vector, but guessing the wrong one costs months.
Wait for a user.

---

## Smaller things worth doing when they block someone

- GFM's own pipe tables are all a table can be: merged cells expand, cell
  blocks flatten to one line, the caption becomes a following paragraph.
  Pandoc falls back to a raw HTML `<table>` instead. Worth revisiting only
  if someone needs the structure more than the grid.
- The HTML reader keeps whitespace inside an inline element where pandoc
  hoists it out: `<em> b</em>` is `Space` + `Emph[b]` for pandoc and
  `Emph[Space, b]` here. Found by making `corpus/inline-elements.html`
  live — a `<main>` element in it had been selecting away the other 49
  lines, so nothing in the file could fail.
- The HTML reader emits no `RawBlock`/`RawInline`: an unknown element
  contributes its children and loses its own tag.
- A regression fixture for *every* mismatch ever found. The recent ones have
  them; older ones live only in `.iterate/` verdicts.
- The three open markdown-reader divergences: entity-encoded spaces inside
  `Str` and refdef-plus-dash-run (see `.iterate/20260810-markdown-reader/`),
  and a lone `\` on its own line, which pandoc reads as one `LineBreak`
  where comrak gives `SoftBreak`+`LineBreak`. The last is what stops a hard
  break directly after a soft one round-tripping exactly.
- DOCX images beyond PNG/JPEG/GIF (SVG, WebP, TIFF, EMF) fall back to alt text.
- `restore_verbatim_newline` scans the source for `<pre>` without knowing
  whether that `<` sits inside another tag's attribute value.

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
