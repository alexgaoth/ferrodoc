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
`4/48`** — it was `0/48` when the corpus was collected:

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
| **D4.3 — the `--wrap` decision** *(decided)* | **Match pandoc, in stages.** See below. | `dropin.sh` re-run; the three modes tested against every writer class. | The dialect question. Any other flag. |
| **D4.4 — the `markdown` dialect decision** *(decided)* | **No alias, and here is the number.** See below. | `diff-pandoc-md corpus` gates the reader at its measured 30%; `diff-spec` and `diff-gfm` unmoved. | Extension syntax. |
| **D4.5 — extension syntax** *(done)* | Accepted where the named dialect already does it; refused by name otherwise, saying which of the three reads it. A name pandoc does not have is a typo and says so — checked before the no-op test, which had accepted `-nothing`. | `extension_syntax_is_accepted_when_it_asks_for_nothing` in `main.rs`; `dropin-008` now runs with the `markdown_github-hard_line_breaks` its source actually wrote. | Implementing a missing extension. |
| **D4.6 — diagnostics** | `--quiet`, `--verbose`, `--log`, `--fail-if-warnings`, and unknown-flag behaviour: fail naming the flag and whether it is unimplemented or out of scope. | Every unknown flag produces a message naming it; `--fail-if-warnings` turns the metadata-block warning into a non-zero exit. **No silent acceptance anywhere.** | Rewriting existing warnings. |
| **D4.7 — paths and defaults** *(done)* | `--defaults` splices its flags in **where the flag appeared** (pandoc's precedence, measured both ways round); `--resource-path` is searched after the document's own directory; `--data-dir` supplies `templates/default.html5` and names a `--template`. A key with no flag behind it is refused by name. | The seven `--defaults` rows in `dropin/` run; `--resource-path` embeds a picture neither binary finds without it. | `--reference-doc`, which is 0.5. |
| **D4.8 — text shaping** *(done)* | All five, plus `--metadata-file`. `--ascii` is HTML-only and refuses the other writers by name — pandoc spells the escape differently in each. | `./scripts/flags.sh`, **224/224 byte-identical**, gated at 100 in `verify.sh`. `--eol=crlf` is compared on bytes. | Highlighting. Anything in 0.5. |

#### D4.3 — decided: match pandoc, and the default cannot flip yet

The card offered two answers: match pandoc's 72-column fill, or keep
`preserve` and count every wrapped output as a deliberate difference.
**Match pandoc** — a converter whose selling line is "put it where pandoc
is" cannot reflow every paragraph of every text conversion and call it a
decision. `ferrodoc -t gfm --wrap=auto --columns=72` already produces
pandoc's default output byte for byte, so nothing is being promised that
has not been measured.

But measuring the flag first turned up three things that had to be fixed
before any default could be flipped, and they are worth more than the
flip:

1. **ferrodoc has no single wrap default.** `html` and `plain` join every
   soft break into a space — pandoc's `--wrap=none` — while `markdown`,
   `gfm`, `latex`, `rst` and `asciidoc` keep the document's breaks, which
   is `--wrap=preserve`. Seven writers written separately, and `README.md`
   claimed one behaviour for all of them. `samples/generate.sh` had
   discovered it empirically — it runs pandoc **both ways and keeps the
   closer output** — without anyone naming what it had found.
2. **`--wrap=none` and `--wrap=preserve` were the same value.** They are
   not the same thing: `none` joins, `preserve` keeps. On the one writer
   that could tell them apart, ferrodoc did `preserve` for both — an
   explicit flag, accepted, doing something else.
3. **`--wrap=auto` was a silent no-op for five of seven writers**, and it
   dropped embedded media on the way past, because the wrapped path
   called the writer that takes no resolver.

All three are fixed: `Format::wrapping()` states what each writer does,
`Wrap` has pandoc's three modes, and a writer that cannot honour the mode
asked for **returns an error naming what it does instead** rather than
the layout it already had.

**So the remaining work is filling, not deciding.** The default becomes
`auto` at the version where `html`, `plain`, `latex`, `rst` and
`asciidoc` can fill to a column — one card each, and the HTML one is the
one worth doing first because it is most of the drop-in corpus.

#### Done, 2026-08-24 — **the default is `auto`**

All five fill, one commit each, in that order. Each is byte-identical to
pandoc over the twelve documents in `corpus/` and `corpus/gfm/` at
several column widths: html 60/60 at five widths, plain 48/48, rst 44/48,
asciidoc 43/48, latex 40/48 — and every remaining miss is either a
recorded deliberate divergence or a narrow width no default uses.

The mechanism is the same everywhere and worth stating once: **the writer
marks its break opportunities as it writes and one function decides what
becomes of them**, so nothing downstream is told which mode it is in.
What differs per writer is where the marks go, and that is where the
measuring went:

- HTML breaks **between a tag's attributes**, needs a mark that ends what
  a break decision may look at (a code block's content is appended
  whatever its width), and counts **display columns** — a table of 78
  ranges, measured codepoint by codepoint rather than transcribed;
- LaTeX needs a **region** mark: a `\footnote{…}` indents its wrapped
  lines by two and returns to the paragraph's column after the brace,
  which nothing in the finished text says;
- RST continues a wrapped item **under its content** and cannot split
  inline markup; AsciiDoc continues **flush with the marker** and can.
  The two are opposite, and each was found by the other's rule failing.

`dropin.sh` went **4/48 to 8/48** the day the default flipped, and the
fill left its attribution entirely: what `--attribute` blamed on 23 of 44
misses is gone, leaving the dialect (18) and highlighting (9).
`samples/` went from four byte-identical to **ten of fifteen**, and
`samples/generate.sh` no longer runs pandoc twice and keeps the closer
output — a workaround it needed for exactly this.

#### 0.5 — a licence fact that decides how far `-s` can go

Byte-identical standalone HTML means reproducing **pandoc's default
template and its default stylesheet**: a 70-line template and a 212-line
`templates/styles.html`, both printable from the pinned binary
(`pandoc --print-default-template=html5`,
`pandoc --print-default-data-file=templates/styles.html`). That is 174 of
the 176 lines by which `ferrodoc -s -t html` differs from
`pandoc -s -t html` today; the other two are `<html lang=…>` and the
`<title>`.

Pandoc is GPL-2.0-or-later, so the question is whether that is even
available to an MIT/Apache-2.0 project. **It is:** pandoc's `COPYRIGHT`
carves the templates out —

> Pandoc's templates (in `data/templates`) are dual-licensed as either
> GPL (v2 or higher, same as pandoc) or (at your option) the BSD 3-clause
> license.

and `templates/styles.html` is one of those files. BSD-3 is compatible
with this project's licences and needs its notice carried.

**That is the owner's decision, not a technical one**, and it is what 0.5
turns on: taking pandoc's template and stylesheet under BSD-3 with
attribution makes `-s` byte-identical and reachable; writing a page of
one's own means `-s` output can never match, and the 1.0 claim has to
name standalone HTML as an exception the way it names citations.

#### What the drop-in number is waiting on now

Updated 2026-08-24, after the writers were brought to pandoc's bytes and
the wrap default flipped. **`8/48`, with no refusals at all**, and —
the number that matters more — `scripts/dropin.sh --attribute` puts
**34 of the 37 misses** on one of two remaining global decisions:

| decision | rows it alone would fix |
|---|---|
| ~~pandoc's 72-column fill (D4.3)~~ | **done** — it was 23 |
| `markdown` meaning pandoc's dialect (D4.4) | 22 |
| ~~syntax highlighting (0.7)~~ | **done for C, Python and bash** — 2 |
| both together | 10 |

The dialect row counts the **writer** side since 2026-08-25 — `-t markdown`
gets pandoc's dialect on the way out as surely as `-f markdown` does on the
way in, and the experiment had only ever neutralised the reader — and it
counts `markdown_github`, the same hypothesis under pandoc's own deprecated
name.

**The remainder is 2**, plus one difference this project keeps on purpose:
a `--reference-doc` DOCX and a LaTeX document. It was 19 when this section
was written and 7 on the morning of 2026-08-25; what emptied it was not
conversion work but the experiment learning to switch a feature off on
**both** sides. Muting pandoc alone had been blaming rows on differences
that no longer existed — and in one case on a deprecation warning that
vanished from pandoc's stderr the moment the dialect was neutralised. That bucket is the useful signal: it is a
**third** cause rather than more of the first two, and nobody has looked
at what it is.

So the order that follows from the measurement is: **the dialect, then
highlighting, then read the nineteen**. Neither of the two is a flag any
more; each is a body of work with a card.

#### D4.4 — decided: `markdown` does not alias `pandoc_markdown`, yet

The card asked whether `-f markdown` should mean pandoc's dialect here as
it does there. The answer is a measurement, and it was not available
until this card: the `pandoc_markdown` reader agrees with
`pandoc -f markdown` on **6 of 20** markdown documents in `corpus/`.

Aliasing a reader that disagrees with pandoc on two thirds of a corpus
would move the difference from *a name you have to type* to *every
conversion you already run*, and it would do it silently. `markdown`
stays CommonMark until the number is close to 100.

The gate that said `3/3` was the shape of the problem, not evidence
against it: three fixtures written for this reader, scoring what a corpus
of one's own constructs always scores. `verify.sh` now runs both — the
hand-authored three at 100, because a fixture that starts failing is a
regression, and every markdown document under `corpus/` at its measured
30%, because that is the number that says how far the dialect is.

**What the 14 failures are**, from the widened run: `smart` quotes
(pandoc's `markdown` turns `'` into `’` by default and this does not),
`implicit_figures` (an image alone in a paragraph is a `Figure` block
there and a `Para` here), a code span inside a table cell, and one YAML
metadata block this reader refuses — `abstract: |`, a block scalar
outside the subset it reads. Each is a card, and `smart` is the one that
touches nearly every prose document.

**One thing the widening fixed on its own:** the gate used to *abort* on
a refused document rather than count it, so a single `abstract: |`
stopped the other nineteen being measured at all.

#### Where this stands, 2026-08-24 — `smart` is done, and the denominator grew

`smart` was the one named as touching nearly every prose document, and it
is read now: dashes, the ellipsis, the apostrophe, and — the half comrak
does not do — a *pair* of quotes as a `Quoted` element.
`COMPATIBILITY.md` carries the three pairing rules and the probe behind
each. `implicit_figures` followed it, which is five more measured shapes
and the second-largest name on the list.

**Twenty documents cannot say how far a dialect is**, which is the corpus
blind spot in its usual costume, so `diff-pandoc-md` now reads a
`spec.json` as well and `verify.sh` gates the reader over the CommonMark
spec: **445/652**, up from 417, with **twenty-eight gained and none
lost**, checked example by example against the trees the reader produced
before. The corpus run went 6/20 to 9/20 and both floors rose.

`implicit_figures`, then three that comrak already parses and whose
attributes only had to be read — `link_attributes`,
`inline_code_attributes` and `inline_footnotes` — take the corpus run to
**10/20**.

**The HTML-block card is done (2026-08-24), and it was worth what the
measurement said.** Pandoc's markdown writes one `RawBlock` per
block-level tag and reads what lies between two of them as markdown;
`native_divs`, `native_spans`, the four verbatim elements and the
`Plain`-versus-`Para` rule sit under it. Seven measured rules,
`COMPATIBILITY.md` carries each with its probe. **HTML blocks went 43/44
failing to 7/44 and the reader 445/652 to 488/652 — 43 gained, none
lost**, checked example by example against the trees it produced before.

There was no second shape available: pandoc's tree holds one raw block
per tag, and the gate compares trees.

The rest of the list is bracketed spans `[text]{#id}`, block scalars in
a metadata block, a multi-word fence info string, `***x***` nesting the
other way round from CommonMark, and what pandoc's markdown does with a
GFM document. The last two need the *source* rather than the tree, which
is the wall `COMPATIBILITY.md` already records for
`[http://x](http://x)`.

**What is left after the HTML card is the parser, and no card closes
it.** Emphasis 40/132, links 26/90, setext headings 16/27, and pandoc's
rule that a heading, a list or a block quote needs a blank line in front
of it — `a\n# H` is one paragraph there and two blocks here. Pandoc's
markdown is not CommonMark plus a feature list, and comrak is a
CommonMark parser. So the exit test for D4.4 — "until the number is
close to 100" — should be re-read as *close to 100 on what a dialect
built this way can reach*, and the honest way to say that is the section
table rather than the total.

**Which makes the aliasing decision a different question than it was.**
`markdown` still does not alias `pandoc_markdown`, and the reason is
unchanged: 496/652 is not 100, and a silent change of meaning on a
quarter of documents is worse than a flag someone types. What has
changed is that the remaining quarter is now *named* — it is the four
parser sections above, not an unread list.

#### Where the dialect stands, 2026-08-24 (end of day)

Seven cards closed in one pass, each measured against the pinned binary
and each with its own test:

| | corpus | spec |
|---|---|---|
| where it started | 6/20 | 417/652 |
| `smart` — dashes, apostrophe, and the `Quoted` pairing | 6/20 | 429 |
| `implicit_figures` | 9/20 | 445 |
| inline attributes, `^[…]` notes | 10/20 | 445 |
| **raw HTML — one `RawBlock` per tag, divs, spans, verbatim** | 10/20 | **488** |
| heading identifiers (pandoc's slug, not GitHub's) | 10/20 | 491 |
| a code span is trimmed | 10/20 | 496 |
| bracketed spans `[t]{#id}` | 11/20 | 496 |
| metadata block scalars (was a **refusal**) | 12/20 | 496 |
| fence attributes, task items, footnote `Para` | 13/20 | 496 |
| an unclosed fence and an unclosed comment are text | **14/20** | **498** |

**Not one example regressed**, at any step: every change was checked
example by example against the trees the reader produced before it.

What is left is the parser, and the section table above says so. **Every
bucket was then sampled example by example, and none of them is one
rule**: the largest coherent cause left is pandoc's setext heading —
which wants a one-line paragraph and an unindented underline — and it is
worth about five examples; the blank-line-before-a-block rule is worth
about four; `***x***` nesting the other way is worth two and needs the
source rather than the tree. The rest are one apiece.

So the next honest question is not "which card next" but **whether 76% is
the ceiling worth living with** — because comrak is a `CommonMark`
parser and these are pandoc's parser. Answering it means either accepting
the ceiling or writing a second reader, and that is a decision, not a
card.

**Sequencing:** D4.1 → D4.2 first and in that order. D4.3 and D4.4 are
decisions with code attached and should not be started until the number
exists to judge them by. D4.5 through D4.8 are independent of each other and
can be taken in any order, or in parallel by two people.

#### Where this stands, 2026-08-22

**D4.1 and D4.2 are done, and the number was `0/48` when they were.**

`dropin/` holds 48 real invocations — collected by GitHub code search
over Makefiles, CI files and shell scripts, 327 distinct lines collapsing
to 34 flag signatures, each row carrying the repository it came from and
what was altered to make it runnable here. `scripts/dropin.sh` runs both
binaries and compares every byte either wrote, stdout, output files and
stderr, and `verify.sh` prints the number. It was a *measurement* while
it was zero, because a floor of zero holds nothing; it became a gate the
day it was not, and the floor rises with the number — **4 as of
2026-08-23**.

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

#### Where this stands, 2026-08-24 — **10/48, and one decision left**

The floor is 10. Four things moved it since the fill landed, and none of
them was a card:

| | what it was | rows |
|---|---|---|
| the attribution's own guard | it skipped the dialect hypothesis on any command line that spelled `-f` out loud, which is most of them | 9 misfiled |
| `<div>` for a section div | pandoc's html5 writes `<section>`, so every EPUB and DOCX differed on every heading | 1 |
| `\!\[` for `\![` | the escaped `!` is already what stops the image | 1 |
| `-H`/`-B`/`-A` not implying `--standalone` | a Makefile got a fragment where pandoc writes a page | — |
| metadata not reaching the template | `-M pagetitle`, `-M linkcolor` | — |

**The remainder is no longer a third cause.** With the guard fixed, the
attribution reads: the dialect 17, the dialect with highlighting 10,
highlighting alone 3, one deliberate, **seven left** — and six of those
seven are the same dialect decision from an angle the counterfactual
cannot model, because it works by handing pandoc `-f commonmark` and
CommonMark cannot say what they need. Two name the dialect
`markdown_github`, one is a `.pmd` written in it, and **three ask
`-t markdown` to write it** — which is the half of D4.4 the card does not
mention. The seventh is `--reference-doc`, which is 0.5.

**So the order is settled by measurement rather than by taste: D4.4,
then 0.7, and there is nothing else to read.** COMPATIBILITY.md carries
the table; `claims.sh` holds both the number and the flag figure to the
commands that derive them, which nothing did before.

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

#### Where this stands, 2026-08-24 — **the exit test passes**

```console
$ ./scripts/flags.sh
224/224 flag combinations byte-identical
```

Every flag over every document in `corpus/`: `-s`, `--toc`,
`--toc-depth`, `--css`, `-V`, `-M`, `--metadata-file`, `--title-prefix`,
`-H`, `-B`, `-A`, and a third-party `--template` — gated at 100 in
`verify.sh`, because reproducing a template is not something one gets
90% right.

Two rules were added on 2026-08-24 and both came from the drop-in
corpus rather than from the card: **`-H`, `-B` and `-A` imply
`--standalone`** (nothing else here does, measured one flag at a time),
and **a template reads the document's own metadata as variables**, so
`-M pagetitle=Home` names the page and `-M linkcolor=…` colours its
links, with `-V` beating `-M`. The figure is under `claims.sh` now; it
had said 144/144 in COMPATIBILITY.md since the corpus was that size.

**The licence question is settled and it is what made this reachable.**
Pandoc's `COPYRIGHT` dual-licenses everything in `data/templates` as GPL
**or BSD-3-clause**; the template and `styles.html` are vendored in
`crates/ferrodoc-html/templates/` under the BSD option, with the notice.
Writing a page of one's own instead would have meant `-s` could never
match, and 1.0 naming standalone HTML as an exception.

The template language is a **stated subset** — `$var$`, `$if$`, `$for$`,
`$sep$`, `$partial()$` — and anything outside it is refused by name, which
is what "Not this version" above asked for.

**`--reference-doc` is done, for DOCX and ODT**: the styles parts come
from the reference and nothing else does, which is what the flag is for
and all a self-consistent package can take. With it, **every one of the
48 command lines in `dropin/` runs** — the corpus has no refusals left.

**Still 0.5:** the default templates for the standalone formats other
than HTML — `samples/07-markdown-to-latex` is 141 lines of difference and
almost all of it is pandoc's LaTeX preamble. What `-s` HTML still differs
on is **syntax highlighting**, which is 0.7.

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
| `diff-html-read` | 641/661 | every miss fixed or in the divergence table with a repro |
| `diff-epub` | **11/12** | the raw-HTML mode decided, so the two are fixed or declared unreachable — **measured 2026-08-25**, below |
| `diff-epub-write` | 8/11 | the three deliberate cases stated as the whole remainder |
| `diff-docx` / `diff-odt` | **37/37**, 32/34 | ~~the non-deliberate misses fixed~~ — **done 2026-08-25**: the one DOCX miss was an empty list paragraph breaking a list in two; both ODT misses are the declared `G7` |
| EPUB spec chunks | **11/22** | resolved by the raw-HTML decision, or the gate retired as measuring the wrong thing — the measurement says the second |
| `scripts/sweep-epub-xhtml.sh` | 12 of 128 differ | **zero unrecorded**, which is the real number for the HTML reader |

#### The raw-HTML mode, measured rather than argued — 2026-08-25

The two rows above have waited on one question since this card was
written, and it was stated as a choice about *output shape*: whether an
EPUB read here should carry raw HTML the way `pandoc -f epub` does.
Measured over the real chunks, that is not what the extension does.

`pandoc -f epub` emits **3 raw nodes out of 186** on `spec-00` and
**none** on `corpus-readme-style.epub`. Ordinary `<em>` and
`<div class="foo">` are structured with the extension on exactly as with
it off. Every raw node across all twelve failing chunks is an unclosed or
unknown tag (`<foo>`, `<bar attr="](baz)">`, `<5001 foo>` — plus the
`<p>`/`<a>` around it, because pandoc stops structuring the whole block),
a stray `</div>`, or an HTML comment.

So `raw_html` is **parse-failure recovery with a wider reach**, and this
gate is measuring the same thing the HTML reader's remaining twenty
measure — how two parsers repair a malformed document. That is the one
place this project has a standing decision to diverge, which makes the
honest resolution *retire the claim that these chunks measure the reader*
rather than build a mode to chase them.

Two details decide how far it could go if the owner wants it anyway:
**the HTML comment is reachable** — it is a real DOM node, and
`spec-10`'s only raw nodes are two of them — and **the stray `</div>` is
not**, because `html5ever` discards an unmatched end tag before this
crate is handed a tree, exactly as it drops `ElementFlags::self_closing`.
`docs/divergences.md` has the per-chunk table.

#### The writer half of this version is **done**, 2026-08-23

`./scripts/writers.sh` compares each text writer against **pandoc's own
writer** on the same AST, byte for byte — the comparison the fidelity
round trips could never make for a format pandoc does not read back. The
card was to choose a floor per writer and gate it. Both halves happened,
in that order:

| writer | was | now | floor |
|---|---|---|---|
| `html` | 8/8 | **38/40** | 38 |
| `rst` | 2/8 | 34/40 | 34 |
| `plain` | 5/8 | **38/40** | 38 |
| `latex` | 0/8 | 36/40 | 36 |
| `asciidoc` | 2/8 | **38/40** | 38 |
| `gfm` | 3/8 | 28/40 | 28 |
| `commonmark` | — | 29/40 | 29 |
| `markdown` | 1/8 | 6/40 | 6 |

The corpus grew from eight documents to twenty, each written twice, on the way, and that is
the part worth carrying forward. Four are read as **GFM**, because
`CommonMark` has no table, no task list and no footnote, so a score over
the original eight could not see the constructs the writers were worst
at; the first run of that wider corpus found the HTML writer — at
`diff-html` 652/652 — **dropping every footnote**.

The last eight are **this repository's own prose**, added 2026-08-25:
README, ROADMAP, COMPATIBILITY, `docs/` and `samples/README.md`, 4,440
lines that exist to be read rather than to be converted. They scored
**asciidoc 0/8 and rst 1/8** against writers sitting at 11/12 and 12/12
on the fixtures. Five real bugs came out of that in one sitting, and one
was a broken fence in `README.md` itself — twelve fixtures written to be
converted had said nothing about any of them.

Each floor is the score that writer reached, because every point below
one is a document that used to be byte-identical and is not any more.

`markdown` stays low for a stated reason rather than an unstated one:
`-t markdown` is `CommonMark` here and pandoc's own dialect there, so
that row measures the dialect gap on the writer side and moves when
D4.4 does.

**Exit test:** the sweep reports no divergence outside the recorded set,
`docs/divergences.md` and `COMPATIBILITY.md` agree with the gates, and
every writer above has a floor. **The third is met**; the sweep and the
reader gates in the table above are what remains.

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

#### Where this stands, 2026-08-24 — **C, Python and bash ship; the method is (c)**

There was a third option, and it is the one this repository already uses
everywhere else: **derive the tables by probing the pinned binary**, and
hold the result to *real source files* rather than to fixtures. No
vendoring, no licence question, and the corpus cannot be chosen to
flatter the result because it was not chosen at all — `scripts/highlight.sh`
runs the C binding's own example and header, 150 lines that exist here
for other reasons.

**C is byte-identical on both, whole output**: wrapper, line anchors and
tokens. All three languages together cost **80.4 KB, 1.16% of the
binary**, and it is a cargo feature so a trimmed build drops it — which
is the size discipline this card asked for, satisfied rather than
promised.

`--no-highlight` and `--syntax-highlighting=none` turn it off, any other
style value is refused by name, and **every gate that mutes pandoc's
highlighting now mutes this one too** — `flags.sh`, `writers.sh`,
`diff-html` and the EPUB writer. Muting one side would have compared two
different questions, and that trap was live the moment the first language
landed.

**Python followed C** — four in-tree files, 459 lines, byte-identical,
and every `.py` in the repository. Its table was probed over *python's
own* vocabulary (`dir(builtins)` plus `keyword.kwlist`, 211 names),
because choosing a probe set by hand is how `file` came to be missing on
the first attempt and only a real file caught it. **Drop-in 10/48 to
11/48.**

**bash followed Python, and it is a different kind of job.** Its classes
are positional rather than lexical: the same word is `fu` at the start of
a command and plain text one word later, so the scanner carries *where in
a command it stands* — plus the open-parenthesis and open-substitution
counts, which is what tells a `case` label's `)` from a `$( … )`'s across
lines. **20/20 shell scripts, 2,065 lines, byte-identical**, this
project's own harness among them; `samples/06` is now identical too.
The command table was probed **one word at a time**, 204 rows, after a
batched probe came back misaligned and would have coloured 69 words
wrongly. The gate is now **26/26 files, 2,650 lines**.

**Two things are still open, and neither is a tokenizer.**

1. **The exit test needs `ruby`, not C.** Of the three spec examples with
   a known language, one is inside an HTML block and never reaches the
   writer, so `diff-html` turns entirely on `def foo(x) / return 3 / end`.
   Ruby's rules are a different order of hair — `[` is a `kw`, `.s` is an
   `at`, `true` is a `dv`, a single-quoted string is `vs` — and there is
   no real Ruby in this repository to gate it on. Writing some would be
   the fixture-fitting this card just avoided.
2. **The stylesheet is a licence question.** A standalone page with a
   highlighted block wants 66 lines of CSS, and that CSS is skylighting's
   style rather than `data/templates` — so the BSD carve-out the template
   is vendored under does not reach it. The spans are written; the
   colours are not. **That one is the owner's call**, and it is the only
   thing between `-s` output and parity.

#### What the measurement said before any of it — 2026-08-24

The wrapper is easy and the tokenizer is not, and the exit test hides
which is which. **Only three of the spec's 652 examples hold a fence in a
language pandoc knows** — two `ruby`, one `c`, each three lines long — so
a tokenizer built to pass `diff-html` would be fitted to nine lines of
code. That is the corpus blind spot in its purest form, and this
repository has paid for it eight times.

What is *not* in doubt, because it was read off the binary tag by tag:

- the wrapper is `<div class="sourceCode" id="cbN">`, `<pre class="sourceCode
  LANG-as-written">`, `<code class="sourceCode CANONICAL-LANG">`, one
  `<span id="cbN-M"><a href="#cbN-M" aria-hidden="true" tabindex="-1"></a>`
  per line. **`cbN` counts every code block**, highlighted or not, and an
  explicit `{#id}` replaces it. `.numberLines` adds `numberSource` to the
  `<pre>` and drops `aria-hidden`/`tabindex` from the anchors;
- **a language pandoc does not know degrades to exactly what this writes
  today** — `<pre class="aaa"><code>` — so nothing regresses for the
  languages left out;
- the token stream is skylighting's, and skylighting is KDE's syntax XML.
  In C: `int`→`dt`, `struct`→`kw`, `if`/`return`→`cf`, `0x1f`→`bn`,
  `1.5e3`→`fl`, the `f` of `1.5f`→`bu`, `#include `→`pp` with `<stdio.h>`
  →`im`, escape runs inside a string→`sc`, and **adjacent operator
  characters merge into one span** (`);`, `};`, `(;;)`). `#if 0` is all
  `pp` while `#define X 1` splits off a `dv`. Those are not rules anyone
  derives; they are one XML file's state machine.

**So there are two options and they cost differently.**

**(a) Vendor the syntax definitions** — KDE's XML, the same input
skylighting reads — and write an interpreter for the subset those files
use. Faithful by construction rather than by probing, and it is what
pandoc does. Costs: a licence question (the templates precedent says
vendoring is acceptable *with the notice beside it*), the interpreter,
and the bundle size the card already warns about.

**(b) Hand-written approximations per language.** Smaller and quicker, and
**it will differ from pandoc in ways no corpus written here can honestly
bound** — the failure mode is plausible-looking highlighting that is
wrong, which is the worst kind this project has.

The measurement says (a) or nothing. Either way the exit test needs a
corpus of real code per language, not the spec's nine lines — otherwise
the number means only that the fixtures were chosen to pass.

### 0.7.5 — Performance and the large-document envelope

**Claim:** the supported conversion paths get materially faster and use
materially less peak memory on real documents, while preserving the exact
output and safety guarantees that make the speed claim worth anything.

This is a required best-effort engineering objective, not a decorative set
of aspirational numbers. Every card below must make the strongest reasonable
attempt to reach its target. A target may be revised only after a profile,
the exact input checksum, and the reason it cannot be reached are committed
to the performance record. "The benchmark improved a little" is not done.

#### Baseline and targets

These are release-build, Linux x86-64 baselines measured on 2026-08-25. The
real public inputs must be recorded by URL and checksum before any result is
made a gate; they are not fixtures to be silently replaced with friendlier
ones.

| end-to-end workload | current baseline | target | why it matters |
|---|---:|---:|---|
| 918,295 B Rust release notes, CommonMark → HTML | 53.2 ms; 31.5 MiB peak RSS | **≤ 40 ms; ≤ 24 MiB** | A large text document where parser, AST and writer all matter. |
| 6,797,467 B public EPUB → plain text | 180 ms; 71 MiB peak RSS | **≤ 140 ms; ≤ 55 MiB** | A genuine multi-chapter archive rather than tiled Markdown. |
| 226 public EPUBs, 199.6 MiB compressed input, CLI → plain text | 26.7 s; 223 MiB worst per-process RSS | **≤ 20 s; ≤ 180 MiB** | Sustained ingestion: the workload a migration actually schedules. |

Use the median of five warm release runs for time and the operating system's
high-water resident set for memory. Conversion must exit successfully,
produce the same bytes as the checked baseline where a byte oracle exists,
and leave all differential gates at their floors. A lower RSS number obtained
by skipping media, refusing a document, or timing only a parser is a failure,
not an improvement.

#### Concrete hot paths already found

The work begins with measured allocations and copies, not with a generic
rewrite in the name of speed.

1. **HTML's footnote preflight clones every AST.**
   `ferrodoc-html::has_note` copies `blocks` just to walk them mutably, even
   for a document with no notes. `take_notes` therefore pays a full-tree
   allocation before it can return its borrowed fast path. Replace it with
   an immutable short-circuiting walk. This must preserve the existing
   footnote numbering and nested-note tests.
2. **HTML layout copies the whole rendered output two or three times.**
   `lay_out` chains `String::replace` for `Wrap::None` and `Wrap::Preserve`.
   Replace that with one output pass and one allocation, byte-identical for
   all wrap modes. This matters most when the output, rather than the input,
   is the large object.
3. **The Markdown reader defeats its own borrowed preprocessing fast path.**
   `preprocess` returns a borrowed `Cow` for already-normal input, but
   `read` immediately calls `into_owned`. Keep the input borrowed until the
   empty-front-matter workaround actually changes it. This removes one full
   input copy on the ordinary path.
4. **The owned interoperability AST is the real large-document ceiling.**
   Comrak builds a complete arena tree and ferrodoc then builds a complete,
   fully owned Pandoc-compatible AST. Literal prose is further represented
   as word-sized `Inline::Str(String)` allocations because Pandoc JSON needs
   those token boundaries. Do not weaken JSON compatibility to hide memory;
   profile an internal compact/borrowed representation or an event path
   behind the existing owning public API, and make it a separately reviewed
   design decision.
5. **Archive readers hold several representations at once.**
   EPUB takes all ZIP bytes, reads a chapter into a `String`, builds an HTML
   tree, and appends its owned blocks to the growing document. DOCX and ODT
   have the same whole-part shape. Design a reader-backed archive and
   streaming-output path for CLI/batch users, while retaining the current
   byte-slice APIs for embedders that need an owned AST.

The first three are compatibility-preserving implementation cards. The last
two are architectural work: no public AST break, lossy shortcut, `unsafe`,
or unbounded partial rewrite is acceptable merely to hit a benchmark.

#### Initial execution cards

**P1 — Remove unconditional HTML copies.** Replace the mutable clone in the
footnote preflight with an immutable walk; make `Wrap::None` and
`Wrap::Preserve` layout one-pass. Add unit tests for no note, nested note,
and each wrap mode; run the HTML differential gate and the pinned 918,295 B
benchmark. **Done:** identical output and a recorded movement toward
40 ms / 24 MiB. **Not this card:** changing the public AST or implementing
streaming.

**P2 — Keep ordinary Markdown input borrowed.** Carry `Cow<str>` through
the Markdown parser setup, allocating only for tabs, carriage returns,
missing final newlines, or the empty-front-matter exception. Run the 652
CommonMark examples, AST round-trip gate, and the pinned large Markdown
benchmark. **Done:** no compatibility loss and an allocation/profile record
showing whether the input copy was removed. **Not this card:** replacing
Comrak.

**P3 — Make the archive/AST decision from evidence.** Profile the largest
real EPUB and the 199.6 MiB batch by phase: input archive, decompressed
chapter, HTML tree, Pandoc AST, and output. Write and review the smallest
design that can release those buffers early — likely a reader-backed archive
plus a streaming conversion API. **Done:** a checked design and a prototype
or a recorded, evidence-backed rejection; it must make the strongest
reasonable attempt at 140 ms / 55 MiB and 20 s / 180 MiB. **Not this card:**
a silent public-API break or a new document format.

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
