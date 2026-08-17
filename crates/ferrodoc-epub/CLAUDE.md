# ferrodoc-epub

Reads EPUB. Gated by `diff-epub` over **three** corpora — see the root
`CLAUDE.md`. There is no writer yet; `TODO.md` item 4 has its criteria.

- **This crate is packaging, not parsing.** An EPUB's content documents are
  XHTML, so `ferrodoc-html` does the work. Everything here is the spine and
  what concatenating files does to a document.
- **Chapters are read with
  `read_html_without_generated_identifiers`.** Pandoc's EPUB reader invents
  no heading identifiers, and it must not: chapters become one document,
  so an identifier invented per chapter is invented against the wrong
  namespace. Using plain `read_html` gave every heading a slug and failed
  every fixture at once.
- **The manifest href and the archive entry are different strings.** A
  space is `%20` in the href and literal in the zip, so the file is found
  by the decoded path — while the anchor is named for the **raw** href.
  `Item` carries both on purpose.
- `linear="no"` contributes **nothing**, not even its anchor. A title page
  contributes its anchor and nothing else. Both drop content a reading
  system shows as furniture, and keeping either grows a duplicate heading
  on every round trip.
- Identifiers are prefixed with the file, and **links must follow** —
  prefixing alone leaves every cross-reference in the book pointing at
  nothing. Do both or neither.
- An **image** src is resolved against the chapter's directory; a **link**
  href is left exactly as written. That asymmetry is pandoc's: its writer
  bundles media at the package root and rewrites each `src` to reach it
  (`../moon.jpg`), so its reader undoes that one step and nothing else.
- **Footnotes are scanned out of the XHTML source, not the parsed blocks.**
  The HTML reader drops the `<aside>` wrapper and with it the identifier
  that says which note it is. `footnote_bodies` does a depth-aware scan;
  do not "simplify" it to work from the AST, because the AST no longer has
  what it needs.
- The three corpora measure three different things and are gated
  separately on purpose. `corpus/epub` is pandoc's output. **`corpus/epub-handmade` is where the bugs were** — EPUB 2, an `OEBPS/`
  layout, a package document at the archive root, a spine that is not the
  file order, a `linear="no"` cover, a percent-encoded href; it found the
  identifier and href rules above, and it is gated at 100.
  `corpus/epub-spec` is 22 files of 30 spec examples each, so one of the
  HTML reader's 26 known divergences fails a whole document — it measures
  compounding, not EPUB fidelity, and averaging it in would report the
  HTML reader's score under this crate's name.
- `epubcheck` validates the hand-authored corpus in CI. It is the only
  judge here that does not know what we intended, and it caught an
  incomplete `toc.ncx`, a missing nav document and an unreachable
  non-linear item.
