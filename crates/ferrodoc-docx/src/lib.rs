//! DOCX reader producing the ferrodoc (pandoc-compatible) AST.
//!
//! [`read_docx`] parses a `.docx` byte stream and maps it to the same AST
//! `pandoc -f docx -t json` produces (differentially verified by
//! `ferrodoc-harness diff-docx`). The mapping follows pandoc's docx reader
//! closely, including several behaviors that are not evident from the
//! OOXML spec:
//!
//! - inline structure is rebuilt with a port of pandoc's `Docx.Combine`
//!   smushing algorithm (see [`combine`]), because OOXML runs do not nest;
//! - paragraphs trim leading/trailing spaces *and* line breaks; headings
//!   do not trim at all;
//! - list numbering continues across separate lists that reuse the same
//!   `w:numId` (a different instance restarts, even when both point at one
//!   abstract definition);
//! - a level whose marker text is blank encodes a continuation paragraph
//!   of the previous list item;
//! - column widths are fractions of `textWidth - 10*(columns-1)` (10 twips
//!   of inter-column space), normalized if they sum above 1, where
//!   `textWidth` comes from the section's page size minus margins and
//!   gutter, defaulting to 9360 twips;
//! - column alignments come from the first *body* row's cells, and a cell's
//!   alignment comes from its first paragraph's justification;
//! - a paragraph whose style (or an ancestor style) is named "caption",
//!   "table caption" or "image caption" captions an adjacent table or
//!   image-only paragraph, on either side.
//!
//! Milestone scope (Phase 2, reader core): paragraphs and formatting runs,
//! headings, hyperlinks, inline code, lists, tables (including
//! `gridSpan`/`vMerge` spans and header rows), images, figures, footnotes,
//! and line breaks. Comments, tracked changes, fields, text boxes, and
//! custom style maps are out of scope and are skipped.

mod combine;
mod xml;

use combine::{Modifier, smush_blocks, smush_inlines, stack};
use ferrodoc_ast::{
    Alignment, Attr, Block, Caption, Cell, ColSpec, ColWidth, Inline, ListAttributes,
    ListNumberDelim, ListNumberStyle, MathType, Pandoc, Row, Table, TableBody, TableFoot,
    TableHead, Target,
};
use std::collections::{HashMap, HashSet};
use std::io::Read;
use xml::Node;

/// Pandoc's default text width in twips, used when the section properties
/// do not give a page size and margins.
const DEFAULT_TEXT_WIDTH: f64 = 9360.0;

/// An error reading a DOCX file.
#[derive(Debug)]
pub enum Error {
    /// The container is not a readable zip archive.
    Zip(String),
    /// An XML part failed to parse.
    Xml(String),
    /// A required part is missing from the archive.
    MissingPart(&'static str),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Zip(e) => write!(f, "not a readable docx (zip) archive: {e}"),
            Error::Xml(e) => write!(f, "malformed XML part: {e}"),
            Error::MissingPart(p) => write!(f, "missing required part: {p}"),
        }
    }
}

impl std::error::Error for Error {}

/// Read a DOCX document into a [`Pandoc`] AST equivalent to pandoc's docx
/// reader output.
pub fn read_docx(bytes: &[u8]) -> Result<Pandoc, Error> {
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes))
        .map_err(|e| Error::Zip(e.to_string()))?;
    let mut part = |name: &str| -> Option<String> {
        let mut file = archive.by_name(name).ok()?;
        let mut s = String::new();
        file.read_to_string(&mut s).ok()?;
        Some(s)
    };
    let document = part("word/document.xml").ok_or(Error::MissingPart("word/document.xml"))?;
    let numbering = part("word/numbering.xml");
    let rels = part("word/_rels/document.xml.rels");
    let footnotes = part("word/footnotes.xml");
    let styles = part("word/styles.xml");

    let document = xml::parse(&document)?;
    let body = document.child("body").ok_or(Error::MissingPart("w:body"))?;
    let ctx = Ctx {
        numbering: numbering.as_deref().map(xml::parse).transpose()?,
        rels: rels
            .as_deref()
            .map(xml::parse)
            .transpose()?
            .as_ref()
            .map(parse_rels)
            .unwrap_or_default(),
        footnotes: footnotes.as_deref().map(xml::parse).transpose()?,
        styles: styles
            .as_deref()
            .map(xml::parse)
            .transpose()?
            .as_ref()
            .map(parse_styles)
            .unwrap_or_default(),
        text_width: text_width(body),
    };
    let mut state = State::default();
    Ok(Pandoc::new(ctx.blocks(body, &mut state)))
}

fn parse_rels(rels: &Node) -> HashMap<String, String> {
    rels.children_named("Relationship")
        .filter_map(|r| Some((r.attr("Id")?.to_owned(), r.attr("Target")?.to_owned())))
        .collect()
}

/// Map style id to (style name, based-on style id).
fn parse_styles(styles: &Node) -> HashMap<String, (String, Option<String>)> {
    styles
        .children_named("style")
        .filter_map(|s| {
            let id = s.attr("w:styleId")?.to_owned();
            let name = s.child("name").and_then(|n| n.attr("w:val")).unwrap_or("");
            let based_on = s
                .child("basedOn")
                .and_then(|b| b.attr("w:val"))
                .map(str::to_owned);
            Some((id, (name.to_owned(), based_on)))
        })
        .collect()
}

/// The section's text width in twips: page width minus left/right margins
/// and gutter, defaulting to [`DEFAULT_TEXT_WIDTH`].
fn text_width(body: &Node) -> f64 {
    let sect = body.child("sectPr");
    let value = |node: Option<&Node>, attr: &str| -> Option<f64> {
        node?.attr(attr)?.parse().ok()
    };
    let sect = sect.as_ref();
    let width = value(sect.and_then(|s| s.child("pgSz")), "w:w");
    let margins = sect.and_then(|s| s.child("pgMar"));
    let left = value(margins, "w:left");
    let right = value(margins, "w:right");
    let gutter = value(margins, "w:gutter");
    match (width, left, right, gutter) {
        (Some(w), Some(l), Some(r), Some(g)) => w - (l + r + g),
        _ => DEFAULT_TEXT_WIDTH,
    }
}

/// Mutable state threaded through the conversion: header identifiers and
/// list-numbering continuation.
#[derive(Default)]
struct State {
    used_idents: HashSet<String>,
    /// Last number used per (`w:numId`, level).
    list_numbers: HashMap<(String, usize), i64>,
}

impl State {
    /// Pandoc's `auto_identifiers`: keep alphanumerics, whitespace and
    /// `_-.`; lowercase; join whitespace-separated *words* with `-` (so
    /// runs of spaces collapse and trailing space vanishes); drop
    /// everything before the first letter; empty becomes `section`;
    /// duplicates get `-1`, `-2`, … suffixes.
    fn ident(&mut self, inlines: &[Inline]) -> String {
        let filtered: String = plain_text(inlines)
            .chars()
            .filter(|c| {
                c.is_alphanumeric() || c.is_whitespace() || matches!(c, '_' | '-' | '.')
            })
            .flat_map(char::to_lowercase)
            .collect();
        let joined: String = filtered.split_whitespace().collect::<Vec<_>>().join("-");
        let start = joined
            .find(char::is_alphabetic)
            .unwrap_or(joined.len());
        let mut id = joined[start..].to_owned();
        if id.is_empty() {
            "section".clone_into(&mut id);
        }
        let mut unique = id.clone();
        let mut n = 0;
        while !self.used_idents.insert(unique.clone()) {
            n += 1;
            unique = format!("{id}-{n}");
        }
        unique
    }

    /// The number this list item takes: a numbering instance picks up
    /// where its previous use left off, and deeper levels' continuation
    /// data expires when a shallower item appears.
    fn list_number(&mut self, num_id: &str, level: usize, level_start: i64) -> i64 {
        let key = (num_id.to_owned(), level);
        let start = match self.list_numbers.get(&key) {
            Some(previous) => previous + 1,
            None => level_start,
        };
        self.list_numbers.retain(|(_, lvl), _| *lvl <= level);
        self.list_numbers.insert(key, start);
        start
    }
}

fn plain_text(inlines: &[Inline]) -> String {
    fn walk(out: &mut String, inlines: &[Inline]) {
        for inline in inlines {
            match inline {
                Inline::Str(s) | Inline::Code(_, s) | Inline::Math(_, s) => out.push_str(s),
                Inline::Space | Inline::SoftBreak | Inline::LineBreak => out.push(' '),
                Inline::Emph(i) | Inline::Strong(i) | Inline::Strikeout(i)
                | Inline::Superscript(i) | Inline::Subscript(i) | Inline::SmallCaps(i)
                | Inline::Underline(i) | Inline::Link(_, i, _) | Inline::Image(_, i, _)
                | Inline::Span(_, i) => walk(out, i),
                _ => {}
            }
        }
    }
    let mut out = String::new();
    walk(&mut out, inlines);
    out
}

struct Ctx {
    numbering: Option<Node>,
    rels: HashMap<String, String>,
    footnotes: Option<Node>,
    styles: HashMap<String, (String, Option<String>)>,
    text_width: f64,
}

/// A numbered paragraph waiting to be rebuilt into a list.
struct ListItem {
    level: usize,
    /// The `w:numId` this item belongs to: lists sharing one continue
    /// each other's numbering, and adjacent items with it form one list.
    num_id: String,
    kind: ListKind,
    /// Whether this level's marker is blank: a continuation paragraph.
    continuation: bool,
    block: Block,
}

/// The kind of list a numbering level defines.
#[derive(PartialEq, Clone)]
enum ListKind {
    Bullet,
    Ordered(i64, ListNumberStyle, ListNumberDelim),
}

/// A parsed table cell, before row spans are resolved.
struct RawCell {
    alignment: Alignment,
    grid_span: i64,
    /// True when the cell continues a vertical merge from above.
    merged: bool,
    blocks: Vec<Block>,
}

/// A parsed table row, before header/body splitting.
struct RawRow {
    header: bool,
    cells: Vec<RawCell>,
}

impl Ctx {
    /// Convert a container's paragraphs and tables to blocks. Captions are
    /// paired with adjacent tables and image paragraphs first, numbered
    /// paragraphs accumulate into lists, and the resulting segments are
    /// merged by pandoc's block smushing (code blocks and quotes coalesce).
    fn blocks(&self, parent: &Node, state: &mut State) -> Vec<Block> {
        let elems: Vec<&Node> = parent
            .elems()
            .filter(|n| n.name == "p" || n.name == "tbl")
            .collect();
        let mut segments: Vec<Vec<Block>> = Vec::new();
        let mut list: Vec<ListItem> = Vec::new();
        let mut i = 0;
        while i < elems.len() {
            let node = elems[i];
            let next = elems.get(i + 1).copied();
            // Caption before body, or body before caption (the latter only
            // when the caption paragraph does not keep with the next one).
            let pair = next.and_then(|next| {
                if self.is_caption_para(node) && self.is_captionable(next) {
                    Some((node, next))
                } else if self.is_captionable(node)
                    && self.is_caption_para(next)
                    && !keep_next(next)
                {
                    Some((next, node))
                } else {
                    None
                }
            });
            if let Some((caption, body)) = pair {
                Self::flush_list(&mut segments, &mut list);
                segments.push(self.captioned(caption, body, state));
                i += 2;
                continue;
            }
            match node.name.as_str() {
                "p" if is_horizontal_rule(node) => {
                    Self::flush_list(&mut segments, &mut list);
                    segments.push(vec![Block::HorizontalRule]);
                }
                "p" => {
                    if let Some(item) = self.list_item(node, state) {
                        list.push(item);
                    } else {
                        Self::flush_list(&mut segments, &mut list);
                        let mut segment: Vec<Block> =
                            self.paragraph(node, state).into_iter().collect();
                        // A bottom border draws a rule after the paragraph.
                        if has_bottom_border(node) {
                            segment.push(Block::HorizontalRule);
                        }
                        if !segment.is_empty() {
                            segments.push(segment);
                        }
                    }
                }
                _ => {
                    Self::flush_list(&mut segments, &mut list);
                    if let Some(table) = self.table(node, state) {
                        segments.push(vec![table]);
                    }
                }
            }
            i += 1;
        }
        Self::flush_list(&mut segments, &mut list);
        smush_blocks(segments)
    }

    fn flush_list(segments: &mut Vec<Vec<Block>>, list: &mut Vec<ListItem>) {
        if !list.is_empty() {
            let items = std::mem::take(list);
            segments.push(build_lists(&items));
        }
    }

    // --- captions ---

    /// Whether a paragraph's style (or one it is based on) is named
    /// "caption", "table caption" or "image caption", case-insensitively.
    fn is_caption_para(&self, node: &Node) -> bool {
        if node.name != "p" {
            return false;
        }
        // Walk the based-on chain, bounded so a cyclic chain cannot hang.
        let mut style: Option<&str> = Some(para_style(node));
        for _ in 0..16 {
            let Some((name, based_on)) = style.and_then(|id| self.styles.get(id)) else {
                return false;
            };
            if matches!(
                name.to_lowercase().as_str(),
                "caption" | "table caption" | "image caption"
            ) {
                return true;
            }
            style = based_on.as_deref();
        }
        false
    }

    /// Whether a body part can take a caption: a table, or a paragraph
    /// whose only content is an image.
    fn is_captionable(&self, node: &Node) -> bool {
        if node.name == "tbl" {
            return true;
        }
        node.name == "p" && {
            let mut state = State::default();
            matches!(
                self.inlines(node, &mut state).as_slice(),
                [Inline::Image(..)]
            )
        }
    }

    /// Attach a caption paragraph's content to a table or image paragraph.
    fn captioned(&self, caption: &Node, body: &Node, state: &mut State) -> Vec<Block> {
        let caption_blocks = self
            .paragraph(caption, state)
            .map_or_else(Vec::new, |b| vec![b]);
        let capt = Caption { short: None, blocks: caption_blocks.clone() };
        let body_blocks = if body.name == "tbl" {
            self.table(body, state).map_or_else(Vec::new, |b| vec![b])
        } else {
            self.paragraph(body, state).map_or_else(Vec::new, |b| vec![b])
        };
        match body_blocks.as_slice() {
            [Block::Table(table)] => {
                let mut table = table.clone();
                table.caption = capt;
                vec![Block::Table(table)]
            }
            [Block::Figure(attr, _, blocks)] => {
                vec![Block::Figure(attr.clone(), capt, blocks.clone())]
            }
            [Block::Para(inlines)] if matches!(inlines.as_slice(), [Inline::Image(..)]) => {
                vec![Block::Figure(
                    Attr::default(),
                    capt,
                    vec![Block::Plain(inlines.clone())],
                )]
            }
            _ => caption_blocks,
        }
    }

    // --- paragraphs ---

    /// A numbered, non-heading paragraph becomes a pending list item.
    fn list_item(&self, p: &Node, state: &mut State) -> Option<ListItem> {
        let style = para_style(p);
        if style.starts_with("Heading") {
            return None;
        }
        let num = p.child("pPr")?.child("numPr")?;
        let level_key = num.child("ilvl")?.attr("w:val")?;
        let level = level_key.parse::<usize>().ok()?;
        let num_id = num.child("numId")?.attr("w:val")?;
        let info = self.num_level(num_id, level);
        let level_start = info.as_ref().map_or(1, |i| i.start);
        let number = state.list_number(num_id, level, level_start);
        let kind = match info.as_ref() {
            Some(info) if info.format != "bullet" => {
                let style = match info.format.as_str() {
                    "lowerLetter" => ListNumberStyle::LowerAlpha,
                    "upperLetter" => ListNumberStyle::UpperAlpha,
                    "lowerRoman" => ListNumberStyle::LowerRoman,
                    "upperRoman" => ListNumberStyle::UpperRoman,
                    _ => ListNumberStyle::Decimal,
                };
                // The delimiter comes from the text around the number
                // placeholder: "%1." → Period, "%1)" → OneParen,
                // "(%1)" → TwoParens.
                let text = &info.text;
                let delim = if text.starts_with('(') && text.ends_with(')') {
                    ListNumberDelim::TwoParens
                } else if text.ends_with(')') {
                    ListNumberDelim::OneParen
                } else {
                    ListNumberDelim::Period
                };
                ListKind::Ordered(number, style, delim)
            }
            _ => ListKind::Bullet,
        };
        let continuation = info.is_some_and(|i| i.text.trim().is_empty());
        let block = self.styled_block(p, style, state)?;
        Some(ListItem { level, num_id: num_id.to_owned(), kind, continuation, block })
    }

    /// Map a non-list paragraph to a block by its style.
    fn paragraph(&self, p: &Node, state: &mut State) -> Option<Block> {
        let style = para_style(p);
        if let Some(level) = style
            .strip_prefix("Heading")
            .and_then(|d| d.parse::<i64>().ok())
        {
            // Headings are not trimmed (unlike paragraphs).
            let inlines = self.inlines(p, state);
            let id = state.ident(&inlines);
            return Some(Block::Header(
                level,
                Attr { identifier: id, ..Attr::default() },
                inlines,
            ));
        }
        self.styled_block(p, style, state)
    }

    /// A paragraph as a block per its style (everything but headings).
    fn styled_block(&self, p: &Node, style: &str, state: &mut State) -> Option<Block> {
        if style == "SourceCode" {
            return Some(Block::CodeBlock(Attr::default(), raw_text(p)));
        }
        let inlines = trim_paragraph(self.inlines(p, state));
        if inlines.is_empty() {
            return None;
        }
        if style == "BlockText" || style == "Quote" || style == "BlockQuote" {
            return Some(Block::BlockQuote(vec![Block::Para(inlines)]));
        }
        if style == "Compact" {
            return Some(Block::Plain(inlines));
        }
        Some(Block::Para(inlines))
    }

    // --- numbering ---

    /// The numbering level definition for (numId, level).
    fn num_level(&self, num_id: &str, level: usize) -> Option<LevelInfo> {
        let numbering = self.numbering.as_ref()?;
        let num = numbering
            .children_named("num")
            .find(|n| n.attr("w:numId") == Some(num_id))?;
        let abstract_id = num.child("abstractNumId")?.attr("w:val")?;
        let start_override = num
            .children_named("lvlOverride")
            .find(|o| o.attr("w:ilvl").and_then(|v| v.parse::<usize>().ok()) == Some(level))
            .and_then(|o| o.child("startOverride"))
            .and_then(|s| s.attr("w:val"))
            .and_then(|v| v.parse::<i64>().ok());
        let lvl = numbering
            .children_named("abstractNum")
            .find(|a| a.attr("w:abstractNumId") == Some(abstract_id))?
            .children_named("lvl")
            .find(|l| l.attr("w:ilvl").and_then(|v| v.parse::<usize>().ok()) == Some(level))?;
        let start = start_override.or_else(|| {
            lvl.child("start")
                .and_then(|s| s.attr("w:val"))
                .and_then(|v| v.parse().ok())
        });
        Some(LevelInfo {
            format: lvl.child("numFmt")?.attr("w:val")?.to_owned(),
            text: lvl
                .child("lvlText")
                .and_then(|t| t.attr("w:val"))
                .unwrap_or_default()
                .to_owned(),
            start: start.unwrap_or(1),
        })
    }

    // --- tables ---

    fn table(&self, tbl: &Node, state: &mut State) -> Option<Block> {
        let rows: Vec<RawRow> = tbl
            .children_named("tr")
            .map(|tr| self.table_row(tr, state))
            .collect();
        if rows.is_empty() {
            return None;
        }
        let grid = self.grid_widths(tbl);
        let first_row_formatting = tbl
            .child("tblPr")
            .and_then(|p| p.child("tblLook"))
            .is_some_and(table_look_first_row);
        let (head, body) = split_header_rows(first_row_formatting, &rows);

        // Column alignments come from the first body row, replicated over
        // each cell's grid span, and are zipped with the widths.
        let width = rows
            .iter()
            .map(|r| r.cells.iter().map(|c| c.grid_span).sum::<i64>())
            .max()
            .unwrap_or(0);
        let alignments: Vec<Alignment> = match body.first() {
            None => (0..width).map(|_| Alignment::AlignDefault).collect(),
            Some(row) => rows[*row]
                .cells
                .iter()
                .flat_map(|c| {
                    std::iter::repeat_n(c.alignment, usize::try_from(c.grid_span).unwrap_or(1))
                })
                .collect(),
        };
        let colspecs: Vec<ColSpec> = alignments
            .into_iter()
            .zip(grid)
            .map(|(alignment, width)| ColSpec {
                alignment,
                width: if width == 0.0 {
                    ColWidth::ColWidthDefault
                } else {
                    ColWidth::ColWidth(width)
                },
            })
            .collect();

        let caption = tbl
            .child("tblPr")
            .and_then(|p| p.child("tblCaption"))
            .and_then(|c| c.attr("w:val"))
            .filter(|c| !c.is_empty())
            .map_or_else(Caption::default, |text| {
                let mut inlines = Vec::new();
                text_tokens(text, &mut inlines);
                Caption {
                    short: Some(inlines.clone()),
                    blocks: vec![Block::Plain(inlines)],
                }
            });

        Some(Block::Table(Box::new(Table {
            attr: Attr::default(),
            caption,
            colspecs,
            head: TableHead {
                attr: Attr::default(),
                rows: resolve_rowspans(&rows, &head),
            },
            bodies: vec![TableBody {
                attr: Attr::default(),
                row_head_columns: 0,
                head: Vec::new(),
                body: resolve_rowspans(&rows, &body),
            }],
            foot: TableFoot::default(),
        })))
    }

    /// Column widths as fractions of the text width less inter-column
    /// space, normalized when they sum above 1.
    fn grid_widths(&self, tbl: &Node) -> Vec<f64> {
        let Some(grid) = tbl.child("tblGrid") else {
            return Vec::new();
        };
        let columns: Vec<f64> = grid
            .children_named("gridCol")
            .map(|c| {
                c.attr("w:w")
                    .and_then(|w| w.parse::<f64>().ok())
                    .unwrap_or(0.0)
            })
            .collect();
        #[allow(clippy::cast_precision_loss)]
        let total = self.text_width - 10.0 * (columns.len().saturating_sub(1)) as f64;
        let mut fractions: Vec<f64> = columns.iter().map(|w| w / total).collect();
        let sum: f64 = fractions.iter().sum();
        if sum > 1.0 {
            for f in &mut fractions {
                *f /= sum;
            }
        }
        fractions
    }

    fn table_row(&self, tr: &Node, state: &mut State) -> RawRow {
        let properties = tr.child("trPr");
        let header = properties
            .and_then(|p| p.child("tblHeader"))
            .is_some_and(|h| h.attr("w:val") != Some("0"));
        // `gridBefore` means the row starts with that many empty cells.
        let grid_before = properties
            .and_then(|p| p.child("gridBefore"))
            .and_then(|g| g.attr("w:val"))
            .and_then(|v| v.parse::<usize>().ok())
            .unwrap_or(0);
        let mut cells: Vec<RawCell> = (0..grid_before)
            .map(|_| RawCell {
                alignment: Alignment::AlignDefault,
                grid_span: 1,
                merged: false,
                blocks: Vec::new(),
            })
            .collect();
        for tc in tr.children_named("tc") {
            let properties = tc.child("tcPr");
            let grid_span = properties
                .and_then(|p| p.child("gridSpan"))
                .and_then(|g| g.attr("w:val"))
                .and_then(|v| v.parse::<i64>().ok())
                .unwrap_or(1);
            // A `vMerge` without `val="restart"` continues the merge above.
            let merged = properties
                .and_then(|p| p.child("vMerge"))
                .is_some_and(|v| v.attr("w:val") != Some("restart"));
            // Alignment comes from the first paragraph's justification.
            let alignment = tc
                .elems()
                .find(|n| n.name == "p")
                .and_then(|p| p.child("pPr"))
                .and_then(|pr| pr.child("jc"))
                .and_then(|j| j.attr("w:val"))
                .map_or(Alignment::AlignDefault, |j| match j {
                    "both" | "left" | "start" => Alignment::AlignLeft,
                    "right" | "end" => Alignment::AlignRight,
                    "center" => Alignment::AlignCenter,
                    _ => Alignment::AlignDefault,
                });
            cells.push(RawCell {
                alignment,
                grid_span,
                merged,
                blocks: single_para_to_plain(self.blocks(tc, state)),
            });
        }
        RawRow { header, cells }
    }

    // --- inlines ---

    /// The smushed inline content of a paragraph-like element.
    fn inlines(&self, parent: &Node, state: &mut State) -> Vec<Inline> {
        let mut sequences = Vec::new();
        self.inline_sequences(parent, &mut sequences, state);
        smush_inlines(sequences)
    }

    /// Collect one formatted inline sequence per paragraph part, pandoc's
    /// `parPartToInlines` granularity.
    fn inline_sequences(&self, parent: &Node, out: &mut Vec<Vec<Inline>>, state: &mut State) {
        for node in parent.elems() {
            match node.name.as_str() {
                "r" => {
                    let seq = self.run(node, state);
                    if !seq.is_empty() {
                        out.push(seq);
                    }
                }
                "hyperlink" => {
                    let url = node
                        .attr("r:id")
                        .and_then(|id| self.rels.get(id).cloned())
                        .or_else(|| node.attr("w:anchor").map(|a| format!("#{a}")))
                        .unwrap_or_default();
                    let mut inner = Vec::new();
                    self.inline_sequences(node, &mut inner, state);
                    out.push(vec![Inline::Link(
                        Attr::default(),
                        smush_inlines(inner),
                        Target { url, title: String::new() },
                    )]);
                }
                // Minimal OMML support: concatenated math-run text.
                "oMath" => out.push(vec![Inline::Math(MathType::InlineMath, omml_text(node))]),
                "ins" | "smartTag" | "sdt" | "sdtContent" => {
                    self.inline_sequences(node, out, state);
                }
                _ => {}
            }
        }
    }

    /// A single run as a formatted inline sequence.
    fn run(&self, r: &Node, state: &mut State) -> Vec<Inline> {
        let rpr = r.child("rPr");
        let rstyle = rpr
            .and_then(|p| p.child("rStyle"))
            .and_then(|s| s.attr("w:val"))
            .unwrap_or("");
        if rstyle == "VerbatimChar" {
            // Inline code: raw text, spaces preserved, no tokenization.
            let text = raw_run_text(r);
            if text.is_empty() {
                return Vec::new();
            }
            return vec![Inline::Code(Attr::default(), text)];
        }

        let mut tokens = Vec::new();
        for node in r.elems() {
            match node.name.as_str() {
                "t" => text_tokens(&node.text(), &mut tokens),
                "br" => tokens.push(Inline::LineBreak),
                "drawing" => {
                    if let Some(img) = self.image(node) {
                        tokens.push(img);
                    }
                }
                "footnoteReference" => {
                    if let Some(note) = self.footnote(node, state) {
                        tokens.push(note);
                    }
                }
                _ => {}
            }
        }
        if tokens.is_empty() {
            return Vec::new();
        }
        let flag = |name: &str| {
            rpr.and_then(|p| p.child(name))
                .is_some_and(|n| !matches!(n.attr("w:val"), Some("false" | "0" | "none")))
        };
        let vert = rpr
            .and_then(|p| p.child("vertAlign"))
            .and_then(|v| v.attr("w:val"));
        // Modifier stack, outermost first (a bold+italic run is
        // Emph[Strong[…]] in pandoc's output).
        let mut modifiers = Vec::new();
        if flag("i") {
            modifiers.push(Modifier::Emph);
        }
        if flag("b") {
            modifiers.push(Modifier::Strong);
        }
        if flag("smallCaps") {
            modifiers.push(Modifier::SmallCaps);
        }
        if flag("strike") {
            modifiers.push(Modifier::Strikeout);
        }
        if flag("u") {
            modifiers.push(Modifier::Underline);
        }
        if vert == Some("superscript") {
            modifiers.push(Modifier::Superscript);
        }
        if vert == Some("subscript") {
            modifiers.push(Modifier::Subscript);
        }
        stack(&modifiers, tokens)
    }

    fn image(&self, drawing: &Node) -> Option<Inline> {
        let blip = find_descendant(drawing, "blip")?;
        let target = self.rels.get(blip.attr("r:embed")?)?;
        let docpr = find_descendant(drawing, "docPr");
        let alt = docpr.and_then(|d| d.attr("descr")).unwrap_or_default();
        let title = docpr.and_then(|d| d.attr("title")).unwrap_or_default();
        let mut attributes = Vec::new();
        if let Some(extent) = find_descendant(drawing, "extent") {
            for (axis, name) in [("cx", "width"), ("cy", "height")] {
                if let Some(emu) = extent.attr(axis).and_then(|v| v.parse::<f64>().ok()) {
                    attributes
                        .push((name.to_owned(), format!("{}in", show_double(emu / 914_400.0))));
                }
            }
        }
        let mut alt_tokens = Vec::new();
        text_tokens(alt, &mut alt_tokens);
        Some(Inline::Image(
            Attr { attributes, ..Attr::default() },
            alt_tokens,
            Target { url: target.clone(), title: title.to_owned() },
        ))
    }

    fn footnote(&self, reference: &Node, state: &mut State) -> Option<Inline> {
        let id = reference.attr("w:id")?;
        let footnotes = self.footnotes.as_ref()?;
        let note = footnotes
            .children_named("footnote")
            .find(|f| f.attr("w:id") == Some(id))?;
        Some(Inline::Note(self.blocks(note, state)))
    }
}

/// A numbering level's format, marker text, and start number.
struct LevelInfo {
    format: String,
    text: String,
    start: i64,
}

/// Rebuild a run of numbered paragraphs into sibling lists, pandoc's
/// `flatToBullets`: each list spans consecutive items that are deeper than
/// its first item or share its (level, `numId`); within a list,
/// continuation items (blank marker text) extend the previous item, and
/// deeper runs recurse into it.
fn build_lists(items: &[ListItem]) -> Vec<Block> {
    let mut lists = Vec::new();
    let mut i = 0;
    while i < items.len() {
        let first = &items[i];
        let (level, num_id) = (first.level, first.num_id.clone());
        let end = items[i..]
            .iter()
            .position(|it| {
                !(it.level > level || (it.level == level && it.num_id == num_id))
            })
            .map_or(items.len(), |p| i + p);
        let children = &items[i..end];

        let mut list_items: Vec<Vec<Block>> = Vec::new();
        let mut j = 0;
        while j < children.len() {
            let item = &children[j];
            if item.level == level {
                if item.continuation
                    && let Some(last) = list_items.last_mut()
                {
                    last.push(item.block.clone());
                } else {
                    list_items.push(vec![item.block.clone()]);
                }
                j += 1;
            } else {
                let sub_end = children[j..]
                    .iter()
                    .position(|it| it.level == level)
                    .map_or(children.len(), |p| j + p);
                let sub = build_lists(&children[j..sub_end]);
                if list_items.is_empty() {
                    list_items.push(Vec::new());
                }
                list_items.last_mut().expect("non-empty").extend(sub);
                j = sub_end;
            }
        }
        lists.push(single_item_header(make_list(&first.kind, list_items)));
        i = end;
    }
    lists
}

fn make_list(kind: &ListKind, items: Vec<Vec<Block>>) -> Block {
    match kind {
        ListKind::Bullet => Block::BulletList(items),
        ListKind::Ordered(start, style, delim) => Block::OrderedList(
            ListAttributes { start: *start, style: *style, delim: *delim },
            items,
        ),
    }
}

/// A single-item ordered list whose item is just a header is that header.
fn single_item_header(block: Block) -> Block {
    if let Block::OrderedList(_, items) = &block
        && let [item] = items.as_slice()
        && let [header @ Block::Header(..)] = item.as_slice()
    {
        return header.clone();
    }
    block
}

/// A cell whose content is a single paragraph holds a `Plain` instead.
fn single_para_to_plain(blocks: Vec<Block>) -> Vec<Block> {
    if let [Block::Para(inlines)] = blocks.as_slice() {
        return vec![Block::Plain(inlines.clone())];
    }
    blocks
}

/// Whether a `w:tblLook` marks the first row as specially formatted.
fn table_look_first_row(look: &Node) -> bool {
    match look.attr("w:firstRow") {
        Some("1") => true,
        Some(_) => false,
        None => look
            .attr("w:val")
            .and_then(|v| i64::from_str_radix(v, 16).ok())
            .is_some_and(|mask| mask & 0x020 != 0),
    }
}

/// Split rows into header and body indices: the first row when the table
/// look says so, then any row explicitly marked as a header — plus rows
/// that continue a header row's vertical merge.
fn split_header_rows(first_row_formatting: bool, rows: &[RawRow]) -> (Vec<usize>, Vec<usize>) {
    let mut head = Vec::new();
    let mut body = Vec::new();
    let mut previous_was_header = first_row_formatting;
    let mut start = 0;
    if first_row_formatting && !rows.is_empty() {
        head.push(0);
        start = 1;
    }
    for (i, row) in rows.iter().enumerate().skip(start) {
        if row.header || (previous_was_header && row.cells.iter().any(|c| c.merged)) {
            head.push(i);
            previous_was_header = true;
        } else {
            body.push(i);
            previous_was_header = false;
        }
    }
    (head, body)
}

/// Resolve vertical merges into row spans over the given row indices,
/// dropping the continuation cells (pandoc's `rowsToRowspans`).
fn resolve_rowspans(rows: &[RawRow], indices: &[usize]) -> Vec<Row> {
    // Walk bottom-up so each row can see the spans accumulated below it.
    let mut spans: Vec<Vec<i64>> = Vec::with_capacity(indices.len());
    for (position, &index) in indices.iter().enumerate().rev() {
        let cells = &rows[index].cells;
        let below = indices.get(position + 1).map(|&below| &rows[below].cells);
        spans.insert(0, row_spans(cells, below, spans.first()));
    }
    indices
        .iter()
        .zip(&spans)
        .map(|(&index, spans)| Row {
            attr: Attr::default(),
            cells: rows[index]
                .cells
                .iter()
                .zip(spans)
                .filter(|(cell, _)| !cell.merged)
                .map(|(cell, &row_span)| Cell {
                    attr: Attr::default(),
                    alignment: cell.alignment,
                    row_span,
                    col_span: cell.grid_span,
                    blocks: cell.blocks.clone(),
                })
                .collect(),
        })
        .collect()
}

/// The row span of each cell in `cells`, given the row below and the spans
/// already computed for it: a cell grows by the span of the cell beneath it
/// when that cell continues a vertical merge.
fn row_spans(cells: &[RawCell], below: Option<&Vec<RawCell>>, below_spans: Option<&Vec<i64>>) -> Vec<i64> {
    let (Some(below), Some(below_spans)) = (below, below_spans) else {
        return cells.iter().map(|_| 1).collect();
    };
    let mut out = Vec::with_capacity(cells.len());
    let mut cursor = 0usize;
    let mut columns_left: Option<i64> = None;
    for cell in cells {
        let Some(below_cell) = below.get(cursor) else {
            out.push(1);
            continue;
        };
        let span = if below_cell.merged {
            1 + below_spans.get(cursor).copied().unwrap_or(0)
        } else {
            1
        };
        out.push(span);
        // Advance through the row below by this cell's width, accounting
        // for a partially consumed cell beneath.
        let to_drop =
            cell.grid_span + (below_cell.grid_span - columns_left.unwrap_or(below_cell.grid_span));
        let (left, next) = drop_columns(to_drop, below, cursor);
        columns_left = Some(left);
        cursor = next;
    }
    out
}

/// Skip `n` columns through `cells` starting at `from`, returning how much
/// of the landing cell is left and the new index.
fn drop_columns(mut n: i64, cells: &[RawCell], from: usize) -> (i64, usize) {
    let mut i = from;
    loop {
        let Some(cell) = cells.get(i) else {
            return (n, i);
        };
        if n < cell.grid_span {
            return (cell.grid_span - n, i);
        }
        n -= cell.grid_span;
        i += 1;
    }
}

/// The paragraph's style id (`w:pPr/w:pStyle/@w:val`), or "".
fn para_style(p: &Node) -> &str {
    p.child("pPr")
        .and_then(|pr| pr.child("pStyle"))
        .and_then(|s| s.attr("w:val"))
        .unwrap_or("")
}

/// Whether a paragraph is marked to keep with the following one.
fn keep_next(p: &Node) -> bool {
    p.child("pPr").and_then(|pr| pr.child("keepNext")).is_some()
}

fn find_descendant<'a>(node: &'a Node, name: &str) -> Option<&'a Node> {
    for child in node.elems() {
        if child.name == name {
            return Some(child);
        }
        if let Some(found) = find_descendant(child, name) {
            return Some(found);
        }
    }
    None
}

/// All math-run text of an OMML island, concatenated.
fn omml_text(node: &Node) -> String {
    fn walk(out: &mut String, node: &Node) {
        for child in node.elems() {
            if child.name == "t" {
                out.push_str(&child.text());
            } else {
                walk(out, child);
            }
        }
    }
    let mut out = String::new();
    walk(&mut out, node);
    out
}

/// The literal text of a paragraph (used for code blocks): run text
/// concatenated, breaks as newlines.
fn raw_text(p: &Node) -> String {
    let mut out = String::new();
    for r in p.children_named("r") {
        out.push_str(&raw_run_text(r));
    }
    out
}

/// The literal text of a single run, breaks as newlines.
fn raw_run_text(r: &Node) -> String {
    let mut out = String::new();
    for node in r.elems() {
        match node.name.as_str() {
            "t" => out.push_str(&node.text()),
            "br" => out.push('\n'),
            _ => {}
        }
    }
    out
}

/// Format a double the way Haskell's `show` does (pandoc emits image
/// dimensions this way): decimal in `[0.1, 1e7)`, scientific otherwise.
fn show_double(x: f64) -> String {
    let magnitude = x.abs();
    if magnitude == 0.0 || (0.1..1e7).contains(&magnitude) {
        format!("{x:?}")
    } else {
        format!("{x:e}")
    }
}

/// Tokenize run text the way pandoc's `text` builder does: runs of
/// whitespace collapse to one token — `SoftBreak` if the run contains a
/// newline, otherwise `Space` — and everything else becomes `Str`.
fn text_tokens(text: &str, out: &mut Vec<Inline>) {
    let is_space = |c: char| matches!(c, ' ' | '\t' | '\r' | '\n');
    let mut word = String::new();
    let mut spaces = String::new();
    for ch in text.chars() {
        if is_space(ch) {
            if !word.is_empty() {
                out.push(Inline::Str(std::mem::take(&mut word)));
            }
            spaces.push(ch);
        } else {
            if !spaces.is_empty() {
                out.push(whitespace_token(&spaces));
                spaces.clear();
            }
            word.push(ch);
        }
    }
    if !word.is_empty() {
        out.push(Inline::Str(word));
    } else if !spaces.is_empty() {
        out.push(whitespace_token(&spaces));
    }
}

fn whitespace_token(spaces: &str) -> Inline {
    if spaces.contains('\n') {
        Inline::SoftBreak
    } else {
        Inline::Space
    }
}

/// A paragraph that is nothing but a bottom border (or a pandoc-written
/// `v:rect` picture) is a horizontal rule.
fn is_horizontal_rule(p: &Node) -> bool {
    let only_child = |node: &Node, name: &str| -> bool {
        let children: Vec<&Node> = node.elems().collect();
        matches!(children.as_slice(), [only] if only.name == name)
    };
    let children: Vec<&Node> = p.elems().collect();
    match children.as_slice() {
        [ppr] if ppr.name == "pPr" => {
            only_child(ppr, "pBdr")
                && ppr.child("pBdr").is_some_and(|b| only_child(b, "bottom"))
        }
        [r] if r.name == "r" => {
            only_child(r, "pict")
                && r.child("pict").is_some_and(|p| only_child(p, "rect"))
        }
        _ => false,
    }
}

/// Whether a paragraph's only border is a bottom one, which pandoc renders
/// as a horizontal rule after the paragraph.
fn has_bottom_border(p: &Node) -> bool {
    p.child("pPr")
        .and_then(|pr| pr.child("pBdr"))
        .is_some_and(|border| {
            let children: Vec<&Node> = border.elems().collect();
            matches!(children.as_slice(), [only] if only.name == "bottom")
        })
}

/// Pandoc's `trimSps`: drop spaces, soft breaks *and* line breaks from both
/// ends of a paragraph's content.
fn trim_paragraph(mut inlines: Vec<Inline>) -> Vec<Inline> {
    let is_space =
        |i: &Inline| matches!(i, Inline::Space | Inline::SoftBreak | Inline::LineBreak);
    while inlines.first().is_some_and(is_space) {
        inlines.remove(0);
    }
    while inlines.last().is_some_and(is_space) {
        inlines.pop();
    }
    inlines
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn haskell_show_double_matches_pandoc() {
        assert_eq!(show_double(0.5), "0.5");
        assert_eq!(show_double(1.388_888_888_888_888_8e-2), "1.3888888888888888e-2");
    }

    #[test]
    fn auto_identifiers_follow_pandoc_rules() {
        let mut state = State::default();
        let ils = |s: &str| vec![Inline::Str(s.to_owned())];
        assert_eq!(state.ident(&ils("Heading One")), "heading-one");
        assert_eq!(state.ident(&ils("Heading One")), "heading-one-1");
        assert_eq!(state.ident(&ils("123 start")), "start");
        assert_eq!(state.ident(&ils("!!!")), "section");
    }

    #[test]
    fn list_numbering_continues_per_instance() {
        let mut state = State::default();
        assert_eq!(state.list_number("1", 0, 1), 1);
        assert_eq!(state.list_number("1", 0, 1), 2);
        // A different numbering instance starts fresh at its own start.
        assert_eq!(state.list_number("2", 0, 5), 5);
        // The first instance keeps counting where it left off.
        assert_eq!(state.list_number("1", 0, 1), 3);
    }
}
