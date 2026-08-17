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

The gates, and what each one proves:

| gate | proves |
|---|---|
| `diff-spec` | the markdown reader produces pandoc's AST |
| `diff-ast` | any `pandoc -t json` document round-trips to an equal value |
| `diff-html` | the HTML writer produces pandoc's HTML |
| `diff-html-read` | the HTML reader produces pandoc's AST |
| `diff-md` | the markdown writer round-trips the document |
| `diff-gfm` / `diff-gfm-md` | the same, for GFM |
| `diff-docx` / `diff-odt` | the office readers produce pandoc's AST, over *two* corpora each: pandoc's own output, and a word processor's |
| `diff-write` / `diff-odt-write` | the office writers survive a round trip — ours through pandoc against pandoc's through pandoc, which is what isolates the writer from the format |
| `diff-epub` | the EPUB reader produces pandoc's AST, over three corpora that measure three different things |
| `diff-epub-write` | the EPUB writer survives a round trip — and is **deliberately** below 100: it refuses to emit a reference the book cannot satisfy, where pandoc emits one and `epubcheck` rejects the result |
| `diff-latex` / `diff-rst` | the text writers round-trip the document, with pandoc's score on the same corpus printed beside it |
| `bench-rss` | no conversion path exceeds its published multiple of the input |

Some checks are not differential because there is no oracle, and those
are the ones a *toolchain* judges: `pdflatex` compiles the LaTeX,
`sphinx-build -W` reads the RST, `asciidoctor` reads the AsciiDoc,
`epubcheck` validates the EPUB fixtures **and every book the writer
produces**, a headless browser runs the npm package, and valgrind runs the
C example. Pandoc cannot read AsciiDoc at
all, so for that writer the toolchain is the *only* judge.

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
the `.docx`/`.odt`/`.epub` themselves, which embed zip mtimes and
generated ids — their `*.readback.md`, which is what the rest of the world
sees when it opens the file, is compared instead.

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
