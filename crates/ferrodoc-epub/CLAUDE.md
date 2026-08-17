# ferrodoc-epub

Reads and writes EPUB. The reader is gated by `diff-epub` over **three**
corpora and the writer by `diff-epub-write` — see the root `CLAUDE.md`.

## The writer

- **`diff-epub-write` is 8/11 on purpose, and raising it means writing
  invalid books.** The three that differ all differ one way: this writer
  refuses to emit a reference the book cannot satisfy — a picture with no
  bytes becomes its alt text, a relative link naming no file in the book
  becomes its text. Pandoc emits both and `epubcheck` rejects its book for
  them. **Check `epubcheck` before "fixing" a case**; it is the judge that
  says which side is right, and CI runs it over every book written from
  `corpus/`.
- **Four fields cannot be matched and are dropped by the gate**: pandoc's
  random `dc:identifier`, its `dcterms:modified` clock, its locale-derived
  `dc:language`, and the `dc:title` it omits although EPUB 3 requires one.
  The gate drops each only in its exact unmatchable form, so a book that
  loses its identifier still fails. Do not widen those rules to buy a
  point.
- **The section classes are `section`, `level{N}`, then the heading's
  own.** Pandoc writes a real `<section class="level1">` and its reader
  adds `section` back from the element name; `ferrodoc-html` writes a
  `<div>`, so the class has to be written. Dropping it scored **0/11**,
  every case differing in that one string.
- **A heading with no identifier is given one** — slugged, uniqued
  document-wide with `-1`, `-2`, an empty heading taking `section` — and
  the identifier lives on the section, never on the `<hN>`.
- **Content before the first level-1 heading gets a synthesized empty
  heading** so it becomes a chapter instead of vanishing. A document with
  no level-1 heading is therefore one chapter, not zero.
- **An unterminated HTML comment is closed, not dropped.** XML has none of
  HTML's tolerance and the book will not open at all — `epubcheck` says
  `RSC-016`. Repair the raw *fragment*, never the rendered chapter:
  applied to the whole chapter it ate the writer's own `</li></ul>` and
  traded one fatal for another. A well-formed comment is left alone;
  dropping those cost a gate case.
- **Media is taken from the AST before rendering**, not scraped out of the
  emitted XHTML. The URL a chapter carries is already `../media/imageN`,
  which is the step the reader takes back off.

## The reader

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
