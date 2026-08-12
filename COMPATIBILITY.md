# Compatibility with pandoc 3.8.2.1

What ferrodoc converts identically to pandoc, what it converts differently,
and what it does not convert at all. Every number here is produced by a
command in this file; none of it is an estimate.

Pinned to **pandoc 3.8.2.1**. A different pandoc will produce different
numbers — some of these behaviours changed between releases, and pandoc's
published sources describe a later pandoc than this binary.

## Formats

| | read | write |
|---|---|---|
| CommonMark | yes | yes |
| HTML | yes | yes (fragments; no `--standalone`) |
| DOCX | yes | yes |
| pandoc JSON AST | yes | yes |
| plain text | — | yes |

Everything else pandoc supports — LaTeX, EPUB, RST, Org, presentations, the
other ~36 — is a declared non-goal. See `TODO.md`.

## Measured conformance

```sh
export PATH="$HOME/.cargo/bin:$HOME/.local/bin:$PATH"
cargo run -p ferrodoc-harness -- diff-spec      corpus/commonmark-spec-0.31.2.json --fail-under 100
cargo run -p ferrodoc-harness -- diff-ast       corpus --fail-under 100
cargo run -p ferrodoc-harness -- diff-html      corpus/commonmark-spec-0.31.2.json --fail-under 100
cargo run -p ferrodoc-harness -- diff-docx      corpus/docx --fail-under 96
cargo run -p ferrodoc-harness -- diff-write     corpus --fail-under 90
cargo run -p ferrodoc-harness -- diff-md        corpus/commonmark-spec-0.31.2.json --fail-under 100
cargo run -p ferrodoc-harness -- diff-html-read corpus/commonmark-spec-0.31.2.json corpus --fail-under 95
```

| gate | what it proves | result |
|---|---|---|
| `diff-spec` | markdown reader produces pandoc's AST | **652/652** |
| `diff-ast` | any pandoc JSON round-trips to an equal value | **11/11** |
| `diff-html` | HTML writer produces pandoc's HTML | **652/652** |
| `diff-docx` | DOCX reader produces pandoc's AST | **36/37** |
| `diff-write` | DOCX writer survives a round trip through pandoc | **10/11** |
| `diff-md` | markdown writer round-trips the document | **652/652** (pandoc: 593/652) |
| `diff-html-read` | HTML reader produces pandoc's AST | **631/657** |

`diff-md` is the one place ferrodoc is measurably *ahead*: pandoc's own
commonmark writer loses 59 of the same 652 documents, at its best setting.

## Known losses, one by one

Nothing here is a surprise waiting to happen; each is a decision or a
measured gap with a reason.

### Markdown reader — 2 open divergences

- Entity-encoded spaces (`&#32;`) should stay inside a `Str` and do not.
- Link reference definitions followed by a run of dashes, in corner cases.

Repros: `.iterate/20260810-markdown-reader/round-3-verdict.md`.

### DOCX reader — 1 corpus document

`corpus/docx/spec-09.docx`: a list nested inside a table cell in a shape the
reader flattens.

### DOCX writer — 1 corpus document, and three categories

- `corpus/nested-structures.md`: a quotation nested in a way the round trip
  does not preserve.
- **Raw blocks** are dropped: OOXML has no equivalent.
- **Images** embed as PNG, JPEG and GIF only. SVG, WebP, TIFF and EMF fall
  back to alt text rather than produce a package Word would reject.
- **`docx → docx` loses images**: the reader records the media part's path,
  not its bytes, so a re-write has nothing to embed. Closing this needs a
  media bag on the reader.

### Markdown writer — 4 limits of CommonMark itself

Listed in `crates/ferrodoc-markdown/src/write.rs`. Briefly: no tables,
footnotes or definition lists; emphasis directly inside emphasis inside a
word; two ordered lists in a row sharing a delimiter; an unterminated raw
HTML block swallowing the blank line after it.

### HTML reader — 26 of 657

Most are one cause: **ferrodoc parses to the HTML5 spec via `html5ever`,
pandoc parses with `tagsoup`, which does not.** On malformed markup the two
build different trees and no mapping reconciles them. Not all of them are
that, and `TODO.md` names the exceptions.

Two deliberate divergences, both chosen on the same principle — *match
pandoc wherever pandoc has a describable rule on well-formed input; diverge
only where matching would mean reproducing a parse failure*:

- An `<a href="…"></a>` with no text is **kept**. Dropping it would match
  pandoc on unclosed `<a>` tags but delete a well-formed empty anchor, which
  real pages use as jump targets.
- A newline immediately after `<pre>` is **kept**, matching pandoc rather
  than the HTML spec, because a code block silently losing its first line is
  worse than disagreeing about an invisible character.

## Where ferrodoc behaves differently on purpose

- **Deterministic output.** The same AST always produces the same `.docx`
  bytes. Pandoc embeds timestamps, so its output differs run to run.
- **Bounded recursion.** Every recursive path is depth-limited and readers
  return `Err`; `unsafe` is forbidden workspace-wide. Two fuzz-found DOCX
  files that hang pandoc for 60 s are handled here in 12 ms and 3.5 s.
- **`data-` attribute symmetry.** A name the AST does not recognize is read
  without its `data-` prefix and written back with it, so a round trip
  cannot turn `data-onclick` into an event handler that runs. Pandoc's
  writer does the same.

## Resource limits worth knowing

Measured with `ferrodoc-harness bench-sizes` and `/usr/bin/time`; see the
table in `TODO.md`. The one to plan around:

**`docx → markdown` peaks at ~3.5 GB of RSS for a 4.3 MB `.docx`** — the
whole XML tree and the whole AST are live at once. Fine on a laptop, fatal
in a small container. Bounded-memory DOCX reading is on the roadmap for
exactly this reason.

## How to check any of this yourself

Everything above is reproducible from a clone with pandoc 3.8.2.1 on the
path. CI runs the same gates on every push, plus a 500,000-mutation fuzz
campaign with a fresh seed each run: `.github/workflows/ci.yml`.
