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
| **H1 Reachable** | callable from the language the pipeline is already in | the ecosystems that hold document pipelines can `install` it | Rust ✅ Python ✅ CLI ✅ JavaScript ✅ **C ABI ✅** (Go, JVM, C#, Ruby) — **met** |
| **H2 Sufficient** | the square covers what an editorial team actually holds | a team gets from what they hold to what they publish without reaching for pandoc once | markdown, GFM, HTML, DOCX, ODT, EPUB in ✅ LaTeX/PDF, RST, AsciiDoc out ✅ · **EPUB out ❌** |
| **H3 Trustworthy** | stated resource bounds that hold on *any* input | every path publishes a bound CI checks | never-panics ✅ deterministic ✅ bounded recursion ✅ peak RSS gated ✅ · **one superlinear path** |
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

## What "done" means

Every item below carries a **Done when** block. It is not a summary of the
work; it is the *test that decides*, and it is written before the work
starts so it cannot be fitted to the result afterwards.

Four rules make these hard to talk around. They exist because each failure
has happened here at least once:

1. **Every criterion is a command that exits non-zero, or a number one
   prints.** "Works well", "reads correctly" and "should be fine" are not
   criteria. If it cannot fail, it is not a criterion.
2. **Thresholds are set before measuring.** A gate whose threshold is chosen
   after seeing the score is a thermometer, not a gate. Where an item below
   states a number, that number is the commitment; scoring under it means
   the item is *not done*, not that the number was ambitious.
3. **Nothing else may regress.** `./scripts/verify.sh` must exit 0 at the
   end of every item, with no existing threshold lowered. Lowering one to
   land something is the single most available form of circumvention, and it
   shows up in the diff of that file.
4. **An external judge where one exists.** "Another program opens it", "the
   package installs from a clean machine", "pandoc reads it back" — an
   oracle that does not know what we intended. Self-graded output is how a
   package that built but could not import shipped once already.

And one that applies to every item without being repeated in each:

> **Done includes the paperwork.** A row in `COMPATIBILITY.md` with its
> number and every deliberate divergence, the gate in `scripts/verify.sh`
> and CI, the rules in the crate's `CLAUDE.md`, and the item moved out of
> this list into `## Where the bet stands` with the ranking re-run. Code
> that works and is undocumented is not done; it is a liability with a
> passing test.

If an item turns out to be a bad idea once measured, that is a real outcome:
say so, record what the measurement showed, and move it to `## Explicit
non-goals`. That is not circumvention — silently redefining the criterion
is.

---

## Next, in order

**Re-ranked after seven items landed.** H1 is met — every ecosystem that
holds document pipelines can install this — so rule 1 has nothing left to
promote, and the queue is what rule 2 and rule 3 leave.

1. **EPUB, write** (item 4b below) — the last gap in H2. A team can now
   get *out* of every format they hold, and into all but one.
2. **Close the HTML reader's 26 divergences** (`## Smaller things`) —
   promoted by evidence rather than taste: they are the *only* reason the
   EPUB reader misses two documents, so they now cost twice.
3. **PDF without TeX** — unchanged, and still waiting on a demonstrated
   need rather than an opinion.

Everything else on this page is done, parked with a measurement, or a
declared non-goal. When one of these lands, run step 2 again.

---

## What landed, and what each item cost

### ~~1. A published resource bound, and the one superlinear path~~ — landed, one criterion missed

*Rule 3: the guarantee was unchecked.*

**Shipped.** `bench-rss` measures peak RSS per path in a child process
(`VmHWM` is a high-water mark the kernel never lowers, so one process per
measurement), `scripts/verify.sh --limits` gates it, CI runs it, and
`COMPATIBILITY.md` publishes the table.

Boxing `Attr` and `Target` inside `Inline` took the type from **152 to 56
bytes** and cut peak memory **1.6–1.9× on every path** — every `Str` and
`Space` in a document pays the width of the widest variant, and `Link` was
setting it. The JSON is unchanged, which `diff-ast` proves at 100%.

| path | before | after |
|---|---|---|
| markdown → AST | 71.9× | **38.6×** |
| DOCX → markdown | 122.8× | **77.2×** |

**The pre-committed 20× was not met, and that is recorded rather than
adjusted.** The floor for holding a pandoc AST is about 38×: one heap
allocation per word, and no amount of boxing reaches it. The gate is set at
**85×** and is explicitly a *regression* bound, not the aspiration.

**The time criterion was also missed** — `docx → AST` grows ~20× for 10×
the input, against the 12× committed. But it was diagnosed, which is worth
more than the number: reading eight 1 MB files takes 4.74 s and one 8 MB
file 8.40 s, so the per-document cost is constant and the mapping holds no
quadratic. Cutting memory 1.6× left the curve unchanged, so it is not byte
volume either. **It is the number of live allocations** — which promotes
the item below.

### ~~2. One allocation per word~~ — measured, and parked with the reason

*Attempted, partly delivered, and then blocked by two of this project's own
constraints. Recorded here so nobody re-derives it.*

**What was done.** `Cite` and `RawInline` were the two variants still
setting `Inline`'s width; boxing them took it 56 → 48 bytes and the worst
path 77.2× → **73.8×**. Cumulatively this session: `Inline` 152 → 48 bytes,
`markdown → AST` 71.9× → **35.9×**, `docx → markdown` 122.8× → **73.8×**.

**Why the 45× criterion is not reachable.** Boxing has run out. The
remaining width is `Emph(Vec<Inline>)` and `Str(String)` at 24 bytes each,
and boxing *those* would add an allocation to the two commonest nodes in
every document — paying in the exact currency the item is trying to save.
Boxing everything else still leaves ~32 bytes, or roughly 65× on the worst
path.

What is left is the **allocation count**: ~1.7M `Str` values per 10 MB, one
heap allocation each. The three ways to fix that are all closed here:

- a small-string optimization needs a union or raw pointers, and the
  workspace is `unsafe_code = "forbid"` — a guarantee worth more than the
  memory;
- an existing crate (`compact_str`, `smol_str`) would do it safely, but
  every crate below the facade must build for `wasm32` with no C library,
  and adding a dependency to the *AST* is the heaviest place to add one;
- an arena, with `Str` holding a range into one buffer, changes the public
  type fundamentally and would not serialize without the arena beside it.

**So the honest state:** 35.9× is the floor for holding a pandoc AST in
this design, it is published in `COMPATIBILITY.md`, and CI holds the worst
path at 80×. Reopen this only with a measurement showing a specific
workload it blocks — that is rule 2, and it is not satisfied by "less
memory would be nicer".

### ~~3. A JavaScript package (wasm)~~ — landed, every criterion met

*Rules 1 and 2: a binding, and the only item that made something
impossible possible.*

`npm install ferrodoc` converts in a browser tab with **no document
leaving the client** — something pandoc cannot offer at any price.

| criterion | committed | measured |
|---|---|---|
| installs into an empty project and runs | required | ✅ CI, from the packed tarball |
| runs in a browser, no network request | required | ✅ headless Chrome over the DevTools protocol |
| one function, typed | required | ✅ `tsc --noEmit` against `ferrodoc.d.mts` |
| bad input throws, module still usable | required | ✅ Rust and browser tests |
| bundle size | **< 3 MB gzipped** | **0.59 MB** |

**Hand-written, with no `wasm-bindgen` and no `unsafe` block.** Buffers
stay owned by Rust in a handle table; JavaScript is told a handle and an
address and writes through its own view, so nothing rebuilds a slice from
a raw pointer. `no_unsafe_blocks_in_this_crate` holds it, and the crate
allows `unsafe_code` only for the `#[unsafe(no_mangle)]` attribute.

Three bugs the cheap tests could not have found:

- picking the loader by **URL scheme** sent a browser on a `file://` page
  down the Node path; only headless Chrome caught it;
- the declaration file as `.d.ts` rather than `.d.mts` left TypeScript
  treating the module as `any` while `tsc` still exited 0;
- caching the `Uint8Array` view reads zeros after any conversion large
  enough to grow the module's memory.

### ~~4a. EPUB, read~~ — landed; the writer is item 4b

*Rule 2 within H2: a publishing pipeline with EPUBs could not convert them
in process at all.*

| criterion | committed | measured |
|---|---|---|
| two corpora, one not pandoc's | required | ✅ `corpus/epub` and `corpus/epub-handmade` |
| independent corpus | **100%** | **3/3** |
| pandoc corpus | **≥ 90%** | **10/12 (83%)** — both misses are HTML reader divergences already counted in `diff-html-read` |
| an EPUB reader opens the output | required | ✅ `epubcheck` clean on the hand-authored corpus, in CI |
| spine order proved by a fixture where file order differs | required | ✅ `reversed-spine.epub` |

**83% against the 90% committed, and the gap is not the EPUB layer.** The
two failures are an unterminated HTML comment and a line break inside code
— members of the HTML reader's 26 known divergences, which this reader
inherits wholesale because an EPUB's content documents *are* XHTML. Closing
them means closing those, which is `## Smaller things` below, not this item.

The hand-authored corpus was worth more than the pandoc one, exactly as
`## How a format gets added` claims: it found that pandoc's EPUB reader
generates no heading identifiers at all, and that the per-file anchor is
named for the raw href rather than the decoded one. It also found two bugs
in the **HTML** reader that `diff-html-read` could not see.

### 4b. EPUB, write — `/iterate`

*H2. Reading shipped first because people have books they want as markdown
far more often than the reverse; that asymmetry is why this is a separate
item rather than half a finished one.*

Writing is the HTML writer plus a manifest, a spine and a nav document,
and chapters split at level-1 headings. The shape is known: pandoc's own
writer wraps each chapter's content in `<section>` divs, and this writer
must too, or every document differs at the first heading.

**Done when**

- `diff-epub-write` at **100%** on `corpus`, the same shape as
  `diff-odt-write` — ours through pandoc against pandoc's through pandoc.
- **`epubcheck` reports 0 errors** on every written book, in CI. It has
  already earned its place: it caught an incomplete `toc.ncx`, a missing
  navigation document and an unreachable non-linear item in the fixtures.
- A written book **round-trips through this crate's own reader** with the
  spine order intact, proved on a document whose chapter order would sort
  differently by file name.
- `Format::Epub` becomes writable, reaches `--help`, and `writable_format`
  stops special-casing it.

### ~~5. A LaTeX writer~~ — landed; PDF for anyone with TeX

`ferrodoc report.docx -t latex | pdflatex`, and the binary did not grow by
a crate. `-s` gives a whole document with a minimal preamble.

| criterion | committed | measured |
|---|---|---|
| the output compiles | required | ✅ `pdflatex -halt-on-error` on every corpus document, in CI |
| every special character has a fixture | required | ✅ and mutation-tested |
| in the README only after CI | required | ✅ |
| `diff-latex` ≥ 95% spec / 100% corpus | **not met — 1/11** | see below |

**The fidelity criterion was impossible and the number is not the point.**
Pandoc's *own* LaTeX round trip scores **0/11** on the same corpus: its
reader turns a code block with a language into two empty divs, drops a
link title, and derives a heading identifier where the document had none.
So the gate was changed to the one `diff-md` already uses — fidelity, with
pandoc's score printed beside it — and the real judge is `pdflatex`, which
is what anyone actually does with LaTeX.

### ~~6. A C ABI~~ — landed early, against the roadmap's own advice

*Rule 2 said wait: Go, JVM and C# pipelines can shell out today, so this
was inconvenient rather than impossible, and nobody had asked. It was
built anyway on instruction. Recording that here rather than pretending
the ranking chose it.*

| criterion | committed | measured |
|---|---|---|
| a header and a worked example in a non-Rust language, compiled and run in CI | required | ✅ `example/convert.c`, `-Wall -Wextra -Werror` |
| no leaks, no double frees | required | ✅ valgrind `--error-exitcode=1` in CI |
| no unwinding across the boundary | required | ✅ caught and returned as a failed conversion |
| `unsafe_code = "allow"` in the ABI crate only | required | ✅ the workspace still forbids it |

Plus one the crate imposes on itself: **every `unsafe` block is one
dereference wide**, checked by a test that fails the build otherwise. It
caught a four-line block in the test helper before it caught anything
else.

### ~~7. Writers for reStructuredText and AsciiDoc~~ — landed; judged by their toolchains

| criterion | committed | measured |
|---|---|---|
| `sphinx-build` accepts the RST | required | ✅ with `-W`, warnings as errors, in CI |
| `asciidoctor` accepts the `AsciiDoc` | required | ✅ `--failure-level=WARN`, in CI |
| both in `--help` and `Format::NAMES` | required | ✅ |
| `diff-rst` ≥ 90% spec / 100% corpus | **not met — 2/11** | pandoc manages 3/11 |
| `diff-asciidoc` ≥ 90% | **impossible** | see below |

**Pandoc writes AsciiDoc and cannot read it** — "Pandoc can convert to
asciidoc, but not from asciidoc" — so there is no oracle and no
differential gate can exist. That writer is judged by `asciidoctor` and by
tests holding the shapes a toolchain accepts and silently mis-renders.

RST's ceiling is the format: it cannot nest inline markup, and has no link
title and no strikeout. `sphinx-build -W` is the check that means
something, because a short title underline or a misaligned grid table is a
*warning* — and both mean the document is wrong rather than untidy.

### Later, and only for a demonstrated need: PDF without a TeX installation

Item 5 gives PDF to anyone who has TeX, at no cost. What remains is PDF for
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
