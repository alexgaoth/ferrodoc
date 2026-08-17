//! `AsciiDoc` writer for the ferrodoc (pandoc-compatible) AST.
//!
//! [`write_asciidoc`] renders a document as `AsciiDoc`.
//!
//! **There is no differential gate for this writer, and there cannot be.**
//! Pandoc writes `AsciiDoc` and does not read it — "Pandoc can convert to
//! asciidoc, but not from asciidoc" — so there is no oracle to compare
//! against. It is judged instead by **`asciidoctor` accepting the output**
//! in CI, which is the check that matters anyway: this writer exists to
//! feed Asciidoctor and Antora, so the toolchain is the judge. The tests
//! below hold the shapes that a toolchain accepts but silently
//! mis-renders.
//!
//! There is deliberately no `AsciiDoc` reader either: people write it by
//! hand in editors that understand it, and convert *out of* it far more
//! often than in.
//!
//! Three things are worth knowing before changing this:
//!
//! - **the emphasis markers are the opposite way round from markdown's.**
//!   `_x_` is italic and `*x*` is bold, which is the single easiest
//!   mistake to make here and produces a document that looks almost right;
//! - **a delimited block is a run of at least four characters**, and the
//!   run has to be longer than any run inside it — otherwise a code sample
//!   containing `----` ends the listing early and the rest of the document
//!   becomes prose;
//! - **a section title's level is its number of `=`**, and level 0 (`= x`)
//!   is the document title, which may appear only once. Every heading here
//!   starts at `==`, so a document with two level-1 headings is still
//!   valid.

use ferrodoc_ast::{Block, Cell, Inline, ListNumberStyle, Pandoc, QuoteType, Table};
use std::fmt::Write as _;

/// Render a document as `AsciiDoc`.
pub fn write_asciidoc(doc: &Pandoc) -> String {
    let mut out = String::new();
    blocks(&doc.blocks, &mut out, 0);
    let text = out.trim_end().to_owned();
    if text.is_empty() { text } else { text + "\n" }
}

fn blocks(list: &[Block], out: &mut String, depth: usize) {
    for block in list {
        block_to(block, out, depth);
        if !out.ends_with("\n\n") {
            out.push('\n');
        }
    }
}

fn block_to(block: &Block, out: &mut String, depth: usize) {
    match block {
        Block::Plain(list) | Block::Para(list) => {
            let mut text = String::new();
            inlines(list, &mut text);
            let _ = writeln!(out, "{}", text.trim_end());
        }
        Block::LineBlock(lines) => {
            // A `[verse]` block is the only one that keeps line breaks
            // without turning the content into code.
            out.push_str("[verse]\n--\n");
            for line in lines {
                let mut text = String::new();
                inlines(line, &mut text);
                let _ = writeln!(out, "{text}");
            }
            out.push_str("--\n");
        }
        Block::CodeBlock(attr, code) => {
            if let Some(language) = attr.classes.first() {
                let _ = writeln!(out, "[source,{language}]");
            }
            let fence = fence_for(code, '-');
            let _ = writeln!(out, "{fence}\n{}\n{fence}", code.trim_end());
        }
        Block::BlockQuote(inner) => {
            let mut text = String::new();
            blocks(inner, &mut text, depth);
            let fence = fence_for(&text, '_');
            let _ = writeln!(out, "[quote]\n{fence}\n{}\n{fence}", text.trim_end());
        }
        Block::OrderedList(attrs, items) => {
            // The marker's *length* is the nesting depth, which is how
            // a nested list is spelled here.
            let marker = ".".repeat(depth + 1);
            if attrs.start != 1 {
                let _ = writeln!(out, "[start={}]", attrs.start);
            }
            if let Some(style) = number_style(attrs.style) {
                let _ = writeln!(out, "[{style}]");
            }
            for item in items {
                item_to(item, &marker, out, depth);
            }
        }
        Block::BulletList(items) => {
            let marker = "*".repeat(depth + 1);
            for item in items {
                item_to(item, &marker, out, depth);
            }
        }
        Block::DefinitionList(entries) => {
            for (term, definitions) in entries {
                let mut text = String::new();
                inlines(term, &mut text);
                let _ = writeln!(out, "{}::", text.trim());
                for definition in definitions {
                    let mut body = String::new();
                    blocks(definition, &mut body, depth);
                    for line in body.trim_end().lines() {
                        let _ = writeln!(out, "  {line}");
                    }
                }
            }
        }
        Block::Header(level, attr, list) => {
            if !attr.identifier.is_empty() {
                let _ = writeln!(out, "[[{}]]", attr.identifier);
            }
            let mut text = String::new();
            inlines(list, &mut text);
            // Levels start at `==`: `=` is the document title and may
            // appear only once, so a document with two level-1 headings
            // would be invalid.
            let marks = "=".repeat(usize::try_from(*level).unwrap_or(1).clamp(1, 5) + 1);
            let _ = writeln!(out, "{marks} {}", text.trim());
        }
        Block::HorizontalRule => out.push_str("'''\n"),
        Block::Table(table) => table_to(table, out),
        Block::Figure(_, caption, inner) => {
            if !caption.blocks.is_empty() {
                let mut text = String::new();
                blocks(&caption.blocks, &mut text, depth);
                let _ = writeln!(out, ".{}", text.trim().replace('\n', " "));
            }
            blocks(inner, out, depth);
        }
        Block::Div(_, inner) => blocks(inner, out, depth),
        Block::RawBlock(format, text) => {
            if format.0 == "asciidoc" {
                out.push_str(text);
                out.push('\n');
            }
        }
    }
}

/// One list item, with any further blocks attached by a `+` continuation.
fn item_to(item: &[Block], marker: &str, out: &mut String, depth: usize) {
    let (first, rest) = item.split_first().unwrap_or((&Block::HorizontalRule, &[]));
    let mut text = String::new();
    match first {
        Block::Plain(list) | Block::Para(list) => inlines(list, &mut text),
        other => block_to(other, &mut text, depth),
    }
    let _ = writeln!(out, "{marker} {}", text.trim());
    for block in rest {
        let mut body = String::new();
        // A nested list is one level deeper; anything else is attached to
        // the item with a `+` line, which is how AsciiDoc keeps a second
        // paragraph inside an item.
        match block {
            Block::BulletList(_) | Block::OrderedList(..) => {
                block_to(block, &mut body, depth + 1);
                out.push_str(body.trim_end());
                out.push('\n');
            }
            other => {
                block_to(other, &mut body, depth);
                let _ = writeln!(out, "+\n{}", body.trim_end());
            }
        }
    }
}

/// A delimiter run longer than any run of the same character inside the
/// content.
///
/// Four is the minimum `AsciiDoc` accepts. A listing containing `----` and
/// fenced with `----` ends where the sample does, and the rest of the
/// document silently becomes prose.
fn fence_for(content: &str, ch: char) -> String {
    let longest = content
        .lines()
        .filter(|line| line.chars().all(|c| c == ch) && !line.is_empty())
        .map(|line| line.chars().count())
        .max()
        .unwrap_or(0);
    ch.to_string().repeat(longest.max(3) + 1)
}

/// The list-style attribute a numbering calls for, or `None` for the
/// arabic default.
fn number_style(style: ListNumberStyle) -> Option<&'static str> {
    match style {
        ListNumberStyle::LowerAlpha => Some("loweralpha"),
        ListNumberStyle::UpperAlpha => Some("upperalpha"),
        ListNumberStyle::LowerRoman => Some("lowerroman"),
        ListNumberStyle::UpperRoman => Some("upperroman"),
        _ => None,
    }
}

fn table_to(table: &Table, out: &mut String) {
    let columns = table.colspecs.len().max(1);
    if !table.caption.blocks.is_empty() {
        let mut text = String::new();
        blocks(&table.caption.blocks, &mut text, 0);
        let _ = writeln!(out, ".{}", text.trim().replace('\n', " "));
    }
    let _ = writeln!(out, "[cols=\"{}\"]", vec!["1"; columns].join(","));
    if !table.head.rows.is_empty() {
        out.push_str("[options=\"header\"]\n");
    }
    out.push_str("|===\n");
    for row in table
        .head
        .rows
        .iter()
        .chain(table.bodies.iter().flat_map(|b| b.head.iter().chain(&b.body)))
        .chain(table.foot.rows.iter())
    {
        for cell in &row.cells {
            let _ = write!(out, "|{} ", cell_text(cell));
        }
        out.push('\n');
    }
    out.push_str("|===\n");
}

fn cell_text(cell: &Cell) -> String {
    let mut out = String::new();
    for block in &cell.blocks {
        match block {
            Block::Plain(list) | Block::Para(list) => inlines(list, &mut out),
            other => block_to(other, &mut out, 0),
        }
    }
    // A newline inside a cell starts a new one; the row would come apart.
    out.replace('\n', " ").trim().to_owned()
}

fn inlines(list: &[Inline], out: &mut String) {
    for inline in list {
        inline_to(inline, out);
    }
}

fn inline_to(inline: &Inline, out: &mut String) {
    let wrap = |marker: &str, inner: &[Inline], out: &mut String| {
        let mut text = String::new();
        inlines(inner, &mut text);
        if text.trim().is_empty() {
            out.push_str(&text);
            return;
        }
        let _ = write!(out, "{marker}{}{marker}", text.trim());
    };
    match inline {
        Inline::Str(text) => out.push_str(&escape(text)),
        Inline::Space => out.push(' '),
        Inline::SoftBreak => out.push('\n'),
        // A trailing `+` is the hard break.
        Inline::LineBreak => out.push_str(" +\n"),
        // The markers are the opposite way round from markdown: `_` is
        // italic and `*` is bold.
        Inline::Emph(inner) => wrap("_", inner, out),
        Inline::Strong(inner) => wrap("*", inner, out),
        Inline::Underline(inner) => wrap("[.underline]#", inner, out),
        Inline::Strikeout(inner) => wrap("[.line-through]#", inner, out),
        Inline::SmallCaps(inner) => wrap("[.smallcaps]#", inner, out),
        Inline::Superscript(inner) => wrap("^", inner, out),
        Inline::Subscript(inner) => wrap("~", inner, out),
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
        Inline::Code(_, code) => {
            let _ = write!(out, "`{code}`");
        }
        Inline::Math(_, math) => {
            let _ = write!(out, "latexmath:[{math}]");
        }
        Inline::RawInline(format, text) => {
            if format.0 == "asciidoc" {
                out.push_str(text);
            }
        }
        Inline::Link(_, inner, target) => {
            let mut text = String::new();
            inlines(inner, &mut text);
            // An internal reference is `<<id,text>>`; everything else is
            // `url[text]`.
            match target.url.strip_prefix('#') {
                Some(id) => {
                    let _ = write!(out, "<<{id},{}>>", text.trim());
                }
                None => {
                    let _ = write!(out, "{}[{}]", target.url, text.trim());
                }
            }
        }
        Inline::Image(_, alt, target) => {
            let mut text = String::new();
            inlines(alt, &mut text);
            let _ = write!(out, "image:{}[{}]", target.url, text.trim());
        }
        Inline::Note(blocks_in_note) => {
            let mut text = String::new();
            blocks(blocks_in_note, &mut text, 0);
            let _ = write!(out, "footnote:[{}]", text.trim().replace('\n', " "));
        }
    }
}

/// Escape the characters `AsciiDoc` gives an inline meaning to.
///
/// A backslash before the character is its own escape, and it is
/// applied only to the markers that could start a construct — escaping
/// more would fill ordinary prose with backslashes for no benefit.
fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        if matches!(ch, '*' | '_' | '`' | '#' | '^' | '~' | '\\') {
            out.push('\\');
        }
        out.push(ch);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use ferrodoc_ast::{Attr, ListAttributes, ListNumberDelim, Target};

    fn doc(blocks: Vec<Block>) -> Pandoc {
        Pandoc::new(blocks)
    }

    #[test]
    fn emphasis_markers_are_the_opposite_way_round_from_markdown() {
        // `_x_` is italic and `*x*` is bold here. Getting it backwards
        // produces a document that looks almost right, which is why it is
        // worth a test of its own.
        let rendered = write_asciidoc(&doc(vec![Block::Para(vec![
            Inline::Emph(vec![Inline::Str("i".into())]),
            Inline::Space,
            Inline::Strong(vec![Inline::Str("b".into())]),
        ])]));
        assert!(rendered.contains("_i_"), "{rendered}");
        assert!(rendered.contains("*b*"), "{rendered}");
    }

    #[test]
    fn a_fence_is_longer_than_any_run_inside_it() {
        // A listing containing `----` fenced with `----` ends where the
        // sample does, and the rest of the document silently becomes
        // prose. This is the failure the check exists for.
        let code = "before\n----\nafter";
        let rendered = write_asciidoc(&doc(vec![Block::CodeBlock(Attr::default(), code.into())]));
        let fence = rendered
            .lines()
            .find(|line| line.starts_with("----"))
            .expect("a fence");
        assert!(fence.len() > 4, "the fence does not clear the content: {rendered}");
        // The fence appears exactly twice; the run inside is shorter.
        let fences = rendered.lines().filter(|line| *line == fence).count();
        assert_eq!(fences, 2, "{rendered}");
    }

    #[test]
    fn headings_start_at_two_equals_signs() {
        // A single `=` is the document title and may appear only once, so
        // a document with two level-1 headings would be invalid.
        let rendered = write_asciidoc(&doc(vec![
            Block::Header(1, Attr::default(), vec![Inline::Str("A".into())]),
            Block::Header(1, Attr::default(), vec![Inline::Str("B".into())]),
            Block::Header(2, Attr::default(), vec![Inline::Str("C".into())]),
        ]));
        assert!(rendered.contains("== A"), "{rendered}");
        assert!(rendered.contains("== B"), "{rendered}");
        assert!(rendered.contains("=== C"), "{rendered}");
        assert!(!rendered.lines().any(|l| l.starts_with("= ")), "{rendered}");
    }

    #[test]
    fn a_nested_list_deepens_its_marker() {
        // Depth is the marker's length in AsciiDoc, not indentation.
        let rendered = write_asciidoc(&doc(vec![Block::BulletList(vec![vec![
            Block::Plain(vec![Inline::Str("outer".into())]),
            Block::BulletList(vec![vec![Block::Plain(vec![Inline::Str("inner".into())])]]),
        ]])]));
        assert!(rendered.contains("* outer"), "{rendered}");
        assert!(rendered.contains("** inner"), "{rendered}");
    }

    #[test]
    fn a_second_paragraph_is_attached_with_a_continuation() {
        let rendered = write_asciidoc(&doc(vec![Block::BulletList(vec![vec![
            Block::Para(vec![Inline::Str("one".into())]),
            Block::Para(vec![Inline::Str("two".into())]),
        ]])]));
        assert!(rendered.contains("* one"), "{rendered}");
        assert!(rendered.contains("+\ntwo"), "the second paragraph escaped the item: {rendered}");
    }

    #[test]
    fn an_internal_link_is_a_cross_reference() {
        let link = |url: &str| {
            write_asciidoc(&doc(vec![Block::Para(vec![Inline::Link(
                Box::default(),
                vec![Inline::Str("text".into())],
                Box::new(Target { url: url.into(), title: String::new() }),
            )])]))
        };
        assert!(link("#target").contains("<<target,text>>"), "{}", link("#target"));
        assert!(link("http://x").contains("http://x[text]"), "{}", link("http://x"));
    }

    #[test]
    fn a_list_states_a_start_value_and_a_numbering_style() {
        let rendered = write_asciidoc(&doc(vec![Block::OrderedList(
            ListAttributes {
                start: 3,
                style: ListNumberStyle::UpperRoman,
                delim: ListNumberDelim::Period,
            },
            vec![vec![Block::Plain(vec![Inline::Str("a".into())])]],
        )]));
        assert!(rendered.contains("[start=3]"), "{rendered}");
        assert!(rendered.contains("[upperroman]"), "{rendered}");
    }

    #[test]
    fn a_table_cell_never_contains_a_newline() {
        // A newline inside a cell starts a new one and the row comes
        // apart, taking the rest of the table with it.
        let cell = ferrodoc_ast::Cell {
            attr: Attr::default(),
            alignment: ferrodoc_ast::Alignment::AlignDefault,
            row_span: 1,
            col_span: 1,
            blocks: vec![
                Block::Para(vec![Inline::Str("one".into())]),
                Block::Para(vec![Inline::Str("two".into())]),
            ],
        };
        assert!(!cell_text(&cell).contains('\n'), "{:?}", cell_text(&cell));
    }
}
