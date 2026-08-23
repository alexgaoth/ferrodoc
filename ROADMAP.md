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

## What "indistinguishable from pandoc" can and cannot mean

The target for 1.0 is a converter someone can put where pandoc is and not
notice, except that it is faster, smaller and embeddable. That is worth
stating precisely, because stated loosely it is a claim no version could
ever satisfy.

**It cannot mean every format.** Pandoc reads 48 formats and writes 69;
ferrodoc reads 9 and writes 13. Closing that is neither achievable nor
desirable — the arithmetic is in this repository's own notes, and the short
version is that a converter's value here is that every format it does have
is checked against pandoc document by document. Adding formats without
oracles trades the only defensible property for a larger number.

**It cannot mean copying pandoc where pandoc is wrong.** Three current
divergences are deliberate and stay: this writer will not emit an EPUB
reference the book cannot satisfy (`epubcheck` rejects pandoc's book for
exactly that), it will not reproduce a parse failure that `tagsoup` produces
and `html5ever` does not, and it refuses a YAML metadata block it cannot read
exactly rather than guessing at the title.

**So the 1.0 claim is scoped, and the scope is the point:**

> For the formats ferrodoc supports and the command lines its surface
> covers, `ferrodoc` produces byte-identical output to `pandoc`, or fails
> loudly saying what it cannot do. Every remaining difference is enumerated,
> reproducible, and defended. Nothing differs silently.

That is checkable, and 1.0 is not reachable until it is checked.

### The measurement that decides it: the drop-in corpus

Today's gates score *ASTs and single conversions*. They cannot answer "would
this user notice". `scripts/dropin.sh` does, and **its number today is
`0/48`**:

- a corpus of **real command lines** — the invocations that appear in
  Makefiles, CI jobs and scripts, not synthetic ones;
- each run through both binaries against the same documents;
- output compared **byte for byte**, exit codes and stderr included;
- one published number, `N/M command lines identical`, with every miss
  classified as *fixable*, *deliberate*, or *out of surface*.

That number is the 1.0 release criterion. It replaces "feels compatible"
with a percentage that can fall — and starting it at zero is the point:
every other score in this repository was above 90% before anyone asked
this question.

---

## The version ladder

Each version states the claim that becomes true, what has to be built, the
test that decides it, and what it deliberately excludes. Cards from the
protocol above are the unit of execution inside each.

### 0.3 — Reachable

**Claim:** every install command in the README resolves from its public
registry.

The code is already at 0.2.0 and the dry run is clean; nothing here is a
converter change. This version exists because an unpublished binding
multiplies by zero, and because the repository currently advertises two
commands that 404.

- Publish crates.io **first** — `bindings/python` resolves `ferrodoc` from
  there, so the release cannot precede it.
- PyPI via trusted publishing; npm with `--provenance`; the *real* WASM
  module attached, never the filesystem-less CLI stub.
- **Migration notes for 0.1 → 0.2 ship with this release, not later.**
  `write_html_standalone` and `render_html_standalone` both changed shape.
  A break published without its note is a break twice.
- Every published number gets a command, and CI runs them. This project's
  most frequent defect is a claim wider than its evidence — seven such bugs
  in one week — and nothing currently gates the claim surface.

**Exit test:** on a clean Linux, macOS and Windows machine, `pip install
ferrodoc`, `npm install ferrodoc` and `cargo add ferrodoc` each resolve, and
one conversion runs through each installed artefact. Building is not
installing.

**Not this version:** any new flag, format or converter behaviour.

#### Cards

| Card | Scope and deliverable | Verify and done | Not this card |
|---|---|---|---|
| **R3.1 — dress rehearsal** | Build every publishable artefact from a clean checkout and confirm each reports 0.2.0 and contains its entrypoint. Record the commands in a release checklist. | `cargo package` for all twelve crates, `maturin build`, `npm pack`, `./bindings/wasm/build.sh`; each artefact reports 0.2.0, the wheel imports, the tarball exposes `convert`, and the module is the 1.8 MB one rather than the 31 KB CLI stub. | Publishing anything. Registry credentials. |
| **R3.2 — migration notes** | Write the 0.1 → 0.2 note: every changed public symbol in Rust, C, Python and TypeScript, with the before/after signature. | `git diff v0.1.0..HEAD` over the public surface names nothing the note omits. `write_html_standalone` and `render_html_standalone` both appear. | New API design. Deprecation shims. |
| **R3.3 — gate the claim surface** | Give every number published in `README.md` and `COMPATIBILITY.md` a command, and make CI run them. Start with the ones that have already gone stale: gate counts, bundle size, the 72× figure. | A script re-derives each published figure and fails when one drifts; it is wired into `verify.sh` and CI. | Changing any figure. Adding new claims. |
| **R3.4 — registry release** *(owner)* | Publish crates.io first, then the GitHub release, which triggers PyPI and npm. Record the immutable tag and release URL. | On a clean Linux, macOS and Windows machine, `pip install ferrodoc`, `npm install ferrodoc` and `cargo add ferrodoc` each resolve and convert one document. Building is not installing. | Any converter change made to enlarge the release. |

#### Where this stands, 2026-08-22

**R3.1, R3.2 and R3.3 are done.** R3.4 is the owner's.

An unplanned card came first, because rule 1 of this file outranks
everything below it: **`main` had been red since 2026-08-20**, in four
jobs, none of which reproduced locally on its own — `\verb` inside a
LaTeX command argument, a Windows path compared against its JSON
spelling, a gate whose binary nothing built, and a sphinx refusal that
pandoc's own output earns identically. All four are fixed.

R3.1 found the blocker the version bump created: **the wheel cannot be
built until crates.io has 0.2.0**, because `bindings/python` resolves
`ferrodoc` from the registry. `docs/releasing.md` is the checklist,
written from the rehearsal rather than from memory.

R3.3 found **nine published figures that had stopped being true**, every
one of them a corpus that grew, and README bundle sizes 1.4% out.
`scripts/claims.sh` now re-derives them and CI runs it.

> **R3.4 needs credentials and cannot be delegated.** Rotate the two tokens
> that were pasted into a chat transcript before using them, add the PyPI
> pending publisher (repo, `wheels.yml`, environment `pypi`), and set
> `NPM_TOKEN`. The `pypi` environment already exists.

### 0.4 — Drop-in command line

**Claim:** a pandoc command line either produces identical bytes or fails
loudly naming what it cannot do. It never produces *different* bytes
silently.

This is the version that makes the 1.0 claim measurable, and it is where the
deliberate CLI divergences have to be settled rather than defended
indefinitely:

- `--wrap` currently defaults to `preserve` where pandoc fills at 72. That
  default was chosen so a migration diff is readable, which was right while
  migrating. **Decide it as a compatibility question**: either match pandoc
  and keep the readable-diff behaviour behind a flag, or keep it and treat
  every wrapped output as a known, counted difference.
- `-f markdown` means CommonMark here and pandoc-markdown there. The dialect
  now exists as `pandoc_markdown`; decide whether `markdown` aliases it,
  with the same reasoning.
- `+ext-ext` syntax is refused by name today. Refusal is honest; **accepting
  the extensions the three dialects actually implement** is the drop-in step.
- The flags a real Makefile breaks on that are not yet present:
  `--defaults`, `--resource-path`, `--data-dir`, `--eol`, `--ascii`,
  `--strip-comments`, `--shift-heading-level-by`, `--id-prefix`,
  `--fail-if-warnings`, `--quiet`/`--verbose`/`--log`.
- Unknown flags fail with a message naming the flag and whether it is
  unimplemented or out of scope. Silence is the failure mode being removed.

**Exit test:** `scripts/dropin.sh` exists, is wired into `verify.sh`, and
publishes its number. Every miss is classified. No miss is "different
output, no message".

**Not this version:** templates, citations, highlighting.

#### Cards

**D4.1 comes first on purpose.** Every other card in this version is judged
by the number it produces, and this repository's own rule is that an
unchecked guarantee outranks a new feature. Building the flags first would
mean shipping eight of them with no way to say whether they helped.

| Card | Scope and deliverable | Verify and done | Not this card |
|---|---|---|---|
| **D4.1 — the drop-in corpus** | Collect **real** pandoc command lines — from Makefiles, CI jobs, README snippets, the pandoc manual's own examples — with the documents they run on. Commit them as data, not as a script. | At least 40 invocations, each recorded with its source, running under pandoc alone. A synthetic invocation nobody writes is not admitted. | Running ferrodoc against them. Any flag work. |
| **D4.2 — `scripts/dropin.sh`** | Run each corpus command line through both binaries, comparing stdout, any output file, exit code and stderr byte for byte. Print `N/M command lines identical` and classify every miss as *fixable*, *deliberate* or *out of surface*. | Wired into `verify.sh`; publishes its number; mutation-tested by breaking one known-good flag and watching the number fall. | Fixing any miss it finds. |
| **D4.3 — the `--wrap` decision** | Settle it as a compatibility question and write the reasoning: match pandoc's 72-column fill by default and keep `preserve` behind a flag, or keep `preserve` and count every wrapped output as a known difference. Implement whichever wins. | `dropin.sh` re-run: the wrap-related misses move to the chosen classification, and none is left unclassified. Existing gates hold. | The dialect question. Any other flag. |
| **D4.4 — the `markdown` dialect decision** | Same treatment for `-f markdown`: CommonMark here, pandoc-markdown there, with `pandoc_markdown` now available. Decide whether `markdown` aliases it, and write why. | `dropin.sh` re-run; `diff-spec` and `diff-gfm` unmoved, proving `commonmark` is still `commonmark` whatever the alias does. | Extension syntax. |
| **D4.5 — extension syntax** | Accept `+ext-ext` for the extensions the three dialects actually implement; refuse the rest **by name**, saying which dialect would have it. | A table test over every extension pandoc names: each is accepted, or refused with a message naming it. Silent acceptance fails the test. | Implementing a missing extension. |
| **D4.6 — diagnostics** | `--quiet`, `--verbose`, `--log`, `--fail-if-warnings`, and unknown-flag behaviour: fail naming the flag and whether it is unimplemented or out of scope. | Every unknown flag produces a message naming it; `--fail-if-warnings` turns the metadata-block warning into a non-zero exit. **No silent acceptance anywhere.** | Rewriting existing warnings. |
| **D4.7 — paths and defaults** | `--defaults`, `--resource-path`, `--data-dir`. These are what a Makefile actually carries. | A `--defaults` file setting from/to/wrap produces the same bytes as the equivalent flags; `--resource-path` resolves an image outside the document's directory. | Templates or `--reference-doc`, which are 0.5. |
| **D4.8 — text shaping** | `--eol`, `--ascii`, `--strip-comments`, `--shift-heading-level-by`, `--id-prefix`. | One literal-output test each, compared against pandoc on the same input. `--eol=crlf` is checked on bytes, not on a rendered diff. | Highlighting. Anything in 0.5. |

**Sequencing:** D4.1 → D4.2 first and in that order. D4.3 and D4.4 are
decisions with code attached and should not be started until the number
exists to judge them by. D4.5 through D4.8 are independent of each other and
can be taken in any order, or in parallel by two people.

#### Where this stands, 2026-08-22

**D4.1 and D4.2 are done, and the number is `0/48`.**

`dropin/` holds 48 real invocations — collected by GitHub code search
over Makefiles, CI files and shell scripts, 327 distinct lines collapsing
to 34 flag signatures, each row carrying the repository it came from and
what was altered to make it runnable here. `scripts/dropin.sh` runs both
binaries and compares every byte either wrote, stdout, output files and
stderr, and `verify.sh` prints the number. It is a *measurement* while it
is zero, because a floor of zero holds nothing; **it becomes a gate the
day it is not zero.**

Three findings, and each of them changes what 0.4 should do:

1. **Fifteen of the 48 fail on a flag ferrodoc does not have**, and the
   distribution is not what the card list assumed. In 327 real
   invocations: `--defaults` 36, `--template` 37, `--css` 23,
   `--toc-depth` 14, `--metadata-file` 12, `--variable` 7. And
   `--eol`, `--ascii`, `--strip-comments`, `--id-prefix` and
   `--shift-heading-level-by` — the whole of card **D4.8** — appear
   **zero** times. **D4.8 should be dropped to the bottom and D4.7
   widened**: `--css` and `--include-in-header` are already implemented
   or trivial, `--toc-depth` is a one-line extension of a flag that
   exists, and `--template` is a 0.5 item that a fifth of real command
   lines need.
2. **`-t html5` is refused as an unknown format.** Pandoc has spelled it
   `html5` for a decade. That is card D4.5's cheapest possible win.
3. **The misses that are not about flags come down to three global
   decisions**, and `scripts/dropin.sh --attribute` names which one each
   row needs: pandoc's default syntax highlighting (0.7), pandoc's
   72-column fill (D4.3), and `-f markdown` meaning pandoc's dialect,
   whose heading identifiers appear in every HTML conversion (D4.4).
   The rest need the standalone page shape, which is 0.5. **So the
   drop-in number is gated by 0.5 and 0.7 as much as by 0.4** — worth
   knowing before the version ladder is treated as an order of work.

### 0.5 — Templates, variables and standalone parity

**Claim:** `-s` output is pandoc's, and a user's own template works.

Standalone output is where "indistinguishable" is most visible and where
ferrodoc is currently furthest away: one fixed page shape against pandoc's
template language. Without this a large class of documentation and
publishing pipelines cannot move at all.

- `--template`, `-V`/`--variable`, `--include-in-header`,
  `--include-before-body`, `--include-after-body`, `--title-prefix`.
- The default templates for the standalone formats, matching pandoc's.
- `--reference-doc` for DOCX and ODT, which is how organisations apply their
  own styles and the single most common reason a team cannot switch.

**Exit test:** pandoc's own default template, fed to both binaries with the
same document and variables, produces identical bytes; a third-party
template from the wild does too.

**Not this version:** the full pandoc template language if it proves to need
a general interpreter — in that case, state the subset and refuse the rest
by name.

### 0.6 — Fidelity closure on the supported square

**Claim:** every gate is at 100%, or its gap is enumerated, reproducible and
defended. No unclassified difference remains.

The current floors are not goals; several are one bug away from 100 and
several encode real pandoc limitations. This version ends the ambiguity
between them.

| gate | now | 0.6 |
|---|---|---|
| `diff-html-read` | 635/661 | every miss fixed or in the divergence table with a repro |
| `diff-epub` | 10/12 | the raw-HTML mode decided, so the two are fixed or declared unreachable |
| `diff-epub-write` | 8/11 | the three deliberate cases stated as the whole remainder |
| `diff-docx` / `diff-odt` | 36/37, 32/34 | the non-deliberate misses fixed |
| EPUB spec chunks | 8/22 | resolved by the raw-HTML decision, or the gate retired as measuring the wrong thing |
| `scripts/sweep-epub-xhtml.sh` | 77 of 128 differ | **zero unrecorded**, which is the real number for the HTML reader |

**Exit test:** the sweep reports no divergence outside the recorded set, and
`docs/divergences.md` and `COMPATIBILITY.md` agree with the gates.

**Not this version:** new formats, performance work.

### 0.7 — Syntax highlighting

**Claim:** highlighted output matches pandoc's, or highlighting is off by an
explicit flag on both sides.

Pandoc highlights code by default; ferrodoc does not, and `diff-html`'s
652/652 is measured with `--syntax-highlighting=none` passed to pandoc. That
is disclosed and honest, and it is also the most visible difference a user
sees on their first conversion of a README.

- The `skylighting` token model, or a defensible subset with the languages
  named and the rest degrading to a plain code block.
- `--highlight-style`, `--no-highlight`, `--syntax-highlighting`.
- The size cost measured before it is accepted: a highlighter with 163
  language definitions is exactly the kind of dependency the wasm bundle
  cannot silently absorb. Feature-gate it.

**Exit test:** `diff-html` runs **without** `--syntax-highlighting=none` and
holds its floor.

**Not this version:** `--listings`, KaTeX/MathJax/MathML output modes.

### 0.8 — The resource contract

**Claim:** every surface enforces the same limits and reports the same typed
error, and the published bound holds for every supported conversion.

This work was drafted as the second thing to build, and is moved after the
embedders who need it. Rule 3 of this file says a measured production limit
outranks a speculative one; by 0.8 there are installed users to measure.

- Source bytes, decompressed archive bytes and entry count, retained media,
  structural depth, output bytes.
- A typed `ResourceLimit` naming the budget crossed, identical in Rust,
  CLI, Python, C and WASM.
- Zip-bomb and giant-media fixtures in the corpus; rejection before
  allocation wherever the format permits it.
- The streaming decision made and written down, either way.

**Exit test:** every binding returns the same error for the same over-limit
input; `bench-rss` has a CI bound per conversion family; the documentation
states a size envelope, not only a ratio.

**Not this version:** a partial streaming API shipped to meet a date.

### 0.9 — The scope decision, and the last gap

**Claim:** what 1.0 does not do is written down and defended, not left
implied.

One item decides how wide "indistinguishable" reaches, and it is a decision
rather than a task:

> **Citations.** CSL processing plus five bibliography readers is, by this
> repository's own estimate, the size of everything built so far. Including
> it makes ferrodoc a drop-in for the academic pipelines that are a large
> share of pandoc's users. Excluding it makes 1.0 reachable and honest, with
> citations the single named exception.
>
> **Recommendation: exclude, and say so in the 1.0 claim.** Ship it as 1.1
> if the drop-in corpus shows real command lines failing on `--citeproc`.
> The claim "indistinguishable for document conversion; citations are the
> named exception" is one a user can act on. "Indistinguishable" with an
> unstated hole is the kind of claim this project spends its time deleting.

Lua filters are excluded by the same reasoning, with the JSON AST as the
supported escape hatch — `-t json | your-filter | -f json` works today and
covers what most filters do.

**Exit test:** the exclusion list is in the README, each entry with the
workaround if one exists.

### 1.0 — Indistinguishable, on a scope that is written down

**Claim:** the sentence at the top of this section, with a number behind it.

**Exit test — all of these, or it is not 1.0:**

- the drop-in corpus is **≥ 95% byte-identical**, and every miss is
  classified *deliberate* or *out of surface* — none is *fixable*;
- every differential gate is at its stated floor or above, and every gap
  below 100% appears in `COMPATIBILITY.md` with a reproducing command;
- the sweep reports zero unrecorded divergences;
- `pip install`, `npm install`, `cargo add` resolve, and each installed
  artefact converts a real document on Linux, macOS and Windows;
- the published resource bound holds for every supported conversion at every
  measured size, with the envelope stated;
- the efficiency claims are re-measured on the release build and still hold:
  faster per document than a pandoc subprocess, smaller on disk, less peak
  memory, deterministic bytes, and running where pandoc cannot run at all;
- the exclusion list is published.

**What 1.0 is not:** all 48 input formats, citations, Lua filters,
templates beyond the stated subset, PDF input, or a promise about pandoc
releases after the pinned 3.8.2.1.


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

At 1.0 ferrodoc is not "Rust pandoc", and the difference is the point.
Pandoc is forty-eight readers, a filter runtime, a template language and a
citation processor. Ferrodoc is a smaller thing done exactly: for the
document families it supports, a user replaces `pandoc` with `ferrodoc` and
gets the same bytes — or a clear message saying what it will not do — while
paying a fraction of the time, size and memory, and reaching places pandoc
cannot go at all.

Two properties carry that, and both are checkable rather than felt:

- **nothing differs silently.** Every difference is either byte-identical,
  or a loud failure, or a row in `COMPATIBILITY.md` with a command that
  reproduces it. The drop-in number is what keeps that true, and it can
  fall;
- **every claim has a command.** The efficiency figures, the fidelity
  scores, the resource envelope and the install lines are each backed by
  something CI runs. This project's most expensive bugs were not wrong code
  but claims wider than their evidence — seven in a single week — and 1.0 is
  the version where that class is gated rather than caught.

After 1.0, format work follows users. A candidate enters through the five
conditions above, not through the length of pandoc's `--list-input-formats`.
