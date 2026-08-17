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
//! - **the ten special characters split into two groups.** Seven are
//!   escaped with a backslash (`# $ % & _ { }`), and three cannot be:
//!   `\`, `~` and `^` have no backslash form and need `\textbackslash{}`,
//!   `\textasciitilde{}` and `\textasciicircum{}`. Escaping `\` as `\\`
//!   emits a line break instead of a character;
//! - **verbatim needs a delimiter the content does not contain.**
//!   `\texttt` escapes its argument, which is wrong for code; `\verb`
//!   takes any delimiter, so the writer picks one the text is missing
//!   rather than assuming `|`;
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
    let mut out = String::new();
    blocks(&doc.blocks, &mut out);
    out.trim_end().to_owned() + "\n"
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

const PREAMBLE: &str = concat!(
    "\\documentclass{article}\n",
    "\\usepackage[T1]{fontenc}\n",
    "\\usepackage[utf8]{inputenc}\n",
    "\\usepackage{graphicx}\n",
    "\\usepackage{longtable,booktabs}\n",
    // `\sout` is `ulem`'s, and `ulem` is not in a base TeX. The name is
    // kept because it is the one a fragment pasted into someone else's
    // document will already have; `\providecommand` yields to theirs.
    // Box the argument, overlay a rule its width, then print it.
    "\\providecommand{\\sout}[1]{{\\leavevmode\\setbox0=\\hbox{#1}%\n",
    "  \\rlap{\\rule[0.5ex]{\\wd0}{0.4pt}}\\box0}}\n",
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
fn blocks(list: &[Block], out: &mut String) {
    let mut first = true;
    for block in list {
        if !first {
            out.push('\n');
        }
        first = false;
        let mut text = String::new();
        block_to(block, &mut text);
        out.push_str(text.trim_end_matches('\n'));
        out.push('\n');
    }
}

fn block_to(block: &Block, out: &mut String) {
    match block {
        Block::Plain(list) | Block::Para(list) => {
            inlines(list, out);
            out.push('\n');
        }
        Block::LineBlock(lines) => {
            // `\\` between lines and a blank line after, which is what a
            // verse block is in LaTeX without loading a package for it.
            let rendered: Vec<String> = lines
                .iter()
                .map(|line| {
                    let mut text = String::new();
                    inlines(line, &mut text);
                    text
                })
                .collect();
            let _ = writeln!(out, "{}\n", rendered.join("\\\\\n"));
        }
        Block::CodeBlock(_, code) => {
            // `verbatim` rather than `lstlisting`: it needs no package, so
            // the output compiles on a minimal TeX installation.
            let _ = writeln!(out, "\\begin{{verbatim}}\n{}\n\\end{{verbatim}}", code.trim_end());
        }
        Block::BlockQuote(inner) => {
            out.push_str("\\begin{quote}\n");
            blocks(inner, out);
            out.push_str("\\end{quote}\n");
        }
        Block::OrderedList(attrs, items) => {
            out.push_str("\\begin{enumerate}\n");
            // `\setcounter` **before** `\def\labelenumi`, and the order
            // is not cosmetic: pandoc's reader takes the start value from
            // the first directive it meets and stops looking, so a list
            // that says `\def` first begins at 1 whatever it asked for.
            // Measured both ways round.
            if attrs.start != 1 {
                let _ = writeln!(out, "\\setcounter{{enumi}}{{{}}}", attrs.start - 1);
            }
            let _ = writeln!(
                out,
                "\\def\\labelenumi{{{}}}",
                enumerate_style(attrs.style, attrs.delim)
            );
            for item in items {
                out.push_str("\\item\n");
                blocks(item, out);
            }
            out.push_str("\\end{enumerate}\n");
        }
        Block::BulletList(items) => {
            out.push_str("\\begin{itemize}\n");
            for item in items {
                out.push_str("\\item\n");
                blocks(item, out);
            }
            out.push_str("\\end{itemize}\n");
        }
        Block::DefinitionList(entries) => {
            out.push_str("\\begin{description}\n");
            for (term, definitions) in entries {
                out.push_str("\\item[");
                inlines(term, out);
                out.push_str("]\n");
                for definition in definitions {
                    blocks(definition, out);
                }
            }
            out.push_str("\\end{description}\n");
        }
        Block::Header(level, attr, list) => {
            let index = usize::try_from(*level).unwrap_or(1).saturating_sub(1);
            let macro_name = SECTIONS.get(index).unwrap_or(&"subparagraph");
            let _ = write!(out, "\\{macro_name}{{");
            inlines(list, out);
            out.push('}');
            // The label is what makes a cross-reference resolve. Without
            // one the heading still reads back with the right identifier,
            // because pandoc derives it from the text — but nothing in the
            // document can point at it.
            if !attr.identifier.is_empty() {
                let _ = write!(out, "\\label{{{}}}", escape(&attr.identifier));
            }
            out.push('\n');
        }
        Block::HorizontalRule => out.push_str("\\begin{center}\\rule{0.5\\linewidth}{0.5pt}\\end{center}\n"),
        Block::Table(table) => table_to(table, out),
        Block::Figure(_, caption, inner) => {
            out.push_str("\\begin{figure}\n\\centering\n");
            blocks(inner, out);
            if !caption.blocks.is_empty() {
                out.push_str("\\caption{");
                caption_text(caption, out);
                out.push_str("}\n");
            }
            out.push_str("\\end{figure}\n");
        }
        // A div carries no LaTeX of its own; its content is the content.
        Block::Div(_, inner) => blocks(inner, out),
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

/// The `\labelenumi` a numbering style and delimiter call for.
///
/// Always emitted, even for the arabic default: pandoc's reader reports
/// `DefaultStyle` for a bare `enumerate`, so a `Decimal` list that says
/// nothing comes back as a different list.
fn enumerate_style(style: ListNumberStyle, delim: ListNumberDelim) -> String {
    let counter = match style {
        ListNumberStyle::LowerAlpha => "\\alph{enumi}",
        ListNumberStyle::UpperAlpha => "\\Alph{enumi}",
        ListNumberStyle::LowerRoman => "\\roman{enumi}",
        ListNumberStyle::UpperRoman => "\\Roman{enumi}",
        _ => "\\arabic{enumi}",
    };
    match delim {
        ListNumberDelim::TwoParens => format!("({counter})"),
        ListNumberDelim::OneParen => format!("{counter})"),
        // Period, and the default, which pandoc also renders as a period.
        ListNumberDelim::Period | ListNumberDelim::DefaultDelim => format!("{counter}."),
    }
}

fn table_to(table: &Table, out: &mut String) {
    let columns = table.colspecs.len().max(1);
    let spec = "l".repeat(columns);
    // `longtable` rather than `tabular`: a table longer than a page is
    // ordinary in a converted document, and `tabular` silently runs off
    // the bottom of it.
    let _ = writeln!(out, "\\begin{{longtable}}[]{{@{{}}{spec}@{{}}}}");
    out.push_str("\\toprule\n");
    for row in &table.head.rows {
        row_to(row, columns, out);
    }
    if !table.head.rows.is_empty() {
        out.push_str("\\midrule\n\\endhead\n");
    }
    for body in &table.bodies {
        for row in body.head.iter().chain(&body.body) {
            row_to(row, columns, out);
        }
    }
    for row in &table.foot.rows {
        row_to(row, columns, out);
    }
    out.push_str("\\bottomrule\n");
    if !table.caption.blocks.is_empty() {
        out.push_str("\\caption{");
        caption_text(&table.caption, out);
        out.push_str("}\\tabularnewline\n");
    }
    out.push_str("\\end{longtable}\n");
}

fn row_to(row: &Row, columns: usize, out: &mut String) {
    let mut cells: Vec<String> = row.cells.iter().map(cell_text).collect();
    cells.resize(columns, String::new());
    let _ = writeln!(out, "{} \\tabularnewline", cells.join(" & "));
}

fn cell_text(cell: &Cell) -> String {
    let mut out = String::new();
    for block in &cell.blocks {
        match block {
            Block::Plain(list) | Block::Para(list) => inlines(list, &mut out),
            other => block_to(other, &mut out),
        }
    }
    // A cell is one line: a newline inside `&`-separated content ends the
    // row early and the table stops making sense.
    out.replace('\n', " ").trim().to_owned()
}

fn caption_text(caption: &Caption, out: &mut String) {
    for block in &caption.blocks {
        match block {
            Block::Plain(list) | Block::Para(list) => inlines(list, out),
            other => block_to(other, out),
        }
    }
}

fn inlines(list: &[Inline], out: &mut String) {
    for inline in list {
        inline_to(inline, out);
    }
}

fn inline_to(inline: &Inline, out: &mut String) {
    let wrap = |name: &str, inner: &[Inline], out: &mut String| {
        let _ = write!(out, "\\{name}{{");
        inlines(inner, out);
        out.push('}');
    };
    match inline {
        Inline::Str(text) => out.push_str(&escape(text)),
        Inline::Space => out.push(' '),
        Inline::SoftBreak => out.push('\n'),
        Inline::LineBreak => out.push_str("\\\\\n"),
        Inline::Emph(inner) => wrap("emph", inner, out),
        Inline::Strong(inner) => wrap("textbf", inner, out),
        Inline::Underline(inner) => wrap("underline", inner, out),
        // `ulem`'s name, but the preamble defines it: LaTeX has no
        // strikeout of its own and `ulem` is not in a base TeX.
        Inline::Strikeout(inner) => wrap("sout", inner, out),
        Inline::Superscript(inner) => wrap("textsuperscript", inner, out),
        Inline::Subscript(inner) => wrap("textsubscript", inner, out),
        Inline::SmallCaps(inner) => wrap("textsc", inner, out),
        Inline::Quoted(kind, inner) => {
            let (open, close) = match kind {
                QuoteType::SingleQuote => ("`", "'"),
                QuoteType::DoubleQuote => ("``", "''"),
            };
            out.push_str(open);
            inlines(inner, out);
            out.push_str(close);
        }
        // A citation and a span are both their content here: LaTeX has
        // no form for either that survives pandoc's reader.
        Inline::Cite(_, inner) | Inline::Span(_, inner) => inlines(inner, out),
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
        Inline::Link(_, inner, target) => {
            // A URL is not escaped the way text is — `\href`'s first
            // argument is verbatim-ish, and escaping `~` or `%` there
            // breaks the link rather than the typesetting.
            let _ = write!(out, "\\href{{{}}}{{", escape_url(&target.url));
            inlines(inner, out);
            out.push('}');
        }
        Inline::Image(_, alt, target) => {
            // The alt text goes in the `alt=` option: it is the only place
            // `\includegraphics` has for it, and without it the words are
            // simply gone.
            let mut text = String::new();
            inlines(alt, &mut text);
            if text.is_empty() {
                let _ = write!(out, "\\includegraphics{{{}}}", escape_url(&target.url));
            } else {
                let _ = write!(
                    out,
                    "\\includegraphics[alt={{{text}}}]{{{}}}",
                    escape_url(&target.url)
                );
            }
        }
        Inline::Note(blocks_in_note) => {
            out.push_str("\\footnote{");
            let mut text = String::new();
            blocks(blocks_in_note, &mut text);
            out.push_str(text.trim());
            out.push('}');
        }
    }
}

/// Escape the characters LaTeX gives a meaning to.
///
/// Ten of them, in two groups: seven take a backslash, and three have no
/// backslash form at all. `\\` is a line break, not a backslash, and
/// `\~`/`\^` are accents waiting for a letter — so those three need their
/// `\text…` commands, each followed by `{}` so a following space survives.
fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        match ch {
            '#' | '$' | '%' | '&' | '_' | '{' | '}' => {
                out.push('\\');
                out.push(ch);
            }
            '\\' => out.push_str("\\textbackslash{}"),
            '~' => out.push_str("\\textasciitilde{}"),
            '^' => out.push_str("\\textasciicircum{}"),
            ch => out.push(ch),
        }
    }
    out
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
            '%' | '#' | '{' | '}' => {
                out.push('\\');
                out.push(ch);
            }
            ch => out.push(ch),
        }
    }
    out
}

/// Inline code, as `\verb` with a delimiter the code does not contain.
///
/// `\texttt` would escape its argument, which is the one thing code must
/// not have done to it. `\verb` takes *any* non-letter delimiter, so the
/// writer picks one that is absent rather than assuming `|` and emitting
/// something that does not compile the moment a pipe appears.
fn verbatim(code: &str) -> String {
    // A newline cannot appear inside `\verb` at all; a code span holding
    // one is written as escaped text instead, which typesets correctly
    // even though it is no longer verbatim.
    if code.contains('\n') {
        return format!("\\texttt{{{}}}", escape(code));
    }
    for delimiter in ['|', '!', '+', '@', '^', '*', '?', '=', '~'] {
        if !code.contains(delimiter) {
            return format!("\\verb{delimiter}{code}{delimiter}");
        }
    }
    format!("\\texttt{{{}}}", escape(code))
}

/// A metadata value as plain text for the preamble.
fn meta_text(value: &ferrodoc_ast::MetaValue) -> String {
    use ferrodoc_ast::MetaValue;
    match value {
        MetaValue::MetaString(text) => escape(text),
        MetaValue::MetaInlines(list) => {
            let mut out = String::new();
            inlines(list, &mut out);
            out
        }
        MetaValue::MetaBlocks(list) => {
            let mut out = String::new();
            blocks(list, &mut out);
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
        let text = r"# $ % & _ { } \ ~ ^";
        let latex = write_latex(&doc(vec![Block::Para(vec![Inline::Str(text.into())])]));
        well_formed(&latex).expect("balanced");
        for forbidden in ["\\\\ ", "\\~ ", "\\^ "] {
            assert!(!latex.contains(forbidden), "{forbidden:?} in {latex}");
        }
        assert!(latex.contains("\\textbackslash{}"), "{latex}");
        assert!(latex.contains("\\textasciitilde{}"), "{latex}");
        assert!(latex.contains("\\textasciicircum{}"), "{latex}");
    }

    #[test]
    fn inline_code_picks_a_delimiter_the_code_does_not_contain() {
        // `\verb|...|` is the usual spelling and it stops compiling the
        // moment the code contains a pipe.
        let code = |text: &str| {
            write_latex(&doc(vec![Block::Para(vec![Inline::Code(
                Box::default(),
                text.into(),
            )])]))
        };
        assert!(code("a|b").contains("\\verb!a|b!"), "{}", code("a|b"));
        assert!(code("a!b").contains("\\verb|a!b|"), "{}", code("a!b"));
        // Code containing every candidate falls back to escaped text,
        // which typesets rather than failing to compile.
        let awkward = "|!+@^*?=~";
        assert!(code(awkward).contains("\\texttt"), "{}", code(awkward));
        well_formed(&code(awkward)).expect("balanced");
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
