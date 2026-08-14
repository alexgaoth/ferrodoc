# ferrodoc-docx

Reads and writes OOXML word processing documents. Gated by `diff-docx`
(reader) and `diff-write` (writer) — see the root `CLAUDE.md`.

- The body is **streamed**: `xml::body_children` yields one `w:p`/`w:tbl`
  subtree at a time and `blocks_inner` consumes an iterator with one
  element of lookahead, so `document.xml`'s tree never exists in full. It
  cost about twenty times the XML. Do not reintroduce a whole-body
  `xml::parse`, and keep the scan forward-only with at most one lookahead.
- Streaming moves errors from before the walk to during it, so both ends
  need guarding: a malformed prologue, a missing body and an unclosed
  element at EOF are all `Err`, never a short document. Pinned by
  `a_malformed_body_is_refused_not_truncated`.
- The body's `w:sectPr` is its *last* child and text width is needed
  first, so `body_section` takes the last one in the source and parses it
  alone. A `sectPr` inside a `w:pPr` is a section break and always
  precedes it.
- Style matching is by *name* through `styles.xml` (case-insensitive,
  whitespace-exact), never by id — ids are localized and may be absent.
  Captions and heading classes use the paragraph's OWN name; block semantics
  (quote, code, compact) inherit through `basedOn`.
- Writing is verified by round trip (`diff-write`), never by zip bytes.
  `w:rPr` children must be emitted in OOXML schema order — an out-of-order
  `w:rStyle` silently loses inline code.
- Images embed only when the caller supplies bytes
  (`write_docx_with_media`); this crate does no IO, which is what keeps it
  building for wasm32. `diff-write` normalizes media part names before
  comparing: the two writers number them differently, and that is zip
  layout, not content.
- `read_docx_with_media` keys the bag by the URL the AST names — the
  relationship target, relative to `word/` unless written package-absolute
  — because that is the string `write_docx_with_media`'s resolver is
  handed. Break that and the two stop composing and `docx -> docx`
  silently drops every picture again. The bag is restricted to targets a
  relationship typed `/image` declares, so a crafted package cannot point
  a `blip` at `comments.xml` and make the reader load it.
- A relationship id resolves against the rels of the part that *uses* it,
  both ways round. Reading: a note's ids come from
  `word/_rels/footnotes.xml.rels`, falling back to the document's only when
  the note declares none — which is what pandoc writes, and why pandoc
  loses footnote images from its own files. Writing: footnote bodies draw
  ids from the same counter the document body does, so that rels part must
  declare them too.
- `part_path` handles all three legal OPC target spellings (relative,
  package-absolute, `..`) and percent-decodes. Each one Word actually
  writes, and getting one wrong loses a picture silently.
- `every_relationship_a_part_uses_is_declared_in_its_own_rels` also checks
  that every part is well-formed with its prefixes bound and every style it
  names is in `styles.xml`. Both caught real breakage: `footnotes.xml`
  declared only `xmlns:w`, so a picture in a note made the package
  unopenable, and `CaptionedFigure` was used before it was declared.
- `media::inspect` reports a pixel count **and the resolution it is
  counted at**, never a bare size. Half the formats have no pixels: an EMF
  is hundredths of a millimetre at 2540 to the inch, a WMF is whatever its
  header says. A 300-dpi PNG placed at 72 is four times too wide.
- A JPEG states its resolution in **two** places and they do not tie:
  JFIF in APP0 and Exif in APP1. Exif wins where it names a resolution
  *and* the unit it is in; otherwise JFIF answers. Both halves measured,
  and both are four-fold errors when wrong: reading JFIF alone mis-sizes
  every scanner JPEG, and preferring Exif unconditionally mis-sizes every
  file whose Exif carries no resolution tags — which is most of them. A
  resolution with no `ResolutionUnit` beside it does not count, for a TIFF
  either. `directory()` reads the Exif IFD and the TIFF one, which are the
  same structure with the same three tags.
- **Probe with real files.** A hand-built minimal JPEG or TIFF is one
  pandoc cannot parse at all: it falls back to 300x200 pt and every
  comparison against it is meaningless. Build them with ImageMagick or
  Pillow, and splice a crafted header into a real file rather than
  synthesising a whole one. Two wrong rules were measured off synthetic
  files before this was understood.
- Offsets in a TIFF or Exif directory come straight out of `u32` fields,
  so the arithmetic on them must be checked: on wasm32 `usize` is 32 bits
  and a crafted offset aborts instead of being refused. The byte readers
  are all `checked_add`, and a chunk length that overflows ends a scan
  rather than failing it — failing would refuse on 32-bit a file 64-bit
  embeds.
- An SVG is recognized by its **root element**, never by searching for
  `<svg`: that sized a picture from a commented-out element and embedded
  an HTML page containing an icon as an `image/svg+xml` part.
- An extent outside `ST_PositiveCoordinate` (1..=27273042316900) makes a
  package Word refuses and LibreOffice opens with the picture silently
  gone. `picture()` gives up before registering the part, so a refused
  image leaves no orphan media part behind.
- An **SVG does not go in the blip**. Word and pandoc both put its
  relationship id in an `asvg:svgBlip` inside `a:blip`'s `extLst`, with the
  raster fallback on the blip itself, and pandoc's reader looks nowhere
  else. Written the ordinary way the round trip here is still perfect and
  pandoc reads no picture at all, so only
  `an_svg_is_referenced_the_way_pandoc_reads_it` holds it.
- Pandoc's EMF size comes from the header's `szlDevice`/`szlMillimeters` —
  the monitor of the machine that recorded the file — and quantises the
  drawing onto that pixel grid, differently per axis. That is not a rule to
  follow; the frame is the size. Same for a WMF, which pandoc cannot size
  at all and gives its 300x200 point default.
- A `Figure` writes its content with the `CaptionedFigure` style. Pandoc's
  reader keys the pair on it: written any other way pandoc drops the
  picture, even though our own round trip looks fine.
- The corpus is regenerated by `bash corpus/docx/generate.sh`. Zip bytes are
  not reproducible, so check conformance, never `git diff`.
