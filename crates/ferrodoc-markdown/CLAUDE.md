# ferrodoc-markdown

Reads `CommonMark` and GFM with comrak, writes both. Gated by `diff-spec`,
`diff-md`, `diff-gfm` and `diff-gfm-md` — see the root `CLAUDE.md`.

## comrak

- `comrak` needs `default-features = false`: its `syntect` feature drags in
  the C library `onig_sys` and breaks wasm32.
- Sourcepos **end** lines are unreliable for unclosed code/HTML blocks (they
  report `[1..1]`); use `NodeCodeBlock::closed` and `start.line +` literal
  line count.
- comrak already merges adjacent Text nodes — do not add `Str` coalescing.
  The one normalization pandoc needs on top: merge directly-adjacent
  same-type `Emph`/`Strong` siblings.
- comrak is ~78% of markdown→HTML, so tuning our mapping barely moves that
  benchmark; the DOCX paths are where reader/writer work pays.

## Divergences from pandoc

- Three reader divergences are OPEN — don't rediscover: entity-encoded
  spaces (`&#32;`) should stay inside `Str`; refdef+dash-run corner cases
  (repros in `.iterate/20260810-markdown-reader/round-3-verdict.md`); and a
  lone `\` on its own line, which is one `LineBreak` for pandoc and
  `SoftBreak`+`LineBreak` here. The writer emits that last shape, so closing
  it would make a hard break after a soft one round-trip exactly.
- Our `gfm` = the five GFM-*spec* extensions plus heading ids. Pandoc's also
  bundles emoji, footnotes, alerts, math and YAML metadata, so `diff-gfm`
  mismatches there are expected. comrak is a port of GitHub's cmark-gfm and
  pandoc's commonmark-hs is stricter than it; where they disagree we follow
  GitHub. Seven such divergences are deliberate — don't "fix" them; they are
  tabulated in `COMPATIBILITY.md` and pinned by
  `deliberate_divergences_from_pandoc_hold`. The two that matter: a pipe
  table may interrupt a paragraph, and a plain line after a row is a row.
- Two GFM list rules are bridged in the mapper: pandoc splits a bullet list
  where a task item meets a plain one, and has no task items in ordered
  lists (so the literal `[x]` is written back). Each run of a split list
  works out its own tightness.
- The space a task marker contributes is the source's own: `- [ ]  two` must
  not gain a second `Space`, and an item with no content gains no space and
  takes the list's own block type (`Plain` tight, `Para` loose).

## Writer

- Gated on *fidelity* (`diff-md`: read back what we wrote, require the
  original), not on matching pandoc's markdown, which is lossy — 593/652 at
  `--wrap=preserve`, 535/652 at `--wrap=none`. Matching it would copy its
  losses. Score pandoc at its best setting.
- Never emit four adjacent tildes: `~~a~~~~b~~` is a tilde code fence that
  swallows the rest of the document (pandoc's own writer does emit it).
  Adjacent `Strikeout` merges into one run; strikeout at the very edge of
  strikeout drops a level. Nesting with text on both sides is exact.
- A fence's info string is the first class that is not `sourceCode`, after
  a space (`` ``` bash ``). Pandoc's HTML writer classes every code block
  `sourceCode`, so taking `classes.first()` labelled every `html → gfm`
  block `sourceCode`; no round-trip gate can see it, because a `CommonMark`
  info string is one word and the reader never makes a two-class list.
- A pipe cell is escaped twice, by `escape_text` for `|` and again by
  `cell_text`, so `cell_text` counts backslash parity. cmark-gfm tolerates
  the doubled escape, so only a unit test catches losing it.
- GFM corpus files are `corpus/gfm/*.gfm`, never `.md`: `diff-ast` and
  `diff-write` walk the same `corpus` tree and take every `.md` in it.
