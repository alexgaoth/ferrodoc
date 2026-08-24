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

/// Render a document as `AsciiDoc`.
pub fn write_asciidoc(doc: &Pandoc) -> String {
    let mut out = String::new();
    blocks(&doc.blocks, &mut out, Depth::default());
    let text = out.trim_end().to_owned();
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
            let mut text = String::new();
            inlines(list, &mut text);
            let text = text.trim_end();
            // A paragraph opening with `[` would be read as a block
            // attribute line — `[line-through]#struck#` at the start of
            // one is an attribute list, not markup. `{empty}` is the
            // no-width attribute that stops that, and it is what pandoc
            // writes.
            if text.starts_with('[') {
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
        Block::CodeBlock(attr, code) => {
            // A **listing** (`----`) when the block names a language and a
            // **literal** block (`....`) when it does not. Pandoc's
            // choice: `[source,py]` needs a listing to apply to, and a
            // block with nothing to highlight is verbatim text.
            let language = attr.classes.iter().find(|class| class.as_str() != "sourceCode");
            let delimiter = if let Some(language) = language {
                let _ = writeln!(out, "[source,{language}]");
                '-'
            } else {
                '.'
            };
            let fence = fence_for(code, delimiter);
            let _ = writeln!(out, "{fence}\n{}\n{fence}", code.trim_end());
        }
        Block::BlockQuote(inner) => quote_to(inner, out, depth),
        Block::OrderedList(attrs, items) => {
            // The marker's *length* is the nesting depth, which is how
            // a nested list is spelled here.
            let marker = ".".repeat(depth.ordered + 1);
            let depth = Depth { ordered: depth.ordered + 1, ..depth };
            // **One attribute line, always.** `arabic` is not a default
            // that can be left out — pandoc names the style on every
            // ordered list — and a start value goes in the same brackets.
            let style = number_style(attrs.style);
            if attrs.start == 1 {
                let _ = writeln!(out, "[{style}]");
            } else {
                let _ = writeln!(out, "[{style}, start={}]", attrs.start);
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
            let marks = "=".repeat(usize::try_from(*level).unwrap_or(1).clamp(1, 5) + 1);
            let _ = writeln!(out, "{marks} {}", text.trim());
        }
        // Five quotes, which is what pandoc writes. Three is a valid
        // break too; the bytes are the test.
        Block::HorizontalRule => out.push_str("'''''\n"),
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
fn number_style(style: ListNumberStyle) -> &'static str {
    match style {
        ListNumberStyle::LowerAlpha => "loweralpha",
        ListNumberStyle::UpperAlpha => "upperalpha",
        ListNumberStyle::LowerRoman => "lowerroman",
        ListNumberStyle::UpperRoman => "upperroman",
        _ => "arabic",
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
    if !table.head.rows.is_empty() {
        out.push_str("options=\"header\",");
    }
    out.push_str("]\n");
    out.push_str("|===\n");
    for row in table
        .head
        .rows
        .iter()
        .chain(table.bodies.iter().flat_map(|b| b.head.iter().chain(&b.body)))
        .chain(table.foot.rows.iter())
    {
        // `|A |B` — the cells are joined by a space rather than each
        // carrying a trailing one, so the row does not end in whitespace.
        let cells: Vec<String> =
            row.cells.iter().map(|cell| format!("|{}", cell_text(cell))).collect();
        let _ = writeln!(out, "{}", cells.join(" "));
    }
    out.push_str("|===\n");
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
    let text = out.replace('\n', " ").replace("++|++", "{vbar}");
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
fn inlines(list: &[Inline], out: &mut String) {
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
        out.push_str(&piece);
        after_break = breaking;
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
        Inline::Space => out.push(' '),
        Inline::SoftBreak => out.push('\n'),
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
        Inline::Quoted(kind, inner) => {
            let (open, close) = match kind {
                QuoteType::SingleQuote => ('\u{2018}', '\u{2019}'),
                QuoteType::DoubleQuote => ('\u{201c}', '\u{201d}'),
            };
            out.push(open);
            inlines(inner, out);
            out.push(close);
        }
        // Pandoc has no small-caps spelling here and writes the content;
        // `[.smallcaps]` is a role `AsciiDoc` does not define. A citation
        // and a span are their content for the same reason.
        Inline::SmallCaps(inner) | Inline::Cite(_, inner) | Inline::Span(_, inner) => {
            inlines(inner, out);
        }
        Inline::Code(_, code) => {
            // A code span is not verbatim in `AsciiDoc`: a backtick ends
            // it, and `<`, `>` and `|` still mean what they mean outside.
            // The `++` passthrough is what pandoc reaches for, inside the
            // span, for each of the four.
            let escaped: String = code
                .chars()
                .map(|ch| match ch {
                    '`' | '<' | '>' | '|' => format!("++{ch}++"),
                    ch => ch.to_string(),
                })
                .collect();
            let _ = write!(out, "`{escaped}`");
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
            // is what pandoc writes.
            let literal = plain_text(inner);
            if literal == target.url || target.url.strip_prefix("mailto:") == Some(literal.as_str())
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

fn escape_in(text: &str, in_cell: bool) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        match ch {
            '+' => out.push_str("{plus}"),
            '|' if in_cell => out.push_str("{vbar}"),
            '<' | '>' | '|' | '`' | '*' | '_' | '[' | ']' | '{' | '\\' => {
                let _ = write!(out, "++{ch}++");
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
