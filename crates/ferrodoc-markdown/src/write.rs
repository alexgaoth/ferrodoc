//! Markdown writer: renders the ferrodoc AST back to `CommonMark`.
//!
//! The contract is semantic, not textual: what this emits must *re-read* to
//! the document it was given, which is what `ferrodoc-harness diff-md`
//! checks against pandoc's own markdown writer. Output is therefore
//! escaped conservatively — a writer that emits `*` where the source meant
//! a literal asterisk is silently lossy, and that is the only way a
//! markdown writer really fails.
//!
//! `CommonMark` cannot represent tables, footnotes or definition lists; like
//! pandoc's `commonmark` writer, those degrade (tables and notes to raw
//! HTML-ish output, definition lists to paragraphs) and are not covered by
//! the differential.

use ferrodoc_ast::{
    Block, Inline, ListNumberDelim, MathType, Pandoc, QuoteType,
};
use std::fmt::Write as _;

/// Render a document as `CommonMark`.
pub fn write_markdown(doc: &Pandoc) -> String {
    let mut out = String::new();
    Writer::default().blocks(&mut out, &doc.blocks, "");
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
}

impl Writer {
    /// Write blocks separated by blank lines, each line carrying `prefix`
    /// (the `> ` of a quote, or a list item's continuation indent).
    fn blocks(&mut self, out: &mut String, blocks: &[Block], prefix: &str) {
        let mut previous: Option<&Block> = None;
        for block in blocks {
            if let Some(previous) = previous {
                // The blank line between blocks still belongs to the
                // container: a bare newline inside a quote ends the quote.
                push_line(out, prefix, "");
                // Two lists in a row would merge into one on re-reading.
                if needs_separator(previous, block) {
                    push_line(out, prefix, "<!-- -->");
                    push_line(out, prefix, "");
                }
            }
            self.block(out, block, prefix);
            previous = Some(block);
        }
        // Footnote bodies belong at the end of the document.
        if prefix.is_empty() && !self.notes.is_empty() {
            for (index, body) in std::mem::take(&mut self.notes).iter().enumerate() {
                out.push('\n');
                let _ = writeln!(out, "[^{}]: {body}", index + 1);
            }
        }
    }

    fn block(&mut self, out: &mut String, block: &Block, prefix: &str) {
        match block {
            Block::Plain(inlines) | Block::Para(inlines) => {
                let text = self.inlines(inlines);
                push_wrapped(out, prefix, &text);
            }
            Block::Header(level, _, inlines) => {
                let hashes = "#".repeat(usize::try_from(*level).unwrap_or(1).clamp(1, 6));
                let text = self.inlines(inlines);
                push_line(out, prefix, &format!("{hashes} {text}"));
            }
            Block::CodeBlock(attr, text) => {
                // A fence must be longer than any backtick run inside.
                let longest = text
                    .split(|c| c != '`')
                    .map(str::len)
                    .max()
                    .unwrap_or(0);
                let fence = "`".repeat(longest.max(2) + 1);
                let info = attr.classes.first().map_or("", String::as_str);
                push_line(out, prefix, &format!("{fence}{info}"));
                for line in text.split('\n') {
                    push_line(out, prefix, line);
                }
                push_line(out, prefix, &fence);
            }
            Block::BlockQuote(blocks) => {
                let inner = format!("{prefix}> ");
                self.blocks(out, blocks, &inner);
            }
            Block::BulletList(items) => {
                self.list(out, items, prefix, |_| "- ".to_owned());
            }
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
                    format!("{label}{close} ")
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
                    push_wrapped(out, prefix, &text);
                    for definition in definitions {
                        push_line(out, prefix, "");
                        self.blocks(out, definition, prefix);
                    }
                }
            }
            Block::HorizontalRule => push_line(out, prefix, "---"),
            Block::LineBlock(lines) => {
                // Hard breaks preserve the line structure.
                let mut text = String::new();
                for (index, line) in lines.iter().enumerate() {
                    if index > 0 {
                        text.push_str("  \n");
                    }
                    text.push_str(&self.inlines(line));
                }
                push_wrapped(out, prefix, &text);
            }
            Block::RawBlock(format, text) => {
                if format.0 == "html" {
                    for line in text.trim_end_matches('\n').split('\n') {
                        push_line(out, prefix, line);
                    }
                }
            }
            Block::Div(_, blocks) => self.blocks(out, blocks, prefix),
            Block::Figure(..) | Block::Table(_) => self.unrepresentable(out, block, prefix),
        }
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
                    .chain(table.bodies.iter().flat_map(|b| b.head.iter().chain(&b.body)))
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
        let tight = items
            .iter()
            .all(|item| item.iter().all(|b| !matches!(b, Block::Para(_))));
        for (index, item) in items.iter().enumerate() {
            if index > 0 && !tight {
                push_line(out, prefix, "");
            }
            let marker = marker(index);
            let indent = " ".repeat(marker.chars().count());
            // A tight list's items must not contain blank lines: one
            // would make the whole list loose when it is read back, and
            // every `Plain` inside would come back as a `Para`.
            let mut body = String::new();
            if tight {
                for block in item {
                    self.block(&mut body, block, "");
                }
            } else {
                self.blocks(&mut body, item, "");
            }
            let mut lines = body.trim_end_matches('\n').split('\n');
            if let Some(first) = lines.next() {
                push_line(out, prefix, &format!("{marker}{first}"));
            }
            for line in lines {
                if line.is_empty() {
                    push_line(out, prefix, "");
                } else {
                    push_line(out, prefix, &format!("{indent}{line}"));
                }
            }
        }
    }

    // --- inlines ---

    fn inlines(&mut self, inlines: &[Inline]) -> String {
        let mut out = String::new();
        for inline in inlines {
            self.inline(&mut out, inline);
        }
        out
    }

    fn inline(&mut self, out: &mut String, inline: &Inline) {
        match inline {
            Inline::Str(text) => escape_text(out, text),
            Inline::Space | Inline::SoftBreak => out.push(' '),
            // Two trailing spaces before the newline is a hard break.
            Inline::LineBreak => out.push_str("  \n"),
            Inline::Emph(inner) => {
                out.push('*');
                out.push_str(&self.inlines(inner));
                out.push('*');
            }
            Inline::Strong(inner) => {
                out.push_str("**");
                out.push_str(&self.inlines(inner));
                out.push_str("**");
            }
            // No CommonMark syntax: keep the text, drop the styling.
            Inline::Strikeout(inner)
            | Inline::Superscript(inner)
            | Inline::Subscript(inner)
            | Inline::SmallCaps(inner)
            | Inline::Underline(inner)
            | Inline::Span(_, inner)
            | Inline::Cite(_, inner) => {
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
                let pad = if text.starts_with('`') || text.ends_with('`') { " " } else { "" };
                let _ = write!(out, "{ticks}{pad}{text}{pad}{ticks}");
            }
            Inline::Math(kind, text) => {
                let delimiter = match kind {
                    MathType::InlineMath => "$",
                    MathType::DisplayMath => "$$",
                };
                escape_text(out, &format!("{delimiter}{text}{delimiter}"));
            }
            Inline::Link(_, inner, target) => {
                let text = self.inlines(inner);
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
                let mut body = String::new();
                self.blocks(&mut body, blocks, "");
                self.notes.push(body.trim_end().replace('\n', " "));
                let _ = write!(out, "[^{}]", self.notes.len());
            }
        }
    }
}

/// Whether two adjacent blocks would merge into one when re-read.
fn needs_separator(previous: &Block, next: &Block) -> bool {
    matches!(
        (previous, next),
        (Block::BulletList(_), Block::BulletList(_))
            | (Block::OrderedList(..), Block::OrderedList(..))
    )
}

/// A link destination, wrapped in angle brackets when it needs them.
fn link_destination(url: &str) -> String {
    if url.is_empty() || url.contains([' ', '(', ')']) {
        format!("<{}>", url.replace('>', "%3E"))
    } else {
        url.to_owned()
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
fn push_wrapped(out: &mut String, prefix: &str, text: &str) {
    for line in text.split('\n') {
        push_line(out, prefix, line);
    }
}

/// Escape text so it re-reads as itself.
///
/// Conservative on purpose: characters that could open markup are always
/// escaped, and line-leading characters that could start a block are
/// escaped too. Everything else is left alone so the output stays readable.
fn escape_text(out: &mut String, text: &str) {
    for ch in text.chars() {
        let at_line_start = out.is_empty() || out.ends_with('\n');
        match ch {
            '\\' | '*' | '_' | '[' | ']' | '<' | '>' | '`' => {
                out.push('\\');
                out.push(ch);
            }
            // These only mean something at the start of a line.
            '#' | '-' | '+' | '=' | '~' | ':' | '|' if at_line_start => {
                out.push('\\');
                out.push(ch);
            }
            '!' => {
                // Only dangerous immediately before a link.
                out.push('\\');
                out.push('!');
            }
            '&' => out.push_str("&amp;"),
            ch => out.push(ch),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::read_commonmark;

    /// Read, write, read again: the second AST must equal the first.
    fn round_trips(markdown: &str) -> bool {
        let first = read_commonmark(markdown).expect("convertible");
        let written = write_markdown(&first);
        let second = read_commonmark(&written).expect("convertible");
        if first.blocks != second.blocks {
            eprintln!("--- input:\n{markdown}\n--- written:\n{written}");
            eprintln!("--- first: {:?}\n--- second: {:?}", first.blocks, second.blocks);
        }
        first.blocks == second.blocks
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
        let two_lists = "- a\n\n<!-- -->\n\n- b\n";
        assert!(round_trips(two_lists));
    }
}
