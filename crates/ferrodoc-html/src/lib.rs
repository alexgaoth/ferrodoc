//! HTML reader and writer for the ferrodoc (pandoc-compatible) AST.
//!
//! [`read_html`] parses a page into the AST; [`write_html`] renders the AST
//! back out. The reader is structural — it is not a CSS engine — and is
//! verified against `pandoc -f html -t json` by `ferrodoc-harness
//! diff-html-read`.
//!
//! [`write_html`] emits the same HTML as
//! `pandoc -f commonmark -t html --syntax-highlighting=none --wrap=none`
//! for every construct reachable from the commonmark reader (verified
//! differentially by `ferrodoc-harness diff-html`). Constructs the
//! commonmark reader cannot produce (tables, figures, notes, …) get
//! reasonable pandoc-shaped output but are not differentially verified;
//! `Note` and non-HTML raw content are dropped, like pandoc's HTML writer
//! does for raw content it cannot place.

mod page;
mod read;
mod template;

pub use page::{Page, write_page};
pub use read::{MAX_NESTING, read_html, read_html_without_generated_identifiers};

/// What can go wrong reading HTML.
#[derive(Debug, PartialEq, Eq)]
pub enum Error {
    /// The element tree nests deeper than [`MAX_NESTING`].
    TooDeep,
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TooDeep => write!(f, "html nests deeper than {MAX_NESTING} levels"),
        }
    }
}

impl std::error::Error for Error {}

use ferrodoc_ast::{
    Alignment, Attr, Block, Caption, Cell, ColSpec, Inline, ListNumberStyle, Pandoc, Row, Table,
};
use std::fmt::Write as _;

/// Render a document as HTML, matching pandoc's HTML writer with
/// `--wrap=none` and no syntax highlighting.
pub fn write_html(doc: &Pandoc) -> String {
    let mut out = String::new();
    write_blocks(&mut out, &doc.blocks);
    if out.is_empty() {
        out.push('\n'); // pandoc's output always ends with a newline
    }
    // A document-final raw block would otherwise leave a blank last line;
    // pandoc ends the document with exactly one newline.
    if out.ends_with("\n\n") {
        out.pop();
    }
    out
}

/// How deep the contents go, matching **pandoc's `--toc-depth` default**
/// rather than anything inherent to the format: `pandoc --toc-depth=4`
/// disagrees with this, which is how a reader can check that claim.
const TOC_DEPTH: i64 = 3;

/// Number the headings, as pandoc's `--number-sections` does.
///
/// Probed against pandoc 3.8.2.1, with `--wrap=none` so no rule here comes
/// from a line pandoc happened to break:
///
/// - the header gains a `number` attribute, which this writer emits as
///   `data-number` **before** the `id`, and a leading
///   `<span class="header-section-number">` followed by a space;
/// - a header whose classes contain `unnumbered` is skipped and keeps the
///   class — pandoc emits neither the attribute nor the span for it;
/// - a number has one component per level **from the document's shallowest
///   heading down**, which is not the same as one per absolute level: a
///   document whose headings are all `##` numbers `1`, `2`, while one that
///   mixes `##` with a `#` numbers the `##` as `0.1`. An `unnumbered`
///   heading still counts toward that shallowest level, and a heading
///   inside a `Div` does too;
/// - headings inside a `Div` are numbered; headings inside a `BlockQuote`
///   are **not**, and neither reach the contents.
pub fn number_sections(doc: &mut Pandoc) {
    let Some(base) = shallowest_header(&doc.blocks) else {
        return;
    };
    let mut counters = [0usize; 6];
    number_blocks(&mut doc.blocks, &mut counters, base);
}

/// The shallowest heading level anywhere numbering reaches, which is where
/// a section number's first component comes from.
fn shallowest_header(blocks: &[Block]) -> Option<usize> {
    let mut shallowest: Option<usize> = None;
    for block in blocks {
        let depth = match block {
            Block::Header(level, _, _) => usize::try_from(*level).unwrap_or(1).clamp(1, 6),
            Block::Div(_, blocks) => match shallowest_header(blocks) {
                Some(depth) => depth,
                None => continue,
            },
            _ => continue,
        };
        shallowest = Some(shallowest.map_or(depth, |current: usize| current.min(depth)));
    }
    shallowest
}

fn number_blocks(blocks: &mut [Block], counters: &mut [usize; 6], base: usize) {
    for block in blocks {
        match block {
            Block::Header(level, attr, inlines) => {
                if attr.classes.iter().any(|class| class == "unnumbered") {
                    continue;
                }
                let depth = usize::try_from(*level).unwrap_or(1).clamp(1, 6);
                counters[depth - 1] += 1;
                for counter in &mut counters[depth..] {
                    *counter = 0;
                }
                let number = counters[base - 1..depth.max(base)]
                    .iter()
                    .map(usize::to_string)
                    .collect::<Vec<_>>()
                    .join(".");
                attr.attributes.push(("number".to_owned(), number.clone()));
                // Built in one pass rather than two `insert(0, …)` calls,
                // which is the quadratic shape this repo keeps re-finding.
                let mut numbered = Vec::with_capacity(inlines.len() + 2);
                numbered.push(Inline::Span(
                    Box::new(Attr {
                        identifier: String::new(),
                        classes: vec!["header-section-number".to_owned()],
                        attributes: Vec::new(),
                    }),
                    vec![Inline::Str(number)],
                ));
                numbered.push(Inline::Space);
                numbered.append(inlines);
                *inlines = numbered;
            }
            Block::Div(_, blocks) => number_blocks(blocks, counters, base),
            _ => {}
        }
    }
}

/// The `<nav id="TOC" role="doc-toc">` block pandoc's `--toc` writes.
///
/// Empty when the document has no heading within [`TOC_DEPTH`]: pandoc
/// writes no element at all rather than an empty one.
///
/// Two rules are easy to get wrong and both are probed. **Nesting is
/// relative, not absolute**: `#`, then `###`, then `##` puts the last two
/// as siblings one level in, because a jump opens exactly one list. And a
/// heading with **no identifier gets no link** — its text goes into the
/// item bare, which is what `-f commonmark` produces, where headings carry
/// no identifiers at all.
pub fn write_toc(doc: &Pandoc) -> String {
    write_toc_to_depth(doc, TOC_DEPTH)
}

/// The same, to a chosen depth — pandoc's `--toc-depth`.
pub fn write_toc_to_depth(doc: &Pandoc, depth: i64) -> String {
    let mut entries = Vec::new();
    collect_toc(&doc.blocks, depth, &mut entries);
    if entries.is_empty() {
        return String::new();
    }
    let mut roots: Vec<TocNode> = Vec::new();
    // Indices into the tree, one per open level, with the level beside it.
    let mut path: Vec<(i64, usize)> = Vec::new();
    for (level, html) in entries {
        while path.last().is_some_and(|(open, _)| *open >= level) {
            path.pop();
        }
        let node = TocNode { html, children: Vec::new() };
        let mut siblings = &mut roots;
        for (_, index) in &path {
            siblings = &mut siblings[*index].children;
        }
        siblings.push(node);
        path.push((level, siblings.len() - 1));
    }
    format!("<nav id=\"TOC\" role=\"doc-toc\">\n{}\n</nav>\n", write_toc_list(&roots))
}

/// The contents **without** its `<nav>`, which is what the page template
/// wraps for itself. Emitting the wrapper here as well is how `-s --toc`
/// produced two of them.
pub fn toc_list_to_depth(doc: &Pandoc, depth: i64, id_prefix: &str) -> String {
    let nav = write_toc_to_depth(doc, depth);
    // The entry ids carry the prefix **before** the `toc-`, which is
    // pandoc's spelling: `id="p-toc-x"` beside `href="#p-x"`. The tree's
    // identifiers already carry it by the time this runs, so the prefix
    // is *moved* rather than added — adding gave `p-toc-p-x`.
    let nav = if id_prefix.is_empty() {
        nav
    } else {
        nav.replace(
            &format!("\" id=\"toc-{id_prefix}"),
            &format!("\" id=\"{id_prefix}toc-"),
        )
    };
    nav.strip_prefix("<nav id=\"TOC\" role=\"doc-toc\">\n")
        .and_then(|rest| rest.strip_suffix("\n</nav>\n"))
        .map(str::to_owned)
        .unwrap_or(nav)
}

struct TocNode {
    html: String,
    children: Vec<TocNode>,
}

/// `<ul>` … `</ul>`, with no trailing newline: a nested list closes as
/// `</ul></li>` on one line, which is pandoc's shape.
fn write_toc_list(nodes: &[TocNode]) -> String {
    let mut out = String::from("<ul>\n");
    for node in nodes {
        out.push_str("<li>");
        out.push_str(&node.html);
        if !node.children.is_empty() {
            out.push('\n');
            out.push_str(&write_toc_list(&node.children));
        }
        out.push_str("</li>\n");
    }
    out.push_str("</ul>");
    out
}

/// Every heading within [`TOC_DEPTH`], as `(level, rendered entry)`.
fn collect_toc(blocks: &[Block], depth: i64, out: &mut Vec<(i64, String)>) {
    for block in blocks {
        match block {
            Block::Header(level, attr, inlines) => {
                if *level > depth {
                    continue;
                }
                let mut html = String::new();
                if !attr.identifier.is_empty() {
                    html.push_str("<a href=\"#");
                    escape_attribute(&mut html, &attr.identifier);
                    html.push_str("\" id=\"toc-");
                    escape_attribute(&mut html, &attr.identifier);
                    html.push_str("\">");
                }
                // A numbered document has already had its section number
                // put into the heading as a `header-section-number` span;
                // the contents carry the same number under a different
                // class, so that span is replaced rather than repeated.
                let mut inlines = inlines.as_slice();
                if let Some(number) = attr.attributes.iter().find(|(key, _)| key == "number") {
                    html.push_str("<span class=\"toc-section-number\">");
                    escape_text(&mut html, &number.1);
                    html.push_str("</span> ");
                    if let [Inline::Span(attr, _), Inline::Space, rest @ ..] = inlines {
                        if attr.classes.iter().any(|class| class == "header-section-number") {
                            inlines = rest;
                        }
                    }
                }
                write_inlines(&mut html, inlines);
                if !attr.identifier.is_empty() {
                    html.push_str("</a>");
                }
                out.push((*level, html));
            }
            Block::Div(_, blocks) => collect_toc(blocks, depth, out),
            _ => {}
        }
    }
}

/// A metadata value as plain text, however it was spelled.
fn meta_text(doc: &Pandoc, key: &str) -> Option<String> {
    meta_texts(doc, key).into_iter().next()
}

/// Every value under `key`: a metadata field may be one value or a list,
/// and `author` routinely is a list.
fn meta_texts(doc: &Pandoc, key: &str) -> Vec<String> {
    fn flatten(value: &ferrodoc_ast::MetaValue, out: &mut Vec<String>) {
        match value {
            ferrodoc_ast::MetaValue::MetaString(s) => out.push(s.clone()),
            ferrodoc_ast::MetaValue::MetaInlines(inlines) => out.push(plain_text(inlines)),
            ferrodoc_ast::MetaValue::MetaBlocks(blocks) => {
                for block in blocks {
                    if let Block::Plain(inlines) | Block::Para(inlines) = block {
                        out.push(plain_text(inlines));
                    }
                }
            }
            ferrodoc_ast::MetaValue::MetaList(values) => {
                for value in values {
                    flatten(value, out);
                }
            }
            ferrodoc_ast::MetaValue::MetaBool(_) | ferrodoc_ast::MetaValue::MetaMap(_) => {}
        }
    }
    let mut out = Vec::new();
    if let Some(value) = doc.meta.get(key) {
        flatten(value, &mut out);
    }
    out.retain(|text| !text.is_empty());
    out
}

fn write_blocks(out: &mut String, blocks: &[Block]) {
    for block in blocks {
        write_block(out, block);
        out.push('\n');
    }
}

/// Like [`write_blocks`] but without the trailing newline after the last
/// block — the form used inside container elements.
fn write_blocks_joined(out: &mut String, blocks: &[Block]) {
    let mut first = true;
    for block in blocks {
        if !first {
            out.push('\n');
        }
        first = false;
        write_block(out, block);
    }
}

fn write_block(out: &mut String, block: &Block) {
    match block {
        Block::Plain(inlines) => write_inlines(out, inlines),
        Block::Para(inlines) => {
            out.push_str("<p>");
            write_inlines(out, inlines);
            out.push_str("</p>");
        }
        Block::Header(level, attr, inlines) => {
            let _ = write!(out, "<h{level}");
            write_header_attr(out, attr);
            out.push('>');
            write_inlines(out, inlines);
            let _ = write!(out, "</h{level}>");
        }
        Block::CodeBlock(attr, text) => {
            out.push_str("<pre");
            write_attr(out, attr);
            out.push_str("><code>");
            escape_code_block(out, text);
            out.push_str("</code></pre>");
        }
        Block::RawBlock(format, text) => {
            if format.0 == "html" {
                // The literal keeps its own trailing newline; with the block
                // separator this yields the blank line pandoc emits after
                // raw blocks.
                out.push_str(text);
            }
        }
        Block::BlockQuote(blocks) => {
            out.push_str("<blockquote>\n");
            write_blocks_joined(out, blocks);
            out.push_str("\n</blockquote>");
        }
        Block::BulletList(items) => {
            // Pandoc classes a bullet list only when every one of its items
            // opens with a box: a mixed list gets no class yet still gets the
            // boxes on the items that have one, and an `<ol>` never gets the
            // class however many of its items are task items. An empty list
            // takes the class, the same vacuous way pandoc's does.
            if items.iter().all(|item| item.first().and_then(task_box).is_some()) {
                out.push_str("<ul class=\"task-list\">\n");
            } else {
                out.push_str("<ul>\n");
            }
            write_list_items(out, items);
            out.push_str("</ul>");
        }
        Block::OrderedList(attrs, items) => {
            out.push_str("<ol");
            if attrs.start != 1 {
                let _ = write!(out, " start=\"{}\"", attrs.start);
            }
            if let Some(t) = list_type(attrs.style) {
                let _ = write!(out, " type=\"{t}\"");
            }
            out.push_str(">\n");
            write_list_items(out, items);
            out.push_str("</ol>");
        }
        Block::DefinitionList(items) => {
            out.push_str("<dl>\n");
            for (term, definitions) in items {
                out.push_str("<dt>");
                write_inlines(out, term);
                out.push_str("</dt>\n");
                for definition in definitions {
                    out.push_str("<dd>\n");
                    write_blocks_joined(out, definition);
                    out.push_str("\n</dd>\n");
                }
            }
            out.push_str("</dl>");
        }
        Block::HorizontalRule => out.push_str("<hr />"),
        Block::LineBlock(lines) => {
            out.push_str("<div class=\"line-block\">");
            let mut first = true;
            for line in lines {
                if !first {
                    out.push_str("<br />\n");
                }
                first = false;
                write_inlines(out, line);
            }
            out.push_str("</div>");
        }
        Block::Div(attr, blocks) => {
            out.push_str("<div");
            write_attr(out, attr);
            out.push_str(">\n");
            write_blocks_joined(out, blocks);
            out.push_str("\n</div>");
        }
        Block::Figure(attr, caption, blocks) => {
            out.push_str("<figure");
            write_attr(out, attr);
            out.push_str(">\n");
            write_blocks_joined(out, blocks);
            write_figcaption(out, caption);
            out.push_str("\n</figure>");
        }
        Block::Table(table) => write_table(out, table),
    }
}

fn write_list_items(out: &mut String, items: &[Vec<Block>]) {
    for item in items {
        out.push_str("<li>");
        match item.first().and_then(task_box) {
            // The box and the space after it become the `<input>`, the rest
            // of that first block goes in the `<label>` beside it, and the
            // item's remaining blocks stand as they are.
            Some((checked, label)) => {
                let para = matches!(item[0], Block::Para(_));
                if para {
                    out.push_str("<p>");
                }
                out.push_str("<label><input type=\"checkbox\"");
                if checked {
                    out.push_str(" checked=\"\"");
                }
                out.push_str(" />");
                write_inlines(out, label);
                out.push_str("</label>");
                if para {
                    out.push_str("</p>");
                }
                for block in &item[1..] {
                    out.push('\n');
                    write_block(out, block);
                }
            }
            None => write_blocks_joined(out, item),
        }
        out.push_str("</li>\n");
    }
}

/// A task-list box opening a list item's first block: whether it is ticked,
/// and the inlines that follow it. Pandoc's HTML writer takes only a
/// `Str "☒"`/`Str "☐"` immediately followed by a `Space` at the head of a
/// `Plain` or `Para`; a `SoftBreak` in the space's place, a box with nothing
/// after it, a box inside an `Emph`, and a box in a `Str` of its own with the
/// space attached all stay literal text.
fn task_box(block: &Block) -> Option<(bool, &[Inline])> {
    let (Block::Plain(inlines) | Block::Para(inlines)) = block else {
        return None;
    };
    let checked = match inlines.first()? {
        Inline::Str(marker) if marker == "\u{2610}" => false,
        Inline::Str(marker) if marker == "\u{2612}" => true,
        _ => return None,
    };
    matches!(inlines.get(1), Some(Inline::Space)).then(|| (checked, &inlines[2..]))
}

fn list_type(style: ListNumberStyle) -> Option<&'static str> {
    match style {
        ListNumberStyle::Decimal => Some("1"),
        ListNumberStyle::LowerAlpha => Some("a"),
        ListNumberStyle::UpperAlpha => Some("A"),
        ListNumberStyle::LowerRoman => Some("i"),
        ListNumberStyle::UpperRoman => Some("I"),
        ListNumberStyle::DefaultStyle | ListNumberStyle::Example => None,
    }
}

fn write_figcaption(out: &mut String, caption: &Caption) {
    if caption.blocks.is_empty() {
        return;
    }
    out.push_str("\n<figcaption>");
    write_blocks_joined(out, &caption.blocks);
    out.push_str("</figcaption>");
}

fn write_table(out: &mut String, table: &Table) {
    out.push_str("<table");
    write_attr(out, &table.attr);
    // A table whose columns carry relative widths says so on the element,
    // and only when they add up to less than the full width. Measured
    // against pandoc 3.8.2.1: the *table* total is rounded, each *column*
    // is truncated — 0.335 is a 33% column inside a 67% table.
    let total: f64 = table.colspecs.iter().filter_map(|c| c.width.fraction()).sum();
    if !table.colspecs.is_empty() && total > 0.0 && total < 1.0 {
        let _ = write!(out, " style=\"width:{}%;\"", percent((total * 100.0).round()));
    }
    out.push('>');
    if !table.caption.blocks.is_empty() {
        out.push_str("\n<caption>");
        write_blocks_joined(out, &table.caption.blocks);
        out.push_str("</caption>");
    }
    // The column widths a word processor set. Dropping them made every
    // converted table equal-width — the DOCX reader had the numbers, and
    // `diff-html` could not see it because its corpus is the CommonMark
    // spec, which has no tables.
    if table.colspecs.iter().any(|c| c.width.fraction().is_some()) {
        out.push_str("\n<colgroup>");
        for colspec in &table.colspecs {
            match colspec.width.fraction() {
                Some(width) => {
                    let _ = write!(out, "\n<col style=\"width: {}%\" />", percent(width * 100.0));
                }
                None => out.push_str("\n<col />"),
            }
        }
        out.push_str("\n</colgroup>");
    }
    if !table.head.rows.is_empty() {
        out.push_str("\n<thead>");
        for row in &table.head.rows {
            write_table_row(out, row, "th", &table.colspecs);
        }
        out.push_str("\n</thead>");
    }
    for body in &table.bodies {
        out.push_str("\n<tbody>");
        for row in body.head.iter().chain(&body.body) {
            write_table_row(out, row, "td", &table.colspecs);
        }
        out.push_str("\n</tbody>");
    }
    if !table.foot.rows.is_empty() {
        out.push_str("\n<tfoot>");
        for row in &table.foot.rows {
            write_table_row(out, row, "td", &table.colspecs);
        }
        out.push_str("\n</tfoot>");
    }
    out.push_str("\n</table>");
}

fn write_table_row(out: &mut String, row: &Row, cell_tag: &str, colspecs: &[ColSpec]) {
    out.push_str("\n<tr>");
    // The column a cell sits in is its position *after* the spans before
    // it, which is what makes the column's alignment findable.
    let mut column = 0usize;
    for cell in &row.cells {
        write_table_cell(out, cell, cell_tag, colspecs.get(column));
        column += usize::try_from(cell.col_span).unwrap_or(1).max(1);
    }
    out.push_str("\n</tr>");
}

fn write_table_cell(out: &mut String, cell: &Cell, tag: &str, colspec: Option<&ColSpec>) {
    let _ = write!(out, "\n<{tag}");
    if cell.row_span != 1 {
        let _ = write!(out, " rowspan=\"{}\"", cell.row_span);
    }
    if cell.col_span != 1 {
        let _ = write!(out, " colspan=\"{}\"", cell.col_span);
    }
    // A cell's own alignment wins, and almost no cell has one: pandoc
    // keeps table alignment in the **column specs**, so a `|---:|` header
    // leaves every cell `AlignDefault` and the column holding the answer.
    // Reading only the cell dropped the alignment of every markdown and
    // HTML table — invisible to `diff-html`, whose corpus is the
    // CommonMark spec, which has no tables in it at all.
    let alignment = match cell.alignment {
        Alignment::AlignDefault => colspec.map_or(Alignment::AlignDefault, |c| c.alignment),
        explicit => explicit,
    };
    if let Some(align) = alignment_style(alignment) {
        let _ = write!(out, " style=\"text-align: {align};\"");
    }
    out.push('>');
    write_blocks_joined(out, &cell.blocks);
    let _ = write!(out, "</{tag}>");
}

/// A scaled width as whole percent. The caller rounds or truncates first
/// — pandoc does both, in different places, and the difference is visible
/// in the output.
#[expect(clippy::cast_possible_truncation, reason = "a percentage, and the truncation is the rule")]
fn percent(scaled: f64) -> i64 {
    scaled as i64
}

fn alignment_style(alignment: Alignment) -> Option<&'static str> {
    match alignment {
        Alignment::AlignLeft => Some("left"),
        Alignment::AlignRight => Some("right"),
        Alignment::AlignCenter => Some("center"),
        Alignment::AlignDefault => None,
    }
}

fn write_inlines(out: &mut String, inlines: &[Inline]) {
    for inline in inlines {
        write_inline(out, inline);
    }
}

fn write_inline(out: &mut String, inline: &Inline) {
    match inline {
        Inline::Str(s) => escape_text(out, s),
        Inline::Space | Inline::SoftBreak => out.push(' '),
        Inline::LineBreak => out.push_str("<br />\n"),
        Inline::Emph(inner) => wrap_tag(out, "em", inner),
        Inline::Strong(inner) => wrap_tag(out, "strong", inner),
        Inline::Strikeout(inner) => wrap_tag(out, "del", inner),
        Inline::Superscript(inner) => wrap_tag(out, "sup", inner),
        Inline::Subscript(inner) => wrap_tag(out, "sub", inner),
        Inline::Underline(inner) => wrap_tag(out, "u", inner),
        Inline::SmallCaps(inner) => {
            out.push_str("<span class=\"smallcaps\">");
            write_inlines(out, inner);
            out.push_str("</span>");
        }
        Inline::Quoted(quote_type, inner) => {
            use ferrodoc_ast::QuoteType;
            let (open, close) = match quote_type {
                QuoteType::SingleQuote => ('\u{2018}', '\u{2019}'),
                QuoteType::DoubleQuote => ('\u{201C}', '\u{201D}'),
            };
            out.push(open);
            write_inlines(out, inner);
            out.push(close);
        }
        Inline::Code(attr, text) => {
            out.push_str("<code");
            write_attr(out, attr);
            out.push('>');
            escape_text(out, text);
            out.push_str("</code>");
        }
        Inline::Math(math_type, text) => {
            use ferrodoc_ast::MathType;
            let (class, open, close) = match math_type {
                MathType::InlineMath => ("math inline", "\\(", "\\)"),
                MathType::DisplayMath => ("math display", "\\[", "\\]"),
            };
            let _ = write!(out, "<span class=\"{class}\">{open}");
            escape_text(out, text);
            let _ = write!(out, "{close}</span>");
        }
        Inline::RawInline(format, text) => {
            if format.0 == "html" {
                out.push_str(text);
            }
        }
        Inline::Link(attr, inner, target) => {
            out.push_str("<a href=\"");
            escape_attribute(out, &target.url);
            out.push('"');
            if !target.title.is_empty() {
                out.push_str(" title=\"");
                escape_attribute(out, &target.title);
                out.push('"');
            }
            write_attr(out, attr);
            out.push('>');
            write_inlines(out, inner);
            out.push_str("</a>");
        }
        Inline::Image(attr, alt, target) => {
            out.push_str("<img src=\"");
            escape_attribute(out, &target.url);
            out.push('"');
            if !target.title.is_empty() {
                out.push_str(" title=\"");
                escape_attribute(out, &target.title);
                out.push('"');
            }
            // Pandoc omits the alt attribute only when the alt inlines are
            // empty (`![](url)`); non-empty inlines that render to empty
            // text still produce alt="".
            if !alt.is_empty() {
                out.push_str(" alt=\"");
                escape_attribute(out, &plain_text(alt));
                out.push('"');
            }
            write_attr(out, attr);
            out.push_str(" />");
        }
        Inline::Span(attr, inner) => {
            out.push_str("<span");
            write_attr(out, attr);
            out.push('>');
            write_inlines(out, inner);
            out.push_str("</span>");
        }
        Inline::Cite(_, inner) => write_inlines(out, inner),
        Inline::Note(_) => {}
    }
}

fn wrap_tag(out: &mut String, tag: &str, inner: &[Inline]) {
    let _ = write!(out, "<{tag}>");
    write_inlines(out, inner);
    let _ = write!(out, "</{tag}>");
}

/// A heading's attributes, which are **not** in the order every other
/// element's are: the identifier comes last, and a section number comes
/// first among the key-values. Probed against pandoc 3.8.2.1 with
/// `--wrap=none`, on `# H {#i .foo data-k=v}`:
///
/// ```text
/// pandoc                       <h1 class="foo" data-k="v" id="i">H</h1>
/// pandoc --number-sections     <h1 class="foo" data-number="1" data-k="v" id="i">…
/// ```
///
/// A `Div` with the same attributes gets `id` first, so this is a heading
/// rule rather than a document-wide one. No gate reached it before
/// `--number-sections` existed: the `CommonMark` spec's headings carry no
/// attributes at all, so `diff-html` scores 652/652 either way.
fn write_header_attr(out: &mut String, attr: &Attr) {
    write_classes(out, attr);
    if let Some((_, number)) = attr.attributes.iter().find(|(key, _)| key == "number") {
        write_kv(out, "number", number);
    }
    for (key, value) in attr.attributes.iter().filter(|(key, _)| key != "number") {
        write_kv(out, key, value);
    }
    write_id(out, attr);
}

fn write_id(out: &mut String, attr: &Attr) {
    if attr.identifier.is_empty() {
        return;
    }
    out.push_str(" id=\"");
    escape_attribute(out, &attr.identifier);
    out.push('"');
}

fn write_classes(out: &mut String, attr: &Attr) {
    if attr.classes.is_empty() {
        return;
    }
    out.push_str(" class=\"");
    escape_attribute(out, &attr.classes.join(" "));
    out.push('"');
}

fn write_kv(out: &mut String, key: &str, value: &str) {
    out.push(' ');
    // Pandoc's rule, probed on `-f json -t html`: a name HTML does not know
    // is written behind `data-` (`foo` becomes `data-foo`), and a name it
    // knows is written as it stands (`onclick`, `style`, `href`). This is
    // **fidelity, not sanitizing** — a live `onclick` in the source passes
    // through here exactly as it passes through pandoc. `is_reserved` is
    // the "HTML knows this name" test, shared with the reader so the two
    // stay symmetric.
    //
    // `starts_with` is not redundant with it. The reader leaves
    // `data-onclick` *whole* precisely because `onclick` is reserved, so
    // the key arriving here is already prefixed; without this check the
    // writer prefixed it again and `ferrodoc -f html -t html` turned
    // `data-onclick` into `data-data-onclick`. It hit only the reserved
    // names — `onclick`, `style`, `href`, `id` — and never an ordinary
    // `data-k`, whose bare `k` the reader had already unwrapped.
    if !key.starts_with("data-") && !read::is_reserved(key) {
        out.push_str("data-");
    }
    // Keys come from the same untrusted AST as values; drop characters
    // that could break out of the tag.
    out.extend(key.chars().filter(|c| !c.is_whitespace() && !"\"'<>=/&".contains(*c)));
    out.push_str("=\"");
    escape_attribute(out, value);
    out.push('"');
}

/// Render attributes as ` id=".." class=".." k="v"`, pandoc's order —
/// except on a heading, which has its own order; see [`write_header_attr`].
fn write_attr(out: &mut String, attr: &Attr) {
    write_id(out, attr);
    write_classes(out, attr);
    for (key, value) in &attr.attributes {
        write_kv(out, key, value);
    }
}

/// The plain-text rendering of inlines (used for `alt` attributes).
fn plain_text(inlines: &[Inline]) -> String {
    let mut out = String::new();
    collect_plain(&mut out, inlines);
    out
}

fn collect_plain(out: &mut String, inlines: &[Inline]) {
    for inline in inlines {
        match inline {
            Inline::Str(s) | Inline::Code(_, s) | Inline::Math(_, s) => out.push_str(s),
            // Pandoc's alt-text stringify renders every break as a space.
            Inline::Space | Inline::SoftBreak | Inline::LineBreak => out.push(' '),
            Inline::Emph(i) | Inline::Strong(i) | Inline::Strikeout(i)
            | Inline::Superscript(i) | Inline::Subscript(i) | Inline::SmallCaps(i)
            | Inline::Underline(i) | Inline::Quoted(_, i) | Inline::Cite(_, i)
            | Inline::Span(_, i) | Inline::Link(_, i, _) | Inline::Image(_, i, _) => {
                collect_plain(out, i);
            }
            Inline::RawInline(..) | Inline::Note(_) => {}
        }
    }
}

/// Escape text content: `&`, `<`, `>` (pandoc leaves `"` alone in text).
fn escape_text(out: &mut String, text: &str) {
    // A plain per-character loop, deliberately: a "smarter" version that
    // searched for the next special character and copied slices measured
    // ~18% slower here, because these strings are short words and the
    // search machinery costs more than the copying it saves.
    for ch in text.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            ch => out.push(ch),
        }
    }
}

/// Escape code-block content: unlike inline code (`&`, `<`, `>` only),
/// pandoc also escapes `"` and `'` inside `<pre><code>` — the same set as
/// attribute values.
fn escape_code_block(out: &mut String, text: &str) {
    escape_attribute(out, text);
}

/// Escape attribute values: `&`, `<`, `>`, `"`, and `'` as `&#39;`
/// (pandoc escapes apostrophes in every attribute context).
fn escape_attribute(out: &mut String, text: &str) {
    for ch in text.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            ch => out.push(ch),
        }
    }
}

#[cfg(test)]
mod tests {
    /// A page with pandoc's defaults, which is what most of these
    /// tests want.
    fn page_of(doc: &Pandoc) -> String {
        write_page(doc, &Page::new()).expect("rendered")
    }

    use super::*;

    /// No differential gate reaches this: `diff-html` is markdown → HTML
    /// through `read_commonmark`, and `CommonMark` cannot express an
    /// arbitrary attribute, so every input to that gate scores the same
    /// before and after. Each line below is `pandoc -f json -t html`
    /// output, run and pasted.
    #[test]
    fn an_attribute_name_is_prefixed_exactly_when_html_does_not_know_it() {
        let div = |key: &str| {
            let attr = ferrodoc_ast::Attr {
                attributes: vec![(key.to_owned(), "v".to_owned())],
                ..ferrodoc_ast::Attr::default()
            };
            write_html(&Pandoc::new(vec![Block::Div(attr, Vec::new())]))
        };
        // Invented names go behind `data-`; names HTML knows do not.
        assert_eq!(div("foo"), "<div data-foo=\"v\">\n\n</div>\n");
        assert_eq!(div("onclick"), "<div onclick=\"v\">\n\n</div>\n");
        assert_eq!(div("style"), "<div style=\"v\">\n\n</div>\n");

        // Already prefixed: written once, not twice. Before this was
        // fixed, `ferrodoc -f html -t html` turned `data-onclick` into
        // `data-data-onclick` — and only ever the reserved names, because
        // the reader unwraps `data-k` to a bare `k` and leaves
        // `data-onclick` whole.
        for key in ["data-onclick", "data-style", "data-href", "data-id", "data-k"] {
            assert_eq!(div(key), format!("<div {key}=\"v\">\n\n</div>\n"), "{key}");
        }
    }

    fn html(md: &str) -> String {
        write_html(&ferrodoc_markdown::read_commonmark(md).expect("convertible"))
    }

    fn header(level: i64, id: &str, classes: &[&str], text: &str) -> Block {
        Block::Header(
            level,
            Attr {
                identifier: id.to_owned(),
                classes: classes.iter().map(|class| (*class).to_owned()).collect(),
                attributes: Vec::new(),
            },
            vec![Inline::Str(text.to_owned())],
        )
    }

    fn doc(blocks: Vec<Block>) -> Pandoc {
        Pandoc { blocks, ..Pandoc::default() }
    }

    /// Three rules `scripts/compare-toc.sh` cannot reach, because none of
    /// them can be written in the markdown this project reads: a class on
    /// a heading, a `Div`, and a `BlockQuote` around one. Each was probed
    /// against pandoc 3.8.2.1 with `--number-sections --wrap=none`.
    #[test]
    fn numbering_skips_unnumbered_and_blockquotes_but_not_divs() {
        let mut document = doc(vec![
            header(1, "one", &[], "One"),
            header(2, "two", &[], "Two"),
            // `pandoc -f markdown -t html --number-sections` on
            // `# Three {.unnumbered}` emits no number and keeps the class —
            // and the heading after it continues at 1.2, so an unnumbered
            // heading consumes nothing.
            header(1, "three", &["unnumbered"], "Three"),
            header(2, "four", &[], "Four"),
            Block::Div(Attr::default(), vec![header(2, "in-div", &[], "In a div")]),
            Block::BlockQuote(vec![header(2, "in-quote", &[], "In a quote")]),
        ]);
        number_sections(&mut document);
        let page = write_html(&document);
        assert!(page.contains(r#"<h1 data-number="1" id="one">"#), "{page}");
        assert!(page.contains(r#"<h2 data-number="1.1" id="two">"#), "{page}");
        assert!(page.contains(r#"<h1 class="unnumbered" id="three">"#), "{page}");
        assert!(page.contains(r#"<h2 data-number="1.2" id="four">"#), "{page}");
        assert!(page.contains(r#"<h2 data-number="1.3" id="in-div">"#), "{page}");
        assert!(page.contains(r#"<h2 id="in-quote">"#), "{page}");
        assert!(!page.contains(r#"id="in-quote" data"#), "{page}");
    }

    /// A heading inside a `BlockQuote` is not in the contents either, and a
    /// document with no heading gets **no** `<nav>` rather than an empty
    /// one — pandoc emits nothing at all.
    #[test]
    fn the_contents_hold_what_pandoc_puts_in_them() {
        assert_eq!(write_toc(&doc(vec![Block::Para(vec![Inline::Str("x".to_owned())])])), "");
        let quoted = doc(vec![Block::BlockQuote(vec![header(1, "q", &[], "Q")])]);
        assert_eq!(write_toc(&quoted), "");
        let nested = doc(vec![
            header(1, "one", &[], "One"),
            header(3, "three", &[], "Three"),
            header(2, "two", &[], "Two"),
        ]);
        // A jump from level 1 to level 3 opens exactly one list, so the
        // level-2 heading after it is a *sibling* of the level-3 one.
        assert_eq!(
            write_toc(&nested),
            concat!(
                "<nav id=\"TOC\" role=\"doc-toc\">\n",
                "<ul>\n",
                "<li><a href=\"#one\" id=\"toc-one\">One</a>\n",
                "<ul>\n",
                "<li><a href=\"#three\" id=\"toc-three\">Three</a></li>\n",
                "<li><a href=\"#two\" id=\"toc-two\">Two</a></li>\n",
                "</ul></li>\n",
                "</ul>\n",
                "</nav>\n",
            )
        );
        // A heading with no identifier gets no link: that is every heading
        // in a `-f commonmark` document, where identifiers are not read.
        let bare = doc(vec![header(1, "", &[], "Bare")]);
        assert!(bare_entry(&bare).contains("<li>Bare</li>"), "{}", bare_entry(&bare));
    }

    fn bare_entry(document: &Pandoc) -> String {
        write_toc(document)
    }

    /// The head and the reader are a matched pair: `-s` writes `title`,
    /// `author` and `lang`, and `read_html` reads exactly those back. No
    /// differential gate covers it — `diff-html-read` reads `corpus/*.html`
    /// and never this writer's own output — so it is asserted here.
    #[test]
    fn the_head_this_writer_produces_reads_back_as_the_metadata_it_came_from() {
        let mut document = doc(vec![Block::Para(vec![Inline::Str("body".to_owned())])]);
        document.meta.insert(
            "title".to_owned(),
            ferrodoc_ast::MetaValue::MetaString("A Title".to_owned()),
        );
        document.meta.insert(
            "author".to_owned(),
            ferrodoc_ast::MetaValue::MetaString("An Author".to_owned()),
        );
        document.meta.insert(
            "lang".to_owned(),
            ferrodoc_ast::MetaValue::MetaString("fr".to_owned()),
        );
        let page = page_of(&document);
        assert!(page.contains("<title>A Title</title>"), "{page}");
        assert!(page.contains(r#"<meta name="author" content="An Author" />"#), "{page}");
        assert!(page.contains(r#"lang="fr""#), "{page}");
        // Metadata the head has no place for stays out of it rather than
        // being invented as a `<meta name="…">`, which the reader would
        // then read back as a field the document never had.
        document.meta.insert(
            "custom".to_owned(),
            ferrodoc_ast::MetaValue::MetaString("value".to_owned()),
        );
        let page = page_of(&document);
        assert!(!page.contains("custom"), "{page}");

        let back = read_html(&page).expect("the page this crate wrote is readable");
        for key in ["title", "author", "lang"] {
            assert!(back.meta.contains_key(key), "{key} did not survive: {:?}", back.meta);
        }
    }

    /// Pandoc keeps a table's alignment in the **column specs**, and
    /// leaves every cell `AlignDefault`. Reading only the cell therefore
    /// dropped the alignment of every table that had one — and no gate
    /// could see it, because `diff-html` runs on the `CommonMark` spec and
    /// `CommonMark` has no tables.
    #[test]
    fn a_column_alignment_reaches_the_cells_it_governs() {
        let table = write_html(
            &ferrodoc_markdown::read_gfm("| L | C | R |\n|:--|:-:|--:|\n| a | b | c |\n")
                .expect("convertible"),
        );
        assert_eq!(table.matches("text-align: left;").count(), 2, "{table}");
        assert_eq!(table.matches("text-align: center;").count(), 2, "{table}");
        assert_eq!(table.matches("text-align: right;").count(), 2, "{table}");
        // A column with no alignment stays bare rather than gaining one.
        let plain = write_html(
            &ferrodoc_markdown::read_gfm("| a |\n|---|\n| b |\n").expect("convertible"),
        );
        assert!(!plain.contains("text-align"), "{plain}");
    }

    /// A word processor sets column widths and the DOCX reader keeps
    /// them exactly; the writer threw them away, so every converted table
    /// came out equal-width. Same blind spot as the alignment above.
    #[test]
    fn column_widths_survive_into_the_colgroup() {
        use ferrodoc_ast::{ColSpec, ColWidth};
        let mut doc = ferrodoc_markdown::read_gfm("| a | b |\n|---|---|\n| 1 | 2 |\n")
            .expect("convertible");
        let Some(Block::Table(table)) = doc.blocks.first_mut() else { panic!("a table") };
        table.colspecs = vec![
            ColSpec { alignment: Alignment::AlignDefault, width: ColWidth::ColWidth(0.335) },
            ColSpec { alignment: Alignment::AlignDefault, width: ColWidth::ColWidth(0.335) },
        ];
        let html = write_html(&doc);
        // The column truncates and the table rounds — pandoc's own
        // arithmetic, and 33/67 rather than 34/67 or 33/66.
        assert!(html.contains("<table style=\"width:67%;\">"), "{html}");
        assert_eq!(html.matches("<col style=\"width: 33%\" />").count(), 2, "{html}");
        // Columns that add up to the whole width name no table width.
        let Some(Block::Table(table)) = doc.blocks.first_mut() else { panic!("a table") };
        table.colspecs = vec![
            ColSpec { alignment: Alignment::AlignDefault, width: ColWidth::ColWidth(0.5) },
            ColSpec { alignment: Alignment::AlignDefault, width: ColWidth::ColWidth(0.5) },
        ];
        let full = write_html(&doc);
        assert!(full.contains("<table>"), "{full}");
        assert_eq!(full.matches("<col style=\"width: 50%\" />").count(), 2, "{full}");
        // A table with no stated widths gains no colgroup at all.
        let plain = write_html(
            &ferrodoc_markdown::read_gfm("| a |\n|---|\n| b |\n").expect("convertible"),
        );
        assert!(!plain.contains("colgroup"), "{plain}");
    }

    #[test]
    fn a_standalone_page_frames_the_fragment() {
        use ferrodoc_ast::MetaValue;
        let mut doc = ferrodoc_markdown::read_commonmark("text\n").expect("convertible");
        doc.meta.insert(
            "title".to_owned(),
            MetaValue::MetaInlines(vec![Inline::Str("My <Doc>".to_owned())]),
        );
        doc.meta.insert("lang".to_owned(), MetaValue::MetaString("fr".to_owned()));
        doc.meta.insert(
            "author".to_owned(),
            MetaValue::MetaList(vec![
                MetaValue::MetaString("Ada".to_owned()),
                MetaValue::MetaInlines(vec![Inline::Str("Grace".to_owned())]),
            ]),
        );
        let mut options = Page::new();
        options.css = vec!["theme.css".to_owned()];
        let page = write_page(&doc, &options).expect("rendered");

        // The body is the fragment, unchanged: one writer, two framings.
        assert!(page.contains(write_html(&doc).trim_end_matches('\n')), "{page}");
        assert!(page.starts_with("<!DOCTYPE html>\n"), "{page}");
        assert!(page.contains(r#"lang="fr""#), "{page}");
        assert!(page.contains("<meta charset=\"utf-8\" />"), "{page}");
        // Metadata is text, and text in a document can contain markup.
        assert!(page.contains("<title>My &lt;Doc&gt;</title>"), "{page}");
        // A field may be one value or a list, however it was spelled.
        assert!(page.contains("content=\"Ada\""), "{page}");
        assert!(page.contains("content=\"Grace\""), "{page}");
        // `--css` links a stylesheet; it does not inline the file, which
        // is what pandoc means by the flag.
        assert!(page.contains(r#"<link rel="stylesheet" href="theme.css" />"#), "{page}");
    }

    #[test]
    fn a_page_without_metadata_is_still_a_page() {
        let doc = ferrodoc_markdown::read_commonmark("text\n").expect("convertible");
        let page = page_of(&doc);
        // `lang` is empty rather than absent, which is pandoc's — and
        // the title element is required, so a page with no title still
        // has an empty one.
        assert!(page.starts_with("<!DOCTYPE html>\n"), "{page}");
        assert!(page.contains(r#"lang="""#), "{page}");
        assert!(page.contains("<title></title>"), "{page}");
        assert!(!page.contains("<meta name=\"author\""), "{page}");
    }
    #[test]
    fn paragraph_and_emphasis() {
        assert_eq!(html("a *b* **c**\n"), "<p>a <em>b</em> <strong>c</strong></p>\n");
    }

    #[test]
    fn tight_and_loose_lists() {
        assert_eq!(
            html("- a\n- b\n"),
            "<ul>\n<li>a</li>\n<li>b</li>\n</ul>\n"
        );
        assert_eq!(
            html("- a\n\n- b\n"),
            "<ul>\n<li><p>a</p></li>\n<li><p>b</p></li>\n</ul>\n"
        );
    }

    /// No round trip can see any of this: `- ☒ a` and `- [x] a` are one AST,
    /// so the boxes have to be checked against the literal bytes pandoc
    /// 3.8.2.1 writes for the same AST (`pandoc -f json -t html --wrap=none`).
    #[test]
    fn task_list_boxes_become_checkbox_inputs() {
        fn boxes(blocks: Vec<Block>) -> String {
            write_html(&Pandoc::new(blocks))
        }
        fn task(marker: &str, text: &str) -> Vec<Block> {
            vec![Block::Plain(vec![
                Inline::Str(marker.to_owned()),
                Inline::Space,
                Inline::Str(text.to_owned()),
            ])]
        }
        let plain = || vec![Block::Plain(vec![Inline::Str("plain".to_owned())])];

        // Every item a task item: the list itself is classed.
        assert_eq!(
            boxes(vec![Block::BulletList(vec![task("☒", "Done"), task("☐", "Todo")])]),
            "<ul class=\"task-list\">\n\
             <li><label><input type=\"checkbox\" checked=\"\" />Done</label></li>\n\
             <li><label><input type=\"checkbox\" />Todo</label></li>\n\
             </ul>\n"
        );
        // One ordinary item and the class goes, but the boxes stay.
        assert_eq!(
            boxes(vec![Block::BulletList(vec![task("☒", "Done"), plain()])]),
            "<ul>\n\
             <li><label><input type=\"checkbox\" checked=\"\" />Done</label></li>\n\
             <li>plain</li>\n\
             </ul>\n"
        );
        // An ordered list never takes the class, and still gets the boxes.
        assert_eq!(
            boxes(vec![Block::OrderedList(
                ferrodoc_ast::ListAttributes {
                    start: 1,
                    style: ListNumberStyle::Decimal,
                    delim: ferrodoc_ast::ListNumberDelim::Period,
                },
                vec![task("☒", "Done"), plain()],
            )]),
            "<ol type=\"1\">\n\
             <li><label><input type=\"checkbox\" checked=\"\" />Done</label></li>\n\
             <li>plain</li>\n\
             </ol>\n"
        );
        // A loose item boxes inside its `<p>`, and its later blocks stand.
        let loose = |marker: &str, text: &str| {
            vec![Block::Para(vec![
                Inline::Str(marker.to_owned()),
                Inline::Space,
                Inline::Str(text.to_owned()),
            ])]
        };
        let mut first = loose("☒", "a");
        first.push(Block::Para(vec![Inline::Str("x".to_owned())]));
        assert_eq!(
            boxes(vec![Block::BulletList(vec![first, loose("☐", "b")])]),
            "<ul class=\"task-list\">\n\
             <li><p><label><input type=\"checkbox\" checked=\"\" />a</label></p>\n\
             <p>x</p></li>\n\
             <li><p><label><input type=\"checkbox\" />b</label></p></li>\n\
             </ul>\n"
        );
    }

    /// The rule is narrower than "an item starting with a box": pandoc wants
    /// a `Str` holding the box alone and a `Space` right after it, at the
    /// head of the item's first `Plain`/`Para`. Everything else is text.
    #[test]
    fn a_box_pandoc_would_leave_alone_stays_text() {
        fn one(inlines: Vec<Inline>) -> String {
            write_html(&Pandoc::new(vec![Block::BulletList(vec![vec![Block::Plain(inlines)]])]))
        }
        let ticked = || Inline::Str("☒".to_owned());
        // No space after the box.
        assert_eq!(one(vec![ticked(), Inline::Str("no space".to_owned())]), "<ul>\n<li>☒no space</li>\n</ul>\n");
        // Nothing after the box at all.
        assert_eq!(one(vec![ticked()]), "<ul>\n<li>☒</li>\n</ul>\n");
        // A box inside an `Emph` is not at the head of the item.
        assert_eq!(
            one(vec![Inline::Emph(vec![ticked()]), Inline::Space, Inline::Str("emph".to_owned())]),
            "<ul>\n<li><em>☒</em> emph</li>\n</ul>\n"
        );
        // A `SoftBreak` is not the `Space` the rule asks for.
        assert_eq!(
            one(vec![ticked(), Inline::SoftBreak, Inline::Str("soft".to_owned())]),
            "<ul>\n<li>☒ soft</li>\n</ul>\n"
        );
        // The box and the space have to be separate inlines.
        assert_eq!(one(vec![Inline::Str("☒ one str".to_owned())]), "<ul>\n<li>☒ one str</li>\n</ul>\n");
        // An item that is only a box and a space is a task item with an
        // empty label, and a `<blockquote>` around the box is not one.
        assert_eq!(
            one(vec![ticked(), Inline::Space]),
            "<ul class=\"task-list\">\n<li><label><input type=\"checkbox\" checked=\"\" /></label></li>\n</ul>\n"
        );
        assert_eq!(
            write_html(&Pandoc::new(vec![Block::BulletList(vec![vec![Block::Plain(vec![
                Inline::Str("outer".to_owned()),
            ]), Block::BulletList(vec![vec![Block::Plain(vec![
                ticked(),
                Inline::Space,
                Inline::Str("i".to_owned()),
            ])]])]])])),
            "<ul>\n<li>outer\n<ul class=\"task-list\">\n\
             <li><label><input type=\"checkbox\" checked=\"\" />i</label></li>\n\
             </ul></li>\n</ul>\n"
        );
    }

    #[test]
    fn code_block_with_language() {
        assert_eq!(
            html("```rust\nfn x() {}\n```\n"),
            "<pre class=\"rust\"><code>fn x() {}</code></pre>\n"
        );
    }

    #[test]
    fn hard_break_in_alt_text_becomes_space() {
        assert_eq!(
            html("![a hard\\\nbreak](x.png)\n"),
            "<p><img src=\"x.png\" alt=\"a hard break\" /></p>\n"
        );
    }

    #[test]
    fn link_title_and_image_alt() {
        assert_eq!(
            html("[l](u \"t\") ![*em* alt](i.png)\n"),
            "<p><a href=\"u\" title=\"t\">l</a> <img src=\"i.png\" alt=\"em alt\" /></p>\n"
        );
    }
}
