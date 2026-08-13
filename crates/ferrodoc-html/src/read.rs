//! HTML reader: turns a page into the ferrodoc AST.
//!
//! Structural only, and deliberately so. Headings, paragraphs, lists,
//! tables, links, images, code, quotes and inline formatting carry meaning
//! that survives into another format; CSS, layout and presentation do not,
//! and are dropped rather than guessed at. What this produces is compared
//! against `pandoc -f html -t json` by `ferrodoc-harness diff-html-read`.
//!
//! Parsing is `html5ever`'s, so the tree this walks is the one a browser
//! would build: implied tags closed, misnesting repaired, text decoded.

use crate::Error;
use ferrodoc_ast::{
    Alignment, Attr, Block, Caption, Cell, ColSpec, ColWidth, Inline, ListAttributes,
    ListNumberDelim, ListNumberStyle, Pandoc, Row, Table, TableBody, TableFoot, TableHead, Target,
};
use html5ever::tendril::TendrilSink as _;
use markup5ever_rcdom::{Handle, NodeData, RcDom};
use std::collections::{HashMap, HashSet};

/// How deep an element tree may nest before the reader refuses it.
///
/// Hostile input reaches this code, and the walk below is recursive. The
/// bound is low on purpose: stack frames here are far larger in an
/// unoptimized build, and the test suite's threads get 2 MiB.
pub const MAX_NESTING: usize = 200;

/// Read an HTML document.
///
/// # Errors
///
/// Returns [`Error::TooDeep`] if the element tree nests deeper than
/// [`MAX_NESTING`]. Malformed markup is not an error — it is repaired the
/// way a browser repairs it.
pub fn read_html(input: &str) -> Result<Pandoc, Error> {
    let source = restore_verbatim_newline(&expand_tabs(input));
    let dom = html5ever::parse_document(RcDom::default(), html5ever::ParseOpts::default())
        .from_utf8()
        .one(source.as_bytes());
    let mut reader = Reader::default();
    let body = body(&dom.document).unwrap_or_else(|| dom.document.clone());
    // A page that marks its main content is read as that content alone,
    // which is how pandoc reads one — and the whole reason this is useful
    // on real pages, where the navigation and footer are not the article.
    let main = main_content(&body);
    let promote = if main.is_some() { Promote::Never } else { Promote::Normally };
    let root = main.unwrap_or(body);
    let result = reader.blocks_in(&children(&root), promote);
    // An `Rc` tree is dropped recursively, so a document deep enough to
    // refuse is also deep enough to overflow on the way out.
    flatten(dom.document);
    Ok(Pandoc { blocks: result?, ..Pandoc::default() })
}

#[derive(Default)]
struct Reader {
    depth: usize,
    /// Identifiers already handed out, so generated ones stay unique.
    used_idents: HashSet<String>,
    /// The next suffix to try for a base identifier already handed out.
    next_suffix: HashMap<String, u32>,
}

/// Whether loose text in a container becomes `Para` when something
/// paragraph-like sits beside it.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Promote {
    /// The ordinary rule.
    Normally,
    /// Inside a list item, where a nested list does not count.
    InsideAList,
    /// Inside a `<div>`, which never promotes.
    Never,
}

impl Reader {
    /// Read a run of sibling nodes as blocks, gathering any loose inline
    /// content between them into `Plain` the way pandoc does.
    fn blocks(&mut self, nodes: &[Handle]) -> Result<Vec<Block>, Error> {
        self.blocks_in(nodes, Promote::Normally)
    }

    /// Read a run of siblings, `promote` saying how loose text among them
    /// turns into `Plain` or `Para`.
    fn blocks_in(&mut self, nodes: &[Handle], promote: Promote) -> Result<Vec<Block>, Error> {
        let mut out = Vec::new();
        let mut loose: Vec<Inline> = Vec::new();
        for node in nodes {
            if let Some(name) = element_name(node)
                && (is_block(&name) || holds_blocks(&name, node))
            {
                flush(&mut out, &mut loose);
                self.block(&name, node, &mut out)?;
            } else {
                self.inline(node, &mut loose)?;
            }
        }
        flush(&mut out, &mut loose);
        fix_plains(&mut out, promote);
        Ok(out)
    }

    fn block(&mut self, name: &str, node: &Handle, out: &mut Vec<Block>) -> Result<(), Error> {
        self.deeper()?;
        let result = self.block_inner(name, node, out);
        self.depth -= 1;
        result
    }

    fn block_inner(
        &mut self,
        name: &str,
        node: &Handle,
        out: &mut Vec<Block>,
    ) -> Result<(), Error> {
        let kids = children(node);
        match name {
            "p" => {
                let inlines = self.inlines(&kids)?;
                if !inlines.is_empty() {
                    out.push(Block::Para(inlines));
                }
            }
            "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
                let level = i64::from(name.as_bytes()[1] - b'0');
                let inlines = self.inlines(&kids)?;
                let mut attr = attributes(node);
                attr.identifier = self.ident(&attr.identifier, &inlines);
                out.push(Block::Header(level, attr, inlines));
            }
            "blockquote" => out.push(Block::BlockQuote(self.blocks(&kids)?)),
            "ul" => out.push(Block::BulletList(self.items(&kids)?)),
            "ol" => {
                let start = attribute(node, "start")
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(1);
                // `type` names the numbering; anything else is default.
                let style = match attribute(node, "type").as_deref() {
                    Some("1") => ListNumberStyle::Decimal,
                    Some("a") => ListNumberStyle::LowerAlpha,
                    Some("A") => ListNumberStyle::UpperAlpha,
                    Some("i") => ListNumberStyle::LowerRoman,
                    Some("I") => ListNumberStyle::UpperRoman,
                    _ => ListNumberStyle::DefaultStyle,
                };
                let attrs = ListAttributes {
                    start,
                    style,
                    delim: ListNumberDelim::DefaultDelim,
                };
                out.push(Block::OrderedList(attrs, self.items(&kids)?));
            }
            "dl" => out.push(self.definition_list(&kids)?),
            "pre" => out.push(Self::code_block(node, &kids)),
            "hr" => out.push(Block::HorizontalRule),
            "table" => out.push(self.table(node, &kids)?),
            "figure" => out.push(self.figure(node, &kids)?),
            "main" => out.append(&mut self.division(&kids)?),
            // A `<div>` and a `<header>`/`<footer>` become a div; a
            // `<section>` becomes one too but takes a `section` class,
            // ahead of any of its own. Measured, one element at a time.
            "div" | "header" => {
                out.push(Block::Div(attributes(node), self.division(&kids)?));
            }
            "section" => {
                let mut attr = attributes(node);
                attr.classes.insert(0, "section".to_owned());
                out.push(Block::Div(attr, self.division(&kids)?));
            }
            // The rest of the sectioning elements carry no meaning of
            // their own: pandoc keeps their content and drops the wrapper.
            // An edit marker around block content goes the same way: it
            // has no block form, so the marking is what gets dropped.
            "article" | "aside" | "nav" | "footer" | "del" | "ins" | "s" | "strike" | "u" => {
                out.append(&mut self.blocks(&kids)?);
            }
            // Not content: scripts, styles and metadata carry nothing that
            // survives conversion, and pandoc drops them too.
            _ => {}
        }
        Ok(())
    }

    /// The items of a list: everything inside each `<li>`.
    fn items(&mut self, nodes: &[Handle]) -> Result<Vec<Vec<Block>>, Error> {
        let mut items = Vec::new();
        for node in nodes {
            if element_name(node).as_deref() == Some("li") {
                items.push(self.item(&children(node))?);
            }
        }
        Ok(items)
    }

    /// A division's contents. Unlike every other container, a `<div>`
    /// leaves its loose text as `Plain` however many blocks sit beside it
    /// — `<div>text<p>para</p></div>` keeps the text plain, where the same
    /// shape inside a quote or a cell would promote it.
    fn division(&mut self, nodes: &[Handle]) -> Result<Vec<Block>, Error> {
        self.deeper()?;
        let blocks = self.blocks_in(nodes, Promote::Never);
        self.depth -= 1;
        blocks
    }

    /// One list item or definition body. A single paragraph of loose
    /// content is `Plain`, not `Para` — that is what makes a list tight.
    fn item(&mut self, nodes: &[Handle]) -> Result<Vec<Block>, Error> {
        self.deeper()?;
        let blocks = self.blocks_in(nodes, Promote::InsideAList);
        self.depth -= 1;
        blocks
    }

    fn definition_list(&mut self, nodes: &[Handle]) -> Result<Block, Error> {
        let mut items: Vec<(Vec<Inline>, Vec<Vec<Block>>)> = Vec::new();
        for node in nodes {
            match element_name(node).as_deref() {
                Some("dt") => {
                    let term = self.inlines(&children(node))?;
                    // Several terms sharing one definition are one item,
                    // their terms separated by a break.
                    match items.last_mut() {
                        Some((previous, definitions)) if definitions.is_empty() => {
                            previous.push(Inline::LineBreak);
                            previous.extend(term);
                        }
                        _ => items.push((term, Vec::new())),
                    }
                }
                Some("dd") => {
                    let body = self.item(&children(node))?;
                    // A definition with no term still belongs somewhere.
                    if let Some((_, definitions)) = items.last_mut() {
                        definitions.push(body);
                    } else {
                        items.push((Vec::new(), vec![body]));
                    }
                }
                _ => {}
            }
        }
        Ok(Block::DefinitionList(items))
    }

    /// `<pre>` is verbatim: its text is the code, and a `<code>` child with
    /// a `language-x` class names the language.
    fn code_block(node: &Handle, kids: &[Handle]) -> Block {
        let mut attr = attributes(node);
        let mut source = kids;
        let inner = kids.iter().find(|k| element_name(k).as_deref() == Some("code"));
        let inner_kids;
        if let Some(inner) = inner {
            let mut inner_attr = attributes(inner);
            // `language-rust` is the HTML spelling of an info string.
            for class in &mut inner_attr.classes {
                if let Some(rest) = class.strip_prefix("language-") {
                    *class = rest.to_owned();
                }
            }
            // The `<pre>`'s own classes win outright: a `language-x` on
            // the inner `<code>` only applies when the `<pre>` said
            // nothing about what this is.
            if attr.classes.is_empty() {
                attr.classes.extend(inner_attr.classes);
            }
            attr.attributes.extend(inner_attr.attributes);
            if attr.identifier.is_empty() {
                attr.identifier = inner_attr.identifier;
            }
            inner_kids = children(inner);
            source = &inner_kids;
        }
        let mut text = String::new();
        raw_text(source, &mut text);
        // The newline that closes the last line is markup, not content.
        // A *leading* newline is content: html5ever has already dropped
        // the one the HTML spec says belongs to the `<pre>` tag itself.
        Block::CodeBlock(attr, text.strip_suffix('\n').unwrap_or(&text).to_owned())
    }

    fn figure(&mut self, node: &Handle, kids: &[Handle]) -> Result<Block, Error> {
        let mut caption = Vec::new();
        let mut body = Vec::new();
        for kid in kids {
            if element_name(kid).as_deref() == Some("figcaption") {
                caption = self.division(&children(kid))?;
            } else {
                body.push(kid.clone());
            }
        }
        Ok(Block::Figure(
            attributes(node),
            Caption { short: None, blocks: caption },
            self.division(&body)?,
        ))
    }

    // --- inlines ---

    fn inlines(&mut self, nodes: &[Handle]) -> Result<Vec<Inline>, Error> {
        let mut out = Vec::new();
        for node in nodes {
            self.inline(node, &mut out)?;
        }
        trim(&mut out);
        merge_adjacent(&mut out);
        drop_breaks_after_breaks(&mut out);
        Ok(out)
    }

    fn inline(&mut self, node: &Handle, out: &mut Vec<Inline>) -> Result<(), Error> {
        match &node.data {
            NodeData::Text { contents } => {
                text_tokens(&contents.borrow(), out);
                return Ok(());
            }
            NodeData::Element { name, .. } => {
                let tag = name.local.to_string();
                let kids = children(node);
                self.deeper()?;
                let result = self.element(&tag, node, &kids, out);
                self.depth -= 1;
                return result;
            }
            _ => {}
        }
        Ok(())
    }

    fn element(
        &mut self,
        tag: &str,
        node: &Handle,
        kids: &[Handle],
        out: &mut Vec<Inline>,
    ) -> Result<(), Error> {
        // Wrapping constructors, applied to the element's own children.
        let wrap: Option<fn(Vec<Inline>) -> Inline> = match tag {
            "em" | "i" => Some(Inline::Emph),
            "strong" | "b" => Some(Inline::Strong),
            "del" | "s" | "strike" => Some(Inline::Strikeout),
            "ins" | "u" => Some(Inline::Underline),
            "sup" => Some(Inline::Superscript),
            "sub" => Some(Inline::Subscript),
            _ => None,
        };
        if let Some(wrap) = wrap {
            out.push(wrap(self.inlines(kids)?));
            return Ok(());
        }
        // Elements that become a span carrying the tag as a class. Each of
        // these was measured against the binary; they are not a family
        // that can be guessed from what the tags mean.
        if let Some(class) = match tag {
            "dfn" => Some("dfn"),
            "kbd" => Some("kbd"),
            "mark" => Some("mark"),
            _ => None,
        } {
            let mut attr = attributes(node);
            attr.classes.insert(0, class.to_owned());
            out.push(Inline::Span(attr, self.inlines(kids)?));
            return Ok(());
        }
        match tag {
            "code" | "tt" | "var" | "samp" => {
                let mut text = String::new();
                raw_text(kids, &mut text);
                let mut attr = attributes(node);
                // `<var>` and `<samp>` are code with a class saying which.
                if let Some(class) = match tag {
                    "var" => Some("variable"),
                    "samp" => Some("sample"),
                    _ => None,
                } {
                    attr.classes.insert(0, class.to_owned());
                }
                // Inline code is still inline: a run of whitespace in the
                // source is one space, as everywhere else in HTML.
                out.push(Inline::Code(attr, collapse_spaces(&text)));
            }
            "br" => out.push(Inline::LineBreak),
            "a" => {
                let mut attr = attributes(node);
                // A legacy jump anchor names itself with `name`, which is
                // an identifier, not an attribute to carry along.
                if attr.identifier.is_empty()
                    && let Some((_, name)) = attr.attributes.iter().find(|(k, _)| k == "name")
                {
                    attr.identifier.clone_from(name);
                }
                attr.attributes.retain(|(k, _)| k != "name");
                let inner = self.inlines(kids)?;
                let Some(url) = attribute(node, "href") else {
                    // An anchor that goes nowhere is not a link; pandoc
                    // keeps it as a span so its attributes survive.
                    out.push(Inline::Span(attr, inner));
                    return Ok(());
                };
                let target = Target {
                    url,
                    title: attribute(node, "title").unwrap_or_default(),
                };
                attr.attributes.retain(|(k, _)| k != "href" && k != "title");
                out.push(Inline::Link(attr, inner, target));
            }
            "img" => {
                let target = Target {
                    url: attribute(node, "src").unwrap_or_default(),
                    title: attribute(node, "title").unwrap_or_default(),
                };
                let mut alt = Vec::new();
                text_tokens(&attribute(node, "alt").unwrap_or_default(), &mut alt);
                let mut attr = attributes(node);
                attr.attributes
                    .retain(|(k, _)| k != "src" && k != "title" && k != "alt");
                out.push(Inline::Image(attr, alt, target));
            }
            "span" => out.push(Inline::Span(attributes(node), self.inlines(kids)?)),
            // Never content, however it is nested. A `<textarea>` holds
            // a form's default value, not the document's text.
            "script" | "style" | "head" | "title" | "meta" | "link" | "textarea" => {}
            // Anything else contributes its children and nothing of
            // itself. A block element reached this way — `<div>` inside an
            // `<a>`, which HTML5 allows — cannot become a block from here,
            // but its text must still not run into its neighbours'.
            _ => {
                let boundary = is_block(tag);
                if boundary && !out.is_empty() {
                    out.push(Inline::SoftBreak);
                }
                for kid in kids {
                    self.inline(kid, out)?;
                }
                if boundary {
                    out.push(Inline::SoftBreak);
                }
            }
        }
        Ok(())
    }

    // --- tables ---

    fn table(&mut self, node: &Handle, kids: &[Handle]) -> Result<Block, Error> {
        let mut head_rows = Vec::new();
        let mut body_rows = Vec::new();
        let mut foot_rows = Vec::new();
        let mut caption = Vec::new();
        let declared = declared_widths(kids);
        for section in flatten_sections(kids) {
            match section.0.as_str() {
                "caption" => caption = self.item(&children(&section.1))?,
                "thead" => head_rows.extend(self.rows(&children(&section.1))?),
                "tfoot" => foot_rows.extend(self.rows(&children(&section.1))?),
                _ => body_rows.extend(self.rows(&children(&section.1))?),
            }
        }
        // A leading row of `<th>` is the header even without a `<thead>`.
        if head_rows.is_empty()
            && body_rows.first().is_some_and(|(all_header, _)| *all_header)
        {
            head_rows.push(body_rows.remove(0));
        }
        let strip = |rows: Vec<(bool, Row)>| rows.into_iter().map(|(_, row)| row).collect();
        let (head_rows, body_rows, foot_rows): (Vec<Row>, Vec<Row>, Vec<Row>) =
            (strip(head_rows), strip(body_rows), strip(foot_rows));
        // A `<colgroup>` states the table's shape; without one it is the
        // widest row that does.
        let columns: usize = if declared.is_empty() {
            head_rows
                .iter()
                .chain(&body_rows)
                .chain(&foot_rows)
                .map(|row| {
                    row.cells.iter().map(|c| usize::try_from(c.col_span).unwrap_or(1)).sum()
                })
                .max()
                .unwrap_or(0)
        } else {
            declared.len()
        };
        // Widths come from `<colgroup>` when the document states them.
        // Failing that they stay unset, unless some cell holds more than a
        // paragraph — a code block or a quotation — in which case pandoc
        // gives every column an equal explicit share.
        #[expect(clippy::cast_precision_loss, reason = "column counts are small")]
        let even = 1.0 / columns.max(1) as f64;
        let rich = head_rows
            .iter()
            .chain(&body_rows)
            .chain(&foot_rows)
            .flat_map(|row| &row.cells)
            .any(|cell| {
                cell.blocks.iter().any(|block| match block {
                    // A hard break inside a cell counts too: what pandoc
                    // is really asking is whether the cell is more than a
                    // single line of text.
                    Block::Plain(inlines) | Block::Para(inlines) => {
                        inlines.iter().any(|i| matches!(i, Inline::LineBreak))
                    }
                    _ => true,
                })
            });
        let width = |column: usize| match declared.get(column) {
            Some(Some(width)) => ColWidth::ColWidth(*width),
            // A `<col>` that states no width states it deliberately.
            Some(None) => ColWidth::ColWidthDefault,
            _ if rich => ColWidth::ColWidth(even),
            _ => ColWidth::ColWidthDefault,
        };
        // A column is aligned the way its cell in the first *body* row
        // is — a header row's own alignment does not set the column's.
        let first = body_rows.first().or_else(|| head_rows.first());
        let alignment = |column: usize| {
            first
                .and_then(|row| row.cells.get(column))
                .map_or(Alignment::AlignDefault, |cell| cell.alignment)
        };
        Ok(Block::Table(Box::new(Table {
            attr: attributes(node),
            caption: Caption { short: None, blocks: caption },
            colspecs: (0..columns)
                .map(|column| ColSpec {
                    alignment: alignment(column),
                    width: width(column),
                })
                .collect(),
            head: TableHead { attr: Attr::default(), rows: head_rows },
            bodies: vec![TableBody {
                attr: Attr::default(),
                row_head_columns: 0,
                head: Vec::new(),
                body: body_rows,
            }],
            foot: TableFoot { attr: Attr::default(), rows: foot_rows },
        })))
    }

    /// Rows, each flagged with whether every cell in it was a `<th>`.
    fn rows(&mut self, nodes: &[Handle]) -> Result<Vec<(bool, Row)>, Error> {
        let mut rows = Vec::new();
        for node in nodes {
            if element_name(node).as_deref() != Some("tr") {
                continue;
            }
            let mut cells = Vec::new();
            let mut all_header = true;
            for cell in children(node) {
                let tag = element_name(&cell);
                match tag.as_deref() {
                    Some("th") => {}
                    Some("td") => all_header = false,
                    _ => continue,
                }
                let span = |name: &str| {
                    attribute(&cell, name)
                        .and_then(|v| v.parse::<i64>().ok())
                        .unwrap_or(1)
                        .max(1)
                };
                let mut attr = attributes(&cell);
                // The spans and the alignment are the cell's shape, not
                // stray attributes: pandoc consumes them out of `align`
                // and out of a `text-align` declaration in `style`.
                attr.attributes
                    .retain(|(k, _)| k != "rowspan" && k != "colspan");
                let alignment = take_alignment(&mut attr);
                cells.push(Cell {
                    attr,
                    alignment,
                    row_span: span("rowspan"),
                    col_span: span("colspan"),
                    blocks: self.item(&children(&cell))?,
                });
            }
            rows.push((all_header && !cells.is_empty(), Row { attr: Attr::default(), cells }));
        }
        Ok(rows)
    }

    // --- bookkeeping ---

    /// An explicit identifier is kept and reserved; an absent one is
    /// generated from the heading text, as pandoc's reader does.
    fn ident(&mut self, explicit: &str, inlines: &[Inline]) -> String {
        if !explicit.is_empty() {
            self.used_idents.insert(explicit.to_owned());
            return explicit.to_owned();
        }
        let filtered: String = plain_text(inlines)
            .chars()
            .filter(|c| c.is_alphanumeric() || c.is_whitespace() || matches!(c, '_' | '-' | '.'))
            .flat_map(char::to_lowercase)
            .collect();
        let joined: String = filtered.split_whitespace().collect::<Vec<_>>().join("-");
        let start = joined.find(char::is_alphabetic).unwrap_or(joined.len());
        let mut id = joined[start..].to_owned();
        if id.is_empty() {
            "section".clone_into(&mut id);
        }
        // Resumed from the last suffix this base name reached rather
        // than restarted at zero: restarting is quadratic in the number of
        // headings that share a name, and identifiers are only ever taken,
        // so every suffix below the mark is already gone.
        let mut n = self.next_suffix.get(&id).copied().unwrap_or(0);
        let mut unique = if n == 0 { id.clone() } else { format!("{id}-{n}") };
        while !self.used_idents.insert(unique.clone()) {
            n += 1;
            unique = format!("{id}-{n}");
        }
        self.next_suffix.insert(id, n + 1);
        unique
    }

    /// Enter one level of nesting, refusing input that is too deep. Every
    /// caller pairs this with a matching decrement.
    fn deeper(&mut self) -> Result<(), Error> {
        if self.depth >= MAX_NESTING {
            return Err(Error::TooDeep);
        }
        self.depth += 1;
        Ok(())
    }
}

// --- tree helpers ---

fn children(node: &Handle) -> Vec<Handle> {
    node.children.borrow().clone()
}

fn element_name(node: &Handle) -> Option<String> {
    match &node.data {
        NodeData::Element { name, .. } => Some(name.local.to_string()),
        _ => None,
    }
}

/// The `<body>` html5ever built, which is where a fragment's content lands.
fn body(document: &Handle) -> Option<Handle> {
    for kid in children(document) {
        if element_name(&kid).as_deref() == Some("html") {
            for grandkid in children(&kid) {
                if element_name(&grandkid).as_deref() == Some("body") {
                    return Some(grandkid);
                }
            }
        }
    }
    None
}

/// The element a page nominates as its main content, if it has one:
/// `<main>`, or anything carrying `role="main"`. Searched iteratively, so
/// a deep page cannot overflow the stack before the depth check runs.
fn main_content(root: &Handle) -> Option<Handle> {
    let mut pending = vec![root.clone()];
    while let Some(node) = pending.pop() {
        if element_name(&node).as_deref() == Some("main")
            || attribute(&node, "role").as_deref() == Some("main")
        {
            return Some(node);
        }
        // Reversed so the walk is document order, not reverse order.
        pending.extend(children(&node).into_iter().rev());
    }
    None
}

fn attribute(node: &Handle, name: &str) -> Option<String> {
    match &node.data {
        NodeData::Element { attrs, .. } => attrs
            .borrow()
            .iter()
            .find(|a| a.name.local.as_ref() == name)
            .map(|a| a.value.to_string()),
        _ => None,
    }
}

/// An element's identifier, classes and remaining attributes, in the shape
/// the AST wants them.
fn attributes(node: &Handle) -> Attr {
    let NodeData::Element { attrs, .. } = &node.data else {
        return Attr::default();
    };
    let mut attr = Attr::default();
    for a in attrs.borrow().iter() {
        let (name, value) = (a.name.local.to_string(), a.value.to_string());
        match name.as_str() {
            "id" => attr.identifier = value,
            "class" => attr
                .classes
                .extend(value.split_whitespace().map(str::to_owned)),
            // `data-foo` is carried as `foo` — unless that name is one
            // the AST already spends on something else, in which case it
            // stays as it was written.
            _ => {
                let name = match name.strip_prefix("data-") {
                    Some(rest) if !is_reserved(rest) => rest.to_owned(),
                    _ => name,
                };
                attr.attributes.push((name, value));
            }
        }
    }
    attr
}

/// Take a cell's alignment out of its attributes: an `align` attribute, or
/// a `text-align` declaration inside `style`. The declaration is consumed —
/// what is left of the style stays, and an empty style is dropped.
fn take_alignment(attr: &mut Attr) -> Alignment {
    let mut alignment = None;
    let mut set = |value: &str| {
        alignment = match value.trim() {
            "left" => Some(Alignment::AlignLeft),
            "right" => Some(Alignment::AlignRight),
            "center" => Some(Alignment::AlignCenter),
            _ => alignment,
        };
    };
    if let Some((_, value)) = attr.attributes.iter().find(|(k, _)| k == "align") {
        set(&value.clone());
    }
    attr.attributes.retain(|(k, _)| k != "align");
    if let Some((_, style)) = attr.attributes.iter_mut().find(|(k, _)| k == "style") {
        let mut kept = Vec::new();
        for declaration in style.split(';') {
            let Some((property, value)) = declaration.split_once(':') else {
                continue;
            };
            if property.trim() == "text-align" {
                set(value);
            } else {
                kept.push(format!("{}: {}", property.trim(), value.trim()));
            }
        }
        *style = kept.join("; ");
    }
    attr.attributes
        .retain(|(k, value)| k != "style" || !value.is_empty());
    alignment.unwrap_or(Alignment::AlignDefault)
}

/// Whether an element that is normally inline is holding block content,
/// which is legal for the edit markers and changes what they mean: pandoc
/// keeps the blocks and drops the marking, having nowhere to put it.
fn holds_blocks(name: &str, node: &Handle) -> bool {
    matches!(name, "del" | "ins" | "s" | "strike" | "u")
        && children(node)
            .iter()
            .filter_map(element_name)
            .any(|child| is_block(&child))
}

/// Attribute names that stay as they are written when they appear behind a
/// `data-` prefix.
///
/// Pandoc strips the prefix from a custom attribute — `data-toggle` is
/// carried as `toggle` — but leaves it alone when the remainder is an HTML
/// attribute name in its own right, so that `data-href` cannot be confused
/// with `href`. This is that vocabulary, derived by asking the 3.8.2.1
/// binary about each name rather than by reading a spec: everything here
/// was kept, and `challenge`, `keytype` and `scoped` were not.
const RESERVED_ATTRIBUTES: &[&str] = &[
    "abbr", "about", "accept", "accept-charset", "accesskey", "action",
    "align", "alink", "allow", "allowfullscreen", "allowpaymentrequest",
    "allowusermedia", "alt", "archive", "as", "async", "autocapitalize",
    "autocomplete", "autofocus", "autoplay", "axis", "background",
    "bgcolor", "border", "cellpadding", "cellspacing", "char", "charoff",
    "charset", "checked", "cite", "class", "classid", "clear", "code",
    "codebase", "codetype", "color", "cols", "colspan", "compact",
    "content", "contenteditable", "controls", "coords", "crossorigin",
    "data", "datatype", "datetime", "declare", "decoding", "default",
    "defer", "dir", "dirname", "disabled", "download", "draggable",
    "enctype", "enterkeyhint", "face", "fetchpriority", "for", "form",
    "formaction", "formenctype", "formmethod", "formnovalidate",
    "formtarget", "frame", "frameborder", "headers", "height", "hidden",
    "high", "href", "hreflang", "hspace", "http-equiv", "id",
    "imagesizes", "imagesrcset", "inputmode", "integrity", "is", "ismap",
    "itemid", "itemprop", "itemref", "itemscope", "itemtype", "kind",
    "label", "lang", "language", "link", "list", "longdesc", "loop",
    "low", "manifest", "marginheight", "marginwidth", "max", "maxlength",
    "media", "method", "min", "minlength", "multiple", "muted", "name",
    "nohref", "nomodule", "nonce", "noresize", "noshade", "novalidate",
    "nowrap", "object", "onabort", "onafterprint", "onauxclick",
    "onbeforeprint", "onbeforeunload", "onblur", "oncancel", "oncanplay",
    "oncanplaythrough", "onchange", "onclick", "onclose", "oncontextmenu",
    "oncopy", "oncuechange", "oncut", "ondblclick", "ondrag", "ondragend",
    "ondragenter", "ondragexit", "ondragleave", "ondragover",
    "ondragstart", "ondrop", "ondurationchange", "onemptied", "onended",
    "onerror", "onfocus", "onhashchange", "oninput", "oninvalid",
    "onkeydown", "onkeypress", "onkeyup", "onlanguagechange", "onload",
    "onloadeddata", "onloadedmetadata", "onloadstart", "onmessage",
    "onmessageerror", "onmousedown", "onmouseenter", "onmouseleave",
    "onmousemove", "onmouseout", "onmouseover", "onmouseup", "onoffline",
    "ononline", "onpagehide", "onpageshow", "onpaste", "onpause",
    "onplay", "onplaying", "onpopstate", "onprogress", "onratechange",
    "onrejectionhandled", "onreset", "onresize", "onscroll",
    "onsecuritypolicyviolation", "onseeked", "onseeking", "onselect",
    "onstalled", "onstorage", "onsubmit", "onsuspend", "ontimeupdate",
    "ontoggle", "onunhandledrejection", "onunload", "onvolumechange",
    "onwaiting", "onwheel", "open", "optimum", "pattern", "ping",
    "placeholder", "playsinline", "poster", "prefix", "preload",
    "profile", "prompt", "property", "readonly", "referrerpolicy", "rel",
    "required", "resource", "rev", "reversed", "role", "rows", "rowspan",
    "rules", "sandbox", "scheme", "scope", "scrolling", "selected",
    "shape", "size", "sizes", "slot", "span", "spellcheck", "src",
    "srcdoc", "srclang", "srcset", "standby", "start", "step", "style",
    "summary", "tabindex", "target", "text", "title", "translate", "type",
    "typeof", "usemap", "valign", "value", "valuetype", "version",
    "vlink", "vocab", "vspace", "width", "wrap",
];

/// Whether `name` is an HTML attribute in its own right, and so keeps its
/// `data-` prefix when read and is written back without one.
///
/// The reader and the writer share this: `data-toggle` is read as `toggle`
/// and written back as `data-toggle`, so a name the AST invented can never
/// become a live attribute on the way out. That symmetry, not a rule about
/// event handlers, is what keeps `data-onpointerdown` inert — pandoc's own
/// vocabulary predates the newer events and reads it as `onpointerdown`,
/// and its writer re-prefixes it for exactly this reason.
pub(crate) fn is_reserved(name: &str) -> bool {
    RESERVED_ATTRIBUTES.binary_search(&name).is_ok()
}

/// Which elements start a block rather than joining the surrounding text.
fn is_block(name: &str) -> bool {
    matches!(
        name,
        "p" | "h1"
            | "h2"
            | "h3"
            | "h4"
            | "h5"
            | "h6"
            | "blockquote"
            | "ul"
            | "ol"
            | "dl"
            | "pre"
            | "hr"
            | "table"
            | "figure"
            | "div"
            | "section"
            | "article"
            | "aside"
            | "header"
            | "footer"
            | "main"
            | "nav"
    )
}

/// The text an element encloses, with no markup interpretation at all.
///
/// Walked with an explicit stack rather than by recursion: this descends
/// through whatever the document nests inside a `<pre>` without consulting
/// the depth bound, so recursing here would abort on input the bound is
/// supposed to reject.
fn raw_text(nodes: &[Handle], out: &mut String) {
    let mut pending: Vec<Handle> = nodes.iter().rev().cloned().collect();
    while let Some(node) = pending.pop() {
        match &node.data {
            NodeData::Text { contents } => out.push_str(&contents.borrow()),
            NodeData::Element { name, .. } if name.local.as_ref() == "br" => out.push('\n'),
            NodeData::Element { .. } => pending.extend(children(&node).into_iter().rev()),
            _ => {}
        }
    }
}

/// The plain text of an inline sequence, for generating identifiers.
fn plain_text(inlines: &[Inline]) -> String {
    let mut out = String::new();
    for inline in inlines {
        match inline {
            Inline::Str(text) | Inline::Code(_, text) => out.push_str(text),
            Inline::Space | Inline::SoftBreak | Inline::LineBreak => out.push(' '),
            Inline::Emph(inner)
            | Inline::Strong(inner)
            | Inline::Strikeout(inner)
            | Inline::Superscript(inner)
            | Inline::Subscript(inner)
            | Inline::SmallCaps(inner)
            | Inline::Underline(inner)
            | Inline::Span(_, inner)
            | Inline::Link(_, inner, _)
            | Inline::Image(_, inner, _) => out.push_str(&plain_text(inner)),
            _ => {}
        }
    }
    out
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

/// Drop leading and trailing whitespace tokens, which HTML indentation
/// produces in quantity and which carry no meaning.
fn trim(inlines: &mut Vec<Inline>) {
    let space = |i: &Inline| matches!(i, Inline::Space | Inline::SoftBreak);
    // Drained in one go: `remove(0)` in a loop is quadratic, and HTML
    // indentation produces long leading whitespace runs.
    let leading = inlines.iter().take_while(|i| space(i)).count();
    inlines.drain(..leading);
    while inlines.last().is_some_and(space) {
        inlines.pop();
    }
}

/// Merge directly adjacent nodes of the same kind.
///
/// `<b>a</b><b>b</b>` is one `Strong`, which is what pandoc's reader
/// produces. Text and whitespace merge too, because an element between
/// them may have contributed nothing at all — an HTML comment mid-sentence
/// is ordinary, and `a<!-- c -->b` is the single word `ab`.
fn merge_adjacent(inlines: &mut Vec<Inline>) {
    let space = |i: &Inline| matches!(i, Inline::Space | Inline::SoftBreak);
    // Built in one pass rather than by removing in place: a page can hold
    // hundreds of thousands of mergeable nodes (`x<wbr>` repeated, a
    // comment between every word), and removing from the middle of a `Vec`
    // inside the scan makes that quadratic.
    let mut out: Vec<Inline> = Vec::with_capacity(inlines.len());
    for next in inlines.drain(..) {
        match (out.last_mut(), next) {
            (Some(Inline::Emph(inner)), Inline::Emph(next))
            | (Some(Inline::Strong(inner)), Inline::Strong(next))
            | (Some(Inline::Strikeout(inner)), Inline::Strikeout(next))
            | (Some(Inline::Subscript(inner)), Inline::Subscript(next))
            | (Some(Inline::Superscript(inner)), Inline::Superscript(next))
            | (Some(Inline::Underline(inner)), Inline::Underline(next)) => inner.extend(next),
            (Some(Inline::Str(text)), Inline::Str(next)) => text.push_str(&next),
            // A run of whitespace is one space however many nodes it
            // spans; a soft break anywhere in the run makes the run one.
            (Some(last), next) if space(last) && space(&next) => {
                if matches!(next, Inline::SoftBreak) {
                    *last = Inline::SoftBreak;
                }
            }
            (_, next) => out.push(next),
        }
    }
    *inlines = out;
}

/// The newline that follows a `<br>` in the source is the markup's own
/// formatting; keeping it would double the break.
fn drop_breaks_after_breaks(inlines: &mut Vec<Inline>) {
    // Retained in one pass: removing from the middle of the `Vec` inside
    // the scan is quadratic, and `x<br>` on its own line is how HTML is
    // ordinarily formatted, so the run can be the length of the page.
    let mut after_break = false;
    inlines.retain(|inline| {
        let drop = after_break && matches!(inline, Inline::Space | Inline::SoftBreak);
        after_break = matches!(inline, Inline::LineBreak) || (after_break && drop);
        !drop
    });
}

/// Emit any gathered loose inlines as a block, recording where it landed
/// so the caller can decide between `Plain` and `Para`.
fn flush(out: &mut Vec<Block>, loose: &mut Vec<Inline>) {
    trim(loose);
    merge_adjacent(loose);
    drop_breaks_after_breaks(loose);
    if !loose.is_empty() {
        out.push(Block::Plain(std::mem::take(loose)));
    }
}

/// Promote bare text to a paragraph when it shares its container with
/// something paragraph-like.
///
/// Text with no block around it reads as `Plain`, which is what keeps a
/// list tight. Pandoc promotes every `Plain` in a container to `Para` as
/// soon as one sibling is paragraph-like — and inside a list item a nested
/// list does not count, which is exactly what stops `<li>a<ul>…</ul></li>`
/// from going loose. Which blocks count was measured against the binary,
/// not read off its source: a table and a figure do not, a code block and
/// a heading do.
fn fix_plains(blocks: &mut [Block], promote: Promote) {
    if promote == Promote::Never {
        return;
    }
    let in_list = promote == Promote::InsideAList;
    let paraish = |block: &Block| match block {
        Block::Para(_)
        | Block::CodeBlock(..)
        | Block::Header(..)
        | Block::BlockQuote(_)
        | Block::LineBlock(_)
        | Block::RawBlock(..)
        | Block::DefinitionList(_) => true,
        Block::BulletList(_) | Block::OrderedList(..) => !in_list,
        _ => false,
    };
    if !blocks.iter().any(paraish) {
        return;
    }
    for block in blocks {
        if let Block::Plain(inlines) = block {
            *block = Block::Para(std::mem::take(inlines));
        }
    }
}

/// Every run of whitespace becomes a single space.
fn collapse_spaces(text: &str) -> String {
    let mut out = String::new();
    let mut in_space = false;
    for ch in text.chars() {
        if matches!(ch, ' ' | '\t' | '\r' | '\n') {
            if !in_space {
                out.push(' ');
            }
            in_space = true;
        } else {
            out.push(ch);
            in_space = false;
        }
    }
    out
}

/// Double a newline that directly follows a `<pre>`, `<textarea>` or
/// `<listing>` start tag.
///
/// The HTML spec says that newline belongs to the tag and a conforming
/// parser drops it, which `html5ever` duly does — but pandoc keeps it, and
/// a code block that silently loses its first line is a worse outcome than
/// a disagreement with the spec about a character nobody can see. Writing
/// two makes the parser drop one and keep one.
fn restore_verbatim_newline(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut rest = input;
    while let Some(at) = rest.find('<') {
        let after = &rest[at + 1..];
        // Compared as bytes: a tag name is ASCII, but the text around it
        // is not, and slicing a `&str` by a byte length would panic in the
        // middle of a character.
        let bytes = after.as_bytes();
        let verbatim = ["pre", "textarea", "listing"].iter().any(|tag| {
            bytes.len() > tag.len()
                && bytes[..tag.len()].eq_ignore_ascii_case(tag.as_bytes())
                // The tag name has to end here, or this is `<presentation>`.
                && !bytes[tag.len()].is_ascii_alphanumeric()
        });
        if !verbatim {
            out.push_str(&rest[..=at]);
            rest = after;
            continue;
        }
        let Some(close) = end_of_tag(after) else {
            // No `>` remains anywhere, so no start tag can close and
            // nothing further can match. Rescanning from the next byte
            // would repeat this search for every `<` that is left.
            break;
        };
        let tag_end = at + 1 + close + 1;
        out.push_str(&rest[..tag_end]);
        rest = &rest[tag_end..];
        if rest.starts_with('\n') {
            out.push('\n');
        }
    }
    out.push_str(rest);
    out
}

/// The offset of the `>` closing a start tag that begins at `after`,
/// skipping quoted attribute values, which may contain one.
fn end_of_tag(after: &str) -> Option<usize> {
    let mut quote = None;
    for (offset, ch) in after.char_indices() {
        match (quote, ch) {
            (None, '"' | '\'') => quote = Some(ch),
            (Some(open), ch) if ch == open => quote = None,
            (None, '>') => return Some(offset),
            _ => {}
        }
    }
    None
}

/// Replace tabs with spaces to the next four-column stop.
///
/// Pandoc does this to the *source text*, before any parsing, so the
/// column a tab expands from counts the surrounding markup too — a tab
/// right after `<pre><code>` starts at column 11, not column 0. Doing it
/// anywhere else gives different spacing for the same document.
fn expand_tabs(input: &str) -> String {
    if !input.contains('\t') {
        return input.to_owned();
    }
    let mut out = String::with_capacity(input.len());
    let mut column = 0usize;
    for ch in input.chars() {
        match ch {
            '\t' => {
                let width = 4 - column % 4;
                out.extend(std::iter::repeat_n(' ', width));
                column += width;
            }
            '\n' => {
                out.push('\n');
                column = 0;
            }
            ch => {
                out.push(ch);
                column += 1;
            }
        }
    }
    out
}

/// The widths a `<colgroup>` states, as fractions, one per `<col>`.
fn declared_widths(kids: &[Handle]) -> Vec<Option<f64>> {
    let mut widths = Vec::new();
    for group in kids {
        if element_name(group).as_deref() != Some("colgroup") {
            continue;
        }
        for col in children(group) {
            if element_name(&col).as_deref() != Some("col") {
                continue;
            }
            // `width="30%"` is the old spelling; `style="width: 30%"` is
            // the one pandoc's own HTML writer emits, so a table that has
            // been through pandoc arrives written the second way.
            let percent = attribute(&col, "width").or_else(|| {
                attribute(&col, "style")?
                    .split(';')
                    .filter_map(|d| d.split_once(':'))
                    .find(|(property, _)| property.trim() == "width")
                    .map(|(_, value)| value.trim().to_owned())
            });
            let width = percent
                .and_then(|w| w.trim().strip_suffix('%').map(str::to_owned))
                .and_then(|w| w.trim().parse::<f64>().ok())
                .map(|percent| percent / 100.0);
            // One entry per `<col>` element. A `span` attribute would
            // stand for several columns in HTML, but pandoc counts the
            // elements, so honouring it would give the wrong shape.
            widths.push(width);
        }
    }
    widths
}

/// A table's parts, with `<tbody>` wrappers seen through: a row may sit
/// directly inside `<table>`, and the tree builder does not always add one.
fn flatten_sections(kids: &[Handle]) -> Vec<(String, Handle)> {
    let mut sections = Vec::new();
    let mut loose = Vec::new();
    for kid in kids {
        match element_name(kid).as_deref() {
            Some(name @ ("thead" | "tfoot" | "tbody" | "caption")) => {
                sections.push((name.to_owned(), kid.clone()));
            }
            Some("tr") => loose.push(kid.clone()),
            _ => {}
        }
    }
    if !loose.is_empty() {
        // Wrap the loose rows in the table itself so `rows` can walk them.
        sections.push(("tbody".to_owned(), synthetic_tbody(loose)));
    }
    sections
}

/// A `<tbody>` that exists only to hold rows found directly in a `<table>`.
fn synthetic_tbody(rows: Vec<Handle>) -> Handle {
    let node = markup5ever_rcdom::Node::new(NodeData::Element {
        name: html5ever::QualName::new(None, html5ever::ns!(html), html5ever::local_name!("tbody")),
        attrs: std::cell::RefCell::new(Vec::new()),
        template_contents: std::cell::RefCell::new(None),
        mathml_annotation_xml_integration_point: false,
    });
    *node.children.borrow_mut() = rows;
    node
}

/// Dismantle a node tree iteratively.
///
/// `Rc` drops its children recursively, so a tree deep enough for the walk
/// to refuse is also deep enough to overflow the stack on the way out —
/// turning a clean error into an abort. Taking the children level by level
/// keeps the drop flat.
fn flatten(root: Handle) {
    let mut pending = vec![root];
    while let Some(node) = pending.pop() {
        let kids = std::mem::take(&mut *node.children.borrow_mut());
        pending.extend(kids);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn blocks(html: &str) -> Vec<Block> {
        read_html(html).expect("readable").blocks
    }

    #[test]
    fn structure_becomes_blocks() {
        assert_eq!(
            blocks("<h1>T</h1>"),
            vec![Block::Header(
                1,
                Attr { identifier: "t".to_owned(), ..Attr::default() },
                vec![Inline::Str("T".to_owned())]
            )]
        );
        assert_eq!(
            blocks("<ul><li>x</li></ul>"),
            vec![Block::BulletList(vec![vec![Block::Plain(vec![Inline::Str("x".to_owned())])]])]
        );
    }

    #[test]
    fn adjacent_emphasis_of_one_kind_merges() {
        let Block::Plain(inlines) = &blocks("<b>a</b><b>b</b>")[0] else {
            panic!("expected loose inlines")
        };
        assert_eq!(inlines.len(), 1, "{inlines:?}");
    }

    #[test]
    fn deep_nesting_is_refused_not_survived_by_luck() {
        let deep = "<div>".repeat(MAX_NESTING + 50);
        assert!(matches!(read_html(&deep), Err(Error::TooDeep)));
    }

    #[test]
    fn very_deep_nesting_neither_overflows_nor_aborts() {
        // Far past the bound: the refusal must come from the check, and
        // dropping the tree afterwards must not overflow either.
        let deep = "<div>".repeat(20_000);
        assert!(matches!(read_html(&deep), Err(Error::TooDeep)));
    }

    #[test]
    fn every_nesting_shape_is_bounded_not_just_divs() {
        // Each of these reaches the walk by a different path. Verbatim
        // content is the one that matters most: it is read without
        // consulting the depth bound at all, so it must not recurse.
        for deep in [
            // 20_000 frames overflows the 2 MiB stack a test thread gets
            // many times over, so a return to recursion fails loudly here.
            "<pre>".to_owned() + &"<span>".repeat(20_000) + "x",
            "<p><code>".to_owned() + &"<span>".repeat(20_000) + "x",
            "<ul><li>".repeat(10_000),
            "<blockquote>".repeat(10_000),
            "<table><tr><td>".repeat(3_000),
            "<div><blockquote><ul><li>".repeat(3_000),
            "<p>".to_owned() + &"<em>".repeat(20_000) + "x",
        ] {
            // Either answer is fine; aborting the process is not.
            let _ = read_html(&deep);
        }
    }

    #[test]
    fn the_reserved_attribute_list_stays_searchable() {
        // `is_reserved` binary-searches it, which quietly returns wrong
        // answers rather than failing if the order ever slips.
        assert!(
            RESERVED_ATTRIBUTES.windows(2).all(|w| w[0] < w[1]),
            "RESERVED_ATTRIBUTES must be sorted and free of duplicates"
        );
        assert!(is_reserved("href") && is_reserved("role") && is_reserved("id"));
        assert!(!is_reserved("toggle") && !is_reserved("testid"));
    }

    #[test]
    fn an_invented_attribute_cannot_become_a_live_one() {
        // The reader strips `data-` from a name the AST has no meaning
        // for, so the writer must put it back. Without that symmetry a
        // document round-tripped through the AST turns `data-onclick`
        // into an event handler that runs.
        let doc = read_html(r#"<span data-onclick="x" data-onpointerdown="y">t</span>"#)
            .expect("readable");
        let html = crate::write_html(&doc);
        assert!(html.contains("data-onclick=\"x\""), "{html}");
        assert!(html.contains("data-onpointerdown=\"y\""), "{html}");
        assert!(!html.contains(" onclick"), "{html}");
        assert!(!html.contains(" onpointerdown"), "{html}");
        // A real attribute still comes back as itself.
        let doc = read_html(r#"<span title="t">x</span>"#).expect("readable");
        assert!(crate::write_html(&doc).contains("title=\"t\""));
    }

    #[test]
    fn non_ascii_text_around_tags_does_not_panic() {
        // The verbatim-newline scan compares tag names byte by byte; a
        // `&str` slice of the same length would land inside a character.
        assert!(read_html("<p>foo\u{a0}bar</p>").is_ok());
        assert!(read_html("\u{a0}<pre>\nx</pre>").is_ok());
        assert!(read_html("<p>\u{a0}").is_ok());
    }

    #[test]
    fn verbatim_content_survives_nesting_rather_than_being_refused() {
        // `raw_text` walks iteratively, so depth inside a `<pre>` is not
        // a reason to reject a document the bound would otherwise allow.
        let html = "<pre>".to_owned() + &"<span>".repeat(1_000) + "x</pre>";
        let doc = read_html(&html).expect("verbatim depth is not a refusal");
        assert_eq!(
            doc.blocks,
            vec![Block::CodeBlock(Attr::default(), "x".to_owned())]
        );
    }
}
