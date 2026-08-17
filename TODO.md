# Roadmap

## The bet

Pandoc owns the command line and will keep owning it: about forty formats,
fifteen years of edge cases, citations, templates and a filter ecosystem.
Competing on format count is a race that cannot be won, and winning it would
not be worth what it cost.

What pandoc cannot be is **embedded**. It is a 153 MB binary you spawn. That
is fine once and ruinous ten thousand times; it is impossible in a browser
tab, impossible inside a request handler with a latency budget, and
impossible to ship inside somebody else's product.

> **The bet: be the document conversion layer other software *links*, not
> the one it shells out to.** SQLite's position, not MySQL's.

The measurements that make it plausible are already in, and each is
reproducible from this repository: **72×** faster per document in process,
**33×** smaller on disk (4.6 MB against 152.9 MB), **22×** less peak memory,
a wasm32 build, deterministic output, and readers that refuse hostile input
rather than panicking.

Everything below follows from that bet, and it rules out more than it rules
in. A format nobody holds documents in does not serve it. A binding for an
ecosystem that actually writes document pipelines does — which is why the
Python wheel was worth more than any format on the list.

**What would falsify it.** If the people who arrive want a faster CLI rather
than a library, the ordering below is wrong and this file should be
re-derived from that instead. The evidence is free to watch: what the Python
wheel's downloads do, and what the issues that arrive ask for. Bringing that
to the user is step 5 of the procedure below — it is the one decision this
file does not get to make on its own.

---

## The four horizons, and how to tell when one is finished

The bet decomposes into four things that have to be true at once. Each has
an exit test, so "are we there" is checkable rather than a matter of taste.

| | what it means | exit test | state |
|---|---|---|---|
| **H1 Reachable** | callable from the language the pipeline is already in | the ecosystems that hold document pipelines can `install` it | Rust ✅ Python ✅ CLI ✅ · **JavaScript ❌ · JVM/Go/C# ❌** |
| **H2 Sufficient** | the square covers what an editorial team actually holds | a team gets from what they hold to what they publish without reaching for pandoc once | markdown, GFM, HTML, DOCX, ODT ✅ · **EPUB ❌ · PDF/LaTeX out ❌** |
| **H3 Trustworthy** | stated resource bounds that hold on *any* input | every path publishes a bound CI checks | never-panics ✅ deterministic ✅ bounded recursion ✅ · **no checked memory bound, one superlinear path** |
| **H4 Believed** | the numbers are reproducible by someone who does not trust us | every README claim has a command in the repo and a CI job | 14 gates ✅ two independent corpora ✅ · standing work, never "done" |

H1 and H2 are the reach of the thing. H3 is why anyone would embed it rather
than shell out — and it is the horizon that quietly decays, because a
guarantee breaks silently as code grows. H4 is what makes the other three
believable to somebody who has been burned by a converter before.

---

## Three rules that decide the order

Applied in order. They are what turns a pile of candidates into a queue, and
they exist so nobody has to re-argue the ordering each time.

**1. A binding outranks a format.** N formats × M bindings: a binding
multiplies, a format adds. The Python wheel put all five formats in front of
every Python pipeline in one release; ODT added one row and one column to a
square only Rust and the CLI could reach.

**2. Impossible outranks inconvenient.** Someone who needs RST today shells
out to pandoc and is mildly annoyed. Someone who needs conversion inside a
browser tab, or in a process that must not fork, or under a 128 MB edge
worker limit, cannot do it at all — at any price. Rank the impossible above
the awkward.

**3. A guarantee decays unless something checks it.** Never-panic,
determinism, bounded memory: these are the whole reason to link rather than
spawn, and each breaks silently. **When a guarantee is currently unchecked,
the item that adds the check outranks any new feature.** This is the rule
that keeps H3 from losing every argument to something more visible.

**Tie-break: close more of the square.** N readers and N writers give N²
conversions, and every new reader immediately gains every writer already
written. ODT was not one feature; it arrived as ODT → markdown, → GFM, →
HTML, → DOCX and → plain, plus the five back, on the day it landed.

---

## How to pick the next item without asking

This is the loop. A future session runs it and gets the same answer, which
is the point — the roadmap re-derives itself instead of being re-decided.

1. **Refresh the evidence.**
   - Did a gate go red? Fix that. Nothing outranks a regression.
   - Did a real document fail to convert? Pin it as a fixture *first*, then
     add the item. A finding from a real document outranks every format on
     this page (rule 2 — it is somebody's impossible).
   - Is a guarantee unchecked? Note it; rule 3 will promote it.
2. **Score the pool** — the list below, plus whatever step 1 added — by the
   three rules in order, tie-broken by the square.
3. **Take the top item.** Write its gate before its code: `## How a format
   gets added` is the checklist, and a format added faster than its gate
   spends the only thing this project has.
4. **When it lands**, move it into `## Where the bet stands`, delete it from
   the list, and *run step 2 again* — writing the new order into this file.
5. **If the bet itself looks wrong**, stop and say so to the user. Re-ranking
   is automatic; re-aiming is not.

> This file is the input to step 2, so it has to be rewritten every time an
> item lands. **A roadmap that is not re-ranked is just a list.**

---

## Next, in order

Derived by applying the three rules to today's pool. The derivation is shown
so that disagreeing with the order means disagreeing with a rule, not with a
mood.

### 1. A published resource bound, and the one superlinear path

*Rule 3: the guarantee is unchecked. And item 2 depends on it.*

`docx → AST` takes **16.9× the time for 10× the input**, and `docx →
markdown` peaks at **1.12 GB of RSS for a 10 MB document**. Every other path
is linear. Nothing in CI would notice if a second path stopped being.

This is ranked first for a reason worth writing down: **the environments a
wasm build unlocks are the memory-constrained ones.** A browser tab or a
128 MB edge worker cannot absorb 1.12 GB, so shipping item 2 on top of an
unbounded path would put the package's worst behaviour in front of exactly
the audience it was built for.

- Gate: `bench-sizes` measures **latency only** — every peak-RSS figure
  quoted in this repository was taken by hand, outside it, and nothing
  re-measures them. So the work is: teach `bench-sizes` peak RSS, add the
  ODT paths it does not cover, make it *fail* when a path exceeds a stated
  multiple of its input, and put the multiples in `COMPATIBILITY.md` under
  "Resource limits worth knowing".
- **Ablate before optimizing.** Deleting the DOCX tree won 5× where interning
  its names would have won 8%. And the last time a path was called
  superlinear it was the benchmark timing a *rejection* — measure the
  measurement first.

**Iterate: yes.** No differential gate sees memory, and this adds a gate —
two of the conditions the loop exists for.

### 2. A JavaScript package (wasm) — `/iterate`

*Rules 1 and 2, both pointing the same way: it is a binding, and it is the
only item that makes something currently impossible possible.*

Conversion in a browser tab with **no document leaving the client** is not
something pandoc can offer at any price, and it is the strongest reason
anyone would choose this over the thing that already works. Edge workers and
Node pipelines come with it.

The wasm32 artefact exists and CI builds it; a *package* does not. Needed: a
JavaScript-facing wrapper, a bytes/string API that matches the Python one's
shape, and installation tested in CI on the platforms claimed — a wheel that
builds but does not import was the failure the Python job exists to catch,
and npm has the same failure mode.

- One binding at a time. Each is a product commitment with platforms
  documented and installation tested, not a build artefact with a README.

**Iterate: yes** — a published surface, and a new install path that can pass
its build and fail its use.

### 3. EPUB, read then write — `/iterate`

*Rule 2 within H2: a publishing pipeline with EPUBs cannot convert them in
process at all today.*

A zip whose content is XHTML, so the reader that does the work already
exists. Reading is: `META-INF/container.xml` names the `.opf`, the `.opf`
gives a manifest and a spine, each spine item goes through `read_html`, and
the results concatenate in spine order. Writing is the HTML writer plus a
manifest, a spine and a nav document. Images come from the media bag
`docx → docx` already uses.

- Gate: `diff-epub` against pandoc, both directions.
- Watch for: the spine is the reading order and it is not the file order; a
  chapter is a document boundary, not a `<div>`; `epub2` and `epub3` differ
  in the manifest, not in the content.
- **The lesson ODT paid for: measure what pandoc's *reader* can actually
  hold before writing a mapping for it.** Half the ODT reader turned out to
  be about *not* producing constructs pandoc's own reader never emits — no
  metadata, no code blocks, no table spans. Writing those mappings first
  would have been days spent making the gate fail.

**Iterate: yes** — a package another program must open, and silent loss is
one wrong spine entry away.

### 4. A LaTeX writer, and deliberately no LaTeX reader

*H2, and it is nearly free.*

Writing LaTeX is bounded work: escape the ten special characters, map each
AST node to a macro, and stop. *Reading* LaTeX means expanding arbitrary
user-defined macros — interpreting a language rather than parsing a format,
and where a converter goes to die.

A LaTeX writer **is** PDF output for everyone with a TeX installation:
`ferrodoc report.docx -t latex | pdflatex`. The binary does not grow by a
single crate, which is exactly what the PDF item below cannot manage.

- Gate: `diff-latex`, round trip — write LaTeX, have **pandoc** read it back,
  require the AST to survive. The same shape as `diff-write`.

### 5. A C ABI, when a second ecosystem asks

*Rule 1 says a binding outranks a format, but rule 2 says wait: Go, JVM and
C# pipelines can shell out today, so this is inconvenient, not impossible.*

One `extern "C"` surface unlocks Go, Java, C#, Ruby and Julia at once, which
is the highest multiplier left. It is ranked below EPUB anyway because
nobody has yet said they cannot proceed without it. **Promote it the moment
somebody does** — that is step 1 of the procedure, not a change of plan.

### 6. Writers for reStructuredText and AsciiDoc, when a pipeline asks

Both are bounded writer work and both unlock a documentation toolchain
(Sphinx, Antora) that currently shells out to pandoc. Neither is worth a
reader: people write these by hand in editors that already understand them,
and convert *out of* them far more often than in.

### 7. Learn from real documents and users — standing

Do not widen the surface on speculation. A corpus failure from a real
document, a measured resource problem, or a workflow somebody cannot
complete outranks every format on this page. Pin each finding as a
regression fixture before fixing it.

The named small gaps live in `## Smaller things worth doing when they block
someone` below, and are promoted from there by step 1, not by tidiness.

### Later, and only for a demonstrated need: PDF without a TeX installation

Item 4 gives PDF to anyone who has TeX, at no cost. What remains is PDF for
someone with nothing installed, and that is where the arithmetic bites:
`typst` + `typst-pdf` takes the dependency tree from **63 crates to 283**,
before the fonts and ICU data Typst needs, and the binary whose 33×
smallness is a published claim would grow by an order of magnitude —
directly against the bet.

So: a separate `ferrodoc-pdf` crate, or a default-off feature shipping a
second binary. Nobody who wants a small converter pays for a typesetter.

**Iterate: yes, per phase** — a new crate with a new output format has no
gate at all until one is built.

---

## Where the bet stands

Detail lives where it is useful rather than here: `COMPATIBILITY.md` has
every gate with its reproducing command and every known loss by name;
`CLAUDE.md` has the rules that are easy to get wrong; `.iterate/*/` has the
critic verdicts behind them.

| | state |
|---|---|
| Markdown reader | 652/652 spec examples identical to pandoc |
| HTML writer | 652/652 identical |
| Markdown writer | 652/652 round-trip fidelity — **pandoc manages 593/652** |
| DOCX reader | 36/37 corpus documents identical |
| DOCX writer | 643/652, with embedded images and document metadata |
| HTML reader | 632/658 identical; closes the Markdown ↔ AST ↔ HTML ↔ DOCX square. Content a browser hides — `<template>`, `<noscript>` — is read rather than dropped |
| GFM | reader 654/655 identical; writer 655/655 round-trip fidelity — **pandoc manages 589/655**. `docx → gfm` keeps its tables |
| ODT | reader 32/34 identical, and **8/8 on documents LibreOffice wrote**; writer 11/11 corpus and 640/652 spec through a pandoc round trip, with embedded images. Completes the office square: what Word wrote and what LibreOffice, Google Docs export and every European public-sector template wrote, in both directions |
| Media | `docx → docx` and `odt → odt` keep their images, byte for byte; `read_*_with_media` and `parse_with_media` expose the bag |
| Image formats | PNG, JPEG, GIF, WebP, TIFF, SVG, EMF and WMF embed, each at the size and resolution its own header states — Exif and JFIF, `pHYs`, TIFF rationals, an EMF frame. All eight open in LibreOffice; ten measured divergences from pandoc, all in `COMPATIBILITY.md` |
| Inline `<svg>` | a picture written into the page is carried as a `data:` URL, and a `data:` URL reaches the office writers — HTML with an inline chart converts to a document LibreOffice opens with the picture |
| Standalone HTML | `-s` writes a whole page — doctype, charset, `lang`, title, authors — and `--css` inlines a stylesheet |
| **H1** Python | `pip install ferrodoc` — one typed function, abi3 wheels for 3.9+ on Linux, macOS and Windows, GIL released so a thread pool overlaps. **72× a pandoc subprocess** per document |
| **H3** DOCX memory | the body streams, so its XML tree never exists in full: **2.7× less peak RSS and ~12% faster**, interleaved against a baseline |
| **H4** Verifiability | 14 gates in CI on three platforms against pinned pandoc, wasm32 build, 500k-mutation fuzz per run, `COMPATIBILITY.md` |
| **H4** Independent corpora | 7/8 DOCX and **8/8 ODT** documents *LibreOffice* wrote read identically to pandoc — the only evidence either reader generalises beyond pandoc's own output |
| Released | **0.1.0 is on crates.io** and `v0.1.0` is tagged on GitHub with binaries for Linux musl, both macOS architectures, Windows and wasm32 |

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
Peak RSS for docx → markdown: 6 MB / 117 MB / **1.12 GB** — what remains is
the AST and the source, not the XML tree. (Absolute figures drift ~2× between
sittings on this machine; only interleaved ratios are comparable.) This
paragraph is item 1 above.

---

## How a format gets added

The same five steps every time, which is what makes this repeatable rather
than a research project:

1. A reader and/or writer crate, or a module beside an existing one.
2. A variant in `Format`, wired into `parse`/`render` and the `--help` text
   in `main.rs` — until then it is unreachable by users.
3. A `diff-<format>` gate in `ferrodoc-harness` comparing against pandoc,
   added to `docs/gates.md`, `.github/workflows/ci.yml` and
   `COMPATIBILITY.md`. For a *binary* format both halves already exist and
   are generic over the format name: `diff_binary` (reader — read it, read
   it with pandoc, compare) and `diff_round_trip` (writer — ours through
   pandoc against pandoc's through pandoc). ODT reused both unchanged; EPUB
   should too.
4. A corpus: documents *another program wrote*, not our own output.
   `corpus/odt-libreoffice/generate.sh` is the pattern, and it is the half
   that finds the real bugs — a corpus of our own output cannot fail on a
   structure our writer never emits.
5. A row in `COMPATIBILITY.md` with its number and every deliberate
   divergence.

Four rules decide *which* formats, inside the ordering above:

- **The AST is the ceiling.** ferrodoc's AST *is* pandoc's, proven by
  `diff-ast` round-tripping any `pandoc -t json` document. Anything pandoc's
  AST cannot express, ferrodoc cannot carry either — slide layouts, tracked
  changes, bibliographic databases. The non-goals below are mostly those.
- **Match pandoc wherever it has a describable rule on well-formed input;
  diverge only where matching would mean reproducing a parse failure — or an
  exponential blowup.** This settled the empty-anchor and `<pre>`-newline
  questions in the HTML reader, and the ODT double-list-read.
- **No format ships without a differential gate.** What makes this project
  worth choosing is not the format count, it is that every one of them is
  checked against pandoc document by document. This is cheap to honour:
  pandoc reads `odt`, `epub`, `rst`, `latex` and `rtf` and writes all of
  them, so a new spoke can be gated on the day it is started.
- **Writers are bounded; readers are not.** A writer walks a finite AST and
  emits text — the work is escaping and a rule per node, and it is done. A
  reader must handle everything anyone has ever written in that format, plus
  malformed input, forever. Coverage is far cheaper on the writer side, and
  a reader is only worth it for a format people already *have documents in*.

---

## When to run `/iterate`

Not every change earns a builder–critic loop. The rule that decides it,
drawn from every finding a critic has found here that the gates did not:

> **Iterate when the failure mode is one the gates cannot see.**

The gates compare ASTs against pandoc. They are excellent at "this mapping
is wrong" and blind to everything else — and *everything else* is where the
expensive bugs have been:

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
| a nested ODT bullet list numbered in LibreOffice | every gate reads the AST, not the rendering |

So: **iterate** a change that writes a file another program must open, that
touches memory or the CLI surface, that adds a gate, or that can lose
content silently. **Skip it** for a change whose whole failure mode is a
differential number — a reader mapping rule is already watched by `diff-*`
far better than a reviewer would be.

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

## Smaller things worth doing when they block someone

None of these need a loop: each is a single rule with a gate that scores it.
They are promoted into `## Next, in order` by step 1 of the procedure — when
one blocks a real document or a real user — not by tidiness.

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
- Flat ODF (`.fodt`) — a single XML file with no zip around it — is refused
  today, and it is one branch in the ODT reader's `read`. The two remaining
  `corpus/odt` mismatches are *not* this: they are pandoc reading every list
  twice, a deliberate divergence recorded in `COMPATIBILITY.md`.
- A **stated** image size is read differently from pandoc, both measured
  while adding the image formats: `{width=100px}` is 100 points here and 75
  for pandoc, which counts a CSS pixel at 96 dpi; and giving only a width
  leaves the height at the image's own, where pandoc scales it to keep the
  aspect ratio. Intrinsic size — what the file itself says — already
  matches. `emu()` in `write.rs` is the whole of it.
- `restore_verbatim_newline` scans the source for `<pre>` without knowing
  whether that `<` sits inside another tag's attribute value.

## Explicit non-goals

Declared, not deferred — so nobody has to re-litigate them. Most of them are
the AST ceiling: pandoc's own AST cannot express them either.

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
