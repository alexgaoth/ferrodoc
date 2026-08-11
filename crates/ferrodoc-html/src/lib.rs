//! HTML writer for the ferrodoc (pandoc-compatible) AST.
//!
//! [`write_html`] emits the same HTML as
//! `pandoc -f commonmark -t html --syntax-highlighting=none --wrap=none`
//! for every construct reachable from the commonmark reader (verified
//! differentially by `ferrodoc-harness diff-html`). Constructs the
//! commonmark reader cannot produce (tables, figures, notes, …) get
//! reasonable pandoc-shaped output but are not differentially verified;
//! `Note` and non-HTML raw content are dropped, like pandoc's HTML writer
//! does for raw content it cannot place.

use ferrodoc_ast::{
    Alignment, Attr, Block, Caption, Cell, Inline, ListNumberStyle, Pandoc, Row, Table,
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
            write_attr(out, attr);
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
            out.push_str("<ul>\n");
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
        write_blocks_joined(out, item);
        out.push_str("</li>\n");
    }
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
    out.push('>');
    if !table.caption.blocks.is_empty() {
        out.push_str("\n<caption>");
        write_blocks_joined(out, &table.caption.blocks);
        out.push_str("</caption>");
    }
    if !table.head.rows.is_empty() {
        out.push_str("\n<thead>");
        for row in &table.head.rows {
            write_table_row(out, row, "th");
        }
        out.push_str("\n</thead>");
    }
    for body in &table.bodies {
        out.push_str("\n<tbody>");
        for row in body.head.iter().chain(&body.body) {
            write_table_row(out, row, "td");
        }
        out.push_str("\n</tbody>");
    }
    if !table.foot.rows.is_empty() {
        out.push_str("\n<tfoot>");
        for row in &table.foot.rows {
            write_table_row(out, row, "td");
        }
        out.push_str("\n</tfoot>");
    }
    out.push_str("\n</table>");
}

fn write_table_row(out: &mut String, row: &Row, cell_tag: &str) {
    out.push_str("\n<tr>");
    for cell in &row.cells {
        write_table_cell(out, cell, cell_tag);
    }
    out.push_str("\n</tr>");
}

fn write_table_cell(out: &mut String, cell: &Cell, tag: &str) {
    let _ = write!(out, "\n<{tag}");
    if cell.row_span != 1 {
        let _ = write!(out, " rowspan=\"{}\"", cell.row_span);
    }
    if cell.col_span != 1 {
        let _ = write!(out, " colspan=\"{}\"", cell.col_span);
    }
    if let Some(align) = alignment_style(cell.alignment) {
        let _ = write!(out, " style=\"text-align: {align};\"");
    }
    out.push('>');
    write_blocks_joined(out, &cell.blocks);
    let _ = write!(out, "</{tag}>");
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

/// Render attributes as ` id=".." class=".." k="v"`, pandoc's order.
fn write_attr(out: &mut String, attr: &Attr) {
    if !attr.identifier.is_empty() {
        out.push_str(" id=\"");
        escape_attribute(out, &attr.identifier);
        out.push('"');
    }
    if !attr.classes.is_empty() {
        out.push_str(" class=\"");
        escape_attribute(out, &attr.classes.join(" "));
        out.push('"');
    }
    for (key, value) in &attr.attributes {
        out.push(' ');
        // Keys come from the same untrusted AST as values; drop characters
        // that could break out of the tag.
        out.extend(key.chars().filter(|c| !c.is_whitespace() && !"\"'<>=/&".contains(*c)));
        out.push_str("=\"");
        escape_attribute(out, value);
        out.push('"');
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
            Inline::Space | Inline::SoftBreak => out.push(' '),
            Inline::LineBreak => out.push('\n'),
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
    use super::*;

    fn html(md: &str) -> String {
        write_html(&ferrodoc_markdown::read_commonmark(md))
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

    #[test]
    fn code_block_with_language() {
        assert_eq!(
            html("```rust\nfn x() {}\n```\n"),
            "<pre class=\"rust\"><code>fn x() {}</code></pre>\n"
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
