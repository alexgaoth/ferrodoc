# Roadmap

## What this project is

Convert ordinary editorial documents — Markdown, HTML, DOCX, and the office
and publishing formats beside them — semantically, quickly, locally and
predictably, from inside a program rather than by shelling out.

The boundary is deliberate, and it is a *square*, not a count: the
conversions people actually perform between the documents they actually
hold. Pandoc's long tail — presentations, citation processing, template
languages, bibliographic databases, the wikis with a handful of users each —
is not a goal, and matching pandoc's ~40 formats is not the measure of
success. `## What "pan" would mean here` says where the edge is and why.

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
| Python | `pip install ferrodoc` — one typed function, abi3 wheels for 3.9+ on Linux, macOS and Windows, GIL released so a thread pool overlaps. **72× a pandoc subprocess** per document |
| Independent corpus | 7/8 documents *LibreOffice* wrote read identically to pandoc — the first evidence the DOCX reader generalises beyond pandoc's own output |
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

## When to run `/tend`

`CLAUDE.md` is read into every future session. A line there costs tokens in
every session forever, so it earns its place only by **changing what an
agent would do**. Deleting a stale line is worth as much as adding a good
one, and "nothing worth adding" is a valid outcome — never pad the file.

Run it in the session that did the work. The transcript is the input, so a
fresh subagent has nothing to harvest; never delegate it.

**What to harvest.** What did I learn by trial, error or grep that one line
would have told me upfront? Where was I wrong — a failed command, a broken
test, a user correction, a review finding? What did the user state as a
rule? Which commands actually work where the obvious guess fails? What
invariant does the code not announce? And which existing line did I
misread — an instruction that failed to instruct is a bug, not a fact.

**Four gates. A candidate needs all four:** non-obvious (two minutes of
reading would not reveal it), durable (still true after the branch merges),
behaviour-changing (a future agent acts differently), and project-scoped
(personal preferences and secrets never go in a committed file).

**Budget by governed file count**, measured with
`git ls-files ":(top)<dir>" | wc -l`: under 100 files → 40 lines; under
1,000 → 60; under 10,000 → 80; beyond → 100, and past that the answer is
more nested files rather than a longer root. Spend it by **displacement**:
at or over budget, a new line must name the weaker line it beats, and that
line goes. If no line is weaker than the candidate, the candidate was not
worth adding. Past double the budget, re-file rather than compress —
package facts into nested `CLAUDE.md`, long-form detail into `docs/` behind
a plain pointer, never an `@import`, which would load it anyway.

**Five traps.** Changelog ("we just refactored X" — that is git history).
Tour guide (narrating a tree anyone can `ls`). Wiki (restating what the
code says plainly). Hedge ("consider", "generally" — if it is not a rule it
does not earn a line). Trophy (recording what a session accomplished; the
file is for the next agent, not about this one).

The root file is repo-wide; a fact about one crate belongs in that crate's
`CLAUDE.md`. Human-written rule lines are authoritative: tighten or
relocate them, never silently drop one — if a rule looks wrong, keep it and
say so.

---

## What "pan" would mean here

Pandoc reads about 40 formats and writes about 60. Matching that count is
not the goal and never was — but "convert ordinary editorial documents" is
a wider square than four formats, and the architecture is already the one
that widens cheaply. Four rules decide what to add and in what order.

**Count the square, not the formats.** N readers and N writers give N²
conversions, and every new reader immediately gains every writer already
written. Adding ODT reading is not one feature; it is ODT → markdown, →
GFM, → HTML, → DOCX and → plain, on the day it lands. Rank by how much of
the square a spoke closes, not by how popular the format sounds.

**The AST is the ceiling.** ferrodoc's AST *is* pandoc's, proven by
`diff-ast` round-tripping any `pandoc -t json` document. Anything pandoc's
AST cannot express, ferrodoc cannot carry either — slide layouts, tracked
changes, bibliographic databases. "Pan" here means the formats that
pandoc's AST already describes, and the non-goals below are mostly the
formats it does not.

**No format ships without a differential gate.** What makes this project
worth choosing is not the number of formats, it is that every one of them
is checked against pandoc document by document. A format added faster than
its gate spends the only thing the project has. This is cheap to honour:
pandoc reads `odt`, `epub`, `rst`, `latex` and `rtf` and writes all of
them, so a new spoke can be gated on the day it is started, with the
harness that already exists.

**Writers are bounded; readers are not.** A writer walks a finite AST and
emits text — the work is escaping and a rule per node, and it is done. A
reader must handle everything anyone has ever written in that format, plus
malformed input, forever. So coverage is far cheaper on the writer side,
and a reader is only worth it for a format people already *have documents
in*.

## How a format gets added

The same five steps every time, which is what makes this repeatable rather
than a research project:

1. A reader and/or writer crate, or a module beside an existing one.
2. A variant in `Format`, wired into `parse`/`render` and the `--help`
   text in `main.rs` — until then it is unreachable by users.
3. A `diff-<format>` gate in `ferrodoc-harness` comparing against pandoc,
   added to the root `CLAUDE.md` list and to `.github/workflows/ci.yml`.
4. A corpus: documents *another program wrote*, not our own output.
   `corpus/docx-libreoffice/generate.sh` is the pattern.
5. A row in `COMPATIBILITY.md` with its number and every deliberate
   divergence.

---

## Next, in order

### 1. ODT, read and write — `/iterate`

The largest gain per line of new code in the project, because it is the
DOCX crate's own machinery pointed at a sibling format. Measured on a
LibreOffice-written file: an `.odt` is a zip whose `content.xml` holds
`<office:body><office:text>`, which is the same shape as
`word/document.xml`'s `<w:body>` — so the streaming `body_children`
pattern transfers directly, `media.rs` transfers whole, and the zip layer
is the dependency already in use.

It also finishes the office square. Today ferrodoc converts what Word
wrote; with ODT it converts what LibreOffice, OpenOffice, Google Docs
export and every European public-sector template wrote, in both
directions.

- Gate: `diff-odt` against `pandoc -f odt`, plus a corpus of documents
  LibreOffice wrote — `corpus/docx-libreoffice/generate.sh` already
  produces `.odt` on the way to `.docx` and needs one argument changed.
- Watch for: ODT keeps styles in a separate `styles.xml` with inheritance,
  the way DOCX does, and the same "match by name, never by id" rule will
  apply. Lists carry their level in the element rather than in a numbering
  part, which is simpler than DOCX, not harder.
- **Compare against pandoc-on-ODT, never against the DOCX of the same
  document.** They are not the same target. Converting one HTML file to
  both and reading each with pandoc gives `[Para, Para, Header, …]` from
  the `.odt` and `[Header, Para, Header, …]` from the `.docx` — the
  heading survives one path and not the other. A gate built on "the ODT
  should read like the DOCX" would be chasing pandoc's own disagreement
  with itself.

**Iterate: yes.** A new reader *and* writer, a package another program
must open, and a new gate — three of the four conditions the loop exists
for, in one item.

### 2. EPUB, read then write — `/iterate`

A zip whose content is XHTML, which means the reader that does the work
already exists. Reading is: `META-INF/container.xml` names the `.opf`, the
`.opf` gives a manifest and a spine, each spine item goes through
`read_html`, and the results concatenate in spine order. Writing is the
HTML writer plus a manifest, a spine and a nav document. Images come from
the media bag that `docx → docx` already uses.

After PDF, this is the format most often asked of a converter, and it is
the one publishing pipelines need. Read first: people have EPUBs they want
as markdown far more often than the reverse.

- Gate: `diff-epub` against pandoc, both directions.
- Watch for: the spine is the reading order and it is not the file order;
  a chapter is a document boundary, not a `<div>`; and `epub2` and `epub3`
  differ in the manifest, not in the content.

**Iterate: yes** — a package another program must open, and silent loss is
one wrong spine entry away.

### 3. A LaTeX writer, and deliberately no LaTeX reader

This is the honest road to PDF, and it costs nothing.

Writing LaTeX is bounded work: escape the ten special characters, map each
AST node to a macro, and stop. *Reading* LaTeX means expanding arbitrary
user-defined macros — that is interpreting a language, not parsing a
format, and it is where a converter goes to die.

And a LaTeX writer *is* PDF output, for everyone who has a TeX
installation: `ferrodoc report.docx -t latex | pdflatex`. The binary does
not grow by a single crate, which is exactly what the PDF item below could
not manage.

- Gate: `diff-latex`, round trip — write LaTeX, have **pandoc** read it
  back, require the AST to survive. The same shape as `diff-write`.

### 4. Writers for reStructuredText and AsciiDoc, when a pipeline asks

Both are bounded writer work and both unlock a documentation toolchain
(Sphinx, Antora) that currently shells out to pandoc. Neither is worth a
reader: people write these by hand in editors that already understand
them, and convert *out* of them far more often than in.

### 5. Learn from real documents and users

Do not widen the surface on speculation beyond the square above. A corpus
failure from a real document, a measured resource problem, or a workflow
someone cannot complete outranks every format on this page. Pin each
finding as a regression fixture before fixing it.

The named small gaps belong here: the remaining markdown-reader
divergences, `spec-09.docx`, the nested quotation DOCX writer case, stated
image dimensions, and the HTML reader's valid-input mismatches.

### 6. An npm package, if the evidence points there

The Python wheel shipped and is the adoption surface to watch. Build an
npm package when the users turn out to be browser, edge or CMS teams — the
wasm32 release artefact is not that package yet, and needs a
JavaScript-facing wrapper and a bytes/string API. Do not build it in
parallel with anything; one binding at a time, each a product commitment
with its platforms documented and its installation tested in CI.

### Later, and only for a demonstrated need: PDF without a TeX installation

Item 3 gives PDF to anyone who has TeX, at no cost. What remains is PDF
for someone who has nothing installed at all, and that is where the
arithmetic bites: `typst` + `typst-pdf` takes the dependency tree from
**63 crates to 283**, before the fonts and ICU data Typst needs, and the
binary whose 33× smallness is a published claim would grow by an order of
magnitude.

So: a separate `ferrodoc-pdf` crate, or a default-off feature shipping a
second binary. Nobody who wants a small converter pays for a typesetter.

**Iterate: yes, per phase** — a new crate with a new output format has no
gate at all until one is built.

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
- **LaTeX *reading***. A `.tex` file expands user-defined macros, so reading
  it means interpreting a language rather than parsing a format. Writing
  LaTeX is planned and bounded; reading it is not on the list at any point.
- Wiki dialects, DocBook, JATS, FB2, man, Textile and the rest of the long
  tail — one reader each, for audiences a converter in Rust will not reach.
  Reconsider only against a real document someone cannot convert.
