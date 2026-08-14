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
| Packaging | licences, crate metadata, tag-driven static binaries — everything except the irreversible steps |
| DOCX memory | the body streams, so its XML tree never exists in full: **2.7× less peak RSS and ~12% faster**, interleaved against a baseline |
| Benchmarks | every path measured on fixtures `corpus/bench/generate.sh` writes, and `bench-sizes` now fails when a read errors instead of timing the refusal |
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

Every path is linear in its input. Peak RSS for docx → markdown: 6 MB /
117 MB / **1.12 GB** — what remains is the AST and the source, not the XML
tree. (Absolute figures drift ~2× between sittings on this machine; only
interleaved ratios are comparable.)

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

### 1. Inline `<svg>` is dropped — `/iterate`

The last silent-loss family left in a reader. A chart or an icon written
inline in a page — `<svg>…</svg>` rather than `<img src>` — becomes nothing
at all here. Pandoc serializes the element back to text and emits it as an
`Image` with a `data:image/svg+xml;base64,…` URL, which is why the picture
survives a conversion through pandoc and not through this.

Found by sweeping the element vocabulary in valid contexts, after the two
losses that sweep was looking for had already been fixed.

- `ferrodoc-html` has neither an XML serializer nor base64. `markup5ever`
  can serialize a subtree; base64 is twenty lines or a dependency, and this
  crate must keep building for wasm32 with no C library.
- The writer side already round-trips a `data:` URL as an image, so only
  the reader is missing.

**Iterate: yes** — silent content loss, which is the condition the loop
exists for, and `diff-html-read` cannot report *what* a user lost.

### 2. Publish 0.1 — blocked on a decision, not on work

Everything reversible is done. Publishing cannot be undone and a yanked
version is still visible, so it needs an owner's go-ahead.

- Publish order: `ferrodoc-ast`, `-markdown`, `-html`, `-text`, `-docx`, then
  `ferrodoc`.
- Tag `v0.1.0` — the tag is what triggers the release build.

**Iterate: no.** Nothing to review; it is a decision.

### 3. PDF output — as a separate crate, not in the binary

The original case was "a single ~3 MB binary that renders markdown to PDF with
no system dependencies". Right idea, wrong arithmetic: `typst` + `typst-pdf`
takes the dependency tree from **63 crates to 283**, before the fonts and ICU
data Typst needs, and the binary whose 34× smallness is a published claim
would grow by an order of magnitude.

So: a separate `ferrodoc-pdf` crate, or a default-off feature shipping a
second binary. Nobody who wants a small converter pays for a typesetter. Large
work; not next.

**Iterate: yes, per phase** — a new crate with a new output format has no gate
at all until one is built, which is the condition the loop exists for.

### 4. Python and Node bindings — blocked on evidence

Bindings are the adoption vector, but guessing the wrong one costs months.
Wait for a user.

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
