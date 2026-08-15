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
| DOCX writer | 643/652, with embedded images and document metadata |
| HTML reader | 632/658 identical; closes the Markdown ↔ AST ↔ HTML ↔ DOCX square. Content a browser hides — `<template>`, `<noscript>` — is read rather than dropped |
| GFM | reader 654/655 identical; writer 655/655 round-trip fidelity — **pandoc manages 589/655**. `docx → gfm` keeps its tables |
| Media | `docx → docx` keeps its images, byte for byte; `read_docx_with_media` and `parse_with_media` expose the bag |
| Image formats | PNG, JPEG, GIF, WebP, TIFF, SVG, EMF and WMF embed, each at the size and resolution its own header states — Exif and JFIF, `pHYs`, TIFF rationals, an EMF frame. All eight open in LibreOffice; ten measured divergences from pandoc, all in `COMPATIBILITY.md` |
| Verifiability | CI on three platforms against pinned pandoc, wasm32 build, 500k-mutation fuzz per run, `COMPATIBILITY.md` |
| Released | **0.1.0 is on crates.io** — all six crates — and `v0.1.0` is tagged on GitHub with binaries for Linux musl, both macOS architectures, Windows and wasm32. CI runs green on the pushed branch |
| DOCX memory | the body streams, so its XML tree never exists in full: **2.7× less peak RSS and ~12% faster**, interleaved against a baseline |
| Benchmarks | every path measured on fixtures `corpus/bench/generate.sh` writes, and `bench-sizes` now fails when a read errors instead of timing the refusal |
| Inline `<svg>` | a picture written into the page is carried as a `data:` URL, and a `data:` URL now reaches the DOCX writer — HTML with an inline chart converts to a `.docx` LibreOffice opens with the picture |
| Standalone HTML | `-s` writes a whole page — doctype, charset, `lang`, title, authors — and `--css` inlines a stylesheet |

Performance at 10 KB / 1 MB / 10 MB, one sitting, on fixtures
`bash corpus/bench/generate.sh` writes:

| path | 10 KB | 1 MB | 10 MB |
|---|---|---|---|
| markdown → AST | 531 µs | 43.8 ms | 551 ms |
| AST → HTML | 37 µs | 10.8 ms | 115 ms |
| AST → markdown | 77 µs | 13.1 ms | 137 ms |
| AST → docx | 1.09 ms | 73.2 ms | 963 ms |
| docx → AST | 4.4 ms | 491 ms | 11.1 s |
| HTML → AST | 381 µs | 53.6 ms | 644 ms |

Five of the six paths are linear in their input. **`docx → AST` is not**:
0.38 s per MB at 1 MB and 0.64 at 10 MB, 16.9× the time for 10× the input.
It is the one path where size costs more than proportionally, and the only
one worth measuring again if a big document feels slow. Peak RSS for
docx → markdown: 6 MB / 117 MB / **1.12 GB** — what remains is the AST and
the source, not the XML tree. (Absolute figures drift ~2× between sittings
on this machine; only interleaved ratios are comparable.)

---

## When to run `/iterate`

Not every change earns a builder–critic loop. The rule that decides it, drawn
from every finding a critic has found here that the gates did not:

> **Iterate when the failure mode is one the gates cannot see.**

The nine gates compare ASTs against pandoc. They are excellent at "this
mapping is wrong" and blind to everything else — and *everything else* is
where the expensive bugs have been:

| what a critic found | why no gate saw it |
|---|---|
| `~~~~` swallowed the rest of the document | the AST round-tripped; the *text* was a code fence |
| a package Word could not open | no gate parses ferrodoc's own output as XML |
| a footnote picture replaced by the body's | no gate compares image bytes |
| `- ☐ a` shipped instead of `- [ ] a` | both spellings read back to one AST |
| `--help` never mentioned `gfm` | no gate reads the help text |
| `docx → markdown` at 840 MB for 5 MB of use | no gate measures memory |
| a threshold that tolerated five failures | the gate itself was the defect |
| a fixture inert for two rounds | it passed, because it could not fail |
| every scanner JPEG 4x too wide | no gate reads an image header |
| an offset overflow that aborts on wasm32 | every gate runs on x86-64 |

So: **iterate** a change that writes a file another program must open, that
touches memory or the CLI surface, that adds a gate, or that can lose content
silently. **Skip it** for a change whose whole failure mode is a differential
number — a reader mapping rule is already watched by `diff-*` far better than
a reviewer would be.

Practice that worked: `max-rounds=3`, a fresh critic each round, and findings
that arrive after the cap get fixed anyway and recorded in an addendum. Give
the critic the real verify commands, including the ones you expect to fail.
The audit trail belongs in `.iterate/<date>-<slug>/`.

---

## Next, in order

### 1. Learn from real documents and users

After 0.1, do not widen the format surface on speculation. The project has
its intended conversion square; the next useful work is a corpus failure from
a real document, a measured resource problem, or a workflow a user cannot
complete. Pin every such finding as a regression fixture before fixing it.

The currently named small gaps belong here: the remaining markdown-reader
divergences, `spec-09.docx`, the nested quotation DOCX writer case, stated
image dimensions, and the HTML reader's valid-input mismatches. They are
repairs, not a reason to delay the first release.

### 2. Add one binding only when usage chooses it

The CLI and Rust crates are the first adoption surface. Do not build Python
and Node bindings in parallel, and do not guess which community matters more.

- Build a **Python wheel** when the users are ingestion, RAG, ETL, notebook,
  or Python-backend teams. It should be a thin native binding over this core,
  not a Python rewrite.
- Build an **npm package** when the users are browser, edge, or CMS teams.
  The current WASM release artefact is not that package yet: it needs a
  JavaScript-facing wrapper and a bytes/string conversion API.

Either binding is a product commitment: document its supported platforms,
test installation in CI, and keep its public surface as small as the Rust
facade. Until evidence selects one, neither is the next task.

### Later, only when demand justifies it: PDF output

The original case was "a single ~3 MB binary that renders markdown to PDF with
no system dependencies". Right idea, wrong arithmetic: `typst` + `typst-pdf`
takes the dependency tree from **63 crates to 283**, before the fonts and ICU
data Typst needs, and the binary whose 34× smallness is a published claim
would grow by an order of magnitude.

So: a separate `ferrodoc-pdf` crate, or a default-off feature shipping a
second binary. Nobody who wants a small converter pays for a typesetter. Large
work; do it only for a demonstrated use case.

**Iterate: yes, per phase** — a new crate with a new output format has no gate
at all until one is built, which is the condition the loop exists for.

---

## Smaller things worth doing when they block someone

None of these need a loop: each is a single rule with a gate that scores it.

- The HTML reader keeps whitespace inside an inline element where pandoc
  hoists it out: `<em> b</em>` is `Space` + `Emph[b]` for pandoc and
  `Emph[Space, b]` here. Found by making `corpus/inline-elements.html`
  live — a `<main>` element in it had been selecting away the other 49
  lines, so nothing in the file could fail.
- The three open markdown-reader divergences: entity-encoded spaces inside
  `Str` and refdef-plus-dash-run (see `.iterate/20260810-markdown-reader/`),
  and a lone `\` on its own line, which pandoc reads as one `LineBreak`
  where comrak gives `SoftBreak`+`LineBreak`. The last is what stops a hard
  break directly after a soft one round-tripping exactly.
- GFM's own pipe tables are all a table can be: merged cells expand, cell
  blocks flatten to one line, the caption becomes a following paragraph.
  Pandoc falls back to a raw HTML `<table>` instead. Worth revisiting only
  if someone needs the structure more than the grid.
- A regression fixture for *every* mismatch ever found. The recent ones have
  them; older ones live only in `.iterate/` verdicts.
- Pandoc counts `<output>`, `<canvas>` and `<textarea>` block-level and
  splits a paragraph around them into `Plain` fragments; this reader keeps
  them inline, because all three are phrasing content. And a `<template>`
  that is the *first* thing in a document goes into the head, which this
  reader does not read, so its content is lost where pandoc keeps it —
  the one position where a template still loses anything.
- A **stated** image size is read differently from pandoc, both measured
  while adding the image formats: `{width=100px}` is 100 points here and 75
  for pandoc, which counts a CSS pixel at 96 dpi; and giving only a width
  leaves the height at the image's own, where pandoc scales it to keep the
  aspect ratio. Intrinsic size — what the file itself says — already
  matches. `emu()` in `write.rs` is the whole of it.
- `docx → AST` is the one superlinear path: 16.9× the time for 10× the
  input. Ablate before optimizing, and remember the last time this was
  claimed of the HTML reader it was the benchmark timing a rejection.
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
