//! ODT (`OpenDocument` Text) reader and writer producing the ferrodoc
//! (pandoc-compatible) AST.
//!
//! [`read_odt`] parses an `.odt` byte stream and maps it to the same AST
//! `pandoc -f odt -t json` produces (differentially verified by
//! `ferrodoc-harness diff-odt`); [`write_odt`] emits a package
//! `LibreOffice` and Word open, gated by round trip through pandoc
//! (`diff-odt-write`).
//!
//! An ODF package is a zip of XML parts like a DOCX, but almost nothing
//! else transfers from that reader: pandoc's ODT reader is a much plainer
//! thing than its docx one, and most of the work here was finding out how
//! plain. The behaviours that are not evident from the ODF specification,
//! each measured against the 3.8.2.1 binary:
//!
//! - **no metadata at all.** `meta.xml` is not read, and the `Title`,
//!   `Author` and `Date` paragraphs pandoc's own writer emits come back as
//!   ordinary paragraphs. `meta` is always empty;
//! - **no code blocks.** A code block written by pandoc's own ODT writer
//!   does not survive its own reader: it comes back as one paragraph per
//!   line;
//! - **a block quote is an indent, not a style.** A paragraph whose
//!   `fo:margin-left` reaches 5.5 mm is a quote and one below it is not,
//!   whatever either is called. `Quotations` qualifies only because its
//!   usual definition carries 0.3937 in; `Table Contents` (0.76 mm) and
//!   `Footnote` (5.0 mm) both sit under the line. The margin is the
//!   largest anywhere in the `style:parent-style-name` chain, and inside a
//!   list item the rule does not apply at all;
//! - inline code is the character style *named* `Source_Text`, whatever
//!   properties it carries — a style with `Source_Text`'s exact properties
//!   under another name is not code, and a style descending from it is not
//!   either;
//! - a span in a font declared `style:font-pitch="fixed"` becomes `Emph`;
//! - **text properties are not inherited** through
//!   `style:parent-style-name`, though paragraph ones are: a style whose
//!   parent is bold and which adds italic is italic alone;
//! - a `text:h` is a heading whatever its style, and a `text:p` is never
//!   one — a paragraph styled `Heading 1` stays a paragraph, which is what
//!   `LibreOffice` writes when it imports an HTML `<h1>`;
//! - a note body holds paragraphs only: a list, heading or table inside
//!   one contributes nothing;
//! - table spans are dropped rather than represented — a
//!   `table:covered-table-cell` shortens its row, which is padded at the
//!   end — and the column count is the widest row rather than the
//!   `table:table-column` count. Widths and alignments are dropped too, so
//!   every column is `ColWidthDefault`/`AlignDefault`;
//! - every bookmark becomes an anchor named `anchor`, `anchor-1`, …
//!   whatever the bookmark is called. The identifiers considered taken are
//!   the *values* of the bookmark map, so a heading whose identifier
//!   equals a bookmark's name rebinds it and frees `anchor` again. A
//!   `text:bookmark-ref` allocates one for a name it has not seen, but a
//!   `text:a` pointing at `#name` is not rewritten to match;
//! - a link's `xlink:href` loses one leading `../`, the step pandoc's
//!   writer adds to reach out of the package;
//! - an image's `svg:title` becomes the target title *slugified*, its
//!   `svg:desc` is dropped, and `svg:width`/`svg:height` are carried
//!   verbatim as attributes;
//! - a `text:section` is a `Div`, and horizontal rules, annotations, soft
//!   page breaks, tables of contents, `draw:text-box` content and the
//!   field elements (`text:date`, `text:page-number`) are dropped —
//!   `text:sequence` alone keeps its text.
//!
//! Known gaps, deliberate and unfixed:
//!
//! - **pandoc reads every list twice** — and 2^n times at n levels of
//!   nesting — which shows up only as a higher identifier suffix on a
//!   heading or bookmark inside a list. Not reproduced: the identifiers
//!   are unique and consistent either way, and copying an exponential
//!   blowup into a converter whose promise is that it cannot be made to
//!   hang is the worse trade. `corpus/odt/spec-03.odt` and `spec-09.odt`;
//! - flat ODF (`.fodt`, a single XML file with no zip around it) is not
//!   read;
//! - `text:bibliography-mark` becomes a `Cite` for pandoc and is dropped
//!   here, citations being out of scope for this project;
//! - tracked changes, forms and embedded objects are skipped;
//! - conversion is bounded: XML deeper than 256 elements is rejected, and
//!   container nesting beyond 64 levels drops the remaining content rather
//!   than recursing without limit.

mod style;
mod write;

pub use write::{write_odt, write_odt_with_media};

use ferrodoc_ast::{
    Alignment, Attr, Block, Caption, Cell, ColSpec, ColWidth, Inline, ListAttributes,
    ListNumberDelim, ListNumberStyle, Pandoc, Row, Table, TableBody, TableFoot, TableHead, Target,
};
use ferrodoc_docx::xml::{self, Child, Node};
use std::collections::HashMap;
use std::io::Read;
use style::{CODE_STYLE, Level, Position, Styles, TextProps};

/// An error reading an ODT file.
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
            Error::Zip(e) => write!(f, "not a readable odt (zip) archive: {e}"),
            Error::Xml(e) => write!(f, "malformed XML part: {e}"),
            Error::MissingPart(p) => write!(f, "missing required part: {p}"),
        }
    }
}

impl std::error::Error for Error {}

impl From<ferrodoc_docx::Error> for Error {
    fn from(e: ferrodoc_docx::Error) -> Self {
        // The shared XML layer reports through the docx crate's error type;
        // its three cases are the same three.
        match e {
            ferrodoc_docx::Error::Zip(e) => Error::Zip(e),
            ferrodoc_docx::Error::Xml(e) => Error::Xml(e),
            ferrodoc_docx::Error::MissingPart(p) => Error::MissingPart(p),
        }
    }
}

/// The image bytes a document carries, keyed by the URL its AST refers to
/// them by — exactly the string [`write_odt_with_media`]'s resolver is
/// asked for, so a bag from one document feeds straight into writing
/// another.
pub type Media = HashMap<String, Vec<u8>>;

/// Read an ODT document into a [`Pandoc`] AST equivalent to pandoc's odt
/// reader output.
///
/// The AST names each image by its part path but does not carry the bytes;
/// use [`read_odt_with_media`] when the images have to survive.
pub fn read_odt(bytes: &[u8]) -> Result<Pandoc, Error> {
    read(bytes, false).map(|(doc, _)| doc)
}

/// Read an ODT document together with the bytes of every image it embeds.
///
/// # Errors
///
/// The same as [`read_odt`]. A part that is named but missing from the
/// archive is left out of the bag rather than failing the read.
pub fn read_odt_with_media(bytes: &[u8]) -> Result<(Pandoc, Media), Error> {
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
    let content = part("content.xml").ok_or(Error::MissingPart("content.xml"))?;
    let styles_part = part("styles.xml");

    let mut styles = Styles::default();
    // Both parts declare styles into one namespace: `styles.xml` the named
    // ones and `content.xml` the automatic ones an editor generates, and a
    // paragraph may name either.
    if let Some(text) = &styles_part {
        styles.absorb(&xml::parse(text)?);
    }
    styles.absorb(&xml::parse(&head(&content)?)?);

    let ctx = Ctx { styles };
    let mut state = State::default();
    // A malformed part stops the stream and is reported, never returned as
    // a truncated document.
    let mut failure: Option<Error> = None;
    let body = xml::body_children(&content, &["body", "text"])?;
    let blocks = {
        let failure = &mut failure;
        let parts = body.map_while(move |child| match child {
            Ok(node) => Some(node),
            Err(e) => {
                *failure = Some(e.into());
                None
            }
        });
        ctx.blocks(parts, &mut state)
    };
    if let Some(failure) = failure {
        return Err(failure);
    }
    // Pandoc's ODT reader produces no metadata: `meta.xml` is not consulted
    // and a `Title` paragraph stays a paragraph. Measured, not assumed.
    let doc = Pandoc::new(blocks);

    let mut media = Media::new();
    if want_media {
        let mut urls = Vec::new();
        collect_image_urls(&doc.blocks, &mut urls);
        for url in urls {
            if media.contains_key(&url) {
                continue;
            }
            // A part named but missing is not an error: the document is
            // still readable and the writer falls back to alt text.
            if let Ok(mut file) = archive.by_name(&url) {
                let mut bytes = Vec::new();
                if file.read_to_end(&mut bytes).is_ok() {
                    media.insert(url, bytes);
                }
            }
        }
    }
    Ok((doc, media))
}

/// Everything in `content.xml` ahead of `<office:body>`, closed off so it
/// parses on its own.
fn head(content: &str) -> Result<String, Error> {
    let cut = xml::element_offset(content, "body")?.unwrap_or(content.len());
    Ok(format!("{}</office:document-content>", &content[..cut]))
}

/// Every image URL the document names, in document order.
fn collect_image_urls(blocks: &[Block], out: &mut Vec<String>) {
    walk_inlines(blocks, &mut |inline| {
        if let Inline::Image(_, _, target) = inline {
            out.push(target.url.clone());
        }
    });
}

fn walk_inlines(blocks: &[Block], f: &mut impl FnMut(&Inline)) {
    fn inlines(list: &[Inline], f: &mut impl FnMut(&Inline)) {
        for inline in list {
            f(inline);
            match inline {
                Inline::Emph(inner)
                | Inline::Underline(inner)
                | Inline::Strong(inner)
                | Inline::Strikeout(inner)
                | Inline::Superscript(inner)
                | Inline::Subscript(inner)
                | Inline::SmallCaps(inner)
                | Inline::Quoted(_, inner)
                | Inline::Cite(_, inner)
                | Inline::Span(_, inner)
                | Inline::Link(_, inner, _)
                | Inline::Image(_, inner, _) => inlines(inner, f),
                Inline::Note(blocks) => walk_inlines(blocks, f),
                _ => {}
            }
        }
    }
    for block in blocks {
        match block {
            Block::Plain(list) | Block::Para(list) | Block::Header(_, _, list) => inlines(list, f),
            Block::LineBlock(lines) => {
                for line in lines {
                    inlines(line, f);
                }
            }
            Block::BlockQuote(inner) | Block::Div(_, inner) => walk_inlines(inner, f),
            Block::BulletList(items) | Block::OrderedList(_, items) => {
                for item in items {
                    walk_inlines(item, f);
                }
            }
            Block::Figure(_, caption, inner) => {
                walk_inlines(&caption.blocks, f);
                walk_inlines(inner, f);
            }
            Block::Table(table) => {
                for row in table
                    .head
                    .rows
                    .iter()
                    .chain(table.bodies.iter().flat_map(|b| b.head.iter().chain(&b.body)))
                    .chain(table.foot.rows.iter())
                {
                    for cell in &row.cells {
                        walk_inlines(&cell.blocks, f);
                    }
                }
            }
            _ => {}
        }
    }
}

/// The style tables, fixed for the whole conversion.
struct Ctx {
    styles: Styles,
}

/// Mutable state threaded through the conversion: identifiers, anchors and
/// list numbering.
#[derive(Default)]
struct State {
    /// Bookmark or reference name to the identifier it was given.
    ///
    /// This map is also what decides which identifiers are *taken*: the
    /// set pandoc uniquifies against is the map's values, so rebinding a
    /// name frees whatever it held before. A heading does exactly that,
    /// which is why the second bookmark in a pandoc-written document is
    /// `anchor` again rather than `anchor-1`.
    anchors: HashMap<String, String>,
    /// How many map entries currently hold each identifier.
    taken: HashMap<String, usize>,
    /// The lowest suffix worth trying for a base name.
    next_suffix: HashMap<String, u32>,
    /// The number the next item of a list style would take, so that
    /// `text:continue-numbering` can pick it up.
    list_numbers: HashMap<String, i64>,
    /// Container nesting depth, to bound recursion on hostile input.
    depth: usize,
    /// Whether the blocks being read are inside a list item.
    in_list_item: bool,
    /// Whether the blocks being read are inside a footnote body.
    in_note: bool,
}

/// The deepest container nesting converted. Real documents nest a handful
/// of levels; beyond this the input is hostile and the remaining content is
/// dropped rather than overflowing the stack.
const MAX_NESTING: usize = 64;

impl State {
    /// The identifier a heading takes.
    ///
    /// Pandoc's `auto_identifiers`: keep alphanumerics, whitespace and
    /// `_-.`; lowercase; join whitespace-separated *words* with `-`; drop
    /// everything before the first letter; empty becomes `section`;
    /// duplicates get `-1`, `-2`, … suffixes. The identifier is then bound
    /// to *itself* in the anchor map, which is what can displace an anchor
    /// a bookmark of the same name was holding.
    fn heading_ident(&mut self, text: &str) -> String {
        let mut base = slug(text);
        let start = base.find(char::is_alphabetic).unwrap_or(base.len());
        base.drain(..start);
        if base.is_empty() {
            "section".clone_into(&mut base);
        }
        let id = self.unique_from(&base);
        self.bind(id.clone(), id.clone());
        id
    }

    /// The identifier a bookmark name was given, allocating one the first
    /// time the name is seen.
    ///
    /// Every bookmark gets `anchor`, `anchor-1`, … whatever it is called:
    /// pandoc drops the name entirely, and a reference to a name that no
    /// bookmark defines still allocates one. Measured.
    fn anchor(&mut self, name: &str) -> String {
        if let Some(id) = self.anchors.get(name) {
            return id.clone();
        }
        let id = self.unique_from("anchor");
        self.bind(name.to_owned(), id.clone());
        id
    }

    /// Bind a name to an identifier, releasing whatever it held before.
    fn bind(&mut self, name: String, id: String) {
        if let Some(old) = self.anchors.insert(name, id.clone())
            && let Some(count) = self.taken.get_mut(&old)
        {
            *count -= 1;
            if *count == 0 {
                self.taken.remove(&old);
                self.free(&old);
            }
        }
        *self.taken.entry(id).or_insert(0) += 1;
    }

    /// The first `base`, `base-1`, `base-2`, … no map entry holds.
    ///
    /// The search resumes from the lowest suffix still worth trying rather
    /// than restarting at zero: restarting is quadratic in the number of
    /// anchors, and every bookmark in a document shares the base `anchor`.
    fn unique_from(&mut self, base: &str) -> String {
        let mut n = self.next_suffix.get(base).copied().unwrap_or(0);
        loop {
            let candidate = if n == 0 { base.to_owned() } else { format!("{base}-{n}") };
            if !self.taken.contains_key(&candidate) {
                self.next_suffix.insert(base.to_owned(), n + 1);
                return candidate;
            }
            n += 1;
        }
    }

    /// Note that an identifier is available again, so the resume point for
    /// its base name drops back to it.
    fn free(&mut self, id: &str) {
        let (base, suffix) = match id.rsplit_once('-') {
            Some((base, tail)) => match tail.parse::<u32>() {
                Ok(suffix) => (base, suffix),
                Err(_) => (id, 0),
            },
            None => (id, 0),
        };
        let hint = self.next_suffix.entry(base.to_owned()).or_insert(suffix);
        *hint = (*hint).min(suffix);
    }
}

impl Ctx {
    /// Convert a run of body-level elements into blocks.
    fn blocks(&self, nodes: impl Iterator<Item = Node>, state: &mut State) -> Vec<Block> {
        let mut out = Vec::new();
        for node in nodes {
            self.block(&node, state, &mut out);
        }
        out
    }

    fn blocks_of(&self, parent: &Node, state: &mut State) -> Vec<Block> {
        let mut out = Vec::new();
        for child in parent.elems() {
            self.block(child, state, &mut out);
        }
        out
    }

    fn block(&self, node: &Node, state: &mut State, out: &mut Vec<Block>) {
        if state.depth >= MAX_NESTING {
            return;
        }
        if state.in_note && node.name != "p" {
            return;
        }
        match node.name.as_str() {
            "p" => {
                let inlines = self.inlines(node, TextProps::default(), state);
                let para = Block::Para(inlines);
                // A list item's own paragraphs are indented by being in a
                // list, so the indent that means "quote" everywhere else
                // means nothing here. Pandoc reads a genuine block quote
                // inside a list item as a plain paragraph for this reason.
                if !state.in_list_item && self.is_quote(node) {
                    out.push(Block::BlockQuote(vec![para]));
                } else {
                    out.push(para);
                }
            }
            "h" => {
                let inlines = self.inlines(node, TextProps::default(), state);
                let level = node
                    .attr("text:outline-level")
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(1);
                let identifier = state.heading_ident(&plain_text(&inlines));
                out.push(Block::Header(
                    level,
                    Attr { identifier, ..Attr::default() },
                    inlines,
                ));
            }
            "list" => out.push(self.list(node, None, 1, state)),
            "table" => out.push(self.table(node, state)),
            // A section is a named region of the document; pandoc keeps the
            // grouping as a `Div` with no attributes of its own.
            "section" => {
                state.depth += 1;
                let inner = self.blocks_of(node, state);
                state.depth -= 1;
                out.push(Block::Div(Attr::default(), inner));
            }
            // Everything else — indexes, tables of contents, sequence
            // declarations, tracked changes — contributes nothing, which is
            // what pandoc does with each of them.
            _ => {}
        }
    }

    /// Whether a paragraph is indented, and so a block quote.
    fn is_quote(&self, node: &Node) -> bool {
        node.attr("text:style-name")
            .is_some_and(|name| self.styles.is_indented(name))
    }

    /// A `text:list`, at `depth` levels of nesting.
    ///
    /// `inherited` is the enclosing list's style name: a nested
    /// `<text:list>` usually names no style of its own and takes the level
    /// below its parent's.
    fn list(&self, node: &Node, inherited: Option<&str>, depth: usize, state: &mut State) -> Block {
        let style = node.attr("text:style-name").or(inherited);
        let level = style.and_then(|name| self.styles.list_level(name, depth));

        let mut items = Vec::new();
        state.depth += 1;
        let outer = std::mem::replace(&mut state.in_list_item, true);
        for child in node.elems() {
            // A `text:list-header` is an unnumbered lead-in, and pandoc
            // reads it as an ordinary item.
            if !matches!(child.name.as_str(), "list-item" | "list-header") {
                continue;
            }
            let mut blocks = Vec::new();
            for grandchild in child.elems() {
                if grandchild.name == "list" {
                    blocks.push(self.list(grandchild, style, depth + 1, state));
                } else {
                    self.block(grandchild, state, &mut blocks);
                }
            }
            items.push(tighten(blocks));
        }
        state.in_list_item = outer;
        state.depth -= 1;

        match level {
            Some(Level { number: Some(format), start }) => {
                let start = Self::list_start(node, style, start, items.len(), state);
                Block::OrderedList(
                    ListAttributes {
                        start,
                        style: number_style(&format.format),
                        delim: delimiter(&format.prefix, &format.suffix),
                    },
                    items,
                )
            }
            Some(Level { number: None, .. }) => Block::BulletList(items),
            // A list whose style is missing entirely is ordered with
            // everything left to the writer — not a bullet list.
            None => Block::OrderedList(
                ListAttributes {
                    start: 1,
                    style: ListNumberStyle::DefaultStyle,
                    delim: ListNumberDelim::DefaultDelim,
                },
                items,
            ),
        }
    }

    /// The number the first item takes, honouring `text:continue-numbering`.
    fn list_start(
        node: &Node,
        style: Option<&str>,
        declared: i64,
        items: usize,
        state: &mut State,
    ) -> i64 {
        let Some(style) = style else { return declared };
        let continuing = node.attr("text:continue-numbering") == Some("true");
        let start = match state.list_numbers.get(style) {
            Some(next) if continuing => *next,
            _ => declared,
        };
        let consumed = i64::try_from(items).unwrap_or(i64::MAX);
        state
            .list_numbers
            .insert(style.to_owned(), start.saturating_add(consumed));
        start
    }

    fn table(&self, node: &Node, state: &mut State) -> Block {
        state.depth += 1;
        let mut head = Vec::new();
        let mut body = Vec::new();
        self.table_rows(node, &mut head, &mut body, state);
        state.depth -= 1;

        // The column count is the widest row, not the number of
        // `table:table-column` elements: pandoc pads short rows and ignores
        // the declarations. Measured with a three-column declaration over
        // two-cell rows.
        let columns = head
            .iter()
            .chain(body.iter())
            .map(Vec::len)
            .max()
            .unwrap_or(0);
        let pad = |rows: Vec<Vec<Cell>>| -> Vec<Row> {
            rows.into_iter()
                .map(|mut cells| {
                    cells.resize_with(columns, empty_cell);
                    Row { attr: Attr::default(), cells }
                })
                .collect()
        };
        Block::Table(Box::new(Table {
            attr: Attr::default(),
            caption: Caption::default(),
            colspecs: vec![
                ColSpec {
                    alignment: Alignment::AlignDefault,
                    width: ColWidth::ColWidthDefault
                };
                columns
            ],
            head: TableHead { attr: Attr::default(), rows: pad(head) },
            bodies: vec![TableBody {
                attr: Attr::default(),
                row_head_columns: 0,
                head: Vec::new(),
                body: pad(body),
            }],
            foot: TableFoot { attr: Attr::default(), rows: Vec::new() },
        }))
    }

    /// Collect a table's rows, descending through the grouping elements
    /// that may wrap them.
    fn table_rows(
        &self,
        node: &Node,
        head: &mut Vec<Vec<Cell>>,
        body: &mut Vec<Vec<Cell>>,
        state: &mut State,
    ) {
        for child in node.elems() {
            match child.name.as_str() {
                "table-row" => body.push(self.table_row(child, state)),
                "table-header-rows" => {
                    for row in child.children_named("table-row") {
                        head.push(self.table_row(row, state));
                    }
                }
                // A row group or a header-column group nests rows one
                // deeper without changing what they mean.
                "table-row-group" | "table-header-columns" | "table-columns" => {
                    self.table_rows(child, head, body, state);
                }
                _ => {}
            }
        }
    }

    fn table_row(&self, node: &Node, state: &mut State) -> Vec<Cell> {
        node.elems()
            .filter_map(|cell| match cell.name.as_str() {
                "table-cell" => Some(Cell {
                    blocks: tighten(self.blocks_of(cell, state)),
                    ..empty_cell()
                }),
                // The positions a span covers are dropped, not kept as
                // empty cells: the row is shorter than the grid and gets
                // padded at the *end*, so the cell after a span moves left.
                _ => None,
            })
            .collect()
    }

    /// The inlines of a paragraph, heading or span.
    ///
    /// `props` is the formatting the enclosing spans accumulated: a nested
    /// span's own style overlays it rather than replacing it, and each span
    /// applies the *whole* accumulated set, which is why bold inside bold
    /// nests twice.
    fn inlines(&self, node: &Node, props: TextProps, state: &mut State) -> Vec<Inline> {
        let mut out = Vec::new();
        for child in &node.children {
            match child {
                Child::Text(text) => {
                    let mut tokens = Vec::new();
                    text_tokens(text, &mut tokens);
                    append(&mut out, tokens);
                }
                Child::Elem(elem) => {
                    let mut chunk = Vec::new();
                    self.inline(elem, props, state, &mut chunk);
                    append(&mut out, chunk);
                }
            }
        }
        out
    }

    fn inline(&self, node: &Node, props: TextProps, state: &mut State, out: &mut Vec<Inline>) {
        if state.depth >= MAX_NESTING {
            return;
        }
        match node.name.as_str() {
            "span" => {
                let name = node.attr("text:style-name").unwrap_or_default();
                if name == CODE_STYLE {
                    // Code takes its whole subtree as text: a span inside a
                    // code span contributes characters, not formatting.
                    // The text is what the *inlines* say, not the raw XML —
                    // a `text:s` asking for two spaces contributes two, and
                    // a whitespace run in the source contributes one.
                    let inner = self.inlines(node, props, state);
                    out.push(Inline::Code(Box::default(), plain_text(&inner)));
                    return;
                }
                let props = props.overlay(self.styles.text_props(name));
                let inner = self.inlines(node, props, state);
                out.extend(decorate(inner, props));
            }
            "a" => {
                let inner = self.inlines(node, props, state);
                out.push(Inline::Link(
                    Box::default(),
                    inner,
                    Box::new(Target {
                        url: href(node.attr("xlink:href").unwrap_or_default()),
                        title: String::new(),
                    }),
                ));
            }
            "line-break" => out.push(Inline::LineBreak),
            // A tab is one space, and `text:s` is the count it declares —
            // one when it declares none.
            "tab" => out.push(Inline::Space),
            "s" => out.extend(std::iter::repeat_n(Inline::Space, space_count(node))),
            "note" => {
                state.depth += 1;
                // A note body holds *paragraphs only*: a list, a heading, a
                // table or a section inside one is read by no reader here
                // and contributes nothing, which is what pandoc does with
                // each of them.
                let outer = std::mem::replace(&mut state.in_note, true);
                let body = node
                    .child("note-body")
                    .map(|body| self.blocks_of(body, state))
                    .unwrap_or_default();
                state.in_note = outer;
                state.depth -= 1;
                out.push(Inline::Note(body));
            }
            // Both spellings of a bookmark mark one position; the closing
            // `text:bookmark-end` marks nothing of its own.
            "bookmark" | "bookmark-start" => {
                if let Some(name) = node.attr("text:name") {
                    let identifier = state.anchor(name);
                    out.push(Inline::Span(
                        Box::new(Attr { identifier, ..Attr::default() }),
                        Vec::new(),
                    ));
                }
            }
            "bookmark-ref" | "reference-ref" => {
                let name = node.attr("text:ref-name").unwrap_or_default();
                let target = format!("#{}", state.anchor(name));
                let inner = self.inlines(node, props, state);
                out.push(Inline::Link(
                    Box::default(),
                    inner,
                    Box::new(Target { url: target, title: String::new() }),
                ));
            }
            "frame" => Self::image(node, out),
            // A numbering field renders as its own text; the other field
            // elements (`text:date`, `text:page-number`) contribute
            // nothing, which is what pandoc does with each.
            "sequence" => text_tokens(&node_text(node), out),
            _ => {}
        }
    }

    /// The picture a `draw:frame` holds, if it holds one.
    fn image(node: &Node, out: &mut Vec<Inline>) {
        // Only the first `draw:image` counts; a frame's later children are
        // alternative representations of the same picture. A frame holding
        // a `draw:text-box` instead holds no picture at all.
        let Some(url) = node
            .children_named("image")
            .find_map(|image| image.attr("xlink:href"))
        else {
            return;
        };
        let mut attributes = Vec::new();
        for (name, key) in [("svg:width", "width"), ("svg:height", "height")] {
            if let Some(value) = node.attr(name) {
                attributes.push((key.to_owned(), value.to_owned()));
            }
        }
        // The title is slugified on the way in — pandoc runs `svg:title`
        // through the same function that makes a heading identifier, so
        // "My Picture" comes back as "my-picture".
        let title = node
            .child("title")
            .map(|t| slug(&t.text()))
            .unwrap_or_default();
        out.push(Inline::Image(
            Box::new(Attr { attributes, ..Attr::default() }),
            Vec::new(),
            Box::new(Target { url: url.to_owned(), title }),
        ));
    }
}

/// How many spaces one `text:s` may stand for. A four-byte element asking
/// for four billion of them is not a document.
const MAX_SPACES: usize = 4096;

/// Wrap inlines in the modifiers a set of text properties calls for.
///
/// The nesting order is pandoc's, measured with every pair: vertical
/// position outermost, then emphasis, then strong, then strikeout. A span
/// whose style says nothing contributes its children and no wrapper at all.
fn decorate(inner: Vec<Inline>, props: TextProps) -> Vec<Inline> {
    if inner.is_empty() {
        return inner;
    }
    let mut out = inner;
    if props.is_strikeout() {
        out = vec![Inline::Strikeout(out)];
    }
    if props.is_bold() {
        out = vec![Inline::Strong(out)];
    }
    if props.is_emph() {
        out = vec![Inline::Emph(out)];
    }
    match props.position() {
        Some(Position::Super) => vec![Inline::Superscript(out)],
        Some(Position::Sub) => vec![Inline::Subscript(out)],
        _ => out,
    }
}

/// The blocks of a list item or table cell, as pandoc shapes them: a single
/// paragraph is `Plain` rather than `Para`, which is what makes a tight
/// list tight.
fn tighten(mut blocks: Vec<Block>) -> Vec<Block> {
    if let [Block::Para(_)] = blocks.as_slice()
        && let Some(Block::Para(inlines)) = blocks.pop()
    {
        return vec![Block::Plain(inlines)];
    }
    blocks
}

fn empty_cell() -> Cell {
    Cell {
        attr: Attr::default(),
        alignment: Alignment::AlignDefault,
        row_span: 1,
        col_span: 1,
        blocks: Vec::new(),
    }
}

fn number_style(format: &str) -> ListNumberStyle {
    match format {
        "1" => ListNumberStyle::Decimal,
        "a" => ListNumberStyle::LowerAlpha,
        "A" => ListNumberStyle::UpperAlpha,
        "i" => ListNumberStyle::LowerRoman,
        "I" => ListNumberStyle::UpperRoman,
        _ => ListNumberStyle::DefaultStyle,
    }
}

fn delimiter(prefix: &str, suffix: &str) -> ListNumberDelim {
    match (prefix, suffix) {
        ("(", ")") => ListNumberDelim::TwoParens,
        (_, ")") => ListNumberDelim::OneParen,
        (_, ".") => ListNumberDelim::Period,
        _ => ListNumberDelim::DefaultDelim,
    }
}

/// All the text an element holds, descendants included.
fn node_text(node: &Node) -> String {
    fn walk(node: &Node, out: &mut String, depth: usize) {
        if depth >= MAX_NESTING {
            return;
        }
        for child in &node.children {
            match child {
                Child::Text(text) => out.push_str(text),
                Child::Elem(elem) => walk(elem, out, depth + 1),
            }
        }
    }
    let mut out = String::new();
    walk(node, &mut out, 0);
    out
}

/// The number of spaces a `text:s` stands for.
fn space_count(node: &Node) -> usize {
    node.attr("text:c")
        .and_then(|c| c.parse::<usize>().ok())
        .unwrap_or(1)
        .min(MAX_SPACES)
}

/// The URL an `xlink:href` names.
///
/// One leading `../` is stripped: a link inside `content.xml` points one
/// level out of the package, and pandoc's writer adds that step while its
/// reader takes it back. A second `../` is left standing, and so is a bare
/// `..`.
fn href(url: &str) -> String {
    url.strip_prefix("../").unwrap_or(url).to_owned()
}

/// The plain text of an inline sequence, for making an identifier out of a
/// heading.
fn plain_text(inlines: &[Inline]) -> String {
    fn walk(out: &mut String, inlines: &[Inline]) {
        for inline in inlines {
            match inline {
                Inline::Str(s) | Inline::Code(_, s) | Inline::Math(_, s) => out.push_str(s),
                Inline::Space | Inline::SoftBreak | Inline::LineBreak => out.push(' '),
                Inline::RawInline(..) | Inline::Note(_) => {}
                Inline::Emph(inner)
                | Inline::Underline(inner)
                | Inline::Strong(inner)
                | Inline::Strikeout(inner)
                | Inline::Superscript(inner)
                | Inline::Subscript(inner)
                | Inline::SmallCaps(inner)
                | Inline::Quoted(_, inner)
                | Inline::Cite(_, inner)
                | Inline::Span(_, inner)
                | Inline::Link(_, inner, _)
                | Inline::Image(_, inner, _) => walk(out, inner),
            }
        }
    }
    let mut out = String::new();
    walk(&mut out, inlines);
    out
}

/// The identifier form of a string, without the uniquing an identifier
/// needs — which is what an image title is put through.
fn slug(text: &str) -> String {
    let filtered: String = text
        .chars()
        .filter(|c| c.is_alphanumeric() || c.is_whitespace() || matches!(c, '_' | '-' | '.'))
        .flat_map(char::to_lowercase)
        .collect();
    filtered.split_whitespace().collect::<Vec<_>>().join("-")
}

/// Split text into the `Str`/`Space`/`SoftBreak` tokens pandoc produces: a
/// whitespace run containing a newline is a soft break, otherwise a space.
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

/// Append one child's inlines to what is built so far, merging **only at
/// the seam**.
///
/// This is pandoc's inline builder: appending two sequences melds the last
/// element of one with the first of the other and leaves everything else
/// alone. The distinction is not academic — a `text:s` asking for three
/// spaces arrives as three `Space`s and stays three, while the space ending
/// one text node and the space starting the next become one.
fn append(out: &mut Vec<Inline>, chunk: Vec<Inline>) {
    let mut chunk = chunk.into_iter();
    let Some(first) = chunk.next() else { return };
    meld(out, first);
    out.extend(chunk);
}

/// Push one inline, merging it into the previous one where pandoc does.
fn meld(out: &mut Vec<Inline>, next: Inline) {
    let space = |i: &Inline| matches!(i, Inline::Space | Inline::SoftBreak);
    match (out.last_mut(), next) {
        // Two runs of one style are one run — but their contents are only
        // concatenated, not melded in turn, so `<span/><span/>` around "a"
        // and "b" keeps two `Str`s inside the one `Strong`.
        (Some(Inline::Emph(inner)), Inline::Emph(next))
        | (Some(Inline::Strong(inner)), Inline::Strong(next))
        | (Some(Inline::Strikeout(inner)), Inline::Strikeout(next))
        | (Some(Inline::Subscript(inner)), Inline::Subscript(next))
        | (Some(Inline::Superscript(inner)), Inline::Superscript(next))
        | (Some(Inline::Underline(inner)), Inline::Underline(next)) => inner.extend(next),
        (Some(Inline::Str(text)), Inline::Str(next)) => text.push_str(&next),
        // A hard break absorbs the whitespace either side of it, and a soft
        // one absorbs a plain space.
        (Some(last), next) if space(last) && matches!(next, Inline::LineBreak) => {
            *last = Inline::LineBreak;
        }
        (Some(Inline::LineBreak), next) if space(&next) => {}
        (Some(last), next) if space(last) && space(&next) => {
            if matches!(next, Inline::SoftBreak) {
                *last = Inline::SoftBreak;
            }
        }
        (_, next) => out.push(next),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ferrodoc_ast::{Meta, MetaValue};
    use std::io::Write as _;

    /// Assemble a package around a body, and the styles it names.
    fn package(body: &str, automatic: &str, named: &str) -> Vec<u8> {
        let content = format!(
            concat!(
                r#"<?xml version="1.0" encoding="UTF-8"?>"#,
                r#"<office:document-content"#,
                r#" xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0""#,
                r#" xmlns:style="urn:oasis:names:tc:opendocument:xmlns:style:1.0""#,
                r#" xmlns:text="urn:oasis:names:tc:opendocument:xmlns:text:1.0""#,
                r#" xmlns:table="urn:oasis:names:tc:opendocument:xmlns:table:1.0""#,
                r#" xmlns:draw="urn:oasis:names:tc:opendocument:xmlns:drawing:1.0""#,
                r#" xmlns:fo="urn:oasis:names:tc:opendocument:xmlns:xsl-fo-compatible:1.0""#,
                r#" xmlns:xlink="http://www.w3.org/1999/xlink""#,
                r#" xmlns:svg="urn:oasis:names:tc:opendocument:xmlns:svg-compatible:1.0">"#,
                "<office:automatic-styles>{}</office:automatic-styles>",
                "<office:body><office:text>{}</office:text></office:body>",
                "</office:document-content>",
            ),
            automatic, body
        );
        let styles = format!(
            concat!(
                r#"<?xml version="1.0" encoding="UTF-8"?>"#,
                r#"<office:document-styles"#,
                r#" xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0""#,
                r#" xmlns:style="urn:oasis:names:tc:opendocument:xmlns:style:1.0""#,
                r#" xmlns:fo="urn:oasis:names:tc:opendocument:xmlns:xsl-fo-compatible:1.0">"#,
                "<office:styles>{}</office:styles>",
                "</office:document-styles>",
            ),
            named
        );
        let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        let options = zip::write::SimpleFileOptions::default();
        for (name, data) in [("content.xml", &content), ("styles.xml", &styles)] {
            zip.start_file(name, options).unwrap();
            zip.write_all(data.as_bytes()).unwrap();
        }
        zip.finish().unwrap().into_inner()
    }

    fn read_body(body: &str) -> Vec<Block> {
        read_odt(&package(body, "", "")).expect("readable").blocks
    }

    #[test]
    fn a_paragraph_is_a_paragraph_and_a_heading_takes_its_level() {
        let blocks = read_body(
            r#"<text:p>Body text.</text:p><text:h text:outline-level="3">A Head</text:h>"#,
        );
        let Block::Header(level, attr, _) = &blocks[1] else {
            panic!("expected a header, got {:?}", blocks[1])
        };
        assert_eq!(*level, 3);
        // Pandoc's `auto_identifiers`, which is where the identifier of an
        // ODF heading comes from: it carries none of its own.
        assert_eq!(attr.identifier, "a-head");
        assert!(matches!(blocks[0], Block::Para(_)));
    }

    #[test]
    fn a_paragraph_styled_like_a_heading_is_not_one() {
        // What LibreOffice writes when it imports an HTML `<h1>`: the style
        // says Heading 1 but the element is a paragraph, and pandoc reads
        // the element. Reading the style instead would turn body text with
        // a borrowed style into a section.
        let blocks = read_body(r#"<text:p text:style-name="Heading_20_1">Not a heading</text:p>"#);
        assert!(matches!(blocks[0], Block::Para(_)), "{:?}", blocks[0]);
    }

    #[test]
    fn the_block_quote_threshold_is_an_indent_not_a_style_name() {
        let style = |name: &str, margin: &str| {
            format!(
                r#"<style:style style:name="{name}" style:family="paragraph"><style:paragraph-properties fo:margin-left="{margin}"/></style:style>"#
            )
        };
        let styles = format!(
            "{}{}{}",
            style("Under", "5.4mm"),
            style("Over", "5.5mm"),
            r#"<style:style style:name="Quotations" style:family="paragraph"/>"#
        );
        let body = concat!(
            r#"<text:p text:style-name="Under">a</text:p>"#,
            r#"<text:p text:style-name="Over">b</text:p>"#,
            r#"<text:p text:style-name="Quotations">c</text:p>"#,
        );
        let blocks = read_odt(&package(body, &styles, "")).unwrap().blocks;
        assert!(matches!(blocks[0], Block::Para(_)), "5.4mm is not a quote");
        assert!(matches!(blocks[1], Block::BlockQuote(_)), "5.5mm is");
        // The name carries no meaning of its own: this Quotations has no
        // margin, so it is an ordinary paragraph.
        assert!(matches!(blocks[2], Block::Para(_)), "the name is not the rule");
    }

    #[test]
    fn a_quote_inside_a_list_item_is_a_paragraph() {
        // A list item is already indented, so the indent that means "quote"
        // outside one means nothing inside it.
        let styles = r#"<style:style style:name="Q" style:family="paragraph"><style:paragraph-properties fo:margin-left="1in"/></style:style>"#;
        let body = r#"<text:list><text:list-item><text:p text:style-name="Q">q</text:p></text:list-item></text:list>"#;
        let blocks = read_odt(&package(body, styles, "")).unwrap().blocks;
        let (Block::OrderedList(_, items) | Block::BulletList(items)) = &blocks[0] else {
            panic!("expected a list, got {:?}", blocks[0])
        };
        assert!(matches!(items[0][0], Block::Plain(_)), "{:?}", items[0][0]);
    }

    #[test]
    fn text_properties_are_not_inherited_but_paragraph_ones_are() {
        let styles = concat!(
            r#"<style:style style:name="B" style:family="text"><style:text-properties fo:font-weight="bold"/></style:style>"#,
            r#"<style:style style:name="BI" style:family="text" style:parent-style-name="B"><style:text-properties fo:font-style="italic"/></style:style>"#,
            r#"<style:style style:name="Deep" style:family="paragraph" style:parent-style-name="Mid"/>"#,
            r#"<style:style style:name="Mid" style:family="paragraph" style:parent-style-name="Wide"/>"#,
            r#"<style:style style:name="Wide" style:family="paragraph"><style:paragraph-properties fo:margin-left="1in"/></style:style>"#,
        );
        let body = concat!(
            r#"<text:p><text:span text:style-name="BI">x</text:span></text:p>"#,
            r#"<text:p text:style-name="Deep">y</text:p>"#,
        );
        let blocks = read_odt(&package(body, styles, "")).unwrap().blocks;
        // Bold is *not* inherited from the parent style: only the italic
        // the style declares itself survives.
        assert_eq!(
            blocks[0],
            Block::Para(vec![Inline::Emph(vec![Inline::Str("x".into())])])
        );
        // The margin, two links up the same kind of chain, *is*.
        assert!(matches!(blocks[1], Block::BlockQuote(_)), "{:?}", blocks[1]);
    }

    #[test]
    fn one_text_s_is_the_spaces_it_declares_and_they_do_not_collapse() {
        // The distinction the whole inline builder exists for: a run of
        // spaces from one element stays a run, while the space ending one
        // text node and the space starting the next become one.
        let blocks = read_body(r#"<text:p>a<text:s text:c="3"/>b c</text:p>"#);
        assert_eq!(
            blocks[0],
            Block::Para(vec![
                Inline::Str("a".into()),
                Inline::Space,
                Inline::Space,
                Inline::Space,
                Inline::Str("b".into()),
                Inline::Space,
                Inline::Str("c".into()),
            ])
        );
    }

    #[test]
    fn a_note_body_holds_paragraphs_and_nothing_else() {
        // Pandoc reads no list, heading or table inside a note; keeping one
        // would put content in our AST that its own reader never produces.
        let body = concat!(
            r#"<text:p>x<text:note text:note-class="footnote"><text:note-citation>1</text:note-citation>"#,
            r#"<text:note-body><text:p>kept</text:p>"#,
            r#"<text:list><text:list-item><text:p>dropped</text:p></text:list-item></text:list>"#,
            r#"</text:note-body></text:note></text:p>"#,
        );
        let blocks = read_body(body);
        let Block::Para(inlines) = &blocks[0] else { panic!() };
        let Some(Inline::Note(note)) = inlines.last() else { panic!("{inlines:?}") };
        assert_eq!(note.len(), 1, "only the paragraph survives: {note:?}");
    }

    #[test]
    fn a_covered_cell_shortens_its_row_rather_than_filling_it() {
        // The cell *after* a span moves left and the row is padded at the
        // end. Filling the covered position instead shifts every later cell
        // one column right, which is how a LibreOffice table came apart.
        let body = concat!(
            r#"<table:table table:name="T">"#,
            r#"<table:table-row><table:table-cell><text:p>a</text:p></table:table-cell>"#,
            r#"<table:table-cell><text:p>b</text:p></table:table-cell>"#,
            r#"<table:table-cell><text:p>c</text:p></table:table-cell></table:table-row>"#,
            r#"<table:table-row><table:table-cell table:number-columns-spanned="2"><text:p>wide</text:p></table:table-cell>"#,
            r#"<table:covered-table-cell/>"#,
            r#"<table:table-cell><text:p>last</text:p></table:table-cell></table:table-row>"#,
            r#"</table:table>"#,
        );
        let blocks = read_body(body);
        let Block::Table(table) = &blocks[0] else { panic!("{:?}", blocks[0]) };
        let row = &table.bodies[0].body[1];
        assert_eq!(row.cells.len(), 3);
        assert_eq!(row.cells[1].blocks, vec![Block::Plain(vec![Inline::Str("last".into())])]);
        assert!(row.cells[2].blocks.is_empty(), "the padding goes at the end");
    }

    #[test]
    fn a_bookmark_is_an_anchor_and_a_heading_can_take_its_name_back() {
        // Every bookmark is called `anchor`, and the identifier set pandoc
        // uniquifies against is the *values* of the bookmark map — so a
        // heading whose identifier equals a bookmark's name rebinds it and
        // frees `anchor` for the next one.
        let heading = |name: &str, text: &str| {
            format!(
                r#"<text:h text:outline-level="1"><text:bookmark-start text:name="{name}"/>{text}</text:h>"#
            )
        };
        let blocks = read_body(&format!("{}{}", heading("one", "One"), heading("b", "Two")));
        let anchor = |block: &Block| match block {
            Block::Header(_, _, inlines) => match inlines.first() {
                Some(Inline::Span(attr, _)) => attr.identifier.clone(),
                other => panic!("expected an anchor span, got {other:?}"),
            },
            other => panic!("expected a header, got {other:?}"),
        };
        assert_eq!(anchor(&blocks[0]), "anchor");
        assert_eq!(anchor(&blocks[1]), "anchor");
    }

    #[test]
    fn a_malformed_body_is_refused_not_truncated() {
        for body in [
            r"<text:p>unclosed",
            r"<text:p>ok</text:p><text:p attr=>",
        ] {
            assert!(read_odt(&package(body, "", "")).is_err(), "accepted {body}");
        }
        // And a package with no content part at all.
        assert!(read_odt(b"not a zip").is_err());
    }

    #[test]
    fn the_package_starts_with_a_stored_mimetype_entry() {
        // How every ODF consumer identifies the file without unzipping it.
        // Deflating it makes LibreOffice refuse the package outright.
        let bytes = write_odt(&Pandoc::new(vec![Block::Para(vec![Inline::Str("x".into())])])).unwrap();
        let mut zip = zip::ZipArchive::new(std::io::Cursor::new(&bytes)).unwrap();
        let first = zip.by_index(0).unwrap();
        assert_eq!(first.name(), "mimetype");
        assert_eq!(first.compression(), zip::CompressionMethod::Stored);
    }

    #[test]
    fn nested_formatting_is_written_as_one_flat_span() {
        // Nesting the spans instead reads back as `Emph [Strong [Emph …]]`,
        // because the reader applies the whole accumulated property set at
        // every level it meets one.
        let doc = Pandoc::new(vec![Block::Para(vec![Inline::Emph(vec![Inline::Strong(
            vec![Inline::Str("x".into())],
        )])])]);
        let bytes = write_odt(&doc).unwrap();
        let back = read_odt(&bytes).unwrap();
        assert_eq!(back.blocks, doc.blocks);
    }

    #[test]
    fn an_indented_code_line_keeps_its_indentation() {
        // ODF collapses a run of spaces in text, so the indentation has to
        // travel as `text:s` — and it is most of what a code sample says.
        let doc = Pandoc::new(vec![Block::CodeBlock(
            Attr::default(),
            "def f():\n    return 1".into(),
        )]);
        let bytes = write_odt(&doc).unwrap();
        // Pandoc's ODT reader has no code blocks at all, so what comes back
        // is paragraphs — the indentation is what is being checked.
        let back = read_odt(&bytes).unwrap();
        let Block::Para(second) = &back.blocks[1] else { panic!("{:?}", back.blocks) };
        let indent = second.iter().take_while(|i| **i == Inline::Space).count();
        assert_eq!(indent, 4, "{second:?}");
    }

    #[test]
    fn a_list_style_declares_the_level_its_list_is_nested_at() {
        // A style that stops at level one makes a word processor number a
        // nested bullet list: it takes the marker for the level matching
        // the list's depth, and falls back to its own default without one.
        let doc = Pandoc::new(vec![Block::BulletList(vec![vec![
            Block::Plain(vec![Inline::Str("a".into())]),
            Block::BulletList(vec![vec![Block::Plain(vec![Inline::Str("b".into())])]]),
        ]])]);
        let bytes = write_odt(&doc).unwrap();
        let content = read_part(&bytes, "content.xml");
        assert!(
            content.contains(r#"text:level="2""#),
            "the nested list declares no level 2: {content}"
        );
        // The nesting survives the round trip; the item's first block turns
        // from `Plain` into `Para` because it is no longer the only one,
        // which is what pandoc's reader does with it too.
        let blocks = read_odt(&bytes).unwrap().blocks;
        let Block::BulletList(items) = &blocks[0] else { panic!("{blocks:?}") };
        assert!(matches!(items[0][1], Block::BulletList(_)), "{:?}", items[0]);
    }

    #[test]
    fn odt_to_odt_keeps_its_pictures_byte_for_byte() {
        let png = one_pixel_png();
        let doc = Pandoc::new(vec![Block::Para(vec![Inline::Image(
            Box::default(),
            vec![Inline::Str("alt".into())],
            Box::new(Target { url: "swatch.png".into(), title: String::new() }),
        )])]);
        let written = write_odt_with_media(&doc, &|_| Some(png.clone())).unwrap();
        let (back, media) = read_odt_with_media(&written).unwrap();
        assert_eq!(media.len(), 1, "the picture was not collected");
        let again = write_odt_with_media(&back, &|url| media.get(url).cloned()).unwrap();
        let (_, media_again) = read_odt_with_media(&again).unwrap();
        assert_eq!(
            media_again.values().next(),
            Some(&png),
            "the bytes changed on the way through"
        );
    }

    #[test]
    fn an_unembeddable_image_falls_back_to_its_alt_text() {
        let doc = Pandoc::new(vec![Block::Para(vec![Inline::Image(
            Box::default(),
            vec![Inline::Str("alt".into())],
            Box::new(Target { url: "nowhere.png".into(), title: String::new() }),
        )])]);
        let bytes = write_odt(&doc).unwrap();
        let back = read_odt(&bytes).unwrap();
        // Emphasized, which is how pandoc marks a substituted picture.
        assert_eq!(
            back.blocks,
            vec![Block::Para(vec![Inline::Emph(vec![Inline::Str("alt".into())])])]
        );
    }

    #[test]
    fn every_style_the_content_names_is_declared_somewhere() {
        // A style name with no definition behind it is silently ignored by
        // every consumer, so a typo loses the formatting with no error.
        let mut meta = Meta::new();
        meta.insert("title".into(), MetaValue::MetaString("T".into()));
        let doc = Pandoc {
            meta,
            blocks: vec![
                Block::Header(1, Attr::default(), vec![Inline::Str("H".into())]),
                Block::BlockQuote(vec![Block::CodeBlock(Attr::default(), "x".into())]),
                Block::BulletList(vec![vec![Block::Plain(vec![Inline::Code(
                    Box::default(),
                    "c".into(),
                )])]]),
                Block::DefinitionList(vec![(
                    vec![Inline::Str("t".into())],
                    vec![vec![Block::Para(vec![Inline::Str("d".into())])]],
                )]),
                Block::HorizontalRule,
            ],
            ..Pandoc::default()
        };
        let bytes = write_odt(&doc).unwrap();
        let content = read_part(&bytes, "content.xml");
        let styles = read_part(&bytes, "styles.xml");
        let all = format!("{content}{styles}");
        for (at, prefix) in content.match_indices("text:style-name=\"") {
            let name = content[at + prefix.len()..]
                .split('"')
                .next()
                .unwrap_or_default();
            assert!(
                all.contains(&format!("style:name=\"{name}\"")),
                "the content names {name}, and nothing declares it"
            );
        }
    }

    fn read_part(bytes: &[u8], name: &str) -> String {
        use std::io::Read as _;
        let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
        let mut file = zip.by_name(name).unwrap();
        let mut out = String::new();
        file.read_to_string(&mut out).unwrap();
        out
    }

    fn one_pixel_png() -> Vec<u8> {
        // A real, decodable PNG: `media::inspect` refuses anything else,
        // and a refused image is written as alt text instead.
        const PNG: &[u8] = &[
            0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a, 0, 0, 0, 0x0d, b'I', b'H', b'D', b'R',
            0, 0, 0, 1, 0, 0, 0, 1, 8, 2, 0, 0, 0, 0x90, 0x77, 0x53, 0xde, 0, 0, 0, 0x0c, b'I',
            b'D', b'A', b'T', 0x08, 0xd7, 0x63, 0xf8, 0xcf, 0xc0, 0x00, 0x00, 0x03, 0x01, 0x01,
            0x00, 0x18, 0xdd, 0x8d, 0xb0, 0, 0, 0, 0, b'I', b'E', b'N', b'D', 0xae, 0x42, 0x60,
            0x82,
        ];
        PNG.to_vec()
    }
}
