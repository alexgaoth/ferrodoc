# ferrodoc-asciidoc

Writes AsciiDoc. **There is no differential gate and there cannot be** —
see the root `CLAUDE.md`.

- **Pandoc writes AsciiDoc and cannot read it.** There is no oracle, so
  this writer is judged by `asciidoctor --failure-level=WARN` accepting
  its output in CI, and by the tests in this crate holding the shapes a
  toolchain accepts but silently mis-renders. Do not add a `diff-asciidoc`
  expecting it to work; the harness records why.
- **The emphasis markers are the opposite way round from markdown.** `_x_`
  is italic, `*x*` is bold. Getting it backwards produces a document that
  looks almost right, which is why it has a test of its own.
- **A fence must be longer than any run of the same character inside it.**
  A listing containing `----` fenced with `----` ends where the sample
  does, and the rest of the document silently becomes prose. `fence_for`
  exists for this.
- **Headings start at `==`.** A single `=` is the document title and may
  appear only once, so a document with two level-1 headings would be
  invalid.
- **Nesting depth is the marker's length**, not indentation: `*` then `**`.
  A second paragraph inside an item is attached with a `+` line, or it
  escapes the item.
- A newline inside a table cell starts a new cell and takes the rest of
  the row with it, so `cell_text` flattens.
