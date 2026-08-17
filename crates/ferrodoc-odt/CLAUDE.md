# ferrodoc-odt

Reads and writes `OpenDocument` text. Gated by `diff-odt` (reader, over two
corpora) and `diff-odt-write` (writer) — see the root `CLAUDE.md`.

- **Pandoc's ODT reader is much plainer than its docx one.** It produces no
  metadata, no code blocks, no table spans, no column widths and no cell
  alignments — a code block written by pandoc's own ODT writer does not
  survive its own reader. Do not add a mapping for something its reader
  cannot produce: `diff-odt` would fail on every document that has one.
- **A block quote is an indent, not a style name.** `fo:margin-left` at or
  above **5.5 mm** is a quote; below it is a paragraph. `Quotations`
  qualifies only because its usual definition carries 0.3937 in, and
  `Table Contents` (0.76 mm) and `Footnote` (5.0 mm) sit deliberately under
  the line — reading the name instead wrapped every table cell in a quote.
  The margin is the **largest anywhere in the parent chain**, not the
  nearest: a style re-declaring `0in` over an indented parent is still a
  quote, and two 0.15 in steps do not add up to one. Inside a list item the
  rule does not apply at all.
- **Text properties are not inherited through `style:parent-style-name`;
  paragraph properties are.** A text style whose parent is bold and which
  adds italic is italic *alone*, which is also why a style descending from
  `Source_Text` is not code. Both halves are load-bearing and they differ.
- Inline code is the character style **named** `Source_Text`. A style
  carrying its exact properties under another name is not code, and a
  monospaced font is not either — a fixed-pitch font gives `Emph`.
- **Inlines meld only at the seam.** Appending two sequences merges the
  last element of one with the first of the other and leaves the rest
  alone, which is pandoc's builder: three spaces from one `text:s` stay
  three, while the space ending one text node and the space starting the
  next become one. `append`/`meld` in `lib.rs`, and getting it wrong is
  invisible until a code sample loses its indentation.
- Every bookmark is called `anchor`. The identifiers treated as taken are
  the **values of the bookmark map**, so a heading whose identifier equals
  a bookmark's name rebinds that entry and frees `anchor` for the next one.
  That is why the second bookmark in a pandoc-written document is `anchor`
  again rather than `anchor-1`.
- A `table:covered-table-cell` **shortens its row**, which is padded at the
  end. Filling the covered position instead shifts every later cell one
  column right; a LibreOffice table with a span came apart that way and the
  pandoc corpus could not see it.
- The writer must **flatten run formatting into one span**: nesting a bold
  span inside an italic one reads back as `Emph [Strong [Emph …]]`, because
  the reader applies the whole accumulated property set at every level.
- The writer gives **every list its own style, declaring every level up to
  the depth that list sits at**. One style shared by nested lists lets two
  sibling sublists overwrite each other's marker; a style that stops at
  level one makes a word processor number a nested bullet list.
- The `mimetype` entry must be **first and stored uncompressed**, or
  LibreOffice refuses the package. Checked by
  `the_package_starts_with_a_stored_mimetype_entry`.
- A link's `xlink:href` gets one `../` on the way out and loses one on the
  way in — the step out of the package. A URL with a scheme, or a bare
  `#fragment`, is left alone.
- **Pandoc reads every list twice**, and 2^n times at n levels of nesting.
  Visible only as a higher identifier suffix on a heading or bookmark
  inside a list, and deliberately not reproduced; `corpus/odt/spec-03.odt`
  and `spec-09.odt` are the two documents that cost.
- Regenerate the corpora with `bash corpus/odt/generate.sh` and `bash
  corpus/odt-libreoffice/generate.sh`. Zip bytes are not reproducible, so
  check conformance, never `git diff`.
