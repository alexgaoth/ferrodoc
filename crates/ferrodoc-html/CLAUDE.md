# ferrodoc-html

Reads and writes HTML. Gated by `diff-html` (writer) and `diff-html-read`
(reader) — see the root `CLAUDE.md`.

- This reader uses `html5ever` (spec-compliant), pandoc uses `tagsoup` (not).
  *Most* `diff-html-read` mismatches are that, on malformed markup — do not
  assume it: reduce the input to well-formed first, because real mapping bugs
  hid behind that excuse (column widths, `<pre>` newlines, `<del>` around
  blocks). Pandoc also expands tabs over the raw *source*, so a tab's column
  counts the surrounding markup.
- Escaping differs by context: text `&<>` only; attributes AND code blocks
  additionally `"`→`&quot;`, `'`→`&#39;`; inline code escapes like text (no
  quote escaping). Verified against pandoc; don't "unify" them.
- The reader and writer share `is_reserved`, and must: the reader drops a
  `data-` prefix the writer puts back. Break the symmetry and a round trip
  turns `data-onclick` into an event handler that runs. Pandoc does the same,
  which is why guarding this in the reader alone is the wrong fix.
- Pandoc merges directly-adjacent same-type `Emph`/`Strong`/`Strikeout`/
  `Sub`/`Super`/`Underline` siblings, and this reader must too. It also
  hoists whitespace out of an inline element first, which this reader does
  NOT — `<em> b</em>` is `Space`+`Emph[b]` for pandoc, `Emph[Space, b]` here.
  OPEN.
- `write_html_standalone` frames what `write_html` emits — one writer,
  two framings, asserted by the test comparing the body against
  `write_html`. Metadata reaches the head as *text*: escape it, and
  flatten a `MetaList`, because `author` is routinely a list. CSS is
  inlined verbatim (escaping it would break every `>` selector), so
  `</style` is the one sequence neutralized.
- An HTML fixture containing `<main>` tests **only what is inside it**: both
  readers select that element as the document. A `<main>` block at the end of
  `corpus/inline-elements.html` silently disabled the other 49 lines of it
  for two rounds. Keep `<main>` cases in `corpus/main-content.html`.
