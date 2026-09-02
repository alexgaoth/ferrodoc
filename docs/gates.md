# The gates

The differential gates, with the thresholds they are held to. **Run every
one of them before claiming a reader or writer change is done** — each is
cheap, and between them they are the only reason to believe any number this
project publishes.

These are also the `conformance` job of `.github/workflows/ci.yml`, and the
table in `COMPATIBILITY.md`. **Keep all three in step when a threshold
moves**; a threshold changed here and nowhere else is a gate that has quietly
stopped gating.

```sh
./scripts/verify.sh           # tests, clippy, wasm32, resource bounds, gates, samples
./scripts/verify.sh --samples # only: samples/ is still what this tree produces
./scripts/verify.sh --fuzz    # and 500k mutations on top
./scripts/verify.sh --wasm    # the npm package, including a headless browser
./scripts/verify.sh --c       # the C ABI, and its example under valgrind
```

**`scripts/verify.sh` is where every threshold lives, and the only place.**
CI calls it rather than repeating the list, so a threshold cannot be lowered
in one file and left standing in another — that had already happened twice.
Read the script for the current numbers; they are not duplicated here on
purpose.

Conformance is pinned to **pandoc 3.8.2.1**, and the script refuses to score
against any other version rather than publish a number that means something
else. A green run means "identical to this pandoc".

## What a green run does not prove

The gates are strong regression evidence, not a blanket compatibility or
security certificate. These limits are deliberate and must stay visible:

- **Platform and oracle scope.** Differential conformance runs against pinned
  pandoc 3.8.2.1 on Linux. The project also builds and tests on macOS and
  Windows, but it does not publish a cross-version or cross-platform pandoc
  equivalence claim.
- **The real-CLI floor is the score.** The corpus measures 47/48 identical
  commands and the threshold is 47, so every one of them is the supported
  contract and a single row going backwards fails the run. It sat at 11
  while the score climbed 26 -> 33, which had left 22 passing commands free
  to regress unread. The one row that differs, `dropin-006`, is a
  deliberate divergence.
- **A sub-100 format floor is not completion.** It prevents a measured
  baseline from falling; it does not turn a 96% reader or a 70% dialect
  reader into general compatibility. Read the current score and each named
  divergence in `COMPATIBILITY.md` before relying on that path.
- **The commands are representative, not literal substitution tests.** 27 of
  48 have a retargeted output format or local stand-in asset; they test the
  command and flag shape, not an unmodified external workflow. See
  `dropin/README.md`.
- **The AST sweep is exhaustive over constructors, not over their
  composition.** Its *flat* axis puts each implemented AST variant alone at
  the top of a document, and its *composition* axis crosses container against
  content — 286 cases with a floor for each axis. Neither covers attribute
  *values*, input grammar, or real document style, and the composition axis
  is one fixed cross product rather than every nesting.
- **The math corpus is hand-picked, not generable.** TeX is not a closed
  set of constructors the way the AST is, so `scripts/math.sh` asks 81
  expressions chosen for what they exercise — scripts, spacing classes,
  fonts, accents, and five that pandoc itself will not render. It says
  what those cover and nothing about the rest of the language. Depth is
  the exception: the parser refuses past 200 levels and falls back to the
  TeX source, which unit tests pin at 20_000 levels on every recursive
  path rather than the math corpus.
- **EPUB writer read-back currently has a floor of zero.** It runs and reports
  differences, but does not prevent an additional EPUB semantic regression.
  `epubcheck` is the validity check; it is not an equivalence gate.
- **The resource gate is narrow.** It bounds supported conversions of a
  generated 10 MB Markdown file to 80 times input RSS. It does not yet impose
  a whole-input/output limit or prove resistance to every hostile archive.
- **Fuzzing is evidence, not a proof.** CI runs corpus mutations for panics
  and hangs; the default local verification does not, and it is not a
  coverage-complete security audit.

The gates, and what each one proves:

| gate | proves |
|---|---|
| `diff-spec` | the markdown reader produces pandoc's AST |
| `diff-ast` | any `pandoc -t json` document round-trips to an equal value |
| `diff-html` | the HTML writer produces pandoc's HTML |
| `diff-html-read` | the HTML reader produces pandoc's AST |
| `diff-md` | the markdown writer round-trips the document |
| `diff-gfm` / `diff-gfm-md` | the same, for GFM |
| `diff-pandoc-md` | the **pandoc-markdown** reader produces pandoc's AST — a YAML metadata block, header attributes, definition lists and super/subscript, on its own corpus with its own extension so that a document written for one dialect is not scored by writers that have none |
| `diff-docx` / `diff-odt` | the office readers produce pandoc's AST, over *two* corpora each: pandoc's own output, and a word processor's |
| `diff-write` / `diff-odt-write` | the office writers survive a round trip — ours through pandoc against pandoc's through pandoc, which is what isolates the writer from the format |
| `diff-epub` | the EPUB reader produces pandoc's AST, over three corpora that measure three different things |
| `diff-epub-write` | the EPUB writer survives a round trip — and is **deliberately** below 100: it refuses to emit a reference the book cannot satisfy, where pandoc emits one and `epubcheck` rejects the result |
| `diff-ipynb` | the notebook reader produces pandoc's AST, over a hand-authored corpus in the shape Jupyter and Colab write — not the shape pandoc's own writer emits |
| `diff-ipynb-write` | the notebook writer survives a round trip, from the AST **pandoc** read out of the corpus so the reader cannot flatter the writer; `nbformat.validate` is the judge that is not pandoc |
| `diff-rst` | the RST writer round-trips the document, with pandoc's score on the same corpus printed beside it |
| `diff-latex` | **printed, and cannot fail the run.** Pandoc round-trips 1/13 of this corpus — our number exactly — so any floor would be a number chosen after seeing the score. The LaTeX writer is decided by `pdflatex` in CI and by literal-output tests |
| `math.sh` | every TeX expression this renders is rendered as pandoc renders it — and every one it *gives up on* is one pandoc gives up on too, since the sweep asks about `$x^2$` and nothing else |
| `bench-rss` | no conversion path exceeds its published multiple of the input |

Some checks are not differential because there is no oracle, and those
are the ones a *toolchain* judges: `pdflatex` compiles the LaTeX,
`sphinx-build -W` reads the RST, `asciidoctor` reads the AsciiDoc,
`epubcheck` validates the EPUB fixtures **and every book the writer
produces**, a headless browser runs the npm package, and valgrind runs the
C example.

## Three comparisons that are not gates over a corpus

The gates above score *conversions*. These score the things a gate cannot
see, and `verify.sh` runs all three:

| script | what it asks | how it is held |
|---|---|---|
| `scripts/flags.sh` | every CLI flag, against pandoc, over every document in `corpus/` | **gated at 100** — a flag's whole job is to produce particular bytes |
| `scripts/dropin.sh` | 48 real pandoc command lines from public Makefiles and CI files, byte for byte, with every miss classified | gated at a **count** of rows, not a percentage |
| `scripts/writers.sh` | each text writer against **pandoc's own writer**, on the same AST, over 21 documents in two wrap modes | gated at **one floor per writer**, each the score that writer reached |

`writers.sh` is why "pandoc cannot read AsciiDoc, so it has no oracle" is
no longer the whole story: pandoc *writes* every text format this writes,
so the bytes are an oracle for all of them — and it is the only judge
AsciiDoc has ever had.

Two things about it are worth knowing before reading its number.

**Its corpus is 21 documents, run twice each — as they fall and filled.**
Four are GFM and nine are this repository's own prose; the eight in
`corpus/` are read as CommonMark, which has no table, no task list and no
footnote — so a score over them alone cannot see the constructs the
writers are worst at. Adding the four in `corpus/gfm/` found, on the
first run, that the HTML writer **dropped every footnote** while
`diff-html` read 652/652: that gate is markdown → HTML over the
CommonMark suite, which has no footnote to lose.

**Its floors are the scores, not a range.** Every point below one is a
document that used to be byte-identical and is not any more, which is a
regression. A floor chosen after seeing a score is not a floor, which is
why this was a measurement for as long as the numbers were low; it became
a gate when five of the seven writers reached the whole corpus or came
within one document of it. `COMPATIBILITY.md` has the table.

## `samples/`, which is where the gates are blind

`diff-html` scores against the CommonMark specification, which has no
tables in it at all, and `diff-md`/`diff-gfm-md` round-trip through this
project's own reader, which never produces an inline CommonMark cannot
spell. Three silent data losses lived in those two blind spots with every
gate above green — table column alignment, table column widths, and
`Superscript`/`Subscript`/`Underline`/`SmallCaps`/`Span`. `samples/` is
what found them, and `./scripts/verify.sh --samples` is what keeps them
found: it regenerates every sample into a scratch directory and requires
the committed artefacts to match, so the folder is a check rather than a
habit. The tree is never written to, so a passing run leaves it clean.

Exactly two things are ignored, because nothing can make them equal: the
`---`/`+++` header lines of a diff, which carry the run's timestamp, and
every `.docx`/`.odt`/`.epub`, which embed zip mtimes and generated ids —
their `*.readback.md`, which is what the rest of the world sees when it
opens the file, is compared instead.

Everything else is compared, and that includes `samples/inputs/`. Those
six files are not hand-maintained: five are copied from `corpus/` on every
run and `page.html` is written by pandoc, so a change there is a real
change. They were outside the compare scope until 2026-08-19, while both
this file and `generate.sh` said everything matched byte for byte — the
same defect this page exists to catch, in the page itself.

**It costs ~3 s of the ~110 s `./scripts/verify.sh` run** (measured warm,
including the `cargo build --release -p ferrodoc` it needs), which is why
it is in the default run rather than behind a flag like `--wasm` or `--c`:
at 3% of the suite there is nothing to buy by skipping it, and a check
that has to be remembered is the unchecked guarantee it replaces. When it
fails, run `./samples/generate.sh`, **read** the diffs — a change there may
be an improvement, but it is never nothing — and commit them with the
change that moved them.

## Reading a failure

Each gate prints `MISMATCH <case> at <json-pointer>: ours=… theirs=…`,
pointing at the **first** divergence in document order. That pointer is a
path into the pandoc JSON AST, so `/blocks/4/c/0/c` is
`blocks[4].c[0].c` — usually enough to identify the rule without dumping
either tree. `--verbose` prints the whole case.

A gate that *rises* deserves as much suspicion as one that falls: widening
what a gate collects can raise the score while covering less. That happened
here — eight DOCX corpus sources landed in the HTML gate, passed, and pushed
the number up.
