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

use ferrodoc_ast::{
    Alignment, Block, Cell, ColWidth, Inline, ListNumberStyle, Pandoc, QuoteType, Table,
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
/// Filling is a **post-pass** over the finished text: every line's own
/// leading whitespace is the indentation its continuation lines take, and
/// by the time a line exists the list or block it sits in has already
/// applied it.
fn lay_out(text: &str, wrap: Wrap) -> String {
    match wrap {
        Wrap::None => text.replace([BREAK, SOFT], " "),
        // A kept line break is still a break and takes the line's own
        // indentation with it.
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
    // **No hanging indent.** An `AsciiDoc` list item's continuation lines
    // are flush with the marker, not with its content — measured: pandoc
    // wraps `* a fairly long bullet item that will` and starts the next
    // line at column zero. Only the line's own leading whitespace counts.
    let indent = " ".repeat(line.chars().take_while(|c| *c == ' ').count());
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

/// Render a document as `AsciiDoc`.
pub fn write_asciidoc(doc: &Pandoc) -> String {
    write_asciidoc_wrapped(doc, Wrap::Preserve)
}

/// The same, laid out the way `--wrap` asks for.
#[must_use]
pub fn write_asciidoc_wrapped(doc: &Pandoc, wrap: Wrap) -> String {
    let mut out = String::new();
    blocks(&doc.blocks, &mut out, Depth::default());
    let text = lay_out(out.trim_end(), wrap);
    if text.is_empty() { text } else { text + "\n" }
}

/// How deep the block being written sits **in each kind of list**.
///
/// The marker's length is the nesting depth here, and `AsciiDoc` counts the
/// two kinds separately: a bullet list inside an ordered one is `*`, not
/// `**`. One counter wrote `**` and `...` where pandoc writes `*` and
/// `..`, which nests the list one level too deep on every render.
#[derive(Clone, Copy, Default)]
struct Depth {
    bullet: usize,
    ordered: usize,
}

fn blocks(list: &[Block], out: &mut String, depth: Depth) {
    for block in list {
        let before = out.len();
        block_to(block, out, depth);
        // A raw block in another format renders to nothing, and its
        // separator goes with it — a quote holding only one came out as
        // `____` around a blank line rather than around nothing.
        if out.len() == before {
            continue;
        }
        if !out.ends_with("\n\n") {
            out.push('\n');
        }
    }
}

/// A block quote, `____` around its content.
fn quote_to(inner: &[Block], out: &mut String, depth: Depth) {
    let mut text = String::new();
    blocks(inner, &mut text, depth);
    // No `[quote]` line: the `____` delimiter already says what the block
    // is, and pandoc writes only the delimiter.
    //
    // A quote **inside** a quote would close the outer one at the wrong
    // place, so pandoc wraps the content in an open block (`--`) rather
    // than lengthening the delimiter.
    if inner.iter().any(|block| matches!(block, Block::BlockQuote(_))) {
        out.push_str("____\n--\n");
        out.push_str(&text);
        out.push_str("--\n____\n");
        return;
    }
    let fence = fence_for(&text, '_');
    let text = text.trim_end();
    // A quote with nothing in it is two delimiters, not two with a blank
    // line between them — a raw block in another format leaves exactly
    // that, and `corpus/truncation-cases.md` has one.
    if text.is_empty() {
        let _ = writeln!(out, "{fence}\n{fence}");
    } else {
        let _ = writeln!(out, "{fence}\n{text}\n{fence}");
    }
}

fn block_to(block: &Block, out: &mut String, depth: Depth) {
    match block {
        Block::Plain(list) | Block::Para(list) => {
            // **Display math is a block, not a role.** `latexmath:[…]` is
            // the inline one; a paragraph that is nothing but display
            // math is a `[latexmath]` passthrough block, which is the
            // shape every reader produces for `$$…$$` on its own line.
            if let [Inline::Math(ferrodoc_ast::MathType::DisplayMath, math)] = list.as_slice() {
                let _ = writeln!(out, "[latexmath]\n++++\n{math}\n++++");
                return;
            }
            let mut text = String::new();
            inlines(list, &mut text);
            let text = text.trim_end();
            // **`{empty}` guards two shapes, and pandoc's rule for the
            // second is a quirk rather than a reason.**
            //
            // A line that would open a list is the plain case: `1. x` at
            // the start of a paragraph is an ordered item, and the
            // no-width attribute stops it being read as one.
            //
            // A leading `[` is guarded **only when the paragraph also
            // holds a footnote**. That is not a rule anyone would design
            // — the two have nothing to do with each other — but it is
            // what the binary does, measured five ways round:
            //
            // ```text
            // [line-through]#x#                 bare
            // [line-through]#x# and             bare
            // [line-through]#x# + a footnote    {empty}
            // text + a footnote                 bare
            // _emph_ + a footnote               bare
            // ```
            //
            // This wrote `{empty}` for every leading `[` until
            // 2026-08-28, which cost four constructs in the AST sweep;
            // dropping it entirely cost `samples/09-markdown-to-asciidoc`
            // its byte identity, and that sample is the reason the shape
            // above got measured at all.
            if opens_a_list(text) || (text.starts_with('[') && holds_a_note(list)) {
                out.push_str("{empty}");
            }
            let _ = writeln!(out, "{text}");
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
        Block::CodeBlock(attr, code) => code_block_to(attr, code, out),
        Block::BlockQuote(inner) => quote_to(inner, out, depth),
        Block::OrderedList(attrs, items) => {
            // The marker's *length* is the nesting depth, which is how
            // a nested list is spelled here.
            let marker = ".".repeat(depth.ordered + 1);
            let depth = Depth { ordered: depth.ordered + 1, ..depth };
            // **A list that names no style gets no attribute line.**
            // `arabic` is the default, and pandoc writes it only where
            // the list actually asked for a numbering — `Example` and
            // `DefaultStyle` ask for none. A start value always needs
            // the brackets, so it brings the style back with it.
            let style = number_style(attrs.style);
            match (style, attrs.start) {
                (None, 1) => {}
                (style, 1) => {
                    let _ = writeln!(out, "[{}]", style.unwrap_or("arabic"));
                }
                (style, start) => {
                    let _ = writeln!(out, "[{}, start={start}]", style.unwrap_or("arabic"));
                }
            }
            for item in items {
                item_to(item, &marker, out, depth);
            }
        }
        Block::BulletList(items) => {
            let marker = "*".repeat(depth.bullet + 1);
            let depth = Depth { bullet: depth.bullet + 1, ..depth };
            for item in items {
                item_to(item, &marker, out, depth);
            }
        }
        Block::DefinitionList(entries) => {
            for (term, definitions) in entries {
                let mut text = String::new();
                inlines(term, &mut text);
                let _ = writeln!(out, "{}::", text.trim());
                // **A `+` joins one definition to the next.** Without
                // it the second is a new paragraph outside the term, and
                // two definitions read as one.
                for (index, definition) in definitions.iter().enumerate() {
                    if index > 0 {
                        let _ = writeln!(out, "  +");
                    }
                    let mut body = String::new();
                    blocks(definition, &mut body, depth);
                    for line in body.trim_end().lines() {
                        let _ = writeln!(out, "  {line}");
                    }
                }
            }
        }
        Block::Header(level, attr, list) => header_to(*level, attr, list, out),
        // Five quotes        // Five quotes, which is what pandoc writes. Three is a valid
        // break too; the bytes are the test.
        Block::HorizontalRule => out.push_str("'''''\n"),
        Block::Table(table) => table_to(table, out),
        Block::Figure(_, caption, inner) => {
            if !caption.blocks.is_empty() {
                let mut text = String::new();
                blocks(&caption.blocks, &mut text, depth);
                let _ = writeln!(out, ".{}", text.trim().replace('\n', " "));
            }
            // **A figure's picture is a block image**, `image::` with
            // two colons; `image:` is the inline form and renders in a
            // paragraph rather than as a figure.
            let mut body = String::new();
            blocks(inner, &mut body, depth);
            out.push_str(&body.replacen("image:", "image::", 1));
        }
        // AsciiDoc has no grouping block, so a div is its content — but
        // an identifier on it is an anchor, and pandoc keeps that.
        Block::Div(attr, inner) => {
            if !attr.identifier.is_empty() {
                let _ = writeln!(out, "[[{}]]", attr.identifier);
            }
            blocks(inner, out, depth);
        }
        Block::RawBlock(format, text) => {
            if format.0 == "asciidoc" {
                out.push_str(text);
                out.push('\n');
            }
        }
    }
}

/// One list item, with any further blocks attached by a `+` continuation.
fn item_to(item: &[Block], marker: &str, out: &mut String, depth: Depth) {
    let (first, rest) = item.split_first().unwrap_or((&Block::HorizontalRule, &[]));
    let mut text = String::new();
    match first {
        // A task item's box reaches this writer as the `☐`/`☒` the GFM
        // reader makes of it, and AsciiDoc has its own spelling. Pandoc
        // writes `[ ]`/`[x]` **only where the box has content after it**:
        // an item that is nothing but a box keeps the character.
        Block::Plain(list) | Block::Para(list) => match list.split_first() {
            Some((Inline::Str(box_text), [Inline::Space, rest @ ..]))
                if box_text == "\u{2610}" || box_text == "\u{2612}" =>
            {
                text.push_str(if box_text == "\u{2612}" { "[x] " } else { "[ ] " });
                let mut body = String::new();
                inlines(rest, &mut body);
                text.push_str(body.trim_start());
            }
            _ => inlines(list, &mut text),
        },
        // A block that is not a paragraph cannot share the marker's line:
        // `. ....` is a marker followed by a literal-block delimiter that
        // never opens. Pandoc writes `{blank}` on the marker's line and
        // attaches the block with a `+`.
        other => {
            let mut body = String::new();
            block_to(other, &mut body, depth);
            let body = body.trim_end();
            // A raw block in another format renders to nothing, and the
            // `+` still belongs to the item — but the empty line after it
            // does not.
            if body.is_empty() {
                let _ = writeln!(out, "{marker} {{blank}}\n+");
            } else {
                let _ = writeln!(out, "{marker} {{blank}}\n+\n{body}");
            }
            for block in rest {
                attached_to(block, out, depth);
            }
            return;
        }
    }
    let _ = writeln!(out, "{marker} {}", text.trim_end());
    for block in rest {
        attached_to(block, out, depth);
    }
}

/// A block after the first inside a list item. A nested list continues at
/// this depth; anything else is attached with a `+` line, which is how
/// `AsciiDoc` keeps a second paragraph inside an item.
fn attached_to(block: &Block, out: &mut String, depth: Depth) {
    let mut body = String::new();
    match block {
        Block::BulletList(_) | Block::OrderedList(..) => {
            block_to(block, &mut body, depth);
            out.push_str(body.trim_end());
            out.push('\n');
        }
        other => {
            block_to(other, &mut body, depth);
            let _ = writeln!(out, "+\n{}", body.trim_end());
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
        ListNumberStyle::Decimal => Some("arabic"),
        ListNumberStyle::Example | ListNumberStyle::DefaultStyle => None,
    }
}

/// Whether any inline in `list` is a footnote, however deeply nested.
fn holds_a_note(list: &[Inline]) -> bool {
    list.iter().any(|inline| match inline {
        Inline::Note(_) => true,
        Inline::Emph(inner)
        | Inline::Strong(inner)
        | Inline::Strikeout(inner)
        | Inline::Superscript(inner)
        | Inline::Subscript(inner)
        | Inline::SmallCaps(inner)
        | Inline::Underline(inner)
        | Inline::Quoted(_, inner)
        | Inline::Cite(_, inner)
        | Inline::Span(_, inner)
        | Inline::Link(_, inner, _)
        | Inline::Image(_, inner, _) => holds_a_note(inner),
        _ => false,
    })
}

/// Whether a paragraph's first line would open a list where it stands.
///
/// `AsciiDoc` reads `1. x`, `1) x` and `2. x` as ordered items, so a
/// paragraph that begins with one needs `{empty}` in front of it.
fn opens_a_list(text: &str) -> bool {
    let line = text.lines().next().unwrap_or_default();
    let digits = line.chars().take_while(char::is_ascii_digit).count();
    digits > 0
        && line[digits..].starts_with(['.', ')'])
        // **A break opportunity is still `BREAK` here** — it becomes a
        // space only when the line is laid out — so the marker's space
        // has two spellings and this must accept both. Asking for `' '`
        // alone matched nothing at all.
        && line[digits + 1..].starts_with([' ', BREAK])
}

/// A heading, with an explicit anchor only where the identifier says
/// something the heading's own text does not.
fn header_to(level: i64, attr: &ferrodoc_ast::Attr, list: &[Inline], out: &mut String) {
    let level = &level;
            let mut text = String::new();
            inlines(list, &mut text);
            // An explicit anchor only where the identifier says something
            // the heading's own text does not. AsciiDoc derives one from
            // the title, so writing `[[a-heading]]` above `=== A heading`
            // is a name for a name it already had.
            //
            // The `-1`, `-2` tail is what pandoc's own uniquing adds to a
            // repeated heading, and it is automatic in the same sense.
            // Matching the shape rather than replaying the uniquing is an
            // approximation, and the one document it gets wrong is a
            // heading given `{#intro-1}` by hand whose text slugs to
            // `intro` — it would lose an anchor for a name AsciiDoc
            // derives anyway.
            let stem = slug(&plain_text(list));
            let automatic = attr.identifier == stem
                || attr
                    .identifier
                    .strip_prefix(&stem)
                    .and_then(|tail| tail.strip_prefix('-'))
                    .is_some_and(|n| !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()));
            if !attr.identifier.is_empty() && !automatic {
                let _ = writeln!(out, "[[{}]]", attr.identifier);
            }
            // Levels start at `==`: `=` is the document title and may
            // appear only once, so a document with two level-1 headings
            // would be invalid.
            // A level-6 heading is `=======`, seven marks: pandoc does
            // not stop at six, and clamping there merged levels 5 and 6.
            let marks = "=".repeat(usize::try_from(*level).unwrap_or(1).clamp(1, 6) + 1);
            // **A heading is never filled.** Pandoc keeps one on a single
            // line however narrow the column; a heading broken in two
            // reads as a heading and a paragraph.
            let _ = writeln!(out, "{marks} {}", text.trim().replace([BREAK, SOFT], " "));
}

/// `[#id .class]#text#`, the attribute-carrying span.
fn span_to(attr: &ferrodoc_ast::Attr, inner: &[Inline], out: &mut String) {
            out.push('[');
            if !attr.identifier.is_empty() {
                let _ = write!(out, "#{}", attr.identifier);
            }
            for class in &attr.classes {
                if !out.ends_with('[') {
                    out.push(' ');
                }
                let _ = write!(out, ".{class}");
            }
            out.push_str("]#");
            inlines(inner, out);
            out.push('#');
}

/// A **listing** (`----`) when the block names a language and a
/// **literal** block (`....`) when it does not.
fn code_block_to(attr: &ferrodoc_ast::Attr, code: &str, out: &mut String) {
            // A **listing** (`----`) when the block names a language and a
            // **literal** block (`....`) when it does not. Pandoc's
            // choice: `[source,py]` needs a listing to apply to, and a
            // block with nothing to highlight is verbatim text.
            // **Every class, in order, after `source`** — `sourceCode`
            // included, which this dropped: pandoc writes
            // `[source,sourceCode,bash]`, and a block classed only
            // `sourceCode` is still a listing, not a literal block.
            let delimiter = if attr.classes.is_empty() {
                '.'
            } else {
                let _ = writeln!(out, "[source,{}]", attr.classes.join(","));
                '-'
            };
            let fence = fence_for(code, delimiter);
            // **Empty content writes no line at all**, where trimming and
            // then writing left a blank one between the two fences.
            let body = code.trim_end();
            if body.is_empty() {
                let _ = writeln!(out, "{fence}\n{fence}");
            } else {
                let _ = writeln!(out, "{fence}\n{body}\n{fence}");
            }
}

fn table_to(table: &Table, out: &mut String) {
    let columns = table.colspecs.len().max(1);
    if !table.caption.blocks.is_empty() {
        let mut text = String::new();
        blocks(&table.caption.blocks, &mut text, Depth::default());
        let _ = writeln!(out, ".{}", text.trim().replace('\n', " "));
    }
    // **One attribute line**, and the trailing comma is pandoc's. The
    // `cols` spec names each column's alignment — `<`, `^`, `>`, or
    // nothing for the default — and an explicit width turns it into a
    // percentage with `width="100%"` in front.
    let cols: Vec<String> = table
        .colspecs
        .iter()
        .map(|spec| {
            let alignment = match spec.alignment {
                Alignment::AlignLeft => "<",
                Alignment::AlignCenter => "^",
                Alignment::AlignRight => ">",
                Alignment::AlignDefault => "",
            };
            match spec.width {
                #[expect(clippy::cast_possible_truncation, reason = "a percentage is small")]
                ColWidth::ColWidth(fraction) => {
                    format!("{alignment}{}%", (fraction * 100.0).round() as i64)
                }
                ColWidth::ColWidthDefault => alignment.to_owned(),
            }
        })
        .collect();
    let sized = table
        .colspecs
        .iter()
        .any(|spec| matches!(spec.width, ColWidth::ColWidth(_)));
    let cols = if cols.is_empty() { vec![String::new(); columns] } else { cols };
    out.push('[');
    if sized {
        out.push_str("width=\"100%\",");
    }
    let _ = write!(out, "cols=\"{}\",", cols.join(","));
    // **A header row of nothing but empty cells is not a header** — the
    // same rule the HTML, RST and markdown writers follow. Kept, it
    // claimed `options="header"` and wrote a row of bare pipes that
    // AsciiDoc renders as an empty heading band.
    let header = table
        .head
        .rows
        .iter()
        .any(|row| row.cells.iter().any(|cell| !cell.blocks.is_empty()));
    if header {
        out.push_str("options=\"header\",");
    }
    out.push_str("]\n");
    out.push_str("|===\n");
    for row in table
        .head
        .rows
        .iter()
        .filter(|_| header)
        .chain(table.bodies.iter().flat_map(|b| b.head.iter().chain(&b.body)))
        .chain(table.foot.rows.iter())
    {
        // **`a|` is the cell that can hold blocks**, and it is the only
        // way AsciiDoc has of putting one in a table: the content goes on
        // the lines after it and a blank line closes it. Flattened into a
        // plain `|` cell instead, a code block came out
        // `|[source,bash] ---- x ----` — one line of the markers that were
        // supposed to be a block.
        if row.cells.iter().any(|cell| !cell_is_simple(cell)) {
            for cell in &row.cells {
                if cell_is_simple(cell) {
                    let _ = writeln!(out, "|{}", cell_text(cell));
                } else {
                    out.push_str("a|\n");
                    for block in &cell.blocks {
                        block_to(block, out, Depth::default());
                    }
                    out.push('\n');
                }
            }
            continue;
        }
        // `|A |B` — the cells are joined by a space rather than each
        // carrying a trailing one, so the row does not end in whitespace.
        let cells: Vec<String> =
            row.cells.iter().map(|cell| format!("|{}", cell_text(cell))).collect();
        let _ = writeln!(out, "{}", cells.join(" "));
    }
    out.push_str("|===\n");
}

/// Whether a cell is one this writer can put after a plain `|`: at most
/// one `Plain` or `Para`, and no span. Anything else needs `a|`.
fn cell_is_simple(cell: &Cell) -> bool {
    cell.col_span.max(1) == 1
        && cell.row_span.max(1) == 1
        && cell.blocks.len() <= 1
        && cell.blocks.iter().all(|b| matches!(b, Block::Plain(_) | Block::Para(_)))
}

fn cell_text(cell: &Cell) -> String {
    let mut out = String::new();
    for block in &cell.blocks {
        match block {
            Block::Plain(list) | Block::Para(list) => inlines(list, &mut out),
            other => block_to(other, &mut out, Depth::default()),
        }
    }
    // A newline inside a cell starts a new one; the row would come apart.
    // A `|` would end the cell, and the `++|++` passthrough cannot save it
    // — `{vbar}` is the attribute that can, and it is what pandoc writes.
    // A code span's `|` arrives here bare and needs the same treatment; a
    // URL's does not, and pandoc leaves `link:u|v[t]` as it stands.
    let text = vbar_in_cells(&out.replace('\n', " "));
    let mut result = String::with_capacity(text.len());
    let mut in_code = false;
    for ch in text.chars() {
        match ch {
            '`' => {
                in_code = !in_code;
                result.push('`');
            }
            '|' if in_code => result.push_str("{vbar}"),
            ch => result.push(ch),
        }
    }
    result.trim().to_owned()
}

/// Render a run of inlines, collapsing the space a dropped inline leaves.
///
/// Pandoc builds its output as a `Doc` where two breaking spaces with
/// nothing between them are one, and a raw inline in another format
/// renders to nothing — so `plus <br/> and` is `plus and` there and was
/// `plus  and` here.
/// The characters that make `*b*` unreadable as strong and force the
/// **unconstrained** `**b**` instead — every alphanumeric, and these.
/// Probed one character at a time over ASCII punctuation with a space
/// held on the other side, because the neighbour on either side is
/// enough on its own and a probe with a letter outside answers for the
/// letter rather than for the character being tested.
const UNCONSTRAINED: &str = "$+<=>^|~";

fn inlines(list: &[Inline], out: &mut String) {
    let mut pieces: Vec<(String, bool)> = Vec::new();
    let mut after_break = false;
    for inline in list {
        let breaking = matches!(inline, Inline::Space | Inline::SoftBreak);
        if breaking && after_break {
            continue;
        }
        let mut piece = String::new();
        inline_to(inline, &mut piece);
        if piece.is_empty() {
            continue;
        }
        let constrainable = matches!(inline, Inline::Emph(_) | Inline::Strong(_));
        pieces.push((piece, constrainable));
        after_break = breaking;
    }
    // **AsciiDoc reads `*b*` as strong only where it stands apart.**
    // Against a word it is literal, and pandoc doubles the marker there:
    // `x**b**y` where `a *b* c` suffices. The neighbour that decides is
    // the sibling inline, as in RST — `` `c`*b* `` keeps the single
    // marker, so it is not "any non-space".
    let tight = |ch: Option<char>| match ch {
        None => false,
        Some(c) => c.is_alphanumeric() || UNCONSTRAINED.contains(c),
    };
    for (index, (piece, constrainable)) in pieces.iter().enumerate() {
        let next = pieces.get(index + 1).and_then(|(text, _)| text.chars().next());
        if *constrainable && (tight(out.chars().last()) || tight(next)) {
            let marker = piece.chars().next().unwrap_or('*');
            let inner = piece.trim_matches(marker);
            let _ = write!(out, "{marker}{marker}{inner}{marker}{marker}");
        } else {
            out.push_str(piece);
        }
    }
}

/// The URL schemes `AsciiDoc` turns into links on its own, so that
/// `https://x[text]` needs no `link:` in front of it and a relative path
/// or a `#fragment` does.
const LINKIFIED: [&str; 5] = ["http:", "https:", "ftp:", "irc:", "mailto:"];

fn inline_to(inline: &Inline, out: &mut String) {
    // The opening and closing markers are **not** the same for the
    // attributed forms: `[line-through]#gone#` closes with a bare `#`,
    // and repeating the whole opener wrote `[.line-through]#gone[.line-
    // through]#` — which renders as the attribute name in the text.
    let wrap = |open: &str, close: &str, inner: &[Inline], out: &mut String| {
        let mut text = String::new();
        inlines(inner, &mut text);
        if text.trim().is_empty() {
            out.push_str(&text);
            return;
        }
        let _ = write!(out, "{open}{}{close}", text.trim());
    };
    match inline {
        Inline::Str(text) => out.push_str(&escape(text)),
        Inline::Space => out.push(BREAK),
        Inline::SoftBreak => out.push(SOFT),
        // A trailing `+` is the hard break.
        Inline::LineBreak => out.push_str(" +\n"),
        // The markers are the opposite way round from markdown: `_` is
        // italic and `*` is bold.
        Inline::Emph(inner) => wrap("_", "_", inner, out),
        Inline::Strong(inner) => wrap("*", "*", inner, out),
        Inline::Underline(inner) => wrap("[.underline]#", "#", inner, out),
        // `[line-through]`, without the dot: pandoc writes the role name
        // and this wrote the shorthand for a CSS class.
        Inline::Strikeout(inner) => wrap("[line-through]#", "#", inner, out),

        Inline::Superscript(inner) => wrap("^", "^", inner, out),
        Inline::Subscript(inner) => wrap("~", "~", inner, out),
        // **AsciiDoc has a curly-quote spelling**, and pandoc uses it:
        // `'`x`'` and `"`x`"`. Writing the characters themselves was
        // legible but not what a toolchain reads back as a quote.
        Inline::Quoted(kind, inner) => {
            let (open, close) = match kind {
                QuoteType::SingleQuote => ("'`", "`'"),
                QuoteType::DoubleQuote => ("\"`", "`\""),
            };
            out.push_str(open);
            inlines(inner, out);
            out.push_str(close);
        }
        // Pandoc has no small-caps spelling here and writes the content;
        // `[.smallcaps]` is a role `AsciiDoc` does not define. A citation
        // and a span are their content for the same reason.
        // **A span carrying attributes is `[#id .class]#text#`.** One
        // carrying none is its content, as a citation and small caps are.
        Inline::Span(attr, inner) if !attr.identifier.is_empty() || !attr.classes.is_empty() => {
            span_to(attr, inner, out);
        }
        Inline::SmallCaps(inner) | Inline::Cite(_, inner) | Inline::Span(_, inner) => {
            inlines(inner, out);
        }
        Inline::Code(_, code) => {
            let _ = write!(out, "`{}`", passthrough(code));
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
            let text = text.trim();
            // A link whose text **is** its target needs no markup at all:
            // AsciiDoc linkifies a bare URL and a bare address, and that
            // is what pandoc writes — but only when the text is *bare*.
            // `[`x.md`](x.md)` has a `Code` for its text and flattens to
            // the URL just the same; pandoc writes ``link:x.md[`x.md`]``
            // and keeps the code font. `ROADMAP.md` opens with one.
            let literal = plain_text(inner);
            let bare = matches!(inner.as_slice(), [Inline::Str(_)]);
            if bare
                && (literal == target.url
                    || target.url.strip_prefix("mailto:") == Some(literal.as_str()))
            {
                out.push_str(&literal);
                return;
            }
            // `link:` only where AsciiDoc would not recognise the URL on
            // its own. It linkifies the five schemes below, so
            // `https://x[text]` needs no macro name and a relative path
            // or a `#fragment` does. **Including the fragment**: this
            // wrote `<<id,text>>`, which is a cross-reference to a block
            // AsciiDoc knows about rather than a link, and pandoc writes
            // neither.
            let macro_name =
                if LINKIFIED.iter().any(|scheme| target.url.starts_with(scheme)) { "" } else { "link:" };
            let _ = write!(out, "{macro_name}{}[{text}]", target.url);
        }
        Inline::Image(_, alt, target) => {
            let mut text = String::new();
            inlines(alt, &mut text);
            // An image with no alt text still gets one: pandoc uses the
            // URL's own file name, without its extension. An empty
            // `image:u.png[]` renders with no alternative text at all.
            let alt = if text.trim().is_empty() {
                target
                    .url
                    .rsplit('/')
                    .next()
                    .unwrap_or(&target.url)
                    .rsplit_once('.')
                    .map_or_else(|| target.url.clone(), |(stem, _)| stem.to_owned())
            } else {
                text.trim().to_owned()
            };
            if target.title.is_empty() {
                let _ = write!(out, "image:{}[{alt}]", target.url);
            } else {
                let _ = write!(out, "image:{}[{alt},title=\"{}\"]", target.url, target.title);
            }
        }
        Inline::Note(blocks_in_note) => {
            let mut text = String::new();
            blocks(blocks_in_note, &mut text, Depth::default());
            // The body keeps the soft breaks it came with — a newline
            // inside `footnote:[…]` is legal and pandoc leaves it — but a
            // **blank** line ends the macro, so a body of more than one
            // block is joined onto one instead.
            //
            // Pandoc writes `[multiblock footnote omitted]` here and
            // loses the body. Joining keeps it, and the note still
            // renders; a placeholder is content deleted for a byte.
            let body = text.trim();
            let body = if body.contains("\n\n") {
                body.split_whitespace().collect::<Vec<_>>().join(" ")
            } else {
                body.to_owned()
            };
            let _ = write!(out, "footnote:[{body}]");
        }
    }
}

/// Escape the characters `AsciiDoc` gives an inline meaning to.
///
/// A backslash before the character is its own escape, and it is
/// applied only to the markers that could start a construct — escaping
/// more would fill ordinary prose with backslashes for no benefit.
/// Escape the characters `AsciiDoc` gives a meaning to.
///
/// **A passthrough, not a backslash.** `AsciiDoc` has no general escape
/// character: `\*` is a backslash and an asterisk in most positions, and
/// The inside of a code span, which is **not** verbatim in `AsciiDoc`: a
/// backtick ends the span and the markup characters still mean what they
/// mean outside. The `++` passthrough is what pandoc reaches for.
///
/// **Ten characters, not four, and a run at a time.** The four this had
/// were chosen by hand and every one of the other six reached a real
/// document — `` `with_capacity` `` in `docs/benchmarking.md` came out as
/// emphasis. Probed over the whole of ASCII punctuation: `}` is *not* in
/// the set, only `{`, and pandoc wraps a maximal run once, so ``` `` ```
/// takes one wrapper rather than two.
fn passthrough(code: &str) -> String {
    // `+` is the exception: it is an attribute reference in `AsciiDoc`,
    // and pandoc spells a literal one `{plus}` rather than wrapping it —
    // one per character, so `a++b` is `a{plus}{plus}b`. Plain text
    // already did this; a code span did not. It is substituted **around**
    // the scan rather than before it, or the `{` it introduces would be
    // wrapped as a mark of its own.
    let marks = |c: char| "`*<>[\\]_{|".contains(c);
    if code.contains('+') {
        let parts: Vec<String> = code.split('+').map(passthrough).collect();
        return parts.join("{plus}");
    }
    let mut out = String::with_capacity(code.len());
    let mut rest = code;
    while !rest.is_empty() {
        let run = rest.find(marks).unwrap_or(rest.len());
        out.push_str(&rest[..run]);
        rest = &rest[run..];
        let end = rest.find(|c: char| !marks(c)).unwrap_or(rest.len());
        if end > 0 {
            let _ = write!(out, "++{}++", &rest[..end]);
            rest = &rest[end..];
        }
    }
    out
}

/// `++*++` is the one spelling that reliably means a literal one. Pandoc
/// uses it for every character in the set below, and `{plus}` for `+`
/// itself, which cannot be wrapped in `++`. Probed character by
/// character; `^`, `~` and `#` are **not** in the set, and escaping them
/// put a backslash into every `2^10` and every `~/path`.
fn escape(text: &str) -> String {
    escape_in(text, false)
}

/// The plain text of an inline run, as pandoc's `stringify` produces it:
/// a break is a space, and raw content and footnotes contribute nothing.
fn plain_text(inlines: &[Inline]) -> String {
    let mut out = String::new();
    plain_text_into(inlines, &mut out);
    out
}

fn plain_text_into(list: &[Inline], out: &mut String) {
    for inline in list {
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
            | Inline::Image(_, inner, _) => plain_text_into(inner, out),
        }
    }
}

/// The identifier a heading's own text already gives it.
fn slug(text: &str) -> String {
    let filtered: String = text
        .chars()
        .filter(|c| c.is_alphanumeric() || c.is_whitespace() || matches!(c, '_' | '-' | '.'))
        .flat_map(char::to_lowercase)
        .collect();
    let joined = filtered.split_whitespace().collect::<Vec<_>>().join("-");
    if joined.is_empty() { "section".to_owned() } else { joined }
}

/// Replace every `|` in a cell with `{vbar}`, **splitting the
/// passthrough it sits in**.
///
/// A `|` would end the cell and the `++…++` passthrough cannot save it;
/// `{vbar}` is the attribute that can, and it is an attribute rather than
/// literal text, so it has to sit *outside* the passthrough. Pandoc
/// writes `p++\++{vbar}q` for `p\|q`, closing the run at the pipe and
/// reopening after it.
///
/// This was a `replace("++|++", "{vbar}")` over the escaped text, which
/// worked only while every escapable character got a passthrough of its
/// own — the moment runs were grouped, `++\|++` stopped matching and a
/// pipe went into a cell as a pipe.
fn vbar_in_cells(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find("++") {
        out.push_str(&rest[..start]);
        let after = &rest[start + 2..];
        let Some(end) = after.find("++") else {
            out.push_str(&rest[start..]);
            return out;
        };
        for (index, piece) in after[..end].split('|').enumerate() {
            if index > 0 {
                out.push_str("{vbar}");
            }
            if !piece.is_empty() {
                let _ = write!(out, "++{piece}++");
            }
        }
        rest = &after[end + 2..];
    }
    out.push_str(rest);
    out
}

/// Whether `ch` needs a passthrough around it.
fn passes_through(ch: char, in_cell: bool) -> bool {
    matches!(ch, '<' | '>' | '`' | '*' | '_' | '[' | ']' | '{' | '\\')
        || (ch == '|' && !in_cell)
}

fn escape_in(text: &str, in_cell: bool) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(ch) = chars.next() {
        match ch {
            '+' => out.push_str("{plus}"),
            '|' if in_cell => out.push_str("{vbar}"),
            // **A whole run in one passthrough**, and the run is any
            // consecutive escapable characters rather than a repeat of
            // one: pandoc writes ``a++`*++b``, not ``a++`++++*++b``.
            // Escaped one at a time, three backticks became
            // ``++`++++`++++`++`` — six passthroughs where pandoc opens
            // one, which a paragraph in this repository's own
            // COMPATIBILITY.md is what caught.
            ch if passes_through(ch, in_cell) => {
                out.push_str("++");
                out.push(ch);
                while chars.peek().is_some_and(|next| passes_through(*next, in_cell)) {
                    out.push(chars.next().unwrap_or_default());
                }
                out.push_str("++");
            }
            ch => out.push(ch),
        }
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
        // A listing containing its own delimiter ends where the sample
        // does, and the rest of the document silently becomes prose. This
        // is the failure the check exists for. The block names a language
        // so that it is a **listing** (`----`) rather than the literal
        // block (`....`) a bare one becomes.
        let code = "before\n----\nafter";
        let attr = Attr { classes: vec!["sh".into()], ..Attr::default() };
        let rendered = write_asciidoc(&doc(vec![Block::CodeBlock(attr, code.into())]));
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
    fn a_link_names_its_macro_only_where_asciidoc_needs_one() {
        let link = |url: &str| {
            write_asciidoc(&doc(vec![Block::Para(vec![Inline::Link(
                Box::default(),
                vec![Inline::Str("text".into())],
                Box::new(Target { url: url.into(), title: String::new() }),
            )])]))
        };
        // A fragment is a link, not a cross-reference: `<<target,text>>`
        // points at a block AsciiDoc knows about, and pandoc writes
        // `link:#target[text]`.
        assert!(link("#target").contains("link:#target[text]"), "{}", link("#target"));
        // A scheme AsciiDoc linkifies on its own needs no macro name.
        assert!(link("http://x").contains("http://x[text]"), "{}", link("http://x"));
        assert!(!link("http://x").contains("link:"), "{}", link("http://x"));
        // A relative path does.
        assert!(link("a/b.html").contains("link:a/b.html[text]"), "{}", link("a/b.html"));
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
        // One attribute line holds both, and the style is named on every
        // ordered list — `arabic` included.
        assert!(rendered.contains("[upperroman, start=3]"), "{rendered}");
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
