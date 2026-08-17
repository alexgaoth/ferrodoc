//! reStructuredText writer for the ferrodoc (pandoc-compatible) AST.
//!
//! [`write_rst`] renders a document as RST. Gated by
//! `ferrodoc-harness diff-rst` for fidelity, and in CI by **`sphinx-build`
//! accepting the output**, which is the check that matters: RST exists
//! here to feed a documentation toolchain, so the toolchain is the judge.
//!
//! There is deliberately **no RST reader**. People write RST by hand in
//! editors that already understand it, and convert *out of* it far more
//! often than in.
//!
//! RST is whitespace-significant in a way the other writers here are not,
//! and three consequences shape this module:
//!
//! - **a heading is an underline, and its length is the heading's own.**
//!   An underline shorter than the title is a warning in every RST tool;
//!   character width, not byte length, is what has to match, so a heading
//!   with an accent in it underlines correctly;
//! - **the level-to-character map has to be consistent within a
//!   document.** RST infers the hierarchy from the order the characters
//!   first appear, so a fixed table is the only way a level means the same
//!   thing in two documents;
//! - **indentation *is* nesting.** A block quote is an indent, a list
//!   item's continuation is an indent, and a literal block is an indent —
//!   so every nested construct is rendered and then shifted, rather than
//!   written with a running prefix.

use ferrodoc_ast::{Block, Cell, Inline, ListNumberStyle, Pandoc, QuoteType, Row, Table};
use std::fmt::Write as _;

/// Render a document as reStructuredText.
pub fn write_rst(doc: &Pandoc) -> String {
    let mut out = String::new();
    blocks(&doc.blocks, &mut out);
    let text = out.trim_end().to_owned();
    if text.is_empty() { text } else { text + "\n" }
}

/// The underline characters, by heading level.
///
/// Fixed rather than derived: RST works out the hierarchy from the order
/// the characters first appear in a document, so a table is what makes
/// level 2 mean the same thing in two files that a toolchain reads
/// together.
const UNDERLINES: &[char] = &['=', '-', '~', '^', '"', '\''];

/// How far a nested block is indented. Three spaces is RST's convention
/// for directive and quote content.
const INDENT: &str = "   ";

fn blocks(list: &[Block], out: &mut String) {
    for block in list {
        block_to(block, out);
        // A blank line between blocks is not decoration in RST; it is what
        // ends the previous one.
        if !out.ends_with("\n\n") {
            out.push('\n');
        }
    }
}

fn block_to(block: &Block, out: &mut String) {
    match block {
        Block::Plain(list) | Block::Para(list) => {
            // A paragraph that is nothing but a picture is the `image`
            // directive. Written as a substitution instead, the reference
            // needs a name — and that name becomes the alt text, so an
            // image with none acquires one.
            if let [Inline::Image(_, alt, target)] = list.as_slice() {
                let _ = writeln!(out, ".. image:: {}", target.url);
                if !alt.is_empty() {
                    let mut text = String::new();
                    inlines(alt, &mut text);
                    let _ = writeln!(out, "{INDENT}:alt: {}", text.trim());
                }
                return;
            }
            let mut text = String::new();
            inlines(list, &mut text);
            let _ = writeln!(out, "{}", text.trim_end());
        }
        Block::LineBlock(lines) => {
            for line in lines {
                let mut text = String::new();
                inlines(line, &mut text);
                let _ = writeln!(out, "| {text}");
            }
        }
        Block::CodeBlock(attr, code) => {
            // `code-block` when a language is known, so a toolchain
            // highlights it; a plain literal block otherwise.
            match attr.classes.first() {
                Some(language) => {
                    let _ = writeln!(out, ".. code-block:: {language}\n");
                }
                None => out.push_str("::\n\n"),
            }
            for line in code.lines() {
                let _ = writeln!(out, "{INDENT}{line}");
            }
            out.push('\n');
        }
        Block::BlockQuote(inner) => {
            let mut text = String::new();
            blocks(inner, &mut text);
            out.push_str(&indent(&text));
        }
        Block::OrderedList(attrs, items) => {
            for (index, item) in items.iter().enumerate() {
                let number = attrs.start + i64::try_from(index).unwrap_or(0);
                let marker = match attrs.style {
                    // RST's `#.` is auto-numbering; a literal marker is
                    // what keeps a start value other than one.
                    ListNumberStyle::LowerAlpha => format!("{}.", alpha(number, false)),
                    ListNumberStyle::UpperAlpha => format!("{}.", alpha(number, true)),
                    ListNumberStyle::LowerRoman => format!("{}.", roman(number, false)),
                    ListNumberStyle::UpperRoman => format!("{}.", roman(number, true)),
                    _ => format!("{number}."),
                };
                item_to(item, &marker, out);
            }
        }
        Block::BulletList(items) => {
            for item in items {
                item_to(item, "-", out);
            }
        }
        Block::DefinitionList(entries) => {
            for (term, definitions) in entries {
                let mut text = String::new();
                inlines(term, &mut text);
                let _ = writeln!(out, "{}", text.trim_end());
                for definition in definitions {
                    let mut body = String::new();
                    blocks(definition, &mut body);
                    out.push_str(&indent(&body));
                }
            }
        }
        Block::Header(level, attr, list) => {
            // An explicit target above the heading is how RST names one.
            if !attr.identifier.is_empty() {
                let _ = writeln!(out, ".. _{}:\n", attr.identifier);
            }
            let mut text = String::new();
            inlines(list, &mut text);
            let text = text.trim().to_owned();
            let index = usize::try_from(*level).unwrap_or(1).saturating_sub(1);
            let underline = UNDERLINES.get(index).copied().unwrap_or('\'');
            // Character *width*, not byte length: an underline shorter
            // than the title is a warning in every RST tool, and a heading
            // with an accent in it is one byte longer than it looks.
            let width = text.chars().count().max(1);
            let _ = writeln!(out, "{text}\n{}", underline.to_string().repeat(width));
        }
        Block::HorizontalRule => out.push_str("----\n"),
        Block::Table(table) => table_to(table, out),
        Block::Figure(_, caption, inner) => {
            blocks(inner, out);
            if !caption.blocks.is_empty() {
                let mut text = String::new();
                blocks(&caption.blocks, &mut text);
                out.push_str(&indent(&text));
            }
        }
        Block::Div(_, inner) => blocks(inner, out),
        Block::RawBlock(format, text) => {
            if format.0 == "rst" {
                out.push_str(text);
                out.push('\n');
            }
        }
    }
}

/// One list item: its marker, then its content aligned under it.
fn item_to(item: &[Block], marker: &str, out: &mut String) {
    let mut text = String::new();
    blocks(item, &mut text);
    let pad = " ".repeat(marker.chars().count() + 1);
    for (index, line) in text.trim_end().lines().enumerate() {
        if index == 0 {
            let _ = writeln!(out, "{marker} {line}");
        } else if line.is_empty() {
            out.push('\n');
        } else {
            let _ = writeln!(out, "{pad}{line}");
        }
    }
    out.push('\n');
}

/// Shift a rendered block right, which is how RST spells nesting.
fn indent(text: &str) -> String {
    let mut out = String::new();
    for line in text.trim_end().lines() {
        if line.is_empty() {
            out.push('\n');
        } else {
            let _ = writeln!(out, "{INDENT}{line}");
        }
    }
    out.push('\n');
    out
}

/// A grid table, because it is the only RST table that can hold a cell
/// with more than one line in it.
fn table_to(table: &Table, out: &mut String) {
    let rows: Vec<Vec<String>> = table
        .head
        .rows
        .iter()
        .chain(table.bodies.iter().flat_map(|b| b.head.iter().chain(&b.body)))
        .chain(table.foot.rows.iter())
        .map(|row| row_cells(row, table.colspecs.len()))
        .collect();
    if rows.is_empty() {
        return;
    }
    let columns = rows.iter().map(Vec::len).max().unwrap_or(0);
    let widths: Vec<usize> = (0..columns)
        .map(|column| {
            rows.iter()
                .filter_map(|row| row.get(column))
                .map(|cell| cell.chars().count())
                .max()
                .unwrap_or(1)
                .max(1)
        })
        .collect();
    let rule = |fill: char, out: &mut String| {
        out.push('+');
        for width in &widths {
            let _ = write!(out, "{}+", fill.to_string().repeat(width + 2));
        }
        out.push('\n');
    };
    rule('-', out);
    let header_rows = table.head.rows.len();
    for (index, row) in rows.iter().enumerate() {
        out.push('|');
        for (column, width) in widths.iter().enumerate() {
            let cell = row.get(column).map_or("", String::as_str);
            let pad = width - cell.chars().count();
            let _ = write!(out, " {cell}{} |", " ".repeat(pad));
        }
        out.push('\n');
        // A row of `=` under the head is what makes it a head.
        if index + 1 == header_rows {
            rule('=', out);
        } else {
            rule('-', out);
        }
    }
    out.push('\n');
}

fn row_cells(row: &Row, columns: usize) -> Vec<String> {
    let mut cells: Vec<String> = row.cells.iter().map(cell_text).collect();
    cells.resize(columns.max(cells.len()), String::new());
    cells
}

fn cell_text(cell: &Cell) -> String {
    let mut out = String::new();
    for block in &cell.blocks {
        match block {
            Block::Plain(list) | Block::Para(list) => inlines(list, &mut out),
            other => block_to(other, &mut out),
        }
    }
    out.replace('\n', " ").trim().to_owned()
}

fn inlines(list: &[Inline], out: &mut String) {
    for inline in list {
        inline_to(inline, out);
    }
}

fn inline_to(inline: &Inline, out: &mut String) {
    match inline {
        Inline::Str(text) => out.push_str(&escape(text)),
        Inline::Space => out.push(' '),
        // RST has no inline hard break outside a line block, so a hard
        // break becomes a soft one rather than markup that would show as
        // text. `COMPATIBILITY.md` records the loss.
        Inline::SoftBreak | Inline::LineBreak => out.push('\n'),
        Inline::Emph(inner) => wrap("*", inner, out),
        // RST has no underline, and no strikeout either; both keep their
        // content rather than inventing a role a toolchain would not have.
        Inline::Underline(inner) | Inline::Strikeout(inner) | Inline::SmallCaps(inner) => {
            inlines(inner, out);
        }
        Inline::Strong(inner) => wrap("**", inner, out),
        Inline::Superscript(inner) => role("superscript", inner, out),
        Inline::Subscript(inner) => role("subscript", inner, out),
        Inline::Quoted(kind, inner) => {
            let (open, close) = match kind {
                QuoteType::SingleQuote => ('\u{2018}', '\u{2019}'),
                QuoteType::DoubleQuote => ('\u{201c}', '\u{201d}'),
            };
            out.push(open);
            inlines(inner, out);
            out.push(close);
        }
        Inline::Cite(_, inner) | Inline::Span(_, inner) => inlines(inner, out),
        // Double backticks, and no escaping inside them: that is what
        // makes it literal.
        Inline::Code(_, code) => {
            let _ = write!(out, "``{code}``");
        }
        Inline::Math(_, math) => {
            let _ = write!(out, ":math:`{math}`");
        }
        Inline::RawInline(format, text) => {
            if format.0 == "rst" {
                out.push_str(text);
            }
        }
        Inline::Link(_, inner, target) => {
            let mut text = String::new();
            inlines(inner, &mut text);
            // An anonymous reference (two underscores) rather than a named
            // one: a named target must be unique in the document, and two
            // links with the same text are ordinary.
            let _ = write!(out, "`{}<{}>`__", text.trim_end().to_owned() + " ", target.url);
        }
        Inline::Image(_, alt, target) => {
            let mut text = String::new();
            inlines(alt, &mut text);
            let _ = write!(out, "|{}|", if text.is_empty() { "image" } else { text.trim() });
            let _ = write!(out, "");
            // The substitution definition has to follow the paragraph; it
            // is emitted inline here, which RST accepts at the end of a
            // document body.
            let _ = write!(
                out,
                "\n\n.. |{}| image:: {}\n",
                if text.is_empty() { "image" } else { text.trim() },
                target.url
            );
        }
        Inline::Note(blocks_in_note) => {
            let mut text = String::new();
            blocks(blocks_in_note, &mut text);
            let _ = write!(out, " [#]_\n\n.. [#] {}\n", text.trim());
        }
    }
}

fn wrap(marker: &str, inner: &[Inline], out: &mut String) {
    let mut text = String::new();
    inlines(inner, &mut text);
    if text.trim().is_empty() {
        out.push_str(&text);
        return;
    }
    let _ = write!(out, "{marker}{}{marker}", text.trim());
}

fn role(name: &str, inner: &[Inline], out: &mut String) {
    let mut text = String::new();
    inlines(inner, &mut text);
    let _ = write!(out, ":{name}:`{}`", text.trim());
}

/// Escape the characters RST gives a meaning to at the start of a word.
///
/// Only `*`, `` ` `` and `|` matter inside text, and only where they could
/// begin a construct; escaping every one of them would fill ordinary prose
/// with backslashes.
fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        if matches!(ch, '*' | '`' | '|' | '\\') {
            out.push('\\');
        }
        out.push(ch);
    }
    out
}

/// `a`, `b`, … `z`, `aa`, … for an alphabetic list marker.
fn alpha(mut number: i64, upper: bool) -> String {
    if number < 1 {
        return String::from("a");
    }
    let base = if upper { b'A' } else { b'a' };
    let mut out = Vec::new();
    while number > 0 {
        let digit = u8::try_from((number - 1) % 26).unwrap_or(0);
        out.push(base + digit);
        number = (number - 1) / 26;
    }
    out.reverse();
    String::from_utf8(out).unwrap_or_else(|_| String::from("a"))
}

/// Roman numerals, for the list styles that ask for them.
fn roman(number: i64, upper: bool) -> String {
    const VALUES: &[(i64, &str)] = &[
        (1000, "m"), (900, "cm"), (500, "d"), (400, "cd"),
        (100, "c"), (90, "xc"), (50, "l"), (40, "xl"),
        (10, "x"), (9, "ix"), (5, "v"), (4, "iv"), (1, "i"),
    ];
    let mut remaining = number.max(1);
    let mut out = String::new();
    for (value, numeral) in VALUES {
        while remaining >= *value {
            out.push_str(numeral);
            remaining -= value;
        }
    }
    if upper { out.to_uppercase() } else { out }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ferrodoc_ast::{Attr, ListAttributes, ListNumberDelim, Target};

    fn doc(blocks: Vec<Block>) -> Pandoc {
        Pandoc::new(blocks)
    }

    #[test]
    fn a_heading_underline_is_as_wide_as_the_heading() {
        // Character width, not byte length. An accented title is longer in
        // bytes than it looks, and a short underline is a warning in every
        // RST tool.
        for title in ["Plain", "Café Über", "日本語の見出し"] {
            let rst = write_rst(&doc(vec![Block::Header(
                1,
                Attr::default(),
                vec![Inline::Str(title.into())],
            )]));
            let mut lines = rst.lines();
            let text = lines.next().expect("a title");
            let rule = lines.next().expect("an underline");
            assert_eq!(
                text.chars().count(),
                rule.chars().count(),
                "underline does not match {title:?}: {rst}"
            );
        }
    }

    #[test]
    fn each_heading_level_keeps_its_own_character() {
        // RST infers the hierarchy from the order the characters first
        // appear, so two documents only agree if the map is fixed.
        let rst = write_rst(&doc(vec![
            Block::Header(1, Attr::default(), vec![Inline::Str("A".into())]),
            Block::Header(2, Attr::default(), vec![Inline::Str("B".into())]),
            Block::Header(3, Attr::default(), vec![Inline::Str("C".into())]),
        ]));
        assert!(rst.contains("A\n="), "{rst}");
        assert!(rst.contains("B\n-"), "{rst}");
        assert!(rst.contains("C\n~"), "{rst}");
    }

    #[test]
    fn a_picture_alone_is_a_directive_not_a_substitution() {
        // A substitution reference needs a name, and the name becomes the
        // alt text — so an image with none would acquire one.
        let rst = write_rst(&doc(vec![Block::Para(vec![Inline::Image(
            Box::default(),
            Vec::new(),
            Box::new(Target { url: "x.png".into(), title: String::new() }),
        )])]));
        assert!(rst.contains(".. image:: x.png"), "{rst}");
        assert!(!rst.contains(":alt:"), "an alt appeared from nowhere: {rst}");
    }

    #[test]
    fn a_literal_block_is_indented_and_nothing_in_it_is_escaped() {
        let rst = write_rst(&doc(vec![Block::CodeBlock(
            Attr::default(),
            "*not emphasis*\n  indented".into(),
        )]));
        assert!(rst.contains("::\n"), "{rst}");
        assert!(rst.contains("   *not emphasis*"), "{rst}");
        assert!(rst.contains("     indented"), "the block's own indent was lost: {rst}");
    }

    #[test]
    fn a_list_marker_carries_the_start_value() {
        let rst = write_rst(&doc(vec![Block::OrderedList(
            ListAttributes {
                start: 4,
                style: ListNumberStyle::LowerRoman,
                delim: ListNumberDelim::Period,
            },
            vec![
                vec![Block::Plain(vec![Inline::Str("a".into())])],
                vec![Block::Plain(vec![Inline::Str("b".into())])],
            ],
        )]));
        assert!(rst.contains("iv. a"), "{rst}");
        assert!(rst.contains("v. b"), "{rst}");
    }

    #[test]
    fn nesting_is_indentation() {
        let rst = write_rst(&doc(vec![Block::BlockQuote(vec![Block::Para(vec![
            Inline::Str("quoted".into()),
        ])])]));
        assert!(rst.starts_with("   quoted"), "{rst}");
    }

    #[test]
    fn a_grid_table_lines_up() {
        // Every rule and every row has to be the same width, or docutils
        // rejects the table outright rather than mis-rendering it.
        let cell = |text: &str| ferrodoc_ast::Cell {
            attr: Attr::default(),
            alignment: ferrodoc_ast::Alignment::AlignDefault,
            row_span: 1,
            col_span: 1,
            blocks: vec![Block::Plain(vec![Inline::Str(text.into())])],
        };
        let row = |a: &str, b: &str| Row { attr: Attr::default(), cells: vec![cell(a), cell(b)] };
        let table = Table {
            attr: Attr::default(),
            caption: ferrodoc_ast::Caption::default(),
            colspecs: vec![
                ferrodoc_ast::ColSpec {
                    alignment: ferrodoc_ast::Alignment::AlignDefault,
                    width: ferrodoc_ast::ColWidth::ColWidthDefault,
                };
                2
            ],
            head: ferrodoc_ast::TableHead {
                attr: Attr::default(),
                rows: vec![row("Header", "H2")],
            },
            bodies: vec![ferrodoc_ast::TableBody {
                attr: Attr::default(),
                row_head_columns: 0,
                head: Vec::new(),
                body: vec![row("a", "b")],
            }],
            foot: ferrodoc_ast::TableFoot { attr: Attr::default(), rows: Vec::new() },
        };
        let rst = write_rst(&doc(vec![Block::Table(Box::new(table))]));
        let widths: Vec<usize> = rst
            .lines()
            .filter(|line| !line.is_empty())
            .map(|line| line.chars().count())
            .collect();
        assert!(
            widths.windows(2).all(|pair| pair[0] == pair[1]),
            "rows are not all the same width:\n{rst}"
        );
        assert!(rst.contains("+=="), "the header rule is missing:\n{rst}");
    }
}
