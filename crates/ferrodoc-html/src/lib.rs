//! HTML reader and writer for the ferrodoc (pandoc-compatible) AST.
//!
//! [`read_html`] parses a page into the AST; [`write_html`] renders the AST
//! back out. The reader is structural — it is not a CSS engine — and is
//! verified against `pandoc -f html -t json` by `ferrodoc-harness
//! diff-html-read`.
//!
//! [`write_html`] emits the same HTML as
//! `pandoc -f commonmark -t html --syntax-highlighting=none --wrap=none`
//! for every construct reachable from the commonmark reader (verified
//! differentially by `ferrodoc-harness diff-html`). Constructs the
//! commonmark reader cannot produce get reasonable pandoc-shaped output
//! and are checked by `./scripts/writers.sh`, which runs the same
//! comparison over `corpus/gfm/*.gfm` — tables, task lists and
//! footnotes, none of which `CommonMark` can express. Raw content in
//! another format is dropped, which is what pandoc's HTML writer does
//! with raw content it cannot place.

mod page;
mod read;
mod template;

pub use page::{Page, write_page};

#[cfg(feature = "highlight")]
/// The syntax highlighter, exposed for the **LaTeX** writer.
///
/// Not part of the supported surface — `#[doc(hidden)]`, like
/// `ferrodoc-docx`'s `xml` and `media`, which `ferrodoc-odt` and
/// `ferrodoc-epub` are built on for the same reason. Pandoc highlights
/// LaTeX with the same token classes it uses for HTML, so a second
/// highlighter would be the same 4,000 lines with different names on the
/// output.
#[doc(hidden)]
pub mod highlight;
pub use read::{MAX_NESTING, read_html, read_html_without_generated_identifiers};

/// What can go wrong reading HTML.
#[derive(Debug, PartialEq, Eq)]
pub enum Error {
    /// The element tree nests deeper than [`MAX_NESTING`].
    TooDeep,
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TooDeep => write!(f, "html nests deeper than {MAX_NESTING} levels"),
        }
    }
}

impl std::error::Error for Error {}

use ferrodoc_ast::{
    Alignment, Attr, Block, Caption, Cell, ColSpec, Inline, ListNumberStyle, Pandoc, Row, Table,
};
use std::fmt::Write as _;

/// Marks a place a line may be broken. Chosen because no reader here can
/// produce one inside text: `CommonMark` replaces NUL with U+FFFD by
/// specification, and XML — which DOCX, ODT and EPUB are — forbids both
/// outright. Every one is turned into a space, a newline, or a line break
/// before the string leaves this crate.
const BREAK: char = '\u{0}';
/// The same, for a `SoftBreak` — which `--wrap=preserve` keeps as a
/// newline where an ordinary space stays a space.
const SOFT: char = '\u{1}';
/// Ends the text a break decision is allowed to look at, without being a
/// break itself. What follows is appended whatever its width — an
/// element's content, where the marks before it belong to its tag.
const STOP: char = '\u{2}';

/// How the writer lays lines out, as pandoc's `--wrap` means it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Wrap {
    /// Every soft break becomes a space and no line is broken.
    #[default]
    None,
    /// A soft break stays a line break; nothing else is broken.
    Preserve,
    /// Fill to this many columns, breaking at spaces, soft breaks **and
    /// between attributes** — which is where pandoc breaks a long tag.
    Fill(usize),
}

/// Who built the section divs in the tree. Pandoc's `--section-divs` is
/// a *writer* decision, and the writer treats a `Div` classed `section`
/// differently depending on which side of it made the div — measured on
/// `-f json -t html5` with and without the flag, on the same input.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Sections {
    /// They arrived with the document, so the header inside is the
    /// section's own and its attributes belong on the section too. This
    /// is plain `-t html5`, and what the CLI always writes.
    Given,
    /// The caller made them from the headers and already put on the
    /// classes it wanted; write them as they stand. This is
    /// `--section-divs`, and what the EPUB writer needs.
    Made,
}

/// Whether the writer colours code, as pandoc's `--syntax-highlighting`
/// asks it to. A gate that mutes pandoc's highlighting must mute this
/// one too, or it compares two different questions.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Highlighting {
    /// Colour every language the highlighter knows.
    #[default]
    Default,
    /// Colour nothing: `<pre class="…"><code>`, as before there was a
    /// highlighter at all.
    None,
}

/// What the block writers carry between them: the section mode, and the
/// running code-block number.
struct Ctx {
    sections: Sections,
    #[cfg_attr(
        not(feature = "highlight"),
        expect(dead_code, reason = "only the highlighter reads it, and it is out of this build")
    )]
    highlighting: Highlighting,
    /// `cb1`, `cb2`, … Pandoc numbers **every** code block, highlighted
    /// or not, and shows the number only where it highlights — so the
    /// counter cannot live in the highlighting path.
    blocks: std::cell::Cell<usize>,
}

impl Ctx {
    fn new(mode: Sections, highlighting: Highlighting) -> Ctx {
        Ctx { sections: mode, highlighting, blocks: std::cell::Cell::new(0) }
    }

    /// The next code block's number.
    fn next_block(&self) -> usize {
        self.blocks.set(self.blocks.get() + 1);
        self.blocks.get()
    }
}

/// Render a document as HTML, matching pandoc's HTML writer with
/// `--wrap=none` and no syntax highlighting.
pub fn write_html(doc: &Pandoc) -> String {
    write_html_with_id_prefix(doc, "")
}

/// The same, for a caller that built the section divs itself — pandoc's
/// `--section-divs`. The EPUB writer is the one that does; see
/// [`Sections`] for what changes.
#[must_use]
pub fn write_html_section_divs(doc: &Pandoc) -> String {
    // **Highlighted, since 2026-08-27.** This was `Highlighting::None`
    // while there was no highlighter: pandoc's EPUB writer colours code,
    // and scoring against skylighting would have measured skylighting.
    // That reasoning expired the moment this project had a highlighter of
    // its own that matches skylighting byte for byte — muting a feature
    // you *have* is the trap this repository documents, and the EPUB
    // writer was the last place still doing it.
    write_body(doc, "", Wrap::None, Sections::Made, Highlighting::Default)
}

/// The same, with `--id-prefix` on the identifiers this writer *invents*.
///
/// Every identifier in the tree is prefixed before it reaches here — the
/// CLI does that — but a footnote's `fn1`/`fnref1` and the
/// `<section id="footnotes">` are made up while writing, so they are the
/// one set the tree cannot carry. Without this, two documents on one page
/// collide on `#fn1`, which is the exact failure `--id-prefix` exists to
/// prevent.
pub fn write_html_with_id_prefix(doc: &Pandoc, id_prefix: &str) -> String {
    write_html_wrapped(doc, id_prefix, Wrap::None)
}

/// The same, laid out the way `--wrap` asks for.
///
/// The writer always marks its break opportunities and this decides what
/// becomes of them, so nothing downstream has to be told which mode it is
/// in. `Wrap::None` is what [`write_html`] does.
#[must_use]
pub fn write_html_wrapped(doc: &Pandoc, id_prefix: &str, wrap: Wrap) -> String {
    write_body(doc, id_prefix, wrap, Sections::Given, Highlighting::Default)
}

/// The same, with highlighting off — what `--syntax-highlighting=none`
/// asks for, and what a gate comparing against a muted pandoc needs.
#[must_use]
pub fn write_html_unhighlighted(doc: &Pandoc, id_prefix: &str, wrap: Wrap) -> String {
    write_body(doc, id_prefix, wrap, Sections::Given, Highlighting::None)
}

fn write_body(
    doc: &Pandoc,
    id_prefix: &str,
    wrap: Wrap,
    sections: Sections,
    highlighting: Highlighting,
) -> String {
    let ctx = &Ctx::new(sections, highlighting);
    let mut out = String::new();
    let (blocks, notes) = take_notes(&doc.blocks, id_prefix);
    write_blocks(&mut out, &blocks, ctx);
    if out.is_empty() {
        out.push('\n'); // pandoc's output always ends with a newline
    }
    // A document-final raw block would otherwise leave a blank last line;
    // pandoc ends the document with exactly one newline.
    if out.ends_with("\n\n") {
        out.pop();
    }
    write_notes(&mut out, &notes, id_prefix, ctx);
    lay_out(&out, wrap)
}

/// Turn the break marks into whatever the mode asks for.
fn lay_out(text: &str, wrap: Wrap) -> String {
    match wrap {
        // **One pass and one allocation.** Chaining `replace` copied the
        // whole rendered output two or three times, which is the cost
        // that matters when the *output* is the large object.
        Wrap::None => resolved(text, " "),
        Wrap::Preserve => resolved(text, "\n"),
        Wrap::Fill(columns) => {
            let mut out = String::with_capacity(text.len());
            for line in text.split('\n') {
                fill(line, columns, &mut out);
                out.push('\n');
            }
            // `split` on a trailing newline yields a final empty piece,
            // which has just added a newline of its own.
            out.pop();
            out
        }
    }
}

/// Resolve the layout markers in one pass: a `BREAK` becomes a space, a
/// `SOFT` becomes `soft`, and a `STOP` is dropped. Copying runs between
/// markers rather than characters, so the common case is a memcpy.
fn resolved(text: &str, soft: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(index) = rest.find([BREAK, SOFT, STOP]) {
        out.push_str(&rest[..index]);
        let marker = rest[index..].chars().next().unwrap_or(STOP);
        match marker {
            BREAK => out.push(' '),
            SOFT => out.push_str(soft),
            _ => {}
        }
        rest = &rest[index + marker.len_utf8()..];
    }
    out.push_str(rest);
    out
}

/// The columns a string occupies, which is **not** its character count:
/// a CJK ideograph or an emoji takes two and a few invisible characters
/// take none. Pandoc counts the same way, so a document of Japanese
/// filled at 72 wraps at the same words.
///
/// The table below was **measured**, not transcribed: every codepoint in
/// the blocks that could plausibly be wide was put through
/// `pandoc -t html --wrap=auto --columns=13` alone in a paragraph, and
/// the ones that pushed the following word onto a second line are these.
/// Outside the probed blocks — planes 4 and above — this says one, which
/// is what pandoc says everywhere except a stretch of unassigned space.
fn display_width(text: &str) -> usize {
    text.chars().map(char_width).sum()
}

fn char_width(ch: char) -> usize {
    match ch {
        // Zero: the marks that occupy no column at all, and the break
        // marks, which are not in the output.
        '\u{200b}' | '\u{200d}' | '\u{fe0f}' | BREAK | SOFT | STOP => 0,
        ch if WIDE.iter().any(|range| range.contains(&(ch as u32))) => 2,
        _ => 1,
    }
}

/// The codepoints pandoc counts as two columns. See [`display_width`].
static WIDE: &[std::ops::RangeInclusive<u32>] = &[
    0x1100..=0x115F, 0x231A..=0x231B, 0x2329..=0x232A, 0x23E9..=0x23EC, 0x23F0..=0x23F0,
    0x23F3..=0x23F3, 0x25FD..=0x25FE, 0x2614..=0x2615, 0x2648..=0x2653, 0x267F..=0x267F,
    0x2693..=0x2693, 0x26A1..=0x26A1, 0x26AA..=0x26AB, 0x26BD..=0x26BE, 0x26C4..=0x26C5,
    0x26CE..=0x26CE, 0x26D4..=0x26D4, 0x26EA..=0x26EA, 0x26F2..=0x26F3, 0x26F5..=0x26F5,
    0x26FA..=0x26FA, 0x26FD..=0x26FD, 0x2705..=0x2705, 0x270A..=0x270B, 0x2728..=0x2728,
    0x274C..=0x274C, 0x274E..=0x274E, 0x2753..=0x2755, 0x2757..=0x2757, 0x2795..=0x2797,
    0x27B0..=0x27B0, 0x27BF..=0x27BF, 0x2E80..=0x3029, 0x302E..=0x303E, 0x3040..=0x3096,
    0x309B..=0x3247, 0x3250..=0x4DBF, 0x4E00..=0xA4CF, 0xA960..=0xA97F, 0xAC00..=0xD7AF,
    0xF900..=0xFAFF, 0xFE10..=0xFE1F, 0xFE30..=0xFE6F, 0xFF01..=0xFF60, 0xFFE0..=0xFFE7,
    0x1F004..=0x1F004, 0x1F0CF..=0x1F0D0, 0x1F18E..=0x1F18E, 0x1F191..=0x1F19A,
    0x1F200..=0x1F320, 0x1F32D..=0x1F335, 0x1F337..=0x1F37C, 0x1F37E..=0x1F393,
    0x1F3A0..=0x1F3CA, 0x1F3CF..=0x1F3D3, 0x1F3E0..=0x1F3F0, 0x1F3F4..=0x1F3F4,
    0x1F3F8..=0x1F43E, 0x1F440..=0x1F440, 0x1F442..=0x1F4FC, 0x1F4FF..=0x1F53D,
    0x1F54B..=0x1F54E, 0x1F550..=0x1F567, 0x1F57A..=0x1F57A, 0x1F595..=0x1F596,
    0x1F5A4..=0x1F5A4, 0x1F5FB..=0x1F64F, 0x1F680..=0x1F6C5, 0x1F6CC..=0x1F6CC,
    0x1F6D0..=0x1F6D2, 0x1F6D5..=0x1F6DF, 0x1F6EB..=0x1F6EF, 0x1F6F4..=0x1F6FF,
    0x1F7E0..=0x1F7FF, 0x1F90C..=0x1F93A, 0x1F93C..=0x1F945, 0x1F947..=0x1F9FF,
    0x1FA70..=0x1FAFF, 0x20000..=0x3FFFD
];

/// Greedy fill: take words while they fit, break at the last mark that
/// did. A word longer than the width goes on its own line and overruns —
/// breaking inside it would invent a break the text does not have.
///
/// The decision measures only as far as the first [`STOP`] in the word,
/// which is where an element's tag ends and its content begins; the whole
/// word is appended either way and the column counts all of it.
fn fill(line: &str, columns: usize, out: &mut String) {
    let mut width = 0;
    let mut first = true;
    for word in line.split([BREAK, SOFT]) {
        let measured = display_width(word.split(STOP).next().unwrap_or(word));
        let whole = display_width(word);
        if first {
            first = false;
            width = whole;
        } else if width + 1 + measured <= columns {
            out.push(' ');
            width += 1 + whole;
        } else {
            out.push('\n');
            width = whole;
        }
        out.extend(word.chars().filter(|c| *c != STOP));
    }
}

/// How deep the contents go, matching **pandoc's `--toc-depth` default**
/// rather than anything inherent to the format: `pandoc --toc-depth=4`
/// disagrees with this, which is how a reader can check that claim.
const TOC_DEPTH: i64 = 3;

/// Number the headings, as pandoc's `--number-sections` does.
///
/// Probed against pandoc 3.8.2.1, with `--wrap=none` so no rule here comes
/// from a line pandoc happened to break:
///
/// - the header gains a `number` attribute, which this writer emits as
///   `data-number` **before** the `id`, and a leading
///   `<span class="header-section-number">` followed by a space;
/// - a header whose classes contain `unnumbered` is skipped and keeps the
///   class — pandoc emits neither the attribute nor the span for it;
/// - a number has one component per level **from the document's shallowest
///   heading down**, which is not the same as one per absolute level: a
///   document whose headings are all `##` numbers `1`, `2`, while one that
///   mixes `##` with a `#` numbers the `##` as `0.1`. An `unnumbered`
///   heading still counts toward that shallowest level, and a heading
///   inside a `Div` does too;
/// - headings inside a `Div` are numbered; headings inside a `BlockQuote`
///   are **not**, and neither reach the contents.
pub fn number_sections(doc: &mut Pandoc) {
    let Some(base) = shallowest_header(&doc.blocks) else {
        return;
    };
    let mut counters = [0usize; 6];
    number_blocks(&mut doc.blocks, &mut counters, base);
}

/// The shallowest heading level anywhere numbering reaches, which is where
/// a section number's first component comes from.
fn shallowest_header(blocks: &[Block]) -> Option<usize> {
    let mut shallowest: Option<usize> = None;
    for block in blocks {
        let depth = match block {
            Block::Header(level, _, _) => usize::try_from(*level).unwrap_or(1).clamp(1, 6),
            Block::Div(_, blocks) => match shallowest_header(blocks) {
                Some(depth) => depth,
                None => continue,
            },
            _ => continue,
        };
        shallowest = Some(shallowest.map_or(depth, |current: usize| current.min(depth)));
    }
    shallowest
}

fn number_blocks(blocks: &mut [Block], counters: &mut [usize; 6], base: usize) {
    for block in blocks {
        match block {
            Block::Header(level, attr, inlines) => {
                if attr.classes.iter().any(|class| class == "unnumbered") {
                    continue;
                }
                let depth = usize::try_from(*level).unwrap_or(1).clamp(1, 6);
                counters[depth - 1] += 1;
                for counter in &mut counters[depth..] {
                    *counter = 0;
                }
                let number = counters[base - 1..depth.max(base)]
                    .iter()
                    .map(usize::to_string)
                    .collect::<Vec<_>>()
                    .join(".");
                attr.attributes.push(("number".to_owned(), number.clone()));
                // Built in one pass rather than two `insert(0, …)` calls,
                // which is the quadratic shape this repo keeps re-finding.
                let mut numbered = Vec::with_capacity(inlines.len() + 2);
                numbered.push(Inline::Span(
                    Box::new(Attr {
                        identifier: String::new(),
                        classes: vec!["header-section-number".to_owned()],
                        attributes: Vec::new(),
                    }),
                    vec![Inline::Str(number)],
                ));
                numbered.push(Inline::Space);
                numbered.append(inlines);
                *inlines = numbered;
            }
            Block::Div(_, blocks) => number_blocks(blocks, counters, base),
            _ => {}
        }
    }
}

/// The `<nav id="TOC" role="doc-toc">` block pandoc's `--toc` writes.
///
/// Empty when the document has no heading within [`TOC_DEPTH`]: pandoc
/// writes no element at all rather than an empty one.
///
/// Two rules are easy to get wrong and both are probed. **Nesting is
/// relative, not absolute**: `#`, then `###`, then `##` puts the last two
/// as siblings one level in, because a jump opens exactly one list. And a
/// heading with **no identifier gets no link** — its text goes into the
/// item bare, which is what `-f commonmark` produces, where headings carry
/// no identifiers at all.
pub fn write_toc(doc: &Pandoc) -> String {
    write_toc_to_depth(doc, TOC_DEPTH)
}

/// The same, to a chosen depth — pandoc's `--toc-depth`.
pub fn write_toc_to_depth(doc: &Pandoc, depth: i64) -> String {
    write_toc_wrapped(doc, depth, Wrap::None)
}

/// The same, laid out the way `--wrap` asks for. The contents fill like
/// any other text: pandoc's `-s --wrap=auto` wraps a long entry.
#[must_use]
pub fn write_toc_wrapped(doc: &Pandoc, depth: i64, wrap: Wrap) -> String {
    lay_out(&toc_marked(doc, depth), wrap)
}

/// The contents with the break marks still in them.
fn toc_marked(doc: &Pandoc, depth: i64) -> String {
    let mut entries = Vec::new();
    collect_toc(&doc.blocks, depth, &mut entries);
    if entries.is_empty() {
        return String::new();
    }
    let mut roots: Vec<TocNode> = Vec::new();
    // Indices into the tree, one per open level, with the level beside it.
    let mut path: Vec<(i64, usize)> = Vec::new();
    for (level, html) in entries {
        while path.last().is_some_and(|(open, _)| *open >= level) {
            path.pop();
        }
        let node = TocNode { html, children: Vec::new() };
        let mut siblings = &mut roots;
        for (_, index) in &path {
            siblings = &mut siblings[*index].children;
        }
        siblings.push(node);
        path.push((level, siblings.len() - 1));
    }
    format!("<nav id=\"TOC\" role=\"doc-toc\">\n{}\n</nav>\n", write_toc_list(&roots))
}

/// The contents **without** its `<nav>`, which is what the page template
/// wraps for itself. Emitting the wrapper here as well is how `-s --toc`
/// produced two of them.
pub fn toc_list_to_depth(doc: &Pandoc, depth: i64, id_prefix: &str) -> String {
    toc_list_wrapped(doc, depth, id_prefix, Wrap::None)
}

/// The same, laid out the way `--wrap` asks for.
#[must_use]
pub fn toc_list_wrapped(doc: &Pandoc, depth: i64, id_prefix: &str, wrap: Wrap) -> String {
    let nav = toc_marked(doc, depth);
    // The entry ids carry the prefix **before** the `toc-`, which is
    // pandoc's spelling: `id="p-toc-x"` beside `href="#p-x"`. The tree's
    // identifiers already carry it by the time this runs, so the prefix
    // is *moved* rather than added — adding gave `p-toc-p-x`.
    let nav = if id_prefix.is_empty() {
        nav
    } else {
        nav.replace(
            &format!("\"{BREAK}id=\"toc-{id_prefix}"),
            &format!("\"{BREAK}id=\"{id_prefix}toc-"),
        )
    };
    let nav = nav
        .strip_prefix("<nav id=\"TOC\" role=\"doc-toc\">\n")
        .and_then(|rest| rest.strip_suffix("\n</nav>\n"))
        .map_or(nav.clone(), str::to_owned);
    lay_out(&nav, wrap)
}

struct TocNode {
    html: String,
    children: Vec<TocNode>,
}

/// `<ul>` … `</ul>`, with no trailing newline: a nested list closes as
/// `</ul></li>` on one line, which is pandoc's shape.
fn write_toc_list(nodes: &[TocNode]) -> String {
    let mut out = String::from("<ul>\n");
    for node in nodes {
        out.push_str("<li>");
        out.push_str(&node.html);
        if !node.children.is_empty() {
            out.push('\n');
            out.push_str(&write_toc_list(&node.children));
        }
        out.push_str("</li>\n");
    }
    out.push_str("</ul>");
    out
}

/// Every heading within [`TOC_DEPTH`], as `(level, rendered entry)`.
fn collect_toc(blocks: &[Block], depth: i64, out: &mut Vec<(i64, String)>) {
    for block in blocks {
        match block {
            Block::Header(level, attr, inlines) => {
                if *level > depth {
                    continue;
                }
                let mut html = String::new();
                if !attr.identifier.is_empty() {
                    // **The attribute gaps are break marks**, as they are
                    // in a footnote reference: pandoc fills the contents
                    // like any other text and breaks a long `<a>` between
                    // its attributes. Writing a literal space here left
                    // every `--toc` page with over-long lines.
                    let _ = write!(html, "<a{BREAK}href=\"#");
                    escape_attribute(&mut html, &attr.identifier);
                    let _ = write!(html, "\"{BREAK}id=\"toc-");
                    escape_attribute(&mut html, &attr.identifier);
                    html.push_str("\">");
                }
                // A numbered document has already had its section number
                // put into the heading as a `header-section-number` span;
                // the contents carry the same number under a different
                // class, so that span is replaced rather than repeated.
                let mut inlines = inlines.as_slice();
                if let Some(number) = attr.attributes.iter().find(|(key, _)| key == "number") {
                    // The attribute is a **break opportunity**, as it is
                    // on the `<a>` above: pandoc fills a contents entry
                    // and will break between `<span` and its class
                    // rather than overrun the column.
                    let _ = write!(html, "<span{BREAK}class=\"toc-section-number\">");
                    escape_text(&mut html, &number.1);
                    html.push_str("</span> ");
                    if let [Inline::Span(attr, _), Inline::Space, rest @ ..] = inlines {
                        if attr.classes.iter().any(|class| class == "header-section-number") {
                            inlines = rest;
                        }
                    }
                }
                write_inlines(&mut html, inlines);
                if !attr.identifier.is_empty() {
                    html.push_str("</a>");
                }
                out.push((*level, html));
            }
            Block::Div(_, blocks) => collect_toc(blocks, depth, out),
            _ => {}
        }
    }
}

/// A metadata value as plain text, however it was spelled.
fn meta_text(doc: &Pandoc, key: &str) -> Option<String> {
    meta_texts(doc, key).into_iter().next()
}

/// Every value under `key`: a metadata field may be one value or a list,
/// and `author` routinely is a list.
fn meta_texts(doc: &Pandoc, key: &str) -> Vec<String> {
    fn flatten(value: &ferrodoc_ast::MetaValue, out: &mut Vec<String>) {
        match value {
            ferrodoc_ast::MetaValue::MetaString(s) => out.push(s.clone()),
            ferrodoc_ast::MetaValue::MetaInlines(inlines) => out.push(plain_text(inlines)),
            ferrodoc_ast::MetaValue::MetaBlocks(blocks) => {
                for block in blocks {
                    if let Block::Plain(inlines) | Block::Para(inlines) = block {
                        out.push(plain_text(inlines));
                    }
                }
            }
            ferrodoc_ast::MetaValue::MetaList(values) => {
                for value in values {
                    flatten(value, out);
                }
            }
            ferrodoc_ast::MetaValue::MetaBool(_) | ferrodoc_ast::MetaValue::MetaMap(_) => {}
        }
    }
    let mut out = Vec::new();
    if let Some(value) = doc.meta.get(key) {
        flatten(value, &mut out);
    }
    out.retain(|text| !text.is_empty());
    out
}

/// Take the footnotes out of the tree, leaving pandoc's reference where
/// each one stood.
///
/// Numbering is the order of first *reference*, which is document order,
/// so the walk that replaces them is also the walk that numbers them. A
/// note inside a note's body is reached by the growing loop below and
/// numbered after every note the document itself holds — pandoc's order.
///
/// The clone is paid for only by a document that has a note: without one
/// the tree is borrowed as it stands.
fn take_notes<'a>(
    blocks: &'a [Block],
    prefix: &str,
) -> (std::borrow::Cow<'a, [Block]>, Vec<Vec<Block>>) {
    if !has_note(blocks) {
        return (std::borrow::Cow::Borrowed(blocks), Vec::new());
    }
    let mut owned = blocks.to_vec();
    let mut bodies: Vec<Vec<Block>> = Vec::new();
    walk_inlines(&mut owned, &mut |list| replace_notes(list, prefix, &mut bodies));
    // A note whose body holds a note: the loop keeps going while the list
    // it is walking grows.
    let mut done = 0;
    while done < bodies.len() {
        let mut body = std::mem::take(&mut bodies[done]);
        walk_inlines(&mut body, &mut |list| replace_notes(list, prefix, &mut bodies));
        bodies[done] = body;
        done += 1;
    }
    (std::borrow::Cow::Owned(owned), bodies)
}

/// Swap each `Note` in one run of inlines for its reference, **including
/// the ones nested inside another inline** — `<em>text[^1]</em>` is a
/// note, and looking only at the top level of the run left it behind.
fn replace_notes(list: &mut [Inline], prefix: &str, bodies: &mut Vec<Vec<Block>>) {
    for inline in list.iter_mut() {
        match inline {
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
            | Inline::Image(_, inner, _) => replace_notes(inner, prefix, bodies),
            _ => {}
        }
        if let Inline::Note(body) = inline {
            bodies.push(std::mem::take(body));
            let number = bodies.len();
            *inline = Inline::RawInline(
                Box::new(ferrodoc_ast::Format("html".into())),
                // The attribute gaps are break marks like any other:
                // pandoc breaks a long reference between them, and this
                // is raw HTML by the time the writer sees it.
                format!(
                    "<a{BREAK}href=\"#{prefix}fn{number}\"{BREAK}class=\"footnote-ref\"\
                     {BREAK}id=\"{prefix}fnref{number}\"{BREAK}role=\"doc-noteref\">\
                     <sup>{number}</sup></a>"
                ),
            );
        }
    }
}

/// Whether any inline anywhere in `blocks` is a `Note`.
///
/// **Immutable and short-circuiting**, which is the whole point of it.
/// The version this replaces reached the mutable walker by cloning the
/// entire tree — `blocks.to_vec()` — so every document *without* a note
/// paid a full-tree allocation before the borrowed fast path could
/// return, and every document with one paid it twice.
fn has_note(blocks: &[Block]) -> bool {
    blocks.iter().any(block_has_note)
}

/// The same traversal `walk_inlines` makes, reading rather than writing.
fn block_has_note(block: &Block) -> bool {
    match block {
        Block::Plain(list) | Block::Para(list) | Block::Header(_, _, list) => note_inside(list),
        Block::LineBlock(lines) => lines.iter().any(|line| note_inside(line)),
        Block::BlockQuote(inner) | Block::Div(_, inner) => has_note(inner),
        Block::BulletList(items) | Block::OrderedList(_, items) => items.iter().any(|i| has_note(i)),
        Block::DefinitionList(entries) => entries.iter().any(|(term, definitions)| {
            note_inside(term) || definitions.iter().any(|d| has_note(d))
        }),
        Block::Figure(_, caption, inner) => has_note(&caption.blocks) || has_note(inner),
        Block::Table(table) => {
            has_note(&table.caption.blocks)
                || table
                    .head
                    .rows
                    .iter()
                    .chain(table.bodies.iter().flat_map(|b| b.head.iter().chain(&b.body)))
                    .chain(table.foot.rows.iter())
                    .any(|row| row.cells.iter().any(|cell| has_note(&cell.blocks)))
        }
        Block::HorizontalRule | Block::CodeBlock(..) | Block::RawBlock(..) => false,
    }
}

fn note_inside(list: &[Inline]) -> bool {
    list.iter().any(|inline| match inline {
        Inline::Note(_) => true,
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
        | Inline::Image(_, inner, _) => note_inside(inner),
        _ => false,
    })
}

/// Apply `f` to every run of inlines in the tree.
fn walk_inlines(blocks: &mut [Block], f: &mut impl FnMut(&mut Vec<Inline>)) {
    for block in blocks {
        match block {
            Block::Plain(list) | Block::Para(list) | Block::Header(_, _, list) => f(list),
            Block::LineBlock(lines) => {
                for line in lines {
                    f(line);
                }
            }
            Block::BlockQuote(inner) | Block::Div(_, inner) => walk_inlines(inner, f),
            Block::BulletList(items) | Block::OrderedList(_, items) => {
                for item in items {
                    walk_inlines(item, f);
                }
            }
            Block::DefinitionList(entries) => {
                for (term, definitions) in entries {
                    f(term);
                    for definition in definitions {
                        walk_inlines(definition, f);
                    }
                }
            }
            Block::Figure(_, caption, inner) => {
                walk_inlines(&mut caption.blocks, f);
                walk_inlines(inner, f);
            }
            Block::Table(table) => {
                walk_inlines(&mut table.caption.blocks, f);
                for row in table
                    .head
                    .rows
                    .iter_mut()
                    .chain(
                        table.bodies.iter_mut().flat_map(|b| b.head.iter_mut().chain(&mut b.body)),
                    )
                    .chain(table.foot.rows.iter_mut())
                {
                    for cell in &mut row.cells {
                        walk_inlines(&mut cell.blocks, f);
                    }
                }
            }
            Block::HorizontalRule | Block::CodeBlock(..) | Block::RawBlock(..) => {}
        }
    }
}

/// The endnote section pandoc puts at the end of a document with notes.
///
/// Two rules, both probed: the backlink goes **inside** the last block
/// when that block is a paragraph and on a line of its own when it is
/// not, and a note with an empty body gets **no backlink at all**.
fn write_notes(out: &mut String, notes: &[Vec<Block>], prefix: &str, ctx: &Ctx) {
    if notes.is_empty() {
        return;
    }
    let _ = writeln!(
        out,
        "<section{BREAK}id=\"{prefix}footnotes\"\
         {BREAK}class=\"footnotes footnotes-end-of-document\"\
         {BREAK}role=\"doc-endnotes\">\n<hr />\n<ol>"
    );
    for (index, body) in notes.iter().enumerate() {
        let number = index + 1;
        let back = format!(
            "<a{BREAK}href=\"#{prefix}fnref{number}\"{BREAK}class=\"footnote-back\"\
             {BREAK}role=\"doc-backlink\">\u{21a9}\u{fe0e}</a>"
        );
        let _ = write!(out, "<li{BREAK}id=\"{prefix}fn{number}\">");
        let mut rendered = String::new();
        write_blocks(&mut rendered, body, ctx);
        let rendered = rendered.trim_end_matches('\n');
        if rendered.is_empty() {
            out.push_str("</li>\n");
        } else if matches!(body.last(), Some(Block::Para(_) | Block::Plain(_))) {
            // The last paragraph ends `…</p>`; the backlink goes before it.
            let (before, close) = rendered.split_at(rendered.len() - "</p>".len());
            let _ = writeln!(out, "{before}{back}{close}</li>");
        } else {
            let _ = writeln!(out, "{rendered}\n{back}</li>");
        }
    }
    out.push_str("</ol>\n</section>\n");
}

fn write_blocks(out: &mut String, blocks: &[Block], ctx: &Ctx) {
    for block in blocks {
        write_block(out, block, ctx);
        out.push('\n');
    }
}

/// Like [`write_blocks`] but without the trailing newline after the last
/// block — the form used inside container elements.
fn write_blocks_joined(out: &mut String, blocks: &[Block], ctx: &Ctx) {
    let mut first = true;
    for block in blocks {
        if !first {
            out.push('\n');
        }
        first = false;
        write_block(out, block, ctx);
    }
}

fn write_block(out: &mut String, block: &Block, ctx: &Ctx) {
    match block {
        Block::Plain(inlines) => write_inlines(out, inlines),
        Block::Para(inlines) => {
            out.push_str("<p>");
            write_inlines(out, inlines);
            out.push_str("</p>");
        }
        Block::Header(level, attr, inlines) => {
            let _ = write!(out, "<h{level}");
            write_header_attr(out, attr);
            out.push('>');
            write_inlines(out, inlines);
            let _ = write!(out, "</h{level}>");
        }
        Block::CodeBlock(attr, text) => write_code_block(out, attr, text, ctx),
        Block::RawBlock(format, text) => {
            if format.0 == "html" {
                // The literal keeps its own trailing newline; with the block
                // separator this yields the blank line pandoc emits after
                // raw blocks.
                out.push_str(text);
            }
        }
        Block::BlockQuote(blocks) => {
            out.push_str("<blockquote>\n");
            write_blocks_joined(out, blocks, ctx);
            out.push_str("\n</blockquote>");
        }
        Block::BulletList(items) => {
            // Pandoc classes a bullet list only when every one of its items
            // opens with a box: a mixed list gets no class yet still gets the
            // boxes on the items that have one, and an `<ol>` never gets the
            // class however many of its items are task items. An empty list
            // takes the class, the same vacuous way pandoc's does.
            if items.iter().all(|item| item.first().and_then(task_box).is_some()) {
                out.push_str("<ul");
                out.push(BREAK);
                out.push_str("class=\"task-list\">\n");
            } else {
                out.push_str("<ul>\n");
            }
            write_list_items(out, items, ctx);
            out.push_str("</ul>");
        }
        Block::OrderedList(attrs, items) => {
            out.push_str("<ol");
            if attrs.start != 1 {
                let _ = write!(out, "{BREAK}start=\"{}\"", attrs.start);
            }
            // **An example list says so**, and takes `type="1"` with it
            // where a plain decimal list takes neither. The order is
            // `start`, then the class, then the type — measured with a
            // start of 5, which is the only way to see it.
            if attrs.style == ListNumberStyle::Example {
                let _ = write!(out, "{BREAK}class=\"example\"{BREAK}type=\"1\"");
            } else if let Some(t) = list_type(attrs.style) {
                let _ = write!(out, "{BREAK}type=\"{t}\"");
            }
            out.push_str(">\n");
            write_list_items(out, items, ctx);
            out.push_str("</ol>");
        }
        Block::DefinitionList(items) => {
            out.push_str("<dl>\n");
            for (term, definitions) in items {
                out.push_str("<dt>");
                write_inlines(out, term);
                out.push_str("</dt>\n");
                for definition in definitions {
                    out.push_str("<dd>\n");
                    write_blocks_joined(out, definition, ctx);
                    out.push_str("\n</dd>\n");
                }
            }
            out.push_str("</dl>");
        }
        Block::HorizontalRule => out.push_str("<hr />"),
        Block::LineBlock(lines) => {
            out.push_str("<div class=\"line-block\">");
            let mut first = true;
            for line in lines {
                if !first {
                    out.push_str("<br />\n");
                }
                first = false;
                write_inlines(out, line);
            }
            out.push_str("</div>");
        }
        Block::Div(attr, blocks) => write_div(out, attr, blocks, ctx),
        Block::Figure(attr, caption, blocks) => {
            out.push_str("<figure");
            write_attr(out, attr);
            out.push_str(">\n");
            write_blocks_joined(out, blocks, ctx);
            write_figcaption(out, caption, blocks, ctx);
            out.push_str("\n</figure>");
        }
        Block::Table(table) => write_table(out, table, ctx),
    }
}

/// Not every `Div` is a `<div>`. Measured on `pandoc -f json -t html5`:
///
/// * a `Div` classed `section`, or one whose first block is a `Header`
///   carrying no identifier of its own, is written as `<section>`;
/// * the `section` class is never written — the element already says it;
/// * such a header is the section's own, so the section takes the
///   header's classes in front of the div's and the header's attributes
///   behind the div's, duplicates dropped and the header's value
///   winning, while the header keeps both;
/// * and when the div has nothing but `section` to say, it is not a
///   section at all: pandoc drops the wrapper and moves its id onto the
///   header.
///
/// `html4` writes `<div class="section ...">` for the same input; this
/// writer only writes html5, so the choice does not arise.
///
/// EPUB and DOCX wrap every heading in one of these, so it is most of
/// what `ferrodoc -t html` writes from either — and it wrote `<div>`
/// for all of them until 2026-08-24.
fn write_div(out: &mut String, attr: &Attr, blocks: &[Block], ctx: &Ctx) {
    let head = match blocks.first() {
        Some(Block::Header(_, header, _))
            if header.identifier.is_empty() && ctx.sections == Sections::Given =>
        {
            Some(header)
        }
        _ => None,
    };
    let own: Vec<&String> = attr.classes.iter().filter(|class| *class != "section").collect();

    if let (Some(head), true) = (head, own.is_empty()) {
        // The wrapper existed only to carry the identifier, so the
        // header carries it and the wrapper goes.
        let Some(Block::Header(level, _, inlines)) = blocks.first() else { unreachable!() };
        let promoted = Attr { identifier: attr.identifier.clone(), ..head.clone() };
        write_block(out, &Block::Header(*level, promoted, inlines.clone()), ctx);
        for block in &blocks[1..] {
            out.push('\n');
            write_block(out, block, ctx);
        }
        return;
    }

    let section = head.is_some() || attr.classes.iter().any(|class| class == "section");
    let tag = if section { "section" } else { "div" };
    let mut merged = Attr {
        identifier: attr.identifier.clone(),
        classes: Vec::new(),
        attributes: attr.attributes.clone(),
    };
    for class in head.map(|h| h.classes.iter()).into_iter().flatten().chain(own) {
        if !merged.classes.contains(class) {
            merged.classes.push(class.clone());
        }
    }
    for (key, value) in head.map(|h| h.attributes.iter()).into_iter().flatten() {
        match merged.attributes.iter_mut().find(|(k, _)| k == key) {
            Some(slot) => slot.1.clone_from(value),
            None => merged.attributes.push((key.clone(), value.clone())),
        }
    }
    let _ = write!(out, "<{tag}");
    write_attr(out, &merged);
    out.push_str(">\n");
    write_blocks_joined(out, blocks, ctx);
    let _ = write!(out, "\n</{tag}>");
}

/// A code block: highlighted where the language is one the highlighter
/// knows, and otherwise exactly the `<pre class="…"><code>` this wrote
/// before there was a highlighter at all.
fn write_code_block(out: &mut String, attr: &Attr, text: &str, ctx: &Ctx) {
    // **A block carrying its own identifier does not take a number.**
    // Pandoc names the wrapper `<div>` after the block's own id and
    // leaves the counter where it was, so every *later* block here was
    // numbered one too high — output for the identified block itself is
    // identical either way, which is why no gate saw it. Found by the
    // AST sweep, where a `cb8` came back as `cb9`.
    let number = if attr.identifier.is_empty() { ctx.next_block() } else { 0 };
    #[cfg(feature = "highlight")]
    if let Some(language) = attr
        .classes
        .iter()
        .find(|class| highlight::known(class))
        .filter(|_| ctx.highlighting == Highlighting::Default)
    {
        write_highlighted(out, attr, text, language, number);
        return;
    }
    let _ = number;
    out.push_str("<pre");
    write_attr(out, attr);
    out.push_str("><code>");
    // The **code itself** is one unbreakable piece, and its width does
    // not enter the decision at the gap before it: pandoc measures a tag
    // against the column and then appends the element's content whatever
    // its width. The mark goes after `<code>`, which is still tag text —
    // measured against 20 columns, `<pre class="bash"><code>` breaks and
    // `<pre class="bash">` alone does not. Without the mark at all,
    // `<pre class="rust">` broke on any long code line, which pandoc
    // never does.
    out.push(STOP);
    escape_code_block(out, text);
    out.push_str("</code></pre>");
}

/// A code block in a language the highlighter knows, in the shape
/// pandoc's writer emits. Every part of it was read off the binary:
///
/// * the wrapper is `<div class="sourceCode" id="cbN">`, and **the block's
///   own key-value attributes go on that div** rather than the `<pre>`;
/// * `cbN` numbers every code block in the document, highlighted or not,
///   and an explicit identifier replaces it — the line anchors then read
///   `myid-1` rather than `cb1-1`;
/// * the `<pre>` carries `sourceCode` and then the block's classes in
///   the order they were written, untouched — matching a syntax is
///   case-insensitive but writing the class back is not;
/// * the `<code>` carries `sourceCode` and the syntax's **canonical**
///   name, which is not always the one written — `sh` is `bash` there;
/// * `.numberLines` adds `numberSource` before the classes and takes
///   `aria-hidden`/`tabindex` off every anchor;
/// * and an empty block has no line spans at all.
#[cfg(feature = "highlight")]
fn write_highlighted(out: &mut String, attr: &Attr, text: &str, language: &str, number: usize) {
    let numbered = attr.classes.iter().any(|class| class == "numberLines");
    let id = if attr.identifier.is_empty() {
        format!("cb{number}")
    } else {
        attr.identifier.clone()
    };
    // **Class before id on this div**, which is not the order
    // `write_attr` uses everywhere else — measured, and the one place
    // pandoc's writer builds the attributes itself.
    out.push_str("<div");
    write_classes(out, &Attr { classes: vec!["sourceCode".to_owned()], ..Attr::default() });
    write_id(out, &Attr { identifier: id.clone(), ..Attr::default() });
    for (key, value) in &attr.attributes {
        write_kv(out, key, value);
    }
    out.push('>');

    let mut classes = vec!["sourceCode".to_owned()];
    if numbered {
        classes.push("numberSource".to_owned());
    }
    // The classes as they were written: the *reader* lowercases a
    // fence's info string, the writer does not touch it, and `{.C}`
    // written out by hand stays `C` here and matches `c` all the same.
    //
    // **`sourceCode` is dropped from the block's own classes**, because
    // this writer has already put one at the front. Reading pandoc's HTML
    // back gives a block that carries it — the AST keeps it, both readers
    // agree — so `html -> html` was emitting
    // `class="sourceCode sourceCode bash"`. Only this class deduplicates:
    // `numberSource` in the block's classes really is written twice, which
    // was probed rather than assumed.
    classes.extend(attr.classes.iter().filter(|c| *c != "sourceCode").cloned());
    out.push_str("<pre");
    write_attr(out, &Attr { classes, ..Attr::default() });
    // **No break opportunity before the `<code>`'s class.** The div's
    // and the pre's attributes each offer one; this one does not, so the
    // fill measures `class="sourceCode python"><code class="sourceCode
    // python">` as a single piece — which is why pandoc breaks after
    // `<pre` and not after `<code`. Measured at 72 columns.
    let _ = write!(
        out,
        "><code class=\"sourceCode {}\">",
        highlight::canonical(language)
    );
    // The code is one unbreakable piece, as it is without highlighting.
    out.push(STOP);

    let body = text.strip_suffix('\n').unwrap_or(text);
    if !body.is_empty() {
        let mut state = highlight::State::default();
        for (index, source) in body.split('\n').enumerate() {
            if index > 0 {
                out.push('\n');
            }
            let anchor = format!("{id}-{}", index + 1);
            let _ = write!(out, "<span id=\"{anchor}\"><a href=\"#{anchor}\"");
            if !numbered {
                out.push_str(" aria-hidden=\"true\" tabindex=\"-1\"");
            }
            out.push_str("></a>");
            highlight::write_line(out, &highlight::line(source, language, &mut state), escape_code_block);
            out.push_str("</span>");
        }
    }
    out.push_str("</code></pre></div>");
}

fn write_list_items(out: &mut String, items: &[Vec<Block>], ctx: &Ctx) {
    for item in items {
        out.push_str("<li>");
        match item.first().and_then(task_box) {
            // The box and the space after it become the `<input>`, the rest
            // of that first block goes in the `<label>` beside it, and the
            // item's remaining blocks stand as they are.
            Some((checked, label)) => {
                let para = matches!(item[0], Block::Para(_));
                if para {
                    out.push_str("<p>");
                }
                out.push_str("<label><input");
                out.push(BREAK);
                out.push_str("type=\"checkbox\"");
                if checked {
                    out.push(BREAK);
                    out.push_str("checked=\"\"");
                }
                out.push_str(" />");
                write_inlines(out, label);
                out.push_str("</label>");
                if para {
                    out.push_str("</p>");
                }
                for block in &item[1..] {
                    out.push('\n');
                    write_block(out, block, ctx);
                }
            }
            None => write_blocks_joined(out, item, ctx),
        }
        out.push_str("</li>\n");
    }
}

/// A task-list box opening a list item's first block: whether it is ticked,
/// and the inlines that follow it. Pandoc's HTML writer takes only a
/// `Str "☒"`/`Str "☐"` immediately followed by a `Space` at the head of a
/// `Plain` or `Para`; a `SoftBreak` in the space's place, a box with nothing
/// after it, a box inside an `Emph`, and a box in a `Str` of its own with the
/// space attached all stay literal text.
fn task_box(block: &Block) -> Option<(bool, &[Inline])> {
    let (Block::Plain(inlines) | Block::Para(inlines)) = block else {
        return None;
    };
    let checked = match inlines.first()? {
        Inline::Str(marker) if marker == "\u{2610}" => false,
        Inline::Str(marker) if marker == "\u{2612}" => true,
        _ => return None,
    };
    matches!(inlines.get(1), Some(Inline::Space)).then(|| (checked, &inlines[2..]))
}

fn list_type(style: ListNumberStyle) -> Option<&'static str> {
    match style {
        ListNumberStyle::Decimal => Some("1"),
        ListNumberStyle::LowerAlpha => Some("a"),
        ListNumberStyle::UpperAlpha => Some("A"),
        ListNumberStyle::LowerRoman => Some("i"),
        ListNumberStyle::UpperRoman => Some("I"),
        ListNumberStyle::DefaultStyle | ListNumberStyle::Example => None,
    }
}

/// Whether a caption says nothing the picture's own alt text does not.
///
/// Such a caption is hidden from assistive technology, which would
/// otherwise announce the same words twice — the shape markdown's
/// implicit figures produce, where the caption *is* the alt text.
/// Measured on `pandoc -f json -t html5`:
///
/// * the body must be exactly `Plain [Image]`. A `Para` body is a
///   paragraph that happens to hold a picture, and anything beside the
///   image — even a trailing word — is a caption worth reading.
/// * the two are compared **rendered**, not structurally, so an
///   emphasized caption still matches a plain alt.
/// * the caption's own block may be `Plain` or `Para`; that much does
///   not matter.
fn caption_repeats_alt(caption: &Caption, blocks: &[Block]) -> bool {
    let [Block::Plain(inlines)] = blocks else {
        return false;
    };
    let [Inline::Image(_, alt, _)] = inlines.as_slice() else {
        return false;
    };
    let [Block::Plain(text) | Block::Para(text)] = caption.blocks.as_slice() else {
        return false;
    };
    plain_text(alt) == plain_text(text)
}

fn write_figcaption(out: &mut String, caption: &Caption, blocks: &[Block], ctx: &Ctx) {
    if caption.blocks.is_empty() {
        return;
    }
    out.push_str(if caption_repeats_alt(caption, blocks) {
        "\n<figcaption aria-hidden=\"true\">"
    } else {
        "\n<figcaption>"
    });
    write_blocks_joined(out, &caption.blocks, ctx);
    out.push_str("</figcaption>");
}

fn write_table(out: &mut String, table: &Table, ctx: &Ctx) {
    out.push_str("<table");
    write_attr(out, &table.attr);
    // A table whose columns carry relative widths says so on the element,
    // and only when they add up to less than the full width. Measured
    // against pandoc 3.8.2.1: the *table* total is rounded, each *column*
    // is truncated — 0.335 is a 33% column inside a 67% table.
    let total: f64 = table.colspecs.iter().filter_map(|c| c.width.fraction()).sum();
    if !table.colspecs.is_empty() && total > 0.0 && total < 1.0 {
        let _ = write!(out, "{BREAK}style=\"width:{}%;\"", percent((total * 100.0).round()));
    }
    out.push('>');
    if !table.caption.blocks.is_empty() {
        out.push_str("\n<caption>");
        write_blocks_joined(out, &table.caption.blocks, ctx);
        out.push_str("</caption>");
    }
    // The column widths a word processor set. Dropping them made every
    // converted table equal-width — the DOCX reader had the numbers, and
    // `diff-html` could not see it because its corpus is the CommonMark
    // spec, which has no tables.
    if table.colspecs.iter().any(|c| c.width.fraction().is_some()) {
        out.push_str("\n<colgroup>");
        for colspec in &table.colspecs {
            match colspec.width.fraction() {
                Some(width) => {
                    let _ = write!(out, "\n<col style=\"width: {}%\" />", percent(width * 100.0));
                }
                None => out.push_str("\n<col />"),
            }
        }
        out.push_str("\n</colgroup>");
    }
    // **A header row of nothing but empty cells is not a header.** A
    // GFM table always has one, and a table converted from a format with
    // no header concept arrives with a row of blanks; pandoc writes
    // straight to `<tbody>` for it, and a `<thead>` full of empty `<th>`
    // is a row a browser draws. One non-empty cell is enough to keep it.
    let header = table.head.rows.iter().any(|row| {
        row.cells.iter().any(|cell| !cell.blocks.is_empty())
    });
    if header {
        out.push_str("\n<thead>");
        for row in &table.head.rows {
            write_table_row(out, row, "th", &table.colspecs, ctx);
        }
        out.push_str("\n</thead>");
    }
    for body in &table.bodies {
        out.push_str("\n<tbody>");
        for row in body.head.iter().chain(&body.body) {
            write_table_row(out, row, "td", &table.colspecs, ctx);
        }
        out.push_str("\n</tbody>");
    }
    if !table.foot.rows.is_empty() {
        out.push_str("\n<tfoot>");
        for row in &table.foot.rows {
            write_table_row(out, row, "td", &table.colspecs, ctx);
        }
        out.push_str("\n</tfoot>");
    }
    out.push_str("\n</table>");
}

fn write_table_row(
    out: &mut String,
    row: &Row,
    cell_tag: &str,
    colspecs: &[ColSpec],
    ctx: &Ctx,
) {
    out.push_str("\n<tr>");
    // The column a cell sits in is its position *after* the spans before
    // it, which is what makes the column's alignment findable.
    let mut column = 0usize;
    for cell in &row.cells {
        write_table_cell(out, cell, cell_tag, colspecs.get(column), ctx);
        column += usize::try_from(cell.col_span).unwrap_or(1).max(1);
    }
    out.push_str("\n</tr>");
}

fn write_table_cell(
    out: &mut String,
    cell: &Cell,
    tag: &str,
    colspec: Option<&ColSpec>,
    ctx: &Ctx,
) {
    let _ = write!(out, "\n<{tag}");
    if cell.row_span != 1 {
        let _ = write!(out, "{BREAK}rowspan=\"{}\"", cell.row_span);
    }
    if cell.col_span != 1 {
        let _ = write!(out, "{BREAK}colspan=\"{}\"", cell.col_span);
    }
    // A cell's own alignment wins, and almost no cell has one: pandoc
    // keeps table alignment in the **column specs**, so a `|---:|` header
    // leaves every cell `AlignDefault` and the column holding the answer.
    // Reading only the cell dropped the alignment of every markdown and
    // HTML table — invisible to `diff-html`, whose corpus is the
    // CommonMark spec, which has no tables in it at all.
    let alignment = match cell.alignment {
        Alignment::AlignDefault => colspec.map_or(Alignment::AlignDefault, |c| c.alignment),
        explicit => explicit,
    };
    if let Some(align) = alignment_style(alignment) {
        let _ = write!(out, "{BREAK}style=\"text-align: {align};\"");
    }
    out.push('>');
    write_blocks_joined(out, &cell.blocks, ctx);
    let _ = write!(out, "</{tag}>");
}

/// A scaled width as whole percent. The caller rounds or truncates first
/// — pandoc does both, in different places, and the difference is visible
/// in the output.
#[expect(clippy::cast_possible_truncation, reason = "a percentage, and the truncation is the rule")]
fn percent(scaled: f64) -> i64 {
    scaled as i64
}

fn alignment_style(alignment: Alignment) -> Option<&'static str> {
    match alignment {
        Alignment::AlignLeft => Some("left"),
        Alignment::AlignRight => Some("right"),
        Alignment::AlignCenter => Some("center"),
        Alignment::AlignDefault => None,
    }
}

fn write_inlines(out: &mut String, inlines: &[Inline]) {
    for inline in inlines {
        write_inline(out, inline);
    }
}

/// A code span, whose attributes are spelled unlike every other
/// element's — see the comment inside.
fn write_code_span(out: &mut String, attr: &Attr, text: &str) {
            out.push_str("<code");
            // **A code span carrying any class is `sourceCode` first**,
            // and its class attribute is written *before* the id — the
            // reverse of every other element here. With no class at all
            // the ordinary order applies. Measured one attribute set at
            // a time; `["c"]` is `class="sourceCode c"`.
            if attr.classes.is_empty() {
                write_attr(out, attr);
            } else {
                let mut classes = vec!["sourceCode".to_owned()];
                classes.extend(attr.classes.iter().cloned());
                let listed =
                    Attr { classes, identifier: String::new(), attributes: Vec::new() };
                write_classes(out, &listed);
                write_id(out, attr);
                for (key, value) in &attr.attributes {
                    write_kv(out, key, value);
                }
            }
            out.push('>');
            escape_text(out, text);
            out.push_str("</code>");
}

fn write_inline(out: &mut String, inline: &Inline) {
    match inline {
        Inline::Str(s) => escape_text(out, s),
        Inline::Space => out.push(BREAK),
        Inline::SoftBreak => out.push(SOFT),
        Inline::LineBreak => out.push_str("<br />\n"),
        Inline::Emph(inner) => wrap_tag(out, "em", inner),
        Inline::Strong(inner) => wrap_tag(out, "strong", inner),
        Inline::Strikeout(inner) => wrap_tag(out, "del", inner),
        Inline::Superscript(inner) => wrap_tag(out, "sup", inner),
        Inline::Subscript(inner) => wrap_tag(out, "sub", inner),
        Inline::Underline(inner) => wrap_tag(out, "u", inner),
        Inline::SmallCaps(inner) => {
            out.push_str("<span class=\"smallcaps\">");
            write_inlines(out, inner);
            out.push_str("</span>");
        }
        Inline::Quoted(quote_type, inner) => {
            use ferrodoc_ast::QuoteType;
            let (open, close) = match quote_type {
                QuoteType::SingleQuote => ('\u{2018}', '\u{2019}'),
                QuoteType::DoubleQuote => ('\u{201C}', '\u{201D}'),
            };
            out.push(open);
            write_inlines(out, inner);
            out.push(close);
        }
        Inline::Code(attr, text) => write_code_span(out, attr, text),
        Inline::Math(math_type, text) => {
            use ferrodoc_ast::MathType;
            // **Dollar delimiters, not `\(`.** Pandoc writes `\(…\)`
            // only under `--mathjax`, which this has no flag for; its
            // default keeps the TeX between dollars, and that is what a
            // reader sees when nothing renders it. Pandoc *does* render
            // simple expressions to markup — `x^2` becomes
            // `<em>x</em><sup>2</sup>` — and falls back to this form for
            // anything with a fraction, a root or a sum. Only the
            // fallback is written here; COMPATIBILITY.md records the gap.
            let (class, open, close) = match math_type {
                MathType::InlineMath => ("math inline", "$", "$"),
                MathType::DisplayMath => ("math display", "$$", "$$"),
            };
            let _ = write!(out, "<span class=\"{class}\">{open}");
            escape_text(out, text);
            let _ = write!(out, "{close}</span>");
        }
        Inline::RawInline(format, text) => {
            if format.0 == "html" {
                out.push_str(text);
            }
        }
        Inline::Link(attr, inner, target) => {
            out.push_str("<a");
            out.push(BREAK);
            out.push_str("href=\"");
            escape_attribute(out, &target.url);
            out.push('"');
            if !target.title.is_empty() {
                out.push(BREAK);
                out.push_str("title=\"");
                escape_attribute(out, &target.title);
                out.push('"');
            }
            write_attr(out, attr);
            out.push('>');
            write_inlines(out, inner);
            out.push_str("</a>");
        }
        Inline::Image(attr, alt, target) => {
            out.push_str("<img");
            out.push(BREAK);
            out.push_str("src=\"");
            escape_attribute(out, &target.url);
            out.push('"');
            if !target.title.is_empty() {
                out.push(BREAK);
                out.push_str("title=\"");
                escape_attribute(out, &target.title);
                out.push('"');
            }
            // Pandoc omits the alt attribute only when the alt inlines are
            // empty (`![](url)`); non-empty inlines that render to empty
            // text still produce alt="".
            if !alt.is_empty() {
                out.push(BREAK);
                out.push_str("alt=\"");
                escape_attribute(out, &plain_text(alt));
                out.push('"');
            }
            write_attr(out, attr);
            out.push_str(" />");
        }
        Inline::Span(attr, inner) => {
            out.push_str("<span");
            write_attr(out, attr);
            out.push('>');
            write_inlines(out, inner);
            out.push_str("</span>");
        }
        // A citation keeps the keys it stands for, space-separated, so a
        // bibliography filter downstream can still find them. Pandoc
        // writes them whether or not it resolved the citation.
        Inline::Cite(citations, inner) => {
            let keys = citations
                .iter()
                .map(|c| c.citation_id.as_str())
                .collect::<Vec<_>>()
                .join(" ");
            let _ = write!(out, "<span class=\"citation\"{BREAK}data-cites=\"{keys}\">");
            write_inlines(out, inner);
            out.push_str("</span>");
        }
        Inline::Note(_) => {}
    }
}

fn wrap_tag(out: &mut String, tag: &str, inner: &[Inline]) {
    let _ = write!(out, "<{tag}>");
    write_inlines(out, inner);
    let _ = write!(out, "</{tag}>");
}

/// A heading's attributes, which are **not** in the order every other
/// element's are: the identifier comes last, and a section number comes
/// first among the key-values. Probed against pandoc 3.8.2.1 with
/// `--wrap=none`, on `# H {#i .foo data-k=v}`:
///
/// ```text
/// pandoc                       <h1 class="foo" data-k="v" id="i">H</h1>
/// pandoc --number-sections     <h1 class="foo" data-number="1" data-k="v" id="i">…
/// ```
///
/// A `Div` with the same attributes gets `id` first, so this is a heading
/// rule rather than a document-wide one. No gate reached it before
/// `--number-sections` existed: the `CommonMark` spec's headings carry no
/// attributes at all, so `diff-html` scores 652/652 either way.
fn write_header_attr(out: &mut String, attr: &Attr) {
    write_classes(out, attr);
    if let Some((_, number)) = attr.attributes.iter().find(|(key, _)| key == "number") {
        write_kv(out, "number", number);
    }
    for (key, value) in attr.attributes.iter().filter(|(key, _)| key != "number") {
        write_kv(out, key, value);
    }
    write_id(out, attr);
}

fn write_id(out: &mut String, attr: &Attr) {
    if attr.identifier.is_empty() {
        return;
    }
    out.push(BREAK);
    out.push_str("id=\"");
    escape_attribute(out, &attr.identifier);
    out.push('"');
}

fn write_classes(out: &mut String, attr: &Attr) {
    if attr.classes.is_empty() {
        return;
    }
    out.push(BREAK);
    out.push_str("class=\"");
    escape_attribute(out, &attr.classes.join(" "));
    out.push('"');
}

fn write_kv(out: &mut String, key: &str, value: &str) {
    out.push(BREAK);
    // Pandoc's rule, probed on `-f json -t html`: a name HTML does not know
    // is written behind `data-` (`foo` becomes `data-foo`), and a name it
    // knows is written as it stands (`onclick`, `style`, `href`). This is
    // **fidelity, not sanitizing** — a live `onclick` in the source passes
    // through here exactly as it passes through pandoc. `is_reserved` is
    // the "HTML knows this name" test, shared with the reader so the two
    // stay symmetric.
    //
    // `starts_with` is not redundant with it. The reader leaves
    // `data-onclick` *whole* precisely because `onclick` is reserved, so
    // the key arriving here is already prefixed; without this check the
    // writer prefixed it again and `ferrodoc -f html -t html` turned
    // `data-onclick` into `data-data-onclick`. It hit only the reserved
    // names — `onclick`, `style`, `href`, `id` — and never an ordinary
    // `data-k`, whose bare `k` the reader had already unwrapped.
    if !key.starts_with("data-") && !read::is_reserved(key) {
        out.push_str("data-");
    }
    // Keys come from the same untrusted AST as values; drop characters
    // that could break out of the tag.
    out.extend(key.chars().filter(|c| !c.is_whitespace() && !"\"'<>=/&".contains(*c)));
    out.push_str("=\"");
    escape_attribute(out, value);
    out.push('"');
}

/// Render attributes as ` id=".." class=".." k="v"`, pandoc's order —
/// except on a heading, which has its own order; see [`write_header_attr`].
fn write_attr(out: &mut String, attr: &Attr) {
    write_id(out, attr);
    write_classes(out, attr);
    for (key, value) in &attr.attributes {
        write_kv(out, key, value);
    }
}

/// The plain-text rendering of inlines (used for `alt` attributes).
fn plain_text(inlines: &[Inline]) -> String {
    let mut out = String::new();
    collect_plain(&mut out, inlines);
    out
}

fn collect_plain(out: &mut String, inlines: &[Inline]) {
    for inline in inlines {
        match inline {
            Inline::Str(s) | Inline::Code(_, s) | Inline::Math(_, s) => out.push_str(s),
            // Pandoc's alt-text stringify renders every break as a space.
            Inline::Space | Inline::SoftBreak | Inline::LineBreak => out.push(' '),
            Inline::Emph(i) | Inline::Strong(i) | Inline::Strikeout(i)
            | Inline::Superscript(i) | Inline::Subscript(i) | Inline::SmallCaps(i)
            | Inline::Underline(i) | Inline::Quoted(_, i) | Inline::Cite(_, i)
            | Inline::Span(_, i) | Inline::Link(_, i, _) | Inline::Image(_, i, _) => {
                collect_plain(out, i);
            }
            Inline::RawInline(..) | Inline::Note(_) => {}
        }
    }
}

/// Escape text content: `&`, `<`, `>` (pandoc leaves `"` alone in text).
fn escape_text(out: &mut String, text: &str) {
    // A plain per-character loop, deliberately: a "smarter" version that
    // searched for the next special character and copied slices measured
    // ~18% slower here, because these strings are short words and the
    // search machinery costs more than the copying it saves.
    for ch in text.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            ch => out.push(ch),
        }
    }
}

/// Escape code-block content: unlike inline code (`&`, `<`, `>` only),
/// pandoc also escapes `"` and `'` inside `<pre><code>` — the same set as
/// attribute values.
fn escape_code_block(out: &mut String, text: &str) {
    escape_attribute(out, text);
}

/// Escape attribute values: `&`, `<`, `>`, `"`, and `'` as `&#39;`
/// (pandoc escapes apostrophes in every attribute context).
fn escape_attribute(out: &mut String, text: &str) {
    for ch in text.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            ch => out.push(ch),
        }
    }
}

#[cfg(test)]
mod tests {
    /// A page with pandoc's defaults, which is what most of these
    /// tests want.
    fn page_of(doc: &Pandoc) -> String {
        write_page(doc, &Page::new()).expect("rendered")
    }

    use super::*;

    /// **The preflight must not clone, and must still find every note.**
    /// `has_note` reached the mutable walker through `blocks.to_vec()`
    /// until 2026-08-27 — a full-tree allocation on every document,
    /// including the ones with no note at all. These pin the answers the
    /// immutable walk has to keep giving, in the places a shallow walk
    /// would miss: inside an inline, inside a list, inside a table cell.
    #[test]
    fn the_footnote_preflight_finds_a_note_wherever_it_is() {
        let note = || Inline::Note(vec![Block::Para(vec![Inline::Str("body".into())])]);
        let para = |inline| Block::Para(vec![inline]);

        assert!(!has_note(&[para(Inline::Str("plain".into()))]));
        assert!(!has_note(&[]));
        assert!(has_note(&[para(note())]));
        // Inside an inline, which is where looking only at the top level
        // of a run left one behind.
        assert!(has_note(&[para(Inline::Emph(vec![note()]))]));
        assert!(has_note(&[para(Inline::Link(
            Box::default(),
            vec![Inline::Strong(vec![note()])],
            Box::new(ferrodoc_ast::Target { url: "u".into(), title: String::new() }),
        ))]));
        // Inside a block that holds blocks.
        assert!(has_note(&[Block::BlockQuote(vec![para(note())])]));
        assert!(has_note(&[Block::BulletList(vec![vec![para(note())]])]));
    }

    /// A note whose body holds a note is numbered after every note the
    /// document itself holds, and the borrowed fast path must not swallow
    /// it — the outer note is what makes the tree owned in the first place.
    #[test]
    fn a_note_inside_a_note_is_still_written() {
        let inner = Inline::Note(vec![Block::Para(vec![Inline::Str("inner".into())])]);
        let outer = Inline::Note(vec![Block::Para(vec![Inline::Str("outer".into()), inner])]);
        let html = write_html(&Pandoc::new(vec![Block::Para(vec![outer])]));
        assert!(html.contains("id=\"fn1\""), "{html}");
        assert!(html.contains("id=\"fn2\""), "{html}");
        assert!(html.contains("inner"), "{html}");
    }

    /// **A caption that only repeats the alt text is hidden from screen
    /// readers**, and no gate here can see it: `writers.sh` reads
    /// `corpus/*.md` as `-f commonmark`, which has no implicit figures,
    /// so the only figure in that corpus never reaches this writer as
    /// one. Every answer below is off `pandoc -f json -t html5`.
    #[test]
    fn a_caption_repeating_the_alt_text_is_hidden_from_assistive_tech() {
        let image = |alt: Vec<Inline>| {
            Inline::Image(
                Box::default(),
                alt,
                Box::new(ferrodoc_ast::Target { url: "x.png".into(), title: String::new() }),
            )
        };
        let str_of = |s: &str| vec![Inline::Str(s.into())];
        let figure = |caption: Vec<Block>, body: Vec<Block>| {
            let doc = Pandoc::new(vec![Block::Figure(
                Attr::default(),
                Caption { short: None, blocks: caption },
                body,
            )]);
            write_html(&doc)
        };
        let plain_image = |alt: &str| vec![Block::Plain(vec![image(str_of(alt))])];

        // The shape `![alt](x.png)` produces: the caption says nothing
        // the alt does not.
        let html = figure(vec![Block::Plain(str_of("alt"))], plain_image("alt"));
        assert!(html.contains("<figcaption aria-hidden=\"true\">"), "{html}");

        // A caption with something of its own to say is announced.
        let html = figure(vec![Block::Plain(str_of("cap"))], plain_image("alt"));
        assert!(html.contains("<figcaption>"), "{html}");

        // Compared **rendered**, not structurally — emphasis does not
        // make a caption say anything new.
        let html = figure(
            vec![Block::Plain(vec![Inline::Emph(str_of("alt"))])],
            plain_image("alt"),
        );
        assert!(html.contains("<figcaption aria-hidden=\"true\">"), "{html}");

        // The body must be exactly `Plain [Image]`. A `Para` is a
        // paragraph that happens to hold a picture...
        let html = figure(
            vec![Block::Plain(str_of("alt"))],
            vec![Block::Para(vec![image(str_of("alt"))])],
        );
        assert!(html.contains("<figcaption>"), "{html}");

        // ...and a trailing word makes the caption worth reading.
        let html = figure(
            vec![Block::Plain(str_of("alt"))],
            vec![Block::Plain(vec![image(str_of("alt")), Inline::Str("tail".into())])],
        );
        assert!(html.contains("<figcaption>"), "{html}");
    }

    /// **One pass over the layout markers, and the same bytes.** `lay_out`
    /// chained `String::replace` until 2026-08-27; these pin what each
    /// wrap mode does with a `BREAK`, a `SOFT` and a `STOP`.
    #[test]
    fn each_wrap_mode_resolves_its_markers() {
        let marked = format!("a{BREAK}b{SOFT}c{STOP}d");
        // A `BREAK` becomes a space, a `SOFT` becomes a space or a
        // newline by mode, and a `STOP` is **dropped** — so `c{STOP}d`
        // closes up rather than gaining a space.
        assert_eq!(lay_out(&marked, Wrap::None), "a b cd");
        assert_eq!(lay_out(&marked, Wrap::Preserve), "a b\ncd");
        // Nothing to resolve is the common case, and must come back whole.
        assert_eq!(lay_out("plain text", Wrap::None), "plain text");
        assert_eq!(lay_out("", Wrap::None), "");
        // A marker at either end, where an off-by-one would show.
        assert_eq!(lay_out(&format!("{BREAK}a{BREAK}"), Wrap::None), " a ");
        assert_eq!(lay_out(&format!("{STOP}a{STOP}"), Wrap::Preserve), "a");
    }

    /// The footnote section, with the two rules that are not guessable.
    ///
    /// Every line here is `pandoc -f json -t html --wrap=none` output,
    /// run and pasted. The writer dropped footnotes entirely until
    /// 2026-08-23 and `diff-html` scored 652/652 throughout, because the
    /// `CommonMark` suite it runs on has no footnote to lose.
    #[test]
    fn a_footnote_becomes_a_reference_and_an_endnote() {
        let note = |body: Vec<Block>| {
            write_html(&Pandoc::new(vec![Block::Para(vec![
                Inline::Str("a".into()),
                Inline::Note(body),
            ])]))
        };
        let para = note(vec![Block::Para(vec![Inline::Str("body".into())])]);
        assert!(
            para.contains(
                "a<a href=\"#fn1\" class=\"footnote-ref\" id=\"fnref1\" \
                 role=\"doc-noteref\"><sup>1</sup></a>"
            ),
            "{para}"
        );
        // The backlink goes **inside** the last paragraph.
        assert!(
            para.contains(
                "<li id=\"fn1\"><p>body<a href=\"#fnref1\" class=\"footnote-back\" \
                 role=\"doc-backlink\">\u{21a9}\u{fe0e}</a></p></li>"
            ),
            "{para}"
        );
        // …and on a line of its own when the last block is not one.
        let list = note(vec![Block::BulletList(vec![vec![Block::Plain(vec![
            Inline::Str("x".into()),
        ])]])]);
        assert!(list.contains("</ul>\n<a href=\"#fnref1\""), "{list}");
        // An empty body gets no backlink at all.
        assert!(note(Vec::new()).contains("<li id=\"fn1\"></li>"), "{}", note(Vec::new()));
    }

    /// A note inside another inline is still a note. Looking only at the
    /// top level of each run left `<em>text[^1]</em>` unnumbered, which
    /// shifted every footnote after it by one.
    #[test]
    fn a_footnote_nested_in_an_inline_is_numbered_in_place() {
        let html = write_html(&Pandoc::new(vec![Block::Para(vec![
            Inline::Emph(vec![Inline::Note(vec![Block::Para(vec![Inline::Str("x".into())])])]),
            Inline::Note(vec![Block::Para(vec![Inline::Str("y".into())])]),
        ])]));
        assert!(html.contains("<em><a href=\"#fn1\""), "{html}");
        assert!(html.contains("</em><a href=\"#fn2\""), "{html}");
    }

    /// `--id-prefix` cannot reach the footnote identifiers through the
    /// tree, because the writer invents them. Two documents on one page
    /// colliding on `#fn1` is what the flag exists to prevent.
    #[test]
    fn id_prefix_reaches_the_identifiers_the_writer_invents() {
        let doc = Pandoc::new(vec![Block::Para(vec![
            Inline::Str("a".into()),
            Inline::Note(vec![Block::Para(vec![Inline::Str("b".into())])]),
        ])]);
        let html = write_html_with_id_prefix(&doc, "P-");
        for expected in ["#P-fn1", "id=\"P-fnref1\"", "id=\"P-fn1\"", "id=\"P-footnotes\""] {
            assert!(html.contains(expected), "{expected} in {html}");
        }
    }

    /// No differential gate reaches this: `diff-html` is markdown → HTML
    /// through `read_commonmark`, and `CommonMark` cannot express an
    /// arbitrary attribute, so every input to that gate scores the same
    /// before and after. Each line below is `pandoc -f json -t html`
    /// output, run and pasted.
    #[test]
    fn an_attribute_name_is_prefixed_exactly_when_html_does_not_know_it() {
        let div = |key: &str| {
            let attr = ferrodoc_ast::Attr {
                attributes: vec![(key.to_owned(), "v".to_owned())],
                ..ferrodoc_ast::Attr::default()
            };
            write_html(&Pandoc::new(vec![Block::Div(attr, Vec::new())]))
        };
        // Invented names go behind `data-`; names HTML knows do not.
        assert_eq!(div("foo"), "<div data-foo=\"v\">\n\n</div>\n");
        assert_eq!(div("onclick"), "<div onclick=\"v\">\n\n</div>\n");
        assert_eq!(div("style"), "<div style=\"v\">\n\n</div>\n");

        // Already prefixed: written once, not twice. Before this was
        // fixed, `ferrodoc -f html -t html` turned `data-onclick` into
        // `data-data-onclick` — and only ever the reserved names, because
        // the reader unwraps `data-k` to a bare `k` and leaves
        // `data-onclick` whole.
        for key in ["data-onclick", "data-style", "data-href", "data-id", "data-k"] {
            assert_eq!(div(key), format!("<div {key}=\"v\">\n\n</div>\n"), "{key}");
        }
    }

    fn html(md: &str) -> String {
        write_html(&ferrodoc_markdown::read_commonmark(md).expect("convertible"))
    }

    fn header(level: i64, id: &str, classes: &[&str], text: &str) -> Block {
        Block::Header(
            level,
            Attr {
                identifier: id.to_owned(),
                classes: classes.iter().map(|class| (*class).to_owned()).collect(),
                attributes: Vec::new(),
            },
            vec![Inline::Str(text.to_owned())],
        )
    }

    fn doc(blocks: Vec<Block>) -> Pandoc {
        Pandoc { blocks, ..Pandoc::default() }
    }

    fn section(id: &str, classes: &[&str], blocks: Vec<Block>) -> Block {
        Block::Div(
            Attr {
                identifier: id.to_owned(),
                classes: classes.iter().map(|class| (*class).to_owned()).collect(),
                attributes: Vec::new(),
            },
            blocks,
        )
    }

    /// `diff-html` cannot reach any of this: its corpus is the spec for
    /// a dialect with no way to write a `Div` at all. Every
    /// expectation here was read off `pandoc -f json -t html5` on the
    /// same AST — see [`write_div`] for the rule they pin.
    #[test]
    fn a_section_div_is_a_section_element() {
        let para = Block::Para(vec![Inline::Str("z".to_owned())]);

        // Classed `section`: the element says it, so the class is not written.
        let given = doc(vec![section("x", &["section", "level1"], vec![para.clone()])]);
        assert_eq!(write_html(&given).trim_end(), "<section id=\"x\" class=\"level1\">\n<p>z</p>\n</section>");

        // No `section` class and no header of that shape: still a `<div>`.
        let plain = doc(vec![section("x", &["level1"], vec![para.clone()])]);
        assert_eq!(write_html(&plain).trim_end(), "<div id=\"x\" class=\"level1\">\n<p>z</p>\n</div>");

        // A header with no identifier of its own is the section's own:
        // its classes go in front of the div's, duplicates dropped, and
        // it keeps them too.
        let owned = doc(vec![section(
            "x",
            &["section", "level1", "unnumbered"],
            vec![header(1, "", &["unnumbered"], "T"), para.clone()],
        )]);
        assert_eq!(
            write_html(&owned).trim_end(),
            "<section id=\"x\" class=\"unnumbered level1\">\n\
             <h1 class=\"unnumbered\">T</h1>\n<p>z</p>\n</section>"
        );

        // The same tree written by a caller that built the sections
        // itself — `--section-divs` — keeps the div's own order and does
        // not merge. This is the EPUB writer's path.
        assert_eq!(
            write_html_section_divs(&owned).trim_end(),
            "<section id=\"x\" class=\"level1 unnumbered\">\n\
             <h1 class=\"unnumbered\">T</h1>\n<p>z</p>\n</section>"
        );

        // Nothing but `section` left to say: the wrapper existed only to
        // carry the identifier, and the header carries it instead.
        let bare = doc(vec![section(
            "x",
            &["section"],
            vec![header(1, "", &["hc"], "T"), para],
        )]);
        assert_eq!(
            write_html(&bare).trim_end(),
            "<h1 class=\"hc\" id=\"x\">T</h1>\n<p>z</p>"
        );
    }

    /// Three rules `scripts/compare-toc.sh` cannot reach, because none of
    /// them can be written in the markdown this project reads: a class on
    /// a heading, a `Div`, and a `BlockQuote` around one. Each was probed
    /// against pandoc 3.8.2.1 with `--number-sections --wrap=none`.
    #[test]
    fn numbering_skips_unnumbered_and_blockquotes_but_not_divs() {
        let mut document = doc(vec![
            header(1, "one", &[], "One"),
            header(2, "two", &[], "Two"),
            // `pandoc -f markdown -t html --number-sections` on
            // `# Three {.unnumbered}` emits no number and keeps the class —
            // and the heading after it continues at 1.2, so an unnumbered
            // heading consumes nothing.
            header(1, "three", &["unnumbered"], "Three"),
            header(2, "four", &[], "Four"),
            Block::Div(Attr::default(), vec![header(2, "in-div", &[], "In a div")]),
            Block::BlockQuote(vec![header(2, "in-quote", &[], "In a quote")]),
        ]);
        number_sections(&mut document);
        let page = write_html(&document);
        assert!(page.contains(r#"<h1 data-number="1" id="one">"#), "{page}");
        assert!(page.contains(r#"<h2 data-number="1.1" id="two">"#), "{page}");
        assert!(page.contains(r#"<h1 class="unnumbered" id="three">"#), "{page}");
        assert!(page.contains(r#"<h2 data-number="1.2" id="four">"#), "{page}");
        assert!(page.contains(r#"<h2 data-number="1.3" id="in-div">"#), "{page}");
        assert!(page.contains(r#"<h2 id="in-quote">"#), "{page}");
        assert!(!page.contains(r#"id="in-quote" data"#), "{page}");
    }

    /// A heading inside a `BlockQuote` is not in the contents either, and a
    /// document with no heading gets **no** `<nav>` rather than an empty
    /// one — pandoc emits nothing at all.
    #[test]
    fn the_contents_hold_what_pandoc_puts_in_them() {
        assert_eq!(write_toc(&doc(vec![Block::Para(vec![Inline::Str("x".to_owned())])])), "");
        let quoted = doc(vec![Block::BlockQuote(vec![header(1, "q", &[], "Q")])]);
        assert_eq!(write_toc(&quoted), "");
        let nested = doc(vec![
            header(1, "one", &[], "One"),
            header(3, "three", &[], "Three"),
            header(2, "two", &[], "Two"),
        ]);
        // A jump from level 1 to level 3 opens exactly one list, so the
        // level-2 heading after it is a *sibling* of the level-3 one.
        assert_eq!(
            write_toc(&nested),
            concat!(
                "<nav id=\"TOC\" role=\"doc-toc\">\n",
                "<ul>\n",
                "<li><a href=\"#one\" id=\"toc-one\">One</a>\n",
                "<ul>\n",
                "<li><a href=\"#three\" id=\"toc-three\">Three</a></li>\n",
                "<li><a href=\"#two\" id=\"toc-two\">Two</a></li>\n",
                "</ul></li>\n",
                "</ul>\n",
                "</nav>\n",
            )
        );
        // A heading with no identifier gets no link: that is every heading
        // in a `-f commonmark` document, where identifiers are not read.
        let bare = doc(vec![header(1, "", &[], "Bare")]);
        assert!(bare_entry(&bare).contains("<li>Bare</li>"), "{}", bare_entry(&bare));
    }

    fn bare_entry(document: &Pandoc) -> String {
        write_toc(document)
    }

    /// The head and the reader are a matched pair: `-s` writes `title`,
    /// `author` and `lang`, and `read_html` reads exactly those back. No
    /// differential gate covers it — `diff-html-read` reads `corpus/*.html`
    /// and never this writer's own output — so it is asserted here.
    #[test]
    fn the_head_this_writer_produces_reads_back_as_the_metadata_it_came_from() {
        let mut document = doc(vec![Block::Para(vec![Inline::Str("body".to_owned())])]);
        document.meta.insert(
            "title".to_owned(),
            ferrodoc_ast::MetaValue::MetaString("A Title".to_owned()),
        );
        document.meta.insert(
            "author".to_owned(),
            ferrodoc_ast::MetaValue::MetaString("An Author".to_owned()),
        );
        document.meta.insert(
            "lang".to_owned(),
            ferrodoc_ast::MetaValue::MetaString("fr".to_owned()),
        );
        let page = page_of(&document);
        assert!(page.contains("<title>A Title</title>"), "{page}");
        assert!(page.contains(r#"<meta name="author" content="An Author" />"#), "{page}");
        assert!(page.contains(r#"lang="fr""#), "{page}");
        // Metadata the head has no place for stays out of it rather than
        // being invented as a `<meta name="…">`, which the reader would
        // then read back as a field the document never had.
        document.meta.insert(
            "custom".to_owned(),
            ferrodoc_ast::MetaValue::MetaString("value".to_owned()),
        );
        let page = page_of(&document);
        assert!(!page.contains("custom"), "{page}");

        let back = read_html(&page).expect("the page this crate wrote is readable");
        for key in ["title", "author", "lang"] {
            assert!(back.meta.contains_key(key), "{key} did not survive: {:?}", back.meta);
        }
    }

    /// Pandoc keeps a table's alignment in the **column specs**, and
    /// leaves every cell `AlignDefault`. Reading only the cell therefore
    /// dropped the alignment of every table that had one — and no gate
    /// could see it, because `diff-html` runs on the `CommonMark` spec and
    /// `CommonMark` has no tables.
    #[test]
    fn a_column_alignment_reaches_the_cells_it_governs() {
        let table = write_html(
            &ferrodoc_markdown::read_gfm("| L | C | R |\n|:--|:-:|--:|\n| a | b | c |\n")
                .expect("convertible"),
        );
        assert_eq!(table.matches("text-align: left;").count(), 2, "{table}");
        assert_eq!(table.matches("text-align: center;").count(), 2, "{table}");
        assert_eq!(table.matches("text-align: right;").count(), 2, "{table}");
        // A column with no alignment stays bare rather than gaining one.
        let plain = write_html(
            &ferrodoc_markdown::read_gfm("| a |\n|---|\n| b |\n").expect("convertible"),
        );
        assert!(!plain.contains("text-align"), "{plain}");
    }

    /// A word processor sets column widths and the DOCX reader keeps
    /// them exactly; the writer threw them away, so every converted table
    /// came out equal-width. Same blind spot as the alignment above.
    #[test]
    fn column_widths_survive_into_the_colgroup() {
        use ferrodoc_ast::{ColSpec, ColWidth};
        let mut doc = ferrodoc_markdown::read_gfm("| a | b |\n|---|---|\n| 1 | 2 |\n")
            .expect("convertible");
        let Some(Block::Table(table)) = doc.blocks.first_mut() else { panic!("a table") };
        table.colspecs = vec![
            ColSpec { alignment: Alignment::AlignDefault, width: ColWidth::ColWidth(0.335) },
            ColSpec { alignment: Alignment::AlignDefault, width: ColWidth::ColWidth(0.335) },
        ];
        let html = write_html(&doc);
        // The column truncates and the table rounds — pandoc's own
        // arithmetic, and 33/67 rather than 34/67 or 33/66.
        assert!(html.contains("<table style=\"width:67%;\">"), "{html}");
        assert_eq!(html.matches("<col style=\"width: 33%\" />").count(), 2, "{html}");
        // Columns that add up to the whole width name no table width.
        let Some(Block::Table(table)) = doc.blocks.first_mut() else { panic!("a table") };
        table.colspecs = vec![
            ColSpec { alignment: Alignment::AlignDefault, width: ColWidth::ColWidth(0.5) },
            ColSpec { alignment: Alignment::AlignDefault, width: ColWidth::ColWidth(0.5) },
        ];
        let full = write_html(&doc);
        assert!(full.contains("<table>"), "{full}");
        assert_eq!(full.matches("<col style=\"width: 50%\" />").count(), 2, "{full}");
        // A table with no stated widths gains no colgroup at all.
        let plain = write_html(
            &ferrodoc_markdown::read_gfm("| a |\n|---|\n| b |\n").expect("convertible"),
        );
        assert!(!plain.contains("colgroup"), "{plain}");
    }

    #[test]
    fn a_standalone_page_frames_the_fragment() {
        use ferrodoc_ast::MetaValue;
        let mut doc = ferrodoc_markdown::read_commonmark("text\n").expect("convertible");
        doc.meta.insert(
            "title".to_owned(),
            MetaValue::MetaInlines(vec![Inline::Str("My <Doc>".to_owned())]),
        );
        doc.meta.insert("lang".to_owned(), MetaValue::MetaString("fr".to_owned()));
        doc.meta.insert(
            "author".to_owned(),
            MetaValue::MetaList(vec![
                MetaValue::MetaString("Ada".to_owned()),
                MetaValue::MetaInlines(vec![Inline::Str("Grace".to_owned())]),
            ]),
        );
        let mut options = Page::new();
        options.css = vec!["theme.css".to_owned()];
        let page = write_page(&doc, &options).expect("rendered");

        // The body is the fragment, unchanged: one writer, two framings.
        assert!(page.contains(write_html(&doc).trim_end_matches('\n')), "{page}");
        assert!(page.starts_with("<!DOCTYPE html>\n"), "{page}");
        assert!(page.contains(r#"lang="fr""#), "{page}");
        assert!(page.contains("<meta charset=\"utf-8\" />"), "{page}");
        // Metadata is text, and text in a document can contain markup.
        assert!(page.contains("<title>My &lt;Doc&gt;</title>"), "{page}");
        // A field may be one value or a list, however it was spelled.
        assert!(page.contains("content=\"Ada\""), "{page}");
        assert!(page.contains("content=\"Grace\""), "{page}");
        // `--css` links a stylesheet; it does not inline the file, which
        // is what pandoc means by the flag.
        assert!(page.contains(r#"<link rel="stylesheet" href="theme.css" />"#), "{page}");
    }

    #[test]
    fn a_page_without_metadata_is_still_a_page() {
        let doc = ferrodoc_markdown::read_commonmark("text\n").expect("convertible");
        let page = page_of(&doc);
        // `lang` is empty rather than absent, which is pandoc's — and
        // the title element is required, so a page with no title still
        // has an empty one.
        assert!(page.starts_with("<!DOCTYPE html>\n"), "{page}");
        assert!(page.contains(r#"lang="""#), "{page}");
        assert!(page.contains("<title></title>"), "{page}");
        assert!(!page.contains("<meta name=\"author\""), "{page}");
    }
    #[test]
    fn paragraph_and_emphasis() {
        assert_eq!(html("a *b* **c**\n"), "<p>a <em>b</em> <strong>c</strong></p>\n");
    }

    #[test]
    fn tight_and_loose_lists() {
        assert_eq!(
            html("- a\n- b\n"),
            "<ul>\n<li>a</li>\n<li>b</li>\n</ul>\n"
        );
        assert_eq!(
            html("- a\n\n- b\n"),
            "<ul>\n<li><p>a</p></li>\n<li><p>b</p></li>\n</ul>\n"
        );
    }

    /// No round trip can see any of this: `- ☒ a` and `- [x] a` are one AST,
    /// so the boxes have to be checked against the literal bytes pandoc
    /// 3.8.2.1 writes for the same AST (`pandoc -f json -t html --wrap=none`).
    #[test]
    fn task_list_boxes_become_checkbox_inputs() {
        fn boxes(blocks: Vec<Block>) -> String {
            write_html(&Pandoc::new(blocks))
        }
        fn task(marker: &str, text: &str) -> Vec<Block> {
            vec![Block::Plain(vec![
                Inline::Str(marker.to_owned()),
                Inline::Space,
                Inline::Str(text.to_owned()),
            ])]
        }
        let plain = || vec![Block::Plain(vec![Inline::Str("plain".to_owned())])];

        // Every item a task item: the list itself is classed.
        assert_eq!(
            boxes(vec![Block::BulletList(vec![task("☒", "Done"), task("☐", "Todo")])]),
            "<ul class=\"task-list\">\n\
             <li><label><input type=\"checkbox\" checked=\"\" />Done</label></li>\n\
             <li><label><input type=\"checkbox\" />Todo</label></li>\n\
             </ul>\n"
        );
        // One ordinary item and the class goes, but the boxes stay.
        assert_eq!(
            boxes(vec![Block::BulletList(vec![task("☒", "Done"), plain()])]),
            "<ul>\n\
             <li><label><input type=\"checkbox\" checked=\"\" />Done</label></li>\n\
             <li>plain</li>\n\
             </ul>\n"
        );
        // An ordered list never takes the class, and still gets the boxes.
        assert_eq!(
            boxes(vec![Block::OrderedList(
                ferrodoc_ast::ListAttributes {
                    start: 1,
                    style: ListNumberStyle::Decimal,
                    delim: ferrodoc_ast::ListNumberDelim::Period,
                },
                vec![task("☒", "Done"), plain()],
            )]),
            "<ol type=\"1\">\n\
             <li><label><input type=\"checkbox\" checked=\"\" />Done</label></li>\n\
             <li>plain</li>\n\
             </ol>\n"
        );
        // A loose item boxes inside its `<p>`, and its later blocks stand.
        let loose = |marker: &str, text: &str| {
            vec![Block::Para(vec![
                Inline::Str(marker.to_owned()),
                Inline::Space,
                Inline::Str(text.to_owned()),
            ])]
        };
        let mut first = loose("☒", "a");
        first.push(Block::Para(vec![Inline::Str("x".to_owned())]));
        assert_eq!(
            boxes(vec![Block::BulletList(vec![first, loose("☐", "b")])]),
            "<ul class=\"task-list\">\n\
             <li><p><label><input type=\"checkbox\" checked=\"\" />a</label></p>\n\
             <p>x</p></li>\n\
             <li><p><label><input type=\"checkbox\" />b</label></p></li>\n\
             </ul>\n"
        );
    }

    /// The rule is narrower than "an item starting with a box": pandoc wants
    /// a `Str` holding the box alone and a `Space` right after it, at the
    /// head of the item's first `Plain`/`Para`. Everything else is text.
    #[test]
    fn a_box_pandoc_would_leave_alone_stays_text() {
        fn one(inlines: Vec<Inline>) -> String {
            write_html(&Pandoc::new(vec![Block::BulletList(vec![vec![Block::Plain(inlines)]])]))
        }
        let ticked = || Inline::Str("☒".to_owned());
        // No space after the box.
        assert_eq!(one(vec![ticked(), Inline::Str("no space".to_owned())]), "<ul>\n<li>☒no space</li>\n</ul>\n");
        // Nothing after the box at all.
        assert_eq!(one(vec![ticked()]), "<ul>\n<li>☒</li>\n</ul>\n");
        // A box inside an `Emph` is not at the head of the item.
        assert_eq!(
            one(vec![Inline::Emph(vec![ticked()]), Inline::Space, Inline::Str("emph".to_owned())]),
            "<ul>\n<li><em>☒</em> emph</li>\n</ul>\n"
        );
        // A `SoftBreak` is not the `Space` the rule asks for.
        assert_eq!(
            one(vec![ticked(), Inline::SoftBreak, Inline::Str("soft".to_owned())]),
            "<ul>\n<li>☒ soft</li>\n</ul>\n"
        );
        // The box and the space have to be separate inlines.
        assert_eq!(one(vec![Inline::Str("☒ one str".to_owned())]), "<ul>\n<li>☒ one str</li>\n</ul>\n");
        // An item that is only a box and a space is a task item with an
        // empty label, and a `<blockquote>` around the box is not one.
        assert_eq!(
            one(vec![ticked(), Inline::Space]),
            "<ul class=\"task-list\">\n<li><label><input type=\"checkbox\" checked=\"\" /></label></li>\n</ul>\n"
        );
        assert_eq!(
            write_html(&Pandoc::new(vec![Block::BulletList(vec![vec![Block::Plain(vec![
                Inline::Str("outer".to_owned()),
            ]), Block::BulletList(vec![vec![Block::Plain(vec![
                ticked(),
                Inline::Space,
                Inline::Str("i".to_owned()),
            ])]])]])])),
            "<ul>\n<li>outer\n<ul class=\"task-list\">\n\
             <li><label><input type=\"checkbox\" checked=\"\" />i</label></li>\n\
             </ul></li>\n</ul>\n"
        );
    }

    /// A language this highlighter does not know degrades to exactly what
    /// the writer emitted before there was a highlighter.
    ///
    /// **This test named `rust` until rust was implemented**, and then
    /// asserted the opposite of the truth. A test whose subject is "a
    /// language we do not support" goes stale the moment one is added, so
    /// it names a language nobody here intends to write: if `haskell`
    /// ever ships, this needs another, not a new expectation.
    #[test]
    fn an_unknown_language_is_not_highlighted() {
        assert_eq!(
            html("```haskell\nmain = pure ()\n```\n"),
            "<pre class=\"haskell\"><code>main = pure ()</code></pre>\n"
        );
    }

    /// And a language it does know is highlighted, in pandoc's wrapper.
    #[test]
    fn a_known_language_is_highlighted() {
        assert_eq!(
            html("```rust\nfn x() {}\n```\n"),
            "<div class=\"sourceCode\" id=\"cb1\"><pre class=\"sourceCode rust\">\
             <code class=\"sourceCode rust\"><span id=\"cb1-1\">\
             <a href=\"#cb1-1\" aria-hidden=\"true\" tabindex=\"-1\"></a>\
             <span class=\"kw\">fn</span> x() <span class=\"op\">{}</span>\
             </span></code></pre></div>\n"
        );
    }

    #[test]
    fn hard_break_in_alt_text_becomes_space() {
        assert_eq!(
            html("![a hard\\\nbreak](x.png)\n"),
            "<p><img src=\"x.png\" alt=\"a hard break\" /></p>\n"
        );
    }

    #[test]
    fn link_title_and_image_alt() {
        assert_eq!(
            html("[l](u \"t\") ![*em* alt](i.png)\n"),
            "<p><a href=\"u\" title=\"t\">l</a> <img src=\"i.png\" alt=\"em alt\" /></p>\n"
        );
    }
}
