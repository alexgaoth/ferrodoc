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
//!   image-only paragraph, on either side;
//! - styles are matched by *name* through `styles.xml` and its `basedOn`
//!   chain, never by style id, so localized ids work and a document
//!   without `styles.xml` yields plain paragraphs;
//! - rows are laid out on a grid as wide as the column specs: spans that
//!   overshoot are clipped and rows with free columns are padded;
//! - leading paragraphs styled Title/Subtitle/Author/Date/Abstract become
//!   document metadata rather than blocks;
//! - lists are *not* rebuilt inside footnotes, only in the body and in
//!   table cells;
//! - bookmarks become empty anchor spans, but an anchor nothing links to
//!   is dropped again at the end of the conversion; anchors take part in
//!   identifier uniquing (so a later heading auto-id cannot collide with
//!   one), consecutive bookmarks alias to the first, and internal links
//!   are rewritten to the identifiers actually emitted;
//! - style *names* are compared ignoring case but not whitespace, so a
//!   concept pandoc spells two ways ("Source Code" and `SourceCode`) lists
//!   both; a heading is exactly "heading <n>" with one space, and a
//!   heading's own style name — not its ancestors' — becomes a class;
//! - a caption style must be the paragraph's *own* style; deriving a style
//!   from Caption does not make it caption;
//! - an ordered list takes a delimiter only from the exact marker shapes
//!   `%N.`, `%N)` and `(%N)`, and a numbering format outside the five
//!   pandoc knows is `DefaultStyle`;
//! - a numbered paragraph stays a list item even when its style would
//!   otherwise caption or open a definition entry;
//! - `w:sdt` content controls (Word's tables of contents, cover pages and
//!   citation fields) are unwrapped rather than skipped;
//! - a hyperlink resolves to `target#fragment`, to `target`, to `#fragment`
//!   or — when its relationship is unresolvable — to the empty string; one
//!   with no target at all is dropped, text included;
//! - inline code is the character style whose own name is "Verbatim Char",
//!   and a code run takes only its vertical alignment, not the rest of its
//!   run formatting;
//! - an indented, unnumbered paragraph is a block quote, as are the
//!   "Block Text", "Quote", "Block Quotation" and "Intense Quote" styles;
//! - a run's `w:highlight` becomes a "mark" span, `w:tab` a space (a tab
//!   inside code), and endnotes are read like footnotes.
//!
//! Milestone scope (Phase 2, reader core): paragraphs and formatting runs,
//! headings, hyperlinks, inline code, lists, definition lists, tables
//! (including `gridSpan`/`vMerge` spans and header rows), images, figures,
//! footnotes, metadata, bookmarks, line breaks, and horizontal rules.
//!
//! Known gaps, deliberate and unfixed:
//!
//! - comments, tracked changes, fields, text boxes and custom style maps
//!   are skipped entirely;
//! - `w:sym` yields the raw character, without pandoc's Symbol/Wingdings
//!   font mapping (so `F0D2` stays a private-use character instead of
//!   becoming `◊`);
//! - OMML math is read as its concatenated run text, not translated to
//!   TeX, so `a^{2}` reads back as `a2`;
//! - an empty list item followed by deeper items attaches the nested list
//!   to the empty item instead of pandoc's separate item
//!   (`corpus/docx/spec-09.docx`, the one corpus document that differs);
//! - paragraphs whose style is a near-miss for a known one ("Definition
//!   Term" spelled `DefinitionTerm`, say) become plain paragraphs, where
//!   pandoc emits custom-style divs — the same out-of-scope mechanism;
//! - XML attribute values are normalized per the XML specification, so a
//!   style name containing a tab compares equal to the spaced spelling
//!   where pandoc treats them as different styles;
//! - conversion is bounded: XML deeper than 256 elements is rejected, and
//!   container nesting beyond 64 levels (or a self-referential footnote)
//!   drops the remaining content rather than recursing without limit.

mod combine;
mod media;
mod write;
mod xml;

pub use write::{write_docx, write_docx_with_media};

use combine::{Modifier, smush_blocks, smush_inlines, stack};
use ferrodoc_ast::{
    Alignment, Attr, Block, Caption, Cell, ColSpec, ColWidth, Inline, ListAttributes,
    ListNumberDelim, ListNumberStyle, MathType, MetaValue, Pandoc, Row, Table, TableBody, TableFoot,
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

/// The image bytes a document carries, keyed by the URL its AST refers to
/// them by — exactly the string [`write_docx_with_media`]'s resolver is
/// asked for, so a bag from one document feeds straight into writing
/// another.
pub type Media = HashMap<String, Vec<u8>>;

/// Read a DOCX document into a [`Pandoc`] AST equivalent to pandoc's docx
/// reader output.
///
/// The AST names each image by its part path but does not carry the bytes;
/// use [`read_docx_with_media`] when the images have to survive.
pub fn read_docx(bytes: &[u8]) -> Result<Pandoc, Error> {
    read(bytes, false).map(|(doc, _)| doc)
}

/// Read a DOCX document together with the bytes of every image it embeds.
///
/// Without this a `docx → docx` conversion silently loses its pictures:
/// the AST records where an image *was*, and the package it came from is
/// the only thing that knows what it held.
///
/// # Errors
///
/// The same as [`read_docx`]. A referenced part that is missing from the
/// archive is left out of the bag rather than failing the read — the
/// document is still readable, and the writer falls back to alt text.
pub fn read_docx_with_media(bytes: &[u8]) -> Result<(Pandoc, Media), Error> {
    read(bytes, true)
}

fn read(bytes: &[u8], want_media: bool) -> Result<(Pandoc, Media), Error> {
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
    let footnote_rels = part("word/_rels/footnotes.xml.rels");
    let endnotes = part("word/endnotes.xml");
    let endnote_rels = part("word/_rels/endnotes.xml.rels");
    let styles = part("word/styles.xml");

    // The body is streamed, not parsed into a tree: see `xml::body_children`.
    // Its section properties are the last thing in it and the conversion
    // needs them first, so that one element is parsed on its own.
    let section = body_section(&document).map(|s| xml::parse(&s)).transpose()?;
    let rels = rels.as_deref().map(xml::parse).transpose()?;
    let footnote_rels = footnote_rels.as_deref().map(xml::parse).transpose()?;
    let endnote_rels = endnote_rels.as_deref().map(xml::parse).transpose()?;
    // Every part's images, because a picture inside a note is declared by
    // that note's relationship table, not the document's.
    let images: HashSet<String> = [&rels, &footnote_rels, &endnote_rels]
        .into_iter()
        .flatten()
        .flat_map(image_targets)
        .collect();
    let ctx = Ctx {
        numbering: numbering.as_deref().map(xml::parse).transpose()?,
        rels: rels.as_ref().map(parse_rels).unwrap_or_default(),
        footnote_rels: footnote_rels.as_ref().map(parse_rels).unwrap_or_default(),
        endnote_rels: endnote_rels.as_ref().map(parse_rels).unwrap_or_default(),
        footnotes: footnotes.as_deref().map(xml::parse).transpose()?,
        endnotes: endnotes.as_deref().map(xml::parse).transpose()?,
        styles: styles
            .as_deref()
            .map(xml::parse)
            .transpose()?
            .as_ref()
            .map(parse_styles)
            .unwrap_or_default(),
        text_width: text_width(section.as_ref()),
    };
    let mut state = State::default();
    // A malformed part stops the stream and is reported, never returned
    // as a truncated document.
    let mut failure: Option<Error> = None;
    let body = xml::body_children(&document)?;
    let doc = {
        let failure = &mut failure;
        let parts = body
            .map_while(move |child| match child {
                Ok(node) => Some(node),
                Err(e) => {
                    *failure = Some(e);
                    None
                }
            })
            .flat_map(|node| {
                let mut parts = Vec::new();
                collect_parts(node, &mut parts, 0);
                parts
            });
        ctx.document(parts, &mut state)
    };
    if let Some(failure) = failure {
        return Err(failure);
    }

    let mut media = Media::new();
    if want_media {
        // Driven by the AST rather than by the relationship table: this
        // collects exactly the parts the writer will ask for, and cannot
        // pick up a header's logo or an embedded spreadsheet.
        let mut urls = Vec::new();
        collect_image_urls(&doc.blocks, &mut urls);
        for url in urls {
            if media.contains_key(&url) || !images.contains(&url) {
                continue;
            }
            // A part named but missing is not an error: the document is
            // still readable and the writer falls back to alt text.
            if let Ok(mut file) = archive.by_name(&part_path(&url)) {
                let mut bytes = Vec::new();
                if file.read_to_end(&mut bytes).is_ok() {
                    media.insert(url, bytes);
                }
            }
        }
    }
    Ok((doc, media))
}

/// The archive entry a relationship target names.
///
/// OPC allows three spellings and Word writes all of them: relative to the
/// declaring part (`media/x.png`, and `word/document.xml` is that part),
/// package-absolute (`/word/media/x.png`), and relative with `..`
/// (`../word/media/x.png`). Reserved characters are percent-encoded in the
/// target and literal in the entry name. Getting any of these wrong loses
/// a picture silently, which is the whole defect this exists to close.
fn part_path(url: &str) -> String {
    let (mut segments, rest) = match url.strip_prefix('/') {
        Some(absolute) => (Vec::new(), absolute),
        None => (vec!["word".to_owned()], url),
    };
    // Decoded first, then resolved: an escape that spells a separator or a
    // dot segment is one, and doing it the other way round leaves a path
    // that matches no entry.
    for segment in rest.split('/').map(percent_decode) {
        for segment in segment.split('/') {
            match segment {
                "" | "." => {}
                ".." => {
                    segments.pop();
                }
                segment => segments.push(segment.to_owned()),
            }
        }
    }
    segments.join("/")
}

/// Decode `%XX` escapes, leaving anything malformed as written.
fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let hex = |b: u8| (b as char).to_digit(16);
        if bytes[i] == b'%'
            && let (Some(hi), Some(lo)) = (
                bytes.get(i + 1).copied().and_then(hex),
                bytes.get(i + 2).copied().and_then(hex),
            )
        {
            out.push(u8::try_from(hi * 16 + lo).unwrap_or(b'?'));
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).unwrap_or_else(|_| text.to_owned())
}

/// Every image URL in the document. The order is unspecified — callers
/// key by the URL, not by position. Iterative: a document nests as deeply
/// as its containers do.
fn collect_image_urls(blocks: &[Block], out: &mut Vec<String>) {
    let mut blocks: Vec<&Block> = blocks.iter().rev().collect();
    let mut inlines: Vec<&Inline> = Vec::new();
    while let Some(block) = blocks.pop() {
        match block {
            Block::Plain(is) | Block::Para(is) | Block::Header(_, _, is) => {
                inlines.extend(is.iter().rev());
            }
            Block::LineBlock(lines) => inlines.extend(lines.iter().flatten().rev()),
            Block::BlockQuote(bs) | Block::Div(_, bs) => blocks.extend(bs.iter().rev()),
            Block::BulletList(items) | Block::OrderedList(_, items) => {
                blocks.extend(items.iter().flatten().rev());
            }
            Block::DefinitionList(items) => {
                for (term, definitions) in items {
                    inlines.extend(term.iter().rev());
                    blocks.extend(definitions.iter().flatten().rev());
                }
            }
            Block::Figure(_, caption, bs) => {
                blocks.extend(caption.blocks.iter().rev());
                blocks.extend(bs.iter().rev());
            }
            Block::Table(table) => {
                blocks.extend(table.caption.blocks.iter().rev());
                for row in table_rows(table) {
                    blocks.extend(row.cells.iter().flat_map(|c| c.blocks.iter()).rev());
                }
            }
            Block::CodeBlock(..) | Block::RawBlock(..) | Block::HorizontalRule => {}
        }
        while let Some(inline) = inlines.pop() {
            match inline {
                Inline::Image(_, alt, target) => {
                    out.push(target.url.clone());
                    inlines.extend(alt.iter().rev());
                }
                Inline::Emph(is)
                | Inline::Strong(is)
                | Inline::Strikeout(is)
                | Inline::Superscript(is)
                | Inline::Subscript(is)
                | Inline::SmallCaps(is)
                | Inline::Underline(is)
                | Inline::Span(_, is)
                | Inline::Quoted(_, is)
                | Inline::Cite(_, is)
                | Inline::Link(_, is, _) => inlines.extend(is.iter().rev()),
                Inline::Note(bs) => blocks.extend(bs.iter().rev()),
                _ => {}
            }
        }
    }
}

/// Every row of a table, head, bodies and foot alike.
fn table_rows(table: &Table) -> impl Iterator<Item = &Row> {
    table
        .head
        .rows
        .iter()
        .chain(table.bodies.iter().flat_map(|b| b.head.iter().chain(&b.body)))
        .chain(&table.foot.rows)
}

fn parse_rels(rels: &Node) -> HashMap<String, String> {
    rels.children_named("Relationship")
        .filter_map(|r| Some((r.attr("Id")?.to_owned(), r.attr("Target")?.to_owned())))
        .collect()
}

/// The targets of the relationships that declare themselves images.
///
/// The bag is restricted to these, so a document whose picture reference
/// resolves to something else — a crafted package can point a `blip` at
/// `comments.xml` — cannot make the reader load an arbitrary part.
fn image_targets(rels: &Node) -> HashSet<String> {
    rels.children_named("Relationship")
        .filter(|r| r.attr("Type").is_some_and(|t| t.ends_with("/image")))
        .filter_map(|r| r.attr("Target").map(str::to_owned))
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
/// The body's own `<w:sectPr>`, as a standalone document.
///
/// It is the body's last child and the conversion needs it before the
/// first, which is the one thing a forward stream cannot supply. Taking
/// the last one in the source finds it: a `sectPr` inside a `w:pPr` is a
/// section *break*, and every one of those precedes the body's own.
fn body_section(xml: &str) -> Option<String> {
    let open = xml.rfind("<w:sectPr")?;
    let rest = &xml[open..];
    // Either spelling: `<w:sectPr/>` or `<w:sectPr …>…</w:sectPr>`.
    let end = rest.find("</w:sectPr>").map_or_else(
        || rest.find("/>").map(|i| i + 2),
        |i| Some(i + "</w:sectPr>".len()),
    )?;
    Some(rest[..end].to_owned())
}

/// The `p` and `tbl` parts a body child contributes, unwrapping the
/// `w:sdt` content controls that may nest them.
fn collect_parts(node: Node, out: &mut Vec<Node>, depth: usize) {
    if depth >= MAX_NESTING {
        return;
    }
    match node.name.as_str() {
        "p" | "tbl" => out.push(node),
        "sdt" => {
            for child in node.children {
                if let xml::Child::Elem(content) = child
                    && content.name == "sdtContent"
                {
                    for inner in content.children {
                        if let xml::Child::Elem(inner) = inner {
                            collect_parts(inner, out, depth + 1);
                        }
                    }
                }
            }
        }
        _ => {}
    }
}

fn text_width(sect: Option<&Node>) -> f64 {
    let value = |node: Option<&Node>, attr: &str| -> Option<f64> {
        node?.attr(attr)?.parse().ok()
    };
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
    /// The next suffix to try for a base identifier already handed out.
    next_suffix: HashMap<String, u32>,
    /// Last number used per (`w:numId`, level).
    list_numbers: HashMap<(String, usize), i64>,
    /// Bookmark name to the identifier actually emitted for it.
    anchors: HashMap<String, String>,
    /// Container nesting depth, to bound recursion on hostile input.
    depth: usize,
    /// Footnote ids currently being expanded, to break reference cycles.
    open_notes: HashSet<String>,
    /// Which part the runs being read belong to. A relationship id is
    /// resolved against the rels of the part that *uses* it, and a
    /// Word-written package numbers each part's relationships from one —
    /// so the same `rId5` names a different picture in each.
    part: Part,
}

/// The part a run belongs to, for resolving its relationship ids.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
enum Part {
    #[default]
    Document,
    Footnotes,
    Endnotes,
}

/// The deepest container nesting converted. Real documents nest a handful
/// of levels; beyond this the input is hostile and the remaining content
/// is dropped rather than overflowing the stack.
const MAX_NESTING: usize = 64;

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
        self.ident_from(&id)
    }

    /// Reserve an explicit identifier (a heading's own bookmark) so a later
    /// auto-generated one cannot collide with it.
    fn claim_ident(&mut self, id: String) -> String {
        self.used_idents.insert(id.clone());
        id
    }

    /// Make `name` unique among the identifiers already used, suffixing
    /// `-1`, `-2`, … like pandoc.
    ///
    /// The search resumes from the last suffix this base name reached
    /// instead of restarting at zero. Restarting is quadratic in the
    /// number of headings that share a name — and headings sharing a name
    /// ("Summary", "Notes") is what an ordinary sectioned document looks
    /// like, so a 1 MB file of them took 72 seconds. Resuming is correct
    /// because identifiers are only ever taken, never released, so every
    /// suffix below the mark is already gone.
    fn ident_from(&mut self, name: &str) -> String {
        let mut n = self.next_suffix.get(name).copied().unwrap_or(0);
        let mut unique = if n == 0 { name.to_owned() } else { format!("{name}-{n}") };
        while !self.used_idents.insert(unique.clone()) {
            n += 1;
            unique = format!("{name}-{n}");
        }
        self.next_suffix.insert(name.to_owned(), n + 1);
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
    footnote_rels: HashMap<String, String>,
    endnote_rels: HashMap<String, String>,
    footnotes: Option<Node>,
    endnotes: Option<Node>,
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

/// Whether an inline is an empty anchor span (a bookmark).
fn is_anchor_span(inline: &Inline) -> bool {
    matches!(inline, Inline::Span(attr, content)
        if content.is_empty() && attr.classes.iter().any(|c| c == "anchor"))
}

/// Collect the identifiers that internal links point at.
fn collect_link_targets(blocks: &[Block], out: &mut HashSet<String>) {
    walk_inlines(blocks, &mut |inline| {
        if let Inline::Link(_, _, target) = inline
            && let Some(anchor) = target.url.strip_prefix('#')
        {
            out.insert(anchor.to_owned());
        }
    });
}

/// Point internal links at the identifiers the bookmarks actually got.
fn rewrite_internal_links(blocks: &mut [Block], anchors: &HashMap<String, String>) {
    map_inline_lists(blocks, &mut |inlines| {
        for inline in inlines.iter_mut() {
            if let Inline::Link(_, _, target) = inline
                && let Some(name) = target.url.strip_prefix('#')
                && let Some(id) = anchors.get(name)
            {
                target.url = format!("#{id}");
            }
        }
    });
}

/// Drop anchor spans nothing links to.
fn remove_orphan_anchors(blocks: &mut [Block], targets: &HashSet<String>) {
    map_inline_lists(blocks, &mut |inlines| {
        inlines.retain(|inline| match inline {
            Inline::Span(attr, content) if content.is_empty() && attr.classes.iter().any(|c| c == "anchor") => {
                targets.contains(&attr.identifier)
            }
            _ => true,
        });
    });
}

/// Split off a heading's first anchor span: pandoc uses its name as the
/// heading identifier and drops the span from the heading's content.
fn take_anchor(inlines: Vec<Inline>) -> (Option<String>, Vec<Inline>) {
    let anchor = inlines
        .iter()
        .find(|i| is_anchor_span(i))
        .and_then(|i| match i {
            Inline::Span(attr, _) => Some(attr.identifier.clone()),
            _ => None,
        });
    let rest = inlines.into_iter().filter(|i| !is_anchor_span(i)).collect();
    (anchor, rest)
}

/// Visit every inline in a block tree.
fn walk_inlines(blocks: &[Block], f: &mut impl FnMut(&Inline)) {
    fn inlines(list: &[Inline], f: &mut impl FnMut(&Inline)) {
        for inline in list {
            f(inline);
            match inline {
                Inline::Emph(i) | Inline::Strong(i) | Inline::Strikeout(i)
                | Inline::Superscript(i) | Inline::Subscript(i) | Inline::SmallCaps(i)
                | Inline::Underline(i) | Inline::Quoted(_, i) | Inline::Cite(_, i)
                | Inline::Span(_, i) | Inline::Link(_, i, _) | Inline::Image(_, i, _) => {
                    inlines(i, f);
                }
                Inline::Note(b) => walk_inlines(b, f),
                _ => {}
            }
        }
    }
    for block in blocks {
        match block {
            Block::Plain(i) | Block::Para(i) | Block::Header(_, _, i) => inlines(i, f),
            Block::LineBlock(lines) => {
                for line in lines {
                    inlines(line, f);
                }
            }
            Block::BlockQuote(b) | Block::Div(_, b) => walk_inlines(b, f),
            Block::Figure(_, caption, b) => {
                walk_inlines(&caption.blocks, f);
                walk_inlines(b, f);
            }
            Block::BulletList(items) | Block::OrderedList(_, items) => {
                for b in items {
                    walk_inlines(b, f);
                }
            }
            Block::DefinitionList(items) => {
                for (term, defs) in items {
                    inlines(term, f);
                    for b in defs {
                        walk_inlines(b, f);
                    }
                }
            }
            Block::Table(table) => {
                walk_inlines(&table.caption.blocks, f);
                let rows = table
                    .head
                    .rows
                    .iter()
                    .chain(table.bodies.iter().flat_map(|b| b.head.iter().chain(&b.body)))
                    .chain(&table.foot.rows);
                for row in rows {
                    for c in &row.cells {
                        walk_inlines(&c.blocks, f);
                    }
                }
            }
            Block::CodeBlock(..) | Block::RawBlock(..) | Block::HorizontalRule => {}
        }
    }
}

/// Rewrite every inline list in a block tree in place.
fn map_inline_lists(blocks: &mut [Block], f: &mut impl FnMut(&mut Vec<Inline>)) {
    fn inlines(list: &mut Vec<Inline>, f: &mut impl FnMut(&mut Vec<Inline>)) {
        f(list);
        for inline in list.iter_mut() {
            match inline {
                Inline::Emph(i) | Inline::Strong(i) | Inline::Strikeout(i)
                | Inline::Superscript(i) | Inline::Subscript(i) | Inline::SmallCaps(i)
                | Inline::Underline(i) | Inline::Quoted(_, i) | Inline::Cite(_, i)
                | Inline::Span(_, i) | Inline::Link(_, i, _) | Inline::Image(_, i, _) => {
                    inlines(i, f);
                }
                Inline::Note(b) => map_inline_lists(b, f),
                _ => {}
            }
        }
    }
    for block in blocks.iter_mut() {
        match block {
            Block::Plain(i) | Block::Para(i) | Block::Header(_, _, i) => inlines(i, f),
            Block::LineBlock(lines) => {
                for line in lines.iter_mut() {
                    inlines(line, f);
                }
            }
            Block::BlockQuote(b) | Block::Div(_, b) => map_inline_lists(b, f),
            Block::Figure(_, caption, b) => {
                map_inline_lists(&mut caption.blocks, f);
                map_inline_lists(b, f);
            }
            Block::BulletList(items) | Block::OrderedList(_, items) => {
                for b in items.iter_mut() {
                    map_inline_lists(b, f);
                }
            }
            Block::DefinitionList(items) => {
                for (term, defs) in items.iter_mut() {
                    inlines(term, f);
                    for b in defs.iter_mut() {
                        map_inline_lists(b, f);
                    }
                }
            }
            Block::Table(table) => {
                map_inline_lists(&mut table.caption.blocks, f);
                for row in table
                    .head
                    .rows
                    .iter_mut()
                    .chain(
                        table
                            .bodies
                            .iter_mut()
                            .flat_map(|b| b.head.iter_mut().chain(b.body.iter_mut())),
                    )
                    .chain(table.foot.rows.iter_mut())
                {
                    for c in &mut row.cells {
                        map_inline_lists(&mut c.blocks, f);
                    }
                }
            }
            Block::CodeBlock(..) | Block::RawBlock(..) | Block::HorizontalRule => {}
        }
    }
}

/// Which half of a definition-list entry a paragraph carries.
#[derive(Clone, Copy, PartialEq, Eq)]
enum DefinitionRole {
    Term,
    Definition,
}

/// Emit any accumulated definition-list entries as one block.
fn flush_definitions(
    segments: &mut Vec<Vec<Block>>,
    definitions: &mut Vec<(Vec<Inline>, Vec<Vec<Block>>)>,
) {
    if !definitions.is_empty() {
        segments.push(vec![Block::DefinitionList(std::mem::take(definitions))]);
    }
}

/// Assemble document metadata from the styled leading paragraphs, matching
/// pandoc: title and subtitle keep the last value, repeated fields become
/// blocks, and repeated authors become a list.
fn build_meta(fields: Vec<(&'static str, Vec<Inline>)>) -> ferrodoc_ast::Meta {
    let mut grouped: Vec<(&'static str, Vec<Vec<Inline>>)> = Vec::new();
    for (field, inlines) in fields {
        if let Some((_, values)) = grouped.iter_mut().find(|(f, _)| *f == field) {
            values.push(inlines);
        } else {
            grouped.push((field, vec![inlines]));
        }
    }
    let mut meta = ferrodoc_ast::Meta::new();
    for (field, mut values) in grouped {
        let value = if field == "title" || field == "subtitle" {
            MetaValue::MetaInlines(values.pop().unwrap_or_default())
        } else if values.len() == 1 {
            MetaValue::MetaInlines(values.pop().expect("length checked"))
        } else if field == "author" {
            MetaValue::MetaList(values.into_iter().map(MetaValue::MetaInlines).collect())
        } else {
            MetaValue::MetaBlocks(values.into_iter().map(Block::Para).collect())
        };
        meta.insert(field.to_owned(), value);
    }
    meta
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
    /// Convert the document body: leading metadata-styled paragraphs
    /// become `meta` (pandoc's `sepBodyParts`), the rest become blocks.
    /// Convert the body's parts, taken one at a time so that the whole
    /// document never exists as an XML tree.
    fn document(&self, parts: impl IntoIterator<Item = Node>, state: &mut State) -> Pandoc {
        let mut parts = parts.into_iter().peekable();
        // Metadata is the leading run of styled paragraphs, so it comes
        // off the front of the stream before the body proper begins.
        let mut fields: Vec<(&'static str, Vec<Inline>)> = Vec::new();
        while let Some(node) = parts.peek() {
            if let Some(field) = self.meta_field(node) {
                let inlines = self.inlines(node, state);
                fields.push((field, inlines));
            } else if !(node.name == "p" && paragraph_is_blank(node)) {
                break;
            }
            parts.next();
        }
        let meta = build_meta(fields);
        let mut blocks = self.blocks_of(parts, state, true);
        // Pandoc drops anchors that no internal link points at.
        rewrite_internal_links(&mut blocks, &state.anchors);
        let mut targets = HashSet::new();
        collect_link_targets(&blocks, &mut targets);
        remove_orphan_anchors(&mut blocks, &targets);
        Pandoc { api_version: ferrodoc_ast::API_VERSION.to_vec(), meta, blocks }
    }

    /// Convert a container's paragraphs and tables to blocks. Captions are
    /// paired with adjacent tables and image paragraphs first, numbered
    /// paragraphs accumulate into lists, and the resulting segments are
    /// merged by pandoc's block smushing (code blocks and quotes coalesce).
    fn blocks(&self, parent: &Node, state: &mut State, lists: bool) -> Vec<Block> {
        let elems = body_parts(parent);
        self.blocks_of(elems, state, lists)
    }

    /// Convert an already-collected run of body parts.
    fn blocks_of<N: std::borrow::Borrow<Node>>(
        &self,
        elems: impl IntoIterator<Item = N>,
        state: &mut State,
        lists: bool,
    ) -> Vec<Block> {
        if state.depth >= MAX_NESTING {
            return Vec::new();
        }
        state.depth += 1;
        let blocks = self.blocks_inner(elems, state, lists);
        state.depth -= 1;
        blocks
    }

    /// Convert a run of body parts, one element at a time.
    ///
    /// The scan only ever looks one element ahead — a caption pairs with
    /// the part beside it — so it holds two at a time and never the
    /// whole body. That is what lets a `.docx` be read without its XML
    /// tree ever existing in full.
    fn blocks_inner<N: std::borrow::Borrow<Node>>(
        &self,
        elems: impl IntoIterator<Item = N>,
        state: &mut State,
        lists: bool,
    ) -> Vec<Block> {
        let mut segments: Vec<Vec<Block>> = Vec::new();
        let mut list: Vec<ListItem> = Vec::new();
        let mut definitions: Vec<(Vec<Inline>, Vec<Vec<Block>>)> = Vec::new();
        let mut elems = elems.into_iter();
        let mut lookahead = elems.next();
        while let Some(current) = lookahead.take() {
            lookahead = elems.next();
            let node: &Node = current.borrow();
            let next: Option<&Node> = lookahead.as_ref().map(std::borrow::Borrow::borrow);
            // Caption before body, or body before caption (the latter only
            // when the caption paragraph does not keep with the next one).
            // A numbered paragraph belongs to its list, so it neither
            // captions nor opens a definition entry.
            let numbered = |n: &Node| {
                n.name == "p"
                    && n.child("pPr").is_some_and(|pr| pr.child("numPr").is_some())
                    && self.heading_level(n).is_none()
            };
            // The style test is a hash lookup and the captionable test
            // converts a paragraph, so the cheap one always goes first.
            let pair = next.filter(|_| !lists || !numbered(node)).and_then(|next| {
                if numbered(next) {
                    None
                } else if self.is_caption_para(node) && self.is_captionable(next) {
                    Some((node, next))
                } else if self.is_caption_para(next)
                    && !keep_next(next)
                    && self.is_captionable(node)
                {
                    Some((next, node))
                } else {
                    None
                }
            });
            if let Some((caption, body)) = pair {
                Self::flush_list(&mut segments, &mut list);
                flush_definitions(&mut segments, &mut definitions);
                segments.push(self.captioned(caption, body, state));
                // Both were consumed, so the lookahead advances again.
                lookahead = elems.next();
                continue;
            }
            match node.name.as_str() {
                "p" if is_horizontal_rule(node) => {
                    Self::flush_list(&mut segments, &mut list);
                    flush_definitions(&mut segments, &mut definitions);
                    segments.push(vec![Block::HorizontalRule]);
                }
                "p" if lists
                    && self.definition_role(node).is_some()
                    && !numbered(node) =>
                {
                    Self::flush_list(&mut segments, &mut list);
                    if let Some(block) = self.definition_paragraph(node, &mut definitions, state)
                    {
                        flush_definitions(&mut segments, &mut definitions);
                        segments.push(vec![block]);
                    }
                }
                "p" => {
                    if let Some(item) = lists.then(|| self.list_item(node, state)).flatten() {
                        list.push(item);
                    } else {
                        flush_definitions(&mut segments, &mut definitions);
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
                    flush_definitions(&mut segments, &mut definitions);
                    if let Some(table) = self.table(node, state) {
                        segments.push(vec![table]);
                    }
                }
            }
        }
        Self::flush_list(&mut segments, &mut list);
        flush_definitions(&mut segments, &mut definitions);
        smush_blocks(segments)
    }

    /// Accumulate a definition-list term or definition paragraph.
    /// Returns the block to emit normally when the paragraph could not
    /// join a definition entry.
    fn definition_paragraph(
        &self,
        p: &Node,
        definitions: &mut Vec<(Vec<Inline>, Vec<Vec<Block>>)>,
        state: &mut State,
    ) -> Option<Block> {
        if self.definition_role(p) == Some(DefinitionRole::Term) {
            let term = trim_paragraph(self.inlines(p, state));
            definitions.push((term, Vec::new()));
            return None;
        }
        let block = self.styled_block(p, state)?;
        let Some((_, items)) = definitions.last_mut() else {
            // No term is open, so this is just a paragraph.
            return Some(block);
        };
        // The first definition opens an item; later ones extend it.
        if let Some(last) = items.last_mut() {
            last.push(block);
        } else {
            items.push(vec![block]);
        }
        None
    }

    fn flush_list(segments: &mut Vec<Vec<Block>>, list: &mut Vec<ListItem>) {
        if !list.is_empty() {
            let items = std::mem::take(list);
            segments.push(build_lists(&items));
        }
    }

    // --- captions ---

    /// The style names a paragraph inherits, nearest first: its own style
    /// name then each `basedOn` ancestor. Bounded so a cyclic chain cannot
    /// hang.
    fn style_names<'a>(&'a self, node: &'a Node) -> impl Iterator<Item = &'a str> {
        self.names_of_style(para_style(node))
    }

    /// Whether a paragraph's own style is named "caption", "table caption"
    /// or "image caption", case-insensitively. Styles merely *based on* a
    /// caption style do not caption, matching pandoc.
    fn is_caption_para(&self, node: &Node) -> bool {
        node.name == "p"
            && self.style_names(node).next().is_some_and(|name| {
                matches!(
                    name.to_lowercase().as_str(),
                    "caption" | "table caption" | "image caption"
                )
            })
    }

    /// The names of a style id and the styles it is based on, walked
    /// lazily: these predicates run several times per paragraph, so
    /// collecting a vector each time was pure allocation.
    fn names_of_style<'a>(&'a self, id: &'a str) -> impl Iterator<Item = &'a str> {
        let mut style = Some(id);
        // Bounded so a cyclic `basedOn` chain cannot loop forever.
        (0..16).map_while(move |_| {
            let (name, based_on) = self.styles.get(style?)?;
            style = based_on.as_deref();
            Some(name.as_str())
        })
    }

    /// Whether a paragraph inherits any of the given style names
    /// (case-insensitive).
    fn has_style_name(&self, node: &Node, names: &[&str]) -> bool {
        self.style_names(node)
            .any(|name| names.iter().any(|n| same_style_name(name, n)))
    }

    /// The heading level of a paragraph whose style is named "Heading <n>"
    /// (any case). The space is required: Word's "Heading1" is a different
    /// style and pandoc does not treat it as a heading.
    fn heading_level(&self, node: &Node) -> Option<i64> {
        self.style_names(node).find_map(heading_level_of)
    }

    /// A heading's classes: the paragraph's own style name, when that is
    /// not itself "Heading <n>", with spaces turned into dashes. Ancestor
    /// styles do not contribute — they only supply the level.
    fn heading_classes(&self, node: &Node) -> Vec<String> {
        self.style_names(node)
            .next()
            .filter(|name| heading_level_of(name).is_none())
            .map(|name| vec![name.replace(char::is_whitespace, "-")])
            .unwrap_or_default()
    }

    /// The metadata field a paragraph's style maps to, if any.
    fn meta_field(&self, node: &Node) -> Option<&'static str> {
        if node.name != "p" {
            return None;
        }
        self.style_names(node).find_map(|name| {
            [
                ("Title", "title"),
                ("Subtitle", "subtitle"),
                ("Author", "author"),
                ("Date", "date"),
                ("Abstract", "abstract"),
            ]
            .into_iter()
            .find(|(style, _)| same_style_name(name, style))
            .map(|(_, field)| field)
        })
    }

    /// Whether a paragraph is a definition-list term or definition.
    fn definition_role(&self, node: &Node) -> Option<DefinitionRole> {
        if node.name != "p" {
            return None;
        }
        self.style_names(node).find_map(|name| {
            if same_style_name(name, "Definition Term") {
                Some(DefinitionRole::Term)
            } else if same_style_name(name, "Definition") {
                Some(DefinitionRole::Definition)
            } else {
                None
            }
        })
    }

    /// Whether a body part can take a caption: a table, or a paragraph
    /// whose content converts to exactly one image.
    ///
    /// This asks the same question the conversion will answer, by doing the
    /// conversion — a cheaper syntactic approximation gets it wrong on
    /// paragraphs carrying bookmarks or stray space runs, and a caption
    /// paired with a body that then converts differently loses content.
    /// The cost is bounded by only calling this next to a caption-styled
    /// paragraph.
    fn is_captionable(&self, node: &Node) -> bool {
        if node.name == "tbl" {
            return true;
        }
        node.name == "p"
            && matches!(
                self.inlines(node, &mut State::default()).as_slice(),
                [Inline::Image(..)]
            )
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
            // Anything else keeps both: dropping the body here would
            // delete content silently. An empty code block is the one thing
            // worth discarding — a styled image paragraph converts to one,
            // and it carries nothing.
            _ => {
                let mut out: Vec<Block> = body_blocks
                    .into_iter()
                    .filter(|block| !matches!(block, Block::CodeBlock(_, text) if text.is_empty()))
                    .collect();
                out.extend(caption_blocks);
                out
            }
        }
    }

    // --- paragraphs ---

    /// A numbered, non-heading paragraph becomes a pending list item.
    fn list_item(&self, p: &Node, state: &mut State) -> Option<ListItem> {
        if self.heading_level(p).is_some() {
            return None;
        }
        let num = p.child("pPr")?.child("numPr")?;
        let level_key = num.child("ilvl")?.attr("w:val")?;
        let level = level_key.parse::<usize>().ok()?;
        let num_id = num.child("numId")?.attr("w:val")?;
        // An unresolvable level (missing numbering.xml, unknown numId, or
        // the `numId="0"` idiom that cancels numbering) is not a list.
        let info = self.num_level(num_id, level)?;
        let level_start = info.start;
        let number = state.list_number(num_id, level, level_start);
        let kind = match Some(&info) {
            Some(info) if info.format != "bullet" => {
                let style = match info.format.as_str() {
                    "decimal" => ListNumberStyle::Decimal,
                    "lowerLetter" => ListNumberStyle::LowerAlpha,
                    "upperLetter" => ListNumberStyle::UpperAlpha,
                    "lowerRoman" => ListNumberStyle::LowerRoman,
                    "upperRoman" => ListNumberStyle::UpperRoman,
                    _ => ListNumberStyle::DefaultStyle,
                };
                // Only the three exact marker shapes name a delimiter;
                // anything else (a multilevel "%1.%2", say) is default.
                let placeholder = format!("%{}", level + 1);
                let text = info.text.as_str();
                let delim = if text == format!("{placeholder}.") {
                    ListNumberDelim::Period
                } else if text == format!("{placeholder})") {
                    ListNumberDelim::OneParen
                } else if text == format!("({placeholder})") {
                    ListNumberDelim::TwoParens
                } else {
                    ListNumberDelim::DefaultDelim
                };
                ListKind::Ordered(number, style, delim)
            }
            _ => ListKind::Bullet,
        };
        let continuation = info.text.trim().is_empty();
        let block = self.styled_block(p, state)?;
        Some(ListItem { level, num_id: num_id.to_owned(), kind, continuation, block })
    }

    /// Map a non-list paragraph to a block by its style.
    fn paragraph(&self, p: &Node, state: &mut State) -> Option<Block> {
        if let Some(level) = self.heading_level(p) {
            // Headings are not trimmed (unlike paragraphs), and pandoc
            // suppresses their anchor spans, using a bookmark's name as
            // the heading id when one is present.
            let inlines = self.inlines(p, state);
            let (anchor, inlines) = take_anchor(inlines);
            let id = match anchor {
                Some(name) => {
                    state.claim_ident(name)
                }
                None => state.ident(&inlines),
            };
            return Some(Block::Header(
                level,
                Attr {
                    identifier: id,
                    classes: self.heading_classes(p),
                    attributes: Vec::new(),
                },
                inlines,
            ));
        }
        self.styled_block(p, state)
    }

    /// A paragraph as a block per its style (everything but headings).
    /// Styles are matched by *name*, resolved through `styles.xml` and its
    /// `basedOn` chain, so localized style ids work and a document without
    /// `styles.xml` simply yields paragraphs.
    fn styled_block(&self, p: &Node, state: &mut State) -> Option<Block> {
        if self.has_style_name(p, &["Source Code", "SourceCode"]) {
            return Some(Block::CodeBlock(Attr::default(), raw_text(p)));
        }
        let inlines = trim_paragraph(self.inlines(p, state));
        if inlines.is_empty() {
            return None;
        }
        // Indented, unnumbered paragraphs are quotes for pandoc too.
        let indented = p
            .child("pPr")
            .filter(|pr| pr.child("numPr").is_none())
            .and_then(|pr| pr.child("ind"))
            .and_then(|ind| ind.attr("w:left"))
            .and_then(|v| v.parse::<i64>().ok())
            .is_some_and(|left| left > 0)
            && !self.has_style_name(p, &["List Paragraph"]);
        if indented
            || self.has_style_name(
                p,
                &["Block Text", "Quote", "Block Quote", "Block Quotation", "Intense Quote"],
            )
        {
            return Some(Block::BlockQuote(vec![Block::Para(inlines)]));
        }
        if self.has_style_name(p, &["Compact"]) {
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
                width: ColWidth::ColWidth(width),
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

        // Pandoc's table builder normalizes every row to the number of
        // columns: over-wide rows and spans are clipped, short rows padded.
        let columns = colspecs.len();
        Some(Block::Table(Box::new(Table {
            attr: Attr::default(),
            caption,
            colspecs,
            head: TableHead {
                attr: Attr::default(),
                rows: normalize_rows(resolve_rowspans(&rows, &head), columns),
            },
            bodies: vec![TableBody {
                attr: Attr::default(),
                row_head_columns: 0,
                head: Vec::new(),
                body: normalize_rows(resolve_rowspans(&rows, &body), columns),
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
        // A column without a usable width is dropped, but it still counts
        // towards the inter-column gaps, as in pandoc.
        let declared = grid.children_named("gridCol").count();
        let columns: Vec<f64> = grid
            .children_named("gridCol")
            .filter_map(|c| c.attr("w:w").and_then(|w| w.parse::<f64>().ok()))
            .collect();
        #[allow(clippy::cast_precision_loss)]
        let total = self.text_width - 10.0 * (declared.saturating_sub(1)) as f64;
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
        let mut cells: Vec<RawCell> = Vec::new();
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
                blocks: single_para_to_plain(self.blocks(tc, state, true)),
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
                // A bookmark is an empty anchor span, so internal links
                // still have a target. Word's own `_GoBack` is noise.
                "bookmarkStart" => {
                    if let Some(name) = node.attr("w:name").filter(|n| *n != "_GoBack") {
                        // Registering the anchor keeps later heading auto
                        // identifiers from colliding with it, and renames a
                        // repeated bookmark the way pandoc does.
                        // Consecutive bookmarks alias to the first one.
                        if let Some(previous) = out.last().and_then(|seq| match seq.as_slice() {
                            [Inline::Span(attr, content)]
                                if content.is_empty()
                                    && attr.classes.iter().any(|c| c == "anchor") =>
                            {
                                Some(attr.identifier.clone())
                            }
                            _ => None,
                        }) {
                            state.anchors.insert(name.to_owned(), previous);
                            continue;
                        }
                        let id = state.ident_from(name);
                        state.anchors.insert(name.to_owned(), id.clone());
                        out.push(vec![Inline::Span(
                            Attr {
                                identifier: id,
                                classes: vec!["anchor".to_owned()],
                                attributes: Vec::new(),
                            },
                            Vec::new(),
                        )]);
                    }
                }
                "r" => {
                    let seq = self.run(node, state);
                    if !seq.is_empty() {
                        out.push(seq);
                    }
                }
                "hyperlink" => {
                    // An external link may carry its fragment separately;
                    // an unresolvable relationship yields an empty target;
                    // a link with no target at all is dropped whole.
                    let relationship = node.attr("r:id");
                    let fragment = node.attr("w:anchor");
                    let url = match (relationship, fragment) {
                        (Some(id), fragment) => match self.rels(state).get(id) {
                            Some(target) => match fragment {
                                Some(fragment) => format!("{target}#{fragment}"),
                                None => target.clone(),
                            },
                            None => String::new(),
                        },
                        (None, Some(fragment)) => format!("#{fragment}"),
                        (None, None) => continue,
                    };
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
        // Inline code is a character style *named* "Verbatim Char".
        let is_code = rpr
            .and_then(|p| p.child("rStyle"))
            .and_then(|s| s.attr("w:val"))
            .is_some_and(|id| {
                // The run style's *own* name, not its ancestors': pandoc's
                // syntax-token styles are based on Verbatim Char and are
                // not themselves code.
                self.styles
                    .get(id)
                    .is_some_and(|(name, _)| name.eq_ignore_ascii_case("Verbatim Char"))
            });
        let mut tokens = Vec::new();
        if is_code {
            // Raw text, spaces and tabs preserved, no tokenization.
            let text = raw_run_text(r);
            if text.is_empty() {
                return Vec::new();
            }
            tokens.push(Inline::Code(Attr::default(), text));
        }
        for node in r.elems().filter(|_| !is_code) {
            match node.name.as_str() {
                "t" => text_tokens(&node.text(), &mut tokens),
                "br" => tokens.push(Inline::LineBreak),
                // Pandoc renders a tab as a space and maps the hyphen and
                // symbol elements to their characters.
                "tab" => tokens.push(Inline::Space),
                "softHyphen" => text_tokens("\u{ad}", &mut tokens),
                "noBreakHyphen" => text_tokens("\u{2011}", &mut tokens),
                "sym" => {
                    if let Some(ch) = node
                        .attr("w:char")
                        .and_then(|c| u32::from_str_radix(c, 16).ok())
                        .and_then(char::from_u32)
                    {
                        text_tokens(&ch.to_string(), &mut tokens);
                    }
                }
                "drawing" => {
                    if let Some(img) = self.image(node, state) {
                        tokens.push(img);
                    }
                }
                "footnoteReference" => {
                    if let Some(note) = self.note(node, self.footnotes.as_ref(), "footnote", state) {
                        tokens.push(note);
                    }
                }
                "endnoteReference" => {
                    if let Some(note) = self.note(node, self.endnotes.as_ref(), "endnote", state) {
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
        // Emph[Strong[…]] in pandoc's output). A code run takes only its
        // vertical alignment: pandoc builds inline code directly and skips
        // the rest of the run's formatting.
        let mut modifiers = Vec::new();
        if !is_code && flag("i") {
            modifiers.push(Modifier::Emph);
        }
        if !is_code && flag("b") {
            modifiers.push(Modifier::Strong);
        }
        if !is_code && flag("smallCaps") {
            modifiers.push(Modifier::SmallCaps);
        }
        if !is_code && flag("strike") {
            modifiers.push(Modifier::Strikeout);
        }
        if !is_code && flag("u") {
            modifiers.push(Modifier::Underline);
        }
        if vert == Some("superscript") {
            modifiers.push(Modifier::Superscript);
        }
        if vert == Some("subscript") {
            modifiers.push(Modifier::Subscript);
        }
        // Highlighted text becomes a "mark" span, like pandoc.
        if !is_code
            && rpr
                .and_then(|p| p.child("highlight"))
                .is_some_and(|h| h.attr("w:val") != Some("none"))
        {
            modifiers.push(Modifier::Span(Attr {
                identifier: String::new(),
                classes: vec!["mark".to_owned()],
                attributes: Vec::new(),
            }));
        }
        stack(&modifiers, tokens)
    }

    /// The relationships of the part currently being read. A package
    /// that declares none for its notes — ferrodoc's own output before
    /// this was understood, and any writer that shares one table — falls
    /// back to the document's, which is what resolved them before.
    fn rels(&self, state: &State) -> &HashMap<String, String> {
        match state.part {
            Part::Footnotes if !self.footnote_rels.is_empty() => &self.footnote_rels,
            Part::Endnotes if !self.endnote_rels.is_empty() => &self.endnote_rels,
            _ => &self.rels,
        }
    }

    fn image(&self, drawing: &Node, state: &State) -> Option<Inline> {
        let blip = find_descendant(drawing, "blip")?;
        let target = self.rels(state).get(blip.attr("r:embed")?)?;
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

    fn note(
        &self,
        reference: &Node,
        part: Option<&Node>,
        element: &str,
        state: &mut State,
    ) -> Option<Inline> {
        let id = reference.attr("w:id")?;
        let footnotes = part?;
        let note = footnotes
            .children_named(element)
            .find(|f| f.attr("w:id") == Some(id))?;
        // A note that references itself would recurse forever.
        if !state.open_notes.insert(format!("{element}{id}")) {
            return None;
        }
        // Pandoc does not rebuild lists inside notes.
        let outer = std::mem::replace(
            &mut state.part,
            if element == "footnote" { Part::Footnotes } else { Part::Endnotes },
        );
        let blocks = self.blocks(note, state, false);
        state.part = outer;
        state.open_notes.remove(&format!("{element}{id}"));
        Some(Inline::Note(blocks))
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
    build_lists_at(items, 0)
}

fn build_lists_at(items: &[ListItem], depth: usize) -> Vec<Block> {
    if depth >= MAX_NESTING {
        return items.iter().map(|i| i.block.clone()).collect();
    }
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
                let sub = build_lists_at(&children[j..sub_end], depth + 1);
                if list_items.is_empty() {
                    list_items.push(Vec::new());
                }
                list_items.last_mut().expect("non-empty").extend(sub);
                j = sub_end;
            }
        }
        lists.push(make_list(&first.kind, list_items));
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

/// Lay rows out on a `columns`-wide grid the way pandoc's table builder
/// does: cells are placed into the columns not already covered by a row
/// span from above, spans that overshoot the grid are clipped, rows that
/// leave free columns are padded, and cells past the grid are dropped.
fn normalize_rows(rows: Vec<Row>, columns: usize) -> Vec<Row> {
    // How many further rows each column is covered for by a span above.
    let mut covered = vec![0usize; columns];
    rows.into_iter()
        .map(|row| {
            let mut cells = Vec::with_capacity(row.cells.len());
            let mut remaining = row.cells.into_iter();
            let mut col = 0usize;
            while col < columns {
                if covered[col] > 0 {
                    covered[col] -= 1;
                    col += 1;
                    continue;
                }
                let mut cell = remaining.next().unwrap_or_else(|| Cell {
                    attr: Attr::default(),
                    alignment: Alignment::AlignDefault,
                    row_span: 1,
                    col_span: 1,
                    blocks: Vec::new(),
                });
                let span = usize::try_from(cell.col_span)
                    .unwrap_or(1)
                    .max(1)
                    .min(columns - col);
                let rows_spanned = usize::try_from(cell.row_span).unwrap_or(1).max(1);
                for c in &mut covered[col..col + span] {
                    *c = rows_spanned - 1;
                }
                cell.col_span = i64::try_from(span).unwrap_or(1);
                cells.push(cell);
                col += span;
            }
            Row { attr: row.attr, cells }
        })
        .collect()
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

/// Whether two style names are the same for pandoc's purposes: case is
/// insignificant, whitespace is not ("Block  Text" is a different style).
/// Concepts that pandoc spells more than one way list every spelling.
fn same_style_name(a: &str, b: &str) -> bool {
    a.eq_ignore_ascii_case(b)
}

/// The level of a style named exactly "Heading <n>" (any case, one space).
fn heading_level_of(name: &str) -> Option<i64> {
    let lower = name.to_lowercase();
    lower.strip_prefix("heading ")?.parse::<i64>().ok()
}

/// Whether a paragraph holds nothing but whitespace, which pandoc allows
/// between the metadata paragraphs of a title block.
fn paragraph_is_blank(p: &Node) -> bool {
    p.elems().all(|n| match n.name.as_str() {
        "pPr" | "proofErr" => true,
        "r" => n.elems().all(|c| match c.name.as_str() {
            "rPr" => true,
            "t" => c.text().trim().is_empty(),
            _ => false,
        }),
        _ => false,
    })
}

/// The paragraphs and tables of a container, looking through the content
/// controls (`w:sdt`) that Word wraps around tables of contents, cover
/// pages and citation fields.
fn body_parts(parent: &Node) -> Vec<&Node> {
    fn collect<'a>(parent: &'a Node, out: &mut Vec<&'a Node>, depth: usize) {
        if depth >= MAX_NESTING {
            return;
        }
        for node in parent.elems() {
            match node.name.as_str() {
                "p" | "tbl" => out.push(node),
                "sdt" => {
                    if let Some(content) = node.child("sdtContent") {
                        collect(content, out, depth + 1);
                    }
                }
                _ => {}
            }
        }
    }
    let mut out = Vec::new();
    collect(parent, &mut out, 0);
    out
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
            "tab" => out.push('\t'),
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

    /// A one-pixel PNG, embedded and read back out.
    #[test]
    fn media_survives_a_round_trip_through_the_package() {
        let png: &[u8] = &[
            0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a, 0, 0, 0, 13, b'I', b'H', b'D', b'R',
            0, 0, 0, 1, 0, 0, 0, 1, 8, 6, 0, 0, 0, 0x1f, 0x15, 0xc4, 0x89, 0, 0, 0, 0, b'I',
            b'E', b'N', b'D', 0xae, 0x42, 0x60, 0x82,
        ];
        let doc = Pandoc::new(vec![Block::Para(vec![Inline::Image(
            Attr::default(),
            vec![Inline::Str("alt".to_owned())],
            Target { url: "pic.png".to_owned(), title: String::new() },
        )])]);
        let bytes = write_docx_with_media(&doc, &|url| {
            (url == "pic.png").then(|| png.to_vec())
        })
        .expect("writable");

        let (back, media) = read_docx_with_media(&bytes).expect("readable");
        // The reader names the image by its part path; whatever that is,
        // the bag must be keyed by the same string the writer will ask
        // for, so the two compose without the caller translating.
        let url = image_url(&back).expect("an image came back");
        assert_eq!(media.get(&url).map(Vec::as_slice), Some(png), "keyed by {url}");

        // ...which is what makes `docx -> docx` keep its picture.
        let again = write_docx_with_media(&back, &|u| media.get(u).cloned()).expect("writable");
        let (_, still) = read_docx_with_media(&again).expect("readable");
        assert_eq!(still.values().next().map(Vec::as_slice), Some(png));

        // Reading without the bag costs nothing and yields nothing.
        assert_eq!(read_docx(&bytes).expect("readable").blocks, back.blocks);

        // A part the document names but the archive does not hold leaves
        // the read succeeding and the bag short — the writer falls back
        // to alt text, which is a document, where an error is nothing.
        let stripped = without_part(&bytes, &format!("word/{url}"));
        let (still, empty) = read_docx_with_media(&stripped).expect("still readable");
        assert!(empty.is_empty(), "a missing part is not an error");
        assert_eq!(still.blocks, back.blocks);
    }

    #[test]
    fn only_an_image_relationship_can_pull_a_part_into_the_bag() {
        let bytes = one_image_docx();
        // A package whose picture reference resolves to something that is
        // not an image — hand-crafted, but nothing stops a real one — must
        // not make the reader load that part. Retyping the relationship
        // is the whole difference.
        let retyped = rewrite_rels(&bytes, |rels| {
            rels.replace("/relationships/image", "/relationships/comments")
        });
        let (_, media) = read_docx_with_media(&retyped).expect("readable");
        assert!(media.is_empty(), "loaded a part no image relationship named");

        // The package-absolute spelling of a target is legal OPC, and
        // naming the same part that way must still find it.
        let absolute = rewrite_rels(&bytes, |rels| rels.replace("Target=\"media/", "Target=\"/word/media/"));
        let (_, media) = read_docx_with_media(&absolute).expect("readable");
        assert_eq!(media.len(), 1, "an absolute target names the same part");
    }

    #[test]
    fn a_note_resolves_its_relationships_against_its_own_part() {
        let doc = Pandoc::new(vec![Block::Para(vec![Inline::Note(vec![Block::Para(vec![
            Inline::Image(
                Attr::default(),
                Vec::new(),
                Target { url: "n.png".to_owned(), title: String::new() },
            ),
        ])])])]);
        let bytes = write_docx_with_media(&doc, &|_| Some(one_pixel_png())).expect("writable");

        // A Word-written package numbers each part's relationships from
        // one, so the same id names a different picture in each. Pointing
        // the note's id somewhere else must move the note's picture.
        let split = rebuild(&bytes, |name, content| {
            let text = String::from_utf8_lossy(content).into_owned();
            Some(match name {
                "word/_rels/footnotes.xml.rels" => {
                    text.replace("Target=\"media/image1.png\"", "Target=\"media/note.png\"")
                        .into_bytes()
                }
                _ => content.to_vec(),
            })
        });
        assert_eq!(image_url(&read_docx(&split).expect("readable")).as_deref(), Some("media/note.png"));

        // Pandoc writes an *empty* `footnotes.xml.rels` and declares the
        // note's image in the document's, which is why it loses its own
        // footnote pictures. Falling back keeps those documents working.
        let empty = rebuild(&bytes, |name, content| {
            Some(match name {
                "word/_rels/footnotes.xml.rels" => br#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"/>"#.to_vec(),
                _ => content.to_vec(),
            })
        });
        assert_eq!(
            image_url(&read_docx(&empty).expect("readable")).as_deref(),
            Some("media/image1.png"),
            "a part with no relationships of its own falls back to the document's"
        );

        // A Word package declares a note's picture *only* in the note's
        // rels, so a bag built from the document's alone would miss it.
        let note_only = rebuild(&bytes, |name, content| {
            let text = String::from_utf8_lossy(content);
            Some(match name {
                "word/_rels/document.xml.rels" => {
                    let start = text.find(r#"<Relationship Id="rId4""#).expect("the image rel");
                    let end = text[start..].find("/>").expect("its end") + start + 2;
                    format!("{}{}", &text[..start], &text[end..]).into_bytes()
                }
                _ => content.to_vec(),
            })
        });
        let (_, media) = read_docx_with_media(&note_only).expect("readable");
        assert_eq!(media.len(), 1, "a picture only the note's rels declare is still carried");
    }

    fn one_pixel_png() -> Vec<u8> {
        vec![
            0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a, 0, 0, 0, 13, b'I', b'H', b'D', b'R',
            0, 0, 0, 1, 0, 0, 0, 1, 8, 6, 0, 0, 0, 0x1f, 0x15, 0xc4, 0x89, 0, 0, 0, 0, b'I',
            b'E', b'N', b'D', 0xae, 0x42, 0x60, 0x82,
        ]
    }

    /// The body is streamed, so a part that fails to parse fails *during*
    /// the walk rather than before it. It must still be reported: a
    /// truncated document is a wrong answer where an error is a true one.
    #[test]
    fn a_malformed_body_is_refused_not_truncated() {
        let good = write_docx(&Pandoc::new(vec![
            Block::Para(vec![Inline::Str("first".to_owned())]),
            Block::Para(vec![Inline::Str("second".to_owned())]),
        ]))
        .expect("writable");
        assert_eq!(read_docx(&good).expect("readable").blocks.len(), 2);

        for (name, break_it) in [
            // Truncated after the first paragraph.
            ("truncated", (|xml: &str| {
                let at = xml.find("second").expect("the second paragraph");
                xml[..at].to_owned()
            }) as fn(&str) -> String),
            // An entity nothing defines, in the middle of the body.
            ("bad entity", |xml: &str| xml.replace("second", "&nope;")),
            // Malformed before the body ever starts.
            ("bad prologue", |xml: &str| xml.replace("<w:body>", "<w:body attr=>")),
            // No body at all.
            ("no body", |xml: &str| xml.replace("w:body", "w:elsewhere")),
        ] {
            let broken = rebuild(&good, |part, content| {
                Some(if part == "word/document.xml" {
                    break_it(&String::from_utf8_lossy(content)).into_bytes()
                } else {
                    content.to_vec()
                })
            });
            assert!(read_docx(&broken).is_err(), "{name} was not refused");
        }
    }

    #[test]
    fn the_section_properties_are_the_body_s_own() {
        // `sectPr` inside a `pPr` is a section *break*, and every one of
        // those precedes the body's own — which is what sets text width,
        // and so every table's column widths.
        let section = "<w:sectPr><w:pgSz w:w=\"12240\"/><w:pgMar w:left=\"1\" w:right=\"2\" w:gutter=\"3\"/></w:sectPr>";
        let xml = format!("<w:body><w:p><w:pPr><w:sectPr><w:pgSz w:w=\"1\"/></w:sectPr></w:pPr></w:p>{section}</w:body>");
        let found = body_section(&xml).expect("a section");
        assert_eq!(found, section);
        let parsed = xml::parse(&found).expect("parsable");
        assert!((text_width(Some(&parsed)) - (12240.0 - 6.0)).abs() < f64::EPSILON);
        // Both spellings, and none at all.
        assert_eq!(body_section("<w:body><w:sectPr/></w:body>").as_deref(), Some("<w:sectPr/>"));
        assert_eq!(body_section("<w:body/>"), None);
        assert!((text_width(None) - DEFAULT_TEXT_WIDTH).abs() < f64::EPSILON);
    }

    #[test]
    fn every_legal_spelling_of_a_target_names_the_same_part() {
        // All four are what Word and pandoc actually write.
        for spelling in [
            "media/image1.png",
            "/word/media/image1.png",
            "../word/media/image1.png",
            "media/./image1.png",
        ] {
            assert_eq!(part_path(spelling), "word/media/image1.png", "{spelling}");
        }
        // Reserved characters are escaped in the target and literal in
        // the archive entry name.
        assert_eq!(part_path("media/my%20image.png"), "word/media/my image.png");
        assert_eq!(part_path("media/a%2Bb.png"), "word/media/a+b.png");
        // Anything that is not an escape stays exactly as written.
        assert_eq!(part_path("media/100%.png"), "word/media/100%.png");
        assert_eq!(part_path("media/%zz.png"), "word/media/%zz.png");
        // `..` past the root cannot escape anywhere: the result is looked
        // up as a zip entry name, and an archive has no parent.
        assert_eq!(part_path("../../../etc/passwd"), "etc/passwd");
        // An escape that spells a separator or a dot segment is one, so
        // it must be decoded before the `..` pass, not after.
        assert_eq!(part_path("media/%2e%2e/other/x.png"), "word/other/x.png");
        assert_eq!(part_path("media%2fx.png"), "word/media/x.png");
    }

    /// A minimal package holding exactly one embedded image.
    fn one_image_docx() -> Vec<u8> {
        let png = one_pixel_png();
        let doc = Pandoc::new(vec![Block::Para(vec![Inline::Image(
            Attr::default(),
            Vec::new(),
            Target { url: "pic.png".to_owned(), title: String::new() },
        )])]);
        write_docx_with_media(&doc, &|_| Some(png.clone())).expect("writable")
    }

    /// The same archive with every relationship part rewritten. Every
    /// one, because the writer declares the same set in each and a test
    /// that changed only the document's would prove nothing.
    fn rewrite_rels(bytes: &[u8], edit: impl Fn(&str) -> String) -> Vec<u8> {
        rebuild(bytes, |name, content| {
            if name.rsplit('.').next() == Some("rels") {
                Some(edit(&String::from_utf8_lossy(content)).into_bytes())
            } else {
                Some(content.to_vec())
            }
        })
    }

    /// The same archive with one entry left out.
    fn without_part(bytes: &[u8], drop: &str) -> Vec<u8> {
        let dropped = drop.to_owned();
        rebuild(bytes, move |name, content| {
            (name != dropped).then(|| content.to_vec())
        })
    }

    /// Copy an archive entry by entry, letting `keep` drop or rewrite each.
    fn rebuild(bytes: &[u8], keep: impl Fn(&str, &[u8]) -> Option<Vec<u8>>) -> Vec<u8> {
        let mut source = zip::ZipArchive::new(std::io::Cursor::new(bytes)).expect("a zip");
        let mut out = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        for i in 0..source.len() {
            let mut file = source.by_index(i).expect("an entry");
            let name = file.name().to_owned();
            let mut content = Vec::new();
            file.read_to_end(&mut content).expect("readable");
            if let Some(content) = keep(&name, &content) {
                out.start_file(name, zip::write::SimpleFileOptions::default())
                    .expect("writable");
                std::io::Write::write_all(&mut out, &content).expect("writable");
            }
        }
        out.finish().expect("finishable").into_inner()
    }

    fn image_url(doc: &Pandoc) -> Option<String> {
        let mut urls = Vec::new();
        collect_image_urls(&doc.blocks, &mut urls);
        urls.into_iter().next()
    }

    #[test]
    fn image_urls_are_collected_from_every_container() {
        let image = |name: &str| {
            Inline::Image(
                Attr::default(),
                Vec::new(),
                Target { url: name.to_owned(), title: String::new() },
            )
        };
        let doc = Pandoc::new(vec![
            Block::Header(1, Attr::default(), vec![image("h.png")]),
            Block::DefinitionList(vec![(
                vec![image("dt.png")],
                vec![vec![Block::Para(vec![image("dd.png")])]],
            )]),
            Block::Figure(
                Attr::default(),
                Caption {
                    short: None,
                    blocks: vec![Block::Plain(vec![image("fc.png")])],
                },
                vec![Block::Plain(vec![image("fb.png")])],
            ),
            Block::LineBlock(vec![vec![image("lb.png")]]),
            Block::BlockQuote(vec![Block::Para(vec![image("q.png")])]),
            Block::BulletList(vec![vec![Block::Plain(vec![image("l.png")])]]),
            Block::Table(Box::new(Table {
                attr: Attr::default(),
                caption: Caption::default(),
                colspecs: vec![ColSpec {
                    alignment: Alignment::AlignDefault,
                    width: ColWidth::ColWidthDefault,
                }],
                head: TableHead {
                    attr: Attr::default(),
                    rows: vec![Row {
                        attr: Attr::default(),
                        cells: vec![Cell {
                            attr: Attr::default(),
                            alignment: Alignment::AlignDefault,
                            row_span: 1,
                            col_span: 1,
                            blocks: vec![Block::Plain(vec![image("t.png")])],
                        }],
                    }],
                },
                bodies: vec![TableBody {
                    attr: Attr::default(),
                    row_head_columns: 0,
                    head: Vec::new(),
                    body: vec![Row {
                        attr: Attr::default(),
                        cells: vec![Cell {
                            attr: Attr::default(),
                            alignment: Alignment::AlignDefault,
                            row_span: 1,
                            col_span: 1,
                            blocks: vec![Block::Plain(vec![image("b.png")])],
                        }],
                    }],
                }],
                foot: TableFoot {
                    attr: Attr::default(),
                    rows: vec![Row {
                        attr: Attr::default(),
                        cells: vec![Cell {
                            attr: Attr::default(),
                            alignment: Alignment::AlignDefault,
                            row_span: 1,
                            col_span: 1,
                            blocks: vec![Block::Plain(vec![image("f.png")])],
                        }],
                    }],
                },
            })),
            Block::Para(vec![Inline::Note(vec![Block::Para(vec![image("n.png")])])]),
            Block::Para(vec![Inline::Emph(vec![Inline::Link(
                Attr::default(),
                vec![image("e.png")],
                Target::default(),
            )])]),
        ]);
        let mut urls = Vec::new();
        collect_image_urls(&doc.blocks, &mut urls);
        urls.sort();
        assert_eq!(
            urls,
            [
                "b.png", "dd.png", "dt.png", "e.png", "f.png", "fb.png", "fc.png", "h.png",
                "l.png", "lb.png", "n.png", "q.png", "t.png",
            ]
        );
    }

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

    fn cell(col_span: i64, row_span: i64) -> Cell {
        Cell {
            attr: Attr::default(),
            alignment: Alignment::AlignDefault,
            row_span,
            col_span,
            blocks: Vec::new(),
        }
    }

    #[test]
    fn rows_are_clipped_and_padded_to_the_grid() {
        let rows = vec![
            Row { attr: Attr::default(), cells: vec![cell(9, 1)] },
            Row { attr: Attr::default(), cells: vec![cell(1, 1)] },
        ];
        let out = normalize_rows(rows, 3);
        // An over-wide span is clipped to the grid.
        assert_eq!(out[0].cells.len(), 1);
        assert_eq!(out[0].cells[0].col_span, 3);
        // A short row is padded to the column count.
        assert_eq!(out[1].cells.len(), 3);
    }

    #[test]
    fn padding_skips_columns_covered_from_above() {
        let rows = vec![
            Row { attr: Attr::default(), cells: vec![cell(1, 2), cell(1, 1)] },
            // The first column is still covered by the row span above, so
            // this row's single cell fills the grid and needs no padding.
            Row { attr: Attr::default(), cells: vec![cell(1, 1)] },
        ];
        let out = normalize_rows(rows, 2);
        assert_eq!(out[1].cells.len(), 1);
    }

    #[test]
    fn metadata_groups_repeated_fields() {
        let ils = |s: &str| vec![Inline::Str(s.to_owned())];
        let meta = build_meta(vec![
            ("title", ils("T")),
            ("author", ils("A1")),
            ("author", ils("A2")),
        ]);
        assert_eq!(meta["title"], MetaValue::MetaInlines(ils("T")));
        assert_eq!(
            meta["author"],
            MetaValue::MetaList(vec![
                MetaValue::MetaInlines(ils("A1")),
                MetaValue::MetaInlines(ils("A2")),
            ])
        );
    }

    #[test]
    fn heading_anchor_becomes_the_identifier() {
        let anchor = Inline::Span(
            Attr {
                identifier: "custom".to_owned(),
                classes: vec!["anchor".to_owned()],
                attributes: Vec::new(),
            },
            Vec::new(),
        );
        let (id, rest) = take_anchor(vec![anchor, Inline::Str("Title".to_owned())]);
        assert_eq!(id.as_deref(), Some("custom"));
        assert_eq!(rest, vec![Inline::Str("Title".to_owned())]);
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

