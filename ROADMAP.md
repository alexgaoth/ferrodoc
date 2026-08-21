# Ferrodoc roadmap

This is the forward plan for ferrodoc. It deliberately does not repeat the
compatibility ledger: [`COMPATIBILITY.md`](COMPATIBILITY.md) says what works
and what differs today; this file says what should change, in what order, and
what evidence makes an item done.

**Current planning baseline: 0.2.0.** Dates are intentionally absent. A date
would be a promise made without users; an exit criterion is a promise the
repository can verify.

## The product this is becoming

Ferrodoc is a small, safe, deterministic document-conversion engine for a
program that needs to convert *editorial documents* in its own process. The
canonical pipeline is:

```text
Markdown / GFM / pandoc-markdown ─┐
HTML                              ├─> Pandoc-compatible AST ─> selected output
DOCX / ODT / EPUB / ipynb         ┘
```

The current output set is CommonMark, GFM, HTML, DOCX, ODT, EPUB, Jupyter
notebooks, LaTeX, reStructuredText, AsciiDoc, pandoc JSON, and plain text.
The engine is reachable through Rust, the CLI, Python, browser/Node/edge
WASM, and a C ABI. Format crates are feature-gated so an embedder can pay for
the formats it uses.

The primary user is the **platform engineer**: a product, service, or client
application that needs document conversion without spawning pandoc. Batch
ingestion is an important secondary use, with the resource bounds below.
Documentation-site generation, a full pandoc command-line clone, and a
pixel-perfect office editor are not the product.

## How work is chosen

Work is ranked by these rules, in order:

1. A user cannot adopt a feature that cannot be installed, called, or run
   safely.
2. Silent information loss on a supported path outranks a cosmetic mismatch.
3. A real document or measured production limit outranks a speculative
   format.
4. A feature enters only with an oracle: a differential gate, an independent
   toolchain validator, or both.
5. A speed claim needs an end-to-end workload and a memory bound. A faster
   microbenchmark is not a priority by itself.

Every new failure is reduced to an allowable fixture, added to `corpus/` or
`samples/`, and kept by `scripts/verify.sh`. A score may not improve by
dropping the document that failed.

## Execution-card protocol

The phases below are outcomes, not single tasks. Execute them as **2–3 hour
cards**. A card may end in a commit, a reproducible measurement, or a written
design decision; it may not end as an unbounded partial rewrite.

Every card has five fields:

| Field | Rule |
|---|---|
| Scope | Touch one subsystem or one cross-cutting contract only. |
| Deliverable | Commit code/tests/docs, or record a measurement/decision with its command. |
| Verify | Name the exact command, independent consumer, or public-install check. |
| Done | State the observable result, not "implementation complete". |
| Not this card | Explicitly exclude adjacent work that would turn it into a multi-day project. |

If a card uncovers a larger design choice, commit its fixture or measurement,
write the decision needed, and stop. The next card starts from that evidence.
Cards below are the initial queue; a real user failure may pre-empt a lower
priority card under the selection rules above.

## The release sequence

### Phase 0 — Make 0.2 reachable

The code and packages are versioned at 0.2.0. The release is not complete
until every advertised install command resolves from its public registry,
rather than only from a CI artifact or a repository checkout.

- Publish the workspace crates in dependency order and tag the release.
- Publish the Python wheel through trusted publishing after the release is
  approved.
- Publish the npm WASM tarball with provenance after the release is approved.
- Attach the static CLI archives and the *actual* WASM package to the GitHub
  release; never ship the filesystem-less CLI wasm stub as the browser module.
- Correct package metadata whenever formats change: crate, PyPI, npm, C
  header, README, and `--version` must agree on the same release.
- Test a fresh public install on Linux, macOS, and Windows, then run one
  conversion through the installed artifact. Building an artifact is not an
  installation test.

#### Initial execution cards

| Card | Scope and deliverable | Verify and done | Not this card |
|---|---|---|---|
| R0.1 — release inventory | Audit version/tag/package names and release workflow inputs; add a checked release checklist if one is missing. | `cargo package --allow-dirty` for every publishable crate, `npm pack`, and `maturin build`; all artifacts report 0.2.0 and contain their expected entrypoint. | Publishing anything. |
| R0.2 — artifact smoke tests | Make release CI install the wheel and npm tarball from the release-built artifacts, not the source tree. | Fresh temporary Python and npm projects import the installed package and convert Markdown to HTML. | Registry publishing or API additions. |
| R0.3 — registry release | Owner publishes the approved release and records the immutable tag/release URL. | A clean machine resolves the three install commands in the Phase 0 criteria. | Changing converter behavior to make a release feel larger. |

**Exit criteria**

```text
pip install ferrodoc
npm install ferrodoc
cargo add ferrodoc@0.2
```

all resolve for the announced version; the installed Python and npm smoke
tests pass; release assets contain the advertised binary/module; and the
README no longer asks a user to build a package that it says is published.

### Phase 1 — Safe operation in services and browsers

The current architecture intentionally owns a Pandoc AST. That has a real
memory floor: on generated prose, the published guard is 80× input size up to
50 MB, and DOCX-to-markdown is the worst path. That is acceptable only when a
caller can decide whether to admit a document.

Build a single resource-limits model shared by CLI, Rust, Python, C, and WASM:

- maximum source bytes before parsing;
- maximum decompressed archive/part bytes and archive-entry count;
- maximum media bytes retained when a conversion needs media;
- maximum structural depth, blocks, inlines, tables, and output bytes;
- a typed, format-aware `ResourceLimit` error that explains which budget was
  crossed.

Defaults must be conservative for a public service and configurable for a
trusted batch job. They must reject before allocation wherever the format
makes that possible. ZIP-bomb and giant-media fixtures belong in the corpus.

Then decide whether a second, conversion-only API is warranted. A streaming
reader into a full AST reduces duplicated XML memory but cannot make a full
document AST bounded. If users need large DOCX-to-text/HTML conversion in a
256 MB worker, design an opt-in streaming output path; do not pretend
`parse() -> Pandoc` can provide that guarantee.

#### Initial execution cards

| Card | Scope and deliverable | Verify and done | Not this card |
|---|---|---|---|
| S1.1 — budget inventory | Measure source, decompressed-part, media, AST, and output sizes for every archive reader on normal and hostile fixtures. Write the results beside the existing RSS command. | Reproducible harness command prints all five values for DOCX, ODT, EPUB, and ipynb. | Enforcing limits or changing a parser. |
| S1.2 — facade limit contract | Design and add the smallest Rust `Limits`/`ResourceLimit` surface, initially enforcing source bytes before parsing. | Unit tests show over-limit input returns the typed error; existing facade tests and `scripts/verify.sh --quick` pass. | Archive accounting or binding plumbing. |
| S1.3 — archive admission | Apply entry-count and decompressed-byte budgets to DOCX, ODT, and EPUB before their content is retained. Add a zip-bomb-shaped fixture. | Each reader rejects its fixture with `ResourceLimit`; fuzz and reader gates still pass. | A full streaming writer. |
| S1.4 — surface parity | Carry the approved limits through CLI, Python, C, and WASM without changing their successful conversion behavior. | One limit-exceeded test per surface; binding checks (`--wasm`, `--c`, wheel tests) pass. | New binding APIs beyond limit configuration. |
| S1.5 — streaming decision spike | Prototype or measure one large DOCX-to-text path without a full AST, then write a decision. | RSS and output-equivalence measurements answer whether a streaming API earns a design phase. | Shipping a partial streaming API. |

**Exit criteria**

- Limits have the same semantics in every binding.
- Exceeding one returns a recoverable error, never a panic, OOM kill, or
  partially successful document.
- `bench-rss` has a CI bound for each supported conversion family.
- The documentation gives a supported size/memory envelope rather than only a
  benchmark ratio.

### Phase 2 — Trustworthy common-path fidelity

Ferrodoc should improve the supported paths it already has before adding a
new one. The current differential scores in `COMPATIBILITY.md` are floors,
not goals. Fixes are selected by impact and by whether they reproduce a
well-formed, non-deliberate semantic rule.

Priority order:

1. **Dialects that are named in the interface.** Keep CommonMark, GFM, and
   `pandoc_markdown` explicit; never silently treat a `.md` file as a wider
   dialect. Expand each dialect's corpus around metadata, attributes,
   footnotes, definition lists, tables, task lists, and math.
2. **EPUB's HTML mode.** EPUB content needs an explicit raw-HTML policy that
   can match pandoc's EPUB reader without weakening the standalone HTML
   reader. The existing divergence census identifies this as the largest
   actionable family.
3. **Office document semantics.** Address non-deliberate DOCX and ODT corpus
   gaps, beginning with lists inside table cells and the remaining nested
   writer case. Add files made by Word/LibreOffice as well as pandoc.
4. **HTML well-formed input.** Fix whitespace, CDATA, processing-instruction,
   and sectioning rules where pandoc has a stable rule. Preserve documented
   deliberate differences for malformed tags unless a user case justifies a
   policy change.
5. **Output validity before byte parity.** EPUB stays valid under `epubcheck`,
   LaTeX compiles, RST is accepted by Sphinx, AsciiDoc is accepted by
   Asciidoctor, notebooks pass `nbformat`, and DOCX/ODT open in independent
   office software. Do not raise a pandoc-parity score by emitting a document
   its native consumer rejects.

#### Initial execution cards

| Card | Scope and deliverable | Verify and done | Not this card |
|---|---|---|---|
| F2.1 — EPUB raw-HTML contract | Probe pandoc's EPUB reader and define the smallest internal HTML-read mode needed for raw HTML. Add only fixtures and a design note. | The fixtures demonstrate the mode difference and `diff-epub` failures are classified one by one. | Changing standalone HTML behavior. |
| F2.2 — EPUB raw-HTML implementation | Thread the approved mode through EPUB content parsing and preserve raw nodes only on that path. | `diff-epub` improves on the fixtures; `diff-html-read`, `epubcheck`, and samples do not regress. | Fixing unrelated malformed-HTML differences. |
| F2.3 — one office mismatch | Fix exactly one non-deliberate DOCX or ODT mismatch, beginning with the table-cell list or nested writer case. | Its named corpus document matches pandoc or gains a deliberate-difference entry with evidence. | General office style preservation. |
| F2.4 — dialect edge family | Add and fix one named Markdown/GFM/pandoc-markdown family (for example entity spaces or refdef-dash runs). | The corpus denominator grows and the relevant differential gate remains at its floor or rises. | Automatic dialect detection. |
| F2.5 — divergence census refresh | Re-run the census and reconcile `COMPATIBILITY.md`, `docs/divergences.md`, gates, and fixtures. | Every sub-100 score has a current, reproducible classification. | Changing behavior solely to raise a percentage. |

**Exit criteria**

- Every remaining divergence is classified as fixed, deliberately different,
  a format limitation, or blocked on a design decision.
- Every classification has a repro and a test; the divergence census and
  compatibility table agree.
- A real document used to expose a bug becomes a minimized fixture without
  losing the relevant shape.

### Phase 3 — Optimize the workloads that matter

The benchmark story is already unusually strong. Preserve it by optimizing
only after profiling a conversion path that users run, not a loop in
isolation.

The next investigation is **large DOCX read performance**. It is linear in
document structure but grows superlinearly per byte because a large document
creates many live AST allocations. Measure allocation count and retained
memory before changing representation; the prior DOM-streaming work proved
that removing a full tree can improve both time and memory.

Possible outcomes, in preferred order:

1. eliminate an identified repeated allocation, copy, or traversal;
2. retain fewer intermediate values when the requested output does not need
   them;
3. add an opt-in streaming conversion path for constrained services;
4. leave the AST representation alone when a proposed change would weaken
   standalone serialization, safety, or the public Rust API for a marginal
   gain.

Maintain workload benchmarks for tiny single documents, realistic office
documents, multi-document batches, and 1/10/25/50 MB generated inputs. Track
latency, throughput, peak RSS, binary/module size, and output validity. Keep
the comparison honest: subprocess cost is a real advantage for Python/Node
pipelines, but it is not a claim that the parser alone is 72× faster.

#### Initial execution cards

| Card | Scope and deliverable | Verify and done | Not this card |
|---|---|---|---|
| P3.1 — allocation profile | Profile 1/10/25/50 MB DOCX reads and attribute retained allocation count/bytes to concrete structures. Commit the command and findings. | A second run reaches the same diagnosis within normal timing variance. | Optimization. |
| P3.2 — one measured allocation fix | Remove one identified repeated allocation/copy/traversal and add a regression case if it was algorithmic. | Interleaved baseline shows improvement at two sizes; `bench-rss` and all gates remain within bounds. | Reformatting code or speculative preallocation. |
| P3.3 — output-only retention study | Measure the memory saved by not retaining media or AST data that a selected output cannot use. | Equivalence and RSS results decide whether an opt-in API is justified. | Altering `parse() -> Pandoc`. |

**Exit criteria**

- Any performance change names the workload, baseline, architecture, and
  measurement method.
- No path crosses its published RSS bound.
- A claimed large-document improvement is measured at more than one input
  size and against an interleaved baseline.

### Phase 4 — Stable embedding surfaces

The Rust facade is the semantic source of truth. The bindings should remain
thin adapters over it rather than forks with separate conversion behavior.

- **Rust:** preserve feature selection, explicit media handling, and typed
  errors. Add options as new types/functions rather than turning every simple
  conversion into a mandatory builder.
- **Python:** keep `convert` simple; add a richer API only when users need
  transformations that the JSON AST route cannot express. Keep GIL release,
  exceptions that survive process boundaries, type stubs, and abi3 support.
- **WASM/npm:** keep browser, Node, and edge tests separate. Enforce no
  network/document upload as part of the browser test, free every handle on
  both success and failure, and publish only the tested tarball.
- **C ABI:** version the ABI independently, preserve allocation/free rules,
  test malformed input and foreign-language ownership, and keep valgrind (or
  an equivalent) in the release checks.
- **CLI:** grow only high-value programmatic controls: explicit format and
  dialect choice, resource limits, media extraction, wrapping, metadata, TOC,
  and reproducible diagnostics. It is not a mandate to clone pandoc's hundred
  flags.

#### Initial execution cards

| Card | Scope and deliverable | Verify and done | Not this card |
|---|---|---|---|
| A4.1 — capability matrix | Generate or test the formats and feature-gated errors visible through Rust, CLI, Python, C, and WASM. | The same unavailable format produces a clear, format-specific error on every applicable surface. | Adding a new format. |
| A4.2 — binding contract test | Add one cross-language conversion fixture that covers text, binary output, invalid input, and media where the surface exposes it. | Rust result is the expected reference; Python, C, and WASM results match it. | Rich AST classes or asynchronous APIs. |
| A4.3 — semver release review | Inventory public Rust, C, Python, and TypeScript symbols before a release and write migration notes for any break. | A reviewer can identify every externally visible change from the release note alone. | Compatibility shims without an actual user need. |

**Exit criteria**

- Every public surface can perform the same supported conversions and reports
  unavailable feature-gated formats clearly.
- A release installs and smoke-tests every advertised binding from its public
  package, not from the source tree.
- API additions follow semver and include migration notes when a major change
  is unavoidable.

### Phase 5 — User-led scope, not format accumulation

No new format is automatically next. A candidate enters a design phase only
when all of these are true:

1. a target user has a workflow blocked by it;
2. the Pandoc AST can represent the semantic result without inventing a
   private shadow model;
3. there is a differential oracle or native validator and a corpus from more
   than one producer;
4. its dependency, binary-size, WASM, memory, and security costs are stated;
5. it can be feature-gated if it materially enlarges the default build.

Potential future work, explicitly **not commitments**:

- an optional PDF-output crate or feature, only with a rendering oracle and
  without making every converter pay for a typesetter;
- deeper EPUB/office preservation requested by real migration workloads;
- narrowly scoped writer improvements for a format that already has a reader.

PDF input/OCR, citations and bibliography processing, templates, Lua filters,
presentations, macro-expanding LaTeX input, static-site generation, reviewer
workflows, and pixel-perfect office layout remain outside this roadmap.

#### Initial execution cards

| Card | Scope and deliverable | Verify and done | Not this card |
|---|---|---|---|
| N5.1 — candidate intake | Record one user-blocked workflow, two representative documents, its producer, and its desired output. | The documents reproduce locally and are classified as a bug, configuration gap, or missing capability. | Implementing the requested format immediately. |
| N5.2 — format design gate | For one candidate capability, write its AST mapping, oracle/validator, corpus plan, feature/dependency cost, and resource risk. | All five Phase 5 entry conditions are answered; maintainers explicitly accept or reject it. | Starting implementation before the gate passes. |

## Continuous obligations

These are not milestones; they apply to every release.

| Area | Requirement |
|---|---|
| Correctness | Run `scripts/verify.sh`; run its fuzz mode for reader changes; keep the pinned pandoc version explicit. |
| Security | Reject hostile input within documented limits; no workspace `unsafe`; audit archive and media handling on every new format. |
| Documentation | Keep README, `COMPATIBILITY.md`, feature help, package metadata, and release notes consistent. |
| Corpus | Add minimized, license-safe real failures and identify their producer/version. |
| Performance | Report both time and RSS; re-measure large inputs after parser, AST, or media changes. |
| Releases | Treat registry publication, binary assets, and binding installation as separate deliverables with smoke tests. |

## What success looks like

At the end of this roadmap, ferrodoc is not "Rust pandoc." It is the
default embedded converter for the document families it supports: a service
or browser application can install it, select only the formats it needs,
enforce a resource policy, convert a real document deterministically, and
understand any remaining loss before it ships. New format work then follows
evidence from users rather than the size of pandoc's format list.
