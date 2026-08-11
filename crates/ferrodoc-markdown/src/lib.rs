//! Markdown reader producing the ferrodoc (pandoc-compatible) AST.
//!
//! [`read_commonmark`] parses pure `CommonMark` (no extensions) with comrak
//! and maps the result to the same AST `pandoc -f commonmark -t json`
//! produces, down to pandoc's inline tokenization: words become `Str`,
//! runs of spaces collapse to a single `Space`, line endings inside a
//! paragraph become `SoftBreak`.
//!
//! Pandoc's commonmark reader (commonmark-hs) has a few observable
//! tokenizer-level behaviors this reader reproduces deliberately:
//! tabs are expanded to 4-column tab stops everywhere (even inside code),
//! a fenced code block keeps its trailing newline only when the fence is
//! never closed, and an unclosed type-1–5 HTML block that runs to the end
//! of the document gains one extra trailing newline.

use comrak::nodes::{AstNode, ListDelimType, ListType, NodeValue};
use comrak::{Arena, Options, parse_document};
use ferrodoc_ast::{
    Attr, Block, Format, Inline, ListAttributes, ListNumberDelim, ListNumberStyle, Pandoc,
    Target,
};
use std::borrow::Cow;

/// Parse a `CommonMark` document into a [`Pandoc`] AST equivalent to
/// pandoc's commonmark reader output.
pub fn read_commonmark(input: &str) -> Pandoc {
    let expanded = expand_tabs(input);
    let src = Source { lines: expanded.split('\n').collect() };
    let arena = Arena::new();
    let root = parse_document(&arena, &expanded, &Options::default());
    Pandoc::new(blocks(root.children(), &src))
}

/// The tab-expanded source, split into lines for fence/EOF lookups.
struct Source<'s> {
    lines: Vec<&'s str>,
}

impl Source<'_> {
    /// 1-based line lookup, as used by comrak's sourcepos.
    fn line(&self, number: usize) -> &str {
        self.lines.get(number.wrapping_sub(1)).copied().unwrap_or("")
    }

    /// Whether the 1-based line number is at (or past) the last source line.
    fn is_last_line(&self, number: usize) -> bool {
        let mut count = self.lines.len();
        if self.lines.last() == Some(&"") {
            count -= 1; // a trailing newline produces one empty trailing piece
        }
        number >= count
    }
}

/// Expand tabs to 4-column tab stops, counting one column per `char`,
/// exactly like pandoc's tokenizer.
fn expand_tabs(input: &str) -> Cow<'_, str> {
    if !input.contains('\t') {
        return Cow::Borrowed(input);
    }
    let mut out = String::with_capacity(input.len());
    let mut col = 0usize;
    for ch in input.chars() {
        match ch {
            '\n' => {
                out.push('\n');
                col = 0;
            }
            '\t' => {
                let n = 4 - col % 4;
                out.extend(std::iter::repeat_n(' ', n));
                col += n;
            }
            ch => {
                out.push(ch);
                col += 1;
            }
        }
    }
    Cow::Owned(out)
}

fn blocks<'a>(nodes: impl Iterator<Item = &'a AstNode<'a>>, src: &Source) -> Vec<Block> {
    nodes.filter_map(|n| block(n, src)).collect()
}

fn block<'a>(node: &'a AstNode<'a>, src: &Source) -> Option<Block> {
    let data = node.data.borrow();
    match &data.value {
        NodeValue::Paragraph => Some(Block::Para(inlines(node.children()))),
        NodeValue::Heading(h) => Some(Block::Header(
            i64::from(h.level),
            Attr::default(),
            inlines(node.children()),
        )),
        NodeValue::BlockQuote => Some(Block::BlockQuote(blocks(node.children(), src))),
        NodeValue::CodeBlock(cb) => {
            // Pandoc keeps the trailing newline only for a fenced block whose
            // closing fence never appears AND that runs to the end of the
            // document; unclosed fences at a container boundary are stripped
            // like closed ones.
            let unclosed_at_eof = cb.fenced
                && !is_closing_fence(
                    src.line(data.sourcepos.end.line),
                    char::from(cb.fence_char),
                    cb.fence_length,
                )
                && src.is_last_line(data.sourcepos.end.line);
            let text = if unclosed_at_eof {
                &cb.literal
            } else {
                cb.literal.strip_suffix('\n').unwrap_or(&cb.literal)
            };
            let classes = cb
                .info
                .split_whitespace()
                .next()
                .map(|lang| vec![lang.to_owned()])
                .unwrap_or_default();
            Some(Block::CodeBlock(
                Attr { classes, ..Attr::default() },
                text.to_owned(),
            ))
        }
        NodeValue::HtmlBlock(hb) => {
            let mut literal = hb.literal.clone();
            // An unclosed type-1..5 HTML block that runs to the end of the
            // document gains one extra newline in pandoc's output.
            if (1..=5).contains(&hb.block_type)
                && !contains_closer(&literal, hb.block_type)
                && src.is_last_line(data.sourcepos.end.line)
            {
                literal.push('\n');
            }
            Some(Block::RawBlock(Format("html".to_owned()), literal))
        }
        NodeValue::ThematicBreak => Some(Block::HorizontalRule),
        NodeValue::List(nl) => {
            let items: Vec<Vec<Block>> = node
                .children()
                .map(|item| {
                    let mut bs = blocks(item.children(), src);
                    if nl.tight {
                        for b in &mut bs {
                            if let Block::Para(is) = b {
                                *b = Block::Plain(std::mem::take(is));
                            }
                        }
                    }
                    bs
                })
                .collect();
            Some(match nl.list_type {
                ListType::Bullet => Block::BulletList(items),
                ListType::Ordered => Block::OrderedList(
                    ListAttributes {
                        start: i64::try_from(nl.start).unwrap_or(i64::MAX),
                        style: ListNumberStyle::Decimal,
                        delim: match nl.delimiter {
                            ListDelimType::Period => ListNumberDelim::Period,
                            ListDelimType::Paren => ListNumberDelim::OneParen,
                        },
                    },
                    items,
                ),
            })
        }
        // Only core-CommonMark nodes occur with default comrak options; the
        // differential harness would surface anything dropped here.
        _ => None,
    }
}

/// Whether `line` is a valid closing fence for a fence of `ch` repeated at
/// least `min` times: optional blockquote markers, at most 3 spaces of
/// indent, the fence characters, then only whitespace.
fn is_closing_fence(line: &str, ch: char, min: usize) -> bool {
    let mut s = line;
    loop {
        let trimmed = s.trim_start_matches(' ');
        if s.len() - trimmed.len() <= 3
            && let Some(rest) = trimmed.strip_prefix('>')
        {
            s = rest.strip_prefix(' ').unwrap_or(rest);
        } else {
            break;
        }
    }
    let trimmed = s.trim_start_matches(' ');
    if s.len() - trimmed.len() > 3 {
        return false;
    }
    let n = trimmed.chars().take_while(|&c| c == ch).count();
    n >= min && trimmed.chars().skip(n).all(|c| c == ' ' || c == '\t')
}

/// Whether a type-1..5 HTML block's literal contains its closing marker.
fn contains_closer(literal: &str, block_type: u8) -> bool {
    match block_type {
        1 => {
            let lower = literal.to_lowercase();
            ["</style>", "</script>", "</pre>", "</textarea>"]
                .iter()
                .any(|c| lower.contains(c))
        }
        2 => literal.contains("-->"),
        3 => literal.contains("?>"),
        4 => literal.contains('>'),
        5 => literal.contains("]]>"),
        _ => true,
    }
}

fn inlines<'a>(nodes: impl Iterator<Item = &'a AstNode<'a>>) -> Vec<Inline> {
    let mut out = Vec::new();
    for node in nodes {
        inline(node, &mut out);
    }
    coalesce(out)
}

fn inline<'a>(node: &'a AstNode<'a>, out: &mut Vec<Inline>) {
    match &node.data.borrow().value {
        NodeValue::Text(t) => text_tokens(t, out),
        NodeValue::SoftBreak => out.push(Inline::SoftBreak),
        NodeValue::LineBreak => out.push(Inline::LineBreak),
        NodeValue::Code(c) => out.push(Inline::Code(Attr::default(), c.literal.clone())),
        NodeValue::HtmlInline(h) => {
            out.push(Inline::RawInline(Format("html".to_owned()), h.clone()));
        }
        NodeValue::Emph => out.push(Inline::Emph(inlines(node.children()))),
        NodeValue::Strong => out.push(Inline::Strong(inlines(node.children()))),
        NodeValue::Link(l) => out.push(Inline::Link(
            Attr::default(),
            inlines(node.children()),
            Target { url: l.url.clone(), title: l.title.clone() },
        )),
        NodeValue::Image(l) => out.push(Inline::Image(
            Attr::default(),
            inlines(node.children()),
            Target { url: l.url.clone(), title: l.title.clone() },
        )),
        _ => {}
    }
}

/// Tokenize literal text the way pandoc does: words become [`Inline::Str`],
/// runs of ASCII spaces become a single [`Inline::Space`]. Non-breaking
/// spaces and other Unicode whitespace stay inside `Str`.
fn text_tokens(text: &str, out: &mut Vec<Inline>) {
    let mut word = String::new();
    for ch in text.chars() {
        if ch == ' ' {
            if !word.is_empty() {
                out.push(Inline::Str(std::mem::take(&mut word)));
            }
            out.push(Inline::Space);
        } else {
            word.push(ch);
        }
    }
    if !word.is_empty() {
        out.push(Inline::Str(word));
    }
}

/// Merge adjacent `Str` tokens (comrak may split text at entity boundaries)
/// and collapse repeated `Space` tokens, matching pandoc's tokenization.
fn coalesce(tokens: Vec<Inline>) -> Vec<Inline> {
    let mut out: Vec<Inline> = Vec::with_capacity(tokens.len());
    for token in tokens {
        match (out.last_mut(), token) {
            (Some(Inline::Str(prev)), Inline::Str(next)) => prev.push_str(&next),
            (Some(Inline::Space), Inline::Space) => {}
            (_, token) => out.push(token),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc(md: &str) -> serde_json::Value {
        serde_json::to_value(read_commonmark(md)).unwrap()["blocks"].clone()
    }

    #[test]
    fn tight_list_items_become_plain() {
        assert_eq!(
            doc("- a\n- b\n"),
            serde_json::json!([{"t": "BulletList", "c": [
                [{"t": "Plain", "c": [{"t": "Str", "c": "a"}]}],
                [{"t": "Plain", "c": [{"t": "Str", "c": "b"}]}]
            ]}])
        );
    }

    #[test]
    fn tabs_expand_to_four_column_stops() {
        // "foo\tbaz\t\tbim" as indented code: pandoc expands tabs.
        assert_eq!(
            doc("\tfoo\tbaz\t\tbim\n"),
            serde_json::json!([{"t": "CodeBlock", "c": [["", [], []], "foo baz     bim"]}])
        );
    }

    #[test]
    fn closed_fence_strips_trailing_newline_unclosed_keeps_it() {
        assert_eq!(
            doc("```\naaa\n```\n"),
            serde_json::json!([{"t": "CodeBlock", "c": [["", [], []], "aaa"]}])
        );
        assert_eq!(
            doc("```\naaa\n"),
            serde_json::json!([{"t": "CodeBlock", "c": [["", [], []], "aaa\n"]}])
        );
    }

    #[test]
    fn spaces_collapse_into_single_space_tokens() {
        assert_eq!(
            doc("a  b\n"),
            serde_json::json!([{"t": "Para", "c": [
                {"t": "Str", "c": "a"}, {"t": "Space"}, {"t": "Str", "c": "b"}
            ]}])
        );
    }
}
