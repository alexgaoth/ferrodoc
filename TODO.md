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
| **H2 Sufficient** | the square covers what an editorial team actually holds | a team gets from what they hold to what they publish without reaching for pandoc once | markdown, GFM, HTML, DOCX, ODT, EPUB in ✅ LaTeX/PDF, RST, AsciiDoc, **EPUB out ✅** — every format read is now also written — **met** |
| **H3 Trustworthy** | stated resource bounds that hold on *any* input | every path publishes a bound CI checks | never-panics ✅ deterministic ✅ bounded recursion ✅ peak RSS gated ✅ · **one superlinear path** |
| **H4 Believed** | the numbers are reproducible by someone who does not trust us | every README claim has a command in the repo and a CI job | 20 differential gates ✅ four independent corpora ✅ six external judges ✅ · standing work, never "done" |

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

**Re-ranked after eight items landed.** H1 is met — every ecosystem that
holds document pipelines can install this — and H2 is met as of the EPUB
writer: every format this reads, it can now also write. So rules 1 and 2
have nothing left to promote, and the queue is what rule 3 leaves.

1. **Close the HTML reader's 26 divergences** (`## Smaller things`) —
   promoted by evidence rather than taste, and it now costs three ways
   rather than one: it is the only reason `diff-epub` misses two
   documents, the only reason `corpus/epub-spec` sits at 8/22, and the
   EPUB *writer* renders through the same crate. One fix, three gates.
2. **PDF without TeX** — unchanged, and still waiting on a demonstrated
   need rather than an opinion.
3. **The four known writer divergences from pandoc** (`COMPATIBILITY.md`)
   — each is deliberate and each is *right*, so this is a watching brief,
   not work: if a later pandoc fixes its own `dc:title` or its dangling
   references, `diff-epub-write` should rise on its own and the table
   should shrink. It is listed so that nobody re-derives the reasoning.

Everything else on this page is done, parked with a measurement, or a
declared non-goal. When one of these lands, run step 2 again.

---

## Now

Working queue. Every item carries an **Eval** — the test that decides, frozen
before any work starts, in the sense `## What "done" means` defines. The four
rules there apply to each without being restated, in particular: *nothing else
may regress*, and *done includes the paperwork*.

**Re-ranked 2026-08-17 after five items landed**, by the three rules in order.
Rules 1 and 2 still have nothing to promote — H1 is met and every format read
is also written — so the queue is what **rule 3** leaves, followed by the
measurement that decides what comes after it. Probes behind each item are in
`.iterate/odyssey-20260817-1409/ODYSSEY.md`.

- [ ] **(IN PROGRESS — REVISE outstanding, do not merge as-is)** **A Jupyter
  notebook reader and writer** — landed as `9b0a33a` on branch
  `odyssey/20260817-1902`, **not approved**. A critic returned REVISE on two
  MAJOR findings; attempt 2 was killed by a session limit before it changed
  anything, and the deadline passed while blocked.

  **The hazard to fix before this is merged.** `scripts/verify.sh` gates both
  new ipynb gates at `--fail-under 100` on a corpus the critic measured as
  unrepresentative: adding **nothing but inline `$…$` math** to three of the
  eight notebooks drops `diff-ipynb` to **5/8** and `diff-ipynb-write` to
  **6/8** — the latter below this item's own committed ≥7/8 floor. Merged
  as-is it advertises a 100% guarantee it does not have. The crate itself is
  sound and independently verified; it is the corpus and the two thresholds
  that are not yet honest.

  Resume from the two MAJORs: read `$…$` and `$$…$$` in ipynb markdown cells
  as pandoc's ipynb reader does, stop `write_gfm` escaping `\` and `_` inside
  `Math`, widen the corpus to carry math and a bare URL, and correct the two
  `COMPATIBILITY.md` rows. **Do not shrink the corpus to make the number
  work** — that is the one move that turns this into bar-lowering.

  What a critic already verified and should not be redone: writer-gate
  isolation (both writers receive pandoc's own reading), id-drop narrowness
  (an id-discarding writer scores 0/8), the fuzz extension (126 seeds against
  a baseline of 118, genuinely calling the new reader), a privacy-clean corpus
  copied from nothing on this machine, `nbformat` 5.11.1 at 9/9, and no
  regression in any pre-existing gate.

- [ ] **(original item text, Eval frozen and unchanged)** **A Jupyter notebook reader and writer** — the significant addition this
  run was asked for, and the ranking supports it independently. `ipynb` is one
  of the **eighteen mainstream formats** `## Why not simply rewrite pandoc`
  identifies, it is **not** among the declared non-goals, and pandoc both reads
  and writes it — so it can be gated in both directions on the day it is
  started, which is the condition `## How a format gets added` sets. The
  tie-break rule decides the rest: a new reader gains every writer already
  written, so this arrives as `ipynb → markdown, → GFM, → HTML, → DOCX, → ODT,
  → EPUB, → LaTeX, → RST, → AsciiDoc, → plain`, plus the return trip.

  It also serves the bet more directly than any format left on the list.
  Notebooks are where Python pipelines keep prose, and the Python wheel is
  already the most-used binding this project ships. `notebook → docx` in
  process, with no 153 MB subprocess, is a pipeline people actually run.

  **Probed before writing this, against pandoc 3.8.2.1** — the premise is
  measured, not assumed:
  - a notebook reads to one `Div` per cell, classed `cell markdown` /
    `cell code` / `cell raw`, with `execution_count` and `tags` as key-value
    attributes; outputs become nested `Div`s classed `output stream stdout`,
    `output execute_result`; a raw cell becomes `RawBlock "ipynb"`; notebook
    metadata lands under a single `jupyter` meta key.
  - **pandoc's ipynb does not round-trip through itself**, and the only
    difference is that its writer invents a **random per-cell UUID `id`**
    (nbformat 4.5 requires one) which its reader then reads back. Everything
    else — meta, every block, cell count — is identical. That is the same
    shape as the EPUB writer's random `dc:identifier`, and the harness already
    has `drop_uuid` for exactly it.
  - **Eval:**
    1. A `crates/ferrodoc-ipynb` crate, `Format::Ipynb` wired into `parse`,
       `render`, `Format::NAMES` and the `--help` text in `main.rs`, and
       `ferrodoc nb.ipynb -t docx` working end to end from the CLI. A format
       unreachable by users is not added.
    2. A corpus of **at least 8** notebooks under `corpus/ipynb-handmade/`,
       written in the shape **Jupyter and Colab actually emit** (nbformat 4.5,
       cell `id`s, `outputs` covering `stream`, `execute_result`,
       `display_data` with a base64 `image/png`, and `error`), **not** in the
       shape pandoc's writer emits. Real notebooks exist on this machine to
       study for shape — **read them for shape only and commit none of them**;
       they are the user's files and third-party package samples.
    3. **Reader gate `diff-ipynb` at 8/8 (100%)** on that corpus, in
       `scripts/verify.sh` and CI. 100% is the commitment because the corpus
       is small and hand-written, which `CLAUDE.md` says to gate at 100
       separately from a spec run — and because ipynb is JSON, so there is no
       parse ambiguity to hide behind.
    4. **Writer gate `diff-ipynb-write` at ≥ 7 of 8**, ours through pandoc
       against pandoc's through pandoc, with the random cell `id` dropped **in
       its exact unmatchable form only** — a notebook that loses a *real* id
       must still fail. Every divergence enumerated by name in
       `COMPATIBILITY.md`. If 8/8 turns out reachable, say so; if fewer than 7,
       the item is **not done** — that number is the commitment, not an
       aspiration.
    5. **An external judge**: the written notebook must be accepted by
       something that is not us. Try `pip install nbformat` and validate with
       `nbformat.validate` if it installs; if it does not, say so plainly and
       use the strongest available substitute — `pandoc -f ipynb` accepting
       every written notebook, plus a check of nbformat 4.5's required keys —
       and record in `COMPATIBILITY.md` which judge was actually used.
    6. `./scripts/verify.sh` exits 0 with **no existing threshold lowered**,
       the wasm32 build still passes (the crate must do no IO and pull in no C
       library), and `--fuzz` is run because a reader changed.
    7. The paperwork: rows in `COMPATIBILITY.md` and `docs/gates.md`, the gate
       in CI, and a `crates/ferrodoc-ipynb/CLAUDE.md` holding the rules that
       are easy to get wrong.

- [ ] **The new samples check claims more than it catches** — three MINORs
  from the `28f064b` review, and the first is the fourth instance this project
  has produced of one defect: **a documented claim wider than the code behind
  it**. That pattern has now cost a comment, a divergence table, a results row
  and a gate description in two days of work.
  - `docs/gates.md` names all three historical losses — including
    `Superscript`/`Subscript`/`Underline`/`SmallCaps`/`Span` — and says
    `--samples` "is what keeps them found". The critic reintroduced the
    `Underline` and `SmallCaps` drops, confirmed the mutation reached the
    binary, and **`--samples` exited 0**: no sample document contains either
    inline. The only `smallcaps` string under `samples/` is pandoc's
    boilerplate CSS. (`cargo test` does catch it, so nothing can silently
    ship — the defect is the sentence, not the mechanism.)
  - **`samples/inputs/` is outside the compare scope entirely.**
    `artefacts()` is `find RESULTS.md [0-9]*-*/ -type f`, so the six committed
    inputs are never compared — including `page.html`, which the script itself
    generates with pandoc on every run. `docs/gates.md` says "Exactly two
    things are ignored" and `generate.sh` says "Everything else must match
    byte for byte"; both are wider than the code.
  - **`samples-check.*` is not in `.gitignore`.** The `EXIT INT TERM` trap is
    sound — verified against SIGINT, SIGTERM and a failing run, all clean —
    but SIGKILL, an OOM or a power loss leaves a 13-directory untracked copy
    of `samples/` at the repo root that a later `git add -A` would commit.
  - **Eval:**
    1. **Make the claim true rather than narrowing it**, which is the better
       of the two fixes the critic offered: `samples/inputs/handbook.md` gains
       an `<u>`, a `<span class="smallcaps">` and an attributed span, and
       reintroducing the five-inline drop in the markdown writer makes
       `./scripts/verify.sh --samples` exit **non-zero** — demonstrated with a
       `cp`-restored mutation, never `git checkout`. If narrowing turns out to
       be the only honest option, say why with evidence.
    2. `samples/inputs/` is either inside the compare scope or named as
       outside it in both `docs/gates.md` and `generate.sh`, so no sentence in
       either is wider than the code.
    3. `samples-check.*` is in `.gitignore`, verified by `git status
       --porcelain` staying empty with such a directory present.
    4. `./scripts/verify.sh` exits 0 with no threshold lowered, and
       `./samples/generate.sh --check` stays clean on three consecutive runs.

- [ ] **(BLOCKED: the Eval is met; a section the Eval never asked for is not)**
  **Nobody has counted what the failing gates are failing on** — `bf289bc` →
  `4bf7595` → `e8bf015`. **Three attempts, three REVISE verdicts from three
  different critics.** The Eval is left **intact and unweakened**; the item is
  blocked rather than revised, because revising it here would be bar-lowering.

  **What is actually true, and it is unusual.** All five criteria are
  *verified met* — criterion 1's 51 rows reproduce from the printed command,
  the grouping arithmetic holds, criterion 4's refutation is confirmed
  correct, criterion 5 is clean, and round 3's critic ran **every command
  block in the document verbatim** and found each demonstrates its claim.
  What failed all three rounds is a section **the Eval never required**: a
  closing list of "HTML reader divergences that no gate measures". It said
  three, then five, and is now measured at **six**. The Eval asked for a
  census of *failing* documents; the author added an audit of *unmeasured*
  ones, and that addition has been wrong every round.

  So the blocking reason is precise: *done includes the paperwork* makes the
  extra section binding once written, and it is not right. The census proper
  is sound and stays committed — nothing is reverted, because the work
  predates this branch and meets its contract.

  **Do not re-attempt this item.** Its remaining defect is now the item
  below, with an Eval that actually names the job.

- [ ] **Enumerate every HTML reader divergence no gate can reach — and fix the
  one that loses content** — the census kept undercounting this because it was
  guessing at a boundary instead of sweeping it. Round 3's critic did sweep
  it: all **119 XHTML files** across the 34 corpus EPUBs, `pandoc -f html` vs
  `ferrodoc -f html`, comparing `.blocks`. That is the method; the census's
  was not.

  It found a **sixth** divergence, and it is a structural loss rather than a
  formatting one. Verified independently by the overseer:

  ```
  <p>a<a href="#fn1" class="footnote-ref" id="fnref1" epub:type="noteref" role="doc-noteref">1</a></p>
  pandoc   → Para[ Str "a", Note [] ]
  ferrodoc → Para[ Str "a", Link ["fnref1",["footnote-ref"],…] ]
  ```

  **A footnote reference silently degrades to a link.** Neither attribute
  triggers it alone — probed both ways — only the combination. It is live in
  `corpus/epub/anchors-and-notes.epub` and `corpus/epub/notes-and-images.epub`,
  and **both books pass `diff-epub`**, because ferrodoc's EPUB path builds
  `Note` by another route. No gate reaches it: none of the 8 `corpus/*.html`
  files contains `noteref`.
  - **Eval:**
    1. A corpus fixture under `corpus/` carrying **every one of the six**,
       so each becomes gate-reachable: trailing space in `<em>`/`<strong>`,
       space before `<br />`, `<head>` metadata, `<li id>`, `epub:type`, and
       `noteref`. `diff-html-read`'s denominator grows and its **numerator
       must grow by the same amount** — a fixture that fails on arrival is a
       fixture, not a fix.
    2. **The `noteref` loss is fixed**: the probe above produces `Note` from
       ferrodoc, asserted as a literal-AST unit test, and mutation-tested by
       breaking it and confirming the new fixture drops.
    3. The remaining five are each either fixed or **recorded in
       `COMPATIBILITY.md` as deliberate with a reason**, and `<li id>` is
       stated with **both** its forms — a `Span` for inline item content and a
       `Div` for block content, which the census printed only half of.
    4. `diff-html-read`'s threshold in `scripts/verify.sh` is **raised**, never
       lowered, and `./scripts/verify.sh` exits 0.
    5. The sweep is repeatable: the 119-file comparison is committed as a
       script so the next session does not re-derive it, and re-running it
       reports **zero** divergences outside those recorded.

- [ ] **The `plain` writer is plainer than pandoc's** — `samples/10` differs by
  47 lines: block quotes are not indented two spaces, tables are tab-separated
  where pandoc column-aligns them, indented code blocks lose their four-space
  indent, ordered lists write `1. ` rather than `1.  ` with sub-items
  unindented, and every list comes out loose. Minor format, lowest rank, listed
  so it is not rediscovered.
  - **Eval:**
    1. `samples/10-markdown-to-plain/diff.txt`, regenerated, is **under 12
       lines** (baseline 47), with every surviving divergence named in
       `COMPATIBILITY.md`.
    2. Literal-output unit tests for each rule changed — quote indent, table
       alignment, code indent, list spacing.
    3. `./scripts/verify.sh` exits 0 with no threshold lowered.

- [ ] **`crates/ferrodoc-html/CLAUDE.md` is at 85 lines against a 40-line
  budget** — measured by the worker on the last paperwork item and flagged
  rather than hidden. `## When to run /tend` says that past double budget the
  answer is **re-filing, not compression**: package facts into nested
  `CLAUDE.md` files, long-form detail into `docs/` behind a plain pointer,
  never an `@import`. The crate governs 4 files, so its budget is 40 and it is
  at 213% of it.
  - **Eval:**
    1. `crates/ferrodoc-html/CLAUDE.md` is **at or under 40 lines**, measured
       with `wc -l`.
    2. **No fact is lost.** Every line removed is either relocated — to a
       nested `CLAUDE.md`, to `docs/`, or to `COMPATIBILITY.md` — or is named
       in the commit message as deliberately dropped with the reason. A
       reviewer must be able to trace each removed line to where it went.
    3. Nothing relocated is behind an `@import`, which would load it anyway
       and defeat the point.
    4. `./scripts/verify.sh` exits 0. This item changes no code.

- [ ] **Two paperwork MINORs a critic raised and the clock did not allow** —
  from the `75cd12e` review, recorded rather than dropped.
  - `COMPATIBILITY.md` records the empty-`Attr` divergence and names
    `samples/05-html-to-markdown/diff.txt`, but `samples/README.md` — the file
    a reader of that diff actually opens, which enumerates five other things
    visible in these diffs — carries **no pointer back**. The link runs one
    way, so the record sits where the reader is not.
  - The three checkbox divergence bullets each assert something about
    *ferrodoc* ("one `Para` here", "matches it byte for byte", "ferrodoc keeps
    `Header 2`") but their reproducing commands show only **pandoc's** half.
    The reader must swap the binary themselves. The empty-`Attr` block, right
    beside them, gives both lines.
  - **Eval:**
    1. `samples/README.md` points at the empty-`Attr` record.
    2. Each of the three checkbox bullets carries both halves of its
       comparison, and **each command is run and its stated output matches
       what it really prints**.
    3. `./scripts/verify.sh` exits 0. This item changes no code.

## Done

- [x] **`samples/` is an unchecked guarantee** (2026-08-17) — eval met:
  `28f064b`. `samples/generate.sh --check` regenerates into a `mktemp -d -p .`
  scratch dir and compares `RESULTS.md`, every text artefact, both
  `*.readback.md` per binary sample and every `diff.txt` with only lines 1-2 of
  the header elided; it detects added *and* removed artefacts. Wired into the
  default `verify.sh` run and into the pandoc-pinned CI `conformance` job.
  **~3 s of a ~110 s suite**, which is why it runs by default: hiding it behind
  an opt-in flag would have reproduced the exact defect being fixed.

  Criterion 2 is what the item was for, and it holds three times over. With the
  colspec fallback removed, `--gates` exits 0 with **all twenty scores
  identical** and `--samples` exits 1. The critic then refused to accept a
  check demonstrated on the one loss it was built against and reproduced it
  with **two different mutations** — column *widths*, and dropped
  `Superscript`/`Subscript` — both caught, both with the twenty gates green.
  It also verified the "never writes to the tree" claim on a clean tree, a
  dirty tree, a *failing* run and SIGINT/SIGTERM interrupts, and planted a
  content change on a diff body line beginning `---` to confirm the header
  elision is scoped to lines 1-2 and not to any matching line.

  One incidental finding worth keeping: **`cp -a` preserves mtime, so cargo
  skips the rebuild** and a restored tree can be measured with a stale binary.
  Restore with plain `cp` plus `touch`.

- [x] **The correction to the fence comment was itself imprecise**
  (2026-08-17) — eval met: `fd66287`, **on branch `comment/fence-sizing-rule`**,
  not `main`. Comment-only, proved by stripping `//` lines and diffing against
  `75cd12e` to nothing; `verify.sh` exit 0, all twenty gates unmoved.

  Third round of one defect class, and it took three refutations to land. The
  original comment stated a false pandoc behaviour. Its replacement stated a
  rule that **contradicted its own examples** — "the longest line that is
  *only* backticks", illustrated with `"   ````"` and `"```` "`, neither of
  which is one. And the fix handed to the worker ("pandoc trims surrounding
  whitespace") **was itself wrong on two counts**, which the worker measured
  rather than accepted: the trim is **spaces and tabs only** — a non-breaking
  space is not trimmed — and what survives must be **one unbroken run**, not
  merely all-backticks-and-whitespace, which ``"``` ```"`` and ``"```` ``"``
  both disprove.

  The critic then built cases designed to separate the new wording from its
  nearest rivals — `` "`` ```` ``" ``, `"````` x\n```"`, `` "```\n`````" ``,
  vertical tab, form feed, em space, U+2028, ideographic space — and
  **could not construct a case where it is wrong**. It is not a fourth
  approximation.

  Why it survived a worker and three critics before this: **the conclusion the
  comment supports is correct** — ferrodoc's fence is strictly wider, never
  narrower — so nothing downstream misbehaves and no test can fail. Nothing in
  this repo tests a *reason*. That is the standing lesson, and it is why
  `CLAUDE.md`'s "probe it first" has to bind prose as hard as it binds code.

- [x] **Five things three critics measured stale or false** (2026-08-17) —
  eval met: `75cd12e`, plus `36f3211` for the one line barred from every
  worker's scope. All six criteria met; `verify.sh` exit 0 with all twenty
  gates identical, and the diff proved behaviour-free by stripping `//` lines
  and finding no remaining difference.

  Three things came out of it that were not asked for. The worker
  **re-measured all five claims itself and all five held** — no critic finding
  was wrong. It **widened one**: pandoc indents an empty-`Attr` code block on
  `-t markdown`, `-t gfm` and `-t commonmark` alike, and *unconditionally* — a
  blank line or a backtick run in the content does not make it fence — which
  the critic then re-probed across four content shapes and three non-empty
  `Attr` shapes and confirmed. And it **corrected the brief it was given**: the
  sample diff has 50 changed lines, not the 47 stated, then declined to put a
  count in the prose at all, which is the better fix than a correct number that
  goes stale.

  It also found `TODO.md:570` genuinely stale and **reported it instead of
  reaching for it**, that file being barred from every worker by scope. The
  displacement it made in `crates/ferrodoc-html/CLAUDE.md` was checked and is
  honest — the removed bullet's full content is at `COMPATIBILITY.md:497-501`
  *with* a measurement the removed line lacked, and the critic re-measured that
  too. **That file is at 85 lines against a 40-line budget**, and past double
  budget the `/tend` rule calls for re-filing into nested files rather than
  compression. That is a real item somebody should take.

- [x] **The HTML writer does not render a task list** (2026-08-17) — eval met:
  `a145326`. `task_box()` recognises a box at the head of an item's first
  `Plain`/`Para`; the `task-list` class is emitted only when **every** item is
  one; `OrderedList` gets boxes but never the class. `samples/06` 27 → 19 diff
  lines with no `task-list`/`checkbox` line left; `diff-html` held at 652/652;
  `verify.sh` exit 0 with `scripts/` untouched.

  The critic compared **42 shapes byte-for-byte** — including `☑` and `✓`,
  which are *not* the two characters pandoc uses, a box wrapped in
  `Strong`/`Link`/`Span`, a `DefinitionList`, three-level nesting, and a
  variation-selector `☒️` — and found **zero divergences**; the only diff in
  the sweep was footnotes, which reproduces outside any task list and is
  declared scope. It broke the rule **seven** independent ways and each failed
  a distinct assertion.

  Two things it settled that the Eval never asked about. **EPUB**, because
  this writer renders EPUB's XHTML: `epubcheck` 5.1.0 on eleven books —
  0 errors, 0 warnings on every ferrodoc-written one — and `diff-epub-write`
  unmoved at 8/11. And **`samples/13` losing a whole hunk**, which was
  adjudicated rather than accepted: pandoc reading ferrodoc's EPUB and pandoc
  reading its *own* now produce identical output, both
  `- <label>☒ one</label>`, so the loss is pandoc's markdown writer and not
  ferrodoc's EPUB — while ferrodoc's own reader still recovers `- [x]`.
  Nothing is lost that pandoc keeps.

- [x] **The markdown writer emits `sourceCode` as the code language**
  (2026-08-17) — eval met: `5b18ff9`. `attr.classes.first()` became "first class
  that is not `sourceCode`, after a space"; six literal-output assertions;
  `samples/05` diff 51 → 47 lines with the ```` ```sourceCode ```` hunk gone and
  nothing new; `diff-md` 652/652 and `diff-gfm-md` 655/655 held; `verify.sh`
  exit 0, `scripts/verify.sh` not touched at all.

  The critic's check was the one that mattered: **a rule derived from six cases
  and gated on those same six cases is fitted, not proven.** It probed **30
  shapes the Eval never named** — duplicate `sourceCode`, empty-string classes,
  a 120-char class, classes that are themselves backtick or tilde runs, ids,
  key-value attributes — in both `gfm` and `commonmark`, and all 30 agreed with
  pandoc. It then killed the test five independent ways to prove the assertions
  are load-bearing. Two MINOR findings went to the item below rather than being
  written off with the approval.

- [x] **The HTML reader loses a task list's checkbox state** (2026-08-17) —
  eval met: `f3a6807`. `diff-html-read` 632/658 → **633/659**, threshold raised
  95 → 96 and load-bearing (rule disabled → 95.9%, exit 1); new fixture
  `corpus/task-lists.html` 1/1 identical to pandoc; literal-AST test
  `a_task_list_keeps_which_boxes_are_ticked` proved able to fail; `verify.sh`
  exit 0 unpiped, `--fuzz-only` exit 0; `samples/05` no longer differs on the
  checklist. A fresh critic rebuilt `b33d121` in a worktree and `comm`'d the
  mismatch sets at both revisions — **26 failures before, 26 after, the two
  sets identical**, so the +1/+1 is the new fixture passing and nothing else
  moving, not a regression masked by two new passes. It also probed 25 shapes
  against pandoc 3.8.2.1 directly; all matched.

  Two things found on the way, neither visible from the gate: the trailing `/`
  in `<input … />` is **load-bearing for pandoc** — written the ordinary HTML
  way the tag stays open in tagsoup and swallows the rest of the list, so a
  naturally-authored fixture would have pinned that parse failure as if it were
  the rule; and the box is read below a `<li>` however deep, but not in a
  `<dd>`, which is why the flag belongs in `items()` and not `item()`.

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

### ~~4b. EPUB, write~~ — landed, and **one committed criterion was wrong**

`ferrodoc manual.md -o manual.epub`. The HTML writer plus a manifest, a
spine, a nav document and an NCX, chapters split at level-1 headings.

| criterion | committed | measured |
|---|---|---|
| `diff-epub-write` at **100%** on `corpus` | required | ❌ **8/11 (72.7%)** — see below; 100% is only reachable by writing invalid books |
| `epubcheck` reports 0 errors on every written book | required | ✅ **0 fatals, 0 errors, 0 warnings** on all six, in CI — pandoc's own books do not reach this |
| round-trips through this crate's reader, spine order intact | required | ✅ `a_written_book_comes_back_in_spine_order`, plus three more |
| `Format::Epub` writable, in `--help`, no special case | required | ✅ `writable_format` deleted, and `Error::NotWritable` with it |

**The 100% was not achievable and should not have been written down
without measuring.** The project's own first rule says a roadmap item's
premise is a claim like any other; this one was a guess and it was wrong.
What the measurement found:

- three documents differ because **this writer will not emit a reference
  the book cannot satisfy** — a picture with no bytes becomes its alt
  text, a relative link naming no file in the book becomes its text.
  Pandoc emits both, and `epubcheck` rejects pandoc's book for exactly
  them (`RSC-007`). Matching pandoc here means shipping invalid books;
- four metadata fields **cannot be matched by anything**: pandoc's random
  `dc:identifier`, its `dcterms:modified` clock, its locale-derived
  `dc:language` — `de_DE.UTF-8` writes `de-DE` — and the `dc:title` it
  omits although EPUB 3 requires one. The gate drops each *only in its
  exact unmatchable form*, so a book that loses its identifier or invents
  a title still fails.

So the replacement criterion, which is the one that now holds and is the
harder of the two: **`diff-epub-write` ≥ 72 on `corpus`, with every
divergence enumerated in `COMPATIBILITY.md`, and `epubcheck` clean on
every written book in CI.** A new divergence fails the gate; an old one
cannot be quietly reclassified, because each is named in the table.

Found along the way, none of it visible from the gate: an unterminated
HTML comment is fatal in XML and the book will not open (`RSC-016`);
repairing it over the rendered chapter instead of the raw fragment ate
the writer's own `</li></ul>` and traded one fatal for another; and
scraping media out of the emitted XHTML never rewrote the `src`, so every
picture was bundled and then lost.

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
| HTML reader | 633/659 identical; closes the Markdown ↔ AST ↔ HTML ↔ DOCX square. Content a browser hides — `<template>`, `<noscript>` — is read rather than dropped, and a task list keeps which boxes are ticked |
| GFM | reader 654/655 identical; writer 655/655 round-trip fidelity — **pandoc manages 589/655**. `docx → gfm` keeps its tables |
| ODT | reader 32/34 identical, and **8/8 on documents LibreOffice wrote**; writer 11/11 corpus and 640/652 spec through a pandoc round trip, with embedded images. Completes the office square: what Word wrote and what LibreOffice, Google Docs export and every European public-sector template wrote, in both directions |
| Media | `docx → docx` and `odt → odt` keep their images, byte for byte; `read_*_with_media` and `parse_with_media` expose the bag |
| Image formats | PNG, JPEG, GIF, WebP, TIFF, SVG, EMF and WMF embed, each at the size and resolution its own header states — Exif and JFIF, `pHYs`, TIFF rationals, an EMF frame. All eight open in LibreOffice; ten measured divergences from pandoc, all in `COMPATIBILITY.md` |
| Inline `<svg>` | a picture written into the page is carried as a `data:` URL, and a `data:` URL reaches the office writers — HTML with an inline chart converts to a document LibreOffice opens with the picture |
| Standalone HTML | `-s` writes a whole page — doctype, charset, `lang`, title, authors — and `--css` inlines a stylesheet |
| **H1** Python | `pip install ferrodoc` — one typed function, abi3 wheels for 3.9+ on Linux, macOS and Windows, GIL released so a thread pool overlaps. **72× a pandoc subprocess** per document |
| **H3** DOCX memory | the body streams, so its XML tree never exists in full: **2.7× less peak RSS and ~12% faster**, interleaved against a baseline |
| **H4** Verifiability | 20 differential gates in CI on three platforms against pinned pandoc, plus a gated memory bound, a 500k-mutation fuzz run, and six judges that are not us: `pdflatex`, `sphinx-build -W`, `asciidoctor`, `epubcheck`, a headless browser and valgrind |
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

## Why not simply rewrite pandoc

Asked directly, and worth answering here once so it is not re-argued: what
stops a complete pandoc rewrite in Rust? Nothing technical. It is a
resourcing question with a badly-shaped finish line, and the parts divide
so unevenly that "about forty formats" is the least informative way to
describe the target.

**The format count is smaller than it looks.** From the pinned binary —
`pandoc --list-input-formats | wc -l` and friends — 3.8.2.1 reads **48**,
writes **69**, carries **73** named extensions and **163** highlighting
languages. Of the 48 readers, five are bibliography databases (`bibtex`,
`biblatex`, `csljson`, `ris`, `endnotexml`), two are CSV/TSV, two are its
own AST, and eight are markdown dialects that are one reader wearing
different extension defaults. Of the ~31 that remain, nine are wiki
dialects and four are man-page or doc-comment formats. The genuinely
mainstream set is **eighteen** — `bits djot docbook docx epub fb2 html
ipynb jats latex odt opml org rst rtf textile typst xml` — and this
project already reads four of them (`docx`, `epub`, `html`, `odt`) plus
markdown, GFM and the AST. Two of the fourteen left are non-goals for
reasons that are not effort (`latex`, `typst`), and `rst` is one by
choice.

**But the readers are not the bulk of pandoc.** Parity means four projects
wearing one name, and only the first is what anyone pictures:

1. readers and writers — the part that looks like the whole;
2. the **extension matrix**: 73 flags toggled per format
   (`markdown+footnotes-raw_html`). That is a combinatorial compatibility
   surface, not a feature list, and it is where "identical to pandoc" gets
   expensive;
3. **citeproc** — CSL processing, five bibliography readers, and a style
   repository in the thousands. On its own it is the size of everything
   this repository has done so far;
4. the **Lua filter runtime and template language**. For a large share of
   pandoc's users that *is* pandoc; conversion is incidental to them.

**Three things are hard for reasons resourcing does not fix.** The rest is
grind — and cheap grind, because `diff_binary` and `diff_round_trip` are
already generic over the format name and pandoc is its own oracle wherever
it round-trips. These three are not:

- **Reading a format that is a language.** A `.tex` expands user-defined
  macros, so reading it means implementing enough TeX to evaluate a
  program. Typst and roff are the same shape. Already a non-goal below,
  and it stays one at any budget.
- **PDF.** Note it is not in pandoc's input list either. A PDF has no
  semantic structure — it is positioned glyphs — so reading one is layout
  reconstruction, and writing one without TeX means owning a typesetter:
  line breaking, hyphenation, font embedding, tables.
- **The oracle runs out.** This project's whole claim is differential
  proof against pandoc, and that works only where pandoc round-trips. It
  has already broken once: pandoc writes AsciiDoc and cannot read it, so
  that writer has no gate at all and `asciidoctor` stands in. Push outward
  and more formats land in that bucket. **Every format added without an
  oracle dilutes the one property that makes this worth choosing.**

**And the target moves.** `CLAUDE.md` already records that pandoc's
published sources describe a later pandoc than the pinned binary and
disagree with it. Parity chases a release train. Worse, pandoc's behaviour
includes its bugs, and this project has found a pile of them — the WebP
header, big-endian TIFF rationals, the omitted `dc:title`, dangling EPUB
references. A complete rewrite forces a per-case ruling on whether to
reproduce each one, and there are hundreds of such cases nobody has found
yet.

**So the finish line is the problem, not the work.** "Parity" is owned by
someone else's release schedule, and reaching for it trades the one
defensible property — every format checked document by document against a
pinned oracle — for a larger number. The bet at the top of this file
already says competing on format count cannot be won; this section is why,
with the arithmetic attached.

**What would justify expanding, then.** Not the count. Two triggers, both
already written into the procedure above:

- a **real document someone holds and cannot convert** (step 1, rule 2 —
  it is somebody's impossible). This is the trigger the long-tail non-goal
  names, and it is the only one that has ever promoted a format here;
- a capability **pandoc does not have**, where the comparison is not parity
  at all. Reading a PDF is the whole of this category today, and it is
  worth more than the next twenty formats combined — which is exactly why
  the non-goal below is now contested rather than settled.

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
- **PDF *reading*** — **declared, and now contested. This one is the
  user's call, not this file's** (procedure step 5: re-ranking is
  automatic, re-aiming is not). The declaration stands until they rule.

  The stated reason was that it is an ML problem — layout analysis and OCR
  — owned by Docling and Marker, so interoperate rather than compete. That
  reason **did not distinguish two different jobs**, and only one of them
  is ML:

  - a **scanned** PDF, or one with a complex multi-column layout, is
    genuinely layout reconstruction. The non-goal is right about these and
    should stay;
  - a **digital-native** PDF with a text layer — what a word processor or
    LaTeX exports — is extraction, not inference. That is a parsing job of
    the same kind as every other reader here.

  What changed: a demonstrated need arrived. A user with a personal
  writing archive to convert is blocked on PDF and on nothing else, which
  is precisely the trigger step 1 names — and it is the only category
  where the comparison is not parity, because **pandoc cannot read PDF
  either**.

  What it would take before this becomes an item: measure what fraction of
  a real archive is digital-native rather than scanned, survey the Rust
  PDF crates against the dependency-count arithmetic that parked the PDF
  *writer*, and answer the question that decides it — **what is the
  differential gate?** There is no pandoc oracle here, so this would need
  an external judge in the shape of `asciidoctor` or `epubcheck`, and
  "no format ships without a gate" applies. If no honest gate exists, that
  is an argument for keeping the non-goal, not for making an exception.
- Citations, templates, Lua filters, presentation formats.
- **LaTeX *reading***. A `.tex` file expands user-defined macros, so reading
  it means interpreting a language rather than parsing a format. Writing
  LaTeX is planned and bounded; reading it is not on the list at any point.
- Wiki dialects, DocBook, JATS, FB2, man, Textile and the rest of the long
  tail — one reader each, for audiences a converter in Rust will not reach.
  Reconsider only against a real document someone cannot convert.
  `## Why not simply rewrite pandoc` has the arithmetic: these are cheap
  individually and still not worth it, because the count is not what makes
  this project worth choosing. **Cheap is not the same as worth doing.**
- **Bibliography readers** (`bibtex`, `csljson`, `ris`) were floated as a
  cheap win for research use and are ruled out by the AST-ceiling rule in
  `## How a format gets added`: a bibliographic database is one of the
  three things named there that pandoc's own AST cannot carry as content.
  Reopen only by first answering whether the AST's `references` metadata
  is enough for the use, which is a different and smaller question.
