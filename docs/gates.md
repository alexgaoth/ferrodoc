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
./scripts/verify.sh          # tests, clippy, wasm32, resource bounds, all gates
./scripts/verify.sh --fuzz   # and 500k mutations on top
./scripts/verify.sh --wasm   # the npm package, including a headless browser
./scripts/verify.sh --c      # the C ABI, and its example under valgrind
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
