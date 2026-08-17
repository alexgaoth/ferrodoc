//! Plain-text writer for the ferrodoc (pandoc-compatible) AST.
//!
//! [`write_text`] is a best-effort text extraction for search indexing and
//! LLM ingestion: inline formatting is dropped, block structure becomes
//! blank-line-separated paragraphs, list items get simple markers, and raw
//! content (HTML, TeX) is omitted entirely. It deliberately does NOT
//! replicate `pandoc -t plain` byte-for-byte.

use ferrodoc_ast::{Block, Inline, Pandoc};

/// Render a document as plain text.
pub fn write_text(doc: &Pandoc) -> String {
    let mut paragraphs = Vec::new();
    blocks_to_paragraphs(&doc.blocks, &mut paragraphs, "");
    let mut out = paragraphs.join("\n\n");
    if !out.is_empty() {
        out.push('\n');
    }
    out
}

fn blocks_to_paragraphs(blocks: &[Block], out: &mut Vec<String>, prefix: &str) {
    for block in blocks {
        match block {
            Block::Plain(inlines) | Block::Para(inlines) | Block::Header(_, _, inlines) => {
                out.push(format!("{prefix}{}", inlines_text(inlines)));
            }
            Block::CodeBlock(_, text) => out.push(format!("{prefix}{text}")),
            Block::BlockQuote(inner) | Block::Div(_, inner) => {
                blocks_to_paragraphs(inner, out, prefix);
            }
            Block::Figure(_, caption, inner) => {
                blocks_to_paragraphs(inner, out, prefix);
                blocks_to_paragraphs(&caption.blocks, out, prefix);
            }
            Block::BulletList(items) => list_items(items, out, prefix, |_| "- ".to_owned()),
            Block::OrderedList(attrs, items) => {
                let start = attrs.start;
                list_items(items, out, prefix, |i| {
                    format!("{}. ", start + i64::try_from(i).unwrap_or(0))
                });
            }
            Block::DefinitionList(items) => {
                for (term, definitions) in items {
                    out.push(format!("{prefix}{}", inlines_text(term)));
                    for definition in definitions {
                        blocks_to_paragraphs(definition, out, prefix);
                    }
                }
            }
            Block::LineBlock(lines) => {
                let text: Vec<String> = lines.iter().map(|l| inlines_text(l)).collect();
                out.push(format!("{prefix}{}", text.join("\n")));
            }
            Block::Table(table) => {
                for body in
                    std::iter::once(&table.head.rows).chain(body_rows(table)).chain(
                        std::iter::once(&table.foot.rows),
                    )
                {
                    for row in body {
                        let cells: Vec<String> = row
                            .cells
                            .iter()
                            .map(|c| {
                                let mut ps = Vec::new();
                                blocks_to_paragraphs(&c.blocks, &mut ps, "");
                                ps.join(" ")
                            })
                            .collect();
                        out.push(format!("{prefix}{}", cells.join("\t")));
                    }
                }
                blocks_to_paragraphs(&table.caption.blocks, out, prefix);
            }
            Block::RawBlock(..) | Block::HorizontalRule => {}
        }
    }
}

/// The bodies of a table as row slices (intermediate heads, then rows).
fn body_rows(table: &ferrodoc_ast::Table) -> impl Iterator<Item = &Vec<ferrodoc_ast::Row>> {
    table.bodies.iter().flat_map(|b| [&b.head, &b.body])
}

fn list_items(
    items: &[Vec<Block>],
    out: &mut Vec<String>,
    prefix: &str,
    marker: impl Fn(usize) -> String,
) {
    for (i, item) in items.iter().enumerate() {
        let mut inner = Vec::new();
        blocks_to_paragraphs(item, &mut inner, "");
        out.push(format!("{prefix}{}{}", marker(i), inner.join("\n")));
    }
}

fn inlines_text(inlines: &[Inline]) -> String {
    let mut out = String::new();
    collect(&mut out, inlines);
    out
}

fn collect(out: &mut String, inlines: &[Inline]) {
    for inline in inlines {
        match inline {
            Inline::Str(s) | Inline::Code(_, s) | Inline::Math(_, s) => out.push_str(s),
            Inline::Space | Inline::SoftBreak => out.push(' '),
            Inline::LineBreak => out.push('\n'),
            Inline::Emph(i) | Inline::Strong(i) | Inline::Strikeout(i)
            | Inline::Superscript(i) | Inline::Subscript(i) | Inline::SmallCaps(i)
            | Inline::Underline(i) | Inline::Quoted(_, i) | Inline::Cite(_, i)
            | Inline::Span(_, i) | Inline::Link(_, i, _) | Inline::Image(_, i, _) => {
                collect(out, i);
            }
            Inline::Note(blocks) => {
                let mut ps = Vec::new();
                blocks_to_paragraphs(blocks, &mut ps, "");
                out.push_str(&ps.join(" "));
            }
            Inline::RawInline(..) => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ferrodoc_ast::{Attr, Target};

    #[test]
    fn formatting_is_dropped_and_blocks_are_separated() {
        let doc = Pandoc::new(vec![
            Block::Header(1, Attr::default(), vec![Inline::Str("Title".into())]),
            Block::Para(vec![
                Inline::Emph(vec![Inline::Str("em".into())]),
                Inline::Space,
                Inline::Link(
                    Box::default(),
                    vec![Inline::Str("link".into())],
                    Box::new(Target { url: "u".into(), title: String::new() }),
                ),
            ]),
            Block::BulletList(vec![
                vec![Block::Plain(vec![Inline::Str("a".into())])],
                vec![Block::Plain(vec![Inline::Str("b".into())])],
            ]),
        ]);
        assert_eq!(write_text(&doc), "Title\n\nem link\n\n- a\n\n- b\n");
    }
}
