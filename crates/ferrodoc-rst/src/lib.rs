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

use ferrodoc_ast::{
    Block, Cell, Inline, ListNumberDelim, ListNumberStyle, Pandoc, QuoteType, Row, Table,
};
use std::fmt::Write as _;

/// Marks a place a line may be broken. Chosen because no reader here can
/// produce one inside text: `CommonMark` replaces NUL with U+FFFD by
/// specification, and XML — which DOCX, ODT and EPUB are — forbids it.
const BREAK: char = '\u{0}';
/// The same, for a `SoftBreak`, which `--wrap=preserve` keeps as a
/// newline where an ordinary space stays a space.
const SOFT: char = '\u{1}';

/// How the writer lays lines out, as pandoc's `--wrap` means it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Wrap {
    /// Every soft break becomes a space and no line is broken.
    None,
    /// A soft break stays a line break; nothing else is broken. This is
    /// what the writer did before it could fill, and the default.
    #[default]
    Preserve,
    /// Fill to this many columns, breaking at spaces and soft breaks.
    Fill(usize),
}

/// Turn the break marks into whatever the mode asks for.
///
/// Filling is a **post-pass** over the finished text, and it can be:
/// every line's own leading whitespace is the indentation its
/// continuation lines take, and by the time a line exists that
/// indentation has already been applied by the list, quote or directive
/// it sits in.
fn lay_out(text: &str, wrap: Wrap) -> String {
    match wrap {
        Wrap::None => text.replace([BREAK, SOFT], " "),
        // A kept line break is still a break, and takes the indentation
        // of the line it is in: a soft break inside a quote or a list
        // item starts its line where that item's content starts. Same
        // rule as the fill, with the column count out of the way and
        // every soft break forced.
        Wrap::Preserve => reflow(text, usize::MAX, true),
        Wrap::Fill(columns) => reflow(text, columns, false),
    }
}

fn reflow(text: &str, columns: usize, force_soft: bool) -> String {
    let mut out = String::with_capacity(text.len());
    for (index, line) in text.split('\n').enumerate() {
        if index > 0 {
            out.push('\n');
        }
        fill(line, columns, force_soft, &mut out);
    }
    out
}

/// Greedy fill: take words while they fit, break at the last mark that
/// did. A word longer than the width goes on its own line and overruns —
/// breaking inside it would invent a break the text does not have.
fn fill(line: &str, columns: usize, force_soft: bool, out: &mut String) {
    let leading = line.chars().take_while(|c| *c == ' ').count();
    // A line that **starts with a marker** continues under its content,
    // not under the marker: a wrapped `- item` lines up two in, and a
    // line block's `| ` the same. The marker is already in the text by
    // the time this runs, so its width is read off rather than passed in.
    let indent = " ".repeat(leading + marker_width(&line[leading..]));
    let mut width = 0;
    let mut rest = line;
    let mut forced = false;
    let mut index = 0;
    loop {
        let (word, next_forced, tail) = match rest.find([BREAK, SOFT]) {
            Some(at) => {
                let mark = rest[at..].chars().next().unwrap_or(BREAK);
                (&rest[..at], mark == SOFT, Some(&rest[at + mark.len_utf8()..]))
            }
            None => (rest, false, None),
        };
        let word_width = word.chars().count();
        if index == 0 {
            width = word_width;
        } else if !(forced && force_soft) && width.saturating_add(1 + word_width) <= columns {
            out.push(' ');
            width += 1 + word_width;
        } else {
            let _ = write!(out, "\n{indent}");
            width = indent.chars().count() + word_width;
        }
        out.push_str(word);
        index += 1;
        forced = next_forced;
        match tail {
            Some(tail) => rest = tail,
            None => return,
        }
    }
}

/// Trim whitespace **and break marks** from both ends. A mark is not
/// whitespace to `str::trim`, so `*emph in \0*` kept its mark inside the
/// emphasis and laid out as `*emph in *`.
fn trimmed(text: &str) -> &str {
    text.trim_matches(|c: char| c.is_whitespace() || c == BREAK || c == SOFT)
}

/// How wide the list or line-block marker at the start of `rest` is, or
/// zero where there is none.
fn marker_width(rest: &str) -> usize {
    if rest.starts_with("- ") || rest.starts_with("| ") {
        return 2;
    }
    // `1. `, `12) `, `iv. `, `a) ` — a run of label characters, a
    // delimiter and a space.
    let label = rest.chars().take_while(char::is_ascii_alphanumeric).count();
    if label == 0 {
        return 0;
    }
    let after = &rest[label..];
    if after.starts_with(". ") || after.starts_with(") ") {
        return label + 2;
    }
    0
}

/// Render a document as reStructuredText.
pub fn write_rst(doc: &Pandoc) -> String {
    write_rst_wrapped(doc, Wrap::Preserve)
}

/// The same, laid out the way `--wrap` asks for.
#[must_use]
pub fn write_rst_wrapped(doc: &Pandoc, wrap: Wrap) -> String {
    let mut out = String::new();
    let mut def = Defs { fills: matches!(wrap, Wrap::Fill(_)), ..Defs::default() };
    blocks(&doc.blocks, &mut out, &mut def);
    def.render_notes();
    def.flush(&mut out);
    let text = lay_out(out.trim_end(), wrap);
    if text.is_empty() { text } else { text + "\n" }
}

/// End what is written so far with exactly one blank line, so a group of
/// definitions is separated from the document by one and not by however
/// many the last block happened to leave.
fn separate(out: &mut String) {
    if out.is_empty() {
        return;
    }
    while out.ends_with('\n') {
        out.pop();
    }
    out.push_str("\n\n");
}

/// The definitions an inline defers to block level.
///
/// An inline image is a substitution reference and a footnote is a label:
/// neither carries its own content, and the block that does cannot be
/// written where the inline sits — the rest of the paragraph lands on the
/// next line and RST reads it as a continuation of the directive. That
/// shipped, and `sphinx-build` read `swatch.png` followed by ` inside a
/// sentence.` as one file name. Definitions are collected in document
/// order and written at the end of the document, which is a block level in
/// every context an inline can appear in — inside a table cell or a list
/// item, no other position is.
#[derive(Default)]
struct Defs {
    /// Substitution name to URL, in first-use order.
    images: Vec<(String, String)>,
    /// Footnote bodies, in label order, filled after the main pass.
    notes: Vec<String>,
    /// The blocks of each **top-level** note, queued while the document is
    /// written. Pandoc numbers the document's own notes first and a note
    /// nested inside one of them after all of them, and gives the nested
    /// one no body — so rendering depth-first here handed the inner note
    /// a label out of order and moved every later one.
    pending: Vec<Vec<Block>>,
    /// The next label to hand out; not `notes.len()`, because a nested
    /// note takes a label and contributes no body.
    next_note: usize,
    /// Whether a note's body is being rendered right now.
    in_note: bool,
    /// How many names have been invented, so the next is `imageN`. One
    /// counter for both cases pandoc invents a name for: an image with no
    /// alt text, and one whose alt text is already taken by another URL.
    generated: usize,
    /// Whether the layout pass that follows may **re-flow** a line.
    ///
    /// A grid table's columns are laid out while it is written, before
    /// that pass runs, so the table has to know what the pass will be
    /// allowed to do: a column too narrow for its content is wrapped
    /// when filling is available and **widened** when it is not, which
    /// is pandoc's rule and the same one the markdown multiline table
    /// follows.
    fills: bool,
}

impl Defs {
    /// Render every queued note body, once the main pass is over.
    ///
    /// A note met **here** takes the next label — past every top-level
    /// note by then — and queues nothing, which leaves a note inside a
    /// note with a reference and no body, as pandoc writes it.
    fn render_notes(&mut self) {
        let mut index = 0;
        while index < self.pending.len() {
            let queued = std::mem::take(&mut self.pending[index]);
            let mut text = String::new();
            let outer = std::mem::replace(&mut self.in_note, true);
            blocks(&queued, &mut text, self);
            self.in_note = outer;
            self.notes.push(trimmed(&text).to_owned());
            index += 1;
        }
    }

    /// The substitution name for one image, reusing a definition when the
    /// same alt text already names the same URL and uniquing it when it
    /// names a different one. Two definitions of one name is an error in
    /// docutils, not a last-one-wins.
    fn image_name(&mut self, alt: &str, url: &str) -> String {
        if !alt.is_empty() {
            match self.images.iter().find(|(name, _)| name == alt) {
                Some((_, existing)) if existing == url => return alt.to_owned(),
                None => {
                    self.images.push((alt.to_owned(), url.to_owned()));
                    return alt.to_owned();
                }
                Some(_) => {}
            }
        }
        self.generated += 1;
        let name = format!("image{}", self.generated);
        self.images.push((name.clone(), url.to_owned()));
        name
    }

    fn flush(&self, out: &mut String) {
        // **Footnotes first, substitutions last** — pandoc's order, and
        // the one `samples/08-markdown-to-rst` is measured against.
        //
        // A footnote is its own block, and its body starts on the line
        // after the label, indented — which is also what lets a body of
        // more than one line stay part of the footnote.
        for (index, body) in self.notes.iter().enumerate() {
            separate(out);
            let _ = writeln!(out, ".. [{}]", index + 1);
            for line in body.lines() {
                if line.is_empty() {
                    out.push('\n');
                } else {
                    let _ = writeln!(out, "{INDENT}{line}");
                }
            }
        }
        // The substitution definitions are **one block**, not one block
        // each: pandoc separates the group from the document with a blank
        // line and then writes them on consecutive lines.
        if !self.images.is_empty() {
            separate(out);
            for (name, url) in &self.images {
                let _ = writeln!(out, ".. |{name}| image:: {url}");
            }
        }
    }
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

fn blocks(list: &[Block], out: &mut String, def: &mut Defs) {
    let mut previous: Option<&Block> = None;
    for block in list {
        // An empty comment between a block that ends indented and a quote
        // that starts indented, or the quote is read as more of the first.
        // Pandoc's rule, probed pair by pair: a paragraph needs no comment
        // and a list, a quote or a literal block does.
        if matches!(block, Block::BlockQuote(_)) && previous.is_some_and(closes_indented) {
            out.push_str("..\n\n");
        }
        let before = out.len();
        block_to(block, out, def);
        // A raw block in another format renders to nothing and takes its
        // separator with it.
        if out.len() == before {
            continue;
        }
        // A blank line between blocks is not decoration in RST; it is what
        // ends the previous one.
        if !out.ends_with("\n\n") {
            out.push('\n');
        }
        previous = Some(block);
    }
}

/// Whether this block leaves the reader inside an indented context, so a
/// following quote would be read as part of it.
fn closes_indented(block: &Block) -> bool {
    matches!(
        block,
        Block::BlockQuote(_)
            | Block::BulletList(_)
            | Block::OrderedList(..)
            | Block::DefinitionList(_)
            | Block::CodeBlock(..)
            | Block::LineBlock(_)
    )
}

fn block_to(block: &Block, out: &mut String, def: &mut Defs) {
    match block {
        Block::Plain(list) | Block::Para(list) => para_to(list, out, def),
        Block::LineBlock(lines) => {
            for line in lines {
                let mut text = String::new();
                inlines(line, &mut text, def);
                let _ = writeln!(out, "| {text}");
            }
        }
        Block::CodeBlock(attr, code) => {
            // `code` when a language is known, so a toolchain highlights
            // it; a plain literal block otherwise. **`code`, not
            // `code-block`** — docutils understands both and pandoc
            // writes the first. The `sourceCode` class is pandoc's own
            // marker and is skipped, the way the markdown writers skip it.
            match attr.classes.iter().find(|class| class.as_str() != "sourceCode") {
                Some(language) => {
                    let _ = writeln!(out, ".. code:: {language}\n");
                }
                None => out.push_str("::\n\n"),
            }
            // A blank line inside the block stays blank: `indent` has
            // always known that and this loop did not, so every code
            // block with an empty line in it carried three stray spaces
            // there. `docs/releasing.md` has two.
            for line in code.lines() {
                if line.is_empty() {
                    out.push('\n');
                } else {
                    let _ = writeln!(out, "{INDENT}{line}");
                }
            }
            out.push('\n');
        }
        Block::BlockQuote(inner) => {
            let mut text = String::new();
            blocks(inner, &mut text, def);
            out.push_str(&indent(&text));
        }
        Block::OrderedList(attrs, items) => {
            for (index, item) in items.iter().enumerate() {
                let number = attrs.start + i64::try_from(index).unwrap_or(0);
                // **`#.` is RST's auto-numbering**, and pandoc writes it
                // for the list that asks for nothing — no style and no
                // delimiter. Anything else gets a literal marker, which
                // is also what keeps a start value other than one.
                //
                // The delimiter is the list's own: RST reads `1.`, `1)`
                // and `(1)`, and all three came out as one before.
                let marker = if attrs.style == ListNumberStyle::DefaultStyle
                    && attrs.delim == ListNumberDelim::DefaultDelim
                {
                    "#.".to_owned()
                } else {
                    let label = attrs.style.label(number);
                    match attrs.delim {
                        ListNumberDelim::TwoParens => format!("({label})"),
                        ListNumberDelim::OneParen => format!("{label})"),
                        _ => format!("{label}."),
                    }
                };
                item_to(item, &marker, tight(items), out, def);
            }
        }
        Block::BulletList(items) => {
            for item in items {
                item_to(item, "-", tight(items), out, def);
            }
        }
        Block::DefinitionList(entries) => {
            for (term, definitions) in entries {
                let mut text = String::new();
                inlines(term, &mut text, def);
                let _ = writeln!(out, "{}", text.trim_end());
                for definition in definitions {
                    let mut body = String::new();
                    blocks(definition, &mut body, def);
                    out.push_str(&indent(&body));
                }
            }
        }
        Block::Header(level, attr, list) => {
            let mut text = String::new();
            inlines(list, &mut text, def);
            // **A heading is never filled.** Pandoc keeps one on a
            // single line however narrow the column — and an underline
            // as long as the title is what makes it a heading at all.
            let text = trimmed(&text).replace([BREAK, SOFT], " ");
            // An explicit target above the heading is how RST names one —
            // but a heading is **already** a target under the name its own
            // text makes, so pandoc writes one only where the identifier
            // says something the text does not. Writing it always put a
            // `.. _a-heading:` above every heading in the document.
            if !attr.identifier.is_empty() && attr.identifier != slug(&stringify(list)) {
                let _ = writeln!(out, ".. _{}:\n", attr.identifier);
            }
            let index = usize::try_from(*level).unwrap_or(1).saturating_sub(1);
            let underline = UNDERLINES.get(index).copied().unwrap_or('\'');
            // Character *width*, not byte length: an underline shorter
            // than the title is a warning in every RST tool, and a heading
            // with an accent in it is one byte longer than it looks.
            let width = text.chars().count().max(1);
            let _ = writeln!(out, "{text}\n{}", underline.to_string().repeat(width));
        }
        // Fourteen dashes, which is what pandoc writes. Four is a valid
        // transition too; the bytes are the test.
        Block::HorizontalRule => out.push_str("--------------\n"),
        Block::Table(table) => table_to(table, out, def),
        Block::Figure(_, caption, inner) => figure_to(caption, inner, out, def),
        Block::Div(attr, inner) => container_to(attr, inner, out, def),
        Block::RawBlock(format, text) => raw_block_to(&format.0, text, out),
    }
}

/// A paragraph holding a hard break, written as a line block: each hard
/// break starts a new `| ` line and a soft break continues the one before
/// it, indented two.
fn hard_broken_para_to(list: &[Inline], out: &mut String, def: &mut Defs) {
    for segment in list.split(|inline| matches!(inline, Inline::LineBreak)) {
        let mut text = String::new();
        inlines(segment, &mut text, def);
        let mut lines = text.trim_end().lines();
        let _ = writeln!(out, "| {}", lines.next().unwrap_or_default());
        for line in lines {
            let _ = writeln!(out, "  {line}");
        }
    }
}

/// A raw block, kept rather than dropped.
///
/// `.. raw:: html` is RST's way to carry another format's syntax through,
/// and a toolchain that emits that format uses it. Dropping the block
/// deleted every table and comment a converted page had.
/// Escape the backticks a `:literal:` role cannot hold bare.
///
/// **Only the ones that could open or close RST inline markup**, which
/// is pandoc's rule and much narrower than escaping them all. RST starts
/// markup at a delimiter with whitespace *before* it and non-whitespace
/// *after*, and ends it at one with non-whitespace before and whitespace
/// after; a backtick in neither position cannot be mistaken for either,
/// so pandoc leaves it. Measured one shape at a time:
///
/// ```text
/// a`b        a`b          neither: bare
/// a `b       a \`b         could open
/// a` b       a\` b         could close
/// a  `  b    a  `  b      space on both sides is neither
/// x `y` z    x \`y\` z     a pair that would read as markup
/// ```        \``\`         ends escaped, middle bare
/// ```
///
/// Escaping every backtick was recorded here as a *deliberate*
/// divergence on the strength of one probe using an interior backtick,
/// which pandoc leaves bare — that looked like pandoc losing the span.
/// It has a scheme, and the scheme is RST's own boundary rule.
fn literal_backticks(code: &str) -> String {
    let chars: Vec<char> = code.chars().collect();
    let mut out = String::with_capacity(code.len());
    for (index, ch) in chars.iter().enumerate() {
        if *ch == '`' {
            let before = index.checked_sub(1).map(|i| chars[i]);
            let after = chars.get(index + 1).copied();
            let opens = before.is_none_or(char::is_whitespace)
                && after.is_some_and(|c| !c.is_whitespace());
            let closes = before.is_some_and(|c| !c.is_whitespace())
                && after.is_none_or(char::is_whitespace);
            // A backtick that is the whole content has neither
            // neighbour, so neither test fires; pandoc escapes it.
            if opens || closes || (before.is_none() && after.is_none()) {
                out.push('\\');
            }
        }
        out.push(*ch);
    }
    out
}

/// A paragraph, in whichever of the three shapes RST needs for it.
fn para_to(list: &[Inline], out: &mut String, def: &mut Defs) {
            // **A hard break has no paragraph spelling in RST**, so a
            // paragraph holding one is written as a line block: each hard
            // break starts a new `| ` line and a soft break continues the
            // one before it, indented two. Written as plain text the
            // break simply vanished.
            if list.iter().any(|inline| matches!(inline, Inline::LineBreak)) {
                hard_broken_para_to(list, out, def);
                return;
            }
            // **Display math is a directive, not a role.** `:math:` is
            // the inline one; a paragraph that is nothing but display
            // math is `.. math:: …`, which is the shape every reader
            // produces for `$$…$$` on a line of its own. Pandoc splits a
            // paragraph that holds one *among other text* as well — that
            // case still writes the role here.
            if let [Inline::Math(ferrodoc_ast::MathType::DisplayMath, math)] = list {
                let _ = writeln!(out, ".. math:: {math}");
                return;
            }
            let mut text = String::new();
            inlines(list, &mut text, def);
            let _ = writeln!(out, "{}", text.trim_end());
}

/// **A figure is a `figure` directive** when its body is one image: the
/// URL is the argument, the image's alt text an `:alt:` option, and the
/// caption the directive's content. Written as a substitution followed
/// by an indented paragraph it produced the alt text as a picture *and*
/// the caption as a block quote, which is two things where the document
/// had one.
fn figure_to(
    caption: &ferrodoc_ast::Caption,
    inner: &[Block],
    out: &mut String,
    def: &mut Defs,
) {
            let lone_image = match inner {
                [Block::Plain(list) | Block::Para(list)] => match list.as_slice() {
                    [Inline::Image(_, alt, target)] => Some((alt, target)),
                    _ => None,
                },
                _ => None,
            };
            let Some((alt, target)) = lone_image else {
                blocks(inner, out, def);
                if !caption.blocks.is_empty() {
                    let mut text = String::new();
                    blocks(&caption.blocks, &mut text, def);
                    out.push_str(&indent(&text));
                }
                return;
            };
            let _ = writeln!(out, ".. figure:: {}", target.url);
            let mut alt_text = String::new();
            inlines(alt, &mut alt_text, def);
            if !alt_text.is_empty() {
                let _ = writeln!(out, "{INDENT}:alt: {}", alt_text.trim());
            }
            if !caption.blocks.is_empty() {
                out.push('\n');
                let mut text = String::new();
                blocks(&caption.blocks, &mut text, def);
                out.push_str(&indent(&text));
            }
}

/// **A div is a `container` directive**, which is what pandoc's own RST
/// reader reads back as a `Div`. Writing only the content dropped the
/// grouping and every attribute on it — the classes become the
/// directive's argument and the identifier its `:name:` option.
fn container_to(attr: &ferrodoc_ast::Attr, inner: &[Block], out: &mut String, def: &mut Defs) {
            let argument = attr.classes.join(" ");
            let _ = writeln!(out, "{}", format!(".. container:: {argument}").trim_end());
            if !attr.identifier.is_empty() {
                let _ = writeln!(out, "{INDENT}:name: {}", attr.identifier);
            }
            out.push('\n');
            let mut body = String::new();
            blocks(inner, &mut body, def);
            for line in body.trim_end().lines() {
                if line.is_empty() {
                    out.push('\n');
                } else {
                    let _ = writeln!(out, "{INDENT}{line}");
                }
            }
}

fn raw_block_to(format: &str, text: &str, out: &mut String) {
    if format == "rst" {
        out.push_str(text);
        out.push('\n');
        return;
    }
    // **`tex` is spelled `latex` in a directive.** Pandoc names the raw
    // block `tex` in its AST and writes `.. raw:: latex`, which is the
    // name docutils knows; `.. raw:: tex` is a format nothing handles.
    let named = if format == "tex" { "latex" } else { format };
    let _ = writeln!(out, ".. raw:: {named}\n");
    for line in text.lines() {
        if line.is_empty() {
            out.push('\n');
        } else {
            let _ = writeln!(out, "{INDENT}{line}");
        }
    }
}

/// One list item: its marker, then its content aligned under it.
fn tight(items: &[Vec<Block>]) -> bool {
    items.iter().all(|item| !item.iter().any(|block| matches!(block, Block::Para(_))))
}

fn item_to(item: &[Block], marker: &str, tight: bool, out: &mut String, def: &mut Defs) {
    let mut text = String::new();
    blocks(item, &mut text, def);
    let pad = " ".repeat(marker.chars().count() + 1);
    // A **directive** cannot start on the marker's own line: `1. .. raw::
    // html` is read as a paragraph beginning with two dots. Pandoc writes
    // the marker, a blank line, and the content indented under it. A
    // literal block's `::` is fine where it stands, so the test is what
    // the content starts with rather than what kind of block it is.
    let own_line = text.starts_with(".. ");
    if own_line {
        let _ = writeln!(out, "{marker} \n");
    }
    for (index, line) in text.trim_end().lines().enumerate() {
        if index == 0 && !own_line {
            let _ = writeln!(out, "{marker} {line}");
        } else if line.is_empty() {
            out.push('\n');
        } else {
            let _ = writeln!(out, "{pad}{line}");
        }
    }
    // A tight list has no blank line between its items — but an item
    // holding more than one block still needs one, or the block that
    // follows is read as more of that item.
    if !tight || text.trim_end().contains("\n\n") {
        out.push('\n');
    }
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
fn table_to(table: &Table, out: &mut String, def: &mut Defs) {
    // **A caption makes the whole table a `table` directive**, with the
    // caption as its argument and the grid indented under it. Written
    // without one the caption simply vanished.
    if !table.caption.blocks.is_empty() {
        let mut caption = String::new();
        blocks(&table.caption.blocks, &mut caption, def);
        let _ = writeln!(out, ".. table:: {}\n", trimmed(&caption.replace('\n', " ")));
        let mut body = String::new();
        let bare = Table { caption: ferrodoc_ast::Caption::default(), ..table.clone() };
        table_to(&bare, &mut body, def);
        for line in body.trim_end().lines() {
            if line.is_empty() {
                out.push('\n');
            } else {
                let _ = writeln!(out, "{INDENT}{line}");
            }
        }
        return;
    }
    if simple_enough(table) {
        simple_table_to(table, out, def);
        return;
    }
    let rows: Vec<Vec<String>> = table
        .head
        .rows
        .iter()
        .chain(table.bodies.iter().flat_map(|b| b.head.iter().chain(&b.body)))
        .chain(table.foot.rows.iter())
        .map(|row| row_cells(row, table.colspecs.len(), def))
        .collect();
    if rows.is_empty() {
        return;
    }
    let columns = rows.iter().map(Vec::len).max().unwrap_or(0);
    // **A column stating its own width is given it**, rather than being
    // sized from the content: that is what keeps the proportions of a
    // table converted from DOCX, ODT or HTML. The arithmetic is the one
    // every other writer here uses — `floor(fraction x available)` where
    // available is `--columns` less the space between each pair — and the
    // two spaces of cell padding come off it, because the rule counts
    // them and the content does not.
    //
    // `--columns` does not reach this writer (the fill is a pass over the
    // finished text), so it is pandoc's default of 72; a table written
    // under a different one is laid out at 72 here.
    let available = RST_COLUMNS.saturating_sub(columns.saturating_sub(1));
    let widths: Vec<usize> = (0..columns)
        .map(|column| {
            let widest = rows
                .iter()
                .filter_map(|row| row.get(column))
                .map(|cell| cell.chars().count())
                .max()
                .unwrap_or(1)
                .max(1);
            if let Some(ferrodoc_ast::ColWidth::ColWidth(fraction)) =
                table.colspecs.get(column).map(|colspec| colspec.width)
            {
                #[expect(
                    clippy::cast_possible_truncation,
                    clippy::cast_sign_loss,
                    clippy::cast_precision_loss,
                    reason = "a column width is small, never negative, and \
                              well inside f64's mantissa"
                )]
                let stated = ((fraction * available as f64).floor() as usize)
                    .saturating_sub(2)
                    .max(1);
                return if def.fills { stated } else { stated.max(widest) };
            }
            widest
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
        // **A cell wider than its column is wrapped**, and the row is as
        // tall as the tallest cell. That could not happen while widths
        // came from the content; a column given its width by the
        // document can be narrower than what it holds, and subtracting
        // the two the other way round overflowed a `usize` and aborted.
        let filled: Vec<Vec<String>> = widths
            .iter()
            .enumerate()
            .map(|(column, width)| {
                wrap_cell(row.get(column).map_or("", String::as_str), *width)
            })
            .collect();
        let height = filled.iter().map(Vec::len).max().unwrap_or(1).max(1);
        for line in 0..height {
            out.push('|');
            for (column, width) in widths.iter().enumerate() {
                let piece = filled[column].get(line).map_or("", String::as_str);
                let pad = width.saturating_sub(piece.chars().count());
                let _ = write!(out, " {piece}{} |", " ".repeat(pad));
            }
            out.push('\n');
        }
        // A row of `=` under the head is what makes it a head.
        if index + 1 == header_rows {
            rule('=', out);
        } else {
            rule('-', out);
        }
    }
    out.push('\n');
}

/// Whether pandoc would write this as a **simple** table — the
/// `=== ===` form — rather than a grid.
///
/// Two conditions, both probed: every cell is one paragraph or empty, and
/// **the table has more than one column**. A one-column simple table is
/// ambiguous with a section underline, and pandoc writes a grid for it
/// however short the cell is.
/// One cell's text, greedily wrapped to `width`.
///
/// Not [`fill`], which indents a wrapped line under the marker its first
/// line begins with — right for a list item and wrong inside a cell,
/// where every line starts at the same column.
fn wrap_cell(text: &str, width: usize) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    let mut line = String::new();
    for word in text.split([' ', BREAK, SOFT]).filter(|w| !w.is_empty()) {
        if line.is_empty() {
            line.push_str(word);
        } else if line.chars().count() + 1 + word.chars().count() <= width {
            line.push(' ');
            line.push_str(word);
        } else {
            lines.push(std::mem::take(&mut line));
            line.push_str(word);
        }
    }
    if !line.is_empty() || lines.is_empty() {
        lines.push(line);
    }
    lines
}

/// Pandoc's default `--columns`, which a grid table's proportions are
/// measured against.
const RST_COLUMNS: usize = 72;

fn simple_enough(table: &Table) -> bool {
    // **A column stating its own width is not simple.** RST's simple
    // table sizes its columns from the content and has nowhere to put a
    // proportion, so pandoc writes a **grid** table for one — which is
    // what every table converted from DOCX, ODT or HTML asks for.
    table
        .colspecs
        .iter()
        .all(|colspec| colspec.width == ferrodoc_ast::ColWidth::ColWidthDefault)
        && table.colspecs.len() >= 2
        && table
            .head
            .rows
            .iter()
            .chain(table.bodies.iter().flat_map(|b| b.head.iter().chain(&b.body)))
            .chain(table.foot.rows.iter())
            .all(|row| {
                row.cells.iter().all(|cell| {
                    cell.col_span <= 1
                        && cell.row_span <= 1
                        && matches!(cell.blocks.as_slice(), [] | [Block::Plain(_) | Block::Para(_)])
                })
            })
}

/// The `=== ===` form: column rules, then the rows, with each cell but
/// the last padded to its column and one space between.
fn simple_table_to(table: &Table, out: &mut String, def: &mut Defs) {
    let columns = table.colspecs.len();
    // **A header row of nothing but empty cells is not a header** — the
    // same rule the HTML writer and the markdown simple table follow.
    // Kept, it wrote an escaped `\ ` row and widened every column to
    // hold it; pandoc writes the body alone.
    let head: Vec<Vec<String>> = table
        .head
        .rows
        .iter()
        // Asked **before** `simple_cells`, which fills an empty leading
        // cell with `\ ` to stop the row reading as a continuation — so
        // by then no row looks empty any more.
        .filter(|row| row.cells.iter().any(|cell| !cell.blocks.is_empty()))
        .map(|row| simple_cells(row, columns, def))
        .collect();
    let body: Vec<Vec<String>> = table
        .bodies
        .iter()
        .flat_map(|b| b.head.iter().chain(&b.body))
        .chain(table.foot.rows.iter())
        .map(|row| simple_cells(row, columns, def))
        .collect();
    if head.is_empty() && body.is_empty() {
        return;
    }
    let widths: Vec<usize> = (0..columns)
        .map(|column| {
            head.iter()
                .chain(&body)
                .filter_map(|row| row.get(column))
                .map(|cell| cell.chars().count())
                .max()
                .unwrap_or(1)
                .max(1)
        })
        .collect();
    let rule = |out: &mut String| {
        let parts: Vec<String> = widths.iter().map(|width| "=".repeat(*width)).collect();
        let _ = writeln!(out, "{}", parts.join(" "));
    };
    let write_row = |row: &[String], out: &mut String| {
        let mut line = String::new();
        for (column, width) in widths.iter().enumerate() {
            let cell = row.get(column).map_or("", String::as_str);
            // An empty first cell would leave the row starting with the
            // separator, which reads as a continuation of the row above.
            // A backslash is RST's way to say "this cell is empty".
            let cell = if column == 0 && cell.is_empty() { "\\" } else { cell };
            line.push_str(cell);
            if column + 1 < widths.len() {
                let _ = write!(line, "{} ", " ".repeat(width - cell.chars().count()));
            }
        }
        let _ = writeln!(out, "{line}");
    };
    rule(out);
    for row in &head {
        write_row(row, out);
    }
    if !head.is_empty() {
        rule(out);
    }
    for row in &body {
        write_row(row, out);
    }
    rule(out);
    out.push('\n');
}

fn row_cells(row: &Row, columns: usize, def: &mut Defs) -> Vec<String> {
    let mut cells: Vec<String> = row.cells.iter().map(|cell| cell_text(cell, def)).collect();
    cells.resize(columns.max(cells.len()), String::new());
    cells
}

/// One simple-table row's cells, with the marker an empty leading cell
/// needs already in place — `\ ` is two columns wide and the column has
/// to be at least that, so it cannot be substituted after the widths are
/// measured.
fn simple_cells(row: &Row, columns: usize, def: &mut Defs) -> Vec<String> {
    let mut cells = row_cells(row, columns, def);
    if cells.first().is_some_and(String::is_empty) {
        "\\ ".clone_into(&mut cells[0]);
    }
    cells
}

fn cell_text(cell: &Cell, def: &mut Defs) -> String {
    let mut out = String::new();
    for block in &cell.blocks {
        match block {
            Block::Plain(list) | Block::Para(list) => inlines(list, &mut out, def),
            other => block_to(other, &mut out, def),
        }
    }
    trimmed(&out.replace('\n', " ")).to_owned()
}

/// Render a run of inlines, collapsing the space a dropped inline leaves.
///
/// Pandoc builds its output as a `Doc` where two breaking spaces with
/// nothing between them are one space, and a raw inline in another format
/// renders to nothing — so `plus <br/> and` is `plus and` there and was
/// `plus  and` here.
/// What may sit directly before an RST start string, and directly after
/// an end string, without the `\ ` separator. Whitespace is fine on
/// either side and is checked apart from these.
const BEFORE: &str = "-:/'\"<([{";
const AFTER: &str = "-.,:;!?\\/'\")]}>";

fn inlines(list: &[Inline], out: &mut String, def: &mut Defs) {
    let mut pieces: Vec<(String, bool)> = Vec::new();
    let mut after_break = false;
    for inline in list {
        let breaking = matches!(inline, Inline::Space | Inline::SoftBreak);
        if breaking && after_break {
            continue;
        }
        let mut piece = String::new();
        inline_to(inline, &mut piece, def);
        if piece.is_empty() {
            continue;
        }
        pieces.push((piece, produces_markup(inline)));
        after_break = breaking;
    }
    // **RST will not read markup that abuts a word**: docutils wants the
    // start string preceded by whitespace or one of `-:/'"<([{`, and the
    // end string followed by whitespace or one of ``-.,:;!?\/'")]}>``.
    // Pandoc writes an escaped space where that does not hold, so
    // ``` ``int``\ →\ ``dt`` ``` and `` ``x``, `` bare.
    //
    // The neighbour that decides is the **sibling inline**, never the
    // container's own marker: the code span closing `**Not ``/tmp``**` is
    // followed by the strong's `**`, and pandoc puts nothing between
    // them. Deciding this over the finished text instead read those
    // markers as neighbours and separated four fixtures that were exact.
    let free = |ch: Option<char>, allowed: &str| match ch {
        None => true,
        Some(c) => c.is_whitespace() || c == BREAK || c == SOFT || allowed.contains(c),
    };
    for (index, (piece, markup)) in pieces.iter().enumerate() {
        if *markup && !free(out.chars().last(), BEFORE) {
            out.push_str("\\ ");
        }
        out.push_str(piece);
        if *markup {
            let next = pieces.get(index + 1).and_then(|(text, _)| text.chars().next());
            if !free(next, AFTER) {
                out.push_str("\\ ");
            }
        }
    }
}

/// Whether an inline is written as RST markup rather than as plain text.
/// A wrapper that keeps only its content — `SmallCaps`, `Span` — is not
/// one: whatever markup lies inside it marked its own edges. Neither is
/// a `Note`, whose `[1]_` carries the space in front of it already.
fn produces_markup(inline: &Inline) -> bool {
    matches!(
        inline,
        Inline::Emph(_)
            | Inline::Strong(_)
            | Inline::Strikeout(_)
            | Inline::Superscript(_)
            | Inline::Subscript(_)
            | Inline::Code(..)
            | Inline::Math(..)
            | Inline::Link(..)
            | Inline::Image(..)
    )
}

fn inline_to(inline: &Inline, out: &mut String, def: &mut Defs) {
    match inline {
        Inline::Str(text) => out.push_str(&escape(text)),
        Inline::Space => out.push(BREAK),
        // RST has no inline hard break outside a line block, so a hard
        // break becomes a soft one rather than markup that would show as
        // text. `COMPATIBILITY.md` records the loss.
        Inline::SoftBreak => out.push(SOFT),
        Inline::LineBreak => out.push('\n'),
        // **An underline is written as emphasis**, which is what pandoc
        // does: RST has no underline role, and dropping to bare content
        // lost the markup where `*x*` keeps something a reader sees.
        Inline::Emph(inner) | Inline::Underline(inner) => wrap("*", inner, out, def),
        // RST has no small caps; it keeps its content rather than
        // inventing a role a toolchain would not have. A citation and a
        // span are their content for the same reason.
        Inline::SmallCaps(inner)
        | Inline::Cite(_, inner)
        | Inline::Span(_, inner) => inlines(inner, out, def),
        // Strikeout has no RST markup either, and pandoc spells it
        // `[STRIKEOUT:…]` — a convention rather than a directive, but it
        // is what its own RST reader reads back.
        Inline::Strikeout(inner) => {
            out.push_str("[STRIKEOUT:");
            inlines(&flattened(inner), out, def);
            out.push(']');
        }
        Inline::Strong(inner) => wrap("**", inner, out, def),
        // **`sup` and `sub`, not the long spellings.** Both are standard
        // roles and pandoc writes the short ones; docutils accepts
        // either, so nothing rendered wrongly and no gate could see it.
        Inline::Superscript(inner) => role("sup", inner, out, def),
        Inline::Subscript(inner) => role("sub", inner, out, def),
        Inline::Quoted(kind, inner) => {
            let (open, close) = match kind {
                QuoteType::SingleQuote => ('\u{2018}', '\u{2019}'),
                QuoteType::DoubleQuote => ('\u{201c}', '\u{201d}'),
            };
            // **Into a fresh buffer**, so the quote this writes is not
            // read as the neighbour of what it wraps. The escaped space
            // before inline markup is decided by the *sibling inline*
            // and never by the container's own marker — the rule this
            // file already states for `**` — and rendering in place made
            // `“:literal:`a`b`”` come out `“\ :literal:`a`b`”`.
            // A quote written as literal text still separates: `“` in a
            // `Str` beside a code span takes the escape from both.
            let mut wrapped = String::new();
            inlines(inner, &mut wrapped, def);
            out.push(open);
            out.push_str(&wrapped);
            out.push(close);
        }
        // Double backticks, and no escaping inside them: that is what
        // makes it literal — **unless the code holds a backtick**, which
        // no run of them can enclose. Pandoc falls back to the `literal`
        // role, where a backslash escape works.
        Inline::Code(_, code) => {
            // **RST cannot hold a space against the marker**, so pandoc
            // trims both edges; the interior is untouched. `` `x ` `` is
            // a code span whose content really does end in a space —
            // `CommonMark` strips one only when there is one at each end
            // — and writing it back put the space where docutils cannot
            // read it. `ROADMAP.md` has `` `#include ` ``.
            let code = code.trim();
            if code.contains('`') {
                let _ = write!(out, ":literal:`{}`", literal_backticks(code));
            } else {
                let _ = write!(out, "``{code}``");
            }
        }
        Inline::Math(_, math) => {
            let _ = write!(out, ":math:`{math}`");
        }
        Inline::RawInline(format, text) => {
            // **RST can carry raw LaTeX**, through the `raw-latex` role
            // its own reader gives back; anything else has no home and
            // is dropped rather than written as prose.
            if format.0 == "rst" {
                out.push_str(text);
            } else if format.0 == "tex" || format.0 == "latex" {
                let _ = write!(out, ":raw-latex:`{text}`");
            }
        }
        Inline::Link(_, inner, target) => {
            // **RST cannot nest inline markup**, so a link's text is
            // plain: `\`link with *emph* inside <x>\`__` is not emphasis
            // inside a link, it is a literal asterisk. Pandoc flattens it
            // and so does this.
            let mut text = String::new();
            inlines(&flattened(inner), &mut text, def);
            // A link whose text **is** its target needs no markup at all:
            // RST linkifies a bare URL, and pandoc writes one. The angle
            // form around the same string is what this wrote, and it is
            // what made every README's autolinks differ.
            let bare = trimmed(&text);
            if bare == target.url || target.url.strip_prefix("mailto:") == Some(bare) {
                out.push_str(bare);
                return;
            }
            // An anonymous reference (two underscores) rather than a named
            // one: a named target must be unique in the document, and two
            // links with the same text are ordinary.
            let _ = write!(out, "`{}<{}>`__", text.trim_end().to_owned() + " ", target.url);
        }
        // Only the reference is written here. The definition it names is a
        // block, and `Defs` records why this is not a place one can go.
        Inline::Image(_, alt, target) => {
            let mut text = String::new();
            inlines(alt, &mut text, def);
            let name = def.image_name(trimmed(&text), &target.url);
            let _ = write!(out, "|{name}|");
        }
        Inline::Note(blocks_in_note) => {
            // Numbered, not `[#]_`: pandoc numbers them, and a numbered
            // label is what pairs a reference with its body when a
            // document is split. **Queued rather than rendered**, so a
            // note inside a note is met only once every top-level one
            // has taken its label.
            def.next_note += 1;
            let _ = write!(out, " [{}]_", def.next_note);
            if !def.in_note {
                def.pending.push(blocks_in_note.clone());
            }
        }
    }
}

/// The same inlines with every emphasis-like wrapper removed, because
/// RST has no way to nest them.
fn flattened(inlines: &[Inline]) -> Vec<Inline> {
    let mut out = Vec::with_capacity(inlines.len());
    for inline in inlines {
        match inline {
            Inline::Emph(inner)
            | Inline::Strong(inner)
            | Inline::Underline(inner)
            | Inline::Strikeout(inner)
            | Inline::SmallCaps(inner)
            | Inline::Span(_, inner)
            | Inline::Cite(_, inner) => out.extend(flattened(inner)),
            other => out.push(other.clone()),
        }
    }
    out
}

/// The plain text of an inline run, as pandoc's `stringify` produces it
/// for identifiers: a break is a space, and raw content and footnotes
/// contribute nothing. Slugging the *rendered* RST instead put the
/// footnote reference into the name, so every heading with a note got an
/// explicit target it did not need.
fn stringify(inlines: &[Inline]) -> String {
    let mut out = String::new();
    stringify_into(inlines, &mut out);
    out
}

fn stringify_into(inlines: &[Inline], out: &mut String) {
    for inline in inlines {
        match inline {
            Inline::Str(text) | Inline::Code(_, text) | Inline::Math(_, text) => {
                out.push_str(text);
            }
            Inline::Space | Inline::SoftBreak | Inline::LineBreak => out.push(' '),
            Inline::RawInline(..) | Inline::Note(_) => {}
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
            | Inline::Image(_, inner, _) => stringify_into(inner, out),
        }
    }
}

/// The identifier a heading's own text already gives it. Pandoc's rule,
/// and the same one the other writers use.
fn slug(text: &str) -> String {
    let filtered: String = text
        .chars()
        .filter(|c| c.is_alphanumeric() || c.is_whitespace() || matches!(c, '_' | '-' | '.'))
        .flat_map(char::to_lowercase)
        .collect();
    let joined = filtered.split_whitespace().collect::<Vec<_>>().join("-");
    if joined.is_empty() { "section".to_owned() } else { joined }
}

fn wrap(marker: &str, inner: &[Inline], out: &mut String, def: &mut Defs) {
    // **RST cannot nest inline markup.** `*emph *strong* text*` is not
    // strong inside emphasis — docutils reads the inner asterisks as
    // literal — so pandoc closes the outer marker, writes the inner one
    // on its own, and opens the outer one again. Without this,
    // `sphinx-build -W` rejected the document.
    if inner.iter().any(|i| matches!(i, Inline::Emph(_) | Inline::Strong(_))) {
        let mut pieces: Vec<String> = Vec::new();
        let mut run: Vec<Inline> = Vec::new();
        for item in inner {
            if matches!(item, Inline::Emph(_) | Inline::Strong(_)) {
                push_run(marker, &run, &mut pieces, def);
                run.clear();
                let mut text = String::new();
                inline_to(item, &mut text, def);
                let text = trimmed(&text);
                if !text.is_empty() {
                    pieces.push(text.to_owned());
                }
            } else {
                run.push(item.clone());
            }
        }
        push_run(marker, &run, &mut pieces, def);
        // The gaps **between** the pieces are break opportunities even
        // though the pieces themselves are not: pandoc breaks after
        // `*emph in*` and never inside it.
        out.push_str(&pieces.join(&BREAK.to_string()));
        return;
    }
    let mut text = String::new();
    inlines(inner, &mut text, def);
    if trimmed(&text).is_empty() {
        out.push_str(&text);
        return;
    }
    // **A span is not one word to the fill.** Docutils joins a
    // paragraph's continuation lines before it reads the markup, so
    // `**a long run**` may be broken anywhere inside it — and pandoc
    // does, filling `**section` onto one line and `for this version**`
    // onto the next. Treating the span as unbreakable pushed it whole
    // onto a new line and overran the column instead. The gate could not
    // see it: `writers.sh` compares at `--wrap=preserve`, where no fill
    // runs at all.
    //
    // Neither mark is dropped here, then: `BREAK` is the fill's own
    // opportunity and `SOFT` is a break the document wrote, and `lay_out`
    // is where both are decided.
    let _ = write!(out, "{marker}{}{marker}", trimmed(&text));
}

/// One run of un-nested inlines, wrapped in the marker its parent uses.
fn push_run(marker: &str, run: &[Inline], pieces: &mut Vec<String>, def: &mut Defs) {
    if run.is_empty() {
        return;
    }
    let mut text = String::new();
    inlines(run, &mut text, def);
    // Breakable, like the un-nested form above.
    let text = trimmed(&text);
    if !text.is_empty() {
        pieces.push(format!("{marker}{text}{marker}"));
    }
}

fn role(name: &str, inner: &[Inline], out: &mut String, def: &mut Defs) {
    let mut text = String::new();
    inlines(inner, &mut text, def);
    let _ = write!(out, ":{name}:`{}`", trimmed(&text).replace([BREAK, SOFT], " "));
}

/// Escape the characters RST gives a meaning to at the start of a word.
///
/// Only `*`, `` ` `` and `|` matter inside text, and only where they could
/// begin a construct; escaping every one of them would fill ordinary prose
/// with backslashes.
/// Escape the characters RST gives a meaning to.
///
/// **Positional, the way RST's own rules are.** A `*` can only open
/// markup where a start-string may stand — after whitespace or one of
/// `-:/'"<([{` — and only close it where an end-string may — before
/// whitespace or one of `-.,:;!?\/'")]}>`. A `*` with ordinary text on
/// both sides is neither, so `2*3` and `a|b` need no backslash, and
/// escaping them anyway put one in the middle of every product and every
/// pipe a document mentioned. Probed against the pinned binary, character
/// by character and position by position.
fn escape(text: &str) -> String {
    const OPENERS: &str = "-:/'\"<([{";
    const CLOSERS: &str = "-.,:;!?\\/'\")]}>";
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    let mut previous: Option<char> = None;
    while let Some(ch) = chars.next() {
        let next = chars.peek().copied();
        match ch {
            // A backslash is an escape wherever it stands.
            '\\' => out.push_str("\\\\"),
            '*' | '`' | '|' | '_' => {
                let inside = previous
                    .is_some_and(|c| !c.is_whitespace() && !OPENERS.contains(c))
                    && next.is_some_and(|c| !c.is_whitespace() && !CLOSERS.contains(c));
                // `__` is an anonymous hyperlink reference even in the
                // middle of a word, so the first of a pair is escaped
                // wherever it stands.
                let intraword = inside && !(ch == '_' && next == Some('_'));
                if !intraword {
                    out.push('\\');
                }
                out.push(ch);
            }
            ch => out.push(ch),
        }
        previous = Some(ch);
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
    fn a_picture_with_no_alt_text_is_named_rather_than_left_unnamed() {
        // A substitution reference needs a name and an image may have no
        // alt text to give it one. This wrote `.. image::` for a picture
        // alone in a paragraph to avoid the question; pandoc invents
        // `image1`, `image2`, … and uses a substitution everywhere.
        let rst = write_rst(&doc(vec![Block::Para(vec![Inline::Image(
            Box::default(),
            Vec::new(),
            Box::new(Target { url: "x.png".into(), title: String::new() }),
        )])]));
        assert!(rst.contains("|image1|"), "{rst}");
        assert!(rst.contains(".. |image1| image:: x.png"), "{rst}");
    }

    #[test]
    fn an_inline_image_leaves_its_paragraph_in_one_piece() {
        // The definition used to be written where the reference sits, so
        // the rest of the sentence landed on the line after `image::` and
        // became part of the file name. `sphinx-build` reported a missing
        // `swatch.pnginsideasentence.`; nothing here could see it, because
        // RST has no reader to round-trip against.
        let rst = write_rst(&doc(vec![Block::Para(vec![
            Inline::Str("before".into()),
            Inline::Space,
            Inline::Image(
                Box::default(),
                vec![Inline::Str("swatch".into())],
                Box::new(Target { url: "swatch.png".into(), title: String::new() }),
            ),
            Inline::Space,
            Inline::Str("after".into()),
        ])]));
        assert!(rst.contains("before |swatch| after"), "the paragraph was broken up: {rst}");
        assert!(rst.contains("\n.. |swatch| image:: swatch.png"), "{rst}");
        // The definition after the paragraph, never inside it.
        let (paragraph, _) = rst.split_once(".. |").expect("no definition: {rst}");
        assert!(paragraph.contains("after"), "the definition cut the paragraph short: {rst}");
    }

    #[test]
    fn one_alt_text_over_two_urls_becomes_two_names() {
        // Docutils rejects a name defined twice rather than taking the
        // last, so the same alt text on a different picture has to unique.
        let picture = |url: &str| {
            Inline::Image(
                Box::default(),
                vec![Inline::Str("logo".into())],
                Box::new(Target { url: url.into(), title: String::new() }),
            )
        };
        let rst = write_rst(&doc(vec![Block::Para(vec![
            picture("a.png"),
            Inline::Space,
            picture("b.png"),
            Inline::Space,
            picture("a.png"),
        ])]));
        // The second URL under the same alt takes an invented name, and
        // the counter is shared with the images that have no alt at all —
        // pandoc's, probed against six pictures in one document.
        assert!(rst.contains("|logo| |image1| |logo|"), "{rst}");
        assert_eq!(rst.matches(".. |logo| image:: a.png").count(), 1, "{rst}");
        assert_eq!(rst.matches(".. |image1| image:: b.png").count(), 1, "{rst}");
    }

    #[test]
    fn a_footnote_body_follows_the_document_not_the_sentence() {
        // Same defect as the image: the body is a block, and written in
        // place it swallowed the rest of the paragraph silently — a
        // footnote body accepts any text, so nothing complained.
        let rst = write_rst(&doc(vec![Block::Para(vec![
            Inline::Str("claim".into()),
            Inline::Note(vec![Block::Para(vec![Inline::Str("source".into())])]),
            Inline::Space,
            Inline::Str("and on".into()),
        ])]));
        assert!(rst.contains("claim [1]_ and on"), "the sentence lost its tail: {rst}");
        // Numbered rather than `[#]_`, and the body starts on the line
        // after the label — both pandoc's.
        assert!(rst.contains("\n.. [1]\n   source"), "{rst}");
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
        //
        // The grid form is reached by a cell holding **two** blocks:
        // pandoc writes the `=== ===` simple form for a table whose cells
        // are each one paragraph, and this one is not.
        let cell = |text: &str| ferrodoc_ast::Cell {
            attr: Attr::default(),
            alignment: ferrodoc_ast::Alignment::AlignDefault,
            row_span: 1,
            col_span: 1,
            blocks: vec![Block::Plain(vec![Inline::Str(text.into())])],
        };
        let row = |a: &str, b: &str| Row { attr: Attr::default(), cells: vec![cell(a), cell(b)] };
        let two_block_cell = ferrodoc_ast::Cell {
            attr: Attr::default(),
            alignment: ferrodoc_ast::Alignment::AlignDefault,
            row_span: 1,
            col_span: 1,
            blocks: vec![
                Block::Plain(vec![Inline::Str("a".into())]),
                Block::Para(vec![Inline::Str("second".into())]),
            ],
        };
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
                body: vec![Row {
                    attr: Attr::default(),
                    cells: vec![two_block_cell, cell("b")],
                }],
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

    #[test]
    fn a_simple_table_pads_every_column_but_the_last() {
        // Pandoc's `=== ===` form, which is what a table of one-paragraph
        // cells becomes. The rule must be as wide as the widest cell in
        // its column or docutils reads the overflow as a new column; the
        // **last** column is not padded, which is why the rows are not
        // all one width.
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
        assert_eq!(rst, "====== ==\nHeader H2\n====== ==\na      b\n====== ==\n");
    }
}
