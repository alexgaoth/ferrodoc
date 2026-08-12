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

use crate::Error;
use ferrodoc_ast::{
    Alignment, Block, Caption, Cell, ColWidth, Inline, ListAttributes, ListNumberDelim,
    ListNumberStyle, MathType, Pandoc, QuoteType, Row, Table,
};
use std::fmt::Write as _;
use std::io::Write as _;
use zip::write::SimpleFileOptions;

/// The text width in twips the reader assumes, used to turn the AST's
/// fractional column widths back into grid columns.
const TEXT_WIDTH: f64 = 9360.0;

/// Render a document as a `.docx` package.
pub fn write_docx(doc: &Pandoc) -> Result<Vec<u8>, Error> {
    let mut w = Writer::default();
    let body = w.blocks(&doc.blocks);

    let mut document = String::new();
    document.push_str(XML_DECL);
    document.push_str(
        r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><w:body>"#,
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
    part("[Content_Types].xml", CONTENT_TYPES)?;
    part("_rels/.rels", PACKAGE_RELS)?;
    part("word/document.xml", &document)?;
    part("word/_rels/document.xml.rels", &w.document_rels())?;
    part("word/styles.xml", STYLES)?;
    part("word/numbering.xml", &w.numbering())?;
    part("word/footnotes.xml", &w.footnotes_part())?;
    let cursor = zip.finish().map_err(|e| Error::Zip(e.to_string()))?;
    Ok(cursor.into_inner())
}

/// Accumulates the parts that depend on document content: hyperlink
/// relationships, numbering definitions and footnote bodies.
#[derive(Default)]
struct Writer {
    /// External link targets, in relationship order.
    links: Vec<String>,
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

struct ListDefinition {
    format: &'static str,
    marker: String,
    start: i64,
}

impl Writer {
    /// The relationship id for an external link target.
    fn link(&mut self, url: &str) -> String {
        self.links.push(url.to_owned());
        format!("rId{}", RELS_BASE + self.links.len())
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
            // Without media parts an image can only survive as its alt text.
            Inline::Image(_, alt, _) => nested(self, alt, style),
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
        for (index, url) in self.links.iter().enumerate() {
            let _ = write!(
                out,
                r#"<Relationship Id="rId{}" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/hyperlink" Target="{}" TargetMode="External"/>"#,
                RELS_BASE + index + 1,
                escape_attribute(url)
            );
        }
        out.push_str("</Relationships>");
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

const XML_DECL: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#;
/// Relationship ids 1..3 are the fixed parts; links start after them.
const RELS_BASE: usize = 3;
const NUM_BASE: usize = 1000;
const CONTINUATION_NUM: usize = 1000;
const CONTINUATION_ABSTRACT: usize = 900;
const FOOTNOTE_BASE: usize = 2;

const CONTENT_TYPES: &str = concat!(
    r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#,
    r#"<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">"#,
    r#"<Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>"#,
    r#"<Default Extension="xml" ContentType="application/xml"/>"#,
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
