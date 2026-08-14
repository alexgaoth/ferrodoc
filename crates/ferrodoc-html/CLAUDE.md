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
- **Content a browser hides is still content.** `html5ever` parses a
  `<template>`'s content into a fragment of its own rather than into the
  element's children, so `children()` reads `template_contents` — walking
  children alone returned nothing at all. And the parser is asked for
  `scripting_enabled: false`, which makes a `<noscript>` hold markup rather
  than the raw text a browser leaves unparsed; pandoc has no notion of
  scripting, so that is also what it reads. Both were losing every word.
- `flatten()` takes `template_contents` as well as children. The content
  hangs off the element by a second link, so taking children alone leaves
  that chain for the recursive `Rc` drop to walk — the one thing the
  function exists to prevent.
- An inline `<svg>` is a picture, and is serialized back to markup and
  carried as a `data:` URL — the only way a picture with no file behind it
  survives into another format. Two things this must not do: lowercase the
  names (pandoc lowercases element *and* attribute names, and SVG is
  case-sensitive — `viewBox` becomes `viewbox` and renders `31x31` where
  the correct one renders `61x41`, and `<linearGradient>` becomes an
  element that does not exist), and grow a base64 dependency (twenty
  lines, because every crate below the facade builds for wasm32 with no C
  library).
- Decoding a `data:` URL back to bytes lives in the **facade**, not here:
  `render_with_media` answers one before consulting the caller's resolver,
  because a resolver would look for a file of that name and find nothing.
  Without it an inline `<svg>` reaches the DOCX writer as an unresolvable
  URL and comes out as alt text — the reader alone is not enough.
- A `<q>` is a quotation, not its children: read as its children the text
  no longer says the words are someone else's. The marks alternate with
  nesting (double, then single), and `Quoted` carries no attributes — the
  element's are dropped, here and in pandoc.
- The span-with-a-class family is `abbr`, `dfn`, `kbd`, `mark`, `small`.
  Measured one element at a time; it cannot be guessed from what the tags
  mean, which is why `cite` is not in it and `var`/`samp` are code instead.
- Pandoc counts `<output>`, `<canvas>` and `<textarea>` block-level and
  splits a paragraph around them. This reader does not: all three are
  phrasing content. Deliberate, and in `COMPATIBILITY.md`.
- **Sweep the element vocabulary in a context where each element is
  valid.** Testing every tag inside a `<p>` mostly measures how two
  parsers recover from invalid markup: 35 tags "differed" that way, of
  which 12 survived being retested in a valid position and only 5 were
  real. `<td>` outside a table proves nothing.
- An HTML fixture containing `<main>` tests **only what is inside it**: both
  readers select that element as the document. A `<main>` block at the end of
  `corpus/inline-elements.html` silently disabled the other 49 lines of it
  for two rounds. Keep `<main>` cases in `corpus/main-content.html`.
