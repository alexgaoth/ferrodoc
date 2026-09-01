//! LaTeX writer for the ferrodoc (pandoc-compatible) AST.
//!
//! [`write_latex`] renders a document body as LaTeX, and
//! [`write_latex_standalone`] wraps it in the smallest preamble that
//! actually compiles. Gated by `ferrodoc-harness diff-latex`, which writes
//! the same AST with both engines, has **pandoc** read each back, and
//! requires the two to agree — the same shape as `diff-write`.
//!
//! **There is deliberately no LaTeX reader, and there never will be.** A
//! `.tex` file expands arbitrary user-defined macros, so reading one means
//! interpreting a language rather than parsing a format. Writing is
//! bounded: escape ten characters, map each node to a macro, stop.
//!
//! Writing LaTeX *is* PDF output for anyone with a TeX installation —
//! `ferrodoc report.docx -t latex | pdflatex` — and it costs the binary
//! nothing, which is exactly what a bundled typesetter could not manage.
//!
//! Three rules that are not obvious:
//!
//! - **the special characters split into two groups.** Seven are escaped
//!   with a backslash (`# $ % & _ { }`); the rest have no backslash form
//!   and need a command or a group — `\textbackslash{}`, `\^{}`,
//!   `\textless{}` and the others in [`escape_char`]. Escaping `\` as `\\`
//!   emits a line break instead of a character, and a bare `<`, `>` or `|`
//!   sets as `¡`, `¿` or `—`;
//! - **inline code is `\texttt`, not `\verb`.** `\verb` needs no escaping
//!   and is illegal inside a command argument, which is where a heading,
//!   a caption and `alt=` put it; pandoc uses `\texttt` everywhere, so
//!   this does too. See [`verbatim`];
//! - a heading carries `\label` so its identifier survives, and pandoc's
//!   reader takes the identifier from the heading text when there is no
//!   label — so the two agree either way, and the label is what makes a
//!   cross-reference work.

use ferrodoc_ast::{
    Block, Caption, Cell, Inline, ListNumberDelim, ListNumberStyle, Pandoc, QuoteType, Row,
    Table,
};
use std::fmt::Write as _;

/// Render a document as a LaTeX fragment: no preamble, no
/// `\begin{document}`.
///
/// This is what goes inside someone else's template. Use
/// [`write_latex_standalone`] for something `pdflatex` can compile on its
/// own.
pub fn write_latex(doc: &Pandoc) -> String {
    write_latex_wrapped(doc, Wrap::Preserve)
}

/// Marks a place a line may be broken. Chosen because no reader here can
/// produce one inside text: `CommonMark` replaces NUL with U+FFFD by
/// specification, and XML — which DOCX, ODT and EPUB are — forbids it.
const BREAK: char = '\u{0}';
/// The same, for a `SoftBreak`, which `--wrap=preserve` keeps as a
/// newline where an ordinary space stays a space.
const SOFT: char = '\u{1}';
/// Opens a region whose continuation lines are indented two further, and
/// closes it. A `\footnote{…}` is one: pandoc indents the body's wrapped
/// lines by two and returns to the paragraph's own indent after the
/// closing brace, which nothing in the finished text says.
const IN: char = '\u{2}';
const OUT: char = '\u{3}';

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

/// Render a document as a LaTeX fragment, laid out the way `--wrap` asks.
#[must_use]
pub fn write_latex_wrapped(doc: &Pandoc, wrap: Wrap) -> String {
    render(doc, wrap, true)
}

/// The same, with **no highlighting**: every code block is `verbatim`
/// whatever language it names, which is `--syntax-highlighting=none`.
///
/// A separate entry point rather than a flag on the old one, because
/// `ferrodoc-html` already spells it that way and a caller who wants the
/// default should not have to say so.
#[must_use]
pub fn write_latex_unhighlighted(doc: &Pandoc, wrap: Wrap) -> String {
    render(doc, wrap, false)
}

fn render(doc: &Pandoc, wrap: Wrap, colour: bool) -> String {
    let mut out = String::new();
    blocks(&doc.blocks, 0, colour, &mut out);
    lay_out(out.trim_end(), wrap) + "\n"
}

/// Turn the break marks into whatever the mode asks for.
///
/// Filling is a **post-pass** over the finished text, and it can be:
/// every line's own leading whitespace is the indentation its
/// continuation lines take, and by the time a line exists that
/// indentation has already been applied by the list or quote it sits in.
fn lay_out(text: &str, wrap: Wrap) -> String {
    match wrap {
        Wrap::None => text.replace([BREAK, SOFT], " ").replace([IN, OUT], ""),
        // A kept line break is still a break, and takes the indentation
        // of the region it is in: a soft break inside a footnote body
        // starts its line at two. Same rule as the fill, with the column
        // count out of the way and every soft break forced.
        Wrap::Preserve => reflow(text, usize::MAX, true),
        Wrap::Fill(columns) => reflow(text, columns, false),
    }
}

/// The body of the two laying-out modes: fill to `columns`, and break at
/// every soft mark when `force_soft` — which is what `--wrap=preserve`
/// asks for.
fn reflow(text: &str, columns: usize, force_soft: bool) -> String {
    {
        let mut out = String::with_capacity(text.len());
        // The depth carries **across lines**: a footnote whose body is
        // more than one block opens on one line and closes several later,
        // and a depth reset per line indented every wrapped line after it.
        let mut depth = 0;
        for (index, line) in text.split('\n').enumerate() {
            if index > 0 {
                out.push('\n');
            }
            fill(line, columns, force_soft, &mut depth, &mut out);
        }
        out
    }
}

/// Split a line at its break marks, saying for each piece whether the
/// mark before it was a **soft break** — which `--wrap=preserve` forces —
/// or an ordinary space, which is only an opportunity.
fn split_marks(line: &str) -> Vec<(&str, bool)> {
    let mut pieces = Vec::new();
    let mut rest = line;
    let mut force = false;
    while let Some(at) = rest.find([BREAK, SOFT]) {
        let mark = rest[at..].chars().next().unwrap_or(BREAK);
        pieces.push((&rest[..at], force));
        force = mark == SOFT;
        rest = &rest[at + mark.len_utf8()..];
    }
    pieces.push((rest, force));
    pieces
}

/// Greedy fill: take words while they fit, break at the last mark that
/// did. A word longer than the width goes on its own line and overruns —
/// breaking inside it would invent a break the text does not have.
fn fill(line: &str, columns: usize, force_soft: bool, depth: &mut usize, out: &mut String) {
    let base = line.chars().take_while(|c| *c == ' ').count();
    let mut width = 0;
    let mut closed = false;
    // Which mark ended the previous word, so that a soft break can be
    // forced where an ordinary space is only an opportunity.
    for (index, (word, soft)) in split_marks(line).into_iter().enumerate() {
        let force = soft && force_soft;
        let clean: String = word.chars().filter(|c| *c != IN && *c != OUT).collect();
        let word_width = clean.chars().count();
        // The decision measures only as far as the **end of the region**
        // the word closes, which is where the nested document it belongs
        // to ends. A footnote's last word is measured without the text
        // that follows its closing brace, and pandoc lets the line
        // overrun rather than breaking one word earlier.
        let measured = word
            .split(OUT)
            .next()
            .map_or(word_width, |head| head.chars().filter(|c| *c != IN).count());
        if index == 0 {
            width = word_width;
        } else if !force && width.saturating_add(1 + measured) <= columns {
            out.push(' ');
            width += 1 + word_width;
        } else {
            // **The deeper of the two**, not their sum: a footnote's
            // wrapped lines sit at two, and a footnote inside a body
            // already indented two stays at two rather than going to
            // four. Measured on `corpus/gfm/footnotes.gfm` at 40 and 72.
            //
            // Once a region has **closed on this line**, its physical
            // indentation is behind us and the text is back at the
            // paragraph's own column: `\end{itemize}}, and a label…`
            // continues at zero however far the footnote body was in.
            let indent = if *depth == 0 && closed { 0 } else { base.max(2 * *depth) };
            let _ = write!(out, "\n{}", " ".repeat(indent));
            width = indent + word_width;
        }
        out.push_str(&clean);
        closed |= word.contains(OUT);
        *depth = depth
            .saturating_add(word.matches(IN).count())
            .saturating_sub(word.matches(OUT).count());
    }
}

/// Render a complete document, with the smallest preamble that compiles.
///
/// Deliberately small. Every package here is one the body can *need*:
/// `graphicx` for an image, `hyperref` for a link, `longtable`/`booktabs`
/// for a table. A preamble that loaded more would fail on a machine with a
/// minimal TeX installation, which is most of the machines this is for —
/// and that is not hypothetical. `ulem` was loaded here for `\sout`, and
/// it is *not* in `texlive-latex-base`: every document failed to compile
/// on the first CI run that tried. Strikeout is drawn from kernel
/// primitives instead, so the preamble needs nothing outside base LaTeX.
pub fn write_latex_standalone(doc: &Pandoc) -> String {
    let mut out = String::from(PREAMBLE);
    // **Only a document that highlights loads these**, which is the same
    // rule as everything else in this preamble: `fancyvrb` and `xcolor`
    // are not in `texlive-latex-base`, and a document with no highlighted
    // code must still compile where they are missing. Pandoc gates
    // `fancyvrb` the same way.
    if highlights_something(doc) {
        out.push_str(HIGHLIGHT_PREAMBLE);
    }
    if let Some(title) = doc.meta.get("title") {
        let _ = writeln!(out, "\\title{{{}}}", meta_text(title));
    }
    if let Some(author) = doc.meta.get("author") {
        let _ = writeln!(out, "\\author{{{}}}", meta_text(author));
    }
    if let Some(date) = doc.meta.get("date") {
        let _ = writeln!(out, "\\date{{{}}}", meta_text(date));
    }
    out.push_str("\\begin{document}\n");
    if doc.meta.contains_key("title") {
        out.push_str("\\maketitle\n");
    }
    out.push_str(&write_latex(doc));
    out.push_str("\\end{document}\n");
    out
}

/// Whether any code block in the document names a language.
///
/// The condition [`highlighted`] uses, asked of the whole tree — and it
/// has to reach **every** container, not the four a list and a quote
/// make obvious: a code block in a table cell or a footnote needs the
/// macros defined just as much, and a preamble that missed it would
/// produce a document that does not compile.
#[cfg(feature = "highlight")]
fn highlights_something(doc: &Pandoc) -> bool {
    fn walk(blocks: &[Block]) -> bool {
        blocks.iter().any(|block| match block {
            Block::CodeBlock(attr, _) => !attr.classes.is_empty(),
            Block::Div(_, inner) | Block::BlockQuote(inner) | Block::Figure(_, _, inner) => {
                walk(inner)
            }
            Block::BulletList(items) | Block::OrderedList(_, items) => {
                items.iter().any(|item| walk(item))
            }
            Block::DefinitionList(items) => {
                items.iter().any(|(_, defs)| defs.iter().any(|d| walk(d)))
            }
            Block::Table(table) => table
                .head
                .rows
                .iter()
                .chain(table.bodies.iter().flat_map(|b| b.head.iter().chain(&b.body)))
                .chain(&table.foot.rows)
                .flat_map(|row| &row.cells)
                .any(|cell| walk(&cell.blocks)),
            _ => false,
        })
    }
    walk(&doc.blocks)
}

#[cfg(not(feature = "highlight"))]
fn highlights_something(_doc: &Pandoc) -> bool {
    false
}

/// The packages and macros a highlighted block needs, appended to
/// [`PREAMBLE`] only for a document that has one.
///
/// `styleToLaTeX pygments`, from skylighting's **BSD-3**
/// `skylighting-format-latex`, on the same footing as the HTML
/// stylesheet — see `styles/LICENSE`.
#[cfg(feature = "highlight")]
const HIGHLIGHT_PREAMBLE: &str = concat!(
    "\\usepackage{fancyvrb}\n",
    "\\usepackage{xcolor}\n",
    include_str!("../styles/highlight.tex"),
);

#[cfg(not(feature = "highlight"))]
const HIGHLIGHT_PREAMBLE: &str = "";

const PREAMBLE: &str = concat!(
    "\\documentclass{article}\n",
    "\\usepackage[T1]{fontenc}\n",
    "\\usepackage[utf8]{inputenc}\n",
    "\\usepackage{graphicx}\n",
    "\\usepackage{longtable,booktabs}\n",
    // `\st` is `soul`'s, and `soul` is not in a base TeX. The name is
    // pandoc's, so a fragment pasted into a pandoc-produced document
    // already has it; `\providecommand` yields to theirs. Box the
    // argument, overlay a rule its width, then print it.
    "\\providecommand{\\st}[1]{{\\leavevmode\\setbox0=\\hbox{#1}%\n",
    "  \\rlap{\\rule[0.5ex]{\\wd0}{0.4pt}}\\box0}}\n",
    // `\tightlist` is pandoc's, and its reader needs it to tell a tight
    // list from a loose one: without it every `Plain` item comes back as
    // `Para`. `\providecommand` again, so a fragment pasted into a
    // pandoc-produced document does not clash with the identical macro.
    "\\providecommand{\\tightlist}{%\n",
    "  \\setlength{\\itemsep}{0pt}\\setlength{\\parskip}{0pt}}\n",
    // Every `\includegraphics` this writer emits is wrapped in
    // `\pandocbounded`, so the preamble has to define it or a document
    // with one picture stops compiling. This is pandoc's own definition,
    // from `templates/common.latex`, and it needs only `graphicx` — which
    // is loaded above for the picture itself.
    "\\makeatletter\n",
    "\\newsavebox\\pandoc@box\n",
    "\\providecommand*\\pandocbounded[1]{% scales image to fit in text height/width\n",
    "  \\sbox\\pandoc@box{#1}%\n",
    "  \\Gscale@div\\@tempa{\\textheight}{\\dimexpr\\ht\\pandoc@box+\\dp\\pandoc@box\\relax}%\n",
    "  \\Gscale@div\\@tempb{\\linewidth}{\\wd\\pandoc@box}%\n",
    "  \\ifdim\\@tempb\\p@<\\@tempa\\p@\\let\\@tempa\\@tempb\\fi%\n",
    "  \\ifdim\\@tempa\\p@<\\p@\\scalebox{\\@tempa}{\\usebox\\pandoc@box}%\n",
    "  \\else\\usebox{\\pandoc@box}%\n",
    "  \\fi%\n",
    "}\n",
    "\\makeatother\n",
    // Loaded last, as hyperref asks.
    "\\usepackage{hyperref}\n",
);

/// The heading macros, deepest last. LaTeX's `article` class stops at
/// `\paragraph`; a level below that has nowhere to go and takes the last.
const SECTIONS: &[&str] = &[
    "section",
    "subsection",
    "subsubsection",
    "paragraph",
    "subparagraph",
];

/// Render a run of blocks, separated by a blank line and with none after
/// the last.
///
/// The separation matters more than it looks: a blank line after an
/// `\item` makes the item a separate paragraph, and pandoc then reads the
/// whole list as `DefaultStyle` rather than the numbering it was given.
fn blocks(list: &[Block], depth: usize, colour: bool, out: &mut String) {
    let mut first = true;
    for block in list {
        let mut text = String::new();
        block_to(block, depth, colour, &mut text);
        let text = text.trim_end_matches('\n');
        // A raw block in somebody else's format renders to nothing, and
        // pandoc's separator goes with it: emitting the blank line anyway
        // left `corpus/code-and-raw.md` with four trailing empty lines
        // and a paragraph break where the document had none.
        if text.is_empty() {
            continue;
        }
        if !first {
            out.push('\n');
        }
        first = false;
        out.push_str(text);
        out.push('\n');
    }
}

fn block_to(block: &Block, depth: usize, colour: bool, out: &mut String) {
    match block {
        Block::Plain(list) | Block::Para(list) => {
            inlines(list, colour, out);
            out.push('\n');
        }
        Block::LineBlock(lines) => {
            // `\\` between lines and a blank line after, which is what a
            // verse block is in LaTeX without loading a package for it.
            let rendered: Vec<String> = lines
                .iter()
                .map(|line| {
                    let mut text = String::new();
                    inlines(line, colour, &mut text);
                    text
                })
                .collect();
            let _ = writeln!(out, "{}\n", rendered.join("\\\\\n"));
        }
        Block::CodeBlock(attr, code) => code_block_to(attr, code, colour, out),
        Block::BlockQuote(inner) => {
            out.push_str("\\begin{quote}\n");
            blocks(inner, depth, colour, out);
            out.push_str("\\end{quote}\n");
        }
        Block::OrderedList(attrs, items) => ordered_list_to(attrs, items, depth, colour, out),
        Block::BulletList(items) => {
            out.push_str("\\begin{itemize}\n");
            tightlist(items, out);
            for item in items {
                item_to(item, depth, colour, out);
            }
            out.push_str("\\end{itemize}\n");
        }
        Block::DefinitionList(entries) => definition_list_to(entries, depth, colour, out),
        Block::Header(level, attr, list) => {
            let index = usize::try_from(*level).unwrap_or(1).saturating_sub(1);
            let mut rendered = String::new();
            inlines(list, colour, &mut rendered);
            // Below `\subparagraph` LaTeX has nowhere to put a heading, and
            // pandoc writes the text as an ordinary paragraph rather than
            // pushing it into the deepest macro it has. Following it costs
            // nothing: neither spelling carries the level.
            let Some(macro_name) = SECTIONS.get(index) else {
                out.push_str(&rendered);
                out.push('\n');
                return;
            };
            // A **short title** for the running head, where the heading
            // holds something that cannot go in one. Pandoc's rule,
            // probed: a `Note` or an `Image` earns the optional argument
            // and nothing else does — and the short title drops both,
            // where `\texorpdfstring`'s plain half keeps an image's alt
            // text. The two are not the same string.
            if fragile(list) {
                // The short title is **rendered**, not stringified:
                // `\texttt{code} and \emph{emphasis}` keeps its markup
                // there, and only the note or picture is left out.
                let kept: Vec<Inline> = list
                    .iter()
                    .filter(|inline| !matches!(inline, Inline::Note(_) | Inline::Image(..)))
                    .cloned()
                    .collect();
                let mut short = String::new();
                inlines(&kept, colour, &mut short);
                // **An empty one is written as none at all.** With any
                // text beside the picture the argument stays, trailing
                // space and all — `\\subsection[a ]`. Probed both ways.
                let arg = if short.is_empty() { short } else { format!("[{short}]") };
                let _ = write!(out, "\\{macro_name}{arg}");
            } else {
                let _ = write!(out, "\\{macro_name}");
            }
            out.push('{');
            // `\texorpdfstring{typeset}{bookmark}` when the two differ.
            // A PDF bookmark is plain text: `\emph` in one stops hyperref
            // with "Token not allowed in a PDF string". Pandoc's own test
            // is exactly this — the heading rendered, against the heading
            // stringified and escaped — so a heading of nothing but words
            // stays a bare argument.
            // A heading is a moving argument, so a fragile command in it
            // needs `\protect` — `\pandocbounded` is one, and without the
            // guard `hyperref` stops on the bookmark.
            let rendered = rendered.replace("\\pandocbounded{", "\\protect\\pandocbounded{");
            // The rendered text carries break marks and the stringified
            // text does not, so the two are compared with the marks taken
            // out — otherwise every heading in the document acquired a
            // `\texorpdfstring` the moment the writer learned to fill.
            let plain = escape(&stringify(list));
            if plain == rendered.replace([BREAK, SOFT], " ") {
                out.push_str(&rendered);
            } else {
                let _ = write!(out, "\\texorpdfstring{{{rendered}}}{{{plain}}}");
            }
            out.push('}');
            // The label is what makes a cross-reference resolve. Without
            // one the heading still reads back with the right identifier,
            // because pandoc derives it from the text — but nothing in the
            // document can point at it.
            if !attr.identifier.is_empty() {
                let _ = write!(out, "\\label{{{}}}", label(&attr.identifier));
            }
            out.push('\n');
        }
        Block::HorizontalRule => out.push_str("\\begin{center}\\rule{0.5\\linewidth}{0.5pt}\\end{center}\n"),
        Block::Table(table) => table_to(table, colour, out),
        Block::Figure(_, caption, inner) => {
            out.push_str("\\begin{figure}\n\\centering\n");
            blocks(inner, depth, colour, out);
            if !caption.blocks.is_empty() {
                out.push_str("\\caption{");
                caption_text(caption, colour, out);
                out.push_str("}\n");
            }
            out.push_str("\\end{figure}\n");
        }
        // A div carries no LaTeX of its own; its content is the content.
        // LaTeX has no grouping block, so a div is its content — but an
        // identifier on it is an anchor, and pandoc keeps that.
        Block::Div(attr, inner) => {
            if !attr.identifier.is_empty() {
                label_to(&attr.identifier, out);
                out.push('\n');
            }
            blocks(inner, depth, colour, out);
        }
        // Raw content is another format's syntax — except LaTeX's own,
        // which is passed through as written. That is the whole point of
        // a raw block.
        Block::RawBlock(format, text) => {
            if format.0 == "latex" || format.0 == "tex" {
                out.push_str(text);
                out.push('\n');
            }
        }
    }
}

/// A `description` environment, one `\item[term]` per entry.
fn definition_list_to(
    entries: &[(Vec<Inline>, Vec<Vec<Block>>)],
    depth: usize,
    colour: bool,
    out: &mut String,
) {
            out.push_str("\\begin{description}\n");
            // **A description list is tight on the same rule as the other
            // two**: every definition opening with a `Plain` rather than a
            // `Para`. Pandoc writes `\tightlist` for one and this wrote
            // none, so a tight definition list rendered with paragraph
            // spacing pandoc does not give it.
            let bodies: Vec<Vec<Block>> =
                entries.iter().flat_map(|(_, d)| d.iter().cloned()).collect();
            tightlist(&bodies, out);
            for (term, definitions) in entries {
                out.push_str("\\item[");
                inlines(term, colour, out);
                // The spaces either side of the tilde are pandoc's.
                let filler = definitions.first().is_some_and(|d| definition_needs_filler(d));
                out.push_str(if filler { "] ~ \n" } else { "]\n" });
                // **A blank line between definitions.** Without it two
                // definitions run into one paragraph.
                for (index, definition) in definitions.iter().enumerate() {
                    if index > 0 {
                        out.push('\n');
                    }
                    blocks(definition, depth, colour, out);
                }
            }
            out.push_str("\\end{description}\n");
}

/// `\protect\phantomsection\label{id}`, the anchor pandoc writes for
/// an identifier LaTeX has nowhere else to put.
///
/// `\phantomsection` is what makes `\label` point at the right place
/// when there is no sectioning command to attach it to, and `\protect`
/// keeps it safe inside a moving argument. Nothing at all for an empty
/// identifier.
fn label_to(identifier: &str, out: &mut String) {
    if !identifier.is_empty() {
        let _ = write!(out, "\\protect\\phantomsection\\label{{{identifier}}}");
    }
}

/// The `\labelenumi` a numbering style and delimiter call for.
///
/// Always emitted, even for the arabic default: pandoc's reader reports
/// Write `\tightlist` when every item opens with a `Plain`, which is what
/// a tight list is. Pandoc emits it and its reader keys on it: a list
/// written without it reads back with every item promoted to `Para`, so
/// this is the difference between a round trip and a divergence, not a
/// matter of spacing. An empty list is not tight — pandoc emits nothing.
fn tightlist(items: &[Vec<Block>], out: &mut String) {
    if !items.is_empty() && items.iter().all(|item| item_is_tight(item)) {
        out.push_str("\\tightlist\n");
    }
}

/// Whether one list item is tight.
///
/// A first block that is `Plain` is the ordinary case. **An item that is
/// nothing but a nested list is tight when that list is** — pandoc marks
/// the outer `itemize` of `bullets([bullets([plain, plain])])` and this
/// did not, because it only ever looked at the first block's type.
///
/// The recursion is narrow on purpose, and the shapes either side of it
/// were measured: `[list, Plain]` is **not** tight even though the list
/// is, and neither is `[Para, list]` or a lone list of `Para`s.
fn item_is_tight(item: &[Block]) -> bool {
    match item {
        [Block::Plain(_), ..] => true,
        [Block::BulletList(inner)] => !inner.is_empty() && inner.iter().all(|i| item_is_tight(i)),
        [Block::OrderedList(_, inner)] => {
            !inner.is_empty() && inner.iter().all(|i| item_is_tight(i))
        }
        _ => false,
    }
}

/// One `\item`, with its content indented two spaces the way pandoc's
/// writer indents it — recursively, so a list inside a list is four.
///
/// A blank line stays empty: indenting it would put trailing whitespace
/// where pandoc has none, and the bytes are the test.
/// A code block: highlighted where there is a language and a
/// highlighter, and `verbatim` otherwise.
///
/// `verbatim` rather than `lstlisting`: it needs no package, so the
/// output compiles on a minimal TeX installation.
fn code_block_to(attr: &ferrodoc_ast::Attr, code: &str, colour: bool, out: &mut String) {
    if let Some(text) = colour.then(|| highlighted(attr, code)).flatten() {
        out.push_str(&text);
    } else {
        let _ = writeln!(out, "\\begin{{verbatim}}\n{}\n\\end{{verbatim}}", code.trim_end());
    }
}

/// Whether `\item` needs a `~` before the block that follows it.
///
/// `\item` expects something on its own line and a **heading** puts a
/// sectioning macro there instead, so pandoc writes `\item ~` and lets
/// the heading start the next line. Probed one first-block at a time: a
/// code block, a quote, a rule, a table and a line block all need
/// nothing, and it is the heading alone.
fn item_needs_filler(item: &[Block]) -> bool {
    matches!(item.first(), Some(Block::Header(..)))
}

/// The same for a **definition**, where the set is not the same: a code
/// block needs it there and does not in a list item. Both measured, not
/// generalised from the one.
fn definition_needs_filler(definition: &[Block]) -> bool {
    matches!(definition.first(), Some(Block::Header(..) | Block::CodeBlock(..)))
}

fn item_to(item: &[Block], depth: usize, colour: bool, out: &mut String) {
    // A task item's box reaches this writer as the `☐`/`☒` the GFM reader
    // makes of it, and LaTeX's is the optional argument of `\item`.
    // Written as text it set as a missing glyph in most fonts.
    let (label, item) = task_box(item);
    let filler = if item_needs_filler(&item) { " ~" } else { "" };
    let _ = writeln!(out, "\\item{label}{filler}");
    let mut text = String::new();
    blocks(&item, depth, colour, &mut text);
    // A `verbatim` environment is flush left however deep it sits: its
    // content is literal, so two spaces of item indentation would be two
    // spaces of code. Pandoc renders it with `flush`, which is the same
    // rule stated in its own layout language.
    //
    // **`Shaded` is literal for the same reason** — it wraps a
    // `Highlighting`, which is `fancyvrb`'s `Verbatim` — so a highlighted
    // block in a list item needs the identical treatment. It did not get
    // it the day highlighting landed, and every line of it came out two
    // spaces to the right of pandoc's.
    let mut literal = false;
    for line in text.lines() {
        if line == "\\begin{verbatim}" || line == "\\begin{Shaded}" {
            literal = true;
        }
        if literal || line.is_empty() {
            let _ = writeln!(out, "{line}");
        } else {
            let _ = writeln!(out, "  {line}");
        }
        if line == "\\end{verbatim}" || line == "\\end{Shaded}" {
            literal = false;
        }
    }
}

/// `\begin{enumerate}` with the counter its nesting depth calls for.
fn ordered_list_to(
    attrs: &ferrodoc_ast::ListAttributes,
    items: &[Vec<Block>],
    depth: usize,
    colour: bool,
    out: &mut String,
) {
    out.push_str("\\begin{enumerate}\n");
    // LaTeX has one counter per nesting level and pandoc names it by
    // depth — `enumi`, `enumii`, `enumiii` — counting only the enclosing
    // `enumerate`s, so a bullet list in between does not advance it.
    // Measured; a list two deep that said `enumi` renumbered its parent.
    let counter = format!("enum{}", roman(depth + 1));
    // `\setcounter` **before** `\def\label…`, and the order is not
    // cosmetic: pandoc's reader takes the start value from the first
    // directive it meets and stops looking, so a list that says `\def`
    // first begins at 1 whatever it asked for. Measured both ways round.
    // **This is the one place this writer deliberately differs from
    // pandoc's bytes** — pandoc writes `\def` first and its own reader
    // then loses the start value. `COMPATIBILITY.md` records it.
    if attrs.start != 1 {
        let _ = writeln!(out, "\\setcounter{{{counter}}}{{{}}}", attrs.start - 1);
    }
    // **A list that names neither a style nor a delimiter gets no
    // `\def`** — `enumerate`'s own default is what it asked for, and
    // pandoc leaves it alone.
    if attrs.style != ListNumberStyle::DefaultStyle
        || attrs.delim != ListNumberDelim::DefaultDelim
    {
        let _ = writeln!(
            out,
            "\\def\\label{counter}{{{}}}",
            enumerate_style(attrs.style, attrs.delim, &counter)
        );
    }
    tightlist(items, out);
    for item in items {
        item_to(item, depth + 1, colour, out);
    }
    out.push_str("\\end{enumerate}\n");
}

/// The `\item` label a task box calls for, and the item with the box
/// taken out of it. Empty label and the item unchanged when there is no
/// box.
fn task_box(item: &[Block]) -> (String, Vec<Block>) {
    let Some(Block::Plain(list) | Block::Para(list)) = item.first() else {
        return (String::new(), item.to_vec());
    };
    let Some((Inline::Str(mark), [Inline::Space, rest @ ..])) = list.split_first() else {
        return (String::new(), item.to_vec());
    };
    let label = match mark.as_str() {
        "\u{2610}" => "[$\\square$]",
        "\u{2612}" => "[$\\boxtimes$]",
        _ => return (String::new(), item.to_vec()),
    };
    let mut stripped = item.to_vec();
    stripped[0] = match &item[0] {
        Block::Para(_) => Block::Para(rest.to_vec()),
        _ => Block::Plain(rest.to_vec()),
    };
    (label.to_owned(), stripped)
}

/// A nesting depth as the lowercase roman numeral LaTeX spells its
/// counters with: `enumi`, `enumii`, … Pandoc keeps going past `enumiv`,
/// which LaTeX does not define — following it is the byte-identical
/// answer and the deeper list was already outside what `article` sets.
fn roman(mut n: usize) -> String {
    const NUMERALS: [(usize, &str); 7] =
        [(100, "c"), (90, "xc"), (50, "l"), (40, "xl"), (10, "x"), (9, "ix"), (5, "v")];
    let mut out = String::new();
    for (value, numeral) in NUMERALS {
        while n >= value {
            out.push_str(numeral);
            n -= value;
        }
    }
    if n == 4 {
        out.push_str("iv");
    } else {
        for _ in 0..n {
            out.push('i');
        }
    }
    out
}

/// `DefaultStyle` for a bare `enumerate`, so a `Decimal` list that says
/// nothing comes back as a different list.
fn enumerate_style(style: ListNumberStyle, delim: ListNumberDelim, name: &str) -> String {
    let macro_name = match style {
        ListNumberStyle::LowerAlpha => "alph",
        ListNumberStyle::UpperAlpha => "Alph",
        ListNumberStyle::LowerRoman => "roman",
        ListNumberStyle::UpperRoman => "Roman",
        _ => "arabic",
    };
    let counter = format!("\\{macro_name}{{{name}}}");
    match delim {
        ListNumberDelim::TwoParens => format!("({counter})"),
        ListNumberDelim::OneParen => format!("{counter})"),
        // Period, and the default, which pandoc also renders as a period.
        ListNumberDelim::Period | ListNumberDelim::DefaultDelim => format!("{counter}."),
    }
}

fn table_to(table: &Table, colour: bool, out: &mut String) {
    let columns = table.colspecs.len().max(1);
    // **A column stating its own width gets a `p{…}` of that width**,
    // which is how a table converted from DOCX, ODT or HTML keeps its
    // proportions; a table that states none gets the bare `l`/`c`/`r`.
    // Measured on `pandoc -f json -t latex`: the available width is
    // `\linewidth` less **2(n-1)** `\tabcolsep`, the fraction is written
    // to four places, and each column is its own line indented two.
    let sized = table
        .colspecs
        .iter()
        .any(|colspec| colspec.width != ferrodoc_ast::ColWidth::ColWidthDefault);
    let letter = |alignment| match alignment {
        ferrodoc_ast::Alignment::AlignRight => 'r',
        ferrodoc_ast::Alignment::AlignCenter => 'c',
        _ => 'l',
    };
    let ragged = |alignment| match alignment {
        ferrodoc_ast::Alignment::AlignRight => "\\raggedleft",
        ferrodoc_ast::Alignment::AlignCenter => "\\centering",
        _ => "\\raggedright",
    };
    let spec: String = if table.colspecs.is_empty() {
        "l".repeat(columns)
    } else if sized {
        let gaps = 2 * columns.saturating_sub(1);
        let mut lines = String::from("\n");
        for colspec in &table.colspecs {
            let fraction = match colspec.width {
                ferrodoc_ast::ColWidth::ColWidth(fraction) => fraction,
                ferrodoc_ast::ColWidth::ColWidthDefault => 0.0,
            };
            let _ = writeln!(
                lines,
                "  >{{{}\\arraybackslash}}p{{(\\linewidth - {gaps}\\tabcolsep) * \\real{{{fraction:.4}}}}}",
                ragged(colspec.alignment)
            );
        }
        lines.pop();
        lines
    } else {
        table.colspecs.iter().map(|colspec| letter(colspec.alignment)).collect()
    };
    // A `longtable` with no caption still advances LaTeX's table counter,
    // so a document of uncaptioned tables numbers figures that are not
    // there. Pandoc wraps it in a group that redefines `\LTcaptype`, and
    // the comment is part of the bytes.
    let captioned = !table.caption.blocks.is_empty();
    if !captioned {
        out.push_str("{\\def\\LTcaptype{none} % do not increment counter\n");
    }
    // `longtable` rather than `tabular`: a table longer than a page is
    // ordinary in a converted document, and `tabular` silently runs off
    // the bottom of it.
    let _ = writeln!(out, "\\begin{{longtable}}[]{{@{{}}{spec}@{{}}}}");
    if captioned {
        out.push_str("\\caption{");
        caption_text(&table.caption, colour, out);
        out.push_str("}\\tabularnewline\n");
    }
    // **The head and the foot are declared before the body.** `\endhead`
    // is what a `longtable` repeats at the top of each page and
    // `\endlastfoot` at the bottom of the last, so the rules come first
    // and the body rows follow. Written the other way round the rules are
    // ordinary rows and the table has no repeating head at all.
    let mut head = String::new();
    head.push_str("\\toprule\\noalign{}\n");
    let aligns: Vec<&str> =
        table.colspecs.iter().map(|colspec| ragged(colspec.alignment)).collect();
    for row in &table.head.rows {
        if sized {
            row_to_boxed(row, columns, &aligns, colour, &mut head);
        } else {
            row_to(row, columns, colour, &mut head);
        }
    }
    if !table.head.rows.is_empty() {
        head.push_str("\\midrule\\noalign{}\n");
    }
    // **A captioned table declares its head twice.** The caption belongs
    // to the first page only, so `\endfirsthead` closes a copy that
    // carries it and `\endhead` closes the one every later page
    // repeats. Written once, a table broken across pages loses its
    // header on page two.
    if captioned {
        out.push_str(&head);
        out.push_str("\\endfirsthead\n");
    }
    out.push_str(&head);
    out.push_str("\\endhead\n\\bottomrule\\noalign{}\n\\endlastfoot\n");
    for body in &table.bodies {
        for row in body.head.iter().chain(&body.body) {
            row_to(row, columns, colour, out);
        }
    }
    for row in &table.foot.rows {
        row_to(row, columns, colour, out);
    }
    out.push_str("\\end{longtable}\n");
    if !captioned {
        out.push_str("}\n");
    }
}

/// One row. `minipage` wraps each cell the way pandoc wraps a **header**
/// row of a table whose columns state their widths: a `p{…}` column is
/// bottom-aligned, so the header needs a box to sit in or it rides low
/// against the rule. Body rows take no such wrapper.
fn row_to_boxed(row: &Row, columns: usize, aligns: &[&str], colour: bool, out: &mut String) {
    let cells: Vec<String> = row
        .cells
        .iter()
        .enumerate()
        .map(|(index, cell)| {
            let text = cell_text(cell, colour);
            let ragged = aligns.get(index).copied().unwrap_or("\\raggedright");
            format!("\\begin{{minipage}}[b]{{\\linewidth}}{ragged}\n{text}\n\\end{{minipage}}")
        })
        .collect();
    let mut cells = cells;
    cells.resize(columns, String::new());
    let row = cells.join(" & ");
    let _ = writeln!(out, "{row} \\\\");
}

fn row_to(row: &Row, columns: usize, colour: bool, out: &mut String) {
    let mut cells: Vec<String> = row.cells.iter().map(|c| cell_text(c, colour)).collect();
    cells.resize(columns, String::new());
    // `\\` and not `\tabularnewline`: both end a row, and pandoc writes
    // the short one. The row is trimmed first, so an empty cell at either
    // end leaves one space before the `\\` rather than two.
    // The gap after each `&` is a break mark: pandoc fills a table row.
    let row = cells.join(&format!(" &{BREAK}"));
    let _ = writeln!(out, "{} \\\\", row.trim_matches(|c: char| c.is_whitespace() || c == BREAK));
}

fn cell_text(cell: &Cell, colour: bool) -> String {
    let mut out = String::new();
    for block in &cell.blocks {
        match block {
            Block::Plain(list) | Block::Para(list) => inlines(list, colour, &mut out),
            other => block_to(other, 0, colour, &mut out),
        }
    }
    // A cell is one line: a newline inside `&`-separated content ends the
    // row early and the table stops making sense. It becomes a break mark
    // rather than a space, because pandoc **does** fill a table row.
    out.replace('\n', &BREAK.to_string()).trim().to_owned()
}

fn caption_text(caption: &Caption, colour: bool, out: &mut String) {
    for block in &caption.blocks {
        match block {
            Block::Plain(list) | Block::Para(list) => inlines(list, colour, out),
            other => block_to(other, 0, colour, out),
        }
    }
}

/// Render a run of inlines, collapsing the space a dropped inline leaves
/// behind.
///
/// Pandoc builds its output as a `Doc` in which two breaking spaces with
/// nothing between them are one space, and a raw inline in another format
/// renders to nothing — so `plus <br/> and` is `plus and` there and was
/// `plus  and` here. The flag is what the `Doc` does implicitly: an empty
/// render does not clear it, so the two spaces around it meet.
fn inlines(list: &[Inline], colour: bool, out: &mut String) {
    let mut after_break = false;
    // **`\\` needs a line to break, and a run that opens with one has
    // none** — `\emph{\\}` is a LaTeX error, so pandoc writes
    // `\hfill\break` there. Not a rule about containers: `\emph{a\\` is
    // what both write, and a break opening a *top-level* paragraph takes
    // `\hfill\break` too. Decided here because `inline_to` renders each
    // inline into a buffer of its own and cannot see what came before it.
    let mut emitted = false;
    for inline in list {
        let breaking = matches!(inline, Inline::Space | Inline::SoftBreak);
        if breaking && after_break {
            continue;
        }
        if matches!(inline, Inline::LineBreak) && !emitted {
            out.push_str("\\hfill\\break\n");
            emitted = true;
            after_break = false;
            continue;
        }
        let mut piece = String::new();
        inline_to(inline, colour, &mut piece);
        if piece.is_empty() {
            continue;
        }
        out.push_str(&piece);
        emitted = true;
        after_break = breaking;
    }
}

/// A `\\footnote{…}`, its content laid out as blocks.
///
/// Lifted out of [`inline_to`] when that function passed clippy's
/// hundred lines — by exact replacement, because counting braces to
/// find an arm's end ate the rest of a `match` earlier in the day.
fn note_to(blocks_in_note: &[Block], colour: bool, out: &mut String) {

            out.push_str("\\footnote{");
            out.push(IN);
            let mut text = String::new();
            blocks(blocks_in_note, 0, colour, &mut text);
            // Every line but the first is indented two, the way an
            // `\item`'s content is: a footnote of more than one block is
            // laid out as a block, not run together.
            // **A literal environment is flush left here too**, for the
            // reason it is inside an `\item`: `Highlighting` is
            // `fancyvrb`'s `Verbatim` and two spaces of indentation are
            // two spaces of code. The rule was written for list items and
            // a footnote holding a code block needed it just as much.
            let mut literal = false;
            for (index, line) in text.trim().lines().enumerate() {
                if line == "\\begin{verbatim}" || line == "\\begin{Shaded}" {
                    literal = true;
                }
                if index > 0 {
                    out.push('\n');
                    if !line.is_empty() && !literal {
                        out.push_str("  ");
                    }
                }
                out.push_str(line);
                if line == "\\end{verbatim}" || line == "\\end{Shaded}" {
                    literal = false;
                }
            }
            // **The closing environment's own newline survives the trim**:
            // pandoc writes `\\end{Shaded}` and then the brace on its own
            // line. Asked of the *last* line rather than latched during
            // the loop, which added the newline to a footnote whose code
            // block was followed by a paragraph.
            if matches!(
                text.trim().lines().last(),
                Some("\\end{verbatim}" | "\\end{Shaded}")
            ) {
                out.push('\n');
            }
            out.push(OUT);
            out.push('}');
}

fn inline_to(inline: &Inline, colour: bool, out: &mut String) {
    let wrap = |name: &str, inner: &[Inline], out: &mut String| {
        let _ = write!(out, "\\{name}{{");
        inlines(inner, colour, out);
        out.push('}');
    };
    match inline {
        Inline::Str(text) => out.push_str(&escape(text)),
        Inline::Space => out.push(BREAK),
        Inline::SoftBreak => out.push(SOFT),
        Inline::LineBreak => out.push_str("\\\\\n"),
        Inline::Emph(inner) => wrap("emph", inner, out),
        Inline::Strong(inner) => wrap("textbf", inner, out),
        // `\ul` is ulem's, which the template loads and pandoc uses;
        // `\underline` does not break across lines.
        Inline::Underline(inner) => wrap("ul", inner, out),
        // `ulem`'s name, but the preamble defines it: LaTeX has no
        // strikeout of its own and `ulem` is not in a base TeX.
        Inline::Strikeout(inner) => wrap("st", inner, out),
        Inline::Superscript(inner) => wrap("textsuperscript", inner, out),
        Inline::Subscript(inner) => wrap("textsubscript", inner, out),
        Inline::SmallCaps(inner) => wrap("textsc", inner, out),
        Inline::Quoted(kind, inner) => {
            let (open, close) = match kind {
                QuoteType::SingleQuote => ("`", "'"),
                QuoteType::DoubleQuote => ("``", "''"),
            };
            out.push_str(open);
            inlines(inner, colour, out);
            out.push_str(close);
        }
        // **A span is braced, and an identifier on it is an anchor.**
        // LaTeX has no form for the classes, but `\label` carries the
        // name, and the braces are what pandoc writes even for a span
        // that says nothing else.
        Inline::Span(attr, inner) => {
            label_to(&attr.identifier, out);
            out.push('{');
            inlines(inner, colour, out);
            out.push('}');
        }
        // A citation is its content: LaTeX has no form that survives
        // pandoc's reader.
        Inline::Cite(_, inner) => inlines(inner, colour, out),
        Inline::Code(_, code) => out.push_str(&verbatim(code)),
        Inline::Math(kind, math) => {
            let (open, close) = match kind {
                ferrodoc_ast::MathType::InlineMath => ("\\(", "\\)"),
                ferrodoc_ast::MathType::DisplayMath => ("\\[", "\\]"),
            };
            let _ = write!(out, "{open}{math}{close}");
        }
        Inline::RawInline(format, text) => {
            if format.0 == "latex" || format.0 == "tex" {
                out.push_str(text);
            }
        }
        Inline::Link(attr, inner, target) => {
            label_to(&attr.identifier, out);
            // A URL is not escaped the way text is — `\href`'s first
            // argument is verbatim-ish, and escaping `~` or `%` there
            // breaks the link rather than the typesetting.
            let url = escape_url(&target.url);
            // A link whose text *is* its target is what an autolink
            // becomes, and pandoc has two shorter spellings for it:
            // `\url` sets the address in the URL font and lets it break,
            // and `\nolinkurl` does the same inside a `mailto:` without
            // making the address a second link. `\href{u}{u}` renders the
            // address in body text with no break points, so this is a
            // typesetting difference as well as a byte one.
            // …but only when the text is *bare*. `[`a.md`](a.md)` has a
            // `Code` for its text and stringifies to the URL just the
            // same, and pandoc writes `\href{a.md}{\texttt{a.md}}` for
            // it: the short spellings are for an autolink, whose text is
            // one `Str` and nothing else. `ROADMAP.md` is full of
            // `[`COMPATIBILITY.md`](COMPATIBILITY.md)`, and every one of
            // them lost its code font.
            let bare = matches!(inner.as_slice(), [Inline::Str(_)]);
            let text = stringify(inner);
            if bare && text == target.url {
                let _ = write!(out, "\\url{{{url}}}");
            } else if bare && target.url.strip_prefix("mailto:") == Some(text.as_str()) {
                let _ = write!(out, "\\href{{{url}}}{{\\nolinkurl{{{}}}}}", escape_url(&text));
            } else {
                let _ = write!(out, "\\href{{{url}}}{{");
                inlines(inner, colour, out);
                out.push('}');
            }
        }
        Inline::Image(_, alt, target) => {
            // The alt text goes in the `alt=` option: it is the only place
            // `\includegraphics` has for it, and without it the words are
            // simply gone.
            let mut text = String::new();
            inlines(alt, colour, &mut text);
            // The whole picture is one unbreakable unit: pandoc never
            // breaks inside `alt={…}` however narrow the column, so the
            // marks come out of the alt text here.
            let text = text.replace([BREAK, SOFT], " ");
            // `\pandocbounded` is pandoc's, defined in its default
            // template and in [`PREAMBLE`] here: it scales a picture down
            // to the text block when it would overflow and leaves it alone
            // when it fits. Without it a photograph runs off the page,
            // which is what every image in the corpus did.
            let options = if text.is_empty() {
                "keepaspectratio".to_owned()
            } else {
                format!("keepaspectratio,alt={{{text}}}")
            };
            let _ = write!(
                out,
                "\\pandocbounded{{\\includegraphics[{options}]{{{}}}}}",
                escape_url(&target.url)
            );
        }
        Inline::Note(blocks_in_note) => note_to(blocks_in_note, colour, out),
    }
}

/// The plain text of an inline sequence, as pandoc's `stringify` produces
/// it — every break a space, raw content and footnotes contributing
/// nothing. Used for the PDF-bookmark half of `\texorpdfstring`.
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

/// An identifier as a LaTeX label.
///
/// **Not [`escape`]:** a label is a name rather than text, so pandoc
/// keeps the ASCII alphanumerics and `-_:.` and spells everything else
/// `ux` plus its codepoint in hex. Escaping it as text turned
/// `punctuation--symbols` into `punctuation-\/-symbols` — the ligature
/// break — and left `ünïcode` as itself where pandoc writes `uxfcnuxef`.
fn label(identifier: &str) -> String {
    let mut out = String::with_capacity(identifier.len());
    for ch in identifier.chars() {
        if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | ':' | '.') {
            out.push(ch);
        } else {
            let _ = write!(out, "ux{:x}", ch as u32);
        }
    }
    out
}

/// Whether the heading holds something a running head cannot: a footnote
/// or a picture. Either earns the optional short-title argument, and
/// nothing else does — probed against the pinned binary.
fn fragile(inlines: &[Inline]) -> bool {
    inlines.iter().any(|inline| match inline {
        Inline::Note(_) | Inline::Image(..) => true,
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
        | Inline::Link(_, inner, _) => fragile(inner),
        _ => false,
    })
}

/// Escape the characters LaTeX gives a meaning to.
///
/// Seven take a backslash (`# $ % & _ { }`). The rest have no backslash
/// form: `\\` is a line break rather than a backslash and `\^` is an accent
/// waiting for a letter, so those need a `\text…` command or a group, each
/// followed by `{}` so a following space survives. Every spelling here is
/// pandoc's, probed rather than chosen:
///
/// ```sh
/// printf 'X<Y' | pandoc -f commonmark -t latex   # X\textless Y
/// ```
///
/// The one remaining difference is that pandoc ends a control word with a
/// space where a letter follows (`\textless Y`) and with `{}` only at the
/// end of a run; this always writes `{}`, which renders identically.
/// `COMPATIBILITY.md` records it.
fn escape(text: &str) -> String {
    escape_run(text, true)
}

/// The body of [`escape`], shared with [`verbatim`], which passes `false`.
///
/// `typographic` is the one difference between the two contexts and it is
/// measured, not assumed: pandoc turns `—` into `---` and `…` into
/// `\ldots` in running text and leaves both **as themselves** inside
/// `\texttt`, while the space-like characters (`\u{a0}`, `\u{202f}`,
/// `\u{ad}`, `\u{200b}`) are escaped in either place.
fn escape_run(text: &str, typographic: bool) -> String {
    let mut out = String::with_capacity(text.len());
    let mut open_word = false;
    let mut chars = text.chars().peekable();
    while let Some(ch) = chars.next() {
        let next = chars.peek().copied();
        if open_word {
            out.push_str(terminator(ch));
        }
        open_word = escape_char_in(ch, next, typographic, &mut out);
    }
    if open_word {
        out.push_str("{}");
    }
    out
}

/// What has to follow a control word so the next character still means
/// itself — pandoc's rule, probed character by character:
///
/// ```sh
/// printf 'X<Y' | pandoc -f commonmark -t latex   # X\textless Y
/// printf 'X< Y' | pandoc -f commonmark -t latex  # X\textless{} Y
/// ```
///
/// A space is the cheapest terminator and LaTeX eats it, which is right
/// before a letter and wrong before a real space — hence `{}` there, and
/// nothing at all before a character that ends the word by itself.
fn terminator(next: char) -> &'static str {
    match next {
        // A letter would be read as more of the word's name. Not
        // `is_whitespace` below and `is_alphabetic` here by accident:
        // a non-breaking space is alphabetic to neither and spells as
        // `~`, which needs nothing.
        ch if ch.is_alphabetic() => " ",
        ' ' | '\t' | '\n' | '\r' => "{}",
        _ => "",
    }
}

/// One character, escaped. Returns whether what it wrote ends in a
/// control word, which [`escape`] then terminates. Shared with
/// [`verbatim`], which adds two rules of its own rather than keeping a
/// second copy of these.
/// One character, with what follows it and whether the typographic
/// replacements apply. See [`escape_run`] for why the two contexts differ.
///
/// `next` exists for one rule: pandoc breaks the `--` and `---` ligatures
/// by writing `-\/-`, so a hyphen followed by a hyphen carries an
/// italic correction. Without it `\texttt{--wrap}` set as `\texttt{–wrap}`
/// and the flag in a README changed name.
fn escape_char_in(ch: char, next: Option<char>, typographic: bool, out: &mut String) -> bool {
    if typographic {
        // Probed one codepoint at a time against the pinned binary; the
        // set is exactly this, and `–`/`—` are *not* re-processed by the
        // hyphen rule below — pandoc writes `---`, not `-\/-\/-`.
        let replacement = match ch {
            '\u{2013}' => Some("--"),
            '\u{2014}' => Some("---"),
            '\u{2018}' => Some("`"),
            '\u{2019}' => Some("'"),
            '\u{201c}' => Some("``"),
            '\u{201d}' => Some("''"),
            _ => None,
        };
        if let Some(text) = replacement {
            out.push_str(text);
            return false;
        }
        if ch == '\u{2026}' {
            out.push_str("\\ldots");
            return true;
        }
    }
    match ch {
        '-' => {
            out.push('-');
            if next == Some('-') {
                out.push_str("\\/");
            }
            false
        }
        // Space-like characters LaTeX has no glyph for, in either context.
        '\u{202f}' => {
            out.push_str("\\,");
            false
        }
        '\u{ad}' => {
            out.push_str("\\-");
            false
        }
        '\u{200b}' => {
            out.push_str("\\hspace{0pt}");
            false
        }
        '#' | '$' | '%' | '&' | '_' | '{' | '}' => {
            out.push('\\');
            out.push(ch);
            false
        }
        '\\' => {
            out.push_str("\\textbackslash");
            true
        }
        '~' => {
            out.push_str("\\textasciitilde");
            true
        }
        // `\^` is a control *symbol*, not a word: nothing runs into it, so
        // it takes the `{}` unconditionally — an accent with no letter
        // under it is what the group is for.
        '^' => {
            out.push_str("\\^{}");
            false
        }
        // Rendering bugs rather than byte differences: with the default
        // font encoding a bare `<`, `>` and `|` set as `¡`, `¿` and `—`,
        // so text saying `a < b` came out saying something else.
        '<' => {
            out.push_str("\\textless");
            true
        }
        '>' => {
            out.push_str("\\textgreater");
            true
        }
        '|' => {
            out.push_str("\\textbar");
            true
        }
        '\'' => {
            out.push_str("\\textquotesingle");
            true
        }
        // Braced so the bracket cannot be read as the optional argument
        // of whatever command precedes it.
        '[' => {
            out.push_str("{[}");
            false
        }
        ']' => {
            out.push_str("{]}");
            false
        }
        '\u{a0}' => {
            out.push('~');
            false
        }
        ch => {
            out.push(ch);
            false
        }
    }
}

/// A URL inside `\href` or `\includegraphics`.
///
/// Only the characters that would end the argument or start a comment need
/// touching; escaping the rest the way text is escaped would change the
/// address.
fn escape_url(url: &str) -> String {
    let mut out = String::with_capacity(url.len());
    for ch in url.chars() {
        match ch {
            // Not legal in a URL at all: pandoc percent-encodes it, and
            // the `%` that produces is then escaped like any other.
            '|' | ' ' | '<' | '>' | '"' | '^' | '`' => {
                let _ = write!(out, "\\%{:02X}", ch as u32);
            }
            '%' | '#' | '{' | '}' => {
                out.push('\\');
                out.push(ch);
            }
            ch => out.push(ch),
        }
    }
    out
}

/// Inline code, written the way pandoc writes it: `\texttt` with an
/// argument escaped so that it still reads back as the same characters.
///
/// This was `\verb` with a delimiter chosen to avoid the content, which
/// looks like the better answer and is illegal in exactly the place a
/// document puts code: **`\verb` cannot appear inside a command
/// argument.** A heading holding a code span produced
/// `\subsubsection{… \verb|code| …}` and stopped `pdflatex` with
/// "\verb illegal in argument" — `corpus/headings-deep.md` does it, and
/// CI had been failing on it. Captions, `\href` text and `alt=` are the
/// same. Pandoc uses `\texttt` in every position, so following it removes
/// the failure and a divergence together.
///
/// Two characters are escaped here that plain text leaves alone, both
/// probed with `pandoc -f json -t latex`: a space becomes `\ ` so runs of
/// them survive, and a backtick becomes `\textasciigrave{}` so it cannot
/// pair with another into a typographic quote.
/// A highlighted code block, or `None` where there is nothing to
/// highlight it with.
///
/// Pandoc wraps highlighted code in `Shaded` around `Highlighting`, both
/// defined by its own preamble, and colours each token with a
/// `\ClassTok` macro. The condition is the one the HTML writer already
/// uses: the block names a language the highlighter knows.
#[cfg(feature = "highlight")]
fn highlighted(attr: &ferrodoc_ast::Attr, code: &str) -> Option<String> {
    use ferrodoc_html::highlight;
    // **Naming *a* language is the condition, not naming a known one.**
    // ```` ```zzz ```` gets `Shaded` from pandoc with every line one
    // `\NormalTok`, and only a fence with no class at all gets
    // `verbatim`. Asking `known()` here instead sent `text` and every
    // other unknown language down the `verbatim` path.
    if attr.classes.is_empty() {
        return None;
    }
    let language = attr.classes.iter().find(|class| highlight::known(class));
    let mut state = highlight::State::default();
    let mut out = String::from("\\begin{Shaded}\n\\begin{Highlighting}[]\n");
    for source in code.trim_end().split('\n') {
        // A tab is four columns here, which is what pandoc's own reader
        // has already done to a known language and has not to this one.
        let source = source.replace('\t', "    ");
        let pieces = match language {
            Some(language) => highlight::line(&source, highlight::canonical(language), &mut state),
            None => vec![(highlight::Class::Normal, source)],
        };
        out.push_str(&highlight::latex_line(&pieces));
        out.push('\n');
    }
    out.push_str("\\end{Highlighting}\n\\end{Shaded}\n");
    Some(out)
}

/// Without the feature there is no highlighter, so every block is
/// `verbatim` — which is what this writer did for every block until
/// 2026-08-31.
#[cfg(not(feature = "highlight"))]
fn highlighted(_attr: &ferrodoc_ast::Attr, _code: &str) -> Option<String> {
    None
}

fn verbatim(code: &str) -> String {
    let mut out = String::with_capacity(code.len() + 8);
    out.push_str("\\texttt{");
    let mut chars = code.chars().peekable();
    while let Some(ch) = chars.next() {
        match ch {
            ' ' => out.push_str("\\ "),
            '`' => out.push_str("\\textasciigrave{}"),
            // Always `{}` here, never the space [`escape`] would use: a
            // space inside `\texttt` is a space in the output, so pandoc
            // writes `\textless{}p` where running text has
            // `\textless p`. Probed, not assumed.
            ch => {
                if escape_char_in(ch, chars.peek().copied(), false, &mut out) {
                    out.push_str("{}");
                }
            }
        }
    }
    out.push('}');
    out
}

/// A metadata value as plain text for the preamble.
fn meta_text(value: &ferrodoc_ast::MetaValue) -> String {
    use ferrodoc_ast::MetaValue;
    // Preamble text — a title or an author. Nothing here reaches a code
    // block, so the flag below never decides anything.
    let colour = false;
    match value {
        MetaValue::MetaString(text) => escape(text),
        MetaValue::MetaInlines(list) => {
            let mut out = String::new();
            inlines(list, colour, &mut out);
            out
        }
        MetaValue::MetaBlocks(list) => {
            let mut out = String::new();
            blocks(list, 0, colour, &mut out);
            out.trim().to_owned()
        }
        // Several authors are `\and`-separated, which is what `\author`
        // takes.
        MetaValue::MetaList(values) => {
            values.iter().map(meta_text).collect::<Vec<_>>().join(" \\and ")
        }
        MetaValue::MetaBool(flag) => flag.to_string(),
        MetaValue::MetaMap(_) => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ferrodoc_ast::{Attr, Format, Target};

    /// Every `\begin{x}` is closed by a `\end{x}`, in order, and braces
    /// balance.
    ///
    /// This is not a substitute for `pdflatex` — CI runs that — but it is
    /// the failure that makes TeX print forty lines about a runaway
    /// argument, and it can be checked without a TeX installation.
    fn well_formed(latex: &str) -> Result<(), String> {
        let mut environments: Vec<&str> = Vec::new();
        let mut rest = latex;
        // Whichever comes *first* — `or_else` would take a later
        // `\begin` over an earlier `\end` and report every document
        // unbalanced.
        while let Some(at) = match (rest.find("\\begin{"), rest.find("\\end{")) {
            (Some(b), Some(e)) => Some(b.min(e)),
            (found, None) | (None, found) => found,
        } {
            let begins = rest[at..].starts_with("\\begin{");
            let open = at + if begins { 7 } else { 5 };
            let Some(close) = rest[open..].find('}') else {
                return Err("an environment name is unterminated".into());
            };
            let name = &rest[open..open + close];
            if begins {
                environments.push(name);
            } else if environments.pop() != Some(name) {
                return Err(format!("\\end{{{name}}} closes nothing"));
            }
            rest = &rest[open + close..];
        }
        if let Some(open) = environments.pop() {
            return Err(format!("\\begin{{{open}}} is never closed"));
        }
        // Braces, ignoring the escaped ones the writer emits for literal
        // text.
        let mut depth = 0i32;
        let mut chars = latex.chars().peekable();
        while let Some(ch) = chars.next() {
            match ch {
                '\\' => {
                    chars.next();
                }
                '{' => depth += 1,
                '}' => {
                    depth -= 1;
                    if depth < 0 {
                        return Err("a closing brace has no opening one".into());
                    }
                }
                _ => {}
            }
        }
        if depth != 0 {
            return Err(format!("{depth} brace(s) left open"));
        }
        Ok(())
    }

    fn doc(blocks: Vec<Block>) -> Pandoc {
        Pandoc::new(blocks)
    }

    #[test]
    fn every_special_character_survives_as_itself() {
        // Ten characters, and three of them have no backslash form: `\\`
        // is a line break, and `\~`/`\^` are accents waiting for a letter.
        // Escaping those three the obvious way produces LaTeX that either
        // fails to compile or typesets something else.
        let text = r"# $ % & _ { } \ ~ ^ < > | ' [ ]";
        let latex = write_latex(&doc(vec![Block::Para(vec![Inline::Str(text.into())])]));
        well_formed(&latex).expect("balanced");
        for forbidden in ["\\\\ ", "\\~ ", "\\^ "] {
            assert!(!latex.contains(forbidden), "{forbidden:?} in {latex}");
        }
        // Every spelling here is what `pandoc -f json -t latex` writes for
        // the same character; `<`, `>` and `|` are the three that set as a
        // different glyph rather than merely differing in bytes.
        for expected in [
            "\\#", "\\$", "\\%", "\\&", "\\_", "\\{", "\\}",
            "\\textbackslash{}", "\\textasciitilde{}", "\\^{}",
            "\\textless{}", "\\textgreater{}", "\\textbar{}",
            "\\textquotesingle{}", "{[}", "{]}",
        ] {
            assert!(latex.contains(expected), "{expected:?} missing from {latex}");
        }
    }

    #[test]
    fn a_tight_list_says_so_and_a_loose_one_does_not() {
        // No gate can see this: pandoc's LaTeX reader promotes every item
        // to `Para` inside a footnote whatever the writer does, so a
        // round trip cannot distinguish the two spellings. Compared
        // against `pandoc -f json -t latex`, which emits exactly this.
        let item = |b: Block| vec![vec![b]];
        let tight = write_latex(&doc(vec![Block::BulletList(item(Block::Plain(vec![
            Inline::Str("a".into()),
        ])))]));
        assert!(tight.contains("\\begin{itemize}\n\\tightlist\n"), "{tight}");

        let loose = write_latex(&doc(vec![Block::BulletList(item(Block::Para(vec![
            Inline::Str("a".into()),
        ])))]));
        assert!(!loose.contains("\\tightlist"), "{loose}");

        // An empty list is not tight; pandoc emits nothing for one.
        let empty = write_latex(&doc(vec![Block::BulletList(Vec::new())]));
        assert!(!empty.contains("\\tightlist"), "{empty}");

        // The macro the output depends on must be in the preamble, or a
        // standalone document stops compiling.
        let standalone = write_latex_standalone(&doc(Vec::new()));
        assert!(standalone.contains(r"\providecommand{\tightlist}"), "{standalone}");
    }

    #[test]
    fn inline_code_is_texttt_and_legal_inside_an_argument() {
        // No round trip can see this: pandoc's LaTeX reader gives back
        // `Code` for `\verb` and for `\texttt` alike, so the gate that
        // scores this writer is blind to the spelling — and the wrong one
        // does not compile. Each expectation below is the literal output
        // of `pandoc -f json -t latex` on the same `Code` inline.
        let code = |text: &str| {
            write_latex(&doc(vec![Block::Para(vec![Inline::Code(
                Box::default(),
                text.into(),
            )])]))
        };
        for (input, expected) in [
            ("a|b", "\\texttt{a\\textbar{}b}"),
            ("a  b", "\\texttt{a\\ \\ b}"),
            ("x\\y", "\\texttt{x\\textbackslash{}y}"),
            ("a_b", "\\texttt{a\\_b}"),
            ("`t`", "\\texttt{\\textasciigrave{}t\\textasciigrave{}}"),
            ("<p>", "\\texttt{\\textless{}p\\textgreater{}}"),
        ] {
            assert!(code(input).contains(expected), "{expected:?} for {input:?} in {}", code(input));
            well_formed(&code(input)).expect("balanced");
        }
        // The failure this replaced: a code span inside a heading is
        // inside a command argument, where `\verb` is illegal and
        // `pdflatex` stops. `corpus/headings-deep.md` has one.
        let heading = write_latex(&doc(vec![Block::Header(
            3,
            Attr::default(),
            vec![Inline::Code(Box::default(), "code".into())],
        )]));
        assert!(!heading.contains("\\verb"), "{heading}");
        // `\texorpdfstring` because the typeset heading and the PDF
        // bookmark differ once a code span is in it — the bookmark is
        // plain text and `\texttt` in one stops `hyperref`.
        assert!(
            heading.contains("\\subsubsection{\\texorpdfstring{\\texttt{code}}{code}}"),
            "{heading}"
        );
    }

    #[test]
    fn a_list_states_its_numbering_and_its_start() {
        // `\setcounter` must come first: pandoc's reader takes the start
        // from the first directive it meets and stops looking.
        let latex = write_latex(&doc(vec![Block::OrderedList(
            ferrodoc_ast::ListAttributes {
                start: 3,
                style: ListNumberStyle::LowerRoman,
                delim: ListNumberDelim::OneParen,
            },
            vec![vec![Block::Plain(vec![Inline::Str("a".into())])]],
        )]));
        let counter = latex.find("setcounter").expect("a start value");
        let label = latex.find("labelenumi").expect("a numbering style");
        assert!(counter < label, "the counter must be set first:\n{latex}");
        assert!(latex.contains("\\roman{enumi})"), "{latex}");
        well_formed(&latex).expect("balanced");
    }

    #[test]
    fn raw_latex_passes_through_and_other_formats_do_not() {
        let raw = |format: &str| {
            write_latex(&doc(vec![Block::RawBlock(
                Format(format.to_owned()),
                "\\pagebreak".into(),
            )]))
        };
        assert!(raw("latex").contains("\\pagebreak"));
        // HTML in a LaTeX document is markup on the page, not markup.
        assert!(!raw("html").contains("pagebreak"));
    }

    #[test]
    fn the_whole_corpus_of_shapes_stays_well_formed() {
        // One document holding every construct the writer knows, because
        // an unbalanced environment is the failure that makes TeX
        // unreadable and no gate here compiles anything.
        let document = doc(vec![
            Block::Header(1, Attr { identifier: "h".into(), ..Attr::default() }, vec![
                Inline::Str("Heading".into()),
            ]),
            Block::Para(vec![
                Inline::Emph(vec![Inline::Str("a".into())]),
                Inline::Strong(vec![Inline::Str("b".into())]),
                Inline::Strikeout(vec![Inline::Str("c".into())]),
                Inline::Superscript(vec![Inline::Str("d".into())]),
                Inline::SmallCaps(vec![Inline::Str("e".into())]),
                Inline::Quoted(QuoteType::DoubleQuote, vec![Inline::Str("f".into())]),
                Inline::Link(Box::default(), vec![Inline::Str("g".into())], Box::new(Target {
                    url: "http://x/%20#y".into(),
                    title: String::new(),
                })),
                Inline::Image(Box::default(), vec![Inline::Str("alt".into())], Box::new(Target {
                    url: "i.png".into(),
                    title: String::new(),
                })),
                Inline::Note(vec![Block::Para(vec![Inline::Str("note".into())])]),
                Inline::LineBreak,
            ]),
            Block::BlockQuote(vec![Block::Para(vec![Inline::Str("q".into())])]),
            Block::CodeBlock(Attr::default(), "x = 1\ny = 2".into()),
            Block::BulletList(vec![vec![Block::Plain(vec![Inline::Str("i".into())])]]),
            Block::DefinitionList(vec![(
                vec![Inline::Str("t".into())],
                vec![vec![Block::Para(vec![Inline::Str("d".into())])]],
            )]),
            Block::LineBlock(vec![vec![Inline::Str("l1".into())], vec![Inline::Str("l2".into())]]),
            Block::HorizontalRule,
        ]);
        let latex = write_latex(&document);
        well_formed(&latex).unwrap_or_else(|e| panic!("{e}\n\n{latex}"));
        let standalone = write_latex_standalone(&document);
        well_formed(&standalone).unwrap_or_else(|e| panic!("{e}\n\n{standalone}"));
        assert!(standalone.contains("\\begin{document}"));
        assert!(standalone.contains("\\end{document}"));
    }

    #[test]
    fn a_url_keeps_the_characters_that_make_it_a_url() {
        // Escaping a URL the way text is escaped changes the address; only
        // what would end the argument or start a comment is touched.
        let latex = write_latex(&doc(vec![Block::Para(vec![Inline::Link(
            Box::default(),
            vec![Inline::Str("x".into())],
            Box::new(Target { url: "http://a/b?c=1&d~e#f".into(), title: String::new() }),
        )])]));
        assert!(latex.contains("http://a/b?c=1&d~e\\#f"), "{latex}");
        well_formed(&latex).expect("balanced");
    }
}
