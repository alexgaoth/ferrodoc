# ferrodoc-rst

Writes reStructuredText. Gated by `diff-rst` (fidelity) and, in CI, by
`sphinx-build -W` accepting the output — see the root `CLAUDE.md`.

- **No reader, deliberately.** People write RST by hand and convert *out*
  of it; `TODO.md` lists it under explicit non-goals.
- **The gate is fidelity with pandoc's score beside it, and the ceiling is
  low for both**: pandoc round-trips 3/11 of `corpus`, we do 2/11. RST
  cannot nest inline markup at all, has no link title and no strikeout, so
  a document using any of those cannot come back unchanged. The number
  measures the *format*; `sphinx-build` measures the writer.
- **An underline is measured in characters, not bytes.** A heading with an
  accent in it is longer in bytes than it looks, and an underline shorter
  than the title is a warning — which `sphinx-build -W` turns into a
  failure.
- **The level-to-underline map is fixed.** RST infers the hierarchy from
  the order the characters first appear, so deriving it per document would
  make level 2 mean different things in two files Sphinx reads together.
- **Indentation is nesting**, so every nested construct is rendered whole
  and then shifted (`indent`), never written with a running prefix.
- **A picture alone in a paragraph is the `image` directive.** As a
  substitution it would need a name, and the name becomes the alt text —
  so an image with no alt text acquires one.
- A grid table, not a simple one: only the grid form can hold a cell with
  more than one line, and docutils rejects a table whose rules do not line
  up rather than mis-rendering it. `a_grid_table_lines_up` holds that.
