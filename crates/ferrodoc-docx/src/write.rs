//! DOCX writer: renders the ferrodoc AST to an OOXML package.
//!
//! The package is minimal but complete enough that Word, `LibreOffice` and
//! pandoc all read it: content types, package and document relationships,
//! a style sheet whose style *names* are the ones pandoc's reader looks
//! for, numbering definitions for lists, and a footnotes part.
//!
//! Output is deterministic — the zip entries carry a fixed timestamp and
//! are written in a fixed order — so the same AST always produces the same
//! bytes, which pandoc's writer does not guarantee.

use crate::{Error, media};
use ferrodoc_ast::{
    Alignment, Attr, Block, Caption, Cell, ColWidth, Inline, ListAttributes, ListNumberDelim,
    ListNumberStyle, MathType, Meta, MetaValue, Pandoc, QuoteType, Row, Table, Target,
};
use std::fmt::Write as _;
use std::io::Write as _;
use zip::write::SimpleFileOptions;

/// The text width in twips the reader assumes, used to turn the AST's
/// fractional column widths back into grid columns.
const TEXT_WIDTH: f64 = 9360.0;

/// Render a document as a `.docx` package, without embedding images.
///
/// Images survive as their alt text. Use [`write_docx_with_media`] to
/// supply their bytes and have them embedded as real pictures.
pub fn write_docx(doc: &Pandoc) -> Result<Vec<u8>, Error> {
    write_docx_with_media(doc, &|_| None)
}

/// Render a document as a `.docx` package, embedding every image whose
/// bytes `media` can supply for its URL.
///
/// Resolution is the caller's job because it is the only part of this that
/// is not pure: a URL may name a file, a cache or nothing at all, and this
/// crate must keep compiling for `wasm32`. An image whose bytes are absent,
/// or are not a format that can be embedded, falls back to its alt text.
pub fn write_docx_with_media(
    doc: &Pandoc,
    media: &dyn Fn(&str) -> Option<Vec<u8>>,
) -> Result<Vec<u8>, Error> {
    let mut w = Writer { media, ..Writer::default() };
    // Metadata is written as the styled leading paragraphs the reader
    // recognizes, which is where a `.docx` actually carries it.
    let mut body = w.metadata(&doc.meta);
    body.push_str(&w.blocks(&doc.blocks));

    let mut document = String::new();
    document.push_str(XML_DECL);
    document.push_str(
        r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:pic="http://schemas.openxmlformats.org/drawingml/2006/picture" xmlns:wp="http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing"><w:body>"#,
    );
    document.push_str(&body);
    document.push_str("<w:sectPr/></w:body></w:document>");

    let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    // A fixed timestamp keeps the output byte-reproducible.
    let options = SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated)
        .last_modified_time(zip::DateTime::from_date_and_time(1980, 1, 1, 0, 0, 0).unwrap_or_default());
    let mut part = |name: &str, data: &str| -> Result<(), Error> {
        zip.start_file(name, options)
            .map_err(|e| Error::Zip(e.to_string()))?;
        zip.write_all(data.as_bytes())
            .map_err(|e| Error::Zip(e.to_string()))?;
        Ok(())
    };
    part("[Content_Types].xml", &w.content_types())?;
    part("_rels/.rels", PACKAGE_RELS)?;
    part("word/document.xml", &document)?;
    part("word/_rels/document.xml.rels", &w.document_rels())?;
    part("word/styles.xml", STYLES)?;
    part("word/numbering.xml", &w.numbering())?;
    part("word/footnotes.xml", &w.footnotes_part())?;
    // `part` borrows the zip; the media parts are written directly.
    #[expect(dropping_references, reason = "releases the borrow on `zip`")]
    drop(&part);
    for (index, relation) in w.relations.iter().enumerate() {
        if let Relation::Image { image, bytes, .. } = relation {
            zip.start_file(media_part(index, image.extension), options)
                .map_err(|e| Error::Zip(e.to_string()))?;
            zip.write_all(bytes).map_err(|e| Error::Zip(e.to_string()))?;
        }
    }
    let cursor = zip.finish().map_err(|e| Error::Zip(e.to_string()))?;
    Ok(cursor.into_inner())
}

/// The package path of the media part backing relationship `index`.
fn media_part(index: usize, extension: &str) -> String {
    format!("word/media/image{}.{extension}", index + 1)
}

/// Accumulates the parts that depend on document content: relationships,
/// numbering definitions and footnote bodies.
struct Writer<'a> {
    /// Where image bytes come from; see [`write_docx_with_media`].
    media: &'a dyn Fn(&str) -> Option<Vec<u8>>,
    /// Document relationships, in id order: links out and images in.
    relations: Vec<Relation>,
    /// One entry per list instance: (numbering format, marker text, start).
    lists: Vec<ListDefinition>,
    /// Footnote bodies, in reference order.
    footnotes: Vec<String>,
    /// Paragraph style to use instead of a block's own (inside quotes,
    /// definitions and notes).
    style_override: Option<&'static str>,
    /// Numbering for the next paragraph emitted, and the level it sits at.
    /// After one paragraph takes it, it becomes the blank continuation
    /// marker, so later paragraphs of the same item stay in that item.
    numbering: Option<(usize, usize)>,
    /// Justification forced on paragraphs (table cells carry the column's).
    justification: Option<&'static str>,
}

impl Default for Writer<'_> {
    fn default() -> Self {
        Self {
            media: &|_| None,
            relations: Vec::new(),
            lists: Vec::new(),
            footnotes: Vec::new(),
            style_override: None,
            numbering: None,
            justification: None,
        }
    }
}

/// Something `word/document.xml` points at from outside itself.
enum Relation {
    Hyperlink(String),
    Image { url: String, image: media::Image, bytes: Vec<u8> },
}

struct ListDefinition {
    format: &'static str,
    marker: String,
    start: i64,
}

impl Writer<'_> {
    /// The relationship id for an external link target.
    fn link(&mut self, url: &str) -> String {
        self.relate(Relation::Hyperlink(url.to_owned()))
    }

    /// Record a relationship and return the id that refers to it.
    fn relate(&mut self, relation: Relation) -> String {
        self.relations.push(relation);
        format!("rId{}", RELS_BASE + self.relations.len())
    }

    /// Define a numbering instance and return its `w:numId`.
    fn list(&mut self, definition: ListDefinition) -> usize {
        self.lists.push(definition);
        NUM_BASE + self.lists.len()
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
            Block::Plain(inlines) => self.paragraph("Compact", inlines),
            Block::Para(inlines) => self.paragraph("BodyText", inlines),
            Block::Header(level, _, inlines) => {
                let style = format!("Heading{}", (*level).clamp(1, 9));
                self.paragraph(&style, inlines)
            }
            // Every paragraph inside a quote takes the quote style, which
            // is how the reader recognizes and re-merges them.
            Block::BlockQuote(blocks) => {
                self.with_style("BlockText", |w| w.blocks(blocks))
            }
            Block::CodeBlock(_, text) => {
                let mut out = String::new();
                let mut runs = String::new();
                for line in code_block_lines(text) {
                    runs.clear();
                    runs.push_str(
                        "<w:r><w:rPr><w:rStyle w:val=\"VerbatimChar\"/></w:rPr><w:t xml:space=\"preserve\">",
                    );
                    escape_into(&mut runs, line);
                    runs.push_str("</w:t></w:r>");
                    out.push_str(&self.emit_paragraph(Some("SourceCode"), "", &runs));
                }
                out
            }
            Block::BulletList(items) => self.list_blocks(items, None, 0),
            Block::OrderedList(attrs, items) => self.list_blocks(items, Some(attrs), 0),
            Block::DefinitionList(items) => {
                let mut out = String::new();
                for (term, definitions) in items {
                    out.push_str(&self.paragraph("DefinitionTerm", term));
                    for definition in definitions {
                        let rendered = self.with_style("Definition", |w| w.blocks(definition));
                        out.push_str(&rendered);
                    }
                }
                out
            }
            Block::HorizontalRule => self.emit_paragraph(
                None,
                "<w:pBdr><w:bottom w:val=\"single\" w:sz=\"6\" w:space=\"1\" w:color=\"auto\"/></w:pBdr>",
                "",
            ),
            Block::Table(table) => self.without_numbering(|w| w.table(table)),
            Block::Figure(_, caption, blocks) => {
                let mut out = self.blocks(blocks);
                out.push_str(&self.caption_paragraphs(caption, "ImageCaption"));
                out
            }
            Block::Div(_, blocks) => self.blocks(blocks),
            Block::LineBlock(lines) => {
                let joined: Vec<Inline> = lines
                    .iter()
                    .enumerate()
                    .flat_map(|(i, line)| {
                        let mut out = if i == 0 { Vec::new() } else { vec![Inline::LineBreak] };
                        out.extend(line.iter().cloned());
                        out
                    })
                    .collect();
                self.paragraph("BodyText", &joined)
            }
            // Raw blocks are not OOXML and have nowhere to go.
            Block::RawBlock(..) => String::new(),
        }
    }

    /// Emit a paragraph, applying the style override, pending numbering and
    /// justification that the surrounding context established. Every `<w:p>`
    /// in the document is produced here, so the properties are built once,
    /// in order, instead of being patched into finished XML.
    fn paragraph(&mut self, default_style: &str, inlines: &[Inline]) -> String {
        let runs = self.inlines(inlines);
        self.emit_paragraph(Some(default_style), "", &runs)
    }

    /// The one place a `<w:p>` is produced. `extra` carries properties that
    /// belong after the numbering (a paragraph border, say); passing no
    /// style omits `w:pStyle` entirely.
    fn emit_paragraph(&mut self, default_style: Option<&str>, extra: &str, runs: &str) -> String {
        let mut out = String::with_capacity(runs.len() + 64);
        out.push_str("<w:p><w:pPr>");
        let properties = &mut out;
        if let Some(default_style) = default_style {
            // Only ordinary paragraphs take the surrounding context's style:
            // a code block inside a quote is still code, and a heading is
            // still a heading.
            let style = match (default_style, self.style_override) {
                ("BodyText", Some(override_style)) => override_style,
                _ => default_style,
            };
            let _ = write!(properties, "<w:pStyle w:val=\"{style}\"/>");
        }
        if let Some((num_id, level)) = self.numbering {
            let _ = write!(
                properties,
                "<w:numPr><w:ilvl w:val=\"{level}\"/><w:numId w:val=\"{num_id}\"/></w:numPr>"
            );
            // Anything further in this item continues it rather than
            // starting a new one.
            self.numbering = Some((CONTINUATION_NUM, level));
        }
        properties.push_str(extra);
        if let Some(justification) = self.justification {
            let _ = write!(properties, "<w:jc w:val=\"{justification}\"/>");
        }
        out.push_str("</w:pPr>");
        out.push_str(runs);
        out.push_str("</w:p>");
        out
    }

    /// Run `body` with a paragraph style forced, restoring the previous one.
    fn with_style(&mut self, style: &'static str, body: impl FnOnce(&mut Self) -> String) -> String {
        let previous = self.style_override.replace(style);
        let out = body(self);
        self.style_override = previous;
        out
    }

    /// Run `body` with numbering suspended (table cells and nested content
    /// must not inherit the enclosing list item's marker).
    fn without_numbering(&mut self, body: impl FnOnce(&mut Self) -> String) -> String {
        let previous = self.numbering.take();
        let out = body(self);
        self.numbering = previous;
        out
    }

    fn caption_paragraphs(&mut self, caption: &Caption, style: &str) -> String {
        caption
            .blocks
            .iter()
            .map(|block| match block {
                Block::Plain(inlines) | Block::Para(inlines) => {
                    self.paragraph(style, inlines)
                }
                other => self.block(other),
            })
            .collect()
    }

    // --- lists ---

    /// Render list items, numbering every paragraph they contain: the
    /// first block of an item carries the list's own marker, later blocks
    /// carry a blank marker, which is how the reader rejoins them.
    fn list_blocks(
        &mut self,
        items: &[Vec<Block>],
        attrs: Option<&ListAttributes>,
        level: usize,
    ) -> String {
        let definition = match attrs {
            None => ListDefinition {
                format: "bullet",
                marker: "\u{2022}".to_owned(),
                start: 1,
            },
            Some(attrs) => ListDefinition {
                format: match attrs.style {
                    ListNumberStyle::LowerAlpha => "lowerLetter",
                    ListNumberStyle::UpperAlpha => "upperLetter",
                    ListNumberStyle::LowerRoman => "lowerRoman",
                    ListNumberStyle::UpperRoman => "upperRoman",
                    _ => "decimal",
                },
                marker: match attrs.delim {
                    ListNumberDelim::OneParen => "%1)".to_owned(),
                    ListNumberDelim::TwoParens => "(%1)".to_owned(),
                    _ => "%1.".to_owned(),
                },
                start: attrs.start,
            },
        };
        let num_id = self.list(definition);
        let outer = self.numbering;
        let mut out = String::new();
        for item in items {
            // The item's first paragraph takes the marker; `paragraph`
            // switches to the blank continuation marker after that.
            self.numbering = Some((num_id, level));
            for block in item {
                // A nested list numbers itself one level deeper, and must
                // not consume this item's marker.
                let rendered = match block {
                    Block::BulletList(inner) => {
                        self.without_numbering(|w| w.list_blocks(inner, None, level + 1))
                    }
                    Block::OrderedList(inner_attrs, inner) => self.without_numbering(|w| {
                        w.list_blocks(inner, Some(inner_attrs), level + 1)
                    }),
                    _ => self.block(block),
                };
                out.push_str(&rendered);
            }
        }
        self.numbering = outer;
        out
    }

    // --- tables ---

    fn table(&mut self, table: &Table) -> String {
        let columns = table.colspecs.len().max(1);
        let widths: Vec<i64> = table
            .colspecs
            .iter()
            .map(|spec| {
                #[allow(clippy::cast_possible_truncation, clippy::cast_precision_loss)]
                match spec.width {
                    ColWidth::ColWidth(fraction) => ((fraction * TEXT_WIDTH).round() as i64).max(1),
                    ColWidth::ColWidthDefault => (TEXT_WIDTH / columns as f64).round() as i64,
                }
            })
            .collect();

        let alignments: Vec<Alignment> =
            table.colspecs.iter().map(|spec| spec.alignment).collect();
        let mut out = String::new();
        out.push_str("<w:tbl><w:tblPr><w:tblStyle w:val=\"Table\"/><w:tblW w:type=\"auto\" w:w=\"0\"/>");
        let _ = write!(
            out,
            "<w:tblLook w:firstRow=\"{}\" w:val=\"0020\"/></w:tblPr><w:tblGrid>",
            i32::from(!table.head.rows.is_empty())
        );
        for width in &widths {
            let _ = write!(out, "<w:gridCol w:w=\"{width}\"/>");
        }
        out.push_str("</w:tblGrid>");
        for row in &table.head.rows {
            out.push_str(&self.table_row(row, &alignments, true));
        }
        for body in &table.bodies {
            for row in body.head.iter().chain(&body.body) {
                out.push_str(&self.table_row(row, &alignments, false));
            }
        }
        for row in &table.foot.rows {
            out.push_str(&self.table_row(row, &alignments, false));
        }
        out.push_str("</w:tbl>");
        // A table must be followed by a paragraph or Word complains.
        out.push_str("<w:p/>");
        if !table.caption.blocks.is_empty() {
            out.push_str(&self.caption_paragraphs(&table.caption, "TableCaption"));
        }
        out
    }

    fn table_row(&mut self, row: &Row, alignments: &[Alignment], header: bool) -> String {
        let mut out = String::new();
        out.push_str("<w:tr>");
        if header {
            out.push_str("<w:trPr><w:tblHeader w:val=\"on\"/></w:trPr>");
        }
        let mut column = 0usize;
        for cell in &row.cells {
            out.push_str(&self.table_cell(cell, alignments.get(column).copied()));
            column += usize::try_from(cell.col_span).unwrap_or(1);
        }
        out.push_str("</w:tr>");
        out
    }

    fn table_cell(&mut self, cell: &Cell, column_alignment: Option<Alignment>) -> String {
        let mut properties = String::new();
        if cell.col_span != 1 {
            let _ = write!(properties, "<w:gridSpan w:val=\"{}\"/>", cell.col_span);
        }
        if cell.row_span != 1 {
            properties.push_str("<w:vMerge w:val=\"restart\"/>");
        }
        // The reader takes a cell's alignment from its first paragraph.
        let alignment = match cell.alignment {
            Alignment::AlignDefault => column_alignment.unwrap_or(Alignment::AlignDefault),
            explicit => explicit,
        };
        let justification = match alignment {
            Alignment::AlignLeft => Some("left"),
            Alignment::AlignRight => Some("right"),
            Alignment::AlignCenter => Some("center"),
            Alignment::AlignDefault => None,
        };
        let previous = std::mem::replace(&mut self.justification, justification);
        let content = self.without_numbering(|w| {
            if cell.blocks.is_empty() {
                w.paragraph("BodyText", &[])
            } else {
                w.blocks(&cell.blocks)
            }
        });
        self.justification = previous;
        let mut out = String::new();
        out.push_str("<w:tc>");
        if !properties.is_empty() {
            let _ = write!(out, "<w:tcPr>{properties}</w:tcPr>");
        }
        out.push_str(&content);
        out.push_str("</w:tc>");
        out
    }

    // --- inlines ---

    fn inlines(&mut self, inlines: &[Inline]) -> String {
        let style = RunStyle::default();
        inlines.iter().map(|i| self.inline(i, &style)).collect()
    }

    /// Render an inline, carrying the run style accumulated by the
    /// formatting containers around it (OOXML runs do not nest).
    fn inline(&mut self, inline: &Inline, style: &RunStyle) -> String {
        let nested = |writer: &mut Self, inner: &[Inline], style: &RunStyle| -> String {
            inner
                .iter()
                .map(|i| writer.inline(i, style))
                .collect::<String>()
        };
        match inline {
            Inline::Str(text) => run(style, &escape(text)),
            Inline::Space | Inline::SoftBreak => run(style, " "),
            Inline::LineBreak => format!("<w:r>{}<w:br/></w:r>", style.render()),
            Inline::Emph(inner) => nested(self, inner, &style.with(|s| s.italic = true)),
            Inline::Strong(inner) => nested(self, inner, &style.with(|s| s.bold = true)),
            Inline::Strikeout(inner) => nested(self, inner, &style.with(|s| s.strike = true)),
            Inline::Underline(inner) => nested(self, inner, &style.with(|s| s.underline = true)),
            Inline::SmallCaps(inner) => {
                nested(self, inner, &style.with(|s| s.small_caps = true))
            }
            Inline::Superscript(inner) => {
                nested(self, inner, &style.with(|s| s.vertical = Some("superscript")))
            }
            Inline::Subscript(inner) => {
                nested(self, inner, &style.with(|s| s.vertical = Some("subscript")))
            }
            Inline::Span(_, inner) | Inline::Cite(_, inner) => nested(self, inner, style),
            Inline::Quoted(quote, inner) => {
                let (open, close) = match quote {
                    QuoteType::SingleQuote => ("\u{2018}", "\u{2019}"),
                    QuoteType::DoubleQuote => ("\u{201C}", "\u{201D}"),
                };
                format!(
                    "{}{}{}",
                    run(style, open),
                    nested(self, inner, style),
                    run(style, close)
                )
            }
            Inline::Code(_, text) => run(
                &style.with(|s| s.character_style = Some("VerbatimChar")),
                &escape(text),
            ),
            Inline::Math(kind, text) => {
                let delimiter = match kind {
                    MathType::InlineMath => "$",
                    MathType::DisplayMath => "$$",
                };
                run(style, &escape(&format!("{delimiter}{text}{delimiter}")))
            }
            Inline::Link(_, inner, target) => {
                let id = self.link(&target.url);
                let inner_style = style.with(|s| s.character_style = Some("Hyperlink"));
                format!(
                    "<w:hyperlink r:id=\"{id}\">{}</w:hyperlink>",
                    nested(self, inner, &inner_style)
                )
            }
            Inline::Image(attr, alt, target) => {
                let alt_text = plain_text(alt);
                self.picture(attr, &alt_text, target)
                    // No bytes, or a format that cannot be embedded: the
                    // alt text is all that is left to carry.
                    .unwrap_or_else(|| nested(self, alt, style))
            }
            Inline::Note(blocks) => {
                let body = self
                    .without_numbering(|w| w.with_style("FootnoteText", |w| w.blocks(blocks)));
                self.footnotes.push(body);
                let id = FOOTNOTE_BASE + self.footnotes.len() - 1;
                format!(
                    "<w:r><w:rPr><w:rStyle w:val=\"FootnoteReference\"/></w:rPr><w:footnoteReference w:id=\"{id}\"/></w:r>"
                )
            }
            // Raw content is not OOXML and has nowhere to go.
            Inline::RawInline(..) => String::new(),
        }
    }

    /// Embed an image as a `w:drawing`, or `None` if its bytes could not
    /// be resolved or are not an embeddable format.
    fn picture(&mut self, attr: &Attr, alt: &str, target: &Target) -> Option<String> {
        // One media part per source image, however often it appears.
        let existing = self.relations.iter().position(|relation| {
            matches!(relation, Relation::Image { url, .. } if *url == target.url)
        });
        let (id, image) = if let Some(index) = existing {
            let Relation::Image { image, .. } = &self.relations[index] else {
                unreachable!("the position predicate matched an image")
            };
            (format!("rId{}", RELS_BASE + index + 1), *image)
        } else {
            let bytes = (self.media)(&target.url)?;
            let image = media::inspect(&bytes)?;
            let url = target.url.clone();
            (self.relate(Relation::Image { url, image, bytes }), image)
        };
        // The document's own size wins; failing that the image's own,
        // which pandoc reads at one pixel per point.
        let width = emu(attr, "width").unwrap_or_else(|| i64::from(image.width) * EMU_PER_POINT);
        let height = emu(attr, "height").unwrap_or_else(|| i64::from(image.height) * EMU_PER_POINT);
        let (alt, title) = (escape_attribute(alt), escape_attribute(&target.title));
        Some(format!(
            concat!(
                r#"<w:r><w:drawing><wp:inline><wp:extent cx="{width}" cy="{height}"/>"#,
                r#"<wp:docPr id="1" name="Picture" descr="{alt}" title="{title}"/>"#,
                r#"<a:graphic><a:graphicData uri="http://schemas.openxmlformats.org/drawingml/2006/picture">"#,
                r#"<pic:pic><pic:nvPicPr><pic:cNvPr id="0" name="Picture" descr="{alt}"/><pic:cNvPicPr/></pic:nvPicPr>"#,
                r#"<pic:blipFill><a:blip r:embed="{id}"/><a:stretch><a:fillRect/></a:stretch></pic:blipFill>"#,
                r#"<pic:spPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="{width}" cy="{height}"/></a:xfrm>"#,
                r#"<a:prstGeom prst="rect"><a:avLst/></a:prstGeom></pic:spPr></pic:pic>"#,
                r#"</a:graphicData></a:graphic></wp:inline></w:drawing></w:r>"#
            ),
            width = width,
            height = height,
            alt = alt,
            title = title,
            id = id
        ))
    }

    /// The styled leading paragraphs that carry document metadata. Only the
    /// four fields the reader recovers are written; anything else in the
    /// metadata has no place in a `.docx` to go.
    fn metadata(&mut self, meta: &Meta) -> String {
        let mut out = String::new();
        for (field, style) in [
            ("title", "Title"),
            ("subtitle", "Subtitle"),
            ("author", "Author"),
            ("date", "Date"),
        ] {
            let Some(value) = meta.get(field) else { continue };
            // A repeated field (several authors) is one paragraph each,
            // which is how the reader tells them apart.
            for inlines in meta_inlines(value) {
                out.push_str(&self.paragraph(style, &inlines));
            }
        }
        out
    }

    // --- generated parts ---

    fn document_rels(&self) -> String {
        let mut out = String::new();
        out.push_str(XML_DECL);
        out.push_str(r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">"#);
        for (id, target, external) in [
            (1, "styles.xml", false),
            (2, "numbering.xml", false),
            (3, "footnotes.xml", false),
        ] {
            let kind = match target {
                "styles.xml" => "styles",
                "numbering.xml" => "numbering",
                _ => "footnotes",
            };
            let _ = write!(
                out,
                r#"<Relationship Id="rId{id}" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/{kind}" Target="{target}"/>"#
            );
            let _ = external;
        }
        for (index, relation) in self.relations.iter().enumerate() {
            let id = RELS_BASE + index + 1;
            match relation {
                Relation::Hyperlink(url) => {
                    let _ = write!(
                        out,
                        r#"<Relationship Id="rId{id}" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/hyperlink" Target="{}" TargetMode="External"/>"#,
                        escape_attribute(url)
                    );
                }
                Relation::Image { image, .. } => {
                    // Relative to `word/`, which is where the part lives.
                    let _ = write!(
                        out,
                        r#"<Relationship Id="rId{id}" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/image" Target="media/image{}.{}"/>"#,
                        index + 1,
                        image.extension
                    );
                }
            }
        }
        out.push_str("</Relationships>");
        out
    }

    /// The content types part, which must declare every media extension
    /// the package actually contains or Word rejects the file.
    fn content_types(&self) -> String {
        let mut out = String::from(CONTENT_TYPES_HEAD);
        let mut declared: Vec<&str> = Vec::new();
        for relation in &self.relations {
            if let Relation::Image { image, .. } = relation
                && !declared.contains(&image.extension)
            {
                declared.push(image.extension);
                let _ = write!(
                    out,
                    r#"<Default Extension="{}" ContentType="{}"/>"#,
                    image.extension, image.content_type
                );
            }
        }
        out.push_str(CONTENT_TYPES_TAIL);
        out
    }

    fn numbering(&self) -> String {
        let mut out = String::new();
        out.push_str(XML_DECL);
        out.push_str(r#"<w:numbering xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">"#);
        // The continuation definition: a blank marker, which the reader
        // treats as "this paragraph continues the previous item".
        out.push_str(&abstract_numbering(CONTINUATION_ABSTRACT, "bullet", " "));
        for (index, list) in self.lists.iter().enumerate() {
            out.push_str(&abstract_numbering(
                CONTINUATION_ABSTRACT + index + 1,
                list.format,
                &list.marker,
            ));
        }
        let _ = write!(
            out,
            r#"<w:num w:numId="{CONTINUATION_NUM}"><w:abstractNumId w:val="{CONTINUATION_ABSTRACT}"/></w:num>"#
        );
        for (index, list) in self.lists.iter().enumerate() {
            let _ = write!(
                out,
                r#"<w:num w:numId="{}"><w:abstractNumId w:val="{}"/><w:lvlOverride w:ilvl="0"><w:startOverride w:val="{}"/></w:lvlOverride></w:num>"#,
                NUM_BASE + index + 1,
                CONTINUATION_ABSTRACT + index + 1,
                list.start
            );
        }
        out.push_str("</w:numbering>");
        out
    }

    fn footnotes_part(&self) -> String {
        let mut out = String::new();
        out.push_str(XML_DECL);
        out.push_str(r#"<w:footnotes xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:footnote w:type="separator" w:id="-1"><w:p><w:r><w:separator/></w:r></w:p></w:footnote><w:footnote w:type="continuationSeparator" w:id="0"><w:p><w:r><w:continuationSeparator/></w:r></w:p></w:footnote>"#);
        for (index, body) in self.footnotes.iter().enumerate() {
            let _ = write!(
                out,
                r#"<w:footnote w:id="{}">{}</w:footnote>"#,
                FOOTNOTE_BASE + index,
                if body.is_empty() { "<w:p/>" } else { body }
            );
        }
        out.push_str("</w:footnotes>");
        out
    }
}

/// The paragraphs a code block becomes: one per line, with a single
/// trailing newline dropped (it is the block terminator, not a blank
/// line), and nothing at all for an empty block — matching what pandoc's
/// own writer round-trips to.
fn code_block_lines(text: &str) -> std::str::Split<'_, char> {
    let trimmed = text.strip_suffix('\n').unwrap_or(text);
    trimmed.split('\n')
}

/// One numbering definition, the same shape at every level.
fn abstract_numbering(id: usize, format: &str, marker: &str) -> String {
    let mut out = String::new();
    let _ = write!(out, r#"<w:abstractNum w:abstractNumId="{id}"><w:multiLevelType w:val="multilevel"/>"#);
    for level in 0..9 {
        let marker = marker.replace("%1", &format!("%{}", level + 1));
        let _ = write!(
            out,
            r#"<w:lvl w:ilvl="{level}"><w:start w:val="1"/><w:numFmt w:val="{format}"/><w:lvlText w:val="{}"/><w:lvlJc w:val="left"/><w:pPr><w:ind w:left="{}" w:hanging="360"/></w:pPr></w:lvl>"#,
            escape_attribute(&marker),
            (level + 1) * 720
        );
    }
    out.push_str("</w:abstractNum>");
    out
}

/// The formatting a run carries. OOXML fixes the order of `w:rPr`
/// children, so the style is collected as flags and rendered in that
/// order rather than concatenated as encountered.
#[derive(Clone, Default)]
#[allow(clippy::struct_excessive_bools)]
struct RunStyle {
    character_style: Option<&'static str>,
    bold: bool,
    italic: bool,
    small_caps: bool,
    strike: bool,
    underline: bool,
    vertical: Option<&'static str>,
}

impl RunStyle {
    /// This style with one field changed.
    fn with(&self, edit: impl FnOnce(&mut Self)) -> Self {
        let mut next = self.clone();
        edit(&mut next);
        next
    }

    /// The `w:rPr` element, with children in schema order.
    fn render(&self) -> String {
        if self.character_style.is_none()
            && !self.bold
            && !self.italic
            && !self.small_caps
            && !self.strike
            && !self.underline
            && self.vertical.is_none()
        {
            return String::new();
        }
        let mut out = String::with_capacity(48);
        out.push_str("<w:rPr>");
        if let Some(style) = self.character_style {
            let _ = write!(out, "<w:rStyle w:val=\"{style}\"/>");
        }
        if self.bold {
            out.push_str("<w:b/>");
        }
        if self.italic {
            out.push_str("<w:i/>");
        }
        if self.small_caps {
            out.push_str("<w:smallCaps/>");
        }
        if self.strike {
            out.push_str("<w:strike/>");
        }
        if self.underline {
            out.push_str("<w:u w:val=\"single\"/>");
        }
        if let Some(vertical) = self.vertical {
            let _ = write!(out, "<w:vertAlign w:val=\"{vertical}\"/>");
        }
        out.push_str("</w:rPr>");
        out
    }
}

/// Wrap text in a run carrying the given style.
fn run(style: &RunStyle, text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 64);
    out.push_str("<w:r>");
    out.push_str(&style.render());
    out.push_str("<w:t xml:space=\"preserve\">");
    out.push_str(text);
    out.push_str("</w:t></w:r>");
    out
}

/// Escape XML text content. Ordinary characters — nearly all of them —
/// are copied in slices rather than one at a time.
fn escape(text: &str) -> String {
    let mut out = String::new();
    escape_into(&mut out, text);
    out
}

/// Escape XML text content into an existing buffer. Per character on
/// purpose — see the note in the HTML writer; searching for the next
/// special character measured slower on the short strings documents are
/// made of.
fn escape_into(out: &mut String, text: &str) {
    for ch in text.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            // Control characters are not representable in XML.
            c if (c as u32) < 0x20 && c != '\t' => {}
            c => out.push(c),
        }
    }
}

/// Escape an XML attribute value.
fn escape_attribute(text: &str) -> String {
    escape(text).replace('"', "&quot;")
}

/// The text an inline sequence carries, for attributes that hold no markup.
fn plain_text(inlines: &[Inline]) -> String {
    fn walk(out: &mut String, inlines: &[Inline]) {
        for inline in inlines {
            match inline {
                Inline::Str(text) | Inline::Code(_, text) | Inline::Math(_, text) => {
                    out.push_str(text);
                }
                Inline::Space | Inline::SoftBreak | Inline::LineBreak => out.push(' '),
                Inline::Emph(inner)
                | Inline::Strong(inner)
                | Inline::Strikeout(inner)
                | Inline::Superscript(inner)
                | Inline::Subscript(inner)
                | Inline::SmallCaps(inner)
                | Inline::Underline(inner)
                | Inline::Quoted(_, inner)
                | Inline::Span(_, inner)
                | Inline::Cite(_, inner)
                | Inline::Link(_, inner, _)
                | Inline::Image(_, inner, _) => walk(out, inner),
                Inline::RawInline(..) | Inline::Note(_) => {}
            }
        }
    }
    let mut out = String::new();
    walk(&mut out, inlines);
    out
}

/// A dimension attribute (`width`, `height`) in EMU, if the document
/// states one. Understands the units pandoc's readers produce.
fn emu(attr: &Attr, name: &str) -> Option<i64> {
    let value = attr
        .attributes
        .iter()
        .find(|(key, _)| key == name)
        .map(|(_, value)| value.trim())?;
    let digits = value.trim_end_matches(|c: char| c.is_ascii_alphabetic() || c == '%');
    let number: f64 = digits.parse().ok()?;
    let per_unit = match &value[digits.len()..] {
        "in" => 914_400.0,
        "cm" => 360_000.0,
        "mm" => 36_000.0,
        // A bare number is pixels, and a pixel is a point here — the same
        // 72 dpi the intrinsic size is read at.
        "pt" | "px" | "" => 12_700.0,
        _ => return None,
    };
    #[expect(clippy::cast_possible_truncation, reason = "EMU are whole units")]
    Some((number * per_unit) as i64)
}

/// The values of a metadata field, one per paragraph to write for it.
fn meta_inlines(value: &MetaValue) -> Vec<Vec<Inline>> {
    match value {
        MetaValue::MetaInlines(inlines) => vec![inlines.clone()],
        MetaValue::MetaString(text) => vec![vec![Inline::Str(text.clone())]],
        MetaValue::MetaList(values) => values.iter().flat_map(meta_inlines).collect(),
        MetaValue::MetaBlocks(blocks) => blocks
            .iter()
            .filter_map(|block| match block {
                Block::Plain(inlines) | Block::Para(inlines) => Some(inlines.clone()),
                _ => None,
            })
            .collect(),
        MetaValue::MetaBool(_) | MetaValue::MetaMap(_) => Vec::new(),
    }
}

const XML_DECL: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#;
/// Relationship ids 1..3 are the fixed parts; links start after them.
const RELS_BASE: usize = 3;
const NUM_BASE: usize = 1000;
const CONTINUATION_NUM: usize = 1000;
const CONTINUATION_ABSTRACT: usize = 900;
const FOOTNOTE_BASE: usize = 2;

/// One EMU is 1/914400 inch; one point is 1/72 inch.
const EMU_PER_POINT: i64 = 12_700;

const CONTENT_TYPES_HEAD: &str = concat!(
    r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#,
    r#"<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">"#,
    r#"<Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>"#,
    r#"<Default Extension="xml" ContentType="application/xml"/>"#,
);

const CONTENT_TYPES_TAIL: &str = concat!(
    r#"<Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/>"#,
    r#"<Override PartName="/word/styles.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.styles+xml"/>"#,
    r#"<Override PartName="/word/numbering.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.numbering+xml"/>"#,
    r#"<Override PartName="/word/footnotes.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.footnotes+xml"/>"#,
    r#"</Types>"#
);

const PACKAGE_RELS: &str = concat!(
    r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#,
    r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">"#,
    r#"<Relationship Id="rId0" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/>"#,
    r#"</Relationships>"#
);

/// The style sheet. Only the style *names* matter for round-tripping —
/// they are what pandoc's reader matches on.
const STYLES: &str = concat!(
    r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#,
    r#"<w:styles xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">"#,
    r#"<w:style w:type="paragraph" w:default="1" w:styleId="Normal"><w:name w:val="Normal"/></w:style>"#,
    r#"<w:style w:type="paragraph" w:styleId="BodyText"><w:name w:val="Body Text"/><w:basedOn w:val="Normal"/></w:style>"#,
    r#"<w:style w:type="paragraph" w:styleId="Compact"><w:name w:val="Compact"/><w:basedOn w:val="BodyText"/></w:style>"#,
    r#"<w:style w:type="paragraph" w:styleId="SourceCode"><w:name w:val="Source Code"/><w:basedOn w:val="Normal"/></w:style>"#,
    r#"<w:style w:type="paragraph" w:styleId="BlockText"><w:name w:val="Block Text"/><w:basedOn w:val="BodyText"/></w:style>"#,
    r#"<w:style w:type="paragraph" w:styleId="FootnoteText"><w:name w:val="Footnote Text"/><w:basedOn w:val="Normal"/></w:style>"#,
    r#"<w:style w:type="paragraph" w:styleId="DefinitionTerm"><w:name w:val="Definition Term"/><w:basedOn w:val="Normal"/></w:style>"#,
    r#"<w:style w:type="paragraph" w:styleId="Definition"><w:name w:val="Definition"/><w:basedOn w:val="Normal"/></w:style>"#,
    r#"<w:style w:type="paragraph" w:styleId="Title"><w:name w:val="Title"/><w:basedOn w:val="Normal"/></w:style>"#,
    r#"<w:style w:type="paragraph" w:styleId="Subtitle"><w:name w:val="Subtitle"/><w:basedOn w:val="Normal"/></w:style>"#,
    r#"<w:style w:type="paragraph" w:styleId="Author"><w:name w:val="Author"/><w:basedOn w:val="Normal"/></w:style>"#,
    r#"<w:style w:type="paragraph" w:styleId="Date"><w:name w:val="Date"/><w:basedOn w:val="Normal"/></w:style>"#,
    r#"<w:style w:type="paragraph" w:styleId="ImageCaption"><w:name w:val="Image Caption"/><w:basedOn w:val="Normal"/></w:style>"#,
    r#"<w:style w:type="paragraph" w:styleId="TableCaption"><w:name w:val="Table Caption"/><w:basedOn w:val="Normal"/></w:style>"#,
    r#"<w:style w:type="paragraph" w:styleId="Heading1"><w:name w:val="heading 1"/><w:basedOn w:val="Normal"/></w:style>"#,
    r#"<w:style w:type="paragraph" w:styleId="Heading2"><w:name w:val="heading 2"/><w:basedOn w:val="Normal"/></w:style>"#,
    r#"<w:style w:type="paragraph" w:styleId="Heading3"><w:name w:val="heading 3"/><w:basedOn w:val="Normal"/></w:style>"#,
    r#"<w:style w:type="paragraph" w:styleId="Heading4"><w:name w:val="heading 4"/><w:basedOn w:val="Normal"/></w:style>"#,
    r#"<w:style w:type="paragraph" w:styleId="Heading5"><w:name w:val="heading 5"/><w:basedOn w:val="Normal"/></w:style>"#,
    r#"<w:style w:type="paragraph" w:styleId="Heading6"><w:name w:val="heading 6"/><w:basedOn w:val="Normal"/></w:style>"#,
    r#"<w:style w:type="paragraph" w:styleId="Heading7"><w:name w:val="heading 7"/><w:basedOn w:val="Normal"/></w:style>"#,
    r#"<w:style w:type="paragraph" w:styleId="Heading8"><w:name w:val="heading 8"/><w:basedOn w:val="Normal"/></w:style>"#,
    r#"<w:style w:type="paragraph" w:styleId="Heading9"><w:name w:val="heading 9"/><w:basedOn w:val="Normal"/></w:style>"#,
    r#"<w:style w:type="character" w:styleId="VerbatimChar"><w:name w:val="Verbatim Char"/></w:style>"#,
    r#"<w:style w:type="character" w:styleId="Hyperlink"><w:name w:val="Hyperlink"/></w:style>"#,
    r#"<w:style w:type="character" w:styleId="FootnoteReference"><w:name w:val="Footnote Reference"/></w:style>"#,
    r#"<w:style w:type="table" w:styleId="Table"><w:name w:val="Table"/></w:style>"#,
    r#"</w:styles>"#
);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::read_docx;

    /// A 64x32 PNG whose pixels do not matter, only its header.
    fn swatch() -> Vec<u8> {
        let mut png = b"\x89PNG\r\n\x1a\n\x00\x00\x00\x0dIHDR".to_vec();
        png.extend_from_slice(&64u32.to_be_bytes());
        png.extend_from_slice(&32u32.to_be_bytes());
        png.extend_from_slice(&[0; 5]);
        png
    }

    fn image(attributes: Vec<(String, String)>) -> Block {
        Block::Para(vec![Inline::Image(
            Attr { attributes, ..Attr::default() },
            vec![Inline::Str("alt".to_owned())],
            Target { url: "swatch.png".to_owned(), title: String::new() },
        )])
    }

    fn round_trip(doc: &Pandoc) -> Pandoc {
        let bytes = write_docx_with_media(doc, &|url| {
            (url == "swatch.png").then(swatch)
        })
        .expect("writes");
        read_docx(&bytes).expect("reads back")
    }

    #[test]
    fn an_image_becomes_a_picture_sized_from_its_header() {
        let doc = Pandoc { blocks: vec![image(Vec::new())], ..Pandoc::default() };
        let back = round_trip(&doc);
        let Block::Para(inlines) = &back.blocks[0] else { panic!("expected a paragraph") };
        let [Inline::Image(attr, alt, target)] = inlines.as_slice() else {
            panic!("expected an image, got {inlines:?}")
        };
        // 64 and 32 pixels, read at one pixel per point.
        assert_eq!(attr.attributes[0], ("width".to_owned(), "0.8888888888888888in".to_owned()));
        assert_eq!(attr.attributes[1], ("height".to_owned(), "0.4444444444444444in".to_owned()));
        assert_eq!(alt, &[Inline::Str("alt".to_owned())]);
        assert!(target.url.starts_with("media/"), "{target:?} should point at a media part");
    }

    #[test]
    fn a_stated_size_wins_over_the_image_header() {
        let doc = Pandoc {
            blocks: vec![image(vec![("width".to_owned(), "2in".to_owned())])],
            ..Pandoc::default()
        };
        let back = round_trip(&doc);
        let Block::Para(inlines) = &back.blocks[0] else { panic!("expected a paragraph") };
        let [Inline::Image(attr, ..)] = inlines.as_slice() else { panic!("expected an image") };
        assert_eq!(attr.attributes[0], ("width".to_owned(), "2.0in".to_owned()));
    }

    #[test]
    fn one_media_part_serves_every_use_of_an_image() {
        let doc = Pandoc {
            blocks: vec![image(Vec::new()), image(Vec::new())],
            ..Pandoc::default()
        };
        let bytes = write_docx_with_media(&doc, &|_| Some(swatch())).expect("writes");
        let back = read_docx(&bytes).expect("reads back");
        let url = |block: &Block| {
            let Block::Para(inlines) = block else { panic!("expected a paragraph") };
            let [Inline::Image(_, _, target)] = inlines.as_slice() else {
                panic!("expected an image")
            };
            target.url.clone()
        };
        assert_eq!(url(&back.blocks[0]), url(&back.blocks[1]));
    }

    #[test]
    fn an_image_with_no_bytes_falls_back_to_its_alt_text() {
        let doc = Pandoc { blocks: vec![image(Vec::new())], ..Pandoc::default() };
        let back = read_docx(&write_docx(&doc).expect("writes")).expect("reads back");
        assert_eq!(back.blocks, vec![Block::Para(vec![Inline::Str("alt".to_owned())])]);
    }

    #[test]
    fn many_headings_sharing_a_name_stay_linear() {
        // Uniquing an identifier used to restart its search at `-1` every
        // time, which is quadratic in the number of headings that share a
        // name — and "Summary" repeated per section is what an ordinary
        // document looks like. 20_000 of them took over a minute.
        let heading = |text: &str| {
            Block::Header(2, Attr::default(), vec![Inline::Str(text.to_owned())])
        };
        let doc = Pandoc {
            blocks: (0..20_000).map(|_| heading("Summary")).collect(),
            ..Pandoc::default()
        };
        let bytes = write_docx(&doc).expect("writes");
        let start = std::time::Instant::now();
        let back = read_docx(&bytes).expect("reads back");
        assert_eq!(back.blocks.len(), 20_000);
        // Generous: the linear version is well under a second in debug.
        assert!(
            start.elapsed() < std::time::Duration::from_secs(20),
            "reading 20_000 same-named headings took {:?}",
            start.elapsed()
        );
    }

    #[test]
    fn metadata_survives_as_styled_leading_paragraphs() {
        let inlines = |text: &str| MetaValue::MetaInlines(vec![Inline::Str(text.to_owned())]);
        let mut meta = Meta::new();
        meta.insert("title".to_owned(), inlines("T"));
        meta.insert("subtitle".to_owned(), inlines("S"));
        meta.insert(
            "author".to_owned(),
            MetaValue::MetaList(vec![inlines("A1"), inlines("A2")]),
        );
        meta.insert("date".to_owned(), inlines("D"));
        let doc = Pandoc {
            meta,
            blocks: vec![Block::Para(vec![Inline::Str("body".to_owned())])],
            ..Pandoc::default()
        };
        let back = read_docx(&write_docx(&doc).expect("writes")).expect("reads back");
        assert_eq!(back.meta, doc.meta);
        assert_eq!(back.blocks, doc.blocks);
    }
}
