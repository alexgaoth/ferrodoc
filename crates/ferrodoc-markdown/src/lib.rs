//! Markdown reader producing the ferrodoc (pandoc-compatible) AST.
//!
//! [`read_commonmark`] parses pure `CommonMark` (no extensions) with comrak
//! and maps the result to the same AST `pandoc -f commonmark -t json`
//! produces, down to pandoc's inline tokenization: words become `Str`,
//! runs of spaces collapse to a single `Space`, line endings inside a
//! paragraph become `SoftBreak`.
//!
//! Pandoc's commonmark reader (commonmark-hs) has observable tokenizer-level
//! behaviors this reader reproduces deliberately:
//!
//! - tabs are expanded to 4-column tab stops everywhere (even inside code),
//!   and input is treated as if it ended with a newline;
//! - a fenced code block whose closing fence never appears keeps its
//!   literal untouched when it sits outside any blockquote and only blank
//!   lines separate it from EOF; every other fence (closed, quote-nested,
//!   or truncated by later content) loses exactly one trailing newline;
//! - an unclosed type-1–5 raw HTML block outside blockquotes absorbs the
//!   blank lines that follow it, plus one bonus newline when only blank
//!   lines separate it from EOF.
//!
//! Where comrak's tree differs structurally from pandoc's, the mapper
//! normalizes: carriage returns are removed before parsing (pandoc's
//! `crFilter`), directly-adjacent same-type `Emph`/`Strong` siblings are
//! merged (`_a_*b*` is one `Emph` in pandoc, two in comrak), and a
//! paragraph consisting of link reference definitions plus a dash-run
//! underline becomes the `HorizontalRule` pandoc produces (comrak emits a
//! literal `---` paragraph there).

mod write;

pub use write::write_markdown;

use comrak::nodes::{AstNode, ListDelimType, ListType, NodeValue};
use comrak::{Arena, Options, parse_document};
use ferrodoc_ast::{
    Attr, Block, Format, Inline, ListAttributes, ListNumberDelim, ListNumberStyle, Pandoc,
    Target,
};
use std::borrow::Cow;

/// A document this reader will not convert.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// Containers nested deeper than [`MAX_NESTING`]. Converting such a
    /// document would recurse until the stack overflows, which aborts the
    /// process, so it is refused instead — and refused loudly, rather than
    /// returned truncated.
    TooDeeplyNested,
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::TooDeeplyNested => write!(
                f,
                "document nests containers {MAX_NESTING} or more levels deep"
            ),
        }
    }
}

impl std::error::Error for Error {}

/// Parse a `CommonMark` document into a [`Pandoc`] AST equivalent to
/// pandoc's commonmark reader output.
///
/// # Errors
///
/// Returns [`Error::TooDeeplyNested`] for pathologically nested input.
pub fn read_commonmark(input: &str) -> Result<Pandoc, Error> {
    let prepared = preprocess(input);
    let src = Src::new(&prepared);
    let arena = Arena::new();
    let root = parse_document(&arena, &prepared, &Options::default());
    // Check the depth once, without recursing, and leave the conversion
    // itself exactly as shallow-document-shaped as it was: threading a
    // depth counter through every call measured ~8% slower.
    if tree_depth(root) > MAX_NESTING {
        return Err(Error::TooDeeplyNested);
    }
    Ok(Pandoc::new(blocks(root.children(), &src, false)))
}

/// The deepest nesting in a parsed tree, computed iteratively so that
/// measuring the depth cannot itself overflow the stack.
fn tree_depth<'a>(root: &'a AstNode<'a>) -> usize {
    let mut deepest = 0;
    let mut stack = vec![(root, 0usize)];
    while let Some((node, depth)) = stack.pop() {
        deepest = deepest.max(depth);
        for child in node.children() {
            stack.push((child, depth + 1));
        }
    }
    deepest
}

/// The deepest nesting converted. Real documents nest a handful
/// of levels, so this is hundreds of times more than anything genuine —
/// but it must also stay safe on the smallest stack the reader might run
/// on. A conversion frame costs roughly a kilobyte, and threads commonly
/// get 2 MiB (Rust's test threads do) — and an unoptimized build's
/// frames are several times larger again, so the bound is set well inside
/// that. Exceeding it is reported, never silently truncated.
pub const MAX_NESTING: usize = 200;

/// The preprocessed source lines, for looking at what follows a node.
struct Src<'s> {
    lines: Vec<&'s str>,
}

impl Src<'_> {
    fn new(text: &str) -> Src<'_> {
        let mut lines: Vec<&str> = text.split('\n').collect();
        if lines.last() == Some(&"") {
            lines.pop(); // a trailing newline produces one phantom piece
        }
        Src { lines }
    }

    /// Whether every line after the 1-based line number `after` is blank
    /// (i.e. only blank lines separate it from EOF).
    fn only_blanks_after(&self, after: usize) -> bool {
        self.lines.iter().skip(after).all(|l| l.trim().is_empty())
    }

    /// 1-based line lookup, as used by comrak's sourcepos.
    fn line(&self, number: usize) -> &str {
        self.lines.get(number.wrapping_sub(1)).copied().unwrap_or("")
    }
}

/// The first character of a line after skipping blockquote markers and
/// spaces — the character that decides what block the line starts.
fn first_nonmarker_char(line: &str) -> Option<char> {
    line.chars().find(|&c| c != '>' && c != ' ')
}

/// Number of lines in a newline-terminated literal (its trailing newline
/// does not start a new line).
fn literal_lines(literal: &str) -> usize {
    literal.split('\n').count() - usize::from(literal.ends_with('\n') || literal.is_empty())
}

/// Expand tabs to 4-column tab stops (counting one column per `char`),
/// remove carriage returns, and guarantee a trailing newline, exactly like
/// pandoc's tokenizer (`crFilter` + tab expansion).
fn preprocess(input: &str) -> Cow<'_, str> {
    if !input.contains('\t') && !input.contains('\r') && input.ends_with('\n') {
        return Cow::Borrowed(input);
    }
    let mut out = String::with_capacity(input.len() + 1);
    let mut col = 0usize;
    for ch in input.chars() {
        match ch {
            '\r' => {}
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
    if !out.ends_with('\n') {
        out.push('\n');
    }
    Cow::Owned(out)
}

/// Map block-level nodes. `in_quote` is true when any ancestor is a
/// blockquote — pandoc's trailing-newline rules differ there (see crate
/// docs).
fn blocks<'a>(
    nodes: impl Iterator<Item = &'a AstNode<'a>>,
    src: &Src,
    in_quote: bool,
) -> Vec<Block> {
    nodes.filter_map(|n| block(n, src, in_quote)).collect()
}

fn block<'a>(node: &'a AstNode<'a>, src: &Src, in_quote: bool) -> Option<Block> {
    let data = node.data.borrow();
    match &data.value {
        NodeValue::Paragraph => {
            let content = inlines(node.children());
            // Comrak quirk: a paragraph of link reference definitions whose
            // last line is a dash-run comes out as a literal `---` paragraph;
            // pandoc consumes the definitions and reads the dashes as a
            // thematic break. Detect via the paragraph's first source line
            // (a reference definition starts with `[`), which also excludes
            // lookalikes such as escaped or entity-encoded dashes.
            if let [Inline::Str(s)] = content.as_slice()
                && s.len() >= 3
                && s.bytes().all(|b| b == b'-')
                && first_nonmarker_char(src.line(data.sourcepos.start.line)) == Some('[')
            {
                return Some(Block::HorizontalRule);
            }
            Some(Block::Para(content))
        }
        NodeValue::Heading(h) => Some(Block::Header(
            i64::from(h.level),
            Attr::default(),
            inlines(node.children()),
        )),
        NodeValue::BlockQuote => Some(Block::BlockQuote(blocks(node.children(), src, true))),
        NodeValue::CodeBlock(cb) => {
            // Pandoc keeps the literal untouched only for a fence that is
            // never closed, sits outside any blockquote, and is followed by
            // nothing but blank lines; every other fence loses one trailing
            // newline (which also drops a trailing blank line comrak kept).
            // Content lines start on the line after the opening fence, so
            // the block's last line is start.line + the literal line count.
            // (Sourcepos *end* lines are unreliable for unclosed blocks.)
            let keep_literal = cb.fenced
                && !cb.closed
                && !in_quote
                && src.only_blanks_after(data.sourcepos.start.line + literal_lines(&cb.literal));
            let text = if keep_literal {
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
            // An unclosed type-1..5 HTML block (outside blockquotes) gains
            // one bonus newline when only blank lines separate it from EOF.
            // Comrak's literal already contains the block's trailing blank
            // lines; its first line is the node's start line.
            if (1..=5).contains(&hb.block_type)
                && !contains_closer(&literal, hb.block_type)
                && !in_quote
                && src.only_blanks_after(
                    data.sourcepos.start.line + literal_lines(&literal) - 1,
                )
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
                    let mut bs = blocks(item.children(), src, in_quote);
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
    let iter = nodes.into_iter();
    let mut out = Vec::with_capacity(iter.size_hint().0 * 2);
    for node in iter {
        inline(node, &mut out);
    }
    merge_adjacent_emphasis(out)
}

/// Merge directly-adjacent same-type `Emph`/`Strong` siblings: pandoc's
/// commonmark reader never emits two in a row (`_a_*b*` is one `Emph`),
/// while comrak keeps them separate.
fn merge_adjacent_emphasis(tokens: Vec<Inline>) -> Vec<Inline> {
    let mut out: Vec<Inline> = Vec::with_capacity(tokens.len());
    for token in tokens {
        match (out.last_mut(), token) {
            (Some(Inline::Emph(prev)), Inline::Emph(next))
            | (Some(Inline::Strong(prev)), Inline::Strong(next)) => prev.extend(next),
            (_, token) => out.push(token),
        }
    }
    out
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
/// spaces and other Unicode whitespace stay inside `Str`. Comrak already
/// merges adjacent literal text (including across entities), so no further
/// coalescing is needed — or wanted: pandoc keeps separate `Str` tokens
/// where its own parse produces them.
fn text_tokens(text: &str, out: &mut Vec<Inline>) {
    let mut word = String::new();
    let mut in_spaces = false;
    for ch in text.chars() {
        if ch == ' ' {
            if !word.is_empty() {
                out.push(Inline::Str(std::mem::take(&mut word)));
            }
            if !in_spaces {
                out.push(Inline::Space);
                in_spaces = true;
            }
        } else {
            in_spaces = false;
            word.push(ch);
        }
    }
    if !word.is_empty() {
        out.push(Inline::Str(word));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc(md: &str) -> serde_json::Value {
        serde_json::to_value(read_commonmark(md).expect("convertible")).unwrap()["blocks"].clone()
    }

    #[test]
    fn pathological_nesting_is_refused_not_truncated() {
        let deep = ">".repeat(MAX_NESTING + 1);
        assert_eq!(read_commonmark(&deep), Err(Error::TooDeeplyNested));
        // A document at the limit still converts.
        let shallow = ">".repeat(10);
        assert!(read_commonmark(&shallow).is_ok());
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
        assert_eq!(
            doc("\tfoo\tbaz\t\tbim\n"),
            serde_json::json!([{"t": "CodeBlock", "c": [["", [], []], "foo baz     bim"]}])
        );
    }

    #[test]
    fn fence_newline_rules() {
        // Closed: stripped.
        assert_eq!(
            doc("```\naaa\n```\n"),
            serde_json::json!([{"t": "CodeBlock", "c": [["", [], []], "aaa"]}])
        );
        // Unclosed at EOF (with or without final newline): kept.
        assert_eq!(
            doc("```\naaa\n"),
            serde_json::json!([{"t": "CodeBlock", "c": [["", [], []], "aaa\n"]}])
        );
        assert_eq!(
            doc("```\naaa"),
            serde_json::json!([{"t": "CodeBlock", "c": [["", [], []], "aaa\n"]}])
        );
        // Unclosed inside a blockquote: stripped.
        assert_eq!(
            doc("> ```\n> aaa\n"),
            serde_json::json!([{"t": "BlockQuote", "c": [
                {"t": "CodeBlock", "c": [["", [], []], "aaa"]}
            ]}])
        );
        // Unclosed inside a list: kept.
        assert_eq!(
            doc("- ```\n  aaa\n"),
            serde_json::json!([{"t": "BulletList", "c": [[
                {"t": "CodeBlock", "c": [["", [], []], "aaa\n"]}
            ]]}])
        );
        // Closed fence indented inside a nested list: stripped.
        assert_eq!(
            doc("  - ```\n    aaa\n    ```\n"),
            serde_json::json!([{"t": "BulletList", "c": [[
                {"t": "CodeBlock", "c": [["", [], []], "aaa"]}
            ]]}])
        );
    }

    #[test]
    fn html_block_newline_rules() {
        // Unclosed comment at EOF gains one newline.
        assert_eq!(
            doc("<!-- x\n"),
            serde_json::json!([{"t": "RawBlock", "c": ["html", "<!-- x\n\n"]}])
        );
        // Also when blank lines intervene.
        assert_eq!(
            doc("<!-- x\n\n"),
            serde_json::json!([{"t": "RawBlock", "c": ["html", "<!-- x\n\n\n"]}])
        );
        // But not inside a blockquote.
        assert_eq!(
            doc("> <!-- x\n"),
            serde_json::json!([{"t": "BlockQuote", "c": [
                {"t": "RawBlock", "c": ["html", "<!-- x\n"]}
            ]}])
        );
        // Closed blocks are unchanged.
        assert_eq!(
            doc("<!-- x -->\n"),
            serde_json::json!([{"t": "RawBlock", "c": ["html", "<!-- x -->\n"]}])
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

    #[test]
    fn adjacent_same_type_emphasis_merges() {
        assert_eq!(
            doc("_a_*b*\n"),
            serde_json::json!([{"t": "Para", "c": [
                {"t": "Emph", "c": [{"t": "Str", "c": "a"}, {"t": "Str", "c": "b"}]}
            ]}])
        );
        assert_eq!(
            doc("**a**__b__\n"),
            serde_json::json!([{"t": "Para", "c": [
                {"t": "Strong", "c": [{"t": "Str", "c": "a"}, {"t": "Str", "c": "b"}]}
            ]}])
        );
    }

    #[test]
    fn carriage_returns_are_filtered() {
        assert_eq!(
            doc("```\r\ncode\r\n```\r\n"),
            serde_json::json!([{"t": "CodeBlock", "c": [["", [], []], "code"]}])
        );
        assert_eq!(
            doc("a\rb\n"),
            serde_json::json!([{"t": "Para", "c": [{"t": "Str", "c": "ab"}]}])
        );
    }

    #[test]
    fn refdef_followed_by_dash_run_is_a_thematic_break() {
        assert_eq!(doc("[r]: /u\n---\n"), serde_json::json!([{"t": "HorizontalRule"}]));
        assert_eq!(
            doc("> [r]: /u\n> ---\n"),
            serde_json::json!([{"t": "BlockQuote", "c": [{"t": "HorizontalRule"}]}])
        );
        // Equals-runs stay a paragraph (pandoc agrees with comrak there).
        assert_eq!(
            doc("[r]: /u\n===\n"),
            serde_json::json!([{"t": "Para", "c": [{"t": "Str", "c": "==="}]}])
        );
        // Escaped dashes are not a thematic break on either side: pandoc
        // agrees with us (`\-` unescapes to `-`, leaving a plain paragraph).
        assert_eq!(
            doc("\\---\n"),
            serde_json::json!([{"t": "Para", "c": [{"t": "Str", "c": "---"}]}])
        );
    }

    #[test]
    fn failed_delimiters_stay_as_pandoc_tokenizes_them() {
        assert_eq!(
            doc("**a* b\n"),
            serde_json::json!([{"t": "Para", "c": [
                {"t": "Str", "c": "*"},
                {"t": "Emph", "c": [{"t": "Str", "c": "a"}]},
                {"t": "Space"}, {"t": "Str", "c": "b"}
            ]}])
        );
        assert_eq!(
            doc("a _b c\n"),
            serde_json::json!([{"t": "Para", "c": [
                {"t": "Str", "c": "a"}, {"t": "Space"},
                {"t": "Str", "c": "_b"}, {"t": "Space"}, {"t": "Str", "c": "c"}
            ]}])
        );
    }
}
