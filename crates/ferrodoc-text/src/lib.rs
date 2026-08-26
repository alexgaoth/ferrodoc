//! Plain-text writer for the ferrodoc (pandoc-compatible) AST.
//!
//! [`write_text`] follows `pandoc -t plain --wrap=none`: inline markup is
//! dropped except strikeout's tildes, block structure is kept by
//! indentation, tables are column-aligned, and footnotes become numbered
//! references with their bodies at the end. Every rule below was probed
//! against pandoc 3.8.2.1 and is asserted on literal output, because no
//! differential gate reads this format — pandoc cannot parse it back.
//!
//! Not matched, and stated rather than hidden: pandoc renders `Math` as
//! Unicode (`$x^2$` becomes `x²`) where this writes the TeX, and it fills
//! to `--columns` where this never wraps.

use ferrodoc_ast::{Alignment, Block, Inline, ListNumberDelim, Pandoc};
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
struct Writer {
    /// Footnote bodies in reference order; written at the end as `[N] …`.
    notes: Vec<String>,
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
    /// How many list items deep the block being written is. A list item
    /// renders its content with an **empty** prefix and adds the
    /// continuation indent afterwards, so the prefix cannot say whether
    /// the block is nested — and a code block four spaces in is markup at
    /// the top level and four stray spaces inside an item.
    nested: usize,
}

impl Writer {
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

    fn blocks(&mut self, blocks: &[Block], out: &mut Vec<String>, prefix: &str) {
        for block in blocks {
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
            // heading broken in two reads as two headings.
            Block::Header(_, _, inlines) => {
                let text = self.inlines(inlines).replace([BREAK, SOFT], " ");
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
            // The **first block of a container** is the exception, and
            // `nested` is what says so: a quote or an item that opens
            // with code gets the container's indent and nothing more —
            // `> ```{.sh}` is 2 where the same block one paragraph later
            // is 6. A list item renders that first block with an empty
            // prefix and adds its continuation indent afterwards, so the
            // prefix cannot be read for it either.
            Block::CodeBlock(_, text) => {
                let inner = if self.nested == 0 {
                    format!("{prefix}    ")
                } else {
                    prefix.to_owned()
                };
                out.push(indent(text.trim_end_matches('\n'), &inner));
            }
            // Two more spaces per level, so nesting is visible.
            Block::BlockQuote(inner) => {
                let inner_prefix = format!("{prefix}  ");
                let before = out.len();
                if let Some((first, rest)) = inner.split_first() {
                    self.nested += 1;
                    self.block(first, out, &inner_prefix);
                    self.nested -= 1;
                    self.blocks(rest, out, &inner_prefix);
                }
                // A quote whose content renders to nothing — a raw block
                // in another format is one — is still a quote, and pandoc
                // writes its indentation on a line of its own.
                if out.len() == before {
                    out.push(inner_prefix);
                }
            }
            Block::Div(_, inner) => self.blocks(inner, out, prefix),
            Block::Figure(_, caption, inner) => {
                self.blocks(inner, out, prefix);
                self.blocks(&caption.blocks, out, prefix);
            }
            Block::BulletList(items) => self.list(items, out, prefix, |_| "- ".to_owned()),
            Block::OrderedList(attrs, items) => {
                let start = attrs.start;
                // The delimiter the list was written with. A list that
                // said `3)` came out saying `3.`, which is the one thing
                // a plain-text rendering of a list can still get wrong.
                let close = match attrs.delim {
                    ListNumberDelim::OneParen | ListNumberDelim::TwoParens => ')',
                    _ => '.',
                };
                self.list(items, out, prefix, move |i| {
                    format!("{}{close}", start + i64::try_from(i).unwrap_or(0))
                });
            }
            Block::DefinitionList(entries) => {
                for (term, definitions) in entries {
                    let text = self.inlines(term);
                    out.push(indent(&self.lay_out(&text, prefix.chars().count()), prefix));
                    for definition in definitions {
                        self.blocks(definition, out, &format!("{prefix}    "));
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
            Block::Table(table) => {
                let rendered = self.table(table);
                if !rendered.is_empty() {
                    // A table opens with a blank line of its own, which
                    // is invisible at the top level and is the
                    // container's indentation inside one. Probed: a quote
                    // holding a table starts with a line of just the
                    // quote's two spaces.
                    let mut text = indent(&rendered, prefix);
                    if !prefix.is_empty() {
                        text.insert_str(0, &format!("{prefix}\n"));
                    }
                    out.push(text);
                }
                self.blocks(&table.caption.blocks, out, prefix);
            }
            // The rule fills the column count asked for, not a fixed 72.
            Block::HorizontalRule => {
                out.push(indent(&"-".repeat(self.columns.unwrap_or(COLUMNS)), prefix));
            }
            Block::RawBlock(..) => {}
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
            if let Some((first, rest)) = item.split_first() {
                self.nested += 1;
                self.block(first, &mut inner, "");
                self.nested -= 1;
                self.blocks(rest, &mut inner, "");
            }
            self.reserved = reserved;
            let body = inner.join(if loose { "\n\n" } else { "\n" });
            let mut lines = body.split('\n');
            let mut text = format!("{}{}", head, lines.next().unwrap_or_default());
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
        let head: Vec<Vec<String>> = self.rows(&table.head.rows);
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
        let widths: Vec<usize> = (0..columns)
            .map(|c| {
                head.iter()
                    .chain(&body)
                    .filter_map(|row| row.get(c))
                    .map(|cell| cell.chars().count())
                    .max()
                    .unwrap_or(0)
                    + 2
            })
            .collect();
        let align = |c: usize| {
            table.colspecs.get(c).map_or(Alignment::AlignDefault, |s| s.alignment)
        };
        // Each cell is padded to its column and the columns are joined by
        // one space — **except the last, which takes only the padding in
        // front of it**. That is why a row ending in an empty cell keeps
        // its trailing spaces and a header ending in its column's widest
        // word does not.
        let line = |row: &Vec<String>| {
            let mut out = String::from("  ");
            for (c, width) in widths.iter().enumerate() {
                let cell = row.get(c).map_or("", String::as_str);
                let slack = width.saturating_sub(cell.chars().count());
                let (before, after) = match align(c) {
                    Alignment::AlignRight => (slack, 0),
                    Alignment::AlignCenter => (slack / 2, slack - slack / 2),
                    _ => (0, slack),
                };
                out.push_str(&" ".repeat(before));
                out.push_str(cell);
                if c + 1 < widths.len() {
                    out.push_str(&" ".repeat(after));
                    out.push(' ');
                }
            }
            out
        };
        let rule: Vec<String> = widths.iter().map(|w| "-".repeat(*w)).collect();
        let mut lines: Vec<String> = head.iter().map(line).collect();
        lines.push(format!("  {}", rule.join(" ")));
        lines.extend(body.iter().map(line));
        lines.join("\n")
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
                Inline::Str(s) | Inline::Code(_, s) | Inline::Math(_, s) => out.push_str(s),
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
                Inline::Emph(i)
                | Inline::Strong(i)
                | Inline::Superscript(i)
                | Inline::Subscript(i)
                | Inline::SmallCaps(i)
                | Inline::Underline(i)
                | Inline::Quoted(_, i)
                | Inline::Cite(_, i)
                | Inline::Span(_, i)
                | Inline::Link(_, i, _) => self.collect(out, i),
                Inline::Note(blocks) => {
                    // Reserve the number before rendering, so a note inside
                    // a note cannot take this one's.
                    let index = self.notes.len();
                    self.notes.push(String::new());
                    let mut inner = Vec::new();
                    // The body is written after `[N] `, and those columns
                    // are gone before the first word: the label is not a
                    // hanging indent — the second line starts at column
                    // zero — but the first line is that much shorter.
                    let hanging = self.hanging;
                    self.hanging = format!("[{}] ", index + 1).chars().count();
                    let in_cell = std::mem::take(&mut self.in_cell);
                    self.blocks(blocks, &mut inner, "");
                    self.in_cell = in_cell;
                    self.hanging = hanging;
                    // The body's blocks stay blocks: a footnote of two
                    // paragraphs and a list is three paragraphs at the
                    // end of the document, not one run-on line.
                    self.notes[index] = inner.join("\n\n");
                    let _ = write!(out, "[{}]", index + 1);
                }
                Inline::RawInline(..) => {}
            }
        }
    }
}

/// Put `prefix` on every non-empty line of `text`.
/// Greedy fill: take words while they fit, break at the last mark that
/// did. A word longer than the width goes on its own line and overruns —
/// breaking inside it would invent a break the text does not have.
fn fill(line: &str, first: usize, rest: usize, out: &mut String) {
    let mut width = 0;
    let mut limit = first;
    for (index, word) in line.split([BREAK, SOFT]).enumerate() {
        let word_width = word.chars().count();
        if index == 0 {
            width = word_width;
        } else if width + 1 + word_width <= limit {
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

fn indent(text: &str, prefix: &str) -> String {
    if prefix.is_empty() {
        return text.to_owned();
    }
    text.split('\n')
        .map(|line| if line.is_empty() { String::new() } else { format!("{prefix}{line}") })
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use ferrodoc_ast::{Attr, Caption, Cell, ColSpec, ColWidth, ListAttributes, ListNumberDelim,
        ListNumberStyle, Row, Table, TableBody, TableFoot, TableHead, Target};

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
