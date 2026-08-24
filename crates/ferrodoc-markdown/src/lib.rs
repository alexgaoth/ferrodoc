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

pub use write::{write_gfm, write_gfm_wrapped, write_markdown, write_markdown_wrapped};

use comrak::nodes::{AstNode, ListDelimType, ListType, NodeValue, TableAlignment};
use comrak::{Arena, Options, parse_document};
use ferrodoc_ast::{
    Alignment, Attr, Block, Caption, Cell, ColSpec, ColWidth, Format, Inline, MathType, ListAttributes,
    ListNumberDelim, ListNumberStyle, Meta, MetaValue, Pandoc, Row, Table, TableBody, TableFoot,
    TableHead, Target,
};
use std::borrow::Cow;
use std::collections::HashMap;

/// A document this reader will not convert.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// Containers nested deeper than [`MAX_NESTING`]. Converting such a
    /// document would recurse until the stack overflows, which aborts the
    /// process, so it is refused instead — and refused loudly, rather than
    /// returned truncated.
    TooDeeplyNested,
    /// A YAML metadata block using something this reader does not read.
    /// The line is quoted, because the whole point is that the document
    /// is refused loudly rather than converted into a plausible wrong
    /// answer — which is what `CommonMark` does with a metadata block.
    Metadata(String),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::TooDeeplyNested => write!(
                f,
                "document nests containers {MAX_NESTING} or more levels deep"
            ),
            Error::Metadata(line) => write!(
                f,
                "metadata block: {line} is outside the YAML subset this reads \
                 (`key: value`, `key:` with `- item` lines, `#` comments)"
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
    read(input, Dialect::Commonmark)
}

/// Parse a `GitHub Flavored Markdown` document into a [`Pandoc`] AST.
///
/// On top of `CommonMark` this recognizes the five extensions the GFM
/// specification defines — pipe tables, task list items, strikethrough and
/// extended autolinks (the fifth, tag filtering, is off because pandoc's
/// `gfm` does not apply it either) — plus the heading identifiers pandoc's
/// `gfm_auto_identifiers` derives.
///
/// Pandoc's own `gfm` bundles further *pandoc* extensions that the GFM
/// specification does not define. `$math$` and **footnotes** are read,
/// because pandoc's `gfm` reads both and a document carrying either is
/// wrong without it — a Jupyter cell is mostly equations, and an unread
/// `[^1]` becomes literal text. Emoji shortcodes, alerts and YAML
/// metadata blocks are not. See `COMPATIBILITY.md`.
///
/// # Errors
///
/// Returns [`Error::TooDeeplyNested`] for pathologically nested input.
pub fn read_gfm(input: &str) -> Result<Pandoc, Error> {
    read(input, Dialect::Gfm)
}

/// Read **pandoc's** markdown rather than `CommonMark`: a YAML metadata
/// block, header attributes, definition lists and `H~2~O`/`E=mc^2^`, on
/// top of what `gfm` reads.
///
/// It is a separate format name rather than a change to
/// [`read_commonmark`], because the two dialects disagree on documents
/// that are valid in both and a silent change of meaning is worse than a
/// flag someone has to type. What is **not** read is in the crate's
/// `CLAUDE.md` and in `COMPATIBILITY.md`, with the probe behind each.
///
/// # Errors
///
/// [`Error::TooDeeplyNested`] for pathologically nested input, and
/// [`Error::Metadata`] for a YAML block outside the subset this reads —
/// which is an error rather than a guess, because a metadata block read
/// wrongly is a document that converts to something plausible and wrong.
pub fn read_pandoc_markdown(input: &str) -> Result<Pandoc, Error> {
    read(input, Dialect::Pandoc)
}

/// Which markdown. The three disagree on real documents, so the reader is
/// told which one it is reading rather than guessing from the content.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Dialect {
    Commonmark,
    Gfm,
    Pandoc,
}

impl Dialect {
    /// Everything `gfm` reads, `pandoc_markdown` reads too.
    fn is_extended(self) -> bool {
        matches!(self, Dialect::Gfm | Dialect::Pandoc)
    }
}

/// The comrak options one dialect is read with. Shared so a fragment —
/// the markdown between two raw HTML tags — is read exactly as the
/// document around it was.
fn options_for(dialect: Dialect) -> Options<'static> {
let mut options = Options::default();
if dialect.is_extended() {
    options.extension.table = true;
    options.extension.strikethrough = true;
    options.extension.tasklist = true;
    // Probed: pandoc's `gfm` reader has `tex_math_dollars` on, so
    // `$x$` is `Math InlineMath` and `$$x$$` is `Math DisplayMath`
    // there — while `-f commonmark` leaves both as literal `Str`.
    // `math_code` stays off: pandoc does not read `` `$x$` `` as math.
    options.extension.math_dollars = true;
    // Probed: `pandoc -f gfm` reads `[^1]` as `Note`, and GitHub renders
    // it. `-f commonmark` does not, which is why this is inside the gfm
    // branch — `diff-spec` and `diff-md` would drop if it leaked out.
    options.extension.footnotes = true;
}
// Probed: `pandoc -f gfm` links a bare `example.com`; `pandoc -f
// markdown` does not, so this one belongs to gfm alone.
options.extension.autolink = dialect == Dialect::Gfm;
if dialect == Dialect::Pandoc {
    // Each of the four is a construct `-f markdown` reads and
    // `-f commonmark` does not; each is probed in the crate's
    // `CLAUDE.md` against `pandoc -f markdown -t json`.
    options.extension.header_attributes = true;
    options.extension.description_lists = true;
    options.extension.superscript = true;
    options.extension.subscript = true;
    options.extension.front_matter_delimiter = Some("---".to_owned());
    options.extension.link_attributes = true;
    options.extension.inline_code_attributes = true;
    options.extension.fenced_code_attributes = true;
    options.extension.inline_footnotes = true;
    // `smart` is pandoc's `markdown` and nobody else's: probed, `-f
    // gfm` and `-f commonmark` both leave `it's` as it stands. It
    // turns `--` into an en dash, `---` into an em dash, `...` into
    // an ellipsis, and the quotes into curly ones — and a *pair* of
    // those becomes a `Quoted` element, which `quoted` does.
    options.parse.smart = true;
}
    options
}

fn read(input: &str, dialect: Dialect) -> Result<Pandoc, Error> {
    let mut prepared: String = preprocess(input).into_owned();
    // comrak does not recognise an **empty** front-matter block, so
    // `---\n---` reaches the parser as two thematic breaks where pandoc
    // reads an empty metadata block and emits nothing.
    if dialect == Dialect::Pandoc {
        for opening in ["---\n---\n", "---\r\n---\r\n"] {
            if let Some(rest) = prepared.strip_prefix(opening) {
                prepared = rest.trim_start_matches(['\n', '\r']).to_owned();
                break;
            }
        }
    }
    let src = Src::new(&prepared);
    let arena = Arena::new();
    let options = options_for(dialect);
    let root = parse_document(&arena, &prepared, &options);
    // Check the depth once, without recursing, and leave the conversion
    // itself exactly as shallow-document-shaped as it was: threading a
    // depth counter through every call measured ~8% slower.
    if tree_depth(root) > MAX_NESTING {
        return Err(Error::TooDeeplyNested);
    }
    // Definitions first: a reference is converted by cloning the body, so
    // the bodies have to exist before the block walk reaches the reference.
    let defs = if dialect.is_extended() { footnotes(root, &src, dialect) } else { Notes::new() };
    let mut blocks = blocks(root.children(), &src, false, &defs, dialect);
    if dialect.is_extended() {
        Identifiers { seen: HashMap::new(), dialect }.assign(&mut blocks);
    }
    let mut document = Pandoc::new(blocks);
    if dialect == Dialect::Pandoc {
        document.meta = front_matter(root)?;
    }
    Ok(document)
}

/// `implicit_figures`: a paragraph that is nothing but one image with
/// alt text is a `Figure`, and the alt text is its caption.
///
/// Measured on `pandoc -f markdown -t json`, five shapes. The image
/// **keeps its classes and its attributes** and gives up only its
/// identifier, which moves to the figure; an image with **empty** alt is
/// left a paragraph however it is written; and an image beside anything
/// else — a word, a second image, even emphasis around it — is not one.
/// It happens where a paragraph is built, which is why a table cell is
/// not affected and a tight list item is: the cell never goes through a
/// paragraph, and the item's has already become a figure by the time the
/// list is tightened.
fn implicit_figure(attr: &Attr, alt: &[Inline], target: &Target) -> Block {
    let figure = Attr { identifier: attr.identifier.clone(), ..Attr::default() };
    let image = Inline::Image(
        Box::new(Attr { identifier: String::new(), ..attr.clone() }),
        alt.to_vec(),
        Box::new(target.clone()),
    );
    Block::Figure(
        figure,
        Caption { short: None, blocks: vec![Block::Plain(alt.to_vec())] },
        vec![Block::Plain(vec![image])],
    )
}

/// One raw HTML run's literal, with the newline comrak drops.
///
/// An unclosed type-1..5 HTML block (outside blockquotes) gains one bonus
/// newline when only blank lines separate it from EOF. Comrak's literal
/// already contains the block's trailing blank lines; its first line is
/// the node's start line.
fn html_literal(
    hb: &comrak::nodes::NodeHtmlBlock,
    data: &comrak::nodes::Ast,
    src: &Src,
    in_quote: bool,
) -> String {
    let mut literal = hb.literal.clone();
    if (1..=5).contains(&hb.block_type)
        && !contains_closer(&literal, hb.block_type)
        && !in_quote
        && src.only_blanks_after(data.sourcepos.start.line + literal_lines(&literal) - 1)
    {
        literal.push('\n');
    }
    literal
}

/// The tags `pandoc -f markdown` treats as **block-level**: each one is
/// a `RawBlock` by itself and what lies between two of them is read as
/// markdown. Measured tag by tag against `pandoc -f markdown -t json`,
/// and the list is not `CommonMark`'s in either direction: `<embed>`,
/// `<meta>`, `<title>`, `<track>` and `<source>` are block-level here
/// while `<a>`, `<img>`, `<input>`, `<label>` and `<span>` are not.
///
/// **Thirty-nine of them are not HTML at all.** Pandoc also knows
/// `DocBook`'s block elements, so `<warning>`, `<note>`, `<tip>`,
/// `<programlisting>` and `<itemizedlist>` open a block while `<danger>`
/// and `<foo>` do not — which is measured, not guessable, and is what
/// the spec's `<Warning>` example turns on. Sorted, and searched as
/// such.
static BLOCK_TAGS: &[&str] = &[
    "address", "area", "article", "aside", "audio", "bibliolist",
    "blockquote", "body", "button", "calloutlist", "canvas", "caption",
    "case", "caution", "center", "classsynopsis", "cmdsynopsis", "col",
    "colgroup", "dd", "default", "del", "details", "dir", "div", "dl",
    "dt", "embed", "epigraph", "equation", "example", "fieldset",
    "figcaption", "figure", "footer", "form", "formalpara", "frameset",
    "funcsynopsis", "glosslist", "h1", "h2", "h3", "h4", "h5", "h6",
    "head", "header", "hgroup", "hr", "html", "iframe", "important",
    "informalequation", "informalexample", "informalfigure",
    "informaltable", "ins", "isindex", "itemizedlist", "li",
    "literallayout", "main", "map", "mediaobject", "menu", "meta",
    "msgset", "nav", "noframes", "noscript", "note", "object", "ol",
    "orderedlist", "output", "p", "para", "procedure", "programlisting",
    "progress", "qandaset", "screen", "section", "segmentedlist",
    "sidebar", "simpara", "simplelist", "source", "summary", "switch",
    "synopsis", "table", "task", "tbody", "td", "tfoot", "th", "thead",
    "tip", "title", "tr", "track", "ul", "variablelist", "video",
    "warning",
];


/// The four whose content is **not** markdown: everything through the
/// matching close tag is one `RawBlock`, verbatim.
static VERBATIM_TAGS: &[&str] = &["pre", "script", "style", "textarea"];

fn is_block_tag(name: &str) -> bool {
    BLOCK_TAGS.binary_search(&name).is_ok()
}

fn is_verbatim_tag(name: &str) -> bool {
    VERBATIM_TAGS.contains(&name)
}

/// One `<…>` found in a raw HTML run.
struct RawTag {
    /// Byte index just past the `>`.
    end: usize,
    /// The element name, lowercased. Empty for a comment, a processing
    /// instruction, a declaration or a CDATA section — none of which
    /// holds markdown, and each of which pandoc keeps raw.
    name: String,
    closing: bool,
}

/// The next `<…>` at or after `from`. A `<` that starts nothing this
/// recognises is skipped rather than guessed at.
fn next_tag(text: &str, from: usize) -> Option<(usize, RawTag)> {
    let mut at = from;
    while let Some(offset) = text.get(at..)?.find('<') {
        let start = at + offset;
        let rest = &text[start..];
        // A comment and a processing instruction are raw; a
        // declaration and a CDATA section are **not**, and pandoc reads
        // `<!DOCTYPE html>` and `<![CDATA[ … ]]>` as the literal text
        // they are. Measured, and the opposite of what `CommonMark`
        // does with them.
        for (open, close) in [("<!--", "-->"), ("<?", "?>")] {
            if let Some(body) = rest.strip_prefix(open) {
                let end = body
                    .find(close)
                    .map_or(text.len(), |index| start + open.len() + index + close.len());
                return Some((start, RawTag { end, name: String::new(), closing: false }));
            }
        }
        let closing = rest.starts_with("</");
        let name_at = start + if closing { 2 } else { 1 };
        let name_len = text[name_at..]
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric() || *c == '-')
            .count();
        let starts_a_name = text[name_at..].starts_with(|c: char| c.is_ascii_alphabetic());
        if name_len > 0 && starts_a_name {
            // Scan to the `>`, and step over a quoted attribute value so
            // a `>` inside one does not end the tag early.
            let mut quote = None;
            let mut end = None;
            for (index, ch) in text[name_at + name_len..].char_indices() {
                match (quote, ch) {
                    (Some(open), _) if ch == open => quote = None,
                    (None, '"' | '\'') => quote = Some(ch),
                    (None, '>') => {
                        end = Some(name_at + name_len + index + 1);
                        break;
                    }
                    _ => {}
                }
            }
            if let Some(end) = end {
                let name = text[name_at..name_at + name_len].to_ascii_lowercase();
                return Some((start, RawTag { end, name, closing }));
            }
        }
        at = start + 1;
    }
    None
}

/// Whether this run holds anything pandoc would break the block at.
fn breaks_the_block(tag: &RawTag) -> bool {
    tag.name.is_empty() || is_block_tag(&tag.name) || is_verbatim_tag(&tag.name)
}

fn has_block_tag(literal: &str) -> bool {
    let mut at = 0;
    while let Some((start, tag)) = next_tag(literal, at) {
        if breaks_the_block(&tag) {
            return true;
        }
        at = tag.end.max(start + 1);
    }
    false
}

/// The index just past `</name>`, searching from `from`.
fn closing_tag(text: &str, from: usize, name: &str) -> Option<usize> {
    let mut at = from;
    while let Some((start, tag)) = next_tag(text, at) {
        if tag.closing && tag.name == name {
            return Some(tag.end);
        }
        at = tag.end.max(start + 1);
    }
    None
}

/// A run of raw HTML, as pandoc's markdown reads it: **one `RawBlock`
/// per block-level tag**, with everything between two of them read as
/// markdown. `CommonMark` keeps the whole run as a single opaque chunk,
/// and 43 of the spec's 44 HTML-block examples differ for that alone.
///
/// The tags come out on their own because that is what pandoc's tree
/// holds — `<table>`, `<tr>`, `<td>` are three `RawBlock`s with the cell
/// text as a `Plain` between them — so there is no other shape to write.
fn html_run(literal: &str, defs: &Notes, dialect: Dialect) -> Vec<Block> {
    // Nothing block-level in it: the run is a paragraph, not a block.
    // This is also what stops the recursion — every chunk below is cut
    // at block tags, so it can only come back through here.
    if !has_block_tag(literal) {
        return inline_html(literal, defs, dialect);
    }
    let mut out = Vec::new();
    let mut chunk_from = 0;
    let mut at = 0;
    while let Some((start, tag)) = next_tag(literal, at) {
        if !breaks_the_block(&tag) {
            at = tag.end.max(start + 1);
            continue;
        }
        // `<pre>`, `<script>`, `<style>` and `<textarea>` hold no
        // markdown: the whole element is one raw block.
        let span_end = if is_verbatim_tag(&tag.name) && !tag.closing {
            closing_tag(literal, tag.end, &tag.name).unwrap_or(literal.len())
        } else {
            tag.end
        };
        // A `</div>` is not a raw tag on the next line: `native_divs`
        // takes it, and the markdown before it is read as a document of
        // its own — which is why `<div>\na\n</div>` ends in a `Para`
        // and `<td>\na\n</td>` in a `Plain`. Measured both ways.
        let absorbed = tag.closing && tag.name == "div";
        push_chunk(&literal[chunk_from..start], absorbed, &mut out, defs, dialect);
        out.push(Block::RawBlock(
            Format("html".to_owned()),
            literal[start..span_end].trim_end_matches('\n').to_owned(),
        ));
        at = span_end;
        chunk_from = span_end;
    }
    push_chunk(&literal[chunk_from..], true, &mut out, defs, dialect);
    out
}

/// The markdown between two raw tags.
///
/// Its last paragraph is a `Plain` unless a blank line closes it, which
/// is `paragraph`'s rule read against the chunk's own text rather than
/// the document's. Two ways it can be closed, both measured:
///
/// * the chunk ends with a blank line — `<p>\n\na\n\n</p>` keeps its
///   `Para` where `<p>\na\n\nb\n</p>` ends in a `Plain`;
/// * or the tag after it is a `</div>`, which `native_divs` takes rather
///   than leaving on the next line, so the chunk is read as a document
///   of its own: `<div>\nx\n</div>` is a `Para` and `<td>\nx\n</td>` a
///   `Plain`.
///
/// A chunk that does not end with a newline at all had the tag on its
/// own line, and is a `Plain` either way: `<div>x</div>`.
fn push_chunk(text: &str, absorbed: bool, out: &mut Vec<Block>, defs: &Notes, dialect: Dialect) {
    if text.trim().is_empty() {
        return;
    }
    let closed = text.ends_with('\n') && (absorbed || text.ends_with("\n\n"));
    let mut parsed = fragment(&dedented(text), defs, dialect);
    if !closed
        && let Some(last) = parsed.last_mut()
        && let Block::Para(inlines) = last
    {
        *last = Block::Plain(std::mem::take(inlines));
    }
    out.append(&mut parsed);
}

/// A chunk whose first line of content is indented four columns or more
/// and has no blank line in front of it.
///
/// Pandoc has no indented code block there — its `codeBlockIndented`
/// wants a blank line first — while a fragment read on its own starts at
/// column one and makes one. So the chunk is moved back to the margin,
/// which keeps every relative indent inside it and is the difference
/// between reading `<td>\n    hi\n</td>` as a cell holding `hi` and as a
/// cell holding a code block.
fn dedented(text: &str) -> Cow<'_, str> {
    // The newline that ended the tag's own line is not a blank line.
    let body = text.strip_prefix('\n').unwrap_or(text);
    let first = body.split('\n').next().unwrap_or_default();
    let indent = first.len() - first.trim_start_matches(' ').len();
    if indent < 4 || first.trim().is_empty() {
        return Cow::Borrowed(text);
    }
    let mut out = String::with_capacity(text.len());
    for (index, line) in text.split('\n').enumerate() {
        if index > 0 {
            out.push('\n');
        }
        let keep = line.len() - line.trim_start_matches(' ').len();
        out.push_str(&line[keep.min(indent)..]);
    }
    Cow::Owned(out)
}

/// Read a fragment with the dialect the document around it was read
/// with. Its own `Src`, because its line numbers start again at one.
fn fragment(text: &str, defs: &Notes, dialect: Dialect) -> Vec<Block> {
    let arena = Arena::new();
    let root = parse_document(&arena, text, &options_for(dialect));
    blocks(root.children(), &Src::new(text), false, defs, dialect)
}

/// A run with no block-level tag in it is a paragraph, not a block:
/// pandoc reads `<foo>\nbar\n</foo>` as inlines with the tags raw, and
/// the markdown between them **is** read — `*bar*` comes out an `Emph`.
///
/// Getting those inlines uses `CommonMark`'s own rule rather than a
/// second parser: an HTML block only ever begins at the **start of a
/// line**, so a word and a space in front of the run make every tag in
/// it inline. The word is taken off again.
///
/// This reads inlines directly rather than going back through `blocks`,
/// and that is load-bearing: `CommonMark`'s block-tag list is not
/// pandoc's — `<link>`, `<option>`, `<param>` and eight more open a
/// block there and are inline here — so a run of one of those came back
/// as a block and asked to be read as inlines again, forever. The
/// measured shape of that bug is a stack overflow.
fn inline_html(literal: &str, defs: &Notes, dialect: Dialect) -> Vec<Block> {
    let raw = || vec![Block::RawBlock(Format("html".to_owned()), literal.to_owned())];
    let source = format!("x {}", literal.trim_start());
    let arena = Arena::new();
    let root = parse_document(&arena, &source, &options_for(dialect));
    let Some(node) = root.first_child() else { return raw() };
    if node.next_sibling().is_some()
        || !matches!(node.data.borrow().value, NodeValue::Paragraph)
    {
        return raw();
    }
    let read = inlines(node.children(), defs, dialect);
    let [Inline::Str(word), Inline::Space | Inline::SoftBreak, rest @ ..] = read.as_slice()
    else {
        return raw();
    };
    if word != "x" {
        return raw();
    }
    vec![Block::Para(rest.to_vec())]
}

/// The tag a raw inline holds, if it is one this reader knows.
fn raw_tag_name(token: &Inline) -> Option<String> {
    let Inline::RawInline(format, raw) = token else { return None };
    if format.0 != "html" {
        return None;
    }
    let (start, tag) = next_tag(raw, 0)?;
    (start == 0 && tag.end == raw.len()).then_some(tag.name)
}

/// A paragraph that **begins** with a block-level tag is not a
/// paragraph: pandoc reads `<del>*foo*</del>` as a raw block, a `Plain`
/// and a raw block, the same shape a whole line of HTML gets. Only at
/// the start — `x <del>a</del>` stays one paragraph — and what follows
/// the last tag opens a new one.
///
/// This is the case comrak never hands over as an HTML block, because
/// `CommonMark` starts one only where a tag stands alone on its line.
fn split_leading_html(block: Block, out: &mut Vec<Block>) {
    let (Block::Para(inlines) | Block::Plain(inlines)) = &block else {
        out.push(block);
        return;
    };
    let leads = inlines.first().and_then(raw_tag_name).is_some_and(|n| is_block_tag(&n));
    if !leads {
        out.push(block);
        return;
    }
    let trailing_is_para = matches!(block, Block::Para(_));
    let (Block::Para(inlines) | Block::Plain(inlines)) = block else { unreachable!() };
    let mut run: Vec<Inline> = Vec::new();
    let mut seen_tag = false;
    for token in inlines {
        let block_tag = raw_tag_name(&token).is_some_and(|name| is_block_tag(&name));
        if !block_tag {
            run.push(token);
            continue;
        }
        if !run.is_empty() {
            out.push(Block::Plain(std::mem::take(&mut run)));
        }
        seen_tag = true;
        let Inline::RawInline(format, raw) = token else { unreachable!() };
        out.push(Block::RawBlock(*format, raw));
    }
    // What follows the last tag is a paragraph of its own, and the space
    // that separated it from the tag is not part of it.
    while matches!(run.first(), Some(Inline::Space | Inline::SoftBreak)) {
        run.remove(0);
    }
    if !run.is_empty() {
        out.push(if seen_tag && trailing_is_para { Block::Para(run) } else { Block::Plain(run) });
    }
}

/// Pandoc's attribute list, `{#id .class key=value}`, as written in a
/// bracketed span or after a heading.
///
/// `None` when any item is malformed, because pandoc then reads the
/// whole construct as the literal text it is: `[t]{foo}` is not a span,
/// it is `[t]{foo}`. The last `#id` wins, classes accumulate, and a
/// value may be quoted so it can hold a space.
fn pandoc_attr(text: &str) -> Option<Attr> {
    let mut attr = Attr::default();
    let mut rest = text.trim();
    while !rest.is_empty() {
        if let Some(after) = rest.strip_prefix('#') {
            let end = after.find(char::is_whitespace).unwrap_or(after.len());
            if end == 0 {
                return None;
            }
            after[..end].clone_into(&mut attr.identifier);
            rest = after[end..].trim_start();
        } else if let Some(after) = rest.strip_prefix('.') {
            let end = after.find(char::is_whitespace).unwrap_or(after.len());
            if end == 0 {
                return None;
            }
            attr.classes.push(after[..end].to_owned());
            rest = after[end..].trim_start();
        } else {
            let name_end = rest.find(['=', ' '])?;
            if !rest[name_end..].starts_with('=') || name_end == 0 {
                return None;
            }
            let after = &rest[name_end + 1..];
            // `smart` runs in the parser, so the quotes around a value
            // are curly by the time this sees them.
            let quotes = [('"', '"'), ('\'', '\''), ('\u{201c}', '\u{201d}'), ('\u{2018}', '\u{2019}')];
            let opener = after.chars().next().and_then(|first| {
                quotes.iter().find(|(open, _)| *open == first).copied()
            });
            let (value, tail) = if let Some((open, close)) = opener {
                let after = &after[open.len_utf8()..];
                let end = after.find(close)?;
                (&after[..end], &after[end + close.len_utf8()..])
            } else {
                let end = after.find(char::is_whitespace).unwrap_or(after.len());
                (&after[..end], &after[end..])
            };
            attr.attributes.push((rest[..name_end].to_owned(), value.to_owned()));
            rest = tail.trim_start();
        }
    }
    Some(attr)
}

/// `[text]{#id .cls k=v}` is a `Span` — a construct only pandoc's
/// markdown reads, and one comrak leaves as the literal text it is
/// written with. The text between the brackets has already been read as
/// inlines, so this only has to find the two ends and wrap what lies
/// between; the shape is the quote pairing's.
///
/// Two rules measured beyond the obvious: `.smallcaps` **alone** is a
/// `SmallCaps` rather than a `Span` carrying that class, and among other
/// attributes it is a `SmallCaps` **inside** the span; and an attribute
/// list with anything malformed in it is not a span at all, so
/// `[t]{foo}` stays `[t]{foo}`.
fn bracketed_spans(tokens: Vec<Inline>) -> Vec<Inline> {
    let holds = |needle: &str| {
        tokens.iter().any(|t| matches!(t, Inline::Str(word) if word.contains(needle)))
    };
    if !(holds("[") && holds("]{")) {
        return tokens;
    }
    let mut rest: std::collections::VecDeque<Inline> = tokens.into();
    let mut out: Vec<Inline> = Vec::new();
    while let Some(token) = rest.pop_front() {
        let Inline::Str(word) = token else {
            out.push(token);
            continue;
        };
        // A `[` that opens nothing leaves the word whole — cutting it
        // there would leave two `Str` tokens where pandoc has one.
        let mut from = 0;
        loop {
            let Some(offset) = word[from..].find('[') else {
                push_word(&mut out, word);
                break;
            };
            let at = from + offset;
            rest.push_front(Inline::Str(word[at + 1..].to_owned()));
            if let Some(span) = take_span(&mut rest) {
                push_word(&mut out, word[..at].to_owned());
                out.push(span);
                break;
            }
            // `take_span` consumed nothing, so the tail goes back into
            // the word and the search moves past this bracket.
            rest.pop_front();
            from = at + 1;
        }
    }
    out
}

fn push_word(out: &mut Vec<Inline>, word: String) {
    if !word.is_empty() {
        out.push(Inline::Str(word));
    }
}

/// The span that opens at the front of `rest`, or nothing — and nothing
/// is consumed unless there is one.
fn take_span(rest: &mut std::collections::VecDeque<Inline>) -> Option<Inline> {
    // Where the `]{` is, and where the `}` that closes the list is.
    let (index, at) = rest.iter().enumerate().find_map(|(index, token)| match token {
        Inline::Str(word) => word.find("]{").map(|at| (index, at)),
        _ => None,
    })?;
    let mut list = String::new();
    let mut end = None;
    for (offset, token) in rest.iter().enumerate().skip(index) {
        let word = match token {
            Inline::Str(word) if offset == index => &word[at + 2..],
            Inline::Str(word) => word.as_str(),
            Inline::Space => {
                list.push(' ');
                continue;
            }
            // An attribute list is text and spaces; anything else means
            // the `]{` was not one.
            _ => return None,
        };
        if let Some(close) = word.find('}') {
            list.push_str(&word[..close]);
            end = Some((offset, word.len() - close - 1));
            break;
        }
        list.push_str(word);
    }
    let (end_index, remaining) = end?;
    let attr = pandoc_attr(&list)?;

    let mut content: Vec<Inline> = Vec::new();
    for _ in 0..index {
        content.push(rest.pop_front().expect("counted just now"));
    }
    let Some(Inline::Str(word)) = rest.pop_front() else {
        unreachable!("the `]{{` was found in a Str")
    };
    push_word(&mut content, word[..at].to_owned());
    // Everything from the `]{` through the `}` goes; what follows the
    // `}` in whichever token held it comes back for the next pass.
    let mut closing = word;
    for _ in index..end_index {
        closing = match rest.pop_front() {
            Some(Inline::Str(next)) => next,
            _ => String::new(),
        };
    }
    rest.push_front(Inline::Str(closing[closing.len() - remaining..].to_owned()));

    let content = pandoc_inlines(content);
    let small = attr.classes.iter().position(|class| class == "smallcaps");
    Some(match small {
        None => Inline::Span(Box::new(attr), content),
        Some(index) => {
            let mut attr = attr;
            attr.classes.remove(index);
            let caps = Inline::SmallCaps(content);
            if attr == Attr::default() {
                caps
            } else {
                Inline::Span(Box::new(attr), vec![caps])
            }
        }
    })
}

/// An HTML tag's attributes as pandoc reads them: `id` is the
/// identifier, `class` splits on whitespace, and everything else keeps
/// its order. Measured — the attribute *names* are matched without
/// regard to case (`ID`, `Class`) while the values keep theirs, and a
/// bare attribute is a pair with an empty value.
fn tag_attr(raw: &str) -> Attr {
    let mut attr = Attr::default();
    // Past `<name`, and short of the `>`.
    let body = raw
        .trim_start_matches('<')
        .trim_end_matches('>')
        .trim_end_matches('/');
    let mut rest = body.trim_start_matches(|c: char| c.is_ascii_alphanumeric() || c == '-');
    while !rest.is_empty() {
        rest = rest.trim_start();
        let name_len = rest
            .chars()
            .take_while(|c| !c.is_whitespace() && *c != '=' && *c != '>')
            .map(char::len_utf8)
            .sum();
        if name_len == 0 {
            break;
        }
        let (name, after) = rest.split_at(name_len);
        let after = after.trim_start();
        let (value, tail) = match after.strip_prefix('=') {
            None => (String::new(), after),
            Some(after) => {
                let after = after.trim_start();
                if let Some(quote @ ('"' | '\'')) = after.chars().next() {
                    let after = &after[quote.len_utf8()..];
                    let end = after.find(quote).unwrap_or(after.len());
                    (after[..end].to_owned(), &after[(end + 1).min(after.len())..])
                } else {
                    let end = after.find(char::is_whitespace).unwrap_or(after.len());
                    (after[..end].to_owned(), &after[end..])
                }
            }
        };
        match name.to_ascii_lowercase().as_str() {
            "id" => attr.identifier = value,
            "class" => attr.classes = value.split_whitespace().map(str::to_owned).collect(),
            _ => attr.attributes.push((name.to_owned(), value)),
        }
        rest = tail;
    }
    attr
}

/// The attributes of `block` if it is a raw opening `<div>` and nothing
/// else.
fn div_open(block: &Block) -> Option<Attr> {
    let Block::RawBlock(format, raw) = block else { return None };
    if format.0 != "html" {
        return None;
    }
    let raw = raw.trim();
    let (start, tag) = next_tag(raw, 0)?;
    (start == 0 && tag.end == raw.len() && !tag.closing && tag.name == "div")
        .then(|| tag_attr(raw))
}

fn div_close(block: &Block) -> bool {
    matches!(block, Block::RawBlock(format, raw)
        if format.0 == "html" && raw.trim().eq_ignore_ascii_case("</div>"))
}

/// Pandoc's `native_divs`: a `<div>` and the `</div>` that closes it are
/// a `Div` carrying the element's attributes, not two raw blocks.
///
/// Measured: an **unclosed** `<div>` takes everything after it, and an
/// unmatched `</div>` stays raw. The depth bound is the reader's
/// never-panic promise — a document of ten thousand nested `<div>`s must
/// not take the stack with it.
fn native_divs(blocks: Vec<Block>, depth: usize) -> Vec<Block> {
    if depth >= MAX_NESTING || !blocks.iter().any(|b| div_open(b).is_some()) {
        return blocks;
    }
    let mut items: std::collections::VecDeque<Block> = blocks.into();
    let mut out = Vec::with_capacity(items.len());
    while let Some(block) = items.pop_front() {
        let Some(attr) = div_open(&block) else {
            out.push(block);
            continue;
        };
        let mut level = 0usize;
        let mut closes_at = None;
        for (index, candidate) in items.iter().enumerate() {
            if div_open(candidate).is_some() {
                level += 1;
            } else if div_close(candidate) {
                if level == 0 {
                    closes_at = Some(index);
                    break;
                }
                level -= 1;
            }
        }
        let take = closes_at.unwrap_or(items.len());
        let inner: Vec<Block> = items.drain(..take).collect();
        if closes_at.is_some() {
            items.pop_front();
        }
        out.push(Block::Div(attr, native_divs(inner, depth + 1)));
    }
    out
}

/// Pandoc's `native_spans`, the inline half of the same rule: a
/// `<span>` and its `</span>` are a `Span`. Runs over one sibling list,
/// as the quote pairing does, and an unclosed one stays raw.
fn native_spans(tokens: Vec<Inline>) -> Vec<Inline> {
    fn span_open(token: &Inline) -> Option<Attr> {
        let Inline::RawInline(format, raw) = token else { return None };
        if format.0 != "html" {
            return None;
        }
        let (start, tag) = next_tag(raw, 0)?;
        (start == 0 && tag.end == raw.len() && !tag.closing && tag.name == "span")
            .then(|| tag_attr(raw))
    }
    fn span_close(token: &Inline) -> bool {
        matches!(token, Inline::RawInline(format, raw)
            if format.0 == "html" && raw.eq_ignore_ascii_case("</span>"))
    }
    if !tokens.iter().any(|t| span_open(t).is_some()) {
        return tokens;
    }
    let mut items: std::collections::VecDeque<Inline> = tokens.into();
    let mut out = Vec::with_capacity(items.len());
    while let Some(token) = items.pop_front() {
        let Some(attr) = span_open(&token) else {
            out.push(token);
            continue;
        };
        let mut level = 0usize;
        let mut closes_at = None;
        for (index, candidate) in items.iter().enumerate() {
            if span_open(candidate).is_some() {
                level += 1;
            } else if span_close(candidate) {
                if level == 0 {
                    closes_at = Some(index);
                    break;
                }
                level -= 1;
            }
        }
        let Some(closes_at) = closes_at else {
            out.push(token);
            continue;
        };
        let inner: Vec<Inline> = items.drain(..closes_at).collect();
        items.pop_front();
        out.push(Inline::Span(Box::new(attr), native_spans(inner)));
    }
    out
}

/// The class `pandoc -f markdown` gives `<http://x>` and `<a@b.example>`.
///
/// Probed on `see <http://x.example> and <a@b.example>`: `-f markdown`
/// classes them `uri` and `email`; `-f gfm` and `-f commonmark` class
/// neither, so this belongs to one dialect and would break `diff-gfm` if
/// it leaked out.
///
/// The shape is a link whose text is its own target. That also catches a
/// hand-written `[http://x](http://x)`, which pandoc leaves bare —
/// recorded in `COMPATIBILITY.md` rather than papered over, because the
/// alternative is reading source positions to tell two identical ASTs
/// apart.
fn autolink_class(dialect: Dialect, text: &[Inline], url: &str) -> Attr {
    if dialect != Dialect::Pandoc {
        return Attr::default();
    }
    let [Inline::Str(literal)] = text else {
        return Attr::default();
    };
    let class = if url == literal {
        "uri"
    } else if url.strip_prefix("mailto:") == Some(literal.as_str()) {
        "email"
    } else {
        return Attr::default();
    };
    Attr { classes: vec![class.to_owned()], ..Attr::default() }
}

/// The document's YAML metadata block, converted to pandoc's `Meta`.
///
/// **A deliberately small subset of YAML**, and everything outside it is
/// an error rather than a guess: `key: scalar`, `key:` followed by
/// `- item` lines, `#` comments and blank lines. Nested maps, block
/// scalars (`|`, `>`), flow collections (`[a, b]`), anchors and aliases
/// are refused by name. A metadata block is the one construct where
/// reading it *nearly* right is worse than refusing it — the values become
/// the document's title and authors, and a wrong one is invisible in the
/// output.
///
/// The value semantics are pandoc's, probed with
/// `pandoc -f markdown -t json`:
///
/// - a scalar is parsed **as markdown inlines**, so `title: A *report*`
///   is `MetaInlines [Str "A", Space, Emph [Str "report"]]`;
/// - `true` and `false` are `MetaBool`; a number is `MetaInlines`, not a
///   number — `count: 3` is `MetaInlines [Str "3"]`;
/// - a list is `MetaList` of whatever its items are.
fn front_matter<'a>(root: &'a AstNode<'a>) -> Result<Meta, Error> {
    let mut meta = Meta::new();
    for node in root.children() {
        let NodeValue::FrontMatter(text) = &node.data.borrow().value else {
            continue;
        };
        let mut pending: Option<(String, Vec<MetaValue>)> = None;
        let lines: Vec<&str> = text.lines().collect();
        let mut at = 0;
        while at < lines.len() {
            let line = lines[at];
            at += 1;
            let trimmed = line.trim_end();
            // The delimiters comrak hands back with the block, and the
            // two things YAML ignores.
            if trimmed.trim().is_empty()
                || trimmed.trim() == "---"
                || trimmed.trim_start().starts_with('#')
            {
                continue;
            }
            if let Some(item) = trimmed.trim_start().strip_prefix("- ") {
                let Some((_, items)) = pending.as_mut() else {
                    return Err(Error::Metadata(format!("{trimmed:?}")));
                };
                items.push(scalar(item.trim()));
                continue;
            }
            if let Some((key, items)) = pending.take() {
                meta.insert(key, MetaValue::MetaList(items));
            }
            // A key at the left margin, and nothing else: an indented key
            // is a nested map, which this does not read.
            if trimmed.starts_with(char::is_whitespace) {
                return Err(Error::Metadata(format!("{trimmed:?}")));
            }
            let Some((key, value)) = trimmed.split_once(':') else {
                return Err(Error::Metadata(format!("{trimmed:?}")));
            };
            let value = value.trim();
            if value.is_empty() {
                pending = Some((key.trim().to_owned(), Vec::new()));
            } else if let Some((folds, strips)) = block_scalar(value) {
                // The lines under it are the ones indented past the key,
                // and a blank line belongs to the block rather than
                // ending it.
                let start = at;
                while at < lines.len()
                    && (lines[at].trim().is_empty()
                        || lines[at].starts_with(char::is_whitespace))
                {
                    at += 1;
                }
                let body = block_body(&lines[start..at], folds, strips);
                meta.insert(key.trim().to_owned(), body);
            } else if value.starts_with(['[', '{', '&', '*']) {
                return Err(Error::Metadata(format!("{trimmed:?}")));
            } else {
                meta.insert(key.trim().to_owned(), scalar(value));
            }
        }
        if let Some((key, items)) = pending.take() {
            meta.insert(key, MetaValue::MetaList(items));
        }
    }
    Ok(meta)
}

/// Whether a value opens a block scalar, and whether it folds.
///
/// `|` keeps its line breaks and `>` folds them into spaces; a chomping
/// indicator (`-`, `+`) changes only the trailing newlines, which the
/// markdown parse ignores anyway. Anything else after the marker is a
/// YAML feature this does not read, and is refused rather than guessed.
fn block_scalar(value: &str) -> Option<(bool, bool)> {
    let (marker, rest) = value.split_at(value.chars().next()?.len_utf8());
    let folds = match marker {
        "|" => false,
        ">" => true,
        _ => return None,
    };
    let rest = rest.trim_end();
    rest.chars()
        .all(|c| c == '-' || c == '+')
        .then_some((folds, rest.contains('-')))
}

/// A block scalar's lines, as the metadata value pandoc reads from it.
///
/// Pandoc gives one `MetaBlocks` — the text read as markdown, not as
/// inlines — so `abstract: |` holding two lines is one paragraph with a
/// soft break in it, and the folded form is one paragraph without.
/// **Unless the chomping indicator strips the trailing newline**: `|-`
/// leaves a value that does not end in one, and pandoc reads that as
/// `MetaInlines`. Measured on all four indicators.
fn block_body(lines: &[&str], folds: bool, strips: bool) -> MetaValue {
    let indent = lines
        .iter()
        .filter(|line| !line.trim().is_empty())
        .map(|line| line.len() - line.trim_start_matches(' ').len())
        .min()
        .unwrap_or(0);
    let mut text = String::new();
    for line in lines {
        let line = if line.len() > indent { &line[indent..] } else { line.trim_start() };
        // Folding joins a line to the one before it; a blank line still
        // separates paragraphs.
        let separator = if folds && !text.is_empty() && !line.trim().is_empty() {
            if text.ends_with('\n') { "" } else { " " }
        } else {
            "\n"
        };
        if !text.is_empty() {
            text.push_str(separator);
        }
        text.push_str(line.trim_end());
    }
    text.push('\n');
    let mut blocks = fragment(&text, &Notes::new(), Dialect::Pandoc);
    if !strips {
        return MetaValue::MetaBlocks(blocks);
    }
    match blocks.pop() {
        Some(Block::Para(inlines) | Block::Plain(inlines)) if blocks.is_empty() => {
            MetaValue::MetaInlines(inlines)
        }
        Some(last) => {
            blocks.push(last);
            MetaValue::MetaBlocks(blocks)
        }
        None => MetaValue::MetaInlines(Vec::new()),
    }
}

/// One YAML scalar as pandoc reads it: a bool, or markdown inlines.
fn scalar(text: &str) -> MetaValue {
    let unquoted = text
        .strip_prefix('"')
        .and_then(|rest| rest.strip_suffix('"'))
        .or_else(|| text.strip_prefix('\'').and_then(|rest| rest.strip_suffix('\'')))
        .unwrap_or(text);
    match unquoted {
        "true" => return MetaValue::MetaBool(true),
        "false" => return MetaValue::MetaBool(false),
        _ => {}
    }
    // Parsed as markdown, which is what makes `title: A *report*` an
    // `Emph` rather than three literal characters.
    let inlines = match read_commonmark(unquoted) {
        Ok(document) => match document.blocks.into_iter().next() {
            Some(Block::Para(inlines) | Block::Plain(inlines)) => inlines,
            _ => vec![Inline::Str(unquoted.to_owned())],
        },
        Err(_) => vec![Inline::Str(unquoted.to_owned())],
    };
    MetaValue::MetaInlines(inlines)
}

/// Footnote bodies keyed by the label comrak parsed them under. Empty for
/// `commonmark`, whose pandoc reader has no footnotes at all.
type Notes = HashMap<String, Vec<Block>>;

/// Every `[^label]: body` in the document, converted once.
///
/// The bodies are converted against an **empty** map, so a reference inside
/// a footnote body resolves to nothing. That is pandoc's behaviour, not a
/// simplification: `[^1]: outer[^2]` gives it `Note [Para [Str "outer",
/// Str ""]]` — the inner reference becomes an empty `Str` and `[^2]`'s body
/// is never reached. It also makes the conversion non-recursive, which
/// matters more than matching a quirk: `[^1]: see [^1]` hangs pandoc
/// indefinitely (measured — killed at 20 s), and a reader here must
/// terminate on every input.
fn footnotes<'a>(root: &'a AstNode<'a>, src: &Src, dialect: Dialect) -> Notes {
    let empty = Notes::new();
    let mut found = Notes::new();
    let mut stack = vec![root];
    while let Some(node) = stack.pop() {
        if let NodeValue::FootnoteDefinition(def) = &node.data.borrow().value {
            found.insert(
                def.name.clone(),
                blocks(node.children(), src, false, &empty, dialect),
            );
        }
        stack.extend(node.children());
    }
    found
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
    defs: &Notes,
    dialect: Dialect,
) -> Vec<Block> {
    let mut out = Vec::new();
    for node in nodes {
        // A list is the one node that can map to more than one block:
        // pandoc's gfm reader treats task items as a different kind of
        // list, so a plain item among them starts a new one.
        let list = match node.data.borrow().value {
            NodeValue::List(nl) => Some(nl),
            _ => None,
        };
        // And so can a run of raw HTML in pandoc's dialect: a
        // block-level tag is a `RawBlock` of its own and what lies
        // between two of them is markdown.
        let html = match &node.data.borrow().value {
            NodeValue::HtmlBlock(hb) if dialect == Dialect::Pandoc => {
                Some(html_literal(hb, &node.data.borrow(), src, in_quote))
            }
            _ => None,
        };
        if let Some(nl) = list {
            out.extend(lists(node, &nl, src, in_quote, defs, dialect));
        } else if let Some(literal) = html {
            out.extend(html_run(&literal, defs, dialect));
        } else if let Some(block) = block(node, src, in_quote, defs, dialect) {
            if dialect == Dialect::Pandoc {
                split_leading_html(block, &mut out);
            } else {
                out.push(block);
            }
        }
    }
    if dialect == Dialect::Pandoc { native_divs(out, 0) } else { out }
}

/// Map one comrak list, splitting it wherever a task item meets a plain
/// one — pandoc parses task items as a different kind of list, so a plain
/// item among them starts a new one.
///
/// Each run's tightness is then its own, not the source list's: pandoc
/// parses the runs separately from the start, so a run with no blank line
/// inside it is tight even when the list it was cut from is loose.
fn lists<'a>(
    node: &'a AstNode<'a>,
    nl: &comrak::nodes::NodeList,
    src: &Src,
    in_quote: bool,
    defs: &Notes,
    dialect: Dialect,
) -> Vec<Block> {
    // Only bullet lists have task items in pandoc, so an ordered list is
    // never split — and **only `gfm` splits at all**. Pandoc's own
    // markdown keeps `- [ ] a` and `- b` in one list, measured; running
    // the gfm rule there cut every mixed list into three.
    let mut runs: Vec<(bool, Vec<&'a AstNode<'a>>)> = Vec::new();
    for item in node.children() {
        let is_task = dialect == Dialect::Gfm
            && nl.list_type == ListType::Bullet
            && matches!(item.data.borrow().value, NodeValue::TaskItem(_));
        match runs.last_mut() {
            Some((kind, nodes)) if *kind == is_task => nodes.push(item),
            _ => runs.push((is_task, vec![item])),
        }
    }
    let split = runs.len() > 1;
    runs.into_iter()
        .map(|(_, nodes)| {
            let tight = nl.tight || (split && !is_loose(&nodes, src));
            let items: Vec<Vec<Block>> = nodes
                .iter()
                .map(|item| {
                    let mut item_blocks = blocks(item.children(), src, in_quote, defs, dialect);
                    if tight {
                        for block in &mut item_blocks {
                            if let Block::Para(inlines) = block {
                                *block = Block::Plain(std::mem::take(inlines));
                            }
                        }
                    }
                    if let NodeValue::TaskItem(t) = item.data.borrow().value {
                        let empty = item_blocks.iter().all(|block| {
                            matches!(block, Block::Plain(inlines) | Block::Para(inlines)
                                if inlines.is_empty())
                        });
                        let marker = task_marker(t.symbol, nl.list_type, dialect, empty);
                        prepend(&mut item_blocks, marker, tight);
                    }
                    item_blocks
                })
                .collect();
            match nl.list_type {
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
            }
        })
        .collect()
}

/// Whether a run of list items is loose in `CommonMark`'s sense: a blank
/// line between two items, or between two blocks inside one item. Read
/// from the source rather than from comrak, whose flag describes the whole
/// list — including blank lines that fall outside this run.
fn is_loose<'a>(items: &[&'a AstNode<'a>], src: &Src) -> bool {
    let blank_between = |a: &'a AstNode<'a>, b: &'a AstNode<'a>| {
        let after = a.data.borrow().sourcepos.end.line;
        let before = b.data.borrow().sourcepos.start.line;
        // A blank line inside a container still carries its `>` markers.
        (after + 1..before).any(|n| first_nonmarker_char(src.line(n)).is_none())
    };
    items.windows(2).any(|pair| blank_between(pair[0], pair[1]))
        || items.iter().any(|item| {
            let blocks: Vec<_> = item.children().collect();
            blocks.windows(2).any(|pair| blank_between(pair[0], pair[1]))
        })
}

/// One paragraph: two shapes that are not one, and the ordinary case.
fn paragraph<'a>(
    node: &'a AstNode<'a>,
    src: &Src,
    data: &comrak::nodes::Ast,
    defs: &Notes,
    dialect: Dialect,
) -> Block {
    let content = inlines(node.children(), defs, dialect);
    // Comrak quirk: a paragraph of link reference definitions whose
    // last line is a dash-run comes out as a literal `---` paragraph;
    // pandoc consumes the definitions and reads the dashes as a
    // thematic break. Detect via the paragraph's first source line
    // (a reference definition starts with `[`), which also excludes
    // lookalikes such as escaped or entity-encoded dashes.
    if let [Inline::Str(dashes)] = content.as_slice()
        && dashes.len() >= 3
        && dashes.bytes().all(|byte| byte == b'-')
        && first_nonmarker_char(src.line(data.sourcepos.start.line)) == Some('[')
    {
        return Block::HorizontalRule;
    }
    if dialect == Dialect::Pandoc
        && let [Inline::Image(attr, alt, target)] = content.as_slice()
        && !alt.is_empty()
    {
        return implicit_figure(attr, alt, target);
    }
    // Pandoc's `para` falls back to `plain` when no blank line follows,
    // and that is a rule about *every* paragraph, not only the ones
    // beside HTML: `a\n<p>x</p>` opens with a `Plain`, `a\n\nb` is two
    // `Para`s, and a paragraph at end of input is a `Para` because
    // pandoc appends the blank line itself. `CommonMark` has no such
    // distinction and comrak writes `Para` throughout.
    //
    // Two exceptions, both measured. A **fenced code block** ends a
    // paragraph as a blank line does (pandoc's `para` looks ahead for
    // one), so `foo\n```\nbar\n```` keeps its `Para`. And a paragraph
    // directly inside a **list item** is left alone: the list's own
    // tightness decides there, and reading the next source line instead
    // made every item but the last of a loose list a `Plain`.
    if dialect == Dialect::Pandoc && !in_item(node) {
        let next = src.line(data.sourcepos.end.line + 1).trim();
        let fence = next.starts_with("```") || next.starts_with("~~~");
        // A `</div>` is not left on the next line: `native_divs` takes
        // it, so the paragraph before it ends its own document.
        let absorbed = next.eq_ignore_ascii_case("</div>");
        if first_nonmarker_char(next).is_some() && !fence && !absorbed {
            return Block::Plain(content);
        }
    }
    Block::Para(content)
}

/// Whether this node's own container reads its content as a document of
/// its own, so the line after the paragraph is not the paragraph's
/// business.
///
/// A list item and a definition settle `Plain` versus `Para` by the
/// list's tightness. A **footnote definition** ends where the next one
/// begins, so `[^1]: a` with `[^2]: b` on the line under it closes at a
/// `Para` — reading the next source line there made it a `Plain`.
fn in_item<'a>(node: &'a AstNode<'a>) -> bool {
    node.parent().is_some_and(|parent| {
        matches!(
            parent.data.borrow().value,
            NodeValue::Item(_)
                | NodeValue::TaskItem(_)
                | NodeValue::DescriptionTerm
                | NodeValue::DescriptionDetails
                | NodeValue::FootnoteDefinition(_)
        )
    })
}

fn block<'a>(node: &'a AstNode<'a>, src: &Src, in_quote: bool, defs: &Notes, dialect: Dialect) -> Option<Block> {
    let data = node.data.borrow();
    match &data.value {
        NodeValue::Paragraph => Some(paragraph(node, src, &data, defs, dialect)),
        NodeValue::Heading(h) => Some(Block::Header(
            i64::from(h.level),
            // `{#id .cls key=val}` off the end of the heading line, which
            // only `pandoc_markdown` turns on. comrak's `Attributes` is
            // pandoc's `Attr` field for field.
            data.attrs.as_ref().map_or_else(Attr::default, |attrs| Attr {
                identifier: attrs.id.clone().unwrap_or_default(),
                classes: attrs.classes.clone(),
                attributes: attrs.pairs.clone(),
            }),
            inlines(node.children(), defs, dialect),
        )),
        NodeValue::BlockQuote => Some(Block::BlockQuote(blocks(node.children(), src, true, defs, dialect))),
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
            // `{#id .class k=v}` after the fence, which only
            // `pandoc_markdown` turns on; comrak hangs it here exactly as
            // it does for a heading. Without it the whole brace was taken
            // for the language and every block came out classed `{.rust`.
            let attr = data.attrs.as_ref().map_or_else(
                || Attr {
                    classes: cb
                        .info
                        .split_whitespace()
                        .next()
                        .map(|lang| vec![lang.to_owned()])
                        .unwrap_or_default(),
                    ..Attr::default()
                },
                |attrs| Attr {
                    identifier: attrs.id.clone().unwrap_or_default(),
                    classes: attrs.classes.clone(),
                    attributes: attrs.pairs.clone(),
                },
            );
            Some(Block::CodeBlock(attr, text.to_owned()))
        }
        NodeValue::HtmlBlock(hb) => Some(Block::RawBlock(
            Format("html".to_owned()),
            html_literal(hb, &data, src, in_quote),
        )),
        NodeValue::ThematicBreak => Some(Block::HorizontalRule),
        // A definition list is a `DescriptionList` of `DescriptionItem`s,
        // each holding a `DescriptionTerm` and one `DescriptionDetails`.
        // Pandoc holds the same shape as one term with a list of
        // definitions, so consecutive items sharing a term would merge —
        // and pandoc keeps them separate, which is what this does.
        NodeValue::DescriptionList => {
            let mut items: Vec<(Vec<Inline>, Vec<Vec<Block>>)> = Vec::new();
            for item in node.children() {
                let mut term = Vec::new();
                let mut definitions = Vec::new();
                // comrak wraps both halves in a `Paragraph`; the term is
                // inlines in pandoc's AST, and a **tight** item's
                // definition is `Plain` where a loose one is `Para` —
                // probed, `Term\n:   text` gives `Plain` and a blank line
                // before the definition gives `Para`.
                let tight = matches!(
                    item.data.borrow().value,
                    NodeValue::DescriptionItem(ref d) if d.tight
                );
                for part in item.children() {
                    match part.data.borrow().value {
                        NodeValue::DescriptionTerm => {
                            term = part
                                .children()
                                .next()
                                .map_or_else(Vec::new, |para| inlines(para.children(), defs, dialect));
                        }
                        NodeValue::DescriptionDetails => {
                            let mut definition = blocks(part.children(), src, in_quote, defs, dialect);
                            if tight {
                                for block in &mut definition {
                                    if let Block::Para(inlines) = block {
                                        *block = Block::Plain(std::mem::take(inlines));
                                    }
                                }
                            }
                            definitions.push(definition);
                        }
                        _ => {}
                    }
                }
                // `Term` / `: one` / `: two` gives comrak a second item
                // with an **empty** term, where pandoc gives one term with
                // two definitions. Merging on the empty term is what makes
                // the two agree.
                match items.last_mut() {
                    Some((_, previous)) if term.is_empty() => previous.extend(definitions),
                    _ => items.push((term, definitions)),
                }
            }
            Some(Block::DefinitionList(items))
        }
        NodeValue::Table(t) => Some(Block::Table(Box::new(table(node, &t.alignments, defs, dialect)))),
        // Only core-CommonMark nodes occur with default comrak options, and
        // the GFM extensions add exactly the ones handled above; the
        // differential harness would surface anything dropped here.
        _ => None,
    }
}

/// The `☐`/`☒` a task list item contributes, as pandoc's gfm reader writes
/// it. Pandoc recognizes task items in bullet lists only, so an ordered
/// list keeps the literal brackets comrak consumed.
fn task_marker(
    symbol: Option<char>,
    list_type: ListType,
    dialect: Dialect,
    empty: bool,
) -> Vec<Inline> {
    // Which items get a box is **opposite** in the two dialects, on both
    // counts, and each was measured against its own reader:
    //
    //                      `- [ ]`, no content    `1. [ ] x`, ordered
    //   `gfm`                    box                    literal
    //   pandoc's `markdown`      literal                box
    //
    // For pandoc's dialect the emptiness decides on its own — `1. [ ]`
    // with nothing after it is literal in both.
    let boxed = if dialect == Dialect::Pandoc {
        !empty
    } else {
        list_type == ListType::Bullet
    };
    if boxed {
        let symbol = if symbol.is_some() { "\u{2612}" } else { "\u{2610}" };
        return vec![Inline::Str(symbol.to_owned()), Inline::Space];
    }
    match symbol {
        None => vec![
            Inline::Str("[".to_owned()),
            Inline::Space,
            Inline::Str("]".to_owned()),
            Inline::Space,
        ],
        Some(c) => vec![Inline::Str(format!("[{c}]")), Inline::Space],
    }
}

/// Put `prefix` in front of an item's first line of text, opening a
/// [`Block::Plain`] for it when the item starts with something that holds
/// no inlines (an empty item, or one opening with a code block).
fn prepend(item: &mut Vec<Block>, mut prefix: Vec<Inline>, tight: bool) {
    let target = match item.first_mut() {
        Some(Block::Plain(inlines) | Block::Para(inlines)) => Some(inlines),
        _ => None,
    };
    // The single space after the marker is the one the source already
    // spells. Where the content opens with its own — `- [ ]  two spaces`
    // — or where there is no content at all, ours would be a second.
    let doubled = target
        .as_ref()
        .is_none_or(|inlines| matches!(inlines.first(), None | Some(Inline::Space)));
    if doubled {
        while prefix.last() == Some(&Inline::Space) {
            prefix.pop();
        }
    }
    match target {
        Some(inlines) => {
            inlines.splice(..0, prefix);
        }
        // An item with no content at all still gets a block, and it is a
        // paragraph exactly when the list around it is loose.
        None => item.insert(0, if tight { Block::Plain(prefix) } else { Block::Para(prefix) }),
    }
}

/// Map a GFM pipe table. comrak has already padded short rows and dropped
/// cells past the column count, which is what pandoc does too, so the grid
/// arrives rectangular.
fn table<'a>(node: &'a AstNode<'a>, alignments: &[TableAlignment], defs: &Notes, dialect: Dialect) -> Table {
    let row = |n: &'a AstNode<'a>| Row {
        attr: Attr::default(),
        cells: n
            .children()
            .map(|c| {
                let content = inlines(c.children(), defs, dialect);
                Cell {
                    attr: Attr::default(),
                    alignment: Alignment::AlignDefault,
                    row_span: 1,
                    col_span: 1,
                    blocks: if content.is_empty() {
                        Vec::new()
                    } else {
                        vec![Block::Plain(content)]
                    },
                }
            })
            .collect(),
    };
    let mut head = Vec::new();
    let mut body = Vec::new();
    for child in node.children() {
        if matches!(child.data.borrow().value, NodeValue::TableRow(true)) {
            head.push(row(child));
        } else {
            body.push(row(child));
        }
    }
    Table {
        attr: Attr::default(),
        caption: Caption::default(),
        colspecs: alignments
            .iter()
            .map(|a| ColSpec {
                alignment: match a {
                    TableAlignment::Left => Alignment::AlignLeft,
                    TableAlignment::Center => Alignment::AlignCenter,
                    TableAlignment::Right => Alignment::AlignRight,
                    TableAlignment::None => Alignment::AlignDefault,
                },
                width: ColWidth::ColWidthDefault,
            })
            .collect(),
        head: TableHead { attr: Attr::default(), rows: head },
        // Pandoc emits one body even when the table is nothing but its
        // header row, so an empty `body` is still one empty `TableBody`.
        bodies: vec![TableBody {
            attr: Attr::default(),
            row_head_columns: 0,
            head: Vec::new(),
            body,
        }],
        foot: TableFoot::default(),
    }
}

/// Heading identifiers, handed out in document order the way pandoc's
/// `gfm_auto_identifiers` does.
///
/// The name is a per-base counter, not a set of names already used: a
/// heading whose own slug is `a-1` takes `a-1` even when an earlier `a`
/// already produced it. Probed, because it looks like a bug and is not
/// ours to fix — `# a` `# a` `# a-1` gives `a`, `a-1`, `a-1`.
struct Identifiers {
    seen: HashMap<String, u32>,
    dialect: Dialect,
}

impl Identifiers {
    /// Assign identifiers to every heading, descending into the only
    /// containers this reader can nest one in.
    fn assign(&mut self, blocks: &mut [Block]) {
        for block in blocks {
            match block {
                Block::Header(_, attr, inlines) => {
                    // A heading that named itself keeps its name: only
                    // `pandoc_markdown` reads `{#custom-id}`, and pandoc
                    // slugs the heading text solely when there is none.
                    // The explicit one still counts toward uniquing, so a
                    // later heading slugging to the same word gets `-1`.
                    if attr.identifier.is_empty() {
                        let mut text = String::new();
                        stringify(inlines, &mut text);
                        attr.identifier = self.unique(&slug(&text, self.dialect));
                    } else {
                        let taken = attr.identifier.clone();
                        let _ = self.unique(&taken);
                    }
                }
                Block::BlockQuote(inner) | Block::Div(_, inner) => self.assign(inner),
                Block::BulletList(items) | Block::OrderedList(_, items) => {
                    for item in items {
                        self.assign(item);
                    }
                }
                _ => {}
            }
        }
    }

    fn unique(&mut self, base: &str) -> String {
        let count = self.seen.entry(base.to_owned()).or_insert(0);
        let identifier = if *count == 0 {
            base.to_owned()
        } else {
            format!("{base}-{count}")
        };
        *count += 1;
        identifier
    }
}

/// GitHub's slug: lowercase, drop everything that is not a word character,
/// a hyphen or a space, and turn spaces into hyphens. Any whitespace
/// counts, not just the ASCII space — pandoc slugs a heading holding a
/// non-breaking space as `a-b`, not `ab`.
fn slug(text: &str, dialect: Dialect) -> String {
    if dialect != Dialect::Pandoc {
        let mut out = String::with_capacity(text.len());
        for ch in text.chars() {
            if ch.is_whitespace() {
                out.push('-');
            } else if ch == '-' || ch == '_' || ch.is_alphanumeric() {
                out.extend(ch.to_lowercase());
            }
        }
        return out;
    }
    // Pandoc's own dialect slugs differently in three ways, each
    // probed: it keeps `.`, it splits the filtered text into **words**
    // and joins them with one hyphen rather than writing one per
    // whitespace character, and it drops everything before the first
    // letter — so `# foo ### b` is `foo-b` where GitHub says `foo--b`,
    // `# a.b` is `a.b` where GitHub says `ab`, and `# 1. x` is `x`.
    // A heading that leaves nothing behind is called `section`.
    let kept: String = text
        .chars()
        .filter(|c| {
            c.is_whitespace() || *c == '-' || *c == '_' || *c == '.' || c.is_alphanumeric()
        })
        .flat_map(char::to_lowercase)
        .collect();
    let mut out = kept.split_whitespace().collect::<Vec<_>>().join("-");
    match out.find(char::is_alphabetic) {
        Some(at) => out.split_off(at),
        None => "section".to_owned(),
    }
}

/// The plain text of an inline sequence, as pandoc's `stringify` produces
/// it for identifiers: every kind of break is a space, and raw HTML and
/// footnotes contribute nothing.
fn stringify(inlines: &[Inline], out: &mut String) {
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
            | Inline::Image(_, inner, _) => stringify(inner, out),
        }
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

fn inlines<'a>(nodes: impl Iterator<Item = &'a AstNode<'a>>, defs: &Notes, dialect: Dialect) -> Vec<Inline> {
    let iter = nodes.into_iter();
    let mut out = Vec::with_capacity(iter.size_hint().0 * 2);
    for node in iter {
        inline(node, &mut out, defs, dialect);
    }
    let out = merge_adjacent_emphasis(out);
    if dialect == Dialect::Pandoc { pandoc_inlines(out) } else { out }
}

/// The three pairings pandoc's dialect makes over one sibling list.
///
/// **`bracketed_spans` runs first**, and must: `smart` has already
/// turned the quotes in `[t]{k="a b"}` curly, and pairing those into a
/// `Quoted` before the attribute list is read would eat the value.
fn pandoc_inlines(tokens: Vec<Inline>) -> Vec<Inline> {
    native_spans(quoted(bracketed_spans(tokens)))
}

/// The curly quotes `smart` produced, paired into `Quoted` elements.
///
/// comrak writes the characters and stops there; pandoc makes a **pair**
/// an element and leaves a lone one as the character it is, which is how
/// `don’t` and `dogs’ bones` survive. Three rules, each probed against
/// `pandoc -f markdown -t json`:
///
/// * an opener with no closer after it is text — `a "unclosed` stays
///   `“unclosed`;
/// * a pair inside emphasis is a `Quoted` inside the `Emph`, and a pair
///   that *straddles* one is not a pair at all: `"opens *and closes"
///   inside*` is two literal characters. That is why this runs over one
///   sibling list — the list `inlines` has just built — rather than over
///   the flattened document;
/// * the body is paired again, so `"nested 'inner' here"` nests.
fn quoted(tokens: Vec<Inline>) -> Vec<Inline> {
    const OPEN_DOUBLE: char = '\u{201c}';
    const CLOSE_DOUBLE: char = '\u{201d}';
    const OPEN_SINGLE: char = '\u{2018}';
    const CLOSE_SINGLE: char = '\u{2019}';

    fn text(out: &mut Vec<Inline>, word: String) {
        if !word.is_empty() {
            out.push(Inline::Str(word));
        }
    }

    /// The length of the mark that ends a quotation, which may be the
    /// closing character or an opening one standing in for it.
    fn ends_at(rest: &str) -> usize {
        rest.chars().next().map_or(0, char::len_utf8)
    }

    // comrak decides that a quote opens from what stands *before* it;
    // pandoc decides that it closes from what stands *after*, and the
    // two disagree wherever a quote has a space on both sides. `a " b "
    // c` is two closing marks there and two opening ones here, and no
    // pair at all either way. Measured on three shapes.
    let mut tokens = tokens;
    for index in 0..tokens.len() {
        let ends_the_line = match tokens.get(index + 1) {
            None | Some(Inline::Space | Inline::SoftBreak | Inline::LineBreak) => true,
            Some(_) => false,
        };
        let Some(Inline::Str(word)) = tokens.get_mut(index).filter(|_| ends_the_line) else {
            continue;
        };
        for (open, close) in [(OPEN_DOUBLE, CLOSE_DOUBLE), (OPEN_SINGLE, CLOSE_SINGLE)] {
            if word.ends_with(open) {
                word.truncate(word.len() - open.len_utf8());
                word.push(close);
            }
        }
    }

    let mut rest: std::collections::VecDeque<Inline> = tokens.into();
    let mut out: Vec<Inline> = Vec::new();
    while let Some(token) = rest.pop_front() {
        let Inline::Str(word) = token else {
            out.push(token);
            continue;
        };
        // An opener with no closer is text, and a later opener in the
        // same word may still have one, so the search moves past it
        // rather than cutting the word up.
        let mut from = 0;
        let found = loop {
            let Some(offset) = word[from..].find([OPEN_DOUBLE, OPEN_SINGLE]) else {
                break None;
            };
            let at = from + offset;
            let open = word[at..].chars().next().expect("the match names one");
            let close = if open == OPEN_DOUBLE { CLOSE_DOUBLE } else { CLOSE_SINGLE };
            let after = at + open.len_utf8();
            // An **empty** pair is not one: pandoc leaves `""` as two
            // characters rather than making a `Quoted` with nothing in it.
            // Inside an open quote the next mark of that kind ends it,
            // opening-shaped or not: pandoc reads `"b and then "c" done`
            // as one quotation and a stray mark, not as two. And an
            // **empty** pair is not one — `""` stays two characters.
            let ends = [close, open];
            if let Some(cut) = word[after..].find(ends)
                && cut > 0
            {
                break Some((at, open, close, None, after + cut));
            }
            if let Some(found) = rest.iter().enumerate().find_map(|(index, token)| match token {
                Inline::Str(word) => word.find(ends).map(|cut| (index, cut)),
                _ => None,
            }) && !(found == (0, 0) && word[after..].is_empty())
            {
                break Some((at, open, close, Some(found.0), found.1));
            }
            from = after;
        };
        let Some((at, open, _, sibling, cut)) = found else {
            text(&mut out, word);
            continue;
        };
        text(&mut out, word[..at].to_owned());
        let after = at + open.len_utf8();
        let mut body: Vec<Inline> = Vec::new();
        let tail = match sibling {
            // Both quotes in one word.
            None => {
                text(&mut body, word[after..cut].to_owned());
                word[cut + ends_at(&word[cut..])..].to_owned()
            }
            // The closer is a later sibling: everything between them is
            // the body, and what follows it goes back for the next pass.
            Some(index) => {
                text(&mut body, word[after..].to_owned());
                for _ in 0..index {
                    body.push(rest.pop_front().expect("counted just now"));
                }
                let Some(Inline::Str(last)) = rest.pop_front() else {
                    unreachable!("the closer was found in a Str")
                };
                text(&mut body, last[..cut].to_owned());
                last[cut + ends_at(&last[cut..])..].to_owned()
            }
        };
        rest.push_front(Inline::Str(tail));
        // A quotation does not end in whitespace: `"a *b* "c"` quotes
        // `a *b*` and not `a *b* `. Only reachable where an opening mark
        // ended the quotation, since a closing one cannot follow a space.
        while matches!(body.last(), Some(Inline::Space | Inline::SoftBreak | Inline::LineBreak)) {
            body.pop();
        }
        let kind = if open == OPEN_DOUBLE {
            ferrodoc_ast::QuoteType::DoubleQuote
        } else {
            ferrodoc_ast::QuoteType::SingleQuote
        };
        out.push(Inline::Quoted(kind, quoted(body)));
    }
    out
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

fn inline<'a>(node: &'a AstNode<'a>, out: &mut Vec<Inline>, defs: &Notes, dialect: Dialect) {
    let data = node.data.borrow();
    // `{#id .cls k=v}` after a link, an image or a code span, which only
    // `pandoc_markdown` turns on. comrak parses it and hangs it here;
    // the shape is the same one the heading arm reads.
    let written = || {
        data.attrs.as_ref().map_or_else(Attr::default, |attrs| Attr {
            identifier: attrs.id.clone().unwrap_or_default(),
            classes: attrs.classes.clone(),
            attributes: attrs.pairs.clone(),
        })
    };
    match &data.value {
        NodeValue::Text(t) => text_tokens(t, out),
        NodeValue::SoftBreak => out.push(Inline::SoftBreak),
        NodeValue::LineBreak => out.push(Inline::LineBreak),
        NodeValue::Code(c) => {
            // Pandoc trims a code span's whitespace on both sides;
            // `CommonMark` strips at most one space, and only when both
            // ends have one. Probed on six shapes: `` ` a` `` is `a`
            // there and ` a` here, and `` ` ` `` is empty. **ASCII
            // whitespace only** — a non-breaking space is content, and
            // trimming it took spec example 333 with it.
            let literal = if dialect == Dialect::Pandoc {
                c.literal.trim_matches(|c: char| c.is_ascii_whitespace()).to_owned()
            } else {
                c.literal.clone()
            };
            out.push(Inline::Code(Box::new(written()), literal));
        }
        NodeValue::HtmlInline(h) => {
            // `<!DOCTYPE …>`, `<!ELEMENT …>` and a CDATA section are the
            // literal text they are written with in pandoc's markdown,
            // not raw HTML. Measured: of the `<!` forms only a comment
            // is raw, and `CommonMark` takes all of them.
            if dialect == Dialect::Pandoc && h.starts_with("<!") && !h.starts_with("<!--") {
                let mut first = true;
                for line in h.split('\n') {
                    if !first {
                        out.push(Inline::SoftBreak);
                    }
                    first = false;
                    text_tokens(line, out);
                }
            } else {
                out.push(Inline::RawInline(Box::new(Format("html".to_owned())), h.clone()));
            }
        }
        NodeValue::Emph => out.push(Inline::Emph(inlines(node.children(), defs, dialect))),
        NodeValue::Strong => out.push(Inline::Strong(inlines(node.children(), defs, dialect))),
        NodeValue::Strikethrough => out.push(Inline::Strikeout(inlines(node.children(), defs, dialect))),
        NodeValue::Superscript => out.push(Inline::Superscript(inlines(node.children(), defs, dialect))),
        NodeValue::Subscript => out.push(Inline::Subscript(inlines(node.children(), defs, dialect))),
        NodeValue::Math(m) => out.push(Inline::Math(
            if m.display_math { MathType::DisplayMath } else { MathType::InlineMath },
            m.literal.clone(),
        )),
        NodeValue::Link(l) => {
            let text = inlines(node.children(), defs, dialect);
            let attr = match written() {
                given if given == Attr::default() => autolink_class(dialect, &text, &l.url),
                given => given,
            };
            out.push(Inline::Link(
                Box::new(attr),
                text,
                Box::new(Target { url: l.url.clone(), title: l.title.clone() }),
            ));
        }
        // One `Note` per reference, body cloned: pandoc duplicates the
        // body when a label is referenced twice rather than sharing it.
        // A label with no definition is `Str ""` — see `footnotes`.
        NodeValue::FootnoteReference(f) => {
            out.push(match defs.get(&f.name) {
                Some(body) => Inline::Note(body.clone()),
                None => Inline::Str(String::new()),
            });
        }
        NodeValue::Image(l) => out.push(Inline::Image(
            Box::new(written()),
            inlines(node.children(), defs, dialect),
            Box::new(Target { url: l.url.clone(), title: l.title.clone() }),
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

    fn gfm(md: &str) -> serde_json::Value {
        serde_json::to_value(read_gfm(md).expect("convertible")).unwrap()["blocks"].clone()
    }

    fn pmd(md: &str) -> serde_json::Value {
        serde_json::to_value(read_pandoc_markdown(md).expect("convertible")).unwrap()["blocks"]
            .clone()
    }

    /// Every heading's identifier, in document order.
    fn ids(md: &str) -> Vec<String> {
        read_gfm(md)
            .expect("convertible")
            .blocks
            .iter()
            .filter_map(|b| match b {
                Block::Header(_, attr, _) => Some(attr.identifier.clone()),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn a_footnote_is_a_note_in_gfm_and_a_link_definition_in_commonmark() {
        // `pandoc -f gfm` reads `[^1]` as `Note`; `-f commonmark` has no
        // footnotes at all and reads the definition as a link reference,
        // which is why the extension is set only in the gfm branch. Both
        // halves were probed against 3.8.2.1.
        let md = "Text.[^1]\n\n[^1]: body.\n";
        assert_eq!(
            gfm(md),
            serde_json::json!([{"t": "Para", "c": [
                {"t": "Str", "c": "Text."},
                {"t": "Note", "c": [{"t": "Para", "c": [{"t": "Str", "c": "body."}]}]},
            ]}])
        );
        assert_eq!(
            doc(md),
            serde_json::json!([{"t": "Para", "c": [
                {"t": "Str", "c": "Text."},
                {"t": "Link", "c": [["", [], []], [{"t": "Str", "c": "^1"}], ["body.", ""]]},
            ]}])
        );
    }

    #[test]
    fn a_reference_with_no_definition_stays_literal_and_one_body_never_recurses() {
        // Undefined: pandoc keeps the whole run as one `Str`.
        assert_eq!(
            gfm("Text.[^missing]\n"),
            serde_json::json!([{"t": "Para", "c": [{"t": "Str", "c": "Text.[^missing]"}]}])
        );
        // A reference inside a body resolves to nothing, exactly as pandoc
        // leaves it — and because bodies never resolve references, the
        // self-referential `[^1]: see [^1]` that exhausts pandoc's memory
        // terminates here.
        assert_eq!(
            gfm("a[^1]\n\n[^1]: outer[^2]\n\n[^2]: inner\n"),
            serde_json::json!([{"t": "Para", "c": [
                {"t": "Str", "c": "a"},
                {"t": "Note", "c": [{"t": "Para", "c": [
                    {"t": "Str", "c": "outer"}, {"t": "Str", "c": ""},
                ]}]},
            ]}])
        );
        assert!(read_gfm("a[^1]\n\n[^1]: see [^1]\n").is_ok());
    }

    #[test]
    fn gfm_constructs_are_off_in_commonmark() {
        // The same input, read both ways: only `read_gfm` sees a table.
        let table = "a|b\n-|-\n1|2\n";
        assert_eq!(doc(table)[0]["t"], "Para");
        assert_eq!(gfm(table)[0]["t"], "Table");
        assert_eq!(doc("~~x~~\n")[0]["c"][0]["t"], "Str");
        assert_eq!(gfm("~~x~~\n")[0]["c"][0]["t"], "Strikeout");
        assert_eq!(doc("# a\n")[0]["c"][1][0], "");
        assert_eq!(gfm("# a\n")[0]["c"][1][0], "a");
    }

    #[test]
    fn table_rows_are_padded_and_truncated_to_the_column_count() {
        let table = &gfm("a|b\n-|-\n1\n1|2|3\n")[0]["c"];
        // Two colspecs, one head row, one body holding both data rows.
        assert_eq!(table[2].as_array().unwrap().len(), 2);
        let rows = &table[4][0][3];
        assert_eq!(rows[0][1].as_array().unwrap().len(), 2);
        assert_eq!(rows[1][1].as_array().unwrap().len(), 2);
        // The padded cell holds no blocks at all, as pandoc's does.
        assert_eq!(rows[0][1][1][4], serde_json::json!([]));
    }

    #[test]
    fn table_alignment_comes_from_the_delimiter_row() {
        let colspecs = &gfm("a|b|c|d\n:-|:-:|-:|-\n1|2|3|4\n")[0]["c"][2];
        let names: Vec<&str> = colspecs
            .as_array()
            .unwrap()
            .iter()
            .map(|c| c[0]["t"].as_str().unwrap())
            .collect();
        assert_eq!(names, ["AlignLeft", "AlignCenter", "AlignRight", "AlignDefault"]);
    }

    #[test]
    fn task_items_become_ballot_boxes_in_bullet_lists_only() {
        assert_eq!(
            gfm("- [ ] a\n- [x] b\n"),
            serde_json::json!([{"t": "BulletList", "c": [
                [{"t": "Plain", "c": [{"t": "Str", "c": "\u{2610}"}, {"t": "Space"}, {"t": "Str", "c": "a"}]}],
                [{"t": "Plain", "c": [{"t": "Str", "c": "\u{2612}"}, {"t": "Space"}, {"t": "Str", "c": "b"}]}]
            ]}])
        );
        // Pandoc has no task items in ordered lists, so comrak's are
        // written back as the literal brackets it consumed.
        assert_eq!(
            gfm("1. [x] a\n")[0]["c"][1][0][0]["c"][0],
            serde_json::json!({"t": "Str", "c": "[x]"})
        );
    }

    #[test]
    fn a_plain_item_among_task_items_starts_a_new_list() {
        let blocks = gfm("- [ ] a\n- b\n- [ ] c\n");
        assert_eq!(blocks.as_array().unwrap().len(), 3);
        for block in blocks.as_array().unwrap() {
            assert_eq!(block["t"], "BulletList");
            assert_eq!(block["c"].as_array().unwrap().len(), 1);
        }
        // An ordered list has no task items, so it is never split.
        assert_eq!(gfm("1. [ ] a\n2. b\n").as_array().unwrap().len(), 1);
    }

    /// Where this reader deliberately follows GitHub's `cmark-gfm` — the
    /// implementation the format is named after — rather than pandoc's
    /// stricter `commonmark-hs`. None of these can go in the corpus, which
    /// is scored against pandoc; pinning them here is what keeps a future
    /// change from moving one by accident. Each is listed in
    /// `COMPATIBILITY.md` with the pandoc output beside it.
    #[test]
    fn deliberate_divergences_from_pandoc_hold() {
        // A single tilde marks strikeout on GitHub; pandoc wants two.
        assert_eq!(gfm("~x~\n")[0]["c"][0]["t"], "Strikeout");
        // A pipe table may interrupt a paragraph; pandoc keeps one `Para`.
        let interrupted = gfm("Some text:\n| a | b |\n|---|---|\n");
        assert_eq!(interrupted.as_array().unwrap().len(), 2);
        assert_eq!(interrupted[1]["t"], "Table");
        // A plain line after a row is another row, not a new paragraph.
        assert_eq!(gfm("| a |\n|---|\n| 1 |\nlazy\n").as_array().unwrap().len(), 1);
        // An autolink inside link text stays text; pandoc nests a `Link`
        // inside a `Link`, which no markdown grammar permits.
        assert_eq!(
            gfm("[http://e.com](http://e.com)\n")[0]["c"][0]["c"][1][0],
            serde_json::json!({"t": "Str", "c": "http://e.com"})
        );
        // The link text of a `mailto:` autolink keeps its scheme; pandoc
        // splits the scheme off into a preceding `Str`.
        assert_eq!(
            gfm("mailto:x@e.com\n")[0]["c"][0]["c"][1][0],
            serde_json::json!({"t": "Str", "c": "mailto:x@e.com"})
        );
        // `www.` autolinks only at a word boundary; pandoc takes it after
        // a dot as well.
        assert_eq!(gfm("a.www.e.com\n")[0]["c"][0]["t"], "Str");
        // Pandoc's `gfm` bundles a YAML metadata block; this does not.
        assert_eq!(gfm("---\n---\n")[0]["t"], "HorizontalRule");
    }

    #[test]
    fn heading_identifiers_follow_githubs_slug() {
        assert_eq!(ids("# Foo, Bar & Baz!\n"), ["foo-bar--baz"]);
        assert_eq!(ids("# a_b-c.d\n"), ["a_b-cd"]);
        assert_eq!(ids("# \u{dc}n\u{ef}code \u{c4}\n"), ["\u{fc}n\u{ef}code-\u{e4}"]);
        assert_eq!(ids("# `code` *em*\n"), ["code-em"]);
        // Breaks are spaces, images contribute their alt text, and
        // footnotes contribute nothing.
        assert_eq!(ids("A b\nc d\n===\n"), ["a-b-c-d"]);
        assert_eq!(ids("# ![alt](x) t\n"), ["alt-t"]);
        // Any whitespace, not just the ASCII space.
        assert_eq!(ids("# a\u{a0}b\n"), ["a-b"]);
    }

    /// A YAML block scalar, which used to be a refusal — and a refusal
    /// is the worst outcome, because the whole document stops
    /// converting. All four chomping indicators measured.
    #[test]
    fn a_block_scalar_is_read() {
        let meta = |md: &str| {
            serde_json::to_value(read_pandoc_markdown(md).expect("convertible"))
                .unwrap()["meta"]
                .clone()
        };
        // `|` keeps its line breaks and is `MetaBlocks`.
        assert_eq!(
            meta("---\nabstract: |\n  one\n  two\n---\n\nx\n")["abstract"],
            serde_json::json!({"t": "MetaBlocks", "c": [{"t": "Para", "c": [
                {"t": "Str", "c": "one"}, {"t": "SoftBreak"}, {"t": "Str", "c": "two"}
            ]}]})
        );
        // `>` folds them into spaces.
        assert_eq!(
            meta("---\nk: >\n  one\n  two\n---\n\nx\n")["k"],
            serde_json::json!({"t": "MetaBlocks", "c": [{"t": "Para", "c": [
                {"t": "Str", "c": "one"}, {"t": "Space"}, {"t": "Str", "c": "two"}
            ]}]})
        );
        // A stripping indicator leaves no trailing newline, and pandoc
        // reads that as inlines rather than blocks.
        assert_eq!(
            meta("---\nk: |-\n  one\n---\n\nx\n")["k"],
            serde_json::json!({"t": "MetaInlines", "c": [{"t": "Str", "c": "one"}]})
        );
        // A blank line inside one still separates paragraphs.
        assert_eq!(
            meta("---\nk: |\n  one\n\n  two\n---\n\nx\n")["k"]["c"]
                .as_array()
                .expect("blocks")
                .len(),
            2
        );
    }

    /// `[text]{#id}` is a `Span`, and only pandoc's dialect reads one.
    #[test]
    fn a_bracketed_span_is_a_span() {
        assert_eq!(
            pmd("[text]{#id .cls k=v}\n"),
            serde_json::json!([{"t": "Para", "c": [{"t": "Span", "c": [
                ["id", ["cls"], [["k", "v"]]], [{"t": "Str", "c": "text"}]
            ]}]}])
        );
        // The text between the brackets is markdown, and a value may be
        // quoted so it can hold a space — which `smart` has already made
        // curly by the time the attributes are read.
        assert_eq!(
            pmd("[a *b*]{k=\"x y\"}\n"),
            serde_json::json!([{"t": "Para", "c": [{"t": "Span", "c": [
                ["", [], [["k", "x y"]]],
                [{"t": "Str", "c": "a"}, {"t": "Space"},
                 {"t": "Emph", "c": [{"t": "Str", "c": "b"}]}]
            ]}]}])
        );
        // `.smallcaps` alone is a `SmallCaps`, and among others it is
        // one inside the span.
        assert_eq!(
            pmd("[t]{.smallcaps}\n"),
            serde_json::json!([{"t": "Para", "c": [
                {"t": "SmallCaps", "c": [{"t": "Str", "c": "t"}]}
            ]}])
        );
        assert_eq!(
            pmd("[t]{.smallcaps #i}\n"),
            serde_json::json!([{"t": "Para", "c": [{"t": "Span", "c": [
                ["i", [], []], [{"t": "SmallCaps", "c": [{"t": "Str", "c": "t"}]}]
            ]}]}])
        );
        // A malformed attribute list is not a span at all, and the
        // bracket that opened nothing does not cut the word in two.
        assert_eq!(
            pmd("[t]{foo}\n"),
            serde_json::json!([{"t": "Para", "c": [{"t": "Str", "c": "[t]{foo}"}]}])
        );
        // Not `gfm`.
        assert_eq!(
            gfm("[t]{#i}\n"),
            serde_json::json!([{"t": "Para", "c": [{"t": "Str", "c": "[t]{#i}"}]}])
        );
    }

    /// Pandoc trims a code span; `CommonMark` strips at most one space
    /// and only when both ends have one. ASCII whitespace only.
    #[test]
    fn a_code_span_is_trimmed_in_pandocs_dialect() {
        let code = |literal: &str| {
            serde_json::json!([{"t": "Para", "c": [
                {"t": "Code", "c": [["", [], []], literal]}
            ]}])
        };
        assert_eq!(pmd("` a`\n"), code("a"));
        assert_eq!(pmd("`a `\n"), code("a"));
        assert_eq!(pmd("`  ``  `\n"), code("``"));
        assert_eq!(pmd("` `\n"), code(""));
        // A non-breaking space is content, not padding.
        assert_eq!(pmd("`\u{a0}b\u{a0}`\n"), code("\u{a0}b\u{a0}"));
        // `gfm` keeps `CommonMark`'s rule.
        assert_eq!(gfm("` a`\n"), code(" a"));
    }

    /// Pandoc's own dialect does not use GitHub's slug, and the three
    /// ways it differs are each probed against `pandoc -f markdown`.
    #[test]
    fn pandocs_dialect_slugs_its_own_way() {
        let ids = |md: &str| {
            let doc = read_pandoc_markdown(md).expect("convertible");
            doc.blocks
                .iter()
                .filter_map(|b| match b {
                    Block::Header(_, attr, _) => Some(attr.identifier.clone()),
                    _ => None,
                })
                .collect::<Vec<_>>()
        };
        // A run of whitespace is one hyphen, not one per character.
        assert_eq!(ids("# foo ### b\n"), ["foo-b"]);
        assert_eq!(ids("# Punctuation, & symbols!\n"), ["punctuation-symbols"]);
        // `.` is kept, where GitHub's slug drops it.
        assert_eq!(ids("# a.b\n"), ["a.b"]);
        assert_eq!(slug("a.b", Dialect::Gfm), "ab");
        // Everything before the first letter goes, and a heading that
        // leaves nothing is called `section`.
        assert_eq!(ids("# 1. x\n"), ["x"]);
        assert_eq!(ids("# 123\n"), ["section"]);
    }

    /// Which items get a box is opposite in the two dialects, on both
    /// counts, and each side was measured against its own reader.
    #[test]
    fn the_two_dialects_disagree_about_task_items() {
        let literal = serde_json::json!([
            {"t": "Str", "c": "["}, {"t": "Space"}, {"t": "Str", "c": "]"},
            {"t": "Space"}, {"t": "Str", "c": "a"}
        ]);
        let boxed = serde_json::json!([
            {"t": "Str", "c": "\u{2610}"}, {"t": "Space"}, {"t": "Str", "c": "a"}
        ]);
        // An **ordered** list has task items in pandoc's markdown and
        // none in `gfm`.
        assert_eq!(pmd("1. [ ] a\n")[0]["c"][1][0][0]["c"], boxed);
        assert_eq!(gfm("1. [ ] a\n")[0]["c"][1][0][0]["c"], literal);
        // An **empty** one is the other way round.
        assert_eq!(
            pmd("- [ ]\n")[0]["c"][0][0]["c"],
            serde_json::json!([{"t": "Str", "c": "["}, {"t": "Space"}, {"t": "Str", "c": "]"}])
        );
        assert_eq!(
            gfm("- [ ]\n")[0]["c"][0][0]["c"],
            serde_json::json!([{"t": "Str", "c": "\u{2610}"}])
        );
        // And a plain bullet task item is a box in both.
        assert_eq!(pmd("- [ ] a\n")[0]["c"][0][0]["c"], boxed);
        assert_eq!(gfm("- [ ] a\n")[0]["c"][0][0]["c"], boxed);
    }

    #[test]
    fn an_empty_task_item_has_no_space_after_its_box() {
        assert_eq!(
            gfm("- [ ]\n- [x] a\n"),
            serde_json::json!([{"t": "BulletList", "c": [
                [{"t": "Plain", "c": [{"t": "Str", "c": "\u{2610}"}]}],
                [{"t": "Plain", "c": [{"t": "Str", "c": "\u{2612}"}, {"t": "Space"}, {"t": "Str", "c": "a"}]}]
            ]}])
        );
    }

    #[test]
    fn each_run_of_a_split_list_works_out_its_own_tightness() {
        // The list is loose in the source, but every run it splits into
        // holds one item and no blank line, so pandoc calls each tight.
        let blocks = gfm("- [ ] a\n\n- b\n\n- [x] c\n");
        assert_eq!(blocks.as_array().unwrap().len(), 3);
        for block in blocks.as_array().unwrap() {
            assert_eq!(block["c"][0][0]["t"], "Plain");
        }
        // A run that is loose on its own stays loose.
        assert_eq!(gfm("- [ ] a\n\n- [ ] b\n")[0]["c"][0][0]["t"], "Para");
    }

    #[test]
    fn duplicate_identifiers_count_per_name_not_per_document() {
        assert_eq!(ids("# a\n\n# a\n\n# a\n"), ["a", "a-1", "a-2"]);
        // The counter is per base name, so a heading whose own slug is
        // already taken repeats it — probed against pandoc, which does
        // the same rather than searching for a free name.
        assert_eq!(ids("# a\n\n# a\n\n# a-1\n\n# a\n"), ["a", "a-1", "a-1", "a-2"]);
        // Headings inside containers share the document's counter.
        assert_eq!(ids("# a\n"), ["a"]);
        assert_eq!(
            read_gfm("# a\n\n> # a\n").unwrap().blocks[1],
            Block::BlockQuote(vec![Block::Header(
                1,
                Attr { identifier: "a-1".to_owned(), ..Attr::default() },
                vec![Inline::Str("a".to_owned())],
            )])
        );
    }

    #[test]
    fn pathological_nesting_is_refused_not_truncated() {
        let deep = ">".repeat(MAX_NESTING + 1);
        assert_eq!(read_commonmark(&deep), Err(Error::TooDeeplyNested));
        assert_eq!(read_gfm(&deep), Err(Error::TooDeeplyNested));
        // A document at the limit still converts.
        let shallow = ">".repeat(10);
        assert!(read_commonmark(&shallow).is_ok());
        assert!(read_gfm(&shallow).is_ok());
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

    /// `smart` is pandoc's `markdown` and nobody else's, and the part
    /// comrak does not do is the pairing: it writes the characters and
    /// stops, while pandoc makes a *pair* a `Quoted` element. Every
    /// expectation here was read off `pandoc -f markdown -t json`.
    #[test]
    fn smart_punctuation_is_read_the_way_pandoc_reads_it() {
        let quoted = |kind: &str, inner: serde_json::Value| {
            serde_json::json!({"t": "Quoted", "c": [{"t": kind}, inner]})
        };
        let str_ = |s: &str| serde_json::json!({"t": "Str", "c": s});
        let space = serde_json::json!({"t": "Space"});

        // Dashes and the ellipsis are characters; an apostrophe is one too.
        assert_eq!(
            pmd("a--b a---b a...b don't\n"),
            serde_json::json!([{"t": "Para", "c": [
                str_("a\u{2013}b"), space, str_("a\u{2014}b"), space,
                str_("a\u{2026}b"), space, str_("don\u{2019}t")
            ]}])
        );

        // A pair is an element, and it nests.
        assert_eq!(
            pmd("\"a 'b' c\"\n"),
            serde_json::json!([{"t": "Para", "c": [quoted("DoubleQuote", serde_json::json!([
                str_("a"), space,
                quoted("SingleQuote", serde_json::json!([str_("b")])),
                space, str_("c")
            ]))]}])
        );

        // An opener with no closer is the character it is, and so is a
        // mark with a space on both sides — which comrak calls opening
        // and pandoc calls closing.
        assert_eq!(
            pmd("a \"unclosed\n"),
            serde_json::json!([{"t": "Para", "c": [
                str_("a"), space, str_("\u{201c}unclosed")
            ]}])
        );
        assert_eq!(
            pmd("a \" b\n"),
            serde_json::json!([{"t": "Para", "c": [
                str_("a"), space, str_("\u{201d}"), space, str_("b")
            ]}])
        );

        // Nothing of this is `gfm` or `commonmark`, measured on both.
        assert_eq!(
            gfm("don't a--b\n"),
            serde_json::json!([{"t": "Para", "c": [str_("don't"), space, str_("a--b")]}])
        );
        assert_eq!(
            doc("don't a--b\n"),
            serde_json::json!([{"t": "Para", "c": [str_("don't"), space, str_("a--b")]}])
        );
    }

    /// `implicit_figures`, five shapes off `pandoc -f markdown -t json`.
    #[test]
    fn an_image_alone_in_a_paragraph_is_a_figure() {
        let image = |alt: &str| serde_json::json!({"t": "Image", "c": [
            ["", [], []], [{"t": "Str", "c": alt}], ["s.png", ""]
        ]});
        let figure = |alt: &str| serde_json::json!({"t": "Figure", "c": [
            ["", [], []],
            [null, [{"t": "Plain", "c": [{"t": "Str", "c": alt}]}]],
            [{"t": "Plain", "c": [image(alt)]}]
        ]});

        assert_eq!(pmd("![a](s.png)\n"), serde_json::json!([figure("a")]));
        // A tight list item's paragraph has already become a figure by
        // the time the list is tightened.
        assert_eq!(
            pmd("- ![a](s.png)\n"),
            serde_json::json!([{"t": "BulletList", "c": [[figure("a")]]}])
        );
        // Empty alt text is not a caption, so it stays a paragraph.
        assert_eq!(
            pmd("![](s.png)\n"),
            serde_json::json!([{"t": "Para", "c": [
                {"t": "Image", "c": [["", [], []], [], ["s.png", ""]]}
            ]}])
        );
        // An image beside anything else is not one either, emphasis
        // around it included.
        assert_eq!(
            pmd("![a](s.png) ![b](s.png)\n"),
            serde_json::json!([{"t": "Para", "c": [image("a"), {"t": "Space"}, image("b")]}])
        );
        assert_eq!(
            pmd("**![a](s.png)**\n"),
            serde_json::json!([{"t": "Para", "c": [{"t": "Strong", "c": [image("a")]}]}])
        );
        // Not `gfm`, and not `commonmark`.
        assert_eq!(
            gfm("![a](s.png)\n"),
            serde_json::json!([{"t": "Para", "c": [image("a")]}])
        );
    }

    /// `link_attributes`, `inline_code_attributes` and `inline_footnotes`
    /// — three more constructs `pandoc -f markdown` reads and plain
    /// `commonmark` does not. comrak parses each; what is new is reading the
    /// attributes it hangs on the node.
    #[test]
    fn attributes_and_inline_notes_are_read() {
        assert_eq!(
            pmd("[a](b){#i .c k=v}\n"),
            serde_json::json!([{"t": "Para", "c": [{"t": "Link", "c": [
                ["i", ["c"], [["k", "v"]]], [{"t": "Str", "c": "a"}], ["b", ""]
            ]}]}])
        );
        assert_eq!(
            pmd("`code`{.rust}\n"),
            serde_json::json!([{"t": "Para", "c": [
                {"t": "Code", "c": [["", ["rust"], []], "code"]}
            ]}])
        );
        // And the same after a fence, where the whole brace used to be
        // taken for the language name.
        assert_eq!(
            pmd("``` {#i .rust k=v}\nx\n```\n"),
            serde_json::json!([{"t": "CodeBlock", "c": [["i", ["rust"], [["k", "v"]]], "x"]}])
        );
        assert_eq!(
            pmd("```rust\nx\n```\n"),
            serde_json::json!([{"t": "CodeBlock", "c": [["", ["rust"], []], "x"]}])
        );
        // An image's identifier still moves to the figure around it.
        assert_eq!(
            pmd("![a](x){#i .c}\n"),
            serde_json::json!([{"t": "Figure", "c": [
                ["i", [], []],
                [null, [{"t": "Plain", "c": [{"t": "Str", "c": "a"}]}]],
                [{"t": "Plain", "c": [{"t": "Image", "c": [
                    ["", ["c"], []], [{"t": "Str", "c": "a"}], ["x", ""]
                ]}]}]
            ]}])
        );
        // An autolink still takes its class, which is the same field.
        assert_eq!(
            pmd("<http://x.example>\n"),
            serde_json::json!([{"t": "Para", "c": [{"t": "Link", "c": [
                ["", ["uri"], []],
                [{"t": "Str", "c": "http://x.example"}],
                ["http://x.example", ""]
            ]}]}])
        );
        // A footnote definition closes where the next one opens, so its
        // last paragraph is a `Para` — reading the next source line made
        // it a `Plain`.
        assert_eq!(
            pmd("a[^1] b[^2]\n\n[^1]: one\n[^2]: two\n")[0]["c"][1]["c"][0]["t"],
            "Para"
        );
        assert_eq!(
            pmd("a^[note]\n"),
            serde_json::json!([{"t": "Para", "c": [
                {"t": "Str", "c": "a"},
                {"t": "Note", "c": [{"t": "Para", "c": [{"t": "Str", "c": "note"}]}]}
            ]}])
        );
        // None of it is `gfm`.
        assert_eq!(
            gfm("`code`{.rust}\n"),
            serde_json::json!([{"t": "Para", "c": [
                {"t": "Code", "c": [["", [], []], "code"]},
                {"t": "Str", "c": "{.rust}"}
            ]}])
        );
    }

    /// Raw HTML in pandoc's dialect. Every expectation is off
    /// `pandoc -f markdown -t json`; `diff-pandoc-md` over the spec is
    /// the wide check and these are the rules it does not reach.
    #[test]
    fn raw_html_is_split_at_block_level_tags() {
        let raw = |html: &str| serde_json::json!({"t": "RawBlock", "c": ["html", html]});

        // One raw block per tag, and the cell text between them.
        assert_eq!(
            pmd("<table><tr><td>\nhi\n</td></tr></table>\n"),
            serde_json::json!([
                raw("<table>"), raw("<tr>"), raw("<td>"),
                {"t": "Plain", "c": [{"t": "Str", "c": "hi"}]},
                raw("</td>"), raw("</tr>"), raw("</table>")
            ])
        );

        // The markdown between them is read as markdown, and a list is
        // a list.
        assert_eq!(
            pmd("<td>\n- x\n</td>\n"),
            serde_json::json!([
                raw("<td>"),
                {"t": "BulletList", "c": [[{"t": "Plain", "c": [{"t": "Str", "c": "x"}]}]]},
                raw("</td>")
            ])
        );

        // `<pre>` holds no markdown, and keeps no trailing newline.
        assert_eq!(
            pmd("<pre>\n**not md**\n</pre>\n"),
            serde_json::json!([raw("<pre>\n**not md**\n</pre>")])
        );

        // A tag pandoc does not call block-level leaves a paragraph, and
        // the markdown inside it is still read.
        assert_eq!(
            pmd("<foo>\n*bar*\n</foo>\n"),
            serde_json::json!([{"t": "Para", "c": [
                {"t": "RawInline", "c": ["html", "<foo>"]},
                {"t": "SoftBreak"},
                {"t": "Emph", "c": [{"t": "Str", "c": "bar"}]},
                {"t": "SoftBreak"},
                {"t": "RawInline", "c": ["html", "</foo>"]}
            ]}])
        );

        // `<warning>` is not HTML at all — it is `DocBook`, and pandoc
        // knows it. `<danger>` is neither.
        assert_eq!(
            pmd("<warning>\nx\n</warning>\n"),
            serde_json::json!([
                raw("<warning>"),
                {"t": "Plain", "c": [{"t": "Str", "c": "x"}]},
                raw("</warning>")
            ])
        );
        assert_eq!(pmd("<danger>\nx\n</danger>\n")[0]["t"], "Para");

        // `native_divs` and `native_spans`: the element's own attributes,
        // `id` and `class` taken out of them.
        assert_eq!(
            pmd("<div class=\"a b\" id=\"i\" data-k=\"v\">\nx\n</div>\n"),
            serde_json::json!([{"t": "Div", "c": [
                ["i", ["a", "b"], [["data-k", "v"]]],
                [{"t": "Para", "c": [{"t": "Str", "c": "x"}]}]
            ]}])
        );
        assert_eq!(
            pmd("a <span class=\"y\">z</span>\n"),
            serde_json::json!([{"t": "Para", "c": [
                {"t": "Str", "c": "a"}, {"t": "Space"},
                {"t": "Span", "c": [["", ["y"], []], [{"t": "Str", "c": "z"}]]}
            ]}])
        );
        // An unmatched closer stays raw; an unclosed opener takes the rest.
        assert_eq!(pmd("</div>\n\nx\n")[0], raw("</div>"));
        assert_eq!(pmd("<div>\n\nx\n")[0]["t"], "Div");

        // A declaration is the literal text it is written with, and only
        // a comment among the `<!` forms is raw.
        assert_eq!(
            pmd("<!DOCTYPE html>\n"),
            serde_json::json!([{"t": "Para", "c": [
                {"t": "Str", "c": "<!DOCTYPE"}, {"t": "Space"}, {"t": "Str", "c": "html>"}
            ]}])
        );
        assert_eq!(pmd("<!-- c -->\n"), serde_json::json!([raw("<!-- c -->")]));

        // None of it is `gfm`: there a run of HTML is one opaque block.
        assert_eq!(
            gfm("<td>\nhi\n</td>\n"),
            serde_json::json!([raw("<td>\nhi\n</td>\n")])
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

    /// The document from `ADOPTION.md` §3.2, which was five separate
    /// losses under `-f markdown`: the metadata block became a thematic
    /// break and a heading in the *body*, and four constructs came through
    /// as the literal characters they are written with.
    #[test]
    fn the_five_losses_are_read() {
        let document = read_pandoc_markdown(concat!(
            "---\ntitle: A report\nauthor: Someone\n---\n\n",
            "# Heading {#custom-id .fancy}\n\n",
            "Text with a footnote.[^1]\n\n[^1]: The note body.\n\n",
            "Term\n:   Definition of the term.\n\nH~2~O and E=mc^2^.\n",
        ))
        .expect("readable");
        assert!(document.meta.contains_key("title"), "{:?}", document.meta);
        assert!(document.meta.contains_key("author"), "{:?}", document.meta);
        let Some(Block::Header(1, attr, _)) = document.blocks.first() else {
            panic!("{:?}", document.blocks.first())
        };
        assert_eq!(attr.identifier, "custom-id");
        assert_eq!(attr.classes, ["fancy"]);
        assert!(
            document.blocks.iter().any(|b| matches!(b, Block::DefinitionList(_))),
            "{:?}",
            document.blocks
        );
        // The same document under `CommonMark`, which is what the other
        // two readers must go on doing: no metadata, and the block is a
        // rule and a heading in the body.
        let commonmark = read_commonmark("---\ntitle: A report\n---\n\ntext\n").expect("readable");
        assert!(commonmark.meta.is_empty());
        assert!(matches!(commonmark.blocks.first(), Some(Block::HorizontalRule)));
    }

    /// A metadata block outside the subset is an **error**, not a guess.
    /// A wrong title is invisible in the output, which is the one place
    /// where refusing beats approximating.
    #[test]
    fn metadata_outside_the_subset_is_refused() {
        for source in [
            "---\nauthor:\n  name: Ann\n---\n\nx\n",   // a nested map
            "---\ntags: [a, b]\n---\n\nx\n",             // a flow sequence
            "---\nanchor: &a value\n---\n\nx\n",
            // A marker this does not read is still refused, and is not
            // mistaken for a block scalar.
            "---\nk: |2\n  indented\n---\n\nx\n",
        ] {
            assert!(
                matches!(read_pandoc_markdown(source), Err(Error::Metadata(_))),
                "{source:?} was not refused"
            );
        }
        // And the subset itself still reads.
        assert!(read_pandoc_markdown("---\ntitle: T\nlist:\n  - a\n  - b\n---\n\nx\n").is_ok());
    }

    /// A heading that names itself keeps its name, and the name it took
    /// still counts toward uniquing the ones that are slugged.
    #[test]
    fn an_explicit_identifier_wins_and_still_uniques() {
        let document =
            read_pandoc_markdown("# Same {#same}\n\n# Same\n\n# Same\n").expect("readable");
        let ids: Vec<&str> = document
            .blocks
            .iter()
            .filter_map(|b| match b {
                Block::Header(_, attr, _) => Some(attr.identifier.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(ids, ["same", "same-1", "same-2"]);
    }
}
