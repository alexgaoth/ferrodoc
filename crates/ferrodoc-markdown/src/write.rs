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
    Alignment, Block, Inline, ListNumberDelim, ListNumberStyle, MathType, Pandoc, QuoteType, Row,
    Table, Target,
};
use std::fmt::Write as _;

/// What a footnote body's continuation lines carry, as pandoc writes them.
const INDENT: &str = "    ";

/// Which markdown this writer is writing.
///
/// `CommonMark` and `Gfm` differ in four constructs a document can carry;
/// `Pandoc` differs from both in what it does to *text* — see
/// [`write_pandoc_markdown`].
#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub enum Flavour {
    /// `CommonMark`, which is what `-t markdown` writes.
    #[default]
    CommonMark,
    /// `GitHub` Flavored Markdown.
    Gfm,
    /// A notebook cell: `GitHub` Flavored Markdown **except for math**.
    ///
    /// Pandoc's `ipynb` writer takes the same four GFM constructs and
    /// writes `$x^2$` where its `gfm` writer writes `` $`x^2`$ ``. One
    /// flavour served both here until 2026-08-28, which is right for
    /// exactly one of them.
    Ipynb,
    /// Pandoc's own markdown.
    Pandoc,
}

impl Flavour {
    /// Whether the four GFM constructs are available: pipe tables, task
    /// items, strikethrough, and the extra escaping a bare URL needs.
    ///
    /// **A method rather than `== Flavour::Gfm`**, because `Ipynb` is
    /// GFM in everything but math and a bare comparison silently gave it
    /// `CommonMark`'s escaping the day it was added — the notebook round
    /// trip fell 11/16 to 6/16 and nothing else moved.
    fn is_gfm(self) -> bool {
        matches!(self, Flavour::Gfm | Flavour::Ipynb)
    }
}

/// Render a document as **pandoc's own markdown**, the dialect
/// `pandoc -t markdown` writes.
///
/// Every rule here was probed by handing pandoc a JSON AST and reading
/// the bytes back:
///
/// | in the tree | written |
/// |---|---|
/// | `—` `–` `…` | `---` `--` `...` |
/// | `'` `"` `~` | `\'` `\"` `\~` |
/// | `LineBreak` | a `\` at the end of the line |
/// | `RawInline("html", "<em>")` | `` `<em>`{=html} `` |
/// | `Header` with an identifier | `# T {#myid}` |
/// | `Note` | `[^1]`, and its body after the block |
/// | `DefinitionList` | `Term`, blank line, `:   Def` |
/// | `Div` with attributes | `::: {#d .warn}` … `:::` |
///
/// The un-smartening is the surprising half: pandoc reads `---` as an
/// em-dash and writes an em-dash back out as `---`, so a document round
/// trips through the dialect unchanged.
#[must_use]
pub fn write_pandoc_markdown(doc: &Pandoc) -> String {
    render_with(doc, Flavour::Pandoc, None, 72)
}

/// The same, filled to `columns`.
#[must_use]
pub fn write_pandoc_markdown_wrapped(doc: &Pandoc, columns: usize) -> String {
    render_with(doc, Flavour::Pandoc, Some(columns), columns)
}

/// Render a document as `CommonMark`.
#[must_use]
pub fn write_markdown(doc: &Pandoc) -> String {
    render(doc, Flavour::CommonMark)
}

/// Render a document as `CommonMark`, filled to `columns`.
///
/// See [`write_gfm_wrapped`] for what "breakable" means here.
pub fn write_markdown_wrapped(doc: &Pandoc, columns: usize) -> String {
    render_with(doc, Flavour::CommonMark, Some(columns), columns)
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
    render_with(doc, Flavour::Gfm, Some(columns), columns)
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
    render(doc, Flavour::Gfm)
}

/// Render a document as a **notebook cell**: [`write_gfm`] with pandoc's
/// `ipynb` spelling of math, `$x^2$` rather than `` $`x^2`$ ``.
#[must_use]
pub fn write_notebook(doc: &Pandoc) -> String {
    render(doc, Flavour::Ipynb)
}

/// The same, filled to `columns`.
#[must_use]
pub fn write_notebook_wrapped(doc: &Pandoc, columns: usize) -> String {
    render_with(doc, Flavour::Ipynb, Some(columns), columns)
}

fn render(doc: &Pandoc, flavour: Flavour) -> String {
    render_with(doc, flavour, None, 72)
}

fn render_with(doc: &Pandoc, flavour: Flavour, columns: Option<usize>, layout: usize) -> String {
    // **`Wrap::None` arrives as `Some(usize::MAX)`** — a sentinel for
    // "do not fill", not a width anyone meant. A table still needs one,
    // and pandoc lays a multiline table out at `--columns` in every wrap
    // mode, so the sentinel falls back to pandoc's default. Multiplied
    // by a column's fraction it asked for an 8-exabyte string instead.
    let layout = if layout == usize::MAX { 72 } else { layout };
    let mut out = String::new();
    let mut writer = Writer { flavour, columns, layout, bullet: '-', ..Writer::default() };
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

/// The two places the writer's position changes what it must escape.
#[derive(Default)]
struct Standing {
    /// Whether the next block written is the first of a list item that
    /// holds more than one block. Four spaces there are the marker's own
    /// column, not a code fence.
    item_start: bool,
    /// Whether something is already written **in front of these inlines
    /// on their line** — a heading's `## `, or the `*`, `**` or `[` of a
    /// container. Nothing opens a block after any of them, so the escapes
    /// that exist to stop one are dead weight. Each container renders its
    /// content into a *fresh* buffer, so without this every one of them
    /// looks like the start of an empty line: pandoc writes
    /// `## 0. Before anything` and `**1. The two gates**` where this
    /// wrote `## 0\.` and `**1\.`.
    preceded: bool,
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
    flavour: Flavour,
    /// Where in the document the writer stands, which is what decides
    /// whether an escape is needed at all.
    at: Standing,
    /// The column to fill to, or `None` to leave every line as it falls.
    columns: Option<usize>,
    /// The width a **table** is laid out to, which is `--columns`
    /// whatever `--wrap` says: pandoc sizes a multiline table's columns
    /// from it even with `--wrap=preserve`, where `columns` above is
    /// `None` because nothing is filled.
    layout: usize,
    /// How many **list items** deep the block being written is. A
    /// blockquote does not count: its `> ` prefix makes four more spaces
    /// unambiguous, where a list item's own indentation does not.
    depth: usize,
}

/// Marks a space a line may be broken at. Chosen because no reader here
/// can produce one inside text: `CommonMark` replaces NUL with U+FFFD by
/// specification, and XML — which DOCX, ODT and EPUB are — forbids it
/// outright. Every one is either broken at or turned back into a space
/// before the string leaves [`push_wrapped`].
const BREAK: char = '\0';

/// Where in the document a block sits, for the two spellings that depend
/// on it. An indented code block is four spaces from the *line*: after a
/// list or a quote those four continue the container, and at the start of
/// a list item they are the marker's own column.
#[derive(Clone, Copy, Default)]
struct Position {
    after_container: bool,
    first_in_item: bool,
    /// Nothing has been written into this container yet.
    ///
    /// A simple table needs a blank line in front of it, and when it
    /// opens a blockquote or a list item there is none to reuse — so
    /// pandoc writes one, prefix and all: `> ` before the header row.
    /// At the top of a document it writes nothing.
    first_in_container: bool,
}

impl Writer {
    /// Whether the four GFM constructs are available: pipe tables, task
    /// items, strikethrough, and the extra escaping a bare URL needs.
    fn gfm(&self) -> bool {
        self.flavour.is_gfm()
    }

    /// Whether this is pandoc's own markdown, which differs from both of
    /// the others in what it does to *text*.
    fn pandoc(&self) -> bool {
        self.flavour == Flavour::Pandoc
    }
}

impl Writer {
    /// Write blocks separated by blank lines, each line carrying `prefix`
    /// (the `> ` of a quote, or a list item's continuation indent).
    fn blocks(&mut self, out: &mut String, blocks: &[Block], prefix: &str) {
        let mut previous: Option<&Block> = None;
        for block in blocks {
            // A raw block in another format renders to nothing, and its
            // separator goes with it — otherwise a document ending in one
            // ends with a blank line pandoc does not write.
            if !self.pandoc()
                && matches!(block, Block::RawBlock(format, _) if format.0 != "html")
            {
                continue;
            }
            if let Some(previous) = previous {
                // The blank line between blocks still belongs to the
                // container: a bare newline inside a quote ends the quote.
                push_line(out, prefix, "");
                // Two lists in a row would merge into one on re-reading,
                // and **the two dialects need different separators**.
                //
                // `CommonMark` and gfm split on a change of bullet
                // character, which adds no block where a separator
                // comment adds a `RawBlock`. **Pandoc's dialect does
                // not**: `- a` then `* b` reads back there as one list,
                // so the trick that keeps the document whole in one
                // dialect silently merges it in the other. Measured on
                // the pinned binary, our own `-t markdown` output:
                //
                // ```text
                // pandoc -f commonmark   [BulletList, BulletList]
                // pandoc -f markdown     [BulletList]
                // ```
                //
                // So the dialect takes the comment, as pandoc's own
                // writer does — three blocks back for two is a worse
                // answer than two, and one is worse than both.
                match (previous, block) {
                    (Block::BulletList(_), Block::BulletList(_)) if !self.pandoc() => {
                        self.bullet = if self.bullet == '-' { '*' } else { '-' };
                    }
                    (Block::BulletList(_), Block::BulletList(_)) => {
                        push_line(out, prefix, "<!-- -->");
                        push_line(out, prefix, "");
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
            let at = Position {
                after_container: matches!(
                    previous,
                    Some(Block::BulletList(_) | Block::OrderedList(..) | Block::BlockQuote(_))
                ),
                first_in_item: self.at.item_start,
                first_in_container: previous.is_none() && !prefix.is_empty(),
            };
            self.at.item_start = false;
            self.block(out, block, prefix, at);
            previous = Some(block);
        }
    }

    /// One footnote: its label here, its body kept for the end.
    ///
    /// GFM spells a footnote `[^1]` and its body `[^1]: …` under a
    /// continuation indent. `CommonMark` has no footnote at all, so pandoc
    /// degrades one to a bracketed number, and so does this — writing
    /// GFM's spelling into `CommonMark` output made a file this reader
    /// would not read back as a footnote.
    /// A definition list in pandoc's dialect: the term on its own line,
    /// then `:` and three spaces, with continuation lines under four.
    fn definition_list(
        &mut self,
        out: &mut String,
        items: &[(Vec<Inline>, Vec<Vec<Block>>)],
        prefix: &str,
    ) {

        let mut first = true;
        for (term, definitions) in items {
            if !first {
                push_line(out, prefix, "");
            }
            first = false;
            let text = self.inlines(term);
            push_wrapped(out, prefix, &text, self.columns);
            for definition in definitions {
                // **Loose or tight, the same distinction a list
                // makes.** A definition whose first block is a
                // `Para` gets a blank line before it and one
                // that is a `Plain` does not — which is how the
                // document said it, and pandoc says it back.
                if !matches!(definition.first(), Some(Block::Plain(_))) {
                    push_line(out, prefix, "");
                }
                let mut body = String::new();
                self.blocks(&mut body, definition, "");
                // `:` then three spaces, and the continuation
                // lines under four — pandoc's own shape, probed.
                for (index, line) in body.trim_end().split('\n').enumerate() {
                    let marker = if index == 0 { ":   " } else { INDENT };
                    let text = if line.is_empty() {
                        String::new()
                    } else {
                        format!("{marker}{line}")
                    };
                    push_line(out, prefix, &text);
                }
            }
        }
    }

    fn note(&mut self, out: &mut String, blocks: &[Block]) {
        // Reserve the number before rendering: a note nested inside this
        // one would otherwise take this one's label.
        let index = self.notes.len();
        self.notes.push(String::new());
        let mut body = String::new();
        let labelled = self.gfm() || self.pandoc();
        self.blocks(&mut body, blocks, if labelled { INDENT } else { "" });
        self.notes[index] = body;
        let caret = if labelled { "^" } else { "" };
        let _ = write!(out, "[{caret}{}]", index + 1);
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
            out.push('\n');
            if self.gfm() || self.pandoc() {
                // The body was rendered under `INDENT`; pandoc puts the
                // first line on the label's line and leaves the rest
                // indented.
                let first = body.strip_prefix(INDENT).unwrap_or(body);
                let _ = writeln!(out, "[^{}]: {first}", index + 1);
            } else {
                // CommonMark has no footnote at all, so pandoc degrades
                // one to a bracketed number and its body to ordinary
                // blocks after the document — no caret, no colon and no
                // indent, because none of them would be read back.
                let _ = writeln!(out, "[{}] {body}", index + 1);
            }
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
    fn code_block(
        &self,
        out: &mut String,
        attr: &ferrodoc_ast::Attr,
        text: &str,
        prefix: &str,
        at: Position,
    ) {
        let indentable = attr == &ferrodoc_ast::Attr::default()
            && !at.after_container
            && !at.first_in_item
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
        // A fence must be longer than any run of backticks that could
        // **close** it, and only a line that is nothing but backticks —
        // after up to three spaces of indent — can. A run in the middle
        // of a line cannot, so `a ``` b` needs no more than three, and
        // neither does ```` ```rust ````: `README.md` holds a `console`
        // block full of both, and every fence around one was a backtick
        // too long.
        let longest = text
            .lines()
            .map(|line| line.trim_matches([' ', '\t']))
            .filter(|line| !line.is_empty() && line.chars().all(|c| c == '`'))
            .map(str::len)
            .max()
            .unwrap_or(0);
        let fence = "`".repeat(longest.max(2) + 1);
        // **The two dialects spell the info string differently**, and
        // pandoc's HTML writer tags every code block `sourceCode`, so
        // the difference shows on anything that came from HTML.
        //
        // `gfm` and `commonmark` drop that class and spell the first one
        // left, after a space: `["sourceCode","bash"]` is
        // ```` ``` bash ```` and `["sourceCode"]` a bare fence. Pandoc's
        // own dialect drops nothing — it writes a bare name only when
        // there is exactly **one** class and no identifier and no
        // attribute, and the whole attribute set otherwise:
        //
        // ```text
        // ["sourceCode","bash"]   ``` {.sourceCode .bash}
        // ["sourceCode"]          ``` sourceCode
        // #i + ["bash"]           ``` {#i .bash}
        // ["bash"] + k="v"        ``` {.bash k="v"}
        // ```
        let info = if self.pandoc() {
            match attr.classes.as_slice() {
                [class] if attr.identifier.is_empty() && attr.attributes.is_empty() => {
                    format!(" {class}")
                }
                _ => attributes(attr).map_or(String::new(), |a| format!(" {a}")),
            }
        } else {
            attr.classes
                .iter()
                .find(|class| class.as_str() != "sourceCode")
                .map_or(String::new(), |class| format!(" {class}"))
        };
        push_line(out, prefix, &format!("{fence}{info}"));
        // **Empty content writes no line at all** — `"".split('\n')` is
        // one empty element, so this wrote a blank line where pandoc
        // writes two adjacent fences, and read its own output back as
        // `"\n"`.
        //
        // The trailing newline is **deliberately not stripped**, though
        // pandoc strips it. `pandoc -f json -t markdown` writes the
        // content `x\n` as plain `x`, and reads that back as `x`: the
        // newline is gone. Matching those bytes costs a round trip that
        // works today, so this keeps the blank line and takes the
        // difference — COMPATIBILITY.md records it beside the other
        // places pandoc's own output does not survive its own reader.
        if !text.is_empty() {
            for line in text.split('\n') {
                push_line(out, prefix, line);
            }
        }
        push_line(out, prefix, &fence);
            
    
    }

    fn block(&mut self, out: &mut String, block: &Block, prefix: &str, at: Position) {
        if self.pandoc() && self.dialect_block(out, block, prefix) {
            return;
        }
        match block {
            Block::Plain(inlines) | Block::Para(inlines) => {
                let text = self.inlines(inlines);
                push_wrapped(out, prefix, &text, self.columns);
            }
            Block::Header(level, attr, inlines) => {
                let mut text = self.inner(inlines);
                // **Pandoc's dialect writes a heading's attributes back**;
                // `CommonMark` has nowhere to put them, so it drops them
                // and the identifier is regenerated on the next read.
                // **An identifier the reader would regenerate is not
                // written.** Pandoc omits `{#my-heading}` on
                // `# My Heading` and keeps `{#custom}`, because the first
                // comes back on its own and the second does not. Probed
                // three ways round.
                let generated = attr.identifier == crate::slug(&plain_text(inlines), crate::Dialect::Pandoc);
                let attr = if generated {
                    ferrodoc_ast::Attr { identifier: String::new(), ..attr.clone() }
                } else {
                    attr.clone()
                };
                if self.pandoc()
                    && let Some(written) = attributes(&attr)
                {
                    text.push(' ');
                    text.push_str(&written);
                }
                header(out, prefix, *level, &text);
            }
            Block::CodeBlock(attr, text) => self.code_block(out, attr, text, prefix, at),
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
            Block::OrderedList(attrs, items) => self.ordered_list(out, attrs, items, prefix),
            Block::DefinitionList(items) => {
                // No CommonMark syntax for one, so pandoc writes the term
                // and its **first** definition as a single paragraph
                // joined by a hard break — `term  \ndef` — and only a
                // *second* definition starts a new one. Written as two
                // paragraphs the term and its definition came back as
                // separate blocks.
                let mut first = true;
                for (term, definitions) in items {
                    if !first {
                        push_line(out, prefix, "");
                    }
                    first = false;
                    let mut text = self.inlines(term);
                    if let Some(definition) = definitions.first() {
                        let mut body = String::new();
                        self.blocks(&mut body, definition, "");
                        text.push_str("  \n");
                        text.push_str(body.trim_end_matches('\n'));
                    }
                    push_wrapped(out, prefix, &text, self.columns);
                    for definition in definitions.iter().skip(1) {
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
                // **The dialect keeps a raw block whatever its format**,
                // because `raw_attribute` gives it somewhere to live;
                // `CommonMark` and gfm can hold only the HTML one and
                // drop the rest rather than write it as prose.
                if format.0 == "html" || self.pandoc() {
                    for line in text.trim_end_matches('\n').split('\n') {
                        push_line(out, prefix, line);
                    }
                }
            }
            // Neither CommonMark nor GFM has a syntax for a div, so
            // pandoc falls back to the raw HTML tag — for a bare `<div>`
            // as much as for one carrying attributes, which is where this
            // differs from a `Span`. Writing only the contents dropped
            // the identifier, the classes and every key-value silently:
            // `samples/inputs/attributes.html` is three divs, and the
            // round trip lost all three until 2026-08-25.
            // …but not for the wrapper pandoc's *own* highlighting puts
            // round a code block. Classes exactly `sourceCode` and one
            // `CodeBlock` inside is that wrapper and nothing else, so it
            // is unwrapped — a `sourceCode x` div, or one holding a
            // paragraph, is a document's own div and stays.
            Block::Div(attr, blocks)
                if attr.classes == ["sourceCode"]
                    && matches!(blocks.as_slice(), [Block::CodeBlock(..)]) =>
            {
                self.blocks(out, blocks, prefix);
            }
            Block::Div(attr, blocks) => {
                push_line(out, prefix, &format!("<div{}>", html_attributes(attr)));
                push_line(out, prefix, "");
                self.blocks(out, blocks, prefix);
                push_line(out, prefix, "");
                push_line(out, prefix, "</div>");
            }
            // A table with no columns has no pipe-table spelling at all —
            // not even an empty one — so it degrades like the rest.
            Block::Table(t) => self.table(out, block, t, prefix, at),
            Block::Figure(..) => Self::unrepresentable(out, block, prefix),
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
        // Pandoc's dialect writes `- [ ]` too; only `CommonMark` has to
        // fall back to the `☐`/`☒` its reader gave us.
        let tasks: Vec<Option<bool>> = if self.gfm() || self.pandoc() {
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
        let mut lines: Vec<Vec<String>> = Vec::new();
        lines.push(header.map(|row| self.cells(row, columns, true, false)).unwrap_or_default());
        for row in body {
            lines.push(self.cells(row, columns, true, false));
        }

        // **Pandoc pads every column to its widest cell**, with a floor of
        // three, and the rule line fills the same width. A table written
        // unpadded reads back identically, which is why no round trip ever
        // saw this and why `corpus/gfm/tables.gfm` differed on every row.
        let width = |index: usize| {
            lines
                .iter()
                .filter_map(|cells| cells.get(index))
                .map(|cell| cell.chars().count())
                .max()
                .unwrap_or(0)
                .max(3)
        };
        let widths: Vec<usize> = (0..columns).map(width).collect();

        let rule: Vec<String> = table
            .colspecs
            .iter()
            .zip(&widths)
            .map(|(spec, width)| {
                // The colons sit inside the column's own width, so a
                // left-aligned column of three is `:---` and not `:---:`.
                let dashes = |n: usize| "-".repeat(width + 2 - n);
                match spec.alignment {
                    Alignment::AlignLeft => format!(":{}", dashes(1)),
                    Alignment::AlignCenter => format!(":{}:", dashes(2)),
                    Alignment::AlignRight => format!("{}:", dashes(1)),
                    Alignment::AlignDefault => dashes(0),
                }
            })
            .collect();

        let alignments: Vec<Alignment> =
            table.colspecs.iter().map(|spec| spec.alignment).collect();
        for (index, cells) in lines.iter().enumerate() {
            push_line(out, prefix, &pipe_row(cells, &widths, &alignments));
            if index == 0 {
                push_line(out, prefix, &format!("|{}|", rule.join("|")));
            }
        }
        if !table.caption.blocks.is_empty() {
            push_line(out, prefix, "");
            self.blocks(out, &table.caption.blocks, prefix);
        }
    }

    /// **Pandoc's dialect has a fenced div**, which the other two
    /// flavours do not — they fall through to a raw `<div>`.
    fn fenced_div(&mut self, out: &mut String, attr: &ferrodoc_ast::Attr, blocks: &[Block], prefix: &str) {
        push_line(out, prefix, &format!("::: {}", fence_attributes(attr)));
        self.blocks(out, blocks, prefix);
        push_line(out, prefix, ":::");
    }

    /// The table spelling this flavour has, or raw HTML where it has
    /// none.
    ///
    /// A table with no columns has no pipe-table spelling at all — not
    /// even an empty one — so it degrades like the rest.
    fn table(&mut self, out: &mut String, block: &Block, table: &Table, prefix: &str, at: Position) {
        if self.gfm() && !table.colspecs.is_empty() {
            self.pipe_table(out, table, prefix);
        } else if self.pandoc() && simple_table_applies(table) {
            self.simple_table(out, table, prefix, at);
        } else if self.pandoc() && multiline_table_applies(table) {
            self.multiline_table(out, table, prefix);
        } else {
            Self::unrepresentable(out, block, prefix);
        }
    }

    /// One row laid out: each cell filled to its column, the row as tall
    /// as the tallest cell, and every column padded but the last.
    fn multiline_row(cells: &[String], widths: &[usize]) -> Vec<String> {
            let filled: Vec<Vec<String>> = widths
                .iter()
                .enumerate()
                .map(|(index, width)| fill(cells.get(index).map_or("", String::as_str), *width))
                .collect();
            let height = filled.iter().map(Vec::len).max().unwrap_or(0).max(1);
            (0..height)
                .map(|line| {
                    let mut text = String::new();
                    for (index, width) in widths.iter().enumerate() {
                        if index > 0 {
                            text.push(' ');
                        }
                        let piece = filled[index].get(line).map_or("", String::as_str);
                        text.push_str(piece);
                        if index + 1 < widths.len() {
                            text.push_str(&" ".repeat(width.saturating_sub(piece.chars().count())));
                        }
                    }
                    format!("  {text}")
                })
                .collect()
    }

    /// One row's cells for a multiline table: the inline text **before**
    /// it is laid out, so the break opportunities survive for `fill` to
    /// break at. `cells` lays each cell out at the writer's own column
    /// first, which leaves nothing to wrap.
    fn multiline_cells(&mut self, row: &Row, columns: usize) -> Vec<String> {
        let mut out: Vec<String> = Vec::with_capacity(columns);
        for cell in &row.cells {
            let text = match cell.blocks.as_slice() {
                [Block::Plain(inlines) | Block::Para(inlines)] => self.inlines(inlines),
                _ => String::new(),
            };
            out.push(text);
            for _ in 1..cell.col_span.max(1) {
                out.push(String::new());
            }
        }
        out
    }

    /// Pandoc's **multiline table**, which is what a column stating its
    /// own width gets — the shape every table converted from DOCX, ODT or
    /// HTML arrives in, and one that fell back to raw HTML here.
    ///
    /// Measured against `pandoc -f json -t markdown` one shape at a time:
    ///
    /// * a column is `floor(fraction x available)` wide, where available
    ///   is `--columns` less the one space between each pair — 0.5 and
    ///   0.5 at 72 columns are 35 and 35, not 36 and 36;
    /// * the width comes from `--columns` **whatever `--wrap` says**,
    ///   which is why this writer carries a layout width beside the fill
    ///   one;
    /// * a cell too wide for its column is filled and the row becomes
    ///   several lines, the other columns padded out beside it;
    /// * body rows are separated by a blank line, and a body of exactly
    ///   **one** row is followed by one as well;
    /// * with a header the table opens and closes on a full-width rule;
    ///   without one it opens and closes on the per-column rules.
    fn multiline_table(&mut self, out: &mut String, table: &Table, prefix: &str) {
        let count = table.colspecs.len();
        let available = self.layout.saturating_sub(count.saturating_sub(1));
        let base: Vec<usize> = table
            .colspecs
            .iter()
            .map(|spec| match spec.width {
                #[expect(
                    clippy::cast_possible_truncation,
                    clippy::cast_sign_loss,
                    clippy::cast_precision_loss,
                    reason = "a column width is small, never negative, and \
                              well inside f64's mantissa"
                )]
                ferrodoc_ast::ColWidth::ColWidth(fraction) => {
                    (fraction * available as f64).floor() as usize
                }
                ferrodoc_ast::ColWidth::ColWidthDefault => 0,
            })
            .collect();

        let header: Vec<String> = table
            .head
            .rows
            .iter()
            .filter(|row| row.cells.iter().any(|cell| !cell.blocks.is_empty()))
            .flat_map(|row| self.multiline_cells(row, count))
            .collect();
        let body: Vec<Vec<String>> = table
            .head
            .rows
            .iter()
            .skip(usize::from(!header.is_empty()))
            .chain(table.bodies.iter().flat_map(|b| b.head.iter().chain(&b.body)))
            .chain(&table.foot.rows)
            .map(|row| self.multiline_cells(row, count))
            .collect();

        // **Without filling, a column widens to hold its widest cell.**
        // `--wrap=preserve` may not re-flow, so pandoc gives the column
        // `widest + 2` where that is more than the fraction asks for —
        // the simple table's rule, reappearing. With `--wrap=auto` the
        // fraction wins and the cell is filled to it.
        // **`Wrap::None` cannot fill either**, and it arrives as
        // `Some(usize::MAX)` rather than `None` — so asking
        // `columns.is_some()` called it a filling mode and left a cell
        // overflowing its column instead of widening it.
        let fills = self.columns.is_some_and(|columns| columns != usize::MAX);
        let widths: Vec<usize> = if fills {
            base
        } else {
            (0..count)
                .map(|index| {
                    let widest = std::iter::once(&header)
                        .chain(&body)
                        .filter_map(|cells| cells.get(index))
                        .map(|cell| cell.chars().count())
                        .max()
                        .unwrap_or(0);
                    base[index].max(widest + 2)
                })
                .collect()
        };

        let rule = |fill: char| {
            widths
                .iter()
                .map(|width| fill.to_string().repeat(*width))
                .collect::<Vec<_>>()
                .join(" ")
        };
        let full = "-".repeat(widths.iter().sum::<usize>() + count.saturating_sub(1));
        let lay = |cells: &[String]| Self::multiline_row(cells, &widths);

        if header.is_empty() {
            push_line(out, prefix, &format!("  {}", rule('-')));
        } else {
            push_line(out, prefix, &format!("  {full}"));
            for line in lay(&header) {
                push_line(out, prefix, &line);
            }
            push_line(out, prefix, &format!("  {}", rule('-')));
        }
        for (index, cells) in body.iter().enumerate() {
            if index > 0 {
                push_line(out, prefix, "");
            }
            for line in lay(cells) {
                push_line(out, prefix, &line);
            }
        }
        if body.len() == 1 {
            push_line(out, prefix, "");
        }
        if header.is_empty() {
            push_line(out, prefix, &format!("  {}", rule('-')));
        } else {
            push_line(out, prefix, &format!("  {full}"));
        }
        if !table.caption.blocks.is_empty() {
            push_line(out, prefix, "");
            let mut caption = String::new();
            self.blocks(&mut caption, &table.caption.blocks, "");
            push_line(out, prefix, &format!("  : {}", caption.trim()));
        }
    }

    /// Pandoc's **simple table**: no pipes and no plus signs, columns
    /// held apart by the space its own reader measures them with.
    ///
    /// Every rule here is `pandoc -f json -t markdown --columns=72` on a
    /// table built one shape at a time:
    ///
    /// * the whole table is indented **two spaces**;
    /// * a column is as wide as its widest cell **plus two**, with no
    ///   floor — a column of empty cells is two dashes wide;
    /// * cells are padded to that width by their column's alignment,
    ///   `AlignCenter` putting the odd space on the **right**, and joined
    ///   by a single space, with the trailing run trimmed;
    /// * a table with a header writes header, rule, rows; one **without**
    ///   writes rule, rows, rule, and a header row whose cells are all
    ///   empty counts as without;
    /// * a caption follows a blank line as `: text`.
    ///
    /// Width never forces a different shape — a column 70 characters wide
    /// is still written this way at `--columns=72`, and so is one whose
    /// text would wrap. What does force one is a cell holding more than a
    /// single block (a grid table) or a column stating its own width (a
    /// multiline table); [`simple_table_applies`] refuses both, and they
    /// keep the raw-HTML fallback until those writers exist.
    fn simple_table(&mut self, out: &mut String, table: &Table, prefix: &str, at: Position) {
        if at.first_in_container {
            // A simple table needs a blank line in front of it, and when
            // it opens a container there is none to reuse. The prefix
            // goes down **untrimmed** — `> `, not the `>` that separates
            // two blocks — and `corpus/gfm/tables.gfm` is one byte
            // different without that space.
            out.push_str(prefix);
            out.push('\n');
        }
        let columns = table.colspecs.len();
        let header: Vec<String> = table
            .head
            .rows
            .first()
            .map(|row| self.cells(row, columns, false, false))
            .unwrap_or_default();
        // A header of nothing but empty cells is not a header: pandoc
        // writes the same table it writes for a `head` with no rows.
        let has_header = header.iter().any(|cell| !cell.is_empty());

        let body: Vec<Vec<String>> = table
            .head
            .rows
            .iter()
            .skip(1)
            .chain(table.bodies.iter().flat_map(|b| b.head.iter().chain(&b.body)))
            .chain(&table.foot.rows)
            .map(|row| self.cells(row, columns, false, false))
            .collect();

        let width = |index: usize| {
            std::iter::once(&header)
                .chain(&body)
                .filter_map(|cells| cells.get(index))
                .map(|cell| cell.chars().count())
                .max()
                .unwrap_or(0)
                + 2
        };
        let widths: Vec<usize> = (0..columns).map(width).collect();
        let rule = widths
            .iter()
            .map(|width| "-".repeat(*width))
            .collect::<Vec<_>>()
            .join(" ");

        let row_line = |cells: &[String]| {
            let mut line = String::new();
            for (index, width) in widths.iter().enumerate() {
                if index > 0 {
                    line.push(' ');
                }
                let cell = cells.get(index).map_or("", String::as_str);
                let pad = width.saturating_sub(cell.chars().count());
                let left = match table.colspecs[index].alignment {
                    Alignment::AlignRight => pad,
                    Alignment::AlignCenter => pad / 2,
                    Alignment::AlignLeft | Alignment::AlignDefault => 0,
                };
                line.push_str(&" ".repeat(left));
                line.push_str(cell);
                // **Only the last cell drops its right padding**, and the
                // separator before it stays: a row whose final cell is
                // empty ends in the previous column's padding plus that
                // space, which is why `  1     2     ` has a trailing run
                // and `  one   two   three` has none.
                if index + 1 < widths.len() {
                    line.push_str(&" ".repeat(pad - left));
                }
            }
            format!("  {line}")
        };

        if has_header {
            push_line(out, prefix, &row_line(&header));
        }
        push_line(out, prefix, &format!("  {rule}"));
        for cells in &body {
            push_line(out, prefix, &row_line(cells));
        }
        if !has_header {
            push_line(out, prefix, &format!("  {rule}"));
        }
        if !table.caption.blocks.is_empty() {
            push_line(out, prefix, "");
            let mut caption = String::new();
            self.blocks(&mut caption, &table.caption.blocks, "");
            push_line(out, prefix, &format!("  : {}", caption.trim()));
        }
    }

    /// The blocks **pandoc's dialect spells** and the other two
    /// flavours have no syntax for, so only they fall back to raw HTML
    /// or to prose. Returns whether it wrote one.
    fn dialect_block(&mut self, out: &mut String, block: &Block, prefix: &str) -> bool {
        match block {
            // An empty line is `| ` — the marker with its space, which
            // `push_line` would trim away.
            Block::LineBlock(lines) => {
                for line in lines {
                    let text = self.inlines(line);
                    if text.is_empty() {
                        out.push_str(prefix);
                        out.push_str("| \n");
                    } else {
                        push_line(out, prefix, &format!("| {text}"));
                    }
                }
            }
            // **Not the `sourceCode` wrapper**, which the main match
            // unwraps: pandoc's HTML reader puts one round every
            // highlighted code block, and writing it as a fenced div
            // adds a `::: {#cb1 .sourceCode}` pandoc does not.
            Block::Div(attr, blocks)
                if attr.classes == ["sourceCode"]
                    && matches!(blocks.as_slice(), [Block::CodeBlock(..)]) =>
            {
                return false;
            }
            Block::Div(attr, blocks) => self.fenced_div(out, attr, blocks, prefix),
            Block::DefinitionList(items) => self.definition_list(out, items, prefix),
            // **A figure whose body is one image is written as one.**
            // `![caption](url)` with the figure's own attributes, and
            // `alt="…"` only where the image's alt text says something
            // the caption does not — equal texts, or an empty alt, write
            // no attribute at all. Anything else in the body (a second
            // block, or no image) keeps the raw-HTML fallback, which is
            // what pandoc writes for it too.
            Block::Figure(attr, caption, blocks) => {
                let [Block::Plain(inlines)] = blocks.as_slice() else {
                    return false;
                };
                let [Inline::Image(_, alt, target)] = inlines.as_slice() else {
                    return false;
                };
                let caption_text = self.inner(&caption_inlines(caption));
                let alt_text = self.inner(alt);
                let mut attr = attr.clone();
                if !alt_text.is_empty() && alt_text != caption_text {
                    attr.attributes.push(("alt".to_owned(), alt_text));
                }
                let suffix = attributes(&attr).unwrap_or_default();
                let line = format!(
                    "![{caption_text}]({}){suffix}",
                    link_destination(&target.url)
                );
                push_line(out, prefix, &line);
            }
            _ => return false,
        }
        true
    }

    /// The six inlines **pandoc's dialect spells** and the other two
    /// flavours have no syntax for, so only they fall back to raw HTML.
    ///
    /// Returns whether it wrote one. Each answer is
    /// `pandoc -f json -t markdown`, probed one construct at a time:
    ///
    /// ```text
    /// Strikeout    ~~x~~              Underline  [x]{.underline}
    /// Subscript    ~x~                SmallCaps  [x]{.smallcaps}
    /// Superscript  ^x^                Span       [x]{#id .class}
    /// ```
    ///
    /// A space inside the two script markers is `\ `; inside `~~` it is
    /// a space.
    fn dialect_inline(&mut self, out: &mut String, inline: &Inline) -> bool {
        match inline {
            Inline::Strikeout(inner) => self.marked(out, inner, "~~", false),
            Inline::Subscript(inner) => self.marked(out, inner, "~", true),
            Inline::Superscript(inner) => self.marked(out, inner, "^", true),
            Inline::Underline(inner) => self.bracketed(out, inner, "{.underline}"),
            Inline::SmallCaps(inner) => self.bracketed(out, inner, "{.smallcaps}"),
            // **A citation is written from its data, not its text.**
            // Pandoc regenerates the syntax and ignores the rendered
            // inlines entirely — `Cite [k] [Str "see", Emph "[@k]"]` is
            // `[@k]` — which is why writing the inner text escaped it
            // into `\[@k\]` and lost the citation.
            Inline::Cite(citations, _) => self.citation(out, citations),
            // A span carrying nothing is only a wrapper, in either dialect.
            Inline::Span(attr, inner) => match attributes(attr) {
                None => {
                    let text = self.inner(inner);
                    out.push_str(&text);
                }
                Some(attributes) => self.bracketed(out, inner, &attributes),
            },
            _ => return false,
        }
        true
    }

    /// A link, in whichever of the three spellings it has here.
    fn link(
        &mut self,
        out: &mut String,
        attr: &ferrodoc_ast::Attr,
        inner: &[Inline],
        target: &Target,
    ) {
                // The autolink escapes are spent inside `[…]` too, where
                // pandoc leaves them off. A link cannot nest, so this
                // looks like waste — but **pandoc's own reader linkifies
                // inside link text**: it reads its own
                // `[www.example.com](http://www.example.com)` back as a
                // `Link` wrapped around a `Link`. `corpus/gfm/
                // extensions.gfm` is the document, and the escape is what
                // keeps `diff-gfm-md` at 100.
                let text = self.inner(inner);
                if write_autolink(out, inner, target) {
                    return;
                }
                // **A link carrying attributes has no CommonMark
                // spelling**, so pandoc writes the raw `<a>` — attributes
                // and all — rather than dropping them. The dialect has
                // `{#i .c}` and falls through.
                //
                // Asked **after** the autolink, which carries a `uri`
                // class of its own: checked first, every `<https://…>`
                // came out as an `<a>` element.
                if !self.pandoc() && attributes(attr).is_some() {
                    let mut html = format!(" href=\"{}\"", target.url);
                    if !target.title.is_empty() {
                        let _ = write!(html, " title=\"{}\"", target.title);
                    }
                    html.push_str(&html_attributes(attr));
                    self.tagged(out, "a", &html, inner);
                    return;
                }
                let _ = write!(out, "[{text}]({}", link_destination(&target.url));
                if !target.title.is_empty() {
                    let _ = write!(out, " \"{}\"", target.title.replace('"', "\\\""));
                }
                out.push(')');
                if self.pandoc() {
                    if let Some(a) = attributes(attr) {
                        out.push_str(&a);
                    }
                }
    }

    /// A code span, with the delimiter and padding its content needs.
    fn code_span(&mut self, out: &mut String, attr: &ferrodoc_ast::Attr, text: &str) {
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
                //
                // It does **not** pad for a space at one end only — it
                // writes `` `#include ` `` where this wrote
                // `` ` #include  ` ``, and nothing is stripped on the way
                // back either way, so the shorter is the one to match.
                // A space at **both** ends is different: there the reader
                // strips one from each, so pandoc's bare form loses them
                // and the padding is what returns the text. `ROADMAP.md`
                // is where the one-sided case showed.
                let spaced = text.starts_with(' ') && text.ends_with(' ');
                let pad = if !all_spaces && (ticks.len() > 1 || spaced)
                {
                    " "
                } else {
                    ""
                };
                let _ = write!(out, "{ticks}{pad}{text}{pad}{ticks}");
                // **The dialect writes a code span's attributes**, which
                // `CommonMark` has nowhere to put and drops.
                if self.pandoc() {
                    if let Some(a) = attributes(attr) {
                        out.push_str(&a);
                    }
                }
    }

    /// An ordered list, with the marker its style and delimiter ask for.
    fn ordered_list(
        &mut self,
        out: &mut String,
        attrs: &ferrodoc_ast::ListAttributes,
        items: &[Vec<Block>],
        prefix: &str,
    ) {
                // **`fancy_lists` is a dialect feature.** CommonMark
                // numbers every ordered list and has no `(a)` at all, so
                // the roman and alphabetic styles degrade to numbers
                // there and only there.
                let (start, delim) = (attrs.start, attrs.delim);
                let dialect = self.pandoc();
                let style = if dialect { attrs.style } else { ListNumberStyle::Decimal };
                self.list(out, items, prefix, move |index| {
                    let n = start + i64::try_from(index).unwrap_or(0);
                    let label = style.label(n);
                    let marker = match delim {
                        // `(1)` is the dialect's; CommonMark has no such
                        // marker at all, so both parenthesised delimiters
                        // collapse to `1)` there.
                        ListNumberDelim::TwoParens if dialect => format!("({label})"),
                        ListNumberDelim::OneParen | ListNumberDelim::TwoParens => {
                            format!("{label})")
                        }
                        _ => format!("{label}."),
                    };
                    // Pandoc pads the marker to four columns, or to one
                    // past its own length when that is wider: `1.  `,
                    // `10. `, `100. `. The continuation indent follows
                    // from it, because the indent is the marker's width.
                    format!("{marker:<width$}", width = marker.len().max(3) + 1)
                });
    }

    /// Pandoc's citation syntax, built from the citations themselves.
    ///
    /// Measured one shape at a time against `pandoc -f json -t markdown`:
    ///
    /// ```text
    /// NormalCitation           [@k]
    /// AuthorInText             @k
    /// SuppressAuthor           [-@k]
    /// prefix and suffix        [see @k p. 3]
    /// two keys                 [@k; @k2]
    /// AuthorInText + suffix    @k [p. 3]
    /// AuthorInText then one    @k [@k2]
    /// ```
    ///
    /// So an `AuthorInText` **first** citation is written bare and
    /// everything after it goes in one bracket; anything else brackets
    /// the lot.
    fn citation(&mut self, out: &mut String, citations: &[ferrodoc_ast::Citation]) {
        let one = |writer: &mut Self, c: &ferrodoc_ast::Citation| {
            let mut text = String::new();
            if !c.citation_prefix.is_empty() {
                text.push_str(&writer.inner(&c.citation_prefix));
                text.push(' ');
            }
            if c.citation_mode == ferrodoc_ast::CitationMode::SuppressAuthor {
                text.push('-');
            }
            let _ = write!(text, "@{}", c.citation_id);
            if !c.citation_suffix.is_empty() {
                text.push(' ');
                text.push_str(&writer.inner(&c.citation_suffix));
            }
            text
        };
        let Some((first, rest)) = citations.split_first() else {
            return;
        };
        if first.citation_mode == ferrodoc_ast::CitationMode::AuthorInText {
            let _ = write!(out, "@{}", first.citation_id);
            let mut tail: Vec<String> = Vec::new();
            if !first.citation_suffix.is_empty() {
                tail.push(self.inner(&first.citation_suffix));
            }
            for c in rest {
                tail.push(one(self, c));
            }
            if !tail.is_empty() {
                let _ = write!(out, " [{}]", tail.join("; "));
            }
            return;
        }
        let all: Vec<String> = citations.iter().map(|c| one(self, c)).collect();
        let _ = write!(out, "[{}]", all.join("; "));
    }

    /// A marker on both sides: `~~struck~~`, `~sub~`, `^sup^`.
    ///
    /// `tight` is what the two script markers need — they cannot hold a
    /// bare space, so pandoc writes `^a\ b^` — and what `~~` does not.
    fn marked(&mut self, out: &mut String, inner: &[Inline], marker: &str, tight: bool) {
        let text = self.nested(inner, false);
        // A break opportunity is still `BREAK` at this point — it becomes
        // a space only when the line is laid out — so both spellings of
        // the same space have to be caught, and **in one pass**: chaining
        // two `replace` calls escapes the space the first one just wrote
        // and gives `~a\\ b~`.
        let text = if tight { text.replace([BREAK, ' '], "\\ ") } else { text };
        let _ = write!(out, "{marker}{text}{marker}");
    }

    /// `[text]{#id .class}`, which is how the dialect carries an
    /// attribute an inline would otherwise need a `<span>` for.
    fn bracketed(&mut self, out: &mut String, inner: &[Inline], attributes: &str) {
        let text = self.nested(inner, false);
        let _ = write!(out, "[{text}]{attributes}");
    }

    /// One row's cells, rendered to single-line text with spans expanded.
    ///
    /// `pipes` says whether a `|` has to be escaped, which is a property
    /// of the **table shape** rather than of the text: a pipe table ends
    /// a cell at one and a simple table does not, so pandoc writes
    /// `` `x|y` `` bare in the second and `` `x\|y` `` in the first.
    fn cells(&mut self, row: &Row, columns: usize, pipes: bool, breaks: bool) -> Vec<String> {
        let mut out: Vec<String> = Vec::with_capacity(columns);
        for cell in &row.cells {
            let mut text = String::new();
            for block in &cell.blocks {
                let mut piece = String::new();
                self.block(&mut piece, block, "", Position::default());
                let piece = piece.trim_end_matches('\n');
                if !text.is_empty() && !piece.is_empty() {
                    text.push(' ');
                }
                text.push_str(piece);
            }
            out.push(cell_text(&text, pipes, breaks));
            // A spanned cell occupies further columns that carry nothing.
            for _ in 1..cell.col_span.max(1) {
                out.push(String::new());
            }
        }
        out
    }

    /// Blocks `CommonMark` has no syntax for: emit their content rather
    /// than dropping it.
    /// A block with no markdown spelling at all. Pandoc writes the raw
    /// HTML for these rather than losing them, and the HTML is this
    /// crate's own writer's, byte for byte.
    ///
    /// A table used to come out as **one paragraph per cell** here,
    /// which destroyed the row-and-column relationship the document was
    /// about — "not recoverable afterwards", as `COMPATIBILITY.md` said
    /// while it was true. The same argument had already been accepted
    /// for superscript, small caps and a span carrying attributes: they
    /// degraded to their content until raw HTML replaced it. This is
    /// that argument, carried to the two blocks it had not reached.
    fn unrepresentable(out: &mut String, block: &Block, prefix: &str) {
        let html = ferrodoc_html::write_html(&Pandoc::new(vec![block.clone()]));
        for line in html.trim_end_matches('\n').split('\n') {
            push_line(out, prefix, line);
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
            // When filling, the item's content is rendered under the
            // whole of what will sit to its left — the enclosing
            // `prefix` **and** this item's own indent — so the fill pays
            // for both: pandoc puts `- word word word` on a 20-column
            // line, not `- word word word word`. Counting only the
            // indent left a nested item's line as wide as the outer
            // list's prefix, 73 columns against 72 at one level of
            // nesting, which no gate could see because `writers.sh`
            // compares at `--wrap=preserve`. When not filling the prefix
            // changes nothing, so it is left empty and the indent is
            // added per line below.
            let nested_prefix = format!("{prefix}{indent}");
            let inner = if self.columns.is_some() { nested_prefix.as_str() } else { "" };
            let mut body = String::new();
            self.depth += 1;
            // A code block cannot open a list item as four spaces,
            // whatever else the item holds: the marker is padded to its
            // own column (`3.  `), and four spaces past a padded marker
            // read back one space wider than they were written. An item
            // of one block was exempt from this until 2026-08-25, and
            // `corpus/truncation-cases.md` is where the lost space
            // showed — in this writer's own round trip, not pandoc's.
            // `CommonMark` examples 273, 274 and 324 are the rest.
            self.at.item_start = true;
            if tight {
                let mut previous: Option<&Block> = None;
                for block in item {
                    let at = Position {
                        after_container: matches!(
                            previous,
                            Some(
                                Block::BulletList(_)
                                    | Block::OrderedList(..)
                                    | Block::BlockQuote(_)
                            )
                        ),
                        first_in_item: self.at.item_start,
                        // A tight item's blocks are written into `body`
                        // with no prefix of their own; the marker and the
                        // indent are added afterwards, so there is no
                        // container here to open with a blank line.
                        first_in_container: false,
                    };
                    self.at.item_start = false;
                    self.block(&mut body, block, inner, at);
                    previous = Some(block);
                }
            } else {
                self.blocks(&mut body, item, inner);
            }
            self.at.item_start = false;
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
                    // `inner` already carries `prefix`, so adding it
                    // again would indent the line twice.
                    push_line(out, "", line);
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
        let text = self.inner(inner);
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
        let text = self.inner(inner);
        let _ = write!(out, "<{tag}{attributes}>{text}</{tag}>");
    }

    fn strikeout(&mut self, out: &mut String, inner: &[Inline]) {
        let text = self.inner(inner);
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

    /// The content of a container — emphasis, a link's text, a heading.
    /// Whatever opens it is already on the line, so nothing inside can
    /// open a block.
    fn inner(&mut self, inlines: &[Inline]) -> String {
        let was = self.at.preceded;
        self.at.preceded = true;
        let text = self.inlines(inlines);
        self.at.preceded = was;
        text
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
        if self.pandoc() && self.dialect_inline(out, inline) {
            return;
        }
        match inline {
            Inline::Str(text) => escape_text(out, text, self.flavour, self.at.preceded),
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
                // Pandoc's dialect always writes the backslash; the two
                // spaces are `CommonMark`'s spelling and are invisible in
                // a diff, which is why pandoc does not use them.
                if self.pandoc() || out.is_empty() || out.ends_with('\n') {
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
            Inline::Strikeout(inner) if self.gfm() => self.strikeout(out, inner),
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
            Inline::SmallCaps(i) => self.tagged(out, "span", " class=\"smallcaps\"", i),
            Inline::Span(attr, inner) => {
                let attributes = html_attributes(attr);
                // A span carrying nothing is only a wrapper; pandoc writes
                // no tag for it either.
                if attributes.is_empty() {
                    let text = self.inner(inner);
                    out.push_str(&text);
                } else {
                    self.tagged(out, "span", &attributes, inner);
                }
            }
            // A citation renders as the text it stands for: pandoc's
            // `citeproc` is what turns one into a reference, and this is
            // not that.
            Inline::Cite(_, inner) => {
                let text = self.inner(inner);
                out.push_str(&text);
            }
            Inline::Quoted(quote, inner) => {
                // **The dialect writes straight quotes**, because its own
                // reader turns them back into `Quoted` — `"a 'b'"` reads
                // as a double quote around a single one. `CommonMark` has
                // no such reader, so a straight quote there would come
                // back as a `Str` and the markup would be lost; it gets
                // the curly characters the quotes actually stand for.
                let (open, close) = match (quote, self.pandoc()) {
                    (QuoteType::SingleQuote, true) => ('\'', '\''),
                    (QuoteType::DoubleQuote, true) => ('"', '"'),
                    (QuoteType::SingleQuote, false) => ('\u{2018}', '\u{2019}'),
                    (QuoteType::DoubleQuote, false) => ('\u{201C}', '\u{201D}'),
                };
                out.push(open);
                out.push_str(&self.inner(inner));
                out.push(close);
            }
            Inline::Code(attr, text) => self.code_span(out, attr, text),
            Inline::Math(kind, text) => {
                // Verbatim: escaping corrupts the TeX, since `\\` is a
                // MathJax line break and `\_` a literal underscore. Probed
                // against pandoc's `ipynb` writer, whose spelling this is.
                // Pandoc's `gfm` writer instead emits GitHub's `` $`x`$ `` and
                // a ```` ```math ```` fence; one writer serves both here, and
                // the dollar form is the one both readers accept.
                write_math(out, *kind, text, self.flavour == Flavour::Gfm);
            }
            Inline::Link(attr, inner, target) => self.link(out, attr, inner, target),
            Inline::Image(_, alt, target) => {
                let text = self.inner(alt);
                let _ = write!(out, "![{text}]({}", link_destination(&target.url));
                if !target.title.is_empty() {
                    let _ = write!(out, " \"{}\"", target.title.replace('"', "\\\""));
                }
                out.push(')');
            }
            Inline::RawInline(format, text) => {
                // **Pandoc's dialect can say what raw text is**, and does:
                // `` `<em>`{=html} ``. `CommonMark` has no way to mark it,
                // so it goes out bare and is read back as markup.
                if self.pandoc() {
                    let longest = text.split(|c| c != '`').map(str::len).max().unwrap_or(0);
                    let ticks = "`".repeat(longest + 1);
                    let padded = text.starts_with('`') || text.ends_with('`');
                    let space = if padded { " " } else { "" };
                    let _ = write!(out, "{ticks}{space}{text}{space}{ticks}{{={}}}", format.0);
                } else if format.0 == "html" {
                    out.push_str(text);
                }
            }
            Inline::Note(blocks) => self.note(out, blocks),
        }
    }
}

/// Whether a list item opens with the `☐`/`☒` a GFM task item reads as,
/// and if so whether it is checked.
fn task_state(item: &[Block]) -> Option<bool> {
    let (Block::Plain(inlines) | Block::Para(inlines)) = item.first()? else {
        return None;
    };
    // **A marker with nothing after it stays a task item here, and does
    // not in pandoc** — one of the few places these writers differ on
    // purpose. Pandoc writes `- ☐` for a lone ballot character, and its
    // own reader then splits the list at it: the three-item list in
    // `corpus/gfm/task-list-runs.gfm` comes back as a list of one and a
    // list of two. `- [ ] ` comes back whole, so this writes that and
    // takes the byte difference. COMPATIBILITY.md records it.
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
/// A caption's inlines, when it is the single paragraph a figure's is.
fn caption_inlines(caption: &ferrodoc_ast::Caption) -> Vec<Inline> {
    match caption.blocks.as_slice() {
        [Block::Plain(inlines) | Block::Para(inlines)] => inlines.clone(),
        _ => Vec::new(),
    }
}

/// Whether a table has a **multiline table** spelling: the same cells a
/// simple table needs, and at least one column stating its own width.
fn multiline_table_applies(table: &Table) -> bool {
    !table.colspecs.is_empty()
        && table
            .colspecs
            .iter()
            .any(|spec| spec.width != ferrodoc_ast::ColWidth::ColWidthDefault)
        && cells_are_single_blocks(table)
}

/// Whether a table has a **simple table** spelling in pandoc's dialect.
///
/// Pandoc picks among four table shapes, and the two this refuses are
/// the two it would write differently — probed one shape at a time
/// against `pandoc -f json -t markdown`:
///
/// * a cell holding **more than one block** gets a grid table, and this
///   writer's [`Writer::cells`] would join those blocks with a space;
/// * a column stating its **own width** (`ColWidth`, which is what
///   reading a pandoc simple or multiline table produces) gets a
///   multiline table, with a full-width rule above and below.
///
/// A spanned cell is refused for the same reason: a simple table has
/// nowhere to put one. Everything refused here keeps the raw-HTML
/// fallback, which loses nothing.
/// A fenced div's header, after the `::: `.
///
/// The same shape as a fenced code block's info string: a bare name when
/// there is exactly **one** class and nothing else, and the whole
/// attribute set otherwise — except that a div carrying *nothing* is
/// `::: {}` rather than a bare `:::`, where a code block with no
/// attributes has no info string at all. Probed one attribute set at a
/// time against `pandoc -f json -t markdown`.
fn fence_attributes(attr: &ferrodoc_ast::Attr) -> String {
    match attr.classes.as_slice() {
        [class] if attr.identifier.is_empty() && attr.attributes.is_empty() => class.clone(),
        _ => attributes(attr).unwrap_or_else(|| "{}".to_owned()),
    }
}

fn simple_table_applies(table: &Table) -> bool {
    if table.colspecs.is_empty() {
        return false;
    }
    if table.colspecs.iter().any(|spec| spec.width != ferrodoc_ast::ColWidth::ColWidthDefault) {
        return false;
    }
    cells_are_single_blocks(table)
}

/// Whether every cell holds at most one `Plain` or `Para` and spans one
/// row and one column — what both the simple and the multiline table
/// need, and what sends anything else to a grid table.
fn cells_are_single_blocks(table: &Table) -> bool {
    let rows = table
        .head
        .rows
        .iter()
        .chain(table.bodies.iter().flat_map(|b| b.head.iter().chain(&b.body)))
        .chain(&table.foot.rows);
    rows.flat_map(|row| &row.cells).all(|cell| {
        cell.col_span.max(1) == 1
            && cell.row_span.max(1) == 1
            && cell.blocks.len() <= 1
            && cell.blocks.iter().all(|b| matches!(b, Block::Plain(_) | Block::Para(_)))
    })
}

fn pipe_row(cells: &[String], widths: &[usize], alignments: &[Alignment]) -> String {
    let mut line = String::from("|");
    for (index, width) in widths.iter().enumerate() {
        let cell = cells.get(index).map_or("", String::as_str);
        let slack = width.saturating_sub(cell.chars().count());
        let (before, after) = match alignments.get(index) {
            Some(Alignment::AlignRight) => (slack, 0),
            Some(Alignment::AlignCenter) => (slack / 2, slack - slack / 2),
            _ => (0, slack),
        };
        let _ = write!(line, " {}{cell}{} |", " ".repeat(before), " ".repeat(after));
    }
    line
}

/// Cell content reduced to what fits between two pipes: a bare `|` would
/// end the cell and a newline would end the row. A reader unescapes `\|`
/// before it parses the cell, so the escape survives even inside a code
/// span.
fn cell_text(text: &str, pipes: bool, breaks: bool) -> String {
    let mut out = String::with_capacity(text.len());
    let mut backslashes = 0usize;
    for ch in text.chars() {
        match ch {
            // Text has already been escaped once; escaping it again would
            // leave a literal backslash and a live cell divider behind.
            // Only the pipes markup carries — inside a code span, a raw
            // inline, a URL — arrive here bare.
            '|' if !pipes || backslashes % 2 == 1 => out.push('|'),
            '|' => out.push_str("\\|"),
            // A row is one line, so a break opportunity in a cell is a
            // space and nothing else — pandoc does not fill inside a
            // pipe table either.
            // A **pipe table** row is one line, so a break opportunity in
            // a cell is a space. A **multiline** table fills its cells to
            // a column, so the opportunity has to survive to be filled at.
            '\n' | BREAK if !breaks => out.push(' '),
            BREAK => out.push(BREAK),
            '\n' => out.push(' '),
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

/// **A word that would re-open a block if a line began with it.**
///
/// This is not a matter of matching pandoc's bytes. A fill that leaves a
/// bare `+` at the start of a line writes a document that **does not read
/// back as itself**: one paragraph came back as a `Para` and a
/// `BulletList`, because `+ 8 = 40, …` is a bullet item.
///
/// The set is `CommonMark`'s paragraph interrupters, measured against
/// pandoc one character at a time: `+`, `-`, `*` and `>`; `1.` and `1)`
/// but **not `2.`, `12.` or `2)`**, because an ordered list may only
/// interrupt a paragraph when it starts at one. `#`, `=`, `~` and `%` are
/// allowed, which a reading of the spec would not predict.
///
/// `ferrodoc-text` carries the same predicate for the plain writer.
fn reopens_a_block(word: &str) -> bool {
    matches!(word, "+" | "-" | "*" | ">" | "1." | "1)")
}

/// Greedy fill: take words while they fit, break at the last space that
/// did. A word longer than the width goes on its own line and overruns —
/// breaking inside it would invent a break the text does not have.
///
/// With **one word of lookahead**: a word is refused when taking it would
/// push a block-reopening word onto the start of the next line.
fn fill(text: &str, width: usize) -> Vec<String> {
    let mut lines = Vec::new();
    let mut line = String::new();
    let mut line_width = 0;
    let words: Vec<&str> = text.split(BREAK).collect();
    for (index, word) in words.iter().enumerate() {
        let word_width = word.chars().count();
        let after = line_width + 1 + word_width;
        let strands_the_next = words.get(index + 1).is_some_and(|next| {
            reopens_a_block(next) && after + 1 + next.chars().count() > width
        });
        if line.is_empty() {
            line.push_str(word);
            line_width = word_width;
        } else if after <= width && !strands_the_next {
            line.push(' ');
            line.push_str(word);
            line_width = after;
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
/// The text of a run of inlines with its markup removed, which is what a
/// heading's identifier is derived from.
fn plain_text(inlines: &[Inline]) -> String {
    let mut out = String::new();
    for inline in inlines {
        match inline {
            Inline::Str(text) | Inline::Code(_, text) | Inline::Math(_, text) => out.push_str(text),
            Inline::Space | Inline::SoftBreak | Inline::LineBreak => out.push(' '),
            Inline::Emph(inner)
            | Inline::Strong(inner)
            | Inline::Strikeout(inner)
            | Inline::Superscript(inner)
            | Inline::Subscript(inner)
            | Inline::SmallCaps(inner)
            | Inline::Underline(inner)
            | Inline::Span(_, inner)
            | Inline::Quoted(_, inner)
            | Inline::Cite(_, inner)
            | Inline::Link(_, inner, _)
            | Inline::Image(_, inner, _) => out.push_str(&plain_text(inner)),
            Inline::RawInline(..) | Inline::Note(_) => {}
        }
    }
    out
}

/// A `{#id .class key="value"}` for pandoc's dialect, or `None` when the
/// attribute carries nothing worth writing.
///
/// Probed: `# T {#myid}`. `CommonMark` has nowhere to put any of this, so
/// it drops the lot and the identifier is regenerated on the next read.
fn attributes(attr: &ferrodoc_ast::Attr) -> Option<String> {
    if attr.identifier.is_empty() && attr.classes.is_empty() && attr.attributes.is_empty() {
        return None;
    }
    let mut out = String::from("{");
    if !attr.identifier.is_empty() {
        let _ = write!(out, "#{}", attr.identifier);
    }
    for class in &attr.classes {
        if !out.ends_with('{') {
            out.push(' ');
        }
        let _ = write!(out, ".{class}");
    }
    for (key, value) in &attr.attributes {
        if !out.ends_with('{') {
            out.push(' ');
        }
        let _ = write!(out, "{key}=\"{value}\"");
    }
    out.push('}');
    Some(out)
}

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
    // A trailing `#` run is the heading's closing sequence **only when a
    // space comes before it**, and `escape_text` escapes exactly the `#`
    // that begins a word — so the run that needs stopping is already
    // stopped and `bar###`, which needs nothing, is left alone. Escaping
    // it here as well wrote `\\###` for one and `bar\###` for the other.
    let text = text.clone();
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
fn write_math(out: &mut String, kind: MathType, text: &str, github: bool) {
    // **GitHub has its own math syntax and pandoc writes it**: `` $`x`$ ``
    // inline and a ```` ```math ```` fence for display, neither of which
    // is the dollar form the other two dialects take. One spelling served
    // all three here, on the note that "the dollar form is the one both
    // readers accept" — true, and not what pandoc writes.
    if github {
        match kind {
            MathType::InlineMath => {
                let _ = write!(out, "$`{text}`$");
            }
            MathType::DisplayMath => {
                let _ = write!(out, "``` math\n{text}\n```");
            }
        }
        return;
    }
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
fn escape_text(out: &mut String, text: &str, flavour: Flavour, preceded: bool) {
    let gfm = flavour.is_gfm();
    // **Pandoc's dialect un-smartens as it writes.** It reads `---` as an
    // em-dash, so it writes an em-dash back out as `---` and the document
    // round trips; and it escapes the three characters its own reader
    // would otherwise turn into something else. Every one probed by
    // handing pandoc a JSON AST and reading the bytes back.
    escape_text_inner(out, text, gfm, preceded, flavour == Flavour::Pandoc);
}

fn escape_text_inner(
    out: &mut String,
    text: &str,
    gfm: bool,
    preceded: bool,
    pandoc: bool,
) {
    let mut chars = text.chars().peekable();
    let mut escaped_bang = false;
    while let Some(ch) = chars.next() {
        let next = chars.peek().copied();
        let after_bang = std::mem::take(&mut escaped_bang);
        let at_line_start = out.is_empty() || out.ends_with('\n');
        // **Six characters pandoc's dialect escapes and `CommonMark`
        // does not**, because its own reader would otherwise read them as
        // a quote, a subscript, a superscript, a table or an escape:
        // `'`, `"`, `~`, `^`, `|` and `\`. The pipe escapes *everywhere*,
        // not only in a table — `a | b` in an ordinary paragraph comes
        // back `a \| b` — and so do the two script markers: `2^n` is
        // `2\^n` in the middle of a word, with no closing `^` anywhere.
        // A literal backslash is `\\`, which `CommonMark` and `gfm`
        // both leave bare. Probed one at a time.
        // **The un-smartening writes its dashes unescaped**, and a
        // literal one is escaped — which is the whole reason these two
        // decisions have to be made in the same pass. Substituting first
        // and escaping afterwards turned every em-dash into `\-\--`.
        if pandoc && matches!(ch, '—' | '–' | '…' | '\u{2018}' | '\u{2019}' | '\u{201C}' | '\u{201D}') {
            out.push_str(match ch {
                '—' => "---",
                '–' => "--",
                '…' => "...",
                // **The curly quotes un-smarten too**, and land here
                // rather than in the escape below for the same reason
                // the dashes do: a *literal* `'` is escaped, because the
                // reader would smarten it, and one written *for* a `’`
                // must not be — it is meant to come back curly.
                '\u{2018}' | '\u{2019}' => "'",
                _ => "\"",
            });
            continue;
        }
        if pandoc && matches!(ch, '\'' | '"' | '~' | '^' | '|' | '\\') {
            out.push('\\');
            out.push(ch);
            continue;
        }
        // **A `-` followed by another is escaped**, wherever it stands —
        // `a---b` comes back `a\-\--b` and `a - b` is untouched. Every
        // dash of a run but the last, which is the same rule seen from
        // the other end, and it is not the line-start rule `CommonMark`
        // needs: this fires in the middle of a word.
        if pandoc && ch == '-' && next == Some('-') {
            out.push_str("\\-");
            continue;
        }
        // A heading's text cannot **open** a block: the `## ` in front of
        // it has already decided what the line is, so the escapes that
        // exist to stop a list or a setext rule are dead weight there.
        // `#` is the exception and still escapes — more hashes on a
        // heading line are more heading, which is a real ambiguity.
        let opens_here = at_line_start && !preceded;
        // A marker only opens a block when the line breaks after it, so
        // `-b` is text and `- b` is a list. Pandoc splits on exactly that.
        let opens_block = next.is_none_or(|c| c == ' ');
        match ch {
            // The `\!` just written is what stops the image, so the
            // bracket behind it stands: pandoc writes `\![CDATA\[`, not
            // `\!\[CDATA\[`, and an ODT holding a CDATA section is where
            // the difference showed up.
            '[' if after_bang => out.push(ch),
            // **`CommonMark` alone leaves `$` bare.** It opens math in
            // the dialect and gfm escapes it too, so only the strict
            // reader treats it as ordinary text — measured all three
            // ways, after a first fix that turned it off for gfm as
            // well and traded one wrong writer for another.
            '$' if !gfm && !pandoc => out.push(ch),
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
            // `|` divides a table row anywhere.
            '|' if gfm => {
                out.push('\\');
                out.push(ch);
            }
            // A `~` that **could pair with a later one**, which is what
            // it takes to open strikeout. Pandoc escapes only a doubled
            // tilde, and it can afford to because its own reader needs
            // two — this reader strikes on one, deliberately, because
            // GitHub does (`COMPATIBILITY.md` lists it among the reader
            // divergences that follow GitHub rather than pandoc).
            //
            // The cost of that decision lands here, and this is as narrow
            // as it can be made without changing it: every `~` was
            // escaped, so `~/path` and `2 ~ 3` carried a backslash they
            // did not need. A lone tilde has nothing to pair with, and a
            // pair split across two `Str`s cannot strike either — a
            // delimiter run must flank its content.
            '~' if gfm && chars.clone().any(|c| c == '~') => {
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
            // `#` opens a heading at a line start, and pandoc escapes it
            // **wherever it begins a word** — `a \#b` but not `a#b`,
            // `a# b` or `C#`. `docs/divergences.md` writes `| # | group |`
            // in a table row that `CommonMark` reads as text.
            '#' if at_line_start || out.ends_with([' ', '\n', BREAK]) => {
                out.push('\\');
                out.push(ch);
            }
            // `-` and `+` open a list only with a space after them — but
            // a *run* of `-` or `=` under a paragraph line is a setext
            // heading, which is not a block opener at all and is why this
            // is wider than pandoc's rule. Pandoc loses `Foo\nbar\n\---`
            // to a heading; `CommonMark` example 106 is that document.
            '-' | '+' if opens_here && (opens_block || next == Some(ch)) => {
                out.push('\\');
                out.push(ch);
            }
            // A setext underline needs a paragraph line **above** it, so
            // a run of `=` is only dangerous on a continuation line —
            // `out.is_empty()` is the block's first line, where pandoc
            // does not escape and neither should this. It escaped `===
            // x` there, which cannot be an underline at all.
            '=' if opens_here && !out.is_empty() && next == Some('=') => {
                out.push('\\');
                out.push(ch);
            }
            // `1.` and `1)` open an ordered list, but only where the line
            // so far is nothing but the number.
            '.' | ')' if !preceded && digits_since_line_start(out) && opens_block => {
                out.push('\\');
                out.push(ch);
            }
            // Only dangerous immediately before a link. Pandoc writes
            // `\![` and lets the bracket stand, relying on the escaped
            // `]` to stop a link forming — which fails when the very next
            // inline **is** a link and supplies the `](url)` itself.
            // `CommonMark` example 593 is that document.
            '!' if next == Some('[') => {
                out.push_str("\\!");
                escaped_bang = true;
            }
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
mod dialect {
    use super::*;
    use ferrodoc_ast::Attr;

    /// **A pandoc-dialect document written back as itself.** Every
    /// expectation here is `pandoc -t markdown` output, run and pasted.
    ///
    /// These four constructs are the ones `CommonMark` cannot say, so
    /// `writers.sh` can never reach them: its corpus is read as
    /// `CommonMark`, and a reader that cannot produce a `Note` gives the
    /// writer no `Note` to get wrong. The gate is blind here by
    /// construction, which is exactly when a test earns its place.
    #[test]
    fn the_constructs_commonmark_cannot_say() {
        let doc = Pandoc::new(vec![
            Block::Header(
                1,
                Attr { identifier: "custom-id".into(), ..Attr::default() },
                vec![Inline::Str("Heading".into())],
            ),
            Block::Para(vec![
                Inline::Str("Text".into()),
                Inline::Note(vec![Block::Para(vec![Inline::Str("body".into())])]),
            ]),
            Block::DefinitionList(vec![(
                vec![Inline::Str("Term".into())],
                vec![vec![Block::Plain(vec![Inline::Str("Tight".into())])]],
            )]),
        ]);
        assert_eq!(
            write_pandoc_markdown(&doc),
            "# Heading {#custom-id}\n\nText[^1]\n\nTerm\n:   Tight\n\n[^1]: body\n"
        );
    }

    /// **An identifier the reader would regenerate is not written.**
    /// Pandoc omits `{#my-heading}` on `# My Heading` and keeps
    /// `{#custom}`, because the first comes back on its own and the
    /// second does not — probed three ways round.
    ///
    /// No gate reaches this either: `writers.sh` reads its corpus as
    /// `CommonMark`, whose reader generates no identifiers at all, so
    /// every heading it produces has an empty one.
    #[test]
    fn a_regenerable_heading_identifier_is_not_written() {
        let heading = |id: &str| {
            Pandoc::new(vec![Block::Header(
                1,
                Attr { identifier: id.into(), ..Attr::default() },
                vec![Inline::Str("My".into()), Inline::Space, Inline::Str("Heading".into())],
            )])
        };
        assert_eq!(write_pandoc_markdown(&heading("my-heading")), "# My Heading\n");
        assert_eq!(write_pandoc_markdown(&heading("custom")), "# My Heading {#custom}\n");
        assert_eq!(
            write_pandoc_markdown(&heading("my-heading-x")),
            "# My Heading {#my-heading-x}\n"
        );
    }

    /// A task item is `- [ ]` in pandoc's dialect as in GFM; only
    /// `CommonMark` falls back to the `☐` its reader handed over.
    #[test]
    fn a_task_item_keeps_its_brackets() {
        let list = Pandoc::new(vec![Block::BulletList(vec![vec![Block::Plain(vec![
            Inline::Str("☐".into()),
            Inline::Space,
            Inline::Str("todo".into()),
        ])]])]);
        assert_eq!(write_pandoc_markdown(&list), "- [ ] todo\n");
        assert_eq!(write_markdown(&list), "- ☐ todo\n");
    }

    /// The text rules, and the one that has to share a pass with another:
    /// an em-dash is written `---` **unescaped** while a literal `-`
    /// before another `-` is escaped. Doing the substitution first and
    /// the escaping second turned every em-dash into `\-\--`.
    #[test]
    fn the_text_rules_share_one_pass() {
        let para = |text: &str| Pandoc::new(vec![Block::Para(vec![Inline::Str(text.into())])]);
        assert_eq!(write_pandoc_markdown(&para("a—b")), "a---b\n");
        assert_eq!(write_pandoc_markdown(&para("a---b")), "a\\-\\--b\n");
        assert_eq!(write_pandoc_markdown(&para("it's")), "it\\'s\n");
        assert_eq!(write_pandoc_markdown(&para("a | b")), "a \\| b\n");
        assert_eq!(write_pandoc_markdown(&para("a - b")), "a - b\n");
        // And `CommonMark` leaves every one of them alone.
        assert_eq!(write_markdown(&para("a—b")), "a—b\n");
    }
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
        // **Only a line that could close the fence lengthens it**, and
        // pandoc sizes one by the longest line that is one unbroken run
        // of backticks once its leading and trailing spaces and tabs are
        // dropped. This matched that description in a comment and not in
        // the code until 2026-08-26 — it counted a run *anywhere*, which
        // is strictly wider — and the assertion below said `````` `````
        // `` for a block pandoc writes with three. Five was never
        // pandoc's: it was this writer's own output written down as the
        // expected value.
        let block = |body: &str| {
            let attr = ferrodoc_ast::Attr {
                identifier: String::new(),
                classes: vec!["sourceCode".to_owned(), "bash".to_owned()],
                attributes: Vec::new(),
            };
            write_gfm(&Pandoc::new(vec![Block::CodeBlock(attr, body.to_owned())]))
        };
        // A run that is not the whole line cannot close anything.
        assert_eq!(block("```` x"), "``` bash\n```` x\n```\n");
        assert_eq!(block("```` ``"), "``` bash\n```` ``\n```\n");
        // A bare run can, however it is padded.
        assert_eq!(block("````"), "````` bash\n````\n`````\n");
        assert_eq!(block("   ````"), "````` bash\n   ````\n`````\n");
        assert_eq!(block("```` "), "````` bash\n```` \n`````\n");
        assert_eq!(block("\t````"), "````` bash\n\t````\n`````\n");
    }

    /// **Empty content writes no line at all**, where `"".split('\n')`
    /// gave one and the block read back as `"\n"`.
    ///
    /// The trailing newline stays, and this is the rare place the bytes
    /// are knowingly not pandoc's: it writes `x\n` as plain `x` and reads
    /// that back as `x`, losing the newline, so copying it would trade a
    /// round trip that works for a byte comparison that does not matter.
    #[test]
    fn a_fence_keeps_its_trailing_newline_but_writes_nothing_for_empty() {
        let block = |body: &str| {
            let attr = ferrodoc_ast::Attr {
                identifier: String::new(),
                classes: vec!["text".to_owned()],
                attributes: Vec::new(),
            };
            write_pandoc_markdown(&Pandoc::new(vec![Block::CodeBlock(attr, body.to_owned())]))
        };
        assert_eq!(block(""), "``` text\n```\n");
        assert_eq!(block("x"), "``` text\nx\n```\n");
        // Pandoc writes these two the same as `x`. Each blank line here
        // is a newline that survives the trip back.
        assert_eq!(block("x\n"), "``` text\nx\n\n```\n");
        assert_eq!(block("x\n\n"), "``` text\nx\n\n\n```\n");
    }

    /// **The five characters pandoc's dialect escapes and `CommonMark`
    /// does not**, each probed against `pandoc -f json -t markdown` on a
    /// one-`Str` paragraph. Four were here from the start; `^` was not,
    /// and the only thing in the corpus that could see it was a `2^n`
    /// written into `docs/divergences.md` as prose — one reword away
    /// from covering nothing.
    #[test]
    fn the_dialect_escapes_five_characters_commonmark_leaves_alone() {
        let para = |text: &str| {
            let doc = Pandoc::new(vec![Block::Para(vec![Inline::Str(text.to_owned())])]);
            write_pandoc_markdown(&doc)
        };
        assert_eq!(para("a'b"), "a\\'b\n");
        assert_eq!(para("a\"b"), "a\\\"b\n");
        assert_eq!(para("a|b"), "a\\|b\n");
        assert_eq!(para("a^b"), "a\\^b\n");
        assert_eq!(para("a~b"), "a\\~b\n");
        // And `CommonMark` leaves every one of them alone.
        assert_eq!(write_markdown(&Pandoc::new(vec![Block::Para(vec![
            Inline::Str("a'b\"c|d^e~f".to_owned())
        ])])), "a'b\"c|d^e~f\n");
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

    /// **A fill must not write a document that reads back as another
    /// one.** Left to a plain greedy fill, this paragraph put a bare `+`
    /// at the start of the second line, and `+ 8 = 40, …` is a bullet
    /// item: the round trip returned a `Para` **and a `BulletList`**.
    ///
    /// Pandoc breaks one word earlier for the same reason, so this is
    /// both a correctness fix and a parity one — but the correctness half
    /// is why the rule is not optional.
    #[test]
    fn a_fill_never_strands_a_bullet_at_the_start_of_a_line() {
        let long = "x".repeat(64);
        let text = format!("{long} 8 + 24 + 8 = 40, and the discriminant makes 48 here.");
        let doc = crate::read_commonmark(&text).expect("read");
        let written = write_markdown_wrapped(&doc, 72);
        for line in written.lines() {
            assert!(
                !matches!(line.split(' ').next(), Some("+" | "-" | "*" | ">" | "1." | "1)")),
                "a line opens a block: {line:?}"
            );
        }
        let again = crate::read_commonmark(&written).expect("re-read");
        assert_eq!(
            doc.blocks.len(),
            again.blocks.len(),
            "the round trip changed the block structure:\n{written}"
        );
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
        assert_eq!(write_notebook(&doc), "$L = \\sum_i (y_i)^2$\n");

        let display = Pandoc::new(vec![Block::Para(vec![
            Inline::Math(MathType::DisplayMath, "E = mc^2".to_owned()),
        ])]);
        assert_eq!(write_notebook(&display), "$$E = mc^2$$\n");

        // **The `gfm` writer spells the same AST differently**, and this
        // test asserted the notebook answer for it until 2026-08-28.
        // Pandoc writes GitHub's syntax there: a code span between
        // dollars inline, and a ```` ```math ```` fence for display.
        assert_eq!(write_gfm(&doc), "$`L = \\sum_i (y_i)^2`$\n");
        assert_eq!(write_gfm(&display), "``` math\nE = mc^2\n```\n");
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
        // A table with no columns has no pipe-table spelling, so it is
        // written as the raw HTML every other unspellable block is.
        // Pandoc writes `||` here and loses the cell; this keeps it.
        let empty_grid = Pandoc::new(vec![Block::Table(Box::new(Table {
            colspecs: Vec::new(),
            ..one_by_one()
        }))]);
        assert!(write_gfm(&empty_grid).contains("<th>a</th>"));
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
            "| a    |\n|------|\n| p\\|q |\n"
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
        assert_eq!(written, "- item\n\n  | a   |\n  |-----|\n");
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
            "|      |     |         |\n|------|-----|---------|\n| wide |     | one two |\n\nCap\n"
        );
        // CommonMark has no table syntax, so the same document degrades to
        // its cell contents there — that is the loss GFM mode exists for.
        assert!(!write_markdown(&doc).contains('|'));
    }

    /// **The simple table**, in the three shapes the corpus cannot show.
    ///
    /// `corpus/gfm/tables.gfm` is byte-identical to pandoc, but every
    /// table in it came from a pipe table, so every one has a header and
    /// none has a caption. Each answer below is
    /// `pandoc -f json -t markdown --columns=72` on a table built one
    /// shape at a time.
    #[test]
    fn a_simple_table_pads_to_its_widest_cell_plus_two() {
        let plain = |t: &str| {
            let mut c = cell(1, vec![Block::Plain(vec![Inline::Str(t.to_owned())])]);
            if t.is_empty() {
                c.blocks.clear();
            }
            c
        };
        let row = |ts: &[&str]| Row {
            attr: ferrodoc_ast::Attr::default(),
            cells: ts.iter().map(|t| plain(t)).collect(),
        };
        let table = |aligns: &[Alignment], head: Vec<Row>, body: Vec<Row>, caption: Vec<Block>| {
            let doc = Pandoc::new(vec![Block::Table(Box::new(Table {
                attr: ferrodoc_ast::Attr::default(),
                caption: ferrodoc_ast::Caption { short: None, blocks: caption },
                colspecs: aligns
                    .iter()
                    .map(|a| ferrodoc_ast::ColSpec {
                        alignment: *a,
                        width: ferrodoc_ast::ColWidth::ColWidthDefault,
                    })
                    .collect(),
                head: ferrodoc_ast::TableHead {
                    attr: ferrodoc_ast::Attr::default(),
                    rows: head,
                },
                bodies: vec![ferrodoc_ast::TableBody {
                    attr: ferrodoc_ast::Attr::default(),
                    row_head_columns: 0,
                    head: Vec::new(),
                    body,
                }],
                foot: ferrodoc_ast::TableFoot::default(),
            }))]);
            write_pandoc_markdown(&doc)
        };
        let default2 = [Alignment::AlignDefault, Alignment::AlignDefault];

        // Two spaces of indent, and each column its widest cell plus two.
        assert_eq!(
            table(&default2, vec![row(&["h1", "h2"])], vec![row(&["a", "b"])], vec![]),
            "  h1   h2\n  ---- ----\n  a    b\n"
        );
        // **Only the last cell drops its right padding**, and the space
        // that separates it from the one before does not go with it — so
        // a row ending in an empty cell ends in a run of spaces.
        assert_eq!(
            table(
                &[Alignment::AlignDefault; 3],
                vec![row(&["one", "two", "three"])],
                vec![row(&["1", "2", ""])],
                vec![]
            ),
            "  one   two   three\n  ----- ----- -------\n  1     2     \n"
        );
        // A header of nothing but empty cells is not a header: the rule
        // goes above *and* below, and the columns are two wide.
        assert_eq!(
            table(&default2, vec![row(&["", ""])], vec![row(&["", "b"])], vec![]),
            "  -- ---\n     b\n  -- ---\n"
        );
        // `AlignCenter` puts the odd space on the right, `AlignRight`
        // pads on the left.
        assert_eq!(
            table(
                &[Alignment::AlignCenter, Alignment::AlignRight],
                vec![row(&["hello", "x"])],
                vec![row(&["a", "bbbb"])],
                vec![]
            ),
            "   hello       x\n  ------- ------\n     a      bbbb\n"
        );
        // A caption follows a blank line as `: text`.
        assert_eq!(
            table(
                &default2,
                vec![row(&["h1", "h2"])],
                vec![row(&["a", "b"])],
                vec![Block::Plain(vec![
                    Inline::Str("My".to_owned()),
                    Inline::Space,
                    Inline::Str("caption".to_owned()),
                ])]
            ),
            "  h1   h2\n  ---- ----\n  a    b\n\n  : My caption\n"
        );
    }

    /// **The constructs pandoc's dialect spells and `CommonMark` does
    /// not**, none of which the writer corpus can see: they reach a
    /// document through HTML or a JSON AST, and `corpus/*.md` and
    /// `corpus/gfm/*.gfm` are read as `CommonMark` and gfm. Every answer
    /// is `pandoc -f json -t markdown --wrap=none`.
    #[test]
    fn the_dialect_spells_what_commonmark_writes_as_html() {
        let x = || vec![Inline::Str("x".to_owned())];
        let ab = || {
            vec![Inline::Str("a".to_owned()), Inline::Space, Inline::Str("b".to_owned())]
        };
        let para = |inline: Inline| write_pandoc_markdown(&Pandoc::new(vec![Block::Para(vec![inline])]));

        assert_eq!(para(Inline::Strikeout(x())), "~~x~~\n");
        assert_eq!(para(Inline::Underline(x())), "[x]{.underline}\n");
        assert_eq!(para(Inline::SmallCaps(x())), "[x]{.smallcaps}\n");
        // A space cannot stand bare inside either script marker, and it
        // must be escaped in **one pass** — chaining two replacements
        // escapes the space the first one wrote and gives `~a\\ b~`.
        assert_eq!(para(Inline::Subscript(ab())), "~a\\ b~\n");
        assert_eq!(para(Inline::Superscript(ab())), "^a\\ b^\n");
        // `~~` takes a bare space.
        assert_eq!(para(Inline::Strikeout(ab())), "~~a b~~\n");

        let attr = ferrodoc_ast::Attr {
            identifier: "marked".to_owned(),
            classes: vec!["note".to_owned()],
            attributes: Vec::new(),
        };
        assert_eq!(para(Inline::Span(Box::new(attr), x())), "[x]{#marked .note}\n");
        // A span carrying nothing is only a wrapper.
        assert_eq!(para(Inline::Span(Box::default(), x())), "x\n");

        // A fenced div takes a bare name for one class and nothing else,
        // the attribute set otherwise — and `{}` when it carries nothing,
        // where a code block would have no info string at all.
        let div = |attr: ferrodoc_ast::Attr| {
            let body = vec![Block::Para(vec![Inline::Str("body".to_owned())])];
            write_pandoc_markdown(&Pandoc::new(vec![Block::Div(attr, body)]))
        };
        let of = |id: &str, classes: &[&str]| ferrodoc_ast::Attr {
            identifier: id.to_owned(),
            classes: classes.iter().map(|c| (*c).to_owned()).collect(),
            attributes: Vec::new(),
        };
        assert_eq!(div(of("", &["callout"])), "::: callout\nbody\n:::\n");
        assert_eq!(div(of("i", &["a", "b"])), "::: {#i .a .b}\nbody\n:::\n");
        assert_eq!(div(of("", &[])), "::: {}\nbody\n:::\n");

        // The same shape decides a fence's info string, except that the
        // dialect drops **nothing** — `sourceCode` included, where gfm
        // strips it and spells the first class left.
        let fence = |classes: &[&str]| {
            let block = Block::CodeBlock(of("", classes), "x".to_owned());
            let doc = Pandoc::new(vec![block]);
            (write_pandoc_markdown(&doc), write_gfm(&doc))
        };
        assert_eq!(
            fence(&["sourceCode", "bash"]),
            ("``` {.sourceCode .bash}\nx\n```\n".to_owned(), "``` bash\nx\n```\n".to_owned())
        );
        assert_eq!(
            fence(&["sourceCode"]),
            ("``` sourceCode\nx\n```\n".to_owned(), "```\nx\n```\n".to_owned())
        );
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
    fn an_escaped_bang_leaves_its_bracket_alone() {
        // Round-tripping cannot see this: `\!\[` and `\![` read back as
        // the same text. What differs is the spelling, and pandoc's is
        // the second — the escaped `!` is already what stops the image,
        // so the bracket behind it stands. An ODT holding a CDATA
        // section is where it showed up (`dropin-045`).
        let doc = Pandoc::new(vec![Block::Para(vec![Inline::Str(
            "<![CDATA[ character data ]]>".to_owned(),
        )])]);
        assert_eq!(
            write_markdown(&doc),
            "\\<\\![CDATA\\[ character data \\]\\]\\>\n"
        );
        // A `!` with no bracket after it is not escaped at all, and a
        // bracket with no `!` before it still is.
        let plain = Pandoc::new(vec![Block::Para(vec![Inline::Str("a!b[c".to_owned())])]);
        assert_eq!(write_markdown(&plain), "a!b\\[c\n");
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
