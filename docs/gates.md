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
./scripts/verify.sh          # tests, clippy, wasm32, and all 14 gates
./scripts/verify.sh --fuzz   # and 500k mutations on top
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
