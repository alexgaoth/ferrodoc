//! Plain-text writer for the ferrodoc (pandoc-compatible) AST.
//!
//! [`write_text`] follows `pandoc -t plain --wrap=none`: inline markup is
//! dropped except strikeout's tildes, block structure is kept by
//! indentation, tables are column-aligned, and footnotes become numbered
//! references with their bodies at the end. Every rule below was probed
//! against pandoc 3.8.2.1 and is asserted on literal output, because no
//! differential gate reads this format — pandoc cannot parse it back.
//!
//! Math is rendered as pandoc renders it — `$x^2$` is `x²`, from the
//! superscript [`ferrodoc_ast::tex_inlines`] makes of it — and the TeX
//! is written between dollars only for the expressions pandoc writes
//! that way too. `scripts/math.sh` is the gate that says how many.

use ferrodoc_ast::{Alignment, Block, Inline, ListNumberDelim, Pandoc, Table};
use std::fmt::Write as _;

/// The column a `HorizontalRule` fills and pandoc's default `--columns`.
const COLUMNS: usize = 72;

/// Marks a place a line may be broken. Chosen because no reader here can
/// produce one inside text: `CommonMark` replaces NUL with U+FFFD by
/// specification, and XML — which DOCX, ODT and EPUB are — forbids it.
/// Every one becomes a space or a newline before the string leaves.
const BREAK: char = '\u{0}';
/// The same, for a `SoftBreak` — which `--wrap=preserve` keeps as a
/// newline where an ordinary space stays a space.
const SOFT: char = '\u{1}';

/// Render a document as plain text, every soft break joined into a space
/// and no line broken — pandoc's `--wrap=none`.
pub fn write_text(doc: &Pandoc) -> String {
    render(doc, None, false)
}

/// The same, filled to `columns` — pandoc's `--wrap=auto`.
#[must_use]
pub fn write_text_wrapped(doc: &Pandoc, columns: usize) -> String {
    render(doc, Some(columns), false)
}

/// The same, with the document's own line breaks kept — pandoc's
/// `--wrap=preserve`. A space stays a space; a soft break stays a break.
#[must_use]
pub fn write_text_preserved(doc: &Pandoc) -> String {
    render(doc, None, true)
}

fn render(doc: &Pandoc, columns: Option<usize>, preserve: bool) -> String {
    let mut writer = Writer { columns, preserve, ..Writer::default() };
    let mut paragraphs = Vec::new();
    writer.blocks(&doc.blocks, &mut paragraphs, "");
    writer.flush_notes();
    for (index, body) in writer.notes.iter().enumerate() {
        paragraphs.push(format!("[{}] {body}", index + 1));
    }
    let mut out = paragraphs.join("\n\n");
    if !out.is_empty() {
        out.push('\n');
    }
    out
}

#[derive(Default)]
// Four independent facts about where the block being written sits, none
// of which is a state the others follow from.
#[allow(clippy::struct_excessive_bools)]
struct Writer {
    /// Footnote bodies in label order, filled after the main pass.
    notes: Vec<String>,
    /// The blocks of each **top-level** note, queued while the document
    /// is written. A note met while rendering one of these queues
    /// nothing — pandoc numbers the document's own notes first and a
    /// nested one after all of them, and gives the nested one no body.
    pending: Vec<Vec<Block>>,
    /// The next label to hand out; not `notes.len()`, because a nested
    /// note takes a label and contributes no body.
    next_note: usize,
    /// Whether a note's body is being rendered right now.
    in_note: bool,
    /// The column to fill to, or `None` to leave every line as it falls.
    columns: Option<usize>,
    /// Columns already spoken for by list markers. A list item renders
    /// its content with an **empty** prefix and adds the continuation
    /// indent afterwards, so the prefix cannot say how wide the line
    /// really is; this can. Every line of the item is that much shorter.
    reserved: usize,
    /// Whether a soft break stays a line break rather than becoming a
    /// space — pandoc's `--wrap=preserve`.
    preserve: bool,
    /// Whether the block being written is inside a table cell, where a
    /// cell is one line whatever the column count — the table lays its
    /// own columns out. A footnote *referenced* from a cell has its body
    /// at the end of the document, outside the table, and is filled.
    in_cell: bool,
    /// Columns spoken for on the **first line only** — a footnote's
    /// `[N] ` label, which shifts the body's first line and nothing
    /// after it. The first line laid out consumes it.
    hanging: usize,
    /// Whether the block being written is the **first** one inside a
    /// container that indents — a quote, a list item, a definition body.
    /// A list item renders its content with an **empty** prefix and adds
    /// the continuation indent afterwards, so the prefix cannot say it,
    /// and a code block four spaces in is markup at the top level and
    /// four stray spaces inside an item.
    ///
    /// It has to be a property of *that one block* rather than a depth:
    /// as a counter it stayed set for a quote's later blocks and for
    /// everything inside a `Div`, so `> <div>a\n\ncode` lost the code
    /// block's four spaces that pandoc writes.
    opening: bool,
}

impl Writer {
    /// Render every queued note body, once the main pass is over.
    ///
    /// A note met **here** takes the next label — by then the counter is
    /// past every top-level note — and queues nothing, which is what
    /// leaves a note inside a note with a reference and no body, exactly
    /// as pandoc writes it.
    fn flush_notes(&mut self) {
        let mut index = 0;
        while index < self.pending.len() {
            let blocks = std::mem::take(&mut self.pending[index]);
            let mut inner = Vec::new();
            // The body is written after `[N] `, and those columns are
            // gone before the first word: the label is not a hanging
            // indent — the second line starts at column zero — but the
            // first line is that much shorter.
            let hanging = std::mem::replace(&mut self.hanging, format!("[{}] ", index + 1).chars().count());
            let in_cell = std::mem::take(&mut self.in_cell);
            let outer = std::mem::replace(&mut self.in_note, true);
            self.blocks(&blocks, &mut inner, "");
            self.in_note = outer;
            self.in_cell = in_cell;
            self.hanging = hanging;
            // The body's blocks stay blocks: a footnote of two paragraphs
            // and a list is three paragraphs at the end of the document,
            // not one run-on line.
            self.notes.push(inner.join("\n\n"));
            index += 1;
        }
    }

    /// A superscript or a subscript: the Unicode characters for it where
    /// every one of them has one, and `^(…)` where any does not.
    fn script(&mut self, out: &mut String, inner: &[Inline], mark: char, map: fn(char) -> Option<char>) {
        let mut text = String::new();
        self.collect(&mut text, inner);
        if let Some(raised) = text.chars().map(map).collect::<Option<String>>() {
            out.push_str(&raised);
            return;
        }
        out.push(mark);
        out.push('(');
        out.push_str(&text);
        out.push(')');
    }

    /// Turn the break marks into spaces, or into a fill at the width the
    /// prefix leaves. The prefix is part of the line pandoc counts, so a
    /// quote or a list item fills to less than the full width.
    fn lay_out(&mut self, text: &str, indent: usize) -> String {
        let Some(columns) = self.columns.filter(|_| !self.in_cell) else {
            let soft = if self.preserve && !self.in_cell { "\n" } else { " " };
            return text.replace(BREAK, " ").replace(SOFT, soft);
        };
        let width = columns.saturating_sub(indent + self.reserved).max(1);
        let first = width.saturating_sub(std::mem::take(&mut self.hanging)).max(1);
        let mut out = String::with_capacity(text.len());
        for (index, paragraph) in text.split('\n').enumerate() {
            if index > 0 {
                out.push('\n');
            }
            fill(paragraph, if index == 0 { first } else { width }, width, &mut out);
        }
        out
    }

    /// Only the first block of a sequence can be the one that opens a
    /// container, so the flag is spent here — which is what carries it
    /// through a `Div`, whose blocks are the container's own.
    fn blocks(&mut self, blocks: &[Block], out: &mut Vec<String>, prefix: &str) {
        for (index, block) in blocks.iter().enumerate() {
            if index > 0 {
                self.opening = false;
            }
            self.block(block, out, prefix);
        }
    }

    fn block(&mut self, block: &Block, out: &mut Vec<String>, prefix: &str) {
        match block {
            Block::Plain(inlines) | Block::Para(inlines) => {
                let text = self.inlines(inlines);
                out.push(indent(&self.lay_out(&text, prefix.chars().count()), prefix));
            }
            // **A heading is never filled.** Pandoc keeps one on a single
            // line however long it is and however narrow the column; a
            // heading broken in two reads as two headings — and a *hard*
            // break in one is a space for the same reason, which the two
            // soft marks alone did not cover: `LineBreak` is written as a
            // real newline here and survived the replacement.
            Block::Header(_, _, inlines) => {
                let text = self.inlines(inlines).replace(['\n', BREAK, SOFT], " ");
                out.push(indent(&text, prefix));
            }
            // Four spaces, which is what makes it read as code at all,
            // **on top of whatever the container already indents by** — a
            // quote's own two columns do not stand in for them, and
            // pandoc writes 6 inside one quote and 8 inside two. Requiring
            // an empty prefix suppressed them entirely there: `README.md`
            // has a `sh` block inside a blockquote, and it came out level
            // with the prose around it.
            //
            // The **first line of a container's first block** is the
            // exception, and `opening` is what says so: it is written on
            // a line the container has already begun, so the four spaces
            // are missing *there* and present on every line below —
            // `> ```{.sh}` is 2 and the code under it is 6. A list item
            // renders that first block with an empty prefix and adds its
            // continuation afterwards, so the prefix cannot be read for
            // it either.
            Block::CodeBlock(_, text) => {
                let body = indent(text.trim_end_matches('\n'), &format!("{prefix}    "));
                out.push(if self.opening { open_line(&body, prefix, 4) } else { body });
            }
            // Two more spaces per level, so nesting is visible.
            Block::BlockQuote(inner) => {
                let inner_prefix = format!("{prefix}  ");
                let before = out.len();
                let opening = std::mem::replace(&mut self.opening, true);
                self.blocks(inner, out, &inner_prefix);
                self.opening = opening;
                // A quote whose content renders to nothing — a raw block
                // in another format is one — is still a quote, and pandoc
                // writes its indentation on a line of its own.
                if out.len() == before {
                    out.push(inner_prefix);
                }
            }
            Block::Div(_, inner) => self.blocks(inner, out, prefix),
            // **A figure is its caption in brackets**, the same shape an
            // image takes here — the picture is not there to be shown, so
            // the caption is the only thing that carries. Written as
            // content then caption it produced the alt text *and* the
            // caption, as two paragraphs.
            Block::Figure(_, caption, inner) => {
                if caption.blocks.is_empty() {
                    self.blocks(inner, out, prefix);
                } else {
                    let mut text = Vec::new();
                    self.blocks(&caption.blocks, &mut text, "");
                    out.push(indent(&format!("[{}]", text.join("\n").trim()), prefix));
                }
            }
            Block::BulletList(items) => self.list(items, out, prefix, |_| "- ".to_owned()),
            Block::OrderedList(attrs, items) => {
                // **The style and the delimiter the list was written
                // with.** A list that said `3)` came out saying `3.`, and
                // every roman or alphabetic list came out in digits —
                // which a plain-text rendering can still get wrong, and
                // did for 19 of the 137 constructs the AST sweep asks
                // about. `(1)` is a marker in its own right, not `1)`.
                let (start, style, delim) = (attrs.start, attrs.style, attrs.delim);
                self.list(items, out, prefix, move |i| {
                    let label = style.label(start + i64::try_from(i).unwrap_or(0));
                    match delim {
                        ListNumberDelim::TwoParens => format!("({label})"),
                        ListNumberDelim::OneParen => format!("{label})"),
                        _ => format!("{label}."),
                    }
                });
            }
            Block::DefinitionList(entries) => {
                for (term, definitions) in entries {
                    let text = self.inlines(term);
                    let term = indent(&self.lay_out(&text, prefix.chars().count()), prefix);
                    // **A tight definition follows its term directly.**
                    // Paragraphs here are joined by a blank line, so
                    // pushing the two separately always separated them —
                    // right for a `Para` definition, and a blank line
                    // pandoc does not write for a `Plain` one.
                    let tight = definitions
                        .iter()
                        .all(|d| matches!(d.first(), Some(Block::Plain(_))));
                    let mut bodies = Vec::new();
                    for definition in definitions {
                        let opening = std::mem::replace(&mut self.opening, true);
                        self.blocks(definition, &mut bodies, &format!("{prefix}    "));
                        self.opening = opening;
                    }
                    if tight {
                        out.push(std::iter::once(term).chain(bodies).collect::<Vec<_>>().join("\n"));
                    } else {
                        out.push(term);
                        out.extend(bodies);
                    }
                }
            }
            Block::LineBlock(lines) => {
                // A line block's lines are the document's own and are
                // never re-filled; only the marks come out.
                let text: Vec<String> = lines
                    .iter()
                    .map(|l| self.inlines(l).replace([BREAK, SOFT], " "))
                    .collect();
                out.push(indent(&text.join("\n"), prefix));
            }
            Block::Table(table) => self.table_block(table, out, prefix),
            // The rule fills the column count asked for, not a fixed 72.
            //
            // Opening a container it takes a line of the container's own
            // indentation above it, then a blank one: a quote holding
            // nothing but a rule is `'  \n\n  ---'`. A *table* there
            // takes the same line without the blank (below), and a rule
            // one paragraph further down takes neither — both probed.
            Block::HorizontalRule => {
                let mut text = indent(&"-".repeat(self.columns.unwrap_or(COLUMNS)), prefix);
                if self.opening && !prefix.is_empty() {
                    text.insert_str(0, &format!("{prefix}\n\n"));
                }
                out.push(text);
            }
            Block::RawBlock(..) => {}
        }
    }

    /// A table and its caption, which are **one block**: pushed
    /// separately, the caption followed the table directly inside a
    /// *tight* list item, whose blocks are joined by one newline.
    fn table_block(&mut self, table: &Table, out: &mut Vec<String>, prefix: &str) {
        let rendered = self.table(table);
        let mut text = String::new();
        if !rendered.is_empty() {
            // A table opens with a blank line of its own, which is invisible
            // at the top level and is the container's indentation inside one.
            // Probed: a quote holding a table starts with a line of just the
            // quote's two spaces.
            //
            // **Only a table that opens with a header does.** One that opens
            // with a border — a multiline or grid table, and a simple table
            // with no header row — carries no blank line, and that border is
            // what sits beside a list marker: `- +----+`, not `- ` and the
            // border below it.
            text = indent(&rendered, prefix);
            if opens_with_border(&rendered) {
                // A multiline table is written two columns in, and those two
                // are the block's own margin: gone from a line its container
                // has already begun.
                if self.opening {
                    let margin = rendered.chars().take_while(|c| *c == ' ').count();
                    text = open_line(&text, prefix, margin);
                }
            } else if !prefix.is_empty() {
                text.insert_str(0, &format!("{prefix}\n"));
            }
        }
        // **A caption is `: text`**, indented with the table — written as an
        // ordinary paragraph it read as prose that had nothing to do with the
        // table above it.
        if !table.caption.blocks.is_empty() {
            let mut caption = Vec::new();
            self.blocks(&table.caption.blocks, &mut caption, "");
            let joined = caption.join("\n");
            if !text.is_empty() {
                text.push_str("\n\n");
            }
            text.push_str(&indent(&format!("  : {}", joined.trim()), prefix));
        }
        if !text.is_empty() {
            out.push(text);
        }
    }

    /// One paragraph per item when the list is loose, one for the whole
    /// list when it is tight — which is how pandoc keeps a tight list tight.
    fn list(
        &mut self,
        items: &[Vec<Block>],
        out: &mut Vec<String>,
        prefix: &str,
        marker: impl Fn(usize) -> String,
    ) {
        // An ordered list's marker column is as wide as its widest marker
        // plus a space, and never narrower than four: pandoc writes `1.  `
        // and `10. ` at the same width.
        let width = (0..items.len())
            .map(|i| marker(i).chars().count() + 1)
            .max()
            .unwrap_or(0)
            .max(4);
        let loose = items
            .iter()
            .any(|item| item.iter().any(|b| matches!(b, Block::Para(_))));
        let mut rendered = Vec::new();
        for (index, item) in items.iter().enumerate() {
            let mark = marker(index);
            // Bullets take exactly `- `; only ordered markers are padded.
            let (head, continuation) = if mark.ends_with(' ') {
                (mark.clone(), " ".repeat(mark.chars().count()))
            } else {
                (format!("{mark:<width$}"), " ".repeat(width))
            };
            let mut inner = Vec::new();
            // **Only the first block sits at the marker.** A code block
            // there takes no indentation of its own — the marker column
            // already sets it apart — and one further down the item takes
            // the usual four. Measured: `3.  fence` for the first and
            // `      indented code` for a later one under a `- ` marker.
            let reserved = self.reserved;
            // The container's own prefix counts too: the item's blocks
            // are rendered with an empty one and it is added afterwards,
            // so a list inside a quote fills to less than a list at the
            // top level by exactly the quote's two columns.
            self.reserved += continuation.chars().count() + prefix.chars().count();
            // **A blank line after a container, even in a tight item.**
            // A tight item's blocks are otherwise written one line after
            // another, but pandoc separates the block that follows a
            // nested list, a quote, a table or a code block — measured a
            // block kind at a time, and a paragraph after a paragraph
            // takes none.
            //
            // Each block is rendered on its own so that the separator
            // can be asked about the block *before* it: a **loose**
            // nested list writes one entry per item, so counting entries
            // against the item's blocks paired the wrong two.
            let opening = std::mem::replace(&mut self.opening, true);
            let mut body = String::new();
            let mut previous: Option<&Block> = None;
            for block in item {
                self.block(block, &mut inner, "");
                self.opening = false;
                // A raw block in another format renders to nothing, and
                // takes its separator with it.
                if inner.is_empty() {
                    continue;
                }
                if !body.is_empty() {
                    let after = previous.is_some_and(|before| {
                        matches!(
                            before,
                            Block::BulletList(_)
                                | Block::OrderedList(..)
                                | Block::BlockQuote(_)
                                | Block::CodeBlock(..)
                                | Block::Table(_)
                        )
                    });
                    body.push_str(if loose || after { "\n\n" } else { "\n" });
                }
                // Whatever one block wrote as several is several blocks,
                // and those take the document's own blank line.
                body.push_str(&inner.join("\n\n"));
                inner.clear();
                previous = Some(block);
            }
            self.opening = opening;
            self.reserved = reserved;
            // **A table and a rule take the marker's line to themselves**,
            // the same rule the RST writer follows and for the same
            // reason: both are laid out in columns of their own, and
            // starting one beside `- ` shifts every line of it. A
            // paragraph, a code block and a nested list all sit at the
            // marker as before.
            // A table that opens with a border does not: that border is
            // the line the marker keeps, exactly as a paragraph's first
            // line would be.
            let alone = match item.first() {
                Some(Block::Table(_)) => !opens_with_border(&body),
                Some(Block::HorizontalRule) => true,
                _ => false,
            };
            let mut lines = body.split('\n');
            let mut text = if alone {
                // The marker keeps its trailing space — `- `, not `-` —
                // and every line of the block takes the continuation. A
                // **rule** additionally takes a blank line above it,
                // which a table does not: `- ` then nothing then the
                // dashes. The same is true of one opening a quote.
                let gap = matches!(item.first(), Some(Block::HorizontalRule));
                if gap { format!("{head}\n") } else { head.clone() }
            } else {
                format!("{}{}", head, lines.next().unwrap_or_default())
            };
            for line in lines {
                text.push('\n');
                if !line.is_empty() {
                    text.push_str(&continuation);
                    text.push_str(line);
                }
            }
            rendered.push(indent(&text, prefix));
        }
        if loose {
            out.extend(rendered);
        } else if !rendered.is_empty() {
            out.push(rendered.join("\n"));
        }
    }

    /// A simple table: two-space margin, each column as wide as its widest
    /// cell plus two, a dashed rule under the head, and `AlignRight`
    /// columns padded on the left. Measured against pandoc cell by cell.
    fn table(&mut self, table: &ferrodoc_ast::Table) -> String {
        // **A header row of nothing but empty cells is not a header** —
        // the same rule the HTML, RST, AsciiDoc and markdown writers
        // follow, and the fifth writer to have had it wrong. Kept, it
        // wrote a row of spaces above the rule and widened nothing.
        let header_rows: Vec<&ferrodoc_ast::Row> = table
            .head
            .rows
            .iter()
            .filter(|row| row.cells.iter().any(|cell| !cell.blocks.is_empty()))
            .collect();
        let head: Vec<Vec<String>> =
            header_rows.iter().flat_map(|row| self.rows(std::slice::from_ref(*row))).collect();
        let body: Vec<Vec<String>> = table
            .bodies
            .iter()
            .flat_map(|b| [&b.head, &b.body])
            .chain(std::iter::once(&table.foot.rows))
            .flat_map(|rows| self.rows(rows))
            .collect();
        let columns = head.iter().chain(&body).map(Vec::len).max().unwrap_or(0);
        if columns == 0 {
            return String::new();
        }
        // **A cell holding more than a paragraph gets a grid table**, the
        // same shape the markdown writer emits. Flattened into a simple
        // table instead, a code block in a cell came out as one run-on
        // line and its block structure was gone.
        if !table.colspecs.is_empty() && !Self::cells_are_simple(table) {
            return self.grid(table);
        }
        // A stated width is one reason for the multiline form; **a hard
        // break in a cell is the other**, and for the same reason: the
        // cell is two lines and the simple form has room for one. Pandoc
        // writes the borders for either.
        let sized = table
            .colspecs
            .iter()
            .any(|spec| spec.width != ferrodoc_ast::ColWidth::ColWidthDefault)
            || Self::cells_break(table);
        let widths = self.column_widths(table, &head, &body, columns);
        let align = |c: usize| {
            table.colspecs.get(c).map_or(Alignment::AlignDefault, |s| s.alignment)
        };
        // Each cell is padded to its column and the columns are joined by
        // one space — **except the last, which takes only the padding in
        // front of it**. That is why a row ending in an empty cell keeps
        // its trailing spaces and a header ending in its column's widest
        // word does not.
        // **The two-space indent is not written on an empty row.** It is
        // the table's own indentation, and pandoc indents no line it
        // does not write: a one-column row whose cell is empty is
        // nothing at all, where a two-column one still carries the
        // padding between the two.
        // **A row is as tall as its tallest cell.** A hard break makes a
        // cell two lines, and the multiline table is the shape that has
        // room for them: each line of the row takes that line of every
        // cell, padded to its column as the single-line row is.
        let line = |row: &Vec<String>| {
            let height = row.iter().map(|cell| cell.lines().count().max(1)).max().unwrap_or(1);
            let mut lines: Vec<String> = Vec::new();
            for index in 0..height {
                let mut out = String::new();
                for (c, width) in widths.iter().enumerate() {
                    let cell = row.get(c).map_or("", String::as_str);
                    let piece = cell.lines().nth(index).unwrap_or("");
                    let slack = width.saturating_sub(piece.chars().count());
                    let (before, after) = match align(c) {
                        Alignment::AlignRight => (slack, 0),
                        Alignment::AlignCenter => (slack / 2, slack - slack / 2),
                        _ => (0, slack),
                    };
                    out.push_str(&" ".repeat(before));
                    out.push_str(piece);
                    if c + 1 < widths.len() {
                        out.push_str(&" ".repeat(after));
                        out.push(' ');
                    }
                }
                lines.push(if out.is_empty() { out } else { format!("  {out}") });
            }
            lines.join("\n")
        };
        let rule: Vec<String> = widths.iter().map(|w| "-".repeat(*w)).collect();
        let rule = format!("  {}", rule.join(" "));
        if sized {
            // A multiline table separates its rows with a blank line and
            // closes on a full-width rule, a body of exactly one row
            // being followed by a blank as well.
            let full = format!(
                "  {}",
                "-".repeat(widths.iter().sum::<usize>() + columns.saturating_sub(1))
            );
            let mut out: Vec<String> = Vec::new();
            if head.is_empty() {
                out.push(rule.clone());
            } else {
                out.push(full.clone());
                out.extend(head.iter().map(line));
                out.push(rule.clone());
            }
            for (index, cells) in body.iter().enumerate() {
                if index > 0 {
                    out.push(String::new());
                }
                let text = line(cells);
                if !text.is_empty() {
                    out.push(text);
                }
            }
            if body.len() == 1 {
                out.push(String::new());
            }
            out.push(if head.is_empty() { rule } else { full });
            return out.join("\n");
        }
        let mut lines: Vec<String> = head.iter().map(line).collect();
        lines.push(rule.clone());
        // A row that renders to nothing is no line: the two-space indent
        // belongs to a line the table writes, and pandoc writes none.
        lines.extend(body.iter().map(line).filter(|text| !text.is_empty()));
        // Without a header the rule goes above *and* below, which is what
        // closes a headerless simple table.
        if head.is_empty() {
            lines.push(rule);
        }
        lines.join("\n")
    }

    /// How wide each column is.
    ///
    /// **A column stating its own width gets pandoc's multiline table**,
    /// which is what a table converted from DOCX, ODT or HTML asks for;
    /// the widest-cell rule is for the simple one. The arithmetic is the
    /// markdown writer's, measured the same way: `floor(fraction x
    /// available)` where available is `--columns` less the space between
    /// each pair — and, where nothing may be re-flowed, at least the
    /// widest cell plus two, because the column has to hold it.
    fn column_widths(
        &self,
        table: &ferrodoc_ast::Table,
        head: &[Vec<String>],
        body: &[Vec<String>],
        columns: usize,
    ) -> Vec<usize> {
        // The widest **line** of a cell, not the length of the whole
        // text: a cell holding a hard break is two lines, and counting
        // both together sized the column to their sum.
        let widest = |c: usize| {
            head.iter()
                .chain(body)
                .filter_map(|row| row.get(c))
                .map(|cell| cell.lines().map(|l| l.chars().count()).max().unwrap_or(0))
                .max()
                .unwrap_or(0)
        };
        let available =
            self.columns.unwrap_or(COLUMNS).saturating_sub(columns.saturating_sub(1));
        (0..columns)
            .map(|c| {
                // **Asked of the column, not of the table.** A table is
                // written in the multiline shape for a hard break in a
                // cell as well as for a stated width, and a column that
                // states none is sized from its content either way —
                // read from the table it came out zero wide, and every
                // rule was empty.
                let Some(ferrodoc_ast::ColWidth::ColWidth(_)) =
                    table.colspecs.get(c).map(|spec| spec.width)
                else {
                    return widest(c) + 2;
                };
                #[expect(
                    clippy::cast_possible_truncation,
                    clippy::cast_sign_loss,
                    clippy::cast_precision_loss,
                    reason = "a column width is small, never negative, and \
                              well inside f64's mantissa"
                )]
                let base = match table.colspecs.get(c).map(|spec| spec.width) {
                    Some(ferrodoc_ast::ColWidth::ColWidth(fraction)) => {
                        (fraction * available as f64).floor() as usize
                    }
                    _ => 0,
                };
                if self.columns.is_some() { base } else { base.max(widest(c) + 2) }
            })
            .collect()
    }

    /// Whether any cell holds a hard break, which is what makes it two
    /// lines.
    fn cells_break(table: &ferrodoc_ast::Table) -> bool {
        table
            .head
            .rows
            .iter()
            .chain(table.bodies.iter().flat_map(|b| b.head.iter().chain(&b.body)))
            .chain(&table.foot.rows)
            .flat_map(|row| &row.cells)
            .flat_map(|cell| &cell.blocks)
            .any(|block| match block {
                Block::Plain(list) | Block::Para(list) => {
                    list.iter().any(|inline| matches!(inline, Inline::LineBreak))
                }
                _ => false,
            })
    }

    /// Whether every cell holds at most one `Plain` or `Para` and spans
    /// one row and one column — what the simple and multiline shapes
    /// need, and what sends anything else to a grid.
    fn cells_are_simple(table: &ferrodoc_ast::Table) -> bool {
        table
            .head
            .rows
            .iter()
            .chain(table.bodies.iter().flat_map(|b| b.head.iter().chain(&b.body)))
            .chain(&table.foot.rows)
            .flat_map(|row| &row.cells)
            .all(|cell| {
                cell.col_span.max(1) == 1
                    && cell.row_span.max(1) == 1
                    && cell.blocks.len() <= 1
                    && cell.blocks.iter().all(|b| matches!(b, Block::Plain(_) | Block::Para(_)))
            })
    }

    /// One cell's blocks as lines, filled to `width` where there is one.
    ///
    /// The writer's own state is put back afterwards — a footnote in a
    /// cell would otherwise be queued by the measuring pass and again by
    /// the laying-out one, and every later note would be numbered high.
    fn grid_cell(&mut self, blocks: &[Block], width: Option<usize>) -> Vec<String> {
        let columns = std::mem::replace(&mut self.columns, width);
        let in_cell = std::mem::replace(&mut self.in_cell, true);
        let hanging = std::mem::take(&mut self.hanging);
        let queued = self.pending.len();
        let numbered = self.next_note;
        let mut out = Vec::new();
        self.blocks(blocks, &mut out, "");
        self.pending.truncate(queued);
        self.next_note = numbered;
        self.hanging = hanging;
        self.in_cell = in_cell;
        self.columns = columns;
        out.join("\n\n").split('\n').map(str::to_owned).collect()
    }

    /// Pandoc's **grid table**, for a cell holding anything a simple or
    /// multiline table cannot: a code block, two paragraphs, a list.
    ///
    /// The same shape and the same arithmetic as the markdown writer's,
    /// measured against `pandoc -t plain` the same way: a stated width
    /// takes `floor(fraction × (columns − count))` with the **last**
    /// column taking the remainder; without one a column is as wide as
    /// its content plus two, and where they do not all fit the ones that
    /// do keep their width while the rest divide what is left.
    fn grid(&mut self, table: &ferrodoc_ast::Table) -> String {
        let count = table.colspecs.len();
        let header: Vec<&ferrodoc_ast::Row> = table
            .head
            .rows
            .iter()
            .filter(|row| row.cells.iter().any(|cell| !cell.blocks.is_empty()))
            .collect();
        let body: Vec<&ferrodoc_ast::Row> = table
            .head
            .rows
            .iter()
            .skip(header.len())
            .chain(table.bodies.iter().flat_map(|b| b.head.iter().chain(&b.body)))
            .chain(&table.foot.rows)
            .collect();
        let measure = |writer: &mut Self, row: &ferrodoc_ast::Row| -> Vec<Vec<String>> {
            row.cells.iter().map(|c| writer.grid_cell(&c.blocks, None)).collect()
        };
        let measured: Vec<Vec<Vec<String>>> =
            header.iter().chain(&body).map(|row| measure(self, row)).collect();
        let wanted: Vec<usize> = (0..count)
            .map(|index| {
                measured
                    .iter()
                    .filter_map(|row| row.get(index))
                    .flat_map(|lines| lines.iter())
                    .map(|line| line.chars().count())
                    .max()
                    .unwrap_or(0)
                    + 2
            })
            .collect();
        let widths = grid_widths(table, &wanted, self.columns.unwrap_or(COLUMNS), self.columns.is_some());
        let aligns: Vec<Alignment> = table.colspecs.iter().map(|s| s.alignment).collect();
        let mut out = Vec::new();
        if header.is_empty() {
            out.push(grid_rule(&widths, '-', Some(&aligns)));
        } else {
            out.push(grid_rule(&widths, '-', None));
            for row in &header {
                let cells: Vec<Vec<String>> =
                    row.cells.iter().enumerate()
                        .map(|(i, c)| self.grid_cell(&c.blocks, widths.get(i).map(|w| w.saturating_sub(2).max(1))))
                        .collect();
                out.extend(grid_row(&cells, &widths));
            }
            out.push(grid_rule(&widths, '=', Some(&aligns)));
        }
        for row in &body {
            let cells: Vec<Vec<String>> =
                row.cells.iter().enumerate()
                    .map(|(i, c)| self.grid_cell(&c.blocks, widths.get(i).map(|w| w.saturating_sub(2).max(1))))
                    .collect();
            out.extend(grid_row(&cells, &widths));
            out.push(grid_rule(&widths, '-', None));
        }
        out.join("\n")
    }

    fn rows(&mut self, rows: &[ferrodoc_ast::Row]) -> Vec<Vec<String>> {
        rows.iter()
            .map(|row| {
                row.cells
                    .iter()
                    .map(|cell| {
                        let in_cell = std::mem::replace(&mut self.in_cell, true);
                        let mut inner = Vec::new();
                        self.blocks(&cell.blocks, &mut inner, "");
                        self.in_cell = in_cell;
                        inner.join(" ")
                    })
                    .collect()
            })
            .collect()
    }

    fn inlines(&mut self, inlines: &[Inline]) -> String {
        let mut out = String::new();
        self.collect(&mut out, inlines);
        out
    }

    /// Two breaking spaces with nothing between them are one space, the
    /// way pandoc's layout has it: a raw inline in another format renders
    /// to nothing, so `plus <br/> and` is `plus and` there and was
    /// `plus  and` here.
    fn collect(&mut self, out: &mut String, inlines: &[Inline]) {
        let mut after_break = false;
        for inline in inlines {
            let breaking = matches!(inline, Inline::Space | Inline::SoftBreak);
            if breaking && after_break {
                continue;
            }
            let before = out.len();
            self.one(out, inline);
            if out.len() == before {
                continue;
            }
            after_break = breaking;
        }
    }

    fn one(&mut self, out: &mut String, inline: &Inline) {
        {
            match inline {
                Inline::Str(s) | Inline::Code(_, s) => out.push_str(s),
                // **A simple expression is rendered to Unicode** — `$x^2$`
                // is `x²`, from the superscript `tex_inlines` makes of it
                // and the rule below that writes one.
                //
                // **What it will not render keeps its dollars.** Stripping
                // them left `\frac{a}{b}` reading as prose, with nothing
                // to say it was ever an expression; pandoc keeps the
                // delimiters for the same expressions.
                Inline::Math(kind, s) => {
                    if let Some(rendered) = ferrodoc_ast::tex_inlines(s) {
                        self.collect(out, &rendered);
                        return;
                    }
                    let fence = match kind {
                        ferrodoc_ast::MathType::InlineMath => "$",
                        ferrodoc_ast::MathType::DisplayMath => "$$",
                    };
                    out.push_str(fence);
                    out.push_str(s);
                    out.push_str(fence);
                }
                Inline::Space => out.push(BREAK),
                Inline::SoftBreak => out.push(SOFT),
                Inline::LineBreak => out.push('\n'),
                // The one markup pandoc keeps: without the tildes the text
                // says the opposite of what it means.
                Inline::Strikeout(inner) => {
                    out.push_str("~~");
                    self.collect(out, inner);
                    out.push_str("~~");
                }
                // A picture has no text, so its alt stands in brackets —
                // otherwise it reads as a sentence the document never had.
                Inline::Image(_, alt, _) => {
                    out.push('[');
                    self.collect(out, alt);
                    out.push(']');
                }
                // **Four inlines that survive into plain text.** Pandoc
                // does not drop these to their content the way it drops
                // emphasis: a superscript keeps a marker a reader can
                // see, small caps are *rendered* by upper-casing, and a
                // quote keeps the curly characters it stands for. Each
                // probed against `pandoc -f json -t plain`.
                // **Unicode where every character has a form for it**,
                // and `^(…)` where one does not: pandoc writes `x²` and
                // `x^(2n)`, and the set it can spell is the digits, the
                // three signs and the two brackets — no letters at all,
                // though Unicode has some. Measured character by
                // character against its `plain` writer.
                Inline::Superscript(i) => self.script(out, i, '^', superscript),
                Inline::Subscript(i) => self.script(out, i, '_', subscript),
                Inline::SmallCaps(i) => {
                    let mut inner = String::new();
                    self.collect(&mut inner, i);
                    out.push_str(&inner.to_uppercase());
                }
                Inline::Quoted(quote, i) => {
                    let (open, close) = match quote {
                        ferrodoc_ast::QuoteType::SingleQuote => ('\u{2018}', '\u{2019}'),
                        ferrodoc_ast::QuoteType::DoubleQuote => ('\u{201C}', '\u{201D}'),
                    };
                    out.push(open);
                    self.collect(out, i);
                    out.push(close);
                }
                Inline::Emph(i)
                | Inline::Strong(i)
                | Inline::Underline(i)
                | Inline::Cite(_, i)
                | Inline::Span(_, i)
                | Inline::Link(_, i, _) => self.collect(out, i),
                Inline::Note(blocks) => {
                    // **Queued, not rendered here.** Pandoc numbers the
                    // document's own notes first and a note nested inside
                    // one of them after all of them — so writing the body
                    // depth-first gave the inner note the next label and
                    // pushed every later note up by one, which is a
                    // difference that runs to the end of the document.
                    self.next_note += 1;
                    let _ = write!(out, "[{}]", self.next_note);
                    if !self.in_note {
                        self.pending.push(blocks.clone());
                    }
                }
                Inline::RawInline(..) => {}
            }
        }
    }
}

/// **A word that would re-open a block if it began a line.**
///
/// Pandoc will not end a filled line in a way that leaves one of these
/// standing at the start of the next, because the text it wrote would
/// then read back as a bullet, a blockquote or a list item rather than
/// as the paragraph it is.
///
/// The set is exactly `CommonMark`'s *paragraph interrupters*, and it was
/// measured character by character rather than reasoned about: `+`, `-`,
/// `*` and `>` are avoided; `1.` and `1)` are avoided and **`2.`, `12.`
/// and `2)` are not**, because an ordered list may only interrupt a
/// paragraph when it starts at one. `#`, `=`, `~` and `%` are all
/// allowed, which is not what a reading of the spec would predict.
fn reopens_a_block(word: &str) -> bool {
    matches!(word, "+" | "-" | "*" | ">" | "1." | "1)")
}

/// Put `prefix` on every non-empty line of `text`.
/// Greedy fill: take words while they fit, break at the last mark that
/// did. A word longer than the width goes on its own line and overruns —
/// breaking inside it would invent a break the text does not have.
fn fill(line: &str, first: usize, rest: usize, out: &mut String) {
    let mut width = 0;
    let mut limit = first;
    let words: Vec<&str> = line.split([BREAK, SOFT]).collect();
    for (index, word) in words.iter().enumerate() {
        let word_width = word.chars().count();
        // **One word of lookahead.** Taking this word is refused when it
        // would push a block-reopening word onto the start of the next
        // line: break here instead, so that word lands mid-line.
        let after = width + 1 + word_width;
        let strands_the_next = words.get(index + 1).is_some_and(|next| {
            reopens_a_block(next) && after + 1 + next.chars().count() > limit
        });
        if index == 0 {
            width = word_width;
        } else if width + 1 + word_width <= limit && !strands_the_next {
            out.push(' ');
            width += 1 + word_width;
        } else {
            out.push('\n');
            width = word_width;
            limit = rest;
        }
        out.push_str(word);
    }
}

/// Drop `margin` columns from the **first line only**, leaving the rest of
/// the block indented as it was: the block's own indentation is never
/// written on a line its container has already begun.
fn open_line(text: &str, prefix: &str, margin: usize) -> String {
    let cut = prefix.len();
    if text[cut..].chars().take(margin).any(|c| c != ' ') {
        return text.to_owned();
    }
    format!("{}{}", &text[..cut], &text[cut + margin..])
}

/// Whether a rendered table starts with a rule rather than a header row —
/// `-----` for a multiline one, `+----+` for a grid.
///
/// Read from the line and not from the table: a multiline table is written
/// two columns in, so testing the first *character* for a dash said no to
/// every one of them.
fn opens_with_border(rendered: &str) -> bool {
    let first = rendered.lines().next().unwrap_or_default();
    !first.trim().is_empty() && first.chars().all(|c| matches!(c, ' ' | '-' | '+'))
}

/// The superscript form of a character, where pandoc has one: the
/// digits, `+`, `-` (either dash), `=` and the two brackets.
fn superscript(ch: char) -> Option<char> {
    Some(match ch {
        '0' => '\u{2070}', '1' => '\u{00B9}', '2' => '\u{00B2}', '3' => '\u{00B3}',
        '4' => '\u{2074}', '5' => '\u{2075}', '6' => '\u{2076}', '7' => '\u{2077}',
        '8' => '\u{2078}', '9' => '\u{2079}',
        '+' => '\u{207A}', '-' | '\u{2212}' => '\u{207B}', '=' => '\u{207C}',
        '(' => '\u{207D}', ')' => '\u{207E}',
        _ => return None,
    })
}

/// The same, below the line.
fn subscript(ch: char) -> Option<char> {
    Some(match ch {
        '0' => '\u{2080}', '1' => '\u{2081}', '2' => '\u{2082}', '3' => '\u{2083}',
        '4' => '\u{2084}', '5' => '\u{2085}', '6' => '\u{2086}', '7' => '\u{2087}',
        '8' => '\u{2088}', '9' => '\u{2089}',
        '+' => '\u{208A}', '-' | '\u{2212}' => '\u{208B}', '=' => '\u{208C}',
        '(' => '\u{208D}', ')' => '\u{208E}',
        _ => return None,
    })
}

fn indent(text: &str, prefix: &str) -> String {
    if prefix.is_empty() {
        return text.to_owned();
    }
    text.split('\n')
        .map(|line| if line.is_empty() { String::new() } else { format!("{prefix}{line}") })
        .collect::<Vec<_>>()
        .join("\n")
}

// ---- grid table geometry -------------------------------------------------
//
// **Duplicated from `ferrodoc-markdown`, deliberately.** The two writers
// emit the same grid, and the arithmetic below was measured once against
// `pandoc -t markdown` and again against `pandoc -t plain` — but sharing
// it would mean this crate depending on `ferrodoc-markdown`, and that
// pulls comrak into a writer that has no parser in it. `ferrodoc-rst`
// carries its own for the same reason. Change one, check the others.

/// The inner width of each column: everything between two `+`, the
/// single space of padding at each side included.
fn grid_widths(table: &ferrodoc_ast::Table, wanted: &[usize], layout: usize, fills: bool) -> Vec<usize> {
    let count = table.colspecs.len();
    let stated: Vec<Option<f64>> = table
        .colspecs
        .iter()
        .map(|spec| match spec.width {
            ferrodoc_ast::ColWidth::ColWidth(fraction) => Some(fraction),
            ferrodoc_ast::ColWidth::ColWidthDefault => None,
        })
        .collect();

    if stated.iter().any(Option::is_some) {
        // The cells share `columns - count` and the separators are
        // `count + 1`, so a row of stated widths comes out one column
        // **wider** than `--columns`. That is pandoc's arithmetic as
        // measured, not a rounding of ours.
        let available = layout.saturating_sub(count);
        let mut widths: Vec<usize> = stated
            .iter()
            .map(|fraction| {
                #[expect(
                    clippy::cast_possible_truncation,
                    clippy::cast_sign_loss,
                    clippy::cast_precision_loss,
                    reason = "a column width is small, never negative, and \
                              well inside f64's mantissa"
                )]
                fraction.map_or(0, |fraction| (fraction * available as f64).floor() as usize)
            })
            .collect();
        // **The last column takes the remainder**, so three columns of a
        // quarter, a quarter and a half come out 9, 9 and 19 of 37 —
        // where flooring the last would give 18 and leave the row short.
        let used: usize = widths.iter().take(count.saturating_sub(1)).sum();
        if let Some(last) = widths.last_mut() {
            *last = available.saturating_sub(used);
        }
        if !fills {
            for (width, want) in widths.iter_mut().zip(wanted) {
                *width = (*width).max(*want);
            }
        }
        return widths;
    }

    if !fills {
        return wanted.to_vec();
    }
    fair_share(wanted, layout.saturating_sub(count + 1))
}

/// Divide `available` among columns that each *want* a width: everyone
/// asking for no more than an equal share gets exactly what they asked
/// for, and what they leave behind is shared out again among the rest.
///
/// Two columns wanting 32 and 7 of 27 come out 20 and 7 — the small one
/// is satisfied and the large one takes the rest — and three wanting 4,
/// 5 and 38 of 26 come out 4, 5 and 17. Both measured.
fn fair_share(wanted: &[usize], available: usize) -> Vec<usize> {
    let mut widths = vec![0_usize; wanted.len()];
    let mut settled = vec![false; wanted.len()];
    let mut left = available;
    let mut open = wanted.len();
    while open > 0 {
        let share = left / open;
        let mut moved = false;
        for (index, want) in wanted.iter().enumerate() {
            if !settled[index] && *want <= share {
                widths[index] = *want;
                settled[index] = true;
                left -= *want;
                open -= 1;
                moved = true;
            }
        }
        if !moved {
            break;
        }
    }
    if let Some(share) = left.checked_div(open) {
        let mut last = None;
        for index in 0..wanted.len() {
            if !settled[index] {
                widths[index] = share;
                last = Some(index);
            }
        }
        // The last unsettled column absorbs the division's remainder, so
        // the row is exactly as wide as it was told to be.
        let used: usize = widths.iter().sum();
        if let Some(index) = last {
            widths[index] += available.saturating_sub(used);
        }
    }
    widths
}

/// A `+---+` rule, or the `+===+` that closes a header — carrying the
/// alignment markers where they belong.
fn grid_rule(widths: &[usize], fill: char, aligns: Option<&[Alignment]>) -> String {
    let mut out = String::from("+");
    for (index, width) in widths.iter().enumerate() {
        let alignment =
            aligns.and_then(|aligns| aligns.get(index)).copied().unwrap_or(Alignment::AlignDefault);
        let (left, right) = match alignment {
            Alignment::AlignLeft => (true, false),
            Alignment::AlignRight => (false, true),
            Alignment::AlignCenter => (true, true),
            Alignment::AlignDefault => (false, false),
        };
        let marks = usize::from(left) + usize::from(right);
        if left {
            out.push(':');
        }
        for _ in 0..width.saturating_sub(marks) {
            out.push(fill);
        }
        if right {
            out.push(':');
        }
        out.push('+');
    }
    out
}

/// One row laid out: as tall as its tallest cell, every cell padded to
/// its column and held between pipes.
fn grid_row(cells: &[Vec<String>], widths: &[usize]) -> Vec<String> {
    let height = cells.iter().map(Vec::len).max().unwrap_or(1).max(1);
    (0..height)
        .map(|line| {
            let mut text = String::from("|");
            for (index, width) in widths.iter().enumerate() {
                let piece =
                    cells.get(index).and_then(|lines| lines.get(line)).map_or("", String::as_str);
                text.push(' ');
                text.push_str(piece);
                for _ in piece.chars().count()..width.saturating_sub(2) {
                    text.push(' ');
                }
                text.push_str(" |");
            }
            text
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use ferrodoc_ast::{Attr, Caption, Cell, ColSpec, ColWidth, ListAttributes, ListNumberDelim,
        ListNumberStyle, Row, Table, TableBody, TableFoot, TableHead, Target};

    /// **The plain writer breaks one word early rather than strand a
    /// bullet.** Pandoc does, and the reason it does is the same one that
    /// makes it a correctness rule in the markdown writers: a line that
    /// opens with a bare `+` reads as a list item.
    ///
    /// The prefix lengths here are the ones that actually diverged —
    /// found by walking the column and diffing against the pinned binary,
    /// not chosen.
    #[test]
    fn a_filled_line_never_strands_a_bullet() {
        for prefix in [64usize, 65, 69, 70] {
            let long = "x".repeat(prefix);
            let text = format!("{long} 8 + 24 + 8 = 40, and the discriminant makes 48 here.");
            // Built as an AST rather than parsed, so this crate keeps
            // its dependencies: `Str` per word, `Space` between.
            let mut inlines = Vec::new();
            for (index, word) in text.split(' ').enumerate() {
                if index > 0 {
                    inlines.push(Inline::Space);
                }
                inlines.push(Inline::Str(word.to_owned()));
            }
            let doc = Pandoc::new(vec![Block::Para(inlines)]);
            let plain = write_text_wrapped(&doc, 72);
            for line in plain.lines() {
                assert!(
                    !matches!(line.split(' ').next(), Some("+" | "-" | "*" | ">" | "1." | "1)")),
                    "prefix {prefix}: a line opens a block: {line:?}"
                );
            }
        }
    }

    fn ordered() -> ListAttributes {
        ListAttributes {
            start: 1,
            style: ListNumberStyle::Decimal,
            delim: ListNumberDelim::Period,
        }
    }

    fn plain(inlines: Vec<Inline>) -> Block {
        Block::Plain(inlines)
    }
    fn str_(s: &str) -> Inline {
        Inline::Str(s.to_owned())
    }

    /// Every assertion here is `pandoc -t plain --wrap=none` output, run
    /// and pasted. Nothing else can check this writer: pandoc cannot read
    /// plain text back, so there is no differential gate at all.
    #[test]
    fn formatting_is_dropped_and_a_tight_list_stays_tight() {
        let doc = Pandoc::new(vec![
            Block::Header(1, Attr::default(), vec![str_("Title")]),
            Block::Para(vec![
                Inline::Emph(vec![str_("em")]),
                Inline::Space,
                Inline::Link(
                    Box::default(),
                    vec![str_("link")],
                    Box::new(Target { url: "u".into(), title: String::new() }),
                ),
            ]),
            Block::BulletList(vec![vec![plain(vec![str_("a")])], vec![plain(vec![str_("b")])]]),
        ]);
        assert_eq!(write_text(&doc), "Title\n\nem link\n\n- a\n- b\n");

        // A `Para` inside any item makes the whole list loose.
        let loose = Pandoc::new(vec![Block::BulletList(vec![
            vec![Block::Para(vec![str_("a")])],
            vec![Block::Para(vec![str_("b")])],
        ])]);
        assert_eq!(write_text(&loose), "- a\n\n- b\n");
    }

    #[test]
    fn a_quote_is_indented_two_and_code_four() {
        let doc = Pandoc::new(vec![Block::BlockQuote(vec![
            Block::Para(vec![str_("outer")]),
            Block::BlockQuote(vec![Block::Para(vec![str_("inner")])]),
        ])]);
        assert_eq!(write_text(&doc), "  outer\n\n    inner\n");

        let code = Pandoc::new(vec![Block::CodeBlock(Attr::default(), "one\ntwo\n".into())]);
        assert_eq!(write_text(&code), "    one\n    two\n");
    }

    #[test]
    fn an_ordered_marker_is_padded_to_a_common_width() {
        // pandoc writes `1.  one` and `10. ten` — the column is as wide as
        // the widest marker plus a space, and never under four.
        let items: Vec<Vec<Block>> =
            (1..=10).map(|n| vec![plain(vec![str_(&format!("i{n}"))])]).collect();
        let doc = Pandoc::new(vec![Block::OrderedList(ordered(), items)]);
        let text = write_text(&doc);
        assert!(text.starts_with("1.  i1\n"), "{text}");
        assert!(text.contains("\n10. i10\n"), "{text}");

        // Continuation lines line up under the content, not the marker.
        let wrapped = Pandoc::new(vec![Block::OrderedList(
            ordered(),
            vec![vec![plain(vec![str_("a")]), plain(vec![str_("b")])]],
        )]);
        assert_eq!(write_text(&wrapped), "1.  a\n    b\n");
    }

    #[test]
    fn a_table_is_column_aligned_with_a_rule_under_the_head() {
        let cell = |s: &str| Cell {
            attr: Attr::default(),
            alignment: Alignment::AlignDefault,
            row_span: 1,
            col_span: 1,
            blocks: vec![plain(vec![str_(s)])],
        };
        let row = |cells: Vec<Cell>| Row { attr: Attr::default(), cells };
        let spec = |a| ColSpec { alignment: a, width: ColWidth::ColWidthDefault };
        let table = Table {
            attr: Attr::default(),
            caption: Caption::default(),
            colspecs: vec![spec(Alignment::AlignDefault), spec(Alignment::AlignRight)],
            head: TableHead {
                attr: Attr::default(),
                rows: vec![row(vec![cell("Phase"), cell("Starts")])],
            },
            bodies: vec![TableBody {
                attr: Attr::default(),
                row_head_columns: 0,
                head: Vec::new(),
                body: vec![
                    row(vec![cell("Inventory"), cell("2026-01-06")]),
                    row(vec![cell("Pilot"), cell("2026-02-17")]),
                ],
            }],
            foot: TableFoot { attr: Attr::default(), rows: Vec::new() },
        };
        // Widths are the widest cell plus two; the right-aligned column
        // pads on the left; trailing space is trimmed.
        assert_eq!(
            write_text(&Pandoc::new(vec![Block::Table(Box::new(table))])),
            concat!(
                "  Phase             Starts\n",
                "  ----------- ------------\n",
                "  Inventory     2026-01-06\n",
                "  Pilot         2026-02-17\n",
            )
        );
    }

    #[test]
    fn a_footnote_becomes_a_number_and_a_body_at_the_end() {
        let note = |s: &str| Inline::Note(vec![Block::Para(vec![str_(s)])]);
        let doc = Pandoc::new(vec![Block::Para(vec![
            str_("a"),
            note("one"),
            Inline::Space,
            str_("b"),
            note("two"),
        ])]);
        assert_eq!(write_text(&doc), "a[1] b[2]\n\n[1] one\n\n[2] two\n");
    }

    #[test]
    fn strikeout_keeps_its_tildes_and_a_picture_keeps_its_brackets() {
        // Without the tildes the sentence says the opposite of what it
        // means; without the brackets an alt text reads as prose.
        let doc = Pandoc::new(vec![Block::Para(vec![
            Inline::Strikeout(vec![str_("gone")]),
            Inline::Space,
            Inline::Image(
                Box::default(),
                vec![str_("the logo")],
                Box::new(Target { url: "l.png".into(), title: String::new() }),
            ),
        ])]);
        assert_eq!(write_text(&doc), "~~gone~~ [the logo]\n");
    }

    #[test]
    fn a_horizontal_rule_is_seventy_two_dashes() {
        let doc = Pandoc::new(vec![Block::HorizontalRule]);
        assert_eq!(write_text(&doc), format!("{}\n", "-".repeat(72)));
    }
}
