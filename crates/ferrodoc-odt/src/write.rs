//! ODT writer: renders the ferrodoc AST to an `OpenDocument` text package.
//!
//! The package is minimal but complete enough that `LibreOffice`, Word and
//! pandoc all open it: the uncompressed `mimetype` entry an ODF consumer
//! looks for first, a manifest, `styles.xml` holding the named styles
//! pandoc's reader keys on, and a `content.xml` whose automatic styles
//! carry the run formatting.
//!
//! Output is deterministic — the zip entries carry a fixed timestamp and
//! are written in a fixed order — so the same AST always produces the same
//! bytes, which pandoc's writer does not guarantee.
//!
//! What the *format* cannot carry is not this writer's doing, and is the
//! same for pandoc's: `diff-odt-write` compares this writer's output read
//! back by pandoc against pandoc's own output read back the same way, so a
//! loss both writers share does not count against either. There are
//! several, and they are large — a code block, a multi-paragraph quote and
//! a horizontal rule all come back as plain paragraphs, because pandoc's
//! ODT *reader* has no construct for any of them.
//!
//! Two rules are not obvious and both were measured:
//!
//! - **run formatting must be flattened into one span.** Nesting a bold
//!   span inside an italic one reads back as `Emph [Strong [Emph […]]]`,
//!   because the reader applies the accumulated property set at *every*
//!   level. One span carrying both properties reads back as the two.
//! - **a leading run of spaces has to be `text:s`.** ODF collapses
//!   whitespace in text, so an indented code line written literally comes
//!   back with one space.

use crate::Error;
use ferrodoc_ast::{
    Block, Cell, Inline, ListAttributes, ListNumberDelim, ListNumberStyle, Meta, MetaValue, Pandoc,
    QuoteType, Row, Table,
};
use ferrodoc_docx::media;
use std::fmt::Write as _;
use std::io::Write as _;
use zip::write::SimpleFileOptions;

/// Render a document as an `.odt` package, without embedding images.
///
/// Images survive as their alt text. Use [`write_odt_with_media`] to supply
/// their bytes and have them embedded as real pictures.
///
/// # Errors
///
/// Only [`Error::Zip`], if the in-memory archive cannot be assembled.
pub fn write_odt(doc: &Pandoc) -> Result<Vec<u8>, Error> {
    write_odt_with_media(doc, &|_| None)
}

/// Render a document as an `.odt` package, embedding every image whose
/// bytes `media` can supply for its URL.
///
/// Resolution is the caller's job because it is the only part of this that
/// is not pure: a URL may name a file, a cache or nothing at all, and this
/// crate must keep compiling for `wasm32`. An image whose bytes are absent,
/// or are not a format that can be embedded, falls back to its alt text.
///
/// # Errors
///
/// Only [`Error::Zip`], if the in-memory archive cannot be assembled.
pub fn write_odt_with_media(
    doc: &Pandoc,
    media: &dyn Fn(&str) -> Option<Vec<u8>>,
) -> Result<Vec<u8>, Error> {
    write_odt_with_reference(doc, media, None)
}

/// The same, taking the document's **styles** from a reference `.odt`.
///
/// Pandoc's `--reference-doc`, for the other office format. `styles.xml`
/// is what a style is in `OpenDocument`, and it is the one part taken —
/// `content.xml` is this document and `META-INF/manifest.xml` has to
/// list the parts this package actually holds.
///
/// # Errors
///
/// A reference that is not a zip, or has no `styles.xml` — named rather
/// than silently falling back, because a team whose branding vanished
/// would find out downstream.
pub fn write_odt_with_reference(
    doc: &Pandoc,
    media: &dyn Fn(&str) -> Option<Vec<u8>>,
    reference: Option<&[u8]>,
) -> Result<Vec<u8>, Error> {
    let styles = match reference {
        None => STYLES.to_owned(),
        Some(bytes) => reference_styles(bytes)?,
    };
    write_package(doc, media, &styles)
}

/// `styles.xml` out of a reference package.
fn reference_styles(bytes: &[u8]) -> Result<String, Error> {
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes))
        .map_err(|e| Error::Zip(format!("the reference document is not an .odt: {e}")))?;
    let mut part = archive
        .by_name("styles.xml")
        .map_err(|_| Error::Zip("the reference document has no styles.xml".to_owned()))?;
    let mut text = String::new();
    std::io::Read::read_to_string(&mut part, &mut text)
        .map_err(|e| Error::Zip(e.to_string()))?;
    Ok(text)
}

fn write_package(
    doc: &Pandoc,
    media: &dyn Fn(&str) -> Option<Vec<u8>>,
    styles: &str,
) -> Result<Vec<u8>, Error> {
    let mut w = Writer { media, ..Writer::default() };
    // Metadata goes out as the leading styled paragraphs pandoc's own
    // writer emits for it. `meta.xml` is not written, because pandoc's ODT
    // reader does not read it — a title put there is lost by the one
    // consumer this is gated against.
    let mut body = w.metadata(&doc.meta);
    body.push_str(&w.blocks(&doc.blocks));

    let mut content = String::new();
    content.push_str(XML_DECL);
    content.push_str(CONTENT_OPEN);
    content.push_str(&w.automatic_styles());
    content.push_str("<office:body><office:text>");
    content.push_str(&body);
    content.push_str("</office:text></office:body></office:document-content>");

    let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let stamp = zip::DateTime::from_date_and_time(1980, 1, 1, 0, 0, 0).unwrap_or_default();
    let options = SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated)
        .last_modified_time(stamp);
    // The `mimetype` entry must come first and be stored uncompressed: that
    // is how a consumer identifies the package without unzipping it.
    zip.start_file(
        "mimetype",
        SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored)
            .last_modified_time(stamp),
    )
    .map_err(|e| Error::Zip(e.to_string()))?;
    zip.write_all(MIMETYPE.as_bytes())
        .map_err(|e| Error::Zip(e.to_string()))?;

    let mut part = |name: &str, data: &str| -> Result<(), Error> {
        zip.start_file(name, options)
            .map_err(|e| Error::Zip(e.to_string()))?;
        zip.write_all(data.as_bytes())
            .map_err(|e| Error::Zip(e.to_string()))?;
        Ok(())
    };
    part("META-INF/manifest.xml", &w.manifest())?;
    part("content.xml", &content)?;
    part("styles.xml", styles)?;
    // `part` borrows the zip; the pictures are written directly.
    #[expect(dropping_references, reason = "releases the borrow on `zip`")]
    drop(&part);
    for picture in &w.pictures {
        zip.start_file(&picture.path, options)
            .map_err(|e| Error::Zip(e.to_string()))?;
        zip.write_all(&picture.bytes)
            .map_err(|e| Error::Zip(e.to_string()))?;
    }
    let cursor = zip.finish().map_err(|e| Error::Zip(e.to_string()))?;
    Ok(cursor.into_inner())
}

const MIMETYPE: &str = "application/vnd.oasis.opendocument.text";
const XML_DECL: &str = r#"<?xml version="1.0" encoding="UTF-8"?>"#;

/// Accumulates what depends on document content: the automatic styles the
/// runs and lists need, and the pictures to store.
struct Writer<'a> {
    /// Where image bytes come from; see [`write_odt_with_media`].
    media: &'a dyn Fn(&str) -> Option<Vec<u8>>,
    /// Distinct run formatting in use; the index is the number in `T1`.
    text_styles: Vec<Props>,
    /// One entry per list written, in order: its marker (`None` for a
    /// bullet list) and the level it is nested at. The index is the number
    /// in `L1`.
    list_styles: Vec<(Option<ListAttributes>, usize)>,
    /// Derived paragraph styles in use: (parent style, quote depth). The
    /// index is the number in `P1`.
    para_styles: Vec<(&'static str, usize)>,
    /// Pictures to store, in reference order.
    pictures: Vec<Picture>,
    /// How deeply block quotes nest, which sets the paragraph indent.
    quote_depth: usize,
    /// How deeply lists nest, which sets the level each declares.
    list_depth: usize,
    /// Footnotes seen so far, for numbering the citations.
    notes: usize,
    /// Tables seen so far, for naming them.
    tables: usize,
}

impl Default for Writer<'_> {
    fn default() -> Self {
        Self {
            media: &|_| None,
            text_styles: Vec::new(),
            list_styles: Vec::new(),
            para_styles: Vec::new(),
            pictures: Vec::new(),
            quote_depth: 0,
            list_depth: 0,
            notes: 0,
            tables: 0,
        }
    }
}

struct Picture {
    path: String,
    content_type: &'static str,
    bytes: Vec<u8>,
}

/// The run formatting in effect, flattened out of the AST's nesting.
///
/// One flag per property rather than a set, because the writer asks each
/// of them separately when it spells the style out.
#[expect(clippy::struct_excessive_bools, reason = "ODF states each property separately")]
#[derive(Default, Clone, Copy, PartialEq, Eq)]
struct Props {
    bold: bool,
    italic: bool,
    underline: bool,
    strike: bool,
    small_caps: bool,
    position: Option<bool>,
}

impl Props {
    fn is_plain(self) -> bool {
        self == Props::default()
    }

    /// The `style:text-properties` attributes this calls for.
    fn attributes(self) -> String {
        let mut out = String::new();
        if self.bold {
            out.push_str(" fo:font-weight=\"bold\"");
        }
        if self.italic {
            out.push_str(" fo:font-style=\"italic\"");
        }
        if self.underline {
            out.push_str(" style:text-underline-style=\"solid\"");
        }
        if self.strike {
            out.push_str(" style:text-line-through-style=\"solid\"");
        }
        if self.small_caps {
            out.push_str(" fo:font-variant=\"small-caps\"");
        }
        match self.position {
            Some(true) => out.push_str(" style:text-position=\"super 58%\""),
            Some(false) => out.push_str(" style:text-position=\"sub 58%\""),
            None => {}
        }
        out
    }
}

impl Writer<'_> {
    /// The document's metadata, as the leading styled paragraphs pandoc's
    /// own writer emits for it.
    fn metadata(&mut self, meta: &Meta) -> String {
        let mut out = String::new();
        for (field, style) in [("title", "Title"), ("author", "Author"), ("date", "Date")] {
            let Some(value) = meta.get(field) else { continue };
            // A repeated field (several authors) is one paragraph each.
            for inlines in meta_inlines(value) {
                let rendered = self.inlines(&inlines, Props::default());
                out.push_str(&paragraph(style, &rendered));
            }
        }
        out
    }

    fn blocks(&mut self, blocks: &[Block]) -> String {
        let mut out = String::new();
        for block in blocks {
            out.push_str(&self.block(block));
        }
        out
    }

    fn block(&mut self, block: &Block) -> String {
        match block {
            Block::Plain(inlines) | Block::Para(inlines) => {
                let inner = self.inlines(inlines, Props::default());
                let style = self.paragraph_style(BODY);
                paragraph(&style, &inner)
            }
            Block::LineBlock(lines) => {
                // One paragraph, the lines joined by hard breaks: an ODF
                // paragraph is the only container that keeps them together.
                let rendered: Vec<String> = lines
                    .iter()
                    .map(|line| self.inlines(line, Props::default()))
                    .collect();
                let style = self.paragraph_style(BODY);
                paragraph(&style, &rendered.join("<text:line-break/>"))
            }
            Block::CodeBlock(_, code) => {
                // A paragraph per line, in the monospaced style. Pandoc's
                // ODT *reader* has no code blocks at all, so both writers'
                // output comes back as paragraphs; matching its encoding is
                // what makes the round trip agree.
                let mut out = String::new();
                let style = self.paragraph_style(PREFORMATTED);
                for line in code.lines() {
                    out.push_str(&paragraph(&style, &text(line)));
                }
                out
            }
            Block::BlockQuote(inner) => {
                self.quote_depth += 1;
                let out = self.blocks(inner);
                self.quote_depth -= 1;
                out
            }
            Block::OrderedList(attrs, items) => self.list(Some(attrs.clone()), items),
            Block::BulletList(items) => self.list(None, items),
            Block::DefinitionList(entries) => {
                let mut out = String::new();
                for (term, definitions) in entries {
                    let rendered = self.inlines(term, Props::default());
                    let style = self.paragraph_style(DEFINITION_TERM);
                    out.push_str(&paragraph(&style, &rendered));
                    for definition in definitions {
                        out.push_str(&self.blocks(definition));
                    }
                }
                out
            }
            Block::Header(level, attr, inlines) => {
                let level = (*level).clamp(1, MAX_HEADING_LEVEL);
                let inner = self.inlines(inlines, Props::default());
                // The identifier travels as a bookmark, the only place ODF
                // has for one, and where pandoc's writer puts it — so both
                // come back carrying the same anchor span.
                let marked = if attr.identifier.is_empty() {
                    inner
                } else {
                    let name = attribute(&attr.identifier);
                    format!(
                        "<text:bookmark-start text:name=\"{name}\"/>{inner}<text:bookmark-end text:name=\"{name}\"/>"
                    )
                };
                format!(
                    "<text:h text:style-name=\"Heading_20_{level}\" text:outline-level=\"{level}\">{marked}</text:h>"
                )
            }
            Block::HorizontalRule => format!("<text:p text:style-name=\"{HORIZONTAL_LINE}\"/>"),
            Block::Table(table) => self.table(table),
            Block::Figure(_, caption, inner) => {
                let mut out = self.blocks(inner);
                out.push_str(&self.blocks(&caption.blocks));
                out
            }
            Block::Div(_, inner) => self.blocks(inner),
            // Raw content is another format's syntax; writing it into an
            // ODF paragraph would put markup on the page as text.
            Block::RawBlock(..) => String::new(),
        }
    }

    /// The style a paragraph takes here: `base`, or an automatic style
    /// derived from it with the indent of the block quote it sits in.
    ///
    /// The indent has to reach *every* paragraph style, not just the body
    /// one: a code block inside a quote is a quote, and pandoc's reader
    /// decides that from the margin alone — so a preformatted paragraph
    /// written at the plain indent comes back outside the quote.
    fn paragraph_style(&mut self, base: &'static str) -> String {
        if self.quote_depth == 0 {
            return base.to_owned();
        }
        let derived = (base, self.quote_depth.min(MAX_QUOTE_DEPTH));
        let index = if let Some(index) = self.para_styles.iter().position(|s| *s == derived) {
            index
        } else {
            self.para_styles.push(derived);
            self.para_styles.len() - 1
        };
        format!("P{}", index + 1)
    }

    /// A `text:list`, with a list style of its very own.
    ///
    /// Every list declares its own style with a single level, nested or
    /// not — which is what pandoc's writer does, and the only encoding that
    /// survives two nested lists of *different* shapes under one parent. A
    /// nested list inheriting its parent's style takes the parent's level
    /// for its depth, so two sibling sublists would overwrite each other's
    /// marker and the second would take the first's.
    fn list(&mut self, attrs: Option<ListAttributes>, items: &[Vec<Block>]) -> String {
        self.list_depth += 1;
        let depth = self.list_depth.min(MAX_LIST_DEPTH);
        // The style has to define the level this list is *nested at*, not
        // just level one: a word processor takes the marker for a list from
        // the level matching its depth, and a style that stops at level one
        // numbers a nested bullet list. Every level up to it takes the same
        // shape, so a gap cannot be reached either.
        self.list_styles.push((attrs, depth));
        let style = self.list_styles.len();
        let mut out = format!("<text:list text:style-name=\"L{style}\">");
        for item in items {
            let inner = self.blocks(item);
            let _ = write!(out, "<text:list-item>{inner}</text:list-item>");
        }
        out.push_str("</text:list>");
        self.list_depth -= 1;
        out
    }

    fn table(&mut self, table: &Table) -> String {
        self.tables += 1;
        let mut out = format!("<table:table table:name=\"Table{}\">", self.tables);
        for _ in 0..table.colspecs.len() {
            out.push_str("<table:table-column/>");
        }
        if !table.head.rows.is_empty() {
            out.push_str("<table:table-header-rows>");
            for row in &table.head.rows {
                out.push_str(&self.row(row, TABLE_HEADING));
            }
            out.push_str("</table:table-header-rows>");
        }
        for body in &table.bodies {
            for row in body.head.iter().chain(&body.body) {
                out.push_str(&self.row(row, TABLE_CONTENTS));
            }
        }
        for row in &table.foot.rows {
            out.push_str(&self.row(row, TABLE_CONTENTS));
        }
        out.push_str("</table:table>");
        // A caption is a following paragraph. Pandoc's reader reads one as
        // a paragraph either way, so both writers lose the association.
        out.push_str(&self.blocks(&table.caption.blocks));
        out
    }

    fn row(&mut self, row: &Row, style: &'static str) -> String {
        let mut out = String::from("<table:table-row>");
        for cell in &row.cells {
            out.push_str(&self.cell(cell, style));
        }
        out.push_str("</table:table-row>");
        out
    }

    fn cell(&mut self, cell: &Cell, style: &'static str) -> String {
        let inner = if cell.blocks.is_empty() {
            let style = self.paragraph_style(style);
            paragraph(&style, "")
        } else {
            self.blocks(&cell.blocks)
        };
        let span = if cell.col_span > 1 {
            format!(" table:number-columns-spanned=\"{}\"", cell.col_span)
        } else {
            String::new()
        };
        // A spanned cell must be followed by the positions it covers, or
        // the grid is short and every later cell shifts left.
        let covered = "<table:covered-table-cell/>"
            .repeat(usize::try_from(cell.col_span.max(1) - 1).unwrap_or(0));
        format!(
            "<table:table-cell{span} office:value-type=\"string\">{inner}</table:table-cell>{covered}"
        )
    }

    /// Render a run of inlines with `props` in effect.
    ///
    /// Formatting is *flattened*: `Emph` and `Strong` do not become nested
    /// spans, they set flags that the leaf text carries. Nesting them
    /// instead reads back with each level applied again.
    fn inlines(&mut self, inlines: &[Inline], props: Props) -> String {
        let mut out = String::new();
        // Consecutive leaf inlines share one span, which is what pandoc's
        // writer emits and keeps the output from being one span per word.
        let mut run = String::new();
        for inline in inlines {
            match inline {
                Inline::Emph(inner) => {
                    flush(&mut out, &mut run, props);
                    out.push_str(&self.inlines(inner, Props { italic: true, ..props }));
                }
                Inline::Strong(inner) => {
                    flush(&mut out, &mut run, props);
                    out.push_str(&self.inlines(inner, Props { bold: true, ..props }));
                }
                Inline::Underline(inner) => {
                    flush(&mut out, &mut run, props);
                    out.push_str(&self.inlines(inner, Props { underline: true, ..props }));
                }
                Inline::Strikeout(inner) => {
                    flush(&mut out, &mut run, props);
                    out.push_str(&self.inlines(inner, Props { strike: true, ..props }));
                }
                Inline::SmallCaps(inner) => {
                    flush(&mut out, &mut run, props);
                    out.push_str(&self.inlines(inner, Props { small_caps: true, ..props }));
                }
                Inline::Superscript(inner) => {
                    flush(&mut out, &mut run, props);
                    out.push_str(&self.inlines(inner, Props { position: Some(true), ..props }));
                }
                Inline::Subscript(inner) => {
                    flush(&mut out, &mut run, props);
                    out.push_str(&self.inlines(inner, Props { position: Some(false), ..props }));
                }
                Inline::Span(_, inner) | Inline::Cite(_, inner) => {
                    flush(&mut out, &mut run, props);
                    out.push_str(&self.inlines(inner, props));
                }
                // ODF has no quotation element; the marks are the content,
                // which is what pandoc's writer emits too.
                Inline::Quoted(kind, inner) => {
                    let (open, close) = match kind {
                        QuoteType::SingleQuote => ('\u{2018}', '\u{2019}'),
                        QuoteType::DoubleQuote => ('\u{201c}', '\u{201d}'),
                    };
                    run.push(open);
                    let quoted = self.inlines(inner, props);
                    flush(&mut out, &mut run, props);
                    out.push_str(&quoted);
                    run.push(close);
                }
                Inline::Link(_, inner, target) => {
                    flush(&mut out, &mut run, props);
                    let inner = self.inlines(inner, props);
                    let _ = write!(
                        out,
                        "<text:a xlink:type=\"simple\" xlink:href=\"{}\">{inner}</text:a>",
                        attribute(&package_relative(&target.url))
                    );
                }
                Inline::Note(blocks) => {
                    flush(&mut out, &mut run, props);
                    self.notes += 1;
                    let id = self.notes;
                    let body = self.blocks(blocks);
                    let _ = write!(
                        out,
                        "<text:note text:id=\"ftn{id}\" text:note-class=\"footnote\"><text:note-citation>{id}</text:note-citation><text:note-body>{body}</text:note-body></text:note>"
                    );
                }
                Inline::Image(attr, alt, target) => {
                    flush(&mut out, &mut run, props);
                    match self.picture(&target.url, attr) {
                        Some(frame) => out.push_str(&frame),
                        // No bytes, or a format that cannot be embedded:
                        // the alt text is what the picture said, and
                        // pandoc marks it as substituted by emphasizing it.
                        None => {
                            out.push_str(&self.inlines(alt, Props { italic: true, ..props }));
                        }
                    }
                }
                Inline::Str(s) => run.push_str(&text(s)),
                Inline::Space | Inline::SoftBreak => run.push(' '),
                Inline::LineBreak => run.push_str("<text:line-break/>"),
                Inline::Code(_, code) => {
                    flush(&mut out, &mut run, props);
                    let _ = write!(
                        out,
                        "<text:span text:style-name=\"{CODE_STYLE}\">{}</text:span>",
                        text(code)
                    );
                }
                Inline::Math(_, math) => run.push_str(&text(math)),
                Inline::RawInline(..) => {}
            }
        }
        flush(&mut out, &mut run, props);
        // Registering the style has to happen after the whole run is
        // rendered, because `flush` cannot borrow `self` mutably while the
        // recursion above holds it.
        self.register(&mut out, props);
        out
    }

    /// Replace the style placeholder `flush` left with a real style name.
    fn register(&mut self, out: &mut String, props: Props) {
        if props.is_plain() || !out.contains(STYLE_PLACEHOLDER) {
            return;
        }
        let index = if let Some(index) = self.text_styles.iter().position(|p| *p == props) {
            index
        } else {
            self.text_styles.push(props);
            self.text_styles.len() - 1
        };
        *out = out.replace(STYLE_PLACEHOLDER, &format!("T{}", index + 1));
    }

    /// A `draw:frame` holding the picture, if its bytes can be embedded.
    fn picture(&mut self, url: &str, attr: &ferrodoc_ast::Attr) -> Option<String> {
        let bytes = (self.media)(url)?;
        let image = media::inspect(&bytes)?;
        let path = format!("Pictures/{}.{}", self.pictures.len(), image.extension);
        // A size stated in the AST wins over the file's own, because that
        // is what the document asked for; otherwise the intrinsic size, at
        // the resolution the file states.
        let stated = |key: &str| {
            attr.attributes
                .iter()
                .find(|(k, _)| k == key)
                .map(|(_, v)| v.clone())
        };
        let width = stated("width").unwrap_or_else(|| points(image.size.width, image.size.dpi_x));
        let height =
            stated("height").unwrap_or_else(|| points(image.size.height, image.size.dpi_y));
        let frame = format!(
            "<draw:frame draw:name=\"img{}\" svg:width=\"{}\" svg:height=\"{}\"><draw:image xlink:href=\"{}\" xlink:type=\"simple\" xlink:show=\"embed\" xlink:actuate=\"onLoad\"/></draw:frame>",
            self.pictures.len() + 1,
            attribute(&width),
            attribute(&height),
            attribute(&path)
        );
        self.pictures.push(Picture {
            path,
            content_type: image.content_type,
            bytes,
        });
        Some(frame)
    }

    fn automatic_styles(&self) -> String {
        let mut out = String::from("<office:automatic-styles>");
        for (index, props) in self.text_styles.iter().enumerate() {
            let _ = write!(
                out,
                "<style:style style:name=\"T{}\" style:family=\"text\"><style:text-properties{}/></style:style>",
                index + 1,
                props.attributes()
            );
        }
        for (index, (parent, depth)) in self.para_styles.iter().enumerate() {
            let _ = write!(
                out,
                "<style:style style:name=\"P{}\" style:family=\"paragraph\" style:parent-style-name=\"{parent}\"><style:paragraph-properties fo:margin-left=\"{:.4}in\" fo:margin-right=\"0.3937in\"/></style:style>",
                index + 1,
                0.3937 * f64::from(u8::try_from(*depth).unwrap_or(u8::MAX))
            );
        }
        for (index, (attrs, depth)) in self.list_styles.iter().enumerate() {
            let _ = write!(out, "<text:list-style style:name=\"L{}\">", index + 1);
            for level in 1..=*depth {
                out.push_str(&list_level(level, attrs.as_ref()));
            }
            out.push_str("</text:list-style>");
        }
        out.push_str("</office:automatic-styles>");
        out
    }

    fn manifest(&self) -> String {
        let mut out = String::from(XML_DECL);
        out.push_str(
            r#"<manifest:manifest xmlns:manifest="urn:oasis:names:tc:opendocument:xmlns:manifest:1.0" manifest:version="1.3">"#,
        );
        let _ = write!(
            out,
            r#"<manifest:file-entry manifest:full-path="/" manifest:version="1.3" manifest:media-type="{MIMETYPE}"/>"#
        );
        for part in ["content.xml", "styles.xml"] {
            let _ = write!(
                out,
                r#"<manifest:file-entry manifest:full-path="{part}" manifest:media-type="text/xml"/>"#
            );
        }
        for picture in &self.pictures {
            let _ = write!(
                out,
                r#"<manifest:file-entry manifest:full-path="{}" manifest:media-type="{}"/>"#,
                attribute(&picture.path),
                picture.content_type
            );
        }
        out.push_str("</manifest:manifest>");
        out
    }
}

/// Where a run's style name goes before [`Writer::register`] knows it.
const STYLE_PLACEHOLDER: &str = "\u{0}STYLE\u{0}";

/// Emit the gathered leaf text as one span, or as bare text when the run
/// carries no formatting.
fn flush(out: &mut String, run: &mut String, props: Props) {
    if run.is_empty() {
        return;
    }
    if props.is_plain() {
        out.push_str(run);
    } else {
        let _ = write!(out, "<text:span text:style-name=\"{STYLE_PLACEHOLDER}\">{run}</text:span>");
    }
    run.clear();
}

fn paragraph(style: &str, inner: &str) -> String {
    if inner.is_empty() {
        format!("<text:p text:style-name=\"{style}\"/>")
    } else {
        format!("<text:p text:style-name=\"{style}\">{inner}</text:p>")
    }
}

/// One level of a list style.
fn list_level(level: usize, attrs: Option<&ListAttributes>) -> String {
    let Some(attrs) = attrs else {
        return format!(
            "<text:list-level-style-bullet text:level=\"{level}\" text:bullet-char=\"\u{2022}\"><style:list-level-properties text:space-before=\"{}in\" text:min-label-width=\"0.25in\"/></text:list-level-style-bullet>",
            indent(level)
        );
    };
    let format = match attrs.style {
        ListNumberStyle::LowerAlpha => "a",
        ListNumberStyle::UpperAlpha => "A",
        ListNumberStyle::LowerRoman => "i",
        ListNumberStyle::UpperRoman => "I",
        // Decimal, and everything the format has no spelling for.
        _ => "1",
    };
    let (prefix, suffix) = match attrs.delim {
        ListNumberDelim::OneParen => ("", ")"),
        ListNumberDelim::TwoParens => ("(", ")"),
        ListNumberDelim::Period | ListNumberDelim::DefaultDelim => ("", "."),
    };
    format!(
        "<text:list-level-style-number text:level=\"{level}\" style:num-format=\"{format}\" style:num-prefix=\"{prefix}\" style:num-suffix=\"{suffix}\" text:start-value=\"{}\"><style:list-level-properties text:space-before=\"{}in\" text:min-label-width=\"0.25in\"/></text:list-level-style-number>",
        attrs.start,
        indent(level)
    )
}

/// The indent of a list level, in inches. Cosmetic — no reader here reads
/// it — but a list with every level at the same indent looks wrong in a
/// word processor.
fn indent(level: usize) -> String {
    format!("{:.2}", 0.25 * f64::from(u8::try_from(level).unwrap_or(u8::MAX)))
}

/// A pixel count at a resolution, as a length in points.
///
/// Points because that is what pandoc's writer emits, and the attribute
/// travels back into the AST verbatim: a different unit would read back as
/// a different string even for the same size.
fn points(pixels: u32, dpi: u32) -> String {
    let dpi = if dpi == 0 { 72 } else { dpi };
    format!("{:.1}pt", f64::from(pixels) * 72.0 / f64::from(dpi))
}

/// The URL as an ODF package link.
///
/// A document-relative target is written one directory up, because a link
/// in `content.xml` is resolved against the package rather than against the
/// document. A URL with a scheme, or a bare fragment, is already absolute
/// and is left alone. Pandoc's writer does exactly this and its reader
/// takes the step back off again.
fn package_relative(url: &str) -> String {
    if url.starts_with('#') || has_scheme(url) {
        return url.to_owned();
    }
    format!("../{url}")
}

/// Whether a URL begins with a scheme (`https:`, `mailto:`, …).
fn has_scheme(url: &str) -> bool {
    let Some(colon) = url.find(':') else { return false };
    let scheme = &url[..colon];
    scheme.starts_with(|c: char| c.is_ascii_alphabetic())
        && scheme
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
}

/// Text content, escaped, with whitespace spelled the way ODF requires.
///
/// An ODF consumer collapses a run of spaces in text to one, so every space
/// after the first in a run — and a run at the very start, where even one
/// would be dropped — is written as `text:s`. Without this an indented code
/// line loses its indentation, which is most of what a code sample is.
fn text(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let mut spaces = 0usize;
    let mut at_start = true;
    let flush_spaces = |out: &mut String, spaces: &mut usize, at_start: bool| {
        if *spaces == 0 {
            return;
        }
        // A leading run has no literal space to lead with: the first one
        // would be collapsed away along with the rest.
        let literal = usize::from(!at_start);
        if literal == 1 {
            out.push(' ');
        }
        if *spaces > literal {
            let _ = write!(out, "<text:s text:c=\"{}\"/>", *spaces - literal);
        }
        *spaces = 0;
    };
    for ch in value.chars() {
        match ch {
            ' ' => spaces += 1,
            '\t' => {
                flush_spaces(&mut out, &mut spaces, at_start);
                out.push_str("<text:tab/>");
                at_start = false;
            }
            ch => {
                flush_spaces(&mut out, &mut spaces, at_start);
                at_start = false;
                match ch {
                    '&' => out.push_str("&amp;"),
                    '<' => out.push_str("&lt;"),
                    '>' => out.push_str("&gt;"),
                    ch => out.push(ch),
                }
            }
        }
    }
    // A trailing run is collapsed the same way a leading one is.
    flush_spaces(&mut out, &mut spaces, at_start);
    out
}

/// An attribute value, escaped.
fn attribute(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for ch in value.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            ch => out.push(ch),
        }
    }
    out
}

/// A metadata value as one inline sequence per paragraph it should become.
fn meta_inlines(value: &MetaValue) -> Vec<Vec<Inline>> {
    match value {
        MetaValue::MetaInlines(inlines) => vec![inlines.clone()],
        MetaValue::MetaString(text) => vec![vec![Inline::Str(text.clone())]],
        MetaValue::MetaBool(flag) => vec![vec![Inline::Str(flag.to_string())]],
        MetaValue::MetaList(values) => values.iter().flat_map(meta_inlines).collect(),
        MetaValue::MetaBlocks(blocks) => blocks
            .iter()
            .filter_map(|block| match block {
                Block::Plain(inlines) | Block::Para(inlines) => Some(inlines.clone()),
                _ => None,
            })
            .collect(),
        MetaValue::MetaMap(_) => Vec::new(),
    }
}

const BODY: &str = "Text_20_body";
const PREFORMATTED: &str = "Preformatted_20_Text";
const HORIZONTAL_LINE: &str = "Horizontal_20_Line";
const DEFINITION_TERM: &str = "Definition_20_Term";
const TABLE_CONTENTS: &str = "Table_20_Contents";
const TABLE_HEADING: &str = "Table_20_Heading";
const CODE_STYLE: &str = crate::style::CODE_STYLE;
const MAX_QUOTE_DEPTH: usize = 6;
/// The deepest list level a style declares. ODF's own limit is ten, and a
/// deeper list takes the tenth level's marker rather than growing the
/// style without bound.
const MAX_LIST_DEPTH: usize = 10;
const MAX_HEADING_LEVEL: i64 = 10;

const CONTENT_OPEN: &str = concat!(
    r#"<office:document-content"#,
    r#" xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0""#,
    r#" xmlns:style="urn:oasis:names:tc:opendocument:xmlns:style:1.0""#,
    r#" xmlns:text="urn:oasis:names:tc:opendocument:xmlns:text:1.0""#,
    r#" xmlns:table="urn:oasis:names:tc:opendocument:xmlns:table:1.0""#,
    r#" xmlns:draw="urn:oasis:names:tc:opendocument:xmlns:drawing:1.0""#,
    r#" xmlns:fo="urn:oasis:names:tc:opendocument:xmlns:xsl-fo-compatible:1.0""#,
    r#" xmlns:xlink="http://www.w3.org/1999/xlink""#,
    r#" xmlns:svg="urn:oasis:names:tc:opendocument:xmlns:svg-compatible:1.0""#,
    r#" office:version="1.3">"#,
);

/// The named styles the reader keys on.
///
/// Two numbers here are load-bearing and neither is cosmetic: the
/// `Quotations_20_N` margins must each be at or above the 5.5 mm the
/// reader treats as a block quote, and `Table_20_Contents` must stay
/// *below* it or every table cell comes back wrapped in one.
const STYLES: &str = concat!(
    r#"<?xml version="1.0" encoding="UTF-8"?>"#,
    r#"<office:document-styles"#,
    r#" xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0""#,
    r#" xmlns:style="urn:oasis:names:tc:opendocument:xmlns:style:1.0""#,
    r#" xmlns:text="urn:oasis:names:tc:opendocument:xmlns:text:1.0""#,
    r#" xmlns:fo="urn:oasis:names:tc:opendocument:xmlns:xsl-fo-compatible:1.0""#,
    r#" xmlns:svg="urn:oasis:names:tc:opendocument:xmlns:svg-compatible:1.0""#,
    r#" office:version="1.3">"#,
    r#"<office:font-face-decls>"#,
    r#"<style:font-face style:name="Courier New" style:font-family-generic="modern" style:font-pitch="fixed" svg:font-family="'Courier New'"/>"#,
    r#"</office:font-face-decls>"#,
    r#"<office:styles>"#,
    r#"<style:style style:name="Standard" style:family="paragraph"/>"#,
    r#"<style:style style:name="Text_20_body" style:display-name="Text body" style:family="paragraph" style:parent-style-name="Standard"/>"#,
    r#"<style:style style:name="Title" style:family="paragraph" style:parent-style-name="Standard"/>"#,
    r#"<style:style style:name="Author" style:family="paragraph" style:parent-style-name="Standard"/>"#,
    r#"<style:style style:name="Date" style:family="paragraph" style:parent-style-name="Standard"/>"#,
    r#"<style:style style:name="Heading_20_1" style:display-name="Heading 1" style:family="paragraph" style:parent-style-name="Standard"/>"#,
    r#"<style:style style:name="Heading_20_2" style:display-name="Heading 2" style:family="paragraph" style:parent-style-name="Standard"/>"#,
    r#"<style:style style:name="Heading_20_3" style:display-name="Heading 3" style:family="paragraph" style:parent-style-name="Standard"/>"#,
    r#"<style:style style:name="Heading_20_4" style:display-name="Heading 4" style:family="paragraph" style:parent-style-name="Standard"/>"#,
    r#"<style:style style:name="Heading_20_5" style:display-name="Heading 5" style:family="paragraph" style:parent-style-name="Standard"/>"#,
    r#"<style:style style:name="Heading_20_6" style:display-name="Heading 6" style:family="paragraph" style:parent-style-name="Standard"/>"#,
    r#"<style:style style:name="Heading_20_7" style:display-name="Heading 7" style:family="paragraph" style:parent-style-name="Standard"/>"#,
    r#"<style:style style:name="Heading_20_8" style:display-name="Heading 8" style:family="paragraph" style:parent-style-name="Standard"/>"#,
    r#"<style:style style:name="Heading_20_9" style:display-name="Heading 9" style:family="paragraph" style:parent-style-name="Standard"/>"#,
    r#"<style:style style:name="Heading_20_10" style:display-name="Heading 10" style:family="paragraph" style:parent-style-name="Standard"/>"#,
    r#"<style:style style:name="Quotations_20_1" style:display-name="Quotations 1" style:family="paragraph" style:parent-style-name="Standard"><style:paragraph-properties fo:margin-left="0.3937in" fo:margin-right="0.3937in"/></style:style>"#,
    r#"<style:style style:name="Quotations_20_2" style:display-name="Quotations 2" style:family="paragraph" style:parent-style-name="Standard"><style:paragraph-properties fo:margin-left="0.7874in" fo:margin-right="0.3937in"/></style:style>"#,
    r#"<style:style style:name="Quotations_20_3" style:display-name="Quotations 3" style:family="paragraph" style:parent-style-name="Standard"><style:paragraph-properties fo:margin-left="1.1811in" fo:margin-right="0.3937in"/></style:style>"#,
    r#"<style:style style:name="Quotations_20_4" style:display-name="Quotations 4" style:family="paragraph" style:parent-style-name="Standard"><style:paragraph-properties fo:margin-left="1.5748in" fo:margin-right="0.3937in"/></style:style>"#,
    r#"<style:style style:name="Quotations_20_5" style:display-name="Quotations 5" style:family="paragraph" style:parent-style-name="Standard"><style:paragraph-properties fo:margin-left="1.9685in" fo:margin-right="0.3937in"/></style:style>"#,
    r#"<style:style style:name="Quotations_20_6" style:display-name="Quotations 6" style:family="paragraph" style:parent-style-name="Standard"><style:paragraph-properties fo:margin-left="2.3622in" fo:margin-right="0.3937in"/></style:style>"#,
    r#"<style:style style:name="Preformatted_20_Text" style:display-name="Preformatted Text" style:family="paragraph" style:parent-style-name="Standard"><style:text-properties style:font-name="Courier New" fo:font-size="10pt"/></style:style>"#,
    r#"<style:style style:name="Horizontal_20_Line" style:display-name="Horizontal Line" style:family="paragraph" style:parent-style-name="Standard"><style:paragraph-properties fo:border-bottom="0.06pt solid #000000"/></style:style>"#,
    r#"<style:style style:name="Definition_20_Term" style:display-name="Definition Term" style:family="paragraph" style:parent-style-name="Standard"/>"#,
    r#"<style:style style:name="Table_20_Contents" style:display-name="Table Contents" style:family="paragraph" style:parent-style-name="Standard"/>"#,
    r#"<style:style style:name="Table_20_Heading" style:display-name="Table Heading" style:family="paragraph" style:parent-style-name="Table_20_Contents"><style:text-properties fo:font-weight="bold"/></style:style>"#,
    r#"<style:style style:name="Footnote" style:family="paragraph" style:parent-style-name="Standard"/>"#,
    r#"<style:style style:name="Source_Text" style:display-name="Source Text" style:family="text"><style:text-properties style:font-name="Courier New" fo:font-size="10pt"/></style:style>"#,
    r#"</office:styles>"#,
    r#"</office:document-styles>"#,
);
