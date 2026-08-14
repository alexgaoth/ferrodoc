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
| DOCX memory | the body streams, so its XML tree never exists in full: **2.7× less peak RSS and ~12% faster**, interleaved against a baseline |
| Standalone HTML | `-s` writes a whole page — doctype, charset, `lang`, title, authors — and `--css` inlines a stylesheet |

Performance, measured with `bench-sizes` at 10 KB / 1 MB / 10 MB: markdown →
AST 0.78 ms / 103 ms / 709 ms; AST → HTML 25 µs / 6.7 ms / 39 ms; AST → docx
1.4 ms / 108 ms / 1.24 s; docx → AST 4.1 ms / 472 ms / 16.6 s. Peak RSS for
docx → markdown: 6 MB / **152 MB** / **1.34 GB** — what remains is the AST
and the source, not the XML tree. (Absolute figures drift ~2× between
sittings on this machine; only interleaved ratios are comparable.)

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

### 1. The image formats Word actually writes — `/iterate`

`docx → docx` keeps its pictures, except when it does not: `media::inspect`
recognizes PNG, JPEG and GIF, and everything else falls back to alt text. A
Word document with a pasted chart (EMF/WMF), a modern icon (SVG) or a
photo saved from the web (WebP) loses it — and the media bag now carries the
bytes all the way to a writer that refuses them, which is the most annoying
possible place to stop.

- Extend `media::inspect` with the header parsing each needs for its
  dimensions: WebP (`RIFF`/`VP8X`), EMF (its bounds record), TIFF (IFD tags
  256/257), and SVG (`width`/`height`, or `viewBox` when they are relative).
- Content types must be declared per extension or Word rejects the package.
- Anything still unrecognized must keep falling back to alt text, never
  produce a package that will not open.

**Iterate: yes.** Silent content loss, and the failure — a package Word
refuses — is exactly what no gate can see. That combination has now produced
a BLOCKER twice.

### 2. `HTML → AST` is superlinear — probably no `/iterate`

1.25 ms / 168 ms / **4.12 s** across 10 KB / 1 MB / 10 MB: the same shape the
DOCX reader had before its body was streamed, and there is now a playbook for
it. Ablate first to find where the cost actually is — interning names in the
DOCX tree would have won 8% where deleting the tree won 5×.

`html5ever` builds an `Rc`-based DOM that this reader then walks and drops, so
the tree may be the cost the same way it was there. Measure before choosing.

**Iterate: only if the fix restructures the reader.** A pure optimization is
judged by an interleaved A/B against a baseline, which is a better critic than
a reviewer. But if it changes *how* the reader walks — as streaming did for
DOCX — the truncation and error-path questions come back, and those are worth
a round.

### 3. Raw passthrough in the HTML reader — `/iterate`

An element the reader does not know contributes its children and loses its own
tag, so `<video>`, `<iframe>`, `<figure>` and every custom element quietly
become their contents. Pandoc emits `RawBlock`/`RawInline` and keeps them.

This is the last silent-loss family left in a reader, which is what makes it
worth doing before the polish items below.

**Iterate: yes** — the same reason as item 1. `diff-html-read` would score it,
but "what did the user lose" is not a number that gate reports.

### 4. Publish 0.1 — blocked on a decision, not on work

Everything reversible is done. Publishing cannot be undone and a yanked
version is still visible, so it needs an owner's go-ahead.

- Publish order: `ferrodoc-ast`, `-markdown`, `-html`, `-text`, `-docx`, then
  `ferrodoc`.
- Tag `v0.1.0` — the tag is what triggers the release build.

**Iterate: no.** Nothing to review; it is a decision.

### 5. PDF output — as a separate crate, not in the binary

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

### 6. Python and Node bindings — blocked on evidence

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
