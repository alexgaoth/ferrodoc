//! Markdown writer: renders the ferrodoc AST back to `CommonMark`, or to
//! `GitHub Flavored Markdown` when the caller asks for it.
//!
//! The contract is semantic, not textual: what this emits must *re-read*
//! to the document it was given. `ferrodoc-harness diff-md` checks exactly
//! that, and it holds for all 652 `CommonMark` spec examples;
//! `diff-gfm-md` does the same for GFM. Output is therefore escaped
//! conservatively — a writer that emits `*` where the source meant a
//! literal asterisk is silently lossy, and that is the only way a markdown
//! writer really fails.
//!
//! Known losses, all of them limits of the format rather than of this
//! code:
//!
//! - `CommonMark` has no tables, footnotes or definition lists. Like
//!   pandoc's `commonmark` writer, those degrade to their content. GFM has
//!   tables; use [`write_gfm`] for anything that has one.
//! - A GFM pipe table keeps its grid and nothing else: extra head rows and
//!   the foot become body rows, merged cells are expanded into separate
//!   ones, a cell's blocks are flattened onto one line, the caption
//!   follows as a paragraph, and column widths are dropped. Each is
//!   stated in [`Writer::pipe_table`] and in `COMPATIBILITY.md`.
//! - Emphasis directly inside emphasis needs `_`, which is not a delimiter
//!   inside a word, so `foo*_bar_*baz` cannot be written.
//! - Two ordered lists in a row that share a delimiter can only be kept
//!   apart by a `<!-- -->` comment, which re-reads as a raw block.
//! - An unterminated raw HTML block swallows the blank line that follows
//!   it, so it absorbs whatever separator a container needs.
//! - A hard break directly after a soft one has no spelling: the line it
//!   would stand on holds nothing else, and a line holding only
//!   whitespace ends the paragraph. It is written as a backslash break,
//!   which keeps the paragraph whole and loses the soft break.

use ferrodoc_ast::{
    Alignment, Block, Inline, ListNumberDelim, MathType, Pandoc, QuoteType, Row, Table, Target,
};
use std::fmt::Write as _;

/// What a footnote body's continuation lines carry, as pandoc writes them.
const INDENT: &str = "    ";

/// Render a document as `CommonMark`.
pub fn write_markdown(doc: &Pandoc) -> String {
    render(doc, false)
}

/// Render a document as `CommonMark`, filled to `columns`.
///
/// See [`write_gfm_wrapped`] for what "breakable" means here.
pub fn write_markdown_wrapped(doc: &Pandoc, columns: usize) -> String {
    render_with(doc, false, Some(columns))
}

/// Render a document as `GitHub Flavored Markdown`, filled to `columns`.
///
/// This is pandoc's `--wrap=auto --columns N`, which is *its* default;
/// ferrodoc's default is `--wrap=preserve`, so [`write_gfm`] never fills.
///
/// A line is broken only where an [`Inline::Space`] or [`Inline::SoftBreak`]
/// stood in the tree. That distinction is the whole of the correctness
/// here: the spaces inside a code span, a link destination and a link
/// title are written by this module rather than read from a `Space`, and
/// breaking at one of those would change what the text means rather than
/// how it looks.
pub fn write_gfm_wrapped(doc: &Pandoc, columns: usize) -> String {
    render_with(doc, true, Some(columns))
}

/// Render a document as `GitHub Flavored Markdown`.
///
/// The difference from [`write_markdown`] is the four GFM constructs a
/// document can actually carry: pipe tables, task list items,
/// strikethrough, and the extra escaping bare URLs need so that text which
/// merely looks like a link does not become one.
///
/// A table always keeps its grid. Everything a pipe table cannot express
/// degrades in a stated way rather than silently: see the module docs.
pub fn write_gfm(doc: &Pandoc) -> String {
    render(doc, true)
}

fn render(doc: &Pandoc, gfm: bool) -> String {
    render_with(doc, gfm, None)
}

fn render_with(doc: &Pandoc, gfm: bool, columns: Option<usize>) -> String {
    let mut out = String::new();
    let mut writer = Writer { gfm, columns, bullet: '-', ..Writer::default() };
    writer.blocks(&mut out, &doc.blocks, "");
    writer.flush_notes(&mut out);
    // Exactly one trailing newline, like every other writer here.
    while out.ends_with("\n\n") {
        out.pop();
    }
    if !out.is_empty() && !out.ends_with('\n') {
        out.push('\n');
    }
    out
}

#[derive(Default)]
struct Writer {
    /// Footnote bodies, collected as they are referenced.
    notes: Vec<String>,
    /// The bullet the next list uses. Two adjacent bullet lists would
    /// merge into one on re-reading; alternating the character splits
    /// them and, unlike a separator comment, adds no block.
    bullet: char,
    /// Whether the emphasis about to be written must use `_` rather than
    /// `*`, because its parent is emphasis that wraps nothing else.
    alternate: bool,
    /// Whether the GFM extensions are available.
    gfm: bool,
    /// The column to fill to, or `None` to leave every line as it falls.
    columns: Option<usize>,
    /// How many **list items** deep the block being written is. A
    /// blockquote does not count: its `> ` prefix makes four more spaces
    /// unambiguous, where a list item's own indentation does not.
    depth: usize,
    /// Whether the previous sibling block was a list. Four spaces after
    /// one are that list's continuation, not a code block.
    after_list: bool,
}

/// Marks a space a line may be broken at. Chosen because no reader here
/// can produce one inside text: `CommonMark` replaces NUL with U+FFFD by
/// specification, and XML — which DOCX, ODT and EPUB are — forbids it
/// outright. Every one is either broken at or turned back into a space
/// before the string leaves [`push_wrapped`].
const BREAK: char = '\0';

impl Writer {
    /// Write blocks separated by blank lines, each line carrying `prefix`
    /// (the `> ` of a quote, or a list item's continuation indent).
    fn blocks(&mut self, out: &mut String, blocks: &[Block], prefix: &str) {
        let mut previous: Option<&Block> = None;
        for block in blocks {
            // A raw block in another format renders to nothing, and its
            // separator goes with it — otherwise a document ending in one
            // ends with a blank line pandoc does not write.
            if matches!(block, Block::RawBlock(format, _) if format.0 != "html") {
                continue;
            }
            if let Some(previous) = previous {
                // The blank line between blocks still belongs to the
                // container: a bare newline inside a quote ends the quote.
                push_line(out, prefix, "");
                // Two bullet lists in a row would merge into one on
                // re-reading; switching the bullet character splits them
                // and, unlike a separator comment, adds no block. Two
                // ordered lists sharing a delimiter have no such escape,
                // so they keep the comment and the round trip loses it.
                match (previous, block) {
                    (Block::BulletList(_), Block::BulletList(_)) => {
                        self.bullet = if self.bullet == '-' { '*' } else { '-' };
                    }
                    (Block::OrderedList(before, _), Block::OrderedList(after, _))
                        if before.delim == after.delim =>
                    {
                        push_line(out, prefix, "<!-- -->");
                        push_line(out, prefix, "");
                    }
                    _ => {}
                }
            }
            self.after_list =
                matches!(previous, Some(Block::BulletList(_) | Block::OrderedList(..)));
            self.block(out, block, prefix);
            previous = Some(block);
        }
    }

    /// Write the collected footnote bodies, which belong at the very end of
    /// the document.
    ///
    /// This is called once, by [`render`], and deliberately not from
    /// [`Writer::blocks`]. It used to live there behind `prefix.is_empty()`
    /// — but a note's *own* body is a nested `blocks` call, and rendering
    /// one with an empty prefix therefore flushed every note collected so
    /// far into it and reset the counter. A two-footnote document came out
    /// with both references labelled `[^1]` and the first body nested
    /// inside the second.
    fn flush_notes(&mut self, out: &mut String) {
        for (index, body) in std::mem::take(&mut self.notes).iter().enumerate() {
            let body = body.trim_end();
            // The body was rendered under `INDENT`; pandoc puts the first
            // line on the label's line and leaves the rest indented.
            let first = body.strip_prefix(INDENT).unwrap_or(body);
            out.push('\n');
            let _ = writeln!(out, "[^{}]: {first}", index + 1);
        }
    }

    /// A code block, fenced or indented.
    /// A code block: indented where that says the same thing, fenced
    /// where it does not.
    ///
    /// **Pandoc indents a block that carries no attributes at all**, and
    /// reaches for a fence only when there is something to put on it. No
    /// round-trip gate could ever see the difference, because both
    /// spellings read back as the same `CodeBlock` — which is why this
    /// wrote a fence for everything until 2026-08-23.
    ///
    /// The three guards are each a `CommonMark` example that failed:
    /// four spaces **inside a list item** are the item's own
    /// indentation, four spaces **after a list** continue it, and an
    /// indented block cannot hold a blank first line or say whether its
    /// content ended with a newline.
    fn code_block(&mut self, out: &mut String, attr: &ferrodoc_ast::Attr, text: &str, prefix: &str) {
        let indentable = attr == &ferrodoc_ast::Attr::default()
            && self.depth == 0
            && !self.after_list
            && !text.is_empty()
            && !text.starts_with('\n')
            && !text.ends_with('\n');
        if indentable {
            for line in text.split('\n') {
                let line = if line.is_empty() { String::new() } else { format!("    {line}") };
                push_line(out, prefix, &line);
            }
            return;
        }
        // A fence must be longer than any backtick run inside.
        let longest = text
            .split(|c| c != '`')
            .map(str::len)
            .max()
            .unwrap_or(0);
        let fence = "`".repeat(longest.max(2) + 1);
        // Pandoc's HTML writer tags every code block `sourceCode`,
        // and its markdown writers drop that class and spell the
        // first one left, after a space: `["sourceCode","bash"]`
        // is ```` ``` bash ````, `["sourceCode"]` a bare fence.
        let info = attr
            .classes
            .iter()
            .find(|class| class.as_str() != "sourceCode")
            .map_or(String::new(), |class| format!(" {class}"));
        push_line(out, prefix, &format!("{fence}{info}"));
        for line in text.split('\n') {
            push_line(out, prefix, line);
        }
        push_line(out, prefix, &fence);
            
    
    }

    fn block(&mut self, out: &mut String, block: &Block, prefix: &str) {
        match block {
            Block::Plain(inlines) | Block::Para(inlines) => {
                let text = self.inlines(inlines);
                push_wrapped(out, prefix, &text, self.columns);
            }
            Block::Header(level, _, inlines) => {
                let text = self.inlines(inlines);
                header(out, prefix, *level, &text);
            }
            Block::CodeBlock(attr, text) => self.code_block(out, attr, text, prefix),
            Block::BlockQuote(blocks) => {
                if blocks.is_empty() {
                    // A quote with no content is still a quote; writing
                    // nothing would delete the block.
                    push_line(out, prefix, ">");
                } else {
                    let inner = format!("{prefix}> ");
                    self.blocks(out, blocks, &inner);
                }
            }
            Block::BulletList(items) => self.bullet_list(out, items, prefix),
            Block::OrderedList(attrs, items) => {
                // CommonMark numbers every ordered list; the roman and
                // alphabetic styles have no syntax and degrade to numbers.
                let (start, delim) = (attrs.start, attrs.delim);
                self.list(out, items, prefix, move |index| {
                    let label = (start + i64::try_from(index).unwrap_or(0)).to_string();
                    let close = match delim {
                        ListNumberDelim::OneParen | ListNumberDelim::TwoParens => ')',
                        _ => '.',
                    };
                    // Pandoc pads the marker to four columns, or to one
                    // past its own length when that is wider: `1.  `,
                    // `10. `, `100. `. The continuation indent follows
                    // from it, because the indent is the marker's width.
                    let marker = format!("{label}{close}");
                    format!("{marker:<width$}", width = marker.len().max(3) + 1)
                });
            }
            Block::DefinitionList(items) => {
                // No CommonMark syntax: term then definition as paragraphs.
                let mut first = true;
                for (term, definitions) in items {
                    if !first {
                        push_line(out, prefix, "");
                    }
                    first = false;
                    let text = self.inlines(term);
                    push_wrapped(out, prefix, &text, self.columns);
                    for definition in definitions {
                        push_line(out, prefix, "");
                        self.blocks(out, definition, prefix);
                    }
                }
            }
            // `***`, not `---`: inside a list item `- ---` is itself a
            // thematic break, so the item disappears.
            // Pandoc's rule is 72 dashes, and it is only safe at the top
            // level: inside a list item a run of dashes is read as the
            // *next item's* setext underline, and `CommonMark` example 61
            // (`- Foo\n- * * *`) loses its rule. `***` reads back the same
            // either way, so the container keeps it.
            Block::HorizontalRule => {
                let rule = if self.depth == 0 { "-".repeat(72) } else { "***".to_owned() };
                push_line(out, prefix, &rule);
            }
            Block::LineBlock(lines) => {
                // Hard breaks preserve the line structure.
                let mut text = String::new();
                for (index, line) in lines.iter().enumerate() {
                    if index > 0 {
                        text.push_str("  \n");
                    }
                    text.push_str(&self.inlines(line));
                }
                push_wrapped(out, prefix, &text, self.columns);
            }
            Block::RawBlock(format, text) => {
                if format.0 == "html" {
                    for line in text.trim_end_matches('\n').split('\n') {
                        push_line(out, prefix, line);
                    }
                }
            }
            Block::Div(_, blocks) => self.blocks(out, blocks, prefix),
            // A table with no columns has no pipe-table spelling at all —
            // not even an empty one — so it degrades like the rest.
            Block::Table(table) if self.gfm && !table.colspecs.is_empty() => {
                self.pipe_table(out, table, prefix);
            }
            Block::Figure(..) | Block::Table(_) => self.unrepresentable(out, block, prefix),
        }
    }

    fn bullet_list(&mut self, out: &mut String, items: &[Vec<Block>], prefix: &str) {
        // The bullet is fixed for the whole list: a marker that changed
        // between items would split the list in two.
        let bullet = self.bullet;
        let saved = self.bullet;
        // A `☐`/`☒` opening an item is what a GFM task item reads as, so
        // write it back as one. Both spellings re-read to the same
        // document; the brackets are the one a reader recognizes.
        let tasks: Vec<Option<bool>> = if self.gfm {
            items.iter().map(|item| task_state(item)).collect()
        } else {
            vec![None; items.len()]
        };
        if tasks.iter().any(Option::is_some) {
            let stripped: Vec<Vec<Block>> = items
                .iter()
                .zip(&tasks)
                .map(|(item, task)| {
                    if task.is_some() { without_task_marker(item) } else { item.clone() }
                })
                .collect();
            self.list(out, &stripped, prefix, |index| match tasks[index] {
                Some(true) => format!("{bullet} [x] "),
                Some(false) => format!("{bullet} [ ] "),
                None => format!("{bullet} "),
            });
        } else {
            self.list(out, items, prefix, move |_| format!("{bullet} "));
        }
        self.bullet = saved;
    }

    /// Write a table as a GFM pipe table.
    ///
    /// The grid always survives; what a pipe table cannot hold degrades in
    /// one of four stated ways. Extra head rows and the foot become body
    /// rows, because GFM has exactly one header row and no foot. A cell
    /// spanning columns is expanded into that many cells, the content in
    /// the first and the rest empty, and a cell spanning rows simply
    /// leaves the rows below it short, which pads them out — a pipe
    /// table's columns are positional and cannot merge. A cell's blocks
    /// are flattened onto one line, joined by spaces, because a row is one
    /// line. The caption follows the table as an ordinary paragraph.
    fn pipe_table(&mut self, out: &mut String, table: &Table, prefix: &str) {
        let columns = table.colspecs.len();
        if columns == 0 {
            return;
        }
        let mut rows = table.head.rows.iter();
        let header = rows.next();
        let body = rows
            .chain(table.bodies.iter().flat_map(|b| b.head.iter().chain(&b.body)))
            .chain(&table.foot.rows);

        // The header row decides the column count, so it must exist even
        // when the table has no head: an empty one still reads back as a
        // table of the right shape.
        let cells = header.map(|row| self.cells(row, columns)).unwrap_or_default();
        push_line(out, prefix, &pipe_row(&cells, columns));
        let rule: Vec<String> = table
            .colspecs
            .iter()
            .map(|spec| match spec.alignment {
                Alignment::AlignLeft => ":---".to_owned(),
                Alignment::AlignCenter => ":---:".to_owned(),
                Alignment::AlignRight => "---:".to_owned(),
                Alignment::AlignDefault => "----".to_owned(),
            })
            .collect();
        push_line(out, prefix, &pipe_row(&rule, columns));
        for row in body {
            let cells = self.cells(row, columns);
            push_line(out, prefix, &pipe_row(&cells, columns));
        }
        if !table.caption.blocks.is_empty() {
            push_line(out, prefix, "");
            self.blocks(out, &table.caption.blocks, prefix);
        }
    }

    /// One row's cells, rendered to single-line text with spans expanded.
    fn cells(&mut self, row: &Row, columns: usize) -> Vec<String> {
        let mut out: Vec<String> = Vec::with_capacity(columns);
        for cell in &row.cells {
            let mut text = String::new();
            for block in &cell.blocks {
                let mut piece = String::new();
                self.block(&mut piece, block, "");
                let piece = piece.trim_end_matches('\n');
                if !text.is_empty() && !piece.is_empty() {
                    text.push(' ');
                }
                text.push_str(piece);
            }
            out.push(cell_text(&text));
            // A spanned cell occupies further columns that carry nothing.
            for _ in 1..cell.col_span.max(1) {
                out.push(String::new());
            }
        }
        out
    }

    /// Blocks `CommonMark` has no syntax for: emit their content rather
    /// than dropping it.
    fn unrepresentable(&mut self, out: &mut String, block: &Block, prefix: &str) {
        match block {
            Block::Figure(_, caption, blocks) => {
                self.blocks(out, blocks, prefix);
                if !caption.blocks.is_empty() {
                    push_line(out, prefix, "");
                    self.blocks(out, &caption.blocks, prefix);
                }
            }
            Block::Table(table) => {
                let rows = table
                    .head
                    .rows
                    .iter()
                    .chain(
                        table
                            .bodies
                            .iter()
                            .flat_map(|b| b.head.iter().chain(&b.body)),
                    )
                    .chain(&table.foot.rows);
                let mut first = true;
                for row in rows {
                    for cell in &row.cells {
                        if !first {
                            push_line(out, prefix, "");
                        }
                        first = false;
                        self.blocks(out, &cell.blocks, prefix);
                    }
                }
            }
            _ => {}
        }
    }

    /// Write list items, each first line carrying `marker(index)` and the
    /// rest indented to line up under it.
    fn list(
        &mut self,
        out: &mut String,
        items: &[Vec<Block>],
        prefix: &str,
        marker: impl Fn(usize) -> String,
    ) {
        // A list whose items are all `Plain` is tight: no blank lines.
        // A pipe table needs one anyway — it cannot interrupt a paragraph.
        let tight = items
            .iter()
            .all(|item| item.iter().all(|b| !matches!(b, Block::Para(_) | Block::Table(_))));
        for (index, item) in items.iter().enumerate() {
            if index > 0 && !tight {
                push_line(out, prefix, "");
            }
            let marker = marker(index);
            // Continuation lines line up under the item's *content*, which
            // starts after the list marker — `- `, `10. `, `1.  `, and for
            // a task item still `- `, since `[x] ` is already content. The
            // marker's list part ends after its **first run of spaces**,
            // not at the first one: `1.  ` is four columns wide and taking
            // three of them put a stray space inside every fenced code
            // block in a list.
            let indent = " ".repeat(
                marker
                    .find(' ')
                    .map_or(marker.len(), |i| i + marker[i..].len() - marker[i..].trim_start_matches(' ').len()),
            );
            // A tight list's items must not contain blank lines: one
            // would make the whole list loose when it is read back, and
            // every `Plain` inside would come back as a `Para`.
            //
            // When filling, the item's content is rendered under `indent`
            // so the fill pays for the marker: pandoc puts `- word word
            // word` on a 20-column line, not `- word word word word`.
            // The marker then replaces the indent on the first line. When
            // not filling the prefix changes nothing, so it is left empty
            // and the indent is added per line below, exactly as before.
            let inner = if self.columns.is_some() { indent.as_str() } else { "" };
            let mut body = String::new();
            self.depth += 1;
            if tight {
                for block in item {
                    self.after_list = false;
                    self.block(&mut body, block, inner);
                }
            } else {
                self.blocks(&mut body, item, inner);
            }
            self.depth -= 1;
            let mut lines = body.trim_end_matches('\n').split('\n');
            if let Some(first) = lines.next() {
                let first = first.strip_prefix(inner).unwrap_or(first);
                push_line(out, prefix, &format!("{marker}{first}"));
            }
            for line in lines {
                if line.is_empty() {
                    push_line(out, prefix, "");
                } else if inner.is_empty() {
                    push_line(out, prefix, &format!("{indent}{line}"));
                } else {
                    push_line(out, prefix, line);
                }
            }
        }
    }

    // --- inlines ---

    /// The body of an emphasis node. `alternate` says the child must not
    /// reuse `*`, which is true only for emphasis directly inside
    /// emphasis: there the two delimiters merge and `**x**` reads back as
    /// strong. Everything else concatenates correctly, and `_` is only a
    /// delimiter at a word boundary, so it is not offered more widely.
    fn nested(&mut self, inner: &[Inline], alternate: bool) -> String {
        self.alternate = alternate;
        let text = self.inlines(inner);
        self.alternate = false;
        text
    }

    /// Write GFM strikeout, never emitting a `~~` that touches another.
    ///
    /// `~~` is a flanking delimiter: whitespace just inside it stops the
    /// run opening or closing, so the spaces move outside. And two runs
    /// that meet make four tildes — not a delimiter at all but a tilde
    /// code fence, which swallows the rest of the document. Where that
    /// would happen the markup degrades to its content instead.
    /// An inline written as a raw HTML element, for the constructs
    /// markdown has no syntax for.
    fn tagged(&mut self, out: &mut String, tag: &str, attributes: &str, inner: &[Inline]) {
        let text = self.inlines(inner);
        let _ = write!(out, "<{tag}{attributes}>{text}</{tag}>");
    }

    fn strikeout(&mut self, out: &mut String, inner: &[Inline]) {
        let text = self.inlines(inner);
        let body = text.trim_matches([' ', '\n', BREAK]);
        // A tilde at either edge is strikeout immediately inside
        // strikeout; nesting with text in between spells out fine.
        if body.is_empty() || body.starts_with('~') || body.ends_with('~') {
            out.push_str(&text);
            return;
        }
        let lead = &text[..text.len() - text.trim_start_matches([' ', '\n', BREAK]).len()];
        let tail = &text[text.trim_end_matches([' ', '\n', BREAK]).len()..];
        if lead.is_empty() && out.ends_with("~~") {
            // Reopening the previous run keeps every word struck. The two
            // nodes arrive back as one, which is what pandoc's own HTML
            // reader makes of adjacent `<del>` anyway.
            out.truncate(out.len() - 2);
            let _ = write!(out, "{body}~~{tail}");
        } else {
            let _ = write!(out, "{lead}~~{body}~~{tail}");
        }
    }

    fn inlines(&mut self, inlines: &[Inline]) -> String {
        let mut out = String::new();
        for inline in inlines {
            // A `!` ending one inline and the `[` starting the next make
            // an image across a boundary [`escape_text`] cannot see: the
            // `!` is the last character of its own `Str`, so there is no
            // next character to test. `CommonMark` example 593 is
            // `\![foo]` beside a reference definition, which came back as
            // an `Image`.
            if out.ends_with('!') && !out.ends_with("\\!") && opens_bracket(inline) {
                out.insert(out.len() - 1, '\\');
            }
            self.inline(&mut out, inline);
        }
        out
    }

    fn inline(&mut self, out: &mut String, inline: &Inline) {
        match inline {
            Inline::Str(text) => escape_text(out, text, self.gfm),
            Inline::Space => out.push(if self.columns.is_some() { BREAK } else { ' ' }),
            // A soft break is a real line break in the source, and this
            // writer never re-wraps, so keeping it is both faithful and
            // free. Callers that cannot hold a newline flatten it back.
            // Filling re-flows, so a soft break becomes just another place
            // the line may be broken; preserving keeps it where it was.
            Inline::SoftBreak => out.push(if self.columns.is_some() { BREAK } else { '\n' }),
            // Two trailing spaces before the newline is a hard break —
            // but on an otherwise empty line they are just whitespace, so
            // the line reads as a blank one and splits the paragraph in
            // two. A backslash is a hard break wherever it stands.
            Inline::LineBreak => {
                if out.is_empty() || out.ends_with('\n') {
                    out.push_str("\\\n");
                } else {
                    out.push_str("  \n");
                }
            }
            // Emphasis wrapping nothing but emphasis must alternate
            // delimiters: `**foo**` is strong, not emphasis in emphasis.
            Inline::Emph(inner) => {
                let delimiter = if self.alternate { "_" } else { "*" };
                let text = self.nested(inner, matches!(inner[..], [Inline::Emph(_)]));
                let _ = write!(out, "{delimiter}{text}{delimiter}");
            }
            Inline::Strong(inner) => {
                // `**` nests by concatenation — `****x****` reads back as
                // strong within strong — so it never needs the alternate.
                let text = self.nested(inner, false);
                let _ = write!(out, "**{text}**");
            }
            Inline::Strikeout(inner) if self.gfm => self.strikeout(out, inner),
            // No markdown syntax for any of these, so pandoc falls back to
            // raw HTML — which markdown allows inline and every renderer
            // shows. Dropping the tag instead loses meaning rather than
            // styling: `H~2~O` became `H2O` and an anchor a link pointed
            // at disappeared, both silently. Measured against pandoc 3.8.2.1
            // one construct at a time.
            Inline::Strikeout(inner) => self.tagged(out, "s", "", inner),
            Inline::Superscript(inner) => self.tagged(out, "sup", "", inner),
            Inline::Subscript(inner) => self.tagged(out, "sub", "", inner),
            Inline::Underline(inner) => self.tagged(out, "u", "", inner),
            Inline::SmallCaps(inner) => {
                self.tagged(out, "span", " class=\"smallcaps\"", inner);
            }
            Inline::Span(attr, inner) => {
                let attributes = html_attributes(attr);
                // A span carrying nothing is only a wrapper; pandoc writes
                // no tag for it either.
                if attributes.is_empty() {
                    let text = self.inlines(inner);
                    out.push_str(&text);
                } else {
                    self.tagged(out, "span", &attributes, inner);
                }
            }
            // A citation renders as the text it stands for: pandoc's
            // `citeproc` is what turns one into a reference, and this is
            // not that.
            Inline::Cite(_, inner) => {
                let text = self.inlines(inner);
                out.push_str(&text);
            }
            Inline::Quoted(quote, inner) => {
                let (open, close) = match quote {
                    QuoteType::SingleQuote => ('\u{2018}', '\u{2019}'),
                    QuoteType::DoubleQuote => ('\u{201C}', '\u{201D}'),
                };
                out.push(open);
                out.push_str(&self.inlines(inner));
                out.push(close);
            }
            Inline::Code(_, text) => {
                // The delimiter must be longer than any run inside, and a
                // literal backtick at either end needs padding spaces.
                let longest = text.split(|c| c != '`').map(str::len).max().unwrap_or(0);
                let ticks = "`".repeat(longest + 1);
                // A reader strips one space from each end of a code span
                // that begins and ends with one, unless it is all spaces.
                // Padding both ends makes that strip give the text back,
                // and also keeps a leading or trailing backtick separate
                // from the delimiter. All-space content is never stripped,
                // so padding it would corrupt it instead.
                let all_spaces = text.chars().all(|c| c == ' ');
                // Pandoc pads **whenever the delimiter is longer than one
                // backtick**, not only where a backtick sits at the edge:
                // `` a`b `` rather than ``a`b``. Both read back the same;
                // the bytes are the test.
                let pad = if !all_spaces
                    && (ticks.len() > 1 || text.starts_with(' ') || text.ends_with(' '))
                {
                    " "
                } else {
                    ""
                };
                let _ = write!(out, "{ticks}{pad}{text}{pad}{ticks}");
            }
            Inline::Math(kind, text) => {
                // Verbatim: escaping corrupts the TeX, since `\\` is a
                // MathJax line break and `\_` a literal underscore. Probed
                // against pandoc's `ipynb` writer, whose spelling this is.
                // Pandoc's `gfm` writer instead emits GitHub's `` $`x`$ `` and
                // a ```` ```math ```` fence; one writer serves both here, and
                // the dollar form is the one both readers accept.
                write_math(out, *kind, text);
            }
            Inline::Link(_, inner, target) => {
                let text = self.inlines(inner);
                if write_autolink(out, inner, target) {
                    return;
                }
                let _ = write!(out, "[{text}]({}", link_destination(&target.url));
                if !target.title.is_empty() {
                    let _ = write!(out, " \"{}\"", target.title.replace('"', "\\\""));
                }
                out.push(')');
            }
            Inline::Image(_, alt, target) => {
                let text = self.inlines(alt);
                let _ = write!(out, "![{text}]({}", link_destination(&target.url));
                if !target.title.is_empty() {
                    let _ = write!(out, " \"{}\"", target.title.replace('"', "\\\""));
                }
                out.push(')');
            }
            Inline::RawInline(format, text) => {
                if format.0 == "html" {
                    out.push_str(text);
                }
            }
            Inline::Note(blocks) => {
                // Reserve the number before rendering: a note nested inside
                // this one would otherwise take this one's label.
                let index = self.notes.len();
                self.notes.push(String::new());
                let mut body = String::new();
                self.blocks(&mut body, blocks, INDENT);
                self.notes[index] = body;
                let _ = write!(out, "[^{}]", index + 1);
            }
        }
    }
}

/// Whether a list item opens with the `☐`/`☒` a GFM task item reads as,
/// and if so whether it is checked.
fn task_state(item: &[Block]) -> Option<bool> {
    let (Block::Plain(inlines) | Block::Para(inlines)) = item.first()? else {
        return None;
    };
    match inlines.first()? {
        Inline::Str(marker) if marker == "\u{2610}" => Some(false),
        Inline::Str(marker) if marker == "\u{2612}" => Some(true),
        _ => None,
    }
}

/// The item without the `☐`/`☒` and the space after it, which the `[ ]`
/// marker replaces.
fn without_task_marker(item: &[Block]) -> Vec<Block> {
    let mut item = item.to_vec();
    if let Some(Block::Plain(inlines) | Block::Para(inlines)) = item.first_mut() {
        let drop = if matches!(inlines.get(1), Some(Inline::Space)) { 2 } else { 1 };
        inlines.drain(..drop);
    }
    item
}

/// A pipe-table row, padded with empty cells or truncated to exactly
/// `columns` of them — the delimiter row sets the width and a row that
/// disagrees would not be part of the table.
fn pipe_row(cells: &[String], columns: usize) -> String {
    let mut line = String::from("|");
    for index in 0..columns {
        let _ = write!(line, " {} |", cells.get(index).map_or("", String::as_str));
    }
    line
}

/// Cell content reduced to what fits between two pipes: a bare `|` would
/// end the cell and a newline would end the row. A reader unescapes `\|`
/// before it parses the cell, so the escape survives even inside a code
/// span.
fn cell_text(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut backslashes = 0usize;
    for ch in text.chars() {
        match ch {
            // Text has already been escaped once; escaping it again would
            // leave a literal backslash and a live cell divider behind.
            // Only the pipes markup carries — inside a code span, a raw
            // inline, a URL — arrive here bare.
            '|' if backslashes % 2 == 1 => out.push('|'),
            '|' => out.push_str("\\|"),
            // A row is one line, so a break opportunity in a cell is a
            // space and nothing else — pandoc does not fill inside a
            // pipe table either.
            '\n' | BREAK => out.push(' '),
            ch => out.push(ch),
        }
        backslashes = if ch == '\\' { backslashes + 1 } else { 0 };
    }
    out
}

/// A link destination, wrapped in angle brackets when it needs them.
///
/// A destination is read with backslash escapes active in both forms, so a
/// URL containing `\` must double it or the character is eaten. Angle
/// brackets additionally cannot hold a bare `<` or `>`.
fn link_destination(url: &str) -> String {
    let mut escaped = String::new();
    for ch in url.chars() {
        if matches!(ch, '\\' | '<' | '>') {
            escaped.push('\\');
        }
        escaped.push(ch);
    }
    if url.is_empty() || url.contains([' ', '(', ')']) {
        format!("<{escaped}>")
    } else {
        escaped
    }
}

/// Append one prefixed line.
fn push_line(out: &mut String, prefix: &str, line: &str) {
    if line.is_empty() {
        // A blank line inside a quote still carries its marker, trimmed.
        out.push_str(prefix.trim_end());
    } else {
        out.push_str(prefix);
        out.push_str(line);
    }
    out.push('\n');
}

/// Append text that may already contain hard-break newlines.
fn push_wrapped(out: &mut String, prefix: &str, text: &str, columns: Option<usize>) {
    let Some(columns) = columns else {
        debug_assert!(!text.contains(BREAK));
        for line in text.split('\n') {
            push_line(out, prefix, line);
        }
        return;
    };
    // The prefix is part of the line pandoc counts, so a quote or a list
    // item fills to less than the full width.
    let width = columns.saturating_sub(prefix.chars().count()).max(1);
    for paragraph in text.split('\n') {
        for line in fill(paragraph, width) {
            push_line(out, prefix, &line);
        }
    }
}

/// Greedy fill: take words while they fit, break at the last space that
/// did. A word longer than the width goes on its own line and overruns —
/// breaking inside it would invent a break the text does not have.
fn fill(text: &str, width: usize) -> Vec<String> {
    let mut lines = Vec::new();
    let mut line = String::new();
    let mut line_width = 0;
    for word in text.split(BREAK) {
        let word_width = word.chars().count();
        if line.is_empty() {
            line.push_str(word);
            line_width = word_width;
        } else if line_width + 1 + word_width <= width {
            line.push(' ');
            line.push_str(word);
            line_width += 1 + word_width;
        } else {
            lines.push(std::mem::take(&mut line));
            line.push_str(word);
            line_width = word_width;
        }
    }
    lines.push(line);
    lines
}

/// Write a heading whose inlines have already been rendered to `text`.
///
/// **A heading is never filled**, whatever `--columns` says, because an
/// ATX heading is one line by construction — pandoc leaves a 151-column
/// heading at 151 columns, measured.
fn header(out: &mut String, prefix: &str, level: i64, text: &str) {
    let unbroken = text.replace(BREAK, " ");
    let text = unbroken.as_str();
    // An ATX heading is one line. A setext heading is not, so levels 1 and
    // 2 can keep a line break; deeper ones must flatten it, which
    // `CommonMark` gives no way around.
    if text.contains('\n') && (level == 1 || level == 2) {
        push_wrapped(out, prefix, text, None);
        push_line(out, prefix, if level == 1 { "===" } else { "---" });
        return;
    }
    let hashes = "#".repeat(usize::try_from(level).unwrap_or(1).clamp(1, 6));
    let text = text.replace('\n', " ");
    // A trailing `#` run would be read as the heading's closing sequence;
    // one escape anywhere inside the run is enough to stop that.
    let stripped = text.trim_end_matches('#');
    let text = if stripped.len() == text.len() {
        text.clone()
    } else {
        format!("{stripped}\\{}", &text[stripped.len()..])
    };
    push_line(out, prefix, &format!("{hashes} {text}"));
}

/// Whether everything written on the current line is digits, and there is
/// at least one — the only position where a following `.` or `)` opens an
/// ordered list. Scans backwards over the digit run only, so prose (whose
/// last character is rarely a digit) costs nothing.
fn digits_since_line_start(out: &str) -> bool {
    let before = out.trim_end_matches(|c: char| c.is_ascii_digit());
    before.len() < out.len() && (before.is_empty() || before.ends_with('\n'))
}

/// Write `$x$` or `$$x$$`, content untouched.
fn write_math(out: &mut String, kind: MathType, text: &str) {
    let delimiter = match kind {
        MathType::InlineMath => "$",
        MathType::DisplayMath => "$$",
    };
    let _ = write!(out, "{delimiter}{text}{delimiter}");
}

/// Write a classed autolink in its `<…>` form, returning whether it did.
///
/// Probed: pandoc writes an autolink back as `<url>`, and that is what keeps
/// the `uri`/`email` class its ipynb reader assigns. Written as
/// `[text](url)` instead, the class is lost on the next read and the writer
/// gate fails on a document it otherwise matches.
///
/// The comparison is against the *unescaped* `Str`: the rendered text has
/// already had `https\://…` escaped into it, which never equals the target.
fn write_autolink(out: &mut String, inner: &[Inline], target: &Target) -> bool {
    let [Inline::Str(literal)] = inner else { return false };
    // **The class is not the test — the text is.** This asked for a
    // `uri`/`email` class, which pandoc's own GFM reader does not attach,
    // so every autolink in a README came back as
    // `[https\://x](https://x)`. Pandoc writes `<url>` for any link whose
    // text is its target, and the classed links the ipynb reader produces
    // are the same shape.
    //
    // A title keeps the long form rather than matching: pandoc drops it
    // here, and a title that vanishes is content lost for a byte.
    let same = *literal == target.url;
    let mail = target.url.strip_prefix("mailto:") == Some(literal.as_str());
    if target.title.is_empty() && (same || mail) {
        let _ = write!(out, "<{literal}>");
        return true;
    }
    false
}

/// Whether this inline's rendering starts with `[`, which a preceding
/// `!` would turn into an image. An `Image` does not count: it writes its
/// own `!`, so the pair is `!![…]` and the first one stays literal.
fn opens_bracket(inline: &Inline) -> bool {
    match inline {
        Inline::Link(..) => true,
        Inline::Str(text) => text.starts_with('['),
        _ => false,
    }
}

/// Whether `ch`, appended here, would complete one of GFM's autolink
/// triggers: a `www.` host, an `http`/`https`/`ftp` scheme, or the `@` of
/// an email address.
fn opens_autolink(out: &str, ch: char) -> bool {
    let before = |word: &str| {
        out.strip_suffix(word)
            .is_some_and(|rest| !rest.ends_with(|c: char| c.is_alphanumeric()))
    };
    match ch {
        ':' => before("http") || before("https") || before("ftp"),
        '.' => before("www"),
        // Anything the local part of an address may end with.
        '@' => out.ends_with(|c: char| c.is_alphanumeric() || matches!(c, '.' | '+' | '-' | '_')),
        _ => false,
    }
}

/// Escape text so it re-reads as itself.
///
/// **Pandoc's rules, probed one character in five contexts at a time**
/// (`scripts/writers.sh` is the check). It escaped more than pandoc for a
/// long time, which is safe and is not the same output: `a_b` was
/// `a\_b`, every `&` was `&amp;`, and a tab was `&#9;` — so a README
/// converted here and there differed on nearly every line.
///
/// Three places keep the wider escape on purpose, each because pandoc's
/// own output does not read back as what went in. They are the only ones,
/// and `COMPATIBILITY.md` has the repro for each:
///
/// - a backslash **before ASCII punctuation**. Pandoc writes `\\` and
///   then *drops the character*: `a\*b` comes out `a\\b`, which reads
///   back as `a\b`;
/// - an `&` that starts something entity-shaped. Pandoc writes it bare,
///   so `a&amp;b` reads back as `a&b`;
/// - the GFM autolink triggers. Pandoc writes literal `http://x` bare and
///   its own reader turns the text into a link.
fn escape_text(out: &mut String, text: &str, gfm: bool) {
    let mut chars = text.chars().peekable();
    while let Some(ch) = chars.next() {
        let next = chars.peek().copied();
        let at_line_start = out.is_empty() || out.ends_with('\n');
        // A marker only opens a block when the line breaks after it, so
        // `-b` is text and `- b` is a list. Pandoc splits on exactly that.
        let opens_block = next.is_none_or(|c| c == ' ');
        match ch {
            '*' | '[' | ']' | '<' | '>' | '`' | '$' => {
                out.push('\\');
                out.push(ch);
            }
            // `_` between two alphanumerics is not emphasis in CommonMark,
            // and `snake_case` is most of what a technical document holds.
            '_' if !(out.ends_with(|c: char| c.is_alphanumeric())
                && next.is_some_and(char::is_alphanumeric)) =>
            {
                out.push('\\');
                out.push(ch);
            }
            // Only where it would be read as an escape. See the note above
            // for why this is wider than pandoc's.
            '\\' if next.is_none_or(|c| c.is_ascii_punctuation()) => out.push_str("\\\\"),
            // `|` divides a table row anywhere, and `~` opens strikeout.
            //
            // **Every** `~`, where pandoc escapes only a doubled one.
            // Pandoc can afford the narrower rule because its own reader
            // needs two tildes; this reader strikes on one (`~b~` is
            // `Strikeout` here and `Str "~b~"` there), so a bare `~`
            // would come back as markup. The divergence is the reader's,
            // and until it is settled the writer has to be safe rather
            // than identical.
            '~' | '|' if gfm => {
                out.push('\\');
                out.push(ch);
            }
            // Text that merely looks like a link becomes one under GFM's
            // extended autolinks. One escape inside the trigger stops
            // that, and the reader gives the character back.
            ':' | '.' | '@' if gfm && opens_autolink(out, ch) => {
                out.push('\\');
                out.push(ch);
            }
            // These only mean something at the start of a line. `#` needs
            // no space after it to be a heading, so it has no `opens_block`.
            '#' if at_line_start => {
                out.push('\\');
                out.push(ch);
            }
            // `-` and `+` open a list only with a space after them — but
            // a *run* of `-` or `=` under a paragraph line is a setext
            // heading, which is not a block opener at all and is why this
            // is wider than pandoc's rule. Pandoc loses `Foo\nbar\n\---`
            // to a heading; `CommonMark` example 106 is that document.
            '-' | '+' if at_line_start && (opens_block || next == Some(ch)) => {
                out.push('\\');
                out.push(ch);
            }
            '=' if at_line_start && next == Some('=') => {
                out.push('\\');
                out.push(ch);
            }
            // `1.` and `1)` open an ordered list, but only where the line
            // so far is nothing but the number.
            '.' | ')' if digits_since_line_start(out) && opens_block => {
                out.push('\\');
                out.push(ch);
            }
            // Only dangerous immediately before a link. Pandoc writes
            // `\![` and lets the bracket stand, relying on the escaped
            // `]` to stop a link forming — which fails when the very next
            // inline **is** a link and supplies the `](url)` itself.
            // `CommonMark` example 593 is that document.
            '!' if next == Some('[') => out.push_str("\\!"),
            '&' if entity_ahead(next, &chars) => out.push_str("&amp;"),
            // A literal newline inside a `Str` is text, not structure:
            // written raw it would split the paragraph. A tab is only
            // structure at the start of a line, where it opens an
            // indented code block.
            '\n' => out.push_str("&#10;"),
            '\t' if at_line_start => out.push_str("&#9;"),
            ch => out.push(ch),
        }
    }
}

/// Whether the `&` just read starts an HTML entity, which is the only
/// case where writing it bare changes what the text says.
fn entity_ahead(next: Option<char>, rest: &std::iter::Peekable<std::str::Chars<'_>>) -> bool {
    let Some(first) = next else { return false };
    let tail: String = rest.clone().take(34).collect();
    let body = if first == '#' {
        tail[1..].trim_start_matches(|c: char| c.is_ascii_hexdigit() || c == 'x')
    } else if first.is_ascii_alphabetic() {
        tail.trim_start_matches(|c: char| c.is_ascii_alphanumeric())
    } else {
        return false;
    };
    body.starts_with(';') && body.len() < tail.len()
}

/// An `Attr` as HTML attributes: `id` first, then the classes as one
/// `class`, then the key-value pairs in order. Pandoc's order, and the
/// empty string when the attribute carries nothing.
fn html_attributes(attr: &ferrodoc_ast::Attr) -> String {
    let mut out = String::new();
    if !attr.identifier.is_empty() {
        let _ = write!(out, " id=\"{}\"", escape_attribute(&attr.identifier));
    }
    if !attr.classes.is_empty() {
        let _ = write!(out, " class=\"{}\"", escape_attribute(&attr.classes.join(" ")));
    }
    for (key, value) in &attr.attributes {
        let _ = write!(out, " {key}=\"{}\"", escape_attribute(value));
    }
    out
}

fn escape_attribute(value: &str) -> String {
    value.replace('&', "&amp;").replace('"', "&quot;").replace('<', "&lt;")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::read_commonmark;
    use ferrodoc_ast::Attr;

    /// Markdown has no syntax for these, so pandoc writes raw HTML and so
    /// does this. Dropping the tag loses meaning, not styling: `H~2~O`
    /// came out as `H2O`, and an anchor a link pointed at vanished.
    ///
    /// No gate could see it. `diff-md` and `diff-gfm-md` are round trips
    /// through *this crate's* reader, which never produces a `Superscript`
    /// — `CommonMark` has no `^x^` — so the arm was never reached.
    #[test]
    fn the_constructs_markdown_cannot_spell_are_written_as_html() {
        let one = |inline: Inline| {
            write_gfm(&Pandoc::new(vec![Block::Para(vec![inline])])).trim_end().to_owned()
        };
        let text = || vec![Inline::Str("x".to_owned())];
        assert_eq!(one(Inline::Superscript(text())), "<sup>x</sup>");
        assert_eq!(one(Inline::Subscript(text())), "<sub>x</sub>");
        assert_eq!(one(Inline::Underline(text())), "<u>x</u>");
        assert_eq!(one(Inline::SmallCaps(text())), "<span class=\"smallcaps\">x</span>");
        // GFM has strikeout; CommonMark does not and takes the tag.
        assert_eq!(one(Inline::Strikeout(text())), "~~x~~");
        let commonmark = write_markdown(&Pandoc::new(vec![Block::Para(vec![
            Inline::Strikeout(text()),
        ])]));
        assert_eq!(commonmark.trim_end(), "<s>x</s>");
    }

    /// A span carrying an identifier is an anchor something links to.
    /// Converting an EPUB to markdown dropped every one of them, so the
    /// cross-references in the output pointed at nothing.
    #[test]
    fn a_span_keeps_the_attributes_that_make_it_an_anchor() {
        use ferrodoc_ast::Attr;
        let attr = Attr {
            identifier: "one.xhtml".to_owned(),
            classes: vec!["c".to_owned()],
            attributes: Vec::new(),
        };
        let written = write_gfm(&Pandoc::new(vec![Block::Para(vec![Inline::Span(
            Box::new(attr),
            Vec::new(),
        )])]));
        assert_eq!(written.trim_end(), "<span id=\"one.xhtml\" class=\"c\"></span>");
        // A span with nothing to say is only a wrapper, and pandoc writes
        // no tag for it either.
        let bare = write_gfm(&Pandoc::new(vec![Block::Para(vec![Inline::Span(
            Box::default(),
            vec![Inline::Str("x".to_owned())],
        )])]));
        assert_eq!(bare.trim_end(), "x");
    }

    /// Pandoc's HTML writer classes every code block `sourceCode`, so
    /// `html → gfm` fed the whole class list to a writer that took the
    /// first one: every code block came out ```` ```sourceCode ````.
    ///
    /// Invisible to `diff-md`/`diff-gfm-md`, which round-trip through this
    /// crate's reader — a `CommonMark` info string is one word, so the
    /// multi-class list this mishandles never reaches the writer there.
    /// Each case below is `pandoc x.json -f json -t gfm` on 3.8.2.1.
    #[test]
    fn a_code_block_is_labelled_the_way_pandoc_labels_it() {
        let fenced = |classes: &[&str]| {
            let attr = ferrodoc_ast::Attr {
                identifier: String::new(),
                classes: classes.iter().map(|c| (*c).to_owned()).collect(),
                attributes: Vec::new(),
            };
            write_gfm(&Pandoc::new(vec![Block::CodeBlock(attr, "x".to_owned())]))
        };
        assert_eq!(fenced(&["sourceCode", "bash"]), "``` bash\nx\n```\n");
        assert_eq!(fenced(&["bash"]), "``` bash\nx\n```\n");
        assert_eq!(fenced(&["sourceCode"]), "```\nx\n```\n");
        assert_eq!(fenced(&["python", "numberLines"]), "``` python\nx\n```\n");
        // `numberLines` is not filtered: only `sourceCode` is.
        assert_eq!(fenced(&["numberLines", "python"]), "``` numberLines\nx\n```\n");
        assert_eq!(fenced(&["a", "b"]), "``` a\nx\n```\n");
        // The fence still outgrows the backticks inside it, which pandoc
        // does not need to do: it sizes a fence by the longest line that is
        // one unbroken run of backticks once the line's leading and trailing
        // spaces and tabs are dropped — contents `"````"`, `"   ````"`,
        // `"\t````"` and `"```` "` each get a five-backtick fence, while
        // `"```` ``"` and `"```` x"` get three, the run no longer being the
        // whole trimmed line. So a three-backtick fence closes this block and
        // pandoc reads its own output straight back, losing nothing. Counting
        // any run instead is strictly wider, never narrower — the one
        // assertion here that diverges on purpose.
        let attr = ferrodoc_ast::Attr {
            identifier: String::new(),
            classes: vec!["sourceCode".to_owned(), "bash".to_owned()],
            attributes: Vec::new(),
        };
        let long = write_gfm(&Pandoc::new(vec![Block::CodeBlock(
            attr,
            "```` x".to_owned(),
        )]));
        assert_eq!(long, "````` bash\n```` x\n`````\n");
    }

    /// Read, write, read again: the second AST must equal the first.
    fn round_trips(markdown: &str) -> bool {
        let first = read_commonmark(markdown).expect("convertible");
        let written = write_markdown(&first);
        let second = read_commonmark(&written).expect("convertible");
        if first.blocks != second.blocks {
            eprintln!("--- input:\n{markdown}\n--- written:\n{written}");
            eprintln!(
                "--- first: {:?}\n--- second: {:?}",
                first.blocks, second.blocks
            );
        }
        first.blocks == second.blocks
    }

    /// The same property for GFM: what `write_gfm` emits must re-read to
    /// the document it was given.
    fn gfm_round_trips(markdown: &str) -> bool {
        let first = crate::read_gfm(markdown).expect("convertible");
        let written = write_gfm(&first);
        let second = crate::read_gfm(&written).expect("convertible");
        if first.blocks != second.blocks {
            eprintln!("--- input:\n{markdown}\n--- written:\n{written}");
            eprintln!(
                "--- first: {:?}\n--- second: {:?}",
                first.blocks, second.blocks
            );
        }
        first.blocks == second.blocks
    }

    fn gfm_of(markdown: &str) -> String {
        write_gfm(&crate::read_gfm(markdown).expect("convertible"))
    }

    #[test]
    fn gfm_constructs_round_trip() {
        assert!(gfm_round_trips("| a | b |\n|:--|--:|\n| 1 | 2 |\n"));
        assert!(gfm_round_trips("| a | b |\n| - | - |\n"));
        assert!(gfm_round_trips("| a |\n|---|\n| `x\\|y` |\n"));
        assert!(gfm_round_trips("- [ ] a\n- [x] b\n"));
        assert!(gfm_round_trips("- [ ] a\n  - [x] deep\n- [ ] b\n"));
        assert!(gfm_round_trips("- [ ] a\n- plain\n- [x] c\n"));
        assert!(gfm_round_trips("~~gone~~ and ~~*both*~~\n"));
        assert!(gfm_round_trips("> | q | r |\n> |---|---|\n> | 1 | 2 |\n"));
    }

    #[test]
    fn text_that_looks_like_a_link_stays_text() {
        // GFM autolinks bare URLs, so a `Str` holding one has to be
        // broken or the round trip gains a `Link` the document never had.
        assert!(gfm_round_trips("http\\://example.com\n"));
        assert!(gfm_round_trips("www\\.example.com\n"));
        assert!(gfm_round_trips("user\\@example.com\n"));
        assert!(gfm_round_trips("a \\~\\~b\\~\\~ c and p\\|q\n"));
        // ...and an escape is only spent where the trigger is real.
        assert_eq!(gfm_of("ratio 3\\:1\n"), "ratio 3:1\n");
    }

    #[test]
    fn a_hard_break_on_an_empty_line_does_not_split_the_paragraph() {
        // `  \n` is only whitespace on a line of its own, so the line
        // reads as blank and one paragraph comes back as two. A backslash
        // is a hard break wherever it stands. The `SoftBreak` before it
        // has no spelling at all and is the residual loss.
        let doc = Pandoc::new(vec![Block::Para(vec![
            Inline::Str("a".to_owned()),
            Inline::SoftBreak,
            Inline::LineBreak,
            Inline::Str("b".to_owned()),
        ])]);
        for written in [write_gfm(&doc), write_markdown(&doc)] {
            assert_eq!(written, "a\n\\\nb\n");
            assert_eq!(crate::read_gfm(&written).unwrap().blocks.len(), 1);
        }
    }

    #[test]
    fn a_ballot_box_is_written_back_as_a_task_item() {
        // The round-trip gates cannot see this: `- ☐ a` re-reads to the
        // same AST as `- [ ] a`. What differs is what GitHub renders — a
        // checkbox, or a literal box character.
        assert_eq!(
            gfm_of("- [ ] a\n- [x] b\n- plain\n"),
            "- [ ] a\n- [x] b\n\n* plain\n"
        );
        // Without the extensions it stays the character the AST holds.
        assert_eq!(
            write_markdown(&crate::read_gfm("- [ ] a\n").unwrap()),
            "- \u{2610} a\n"
        );
    }

    #[test]
    fn filling_breaks_only_where_a_space_stood_in_the_tree() {
        let words = |n: usize| {
            let mut out = Vec::new();
            for i in 0..n {
                if i > 0 {
                    out.push(Inline::Space);
                }
                out.push(Inline::Str("word".to_owned()));
            }
            out
        };
        let para = |inlines: Vec<Inline>| Pandoc::new(vec![Block::Para(inlines)]);

        // Greedy fill, and no line over the width.
        let filled = write_gfm_wrapped(&para(words(6)), 20);
        assert_eq!(filled, "word word word word\nword word\n");

        // A soft break is just another break opportunity when filling...
        let soft = para(vec![
            Inline::Str("a".to_owned()),
            Inline::SoftBreak,
            Inline::Str("b".to_owned()),
        ]);
        assert_eq!(write_gfm_wrapped(&soft, 72), "a b\n");
        // ...and stays where it was when not.
        assert_eq!(write_gfm(&soft), "a\nb\n");

        // A word wider than the column goes on its own line and overruns:
        // breaking inside it would invent a break the text does not have.
        let long = para(vec![
            Inline::Str("a".to_owned()),
            Inline::Space,
            Inline::Str("supercalifragilistic".to_owned()),
        ]);
        assert_eq!(write_gfm_wrapped(&long, 5), "a\nsupercalifragilistic\n");
    }

    #[test]
    fn filling_never_breaks_what_a_break_would_change() {
        let para = |inlines: Vec<Inline>| Pandoc::new(vec![Block::Para(inlines)]);
        // A space inside a code span is the literal's, not a `Space`.
        let code = para(vec![Inline::Code(Box::default(), "a b c d e f".to_owned())]);
        assert_eq!(write_gfm_wrapped(&code, 5), "`a b c d e f`\n");

        // A link destination holds spaces the writer put there, and the
        // title is separated by one. Neither is a break opportunity.
        let link = para(vec![Inline::Link(
            Box::default(),
            vec![Inline::Str("t".to_owned())],
            Box::new(Target { url: "http://e.com/a".to_owned(), title: "a title".to_owned() }),
        )]);
        assert_eq!(write_gfm_wrapped(&link, 5), "[t](http://e.com/a \"a title\")\n");

        // A pipe table row is one line whatever the width says.
        let table = write_gfm_wrapped(
            &Pandoc::new(vec![Block::Para(vec![Inline::Str("x".to_owned())])]),
            5,
        );
        assert_eq!(table, "x\n");

        // A heading is one line: pandoc leaves a 151-column heading at 151.
        let heading = Pandoc::new(vec![Block::Header(
            1,
            Attr::default(),
            vec![
                Inline::Str("one".to_owned()),
                Inline::Space,
                Inline::Str("two".to_owned()),
                Inline::Space,
                Inline::Str("three".to_owned()),
            ],
        )]);
        assert_eq!(write_gfm_wrapped(&heading, 5), "# one two three\n");
    }

    #[test]
    fn a_prefix_is_part_of_the_width_it_fills_to() {
        // Measured against pandoc: `> word …` and `- word …` both come out
        // at 71 columns for `--columns 72`, marker included.
        let words: Vec<Inline> = (0..6)
            .flat_map(|i| {
                if i > 0 {
                    vec![Inline::Space, Inline::Str("word".to_owned())]
                } else {
                    vec![Inline::Str("word".to_owned())]
                }
            })
            .collect();
        let quoted = Pandoc::new(vec![Block::BlockQuote(vec![Block::Para(words.clone())])]);
        assert_eq!(write_gfm_wrapped(&quoted, 20), "> word word word\n> word word word\n");

        let listed = Pandoc::new(vec![Block::BulletList(vec![vec![Block::Plain(words)]])]);
        assert_eq!(write_gfm_wrapped(&listed, 20), "- word word word\n  word word word\n");
    }

    #[test]
    fn two_footnotes_keep_two_labels_and_two_bodies() {
        // A note's body is itself a nested `blocks` call. When the flush
        // lived in `blocks` behind `prefix.is_empty()`, rendering the
        // second note drained the first into its body and reset the
        // counter: both references came out `[^1]` and one body vanished
        // inside the other. Byte-compared against pandoc's gfm writer.
        let note = |text: &str| {
            Inline::Note(vec![Block::Para(vec![Inline::Str(text.to_owned())])])
        };
        let doc = Pandoc::new(vec![Block::Para(vec![
            Inline::Str("a".to_owned()),
            note("one"),
            Inline::Space,
            Inline::Str("b".to_owned()),
            note("two"),
        ])]);
        assert_eq!(write_gfm(&doc), "a[^1] b[^2]\n\n[^1]: one\n\n[^2]: two\n");
    }

    #[test]
    fn a_footnote_body_keeps_its_blocks_under_the_continuation_indent() {
        let doc = Pandoc::new(vec![Block::Para(vec![
            Inline::Str("a".to_owned()),
            Inline::Note(vec![
                Block::Para(vec![Inline::Str("one".to_owned())]),
                Block::Para(vec![Inline::Str("two".to_owned())]),
            ]),
        ])]);
        assert_eq!(write_gfm(&doc), "a[^1]\n\n[^1]: one\n\n    two\n");
    }

    #[test]
    fn math_is_written_verbatim_because_escaping_corrupts_the_tex() {
        // Escaped, `\sum_i` becomes `\\sum\_i`, and MathJax reads `\\`
        // as a line break and `\_` as a literal underscore — the equation
        // renders wrong in a notebook. Probed against pandoc's ipynb writer.
        let doc = Pandoc::new(vec![Block::Para(vec![
            Inline::Math(MathType::InlineMath, r"L = \sum_i (y_i)^2".to_owned()),
        ])]);
        assert_eq!(write_gfm(&doc), "$L = \\sum_i (y_i)^2$\n");

        let display = Pandoc::new(vec![Block::Para(vec![
            Inline::Math(MathType::DisplayMath, "E = mc^2".to_owned()),
        ])]);
        assert_eq!(write_gfm(&display), "$$E = mc^2$$\n");
    }

    #[test]
    fn an_autolink_goes_back_in_its_angle_form() {
        // `[text](url)` would lose the `uri` class on the next read, which
        // is what the ipynb writer gate scores. Probed: pandoc writes
        // `<url>` for **any** link whose text is its target — the class
        // is not part of its test, and requiring one here meant every
        // autolink a GFM document contains came back in bracket form.
        let uri = Pandoc::new(vec![Block::Para(vec![Inline::Link(
            Box::new(Attr { classes: vec!["uri".to_owned()], ..Attr::default() }),
            vec![Inline::Str("https://example.org/r".to_owned())],
            Box::new(Target { url: "https://example.org/r".to_owned(), title: String::new() }),
        )])]);
        assert_eq!(write_gfm(&uri), "<https://example.org/r>\n");

        let mail = Pandoc::new(vec![Block::Para(vec![Inline::Link(
            Box::new(Attr { classes: vec!["email".to_owned()], ..Attr::default() }),
            vec![Inline::Str("ops@example.com".to_owned())],
            Box::new(Target { url: "mailto:ops@example.com".to_owned(), title: String::new() }),
        )])]);
        assert_eq!(write_gfm(&mail), "<ops@example.com>\n");

        // An unclassed link with the same text is the same autolink.
        let plain = Pandoc::new(vec![Block::Para(vec![Inline::Link(
            Box::default(),
            vec![Inline::Str("https://example.org/r".to_owned())],
            Box::new(Target { url: "https://example.org/r".to_owned(), title: String::new() }),
        )])]);
        assert_eq!(write_gfm(&plain), "<https://example.org/r>\n");

        // A title has nowhere to go in the angle form, so it keeps the
        // brackets. Pandoc drops the title; a title that vanishes is
        // content lost for a byte.
        let titled = Pandoc::new(vec![Block::Para(vec![Inline::Link(
            Box::default(),
            vec![Inline::Str("https://example.org/r".to_owned())],
            Box::new(Target {
                url: "https://example.org/r".to_owned(),
                title: "t".to_owned(),
            }),
        )])]);
        assert_eq!(
            write_gfm(&titled),
            "[https\\://example.org/r](https://example.org/r \"t\")\n"
        );
    }

    #[test]
    fn strikeout_never_emits_a_tilde_fence() {
        // Two `~~` runs that meet make four tildes, which opens a code
        // fence and swallows the rest of the document. Neither nesting
        // nor adjacency may produce one.
        let doc = Pandoc::new(vec![
            Block::Para(vec![
                Inline::Strikeout(vec![Inline::Strikeout(vec![Inline::Str("deep".to_owned())])]),
                Inline::Space,
                Inline::Str("tail".to_owned()),
            ]),
            Block::Para(vec![
                Inline::Strikeout(vec![Inline::Str("a".to_owned())]),
                Inline::Strikeout(vec![Inline::Str("b".to_owned())]),
            ]),
        ]);
        let written = write_gfm(&doc);
        assert_eq!(written, "~~deep~~ tail\n\n~~ab~~\n");
        // Every word survives, struck, and nothing after it is swallowed.
        let back = crate::read_gfm(&written).unwrap();
        assert_eq!(back.blocks.len(), 2);
        assert!(matches!(back.blocks[1], Block::Para(ref is) if is.len() == 1));
    }

    #[test]
    fn gfm_degrades_rather_than_drops() {
        // A table with no columns has no pipe-table spelling; its content
        // must still come out.
        let empty_grid = Pandoc::new(vec![Block::Table(Box::new(Table {
            colspecs: Vec::new(),
            ..one_by_one()
        }))]);
        assert_eq!(write_gfm(&empty_grid), "a\n");
        // `~~` will not open or close against whitespace, so the spaces
        // move outside the delimiters instead of stranding them.
        let spaced = Pandoc::new(vec![Block::Para(vec![
            Inline::Strikeout(vec![Inline::Str("a".to_owned()), Inline::Space]),
            Inline::Str("b".to_owned()),
        ])]);
        assert_eq!(write_gfm(&spaced), "~~a~~ b\n");
        // Content that is only whitespace cannot carry `~~` at all, so GFM
        // drops the markup. Pandoc writes `~~~~` here, which is a tilde
        // code fence that swallows the rest of the document — the one
        // place this writer will not follow it.
        let blank = Pandoc::new(vec![Block::Para(vec![Inline::Strikeout(vec![Inline::Space])])]);
        assert_eq!(write_gfm(&blank), " \n");
        // CommonMark has no `~~`, so it takes the tag either way, and this
        // is pandoc's output byte for byte.
        assert_eq!(write_markdown(&blank), "<s> </s>\n");
    }

    #[test]
    fn a_pipe_in_a_cell_is_escaped_exactly_once() {
        // Both escapers can want the same `|`: the inline one, so a
        // paragraph of pipes cannot become a table, and the cell one, so
        // the cell does not end early. cmark-gfm's splitter is lenient
        // enough to survive a doubled escape, but `p\\|q` is not what the
        // document says.
        assert_eq!(
            gfm_of("| a |\n|---|\n| p\\|q |\n"),
            "| a |\n| ---- |\n| p\\|q |\n"
        );
    }

    #[test]
    fn a_table_in_a_list_item_gets_the_blank_line_it_needs() {
        // A pipe table cannot interrupt a paragraph, so an item holding a
        // `Plain` and a table must be written loose. Only a converted
        // document reaches this shape; GFM source cannot express it.
        let doc = Pandoc::new(vec![Block::BulletList(vec![vec![
            Block::Plain(vec![Inline::Str("item".to_owned())]),
            Block::Table(Box::new(one_by_one())),
        ]])]);
        let written = write_gfm(&doc);
        assert_eq!(written, "- item\n\n  | a |\n  | ---- |\n");
        assert!(matches!(
            crate::read_gfm(&written).unwrap().blocks[0],
            Block::BulletList(ref items) if matches!(items[0][1], Block::Table(_))
        ));
    }

    /// A table whose only content is one header cell reading `a`.
    fn one_by_one() -> Table {
        Table {
            attr: ferrodoc_ast::Attr::default(),
            caption: ferrodoc_ast::Caption::default(),
            colspecs: vec![ferrodoc_ast::ColSpec {
                alignment: Alignment::AlignDefault,
                width: ferrodoc_ast::ColWidth::ColWidthDefault,
            }],
            head: ferrodoc_ast::TableHead {
                attr: ferrodoc_ast::Attr::default(),
                rows: vec![Row {
                    attr: ferrodoc_ast::Attr::default(),
                    cells: vec![cell(1, vec![Block::Plain(vec![Inline::Str("a".to_owned())])])],
                }],
            },
            bodies: vec![ferrodoc_ast::TableBody {
                attr: ferrodoc_ast::Attr::default(),
                row_head_columns: 0,
                head: Vec::new(),
                body: Vec::new(),
            }],
            foot: ferrodoc_ast::TableFoot::default(),
        }
    }

    #[test]
    fn tables_keep_their_grid_whatever_the_cells_hold() {
        // A cell's blocks are flattened onto the row's single line.
        let table = Block::Table(Box::new(Table {
            attr: ferrodoc_ast::Attr::default(),
            caption: ferrodoc_ast::Caption {
                short: None,
                blocks: vec![Block::Para(vec![Inline::Str("Cap".to_owned())])],
            },
            colspecs: vec![
                ferrodoc_ast::ColSpec {
                    alignment: Alignment::AlignDefault,
                    width: ferrodoc_ast::ColWidth::ColWidthDefault,
                };
                3
            ],
            head: ferrodoc_ast::TableHead::default(),
            bodies: vec![ferrodoc_ast::TableBody {
                attr: ferrodoc_ast::Attr::default(),
                row_head_columns: 0,
                head: Vec::new(),
                body: vec![Row {
                    attr: ferrodoc_ast::Attr::default(),
                    cells: vec![
                        cell(2, vec![Block::Para(vec![Inline::Str("wide".to_owned())])]),
                        cell(
                            1,
                            vec![
                                Block::Para(vec![Inline::Str("one".to_owned())]),
                                Block::Para(vec![Inline::Str("two".to_owned())]),
                            ],
                        ),
                    ],
                }],
            }],
            foot: ferrodoc_ast::TableFoot::default(),
        }));
        let doc = Pandoc::new(vec![table]);
        assert_eq!(
            write_gfm(&doc),
            "|  |  |  |\n| ---- | ---- | ---- |\n| wide |  | one two |\n\nCap\n"
        );
        // CommonMark has no table syntax, so the same document degrades to
        // its cell contents there — that is the loss GFM mode exists for.
        assert!(!write_markdown(&doc).contains('|'));
    }

    fn cell(col_span: i64, blocks: Vec<Block>) -> ferrodoc_ast::Cell {
        ferrodoc_ast::Cell {
            attr: ferrodoc_ast::Attr::default(),
            alignment: Alignment::AlignDefault,
            row_span: 1,
            col_span,
            blocks,
        }
    }

    #[test]
    fn basic_shapes_round_trip() {
        assert!(round_trips("# Title\n\nA *para* with `code`.\n"));
        assert!(round_trips("- a\n- b\n"));
        assert!(round_trips("1. one\n2. two\n"));
        assert!(round_trips("> quoted\n>\n> again\n"));
        assert!(round_trips("```rust\nfn x() {}\n```\n"));
        assert!(round_trips("[link](http://e.x \"t\")\n"));
    }

    #[test]
    fn literal_markup_characters_survive() {
        assert!(round_trips("a \\* literal asterisk\n"));
        assert!(round_trips("under\\_score and \\[bracket\\]\n"));
        assert!(round_trips("a \\# hash and 5 \\< 6\n"));
    }

    #[test]
    fn adjacent_lists_do_not_merge() {
        // The bullet character alternates, so no separator block is needed.
        assert!(round_trips("- a\n\n* b\n"));
        assert!(round_trips("1. a\n\n1) b\n"));
    }

    #[test]
    fn line_structure_survives() {
        // A soft break is a line break in the source, not a space.
        assert!(round_trips("one\ntwo\n"));
        assert!(round_trips("> one\n> two\n"));
        assert!(round_trips("Foo *bar\nbaz*\n====\n"));
    }

    #[test]
    fn block_openers_in_text_are_neutralized() {
        // `1.` opens a list only where the line is nothing but the number.
        assert!(round_trips("1\\. not a list\n"));
        assert!(round_trips("version 1.2 is fine\n"));
        // Newline and tab inside a `Str` are text, not structure.
        assert!(round_trips("foo&#10;&#10;bar\n"));
        assert!(round_trips("&#9;foo\n"));
        // A trailing `#` run is otherwise a heading's closing sequence.
        assert!(round_trips("## foo #\\##\n"));
    }

    #[test]
    fn blocks_with_no_content_are_kept() {
        assert!(round_trips(">\n"));
        // `- ---` is itself a thematic break, which would eat the item.
        assert!(round_trips("- Foo\n- * * *\n"));
    }

    #[test]
    fn nested_emphasis_keeps_its_levels() {
        assert!(round_trips("*_foo_*\n"));
        assert!(round_trips("foo***bar***baz\n"));
        assert!(round_trips("foo******bar******baz\n"));
    }

    #[test]
    fn awkward_code_and_link_payloads_survive() {
        // One space is stripped from each end of a code span.
        assert!(round_trips("`  ``  `\n"));
        assert!(round_trips("` `\n"));
        // Destinations are read with backslash escapes active.
        assert!(round_trips("<https://example.com?find=\\*>\n"));
        assert!(round_trips("[a](<b)c>)\n"));
    }
}
