//! EPUB reader and writer producing the ferrodoc (pandoc-compatible) AST.
//!
//! [`read_epub`] parses an `.epub` byte stream and maps it to the same AST
//! `pandoc -f epub -t json` produces (differentially verified by
//! `ferrodoc-harness diff-epub`); [`write_epub`] emits a package a reading
//! system opens, checked by `epubcheck` as well as by round trip.
//!
//! An EPUB is a zip whose content documents are XHTML, so **the reader
//! that does the work already exists** — this crate is the packaging
//! around `ferrodoc-html`. What it adds is the spine, and four behaviours
//! that are pandoc's rather than the format's, each measured against the
//! 3.8.2.1 binary:
//!
//! - **every spine item contributes an anchor**, `Para [Span(name, …) []]`,
//!   named for the *basename* of its href. It marks where a file began, and
//!   it is emitted even for an item whose content is skipped;
//! - **`linear="no"` items contribute nothing at all**, not even their
//!   anchor;
//! - a **title page contributes its anchor and nothing else**, whether or
//!   not it is linear: it repeats the title and byline as page furniture,
//!   and keeping it would grow a duplicate heading on every round trip;
//! - **every identifier is prefixed with the file it came from**
//!   (`ch001.xhtml_one`), because two chapters may each define `#intro` and
//!   a book is one document once it is read;
//! - reading order is the **spine**, not the manifest and not the order of
//!   entries in the zip. A reading system follows the spine, so a reader
//!   that walked the zip would produce a book in file-name order, which is
//!   right often enough to be dangerous.
//!
//! Known gaps, deliberate and unfixed:
//!
//! - media overlays, encryption, fixed-layout metadata and the EPUB 2
//!   `toc.ncx` are not read; the spine is authoritative in both versions,
//!   and the navigation document is written but not interpreted;
//! - a spine item that is not XHTML (an SVG page) contributes its anchor
//!   and nothing else;
//! - conversion is bounded the way every reader here is: a hostile archive
//!   is refused rather than exhausting memory.

mod write;

pub use write::{write_epub, write_epub_with_media};

use ferrodoc_ast::{Attr, Block, Inline, MetaValue, Pandoc};
use ferrodoc_docx::xml::{self, Node};
use std::collections::HashMap;
use std::io::Read;

/// An error reading an EPUB file.
#[derive(Debug)]
pub enum Error {
    /// The container is not a readable zip archive.
    Zip(String),
    /// An XML part failed to parse.
    Xml(String),
    /// The archive declares more decompressed content than one its size
    /// may hold — see `ferrodoc_docx::archive`.
    TooLarge {
        /// What the archive's own headers say it decompresses to.
        declared: u64,
        /// What an archive that size is allowed.
        budget: u64,
    },
    /// A required part is missing from the archive.
    MissingPart(&'static str),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Zip(e) => write!(f, "not a readable epub (zip) archive: {e}"),
            Error::Xml(e) => write!(f, "malformed XML part: {e}"),
            Error::TooLarge { declared, budget } => write!(
                f,
                "archive declares {declared} bytes of content, more than the \
                 {budget} an archive this size may decompress to"
            ),
            Error::MissingPart(p) => write!(f, "missing required part: {p}"),
        }
    }
}

impl std::error::Error for Error {}

impl From<ferrodoc_docx::Error> for Error {
    fn from(e: ferrodoc_docx::Error) -> Self {
        match e {
            ferrodoc_docx::Error::Zip(e) => Error::Zip(e),
            ferrodoc_docx::Error::Xml(e) => Error::Xml(e),
            ferrodoc_docx::Error::TooLarge { declared, budget } => {
                Error::TooLarge { declared, budget }
            }
            ferrodoc_docx::Error::MissingPart(p) => Error::MissingPart(p),
        }
    }
}

/// The image bytes a document carries, keyed by the URL its AST names them
/// by — the same shape the other readers here produce.
pub type Media = HashMap<String, Vec<u8>>;

/// Read an EPUB into a [`Pandoc`] AST equivalent to pandoc's epub reader
/// output.
///
/// # Errors
///
/// [`Error::Zip`] if the bytes are not a readable archive, [`Error::Xml`]
/// if a required part is malformed, [`Error::MissingPart`] if the
/// container or package document is absent.
pub fn read_epub(bytes: &[u8]) -> Result<Pandoc, Error> {
    read(bytes, false).map(|(doc, _)| doc)
}

/// Read an EPUB together with the bytes of every image it embeds.
///
/// # Errors
///
/// The same as [`read_epub`]. A part that is named but missing from the
/// archive is left out of the bag rather than failing the read.
pub fn read_epub_with_media(bytes: &[u8]) -> Result<(Pandoc, Media), Error> {
    read(bytes, true)
}

fn read(bytes: &[u8], want_media: bool) -> Result<(Pandoc, Media), Error> {
    let mut archive =
        zip::ZipArchive::new(std::io::Cursor::new(bytes)).map_err(|e| Error::Zip(e.to_string()))?;
    ferrodoc_docx::declared_within_budget(&mut archive, bytes.len())?;
    let mut text = |name: &str| -> Option<String> {
        let mut file = archive.by_name(name).ok()?;
        let mut s = String::new();
        file.read_to_string(&mut s).ok()?;
        Some(s)
    };

    // `META-INF/container.xml` is the one entry whose name is fixed; it
    // names the package document, which names everything else.
    let container = text("META-INF/container.xml").ok_or(Error::MissingPart("META-INF/container.xml"))?;
    let opf_path = root_file(&xml::parse(&container)?).ok_or(Error::MissingPart("rootfile"))?;
    let opf = text(&opf_path).ok_or(Error::MissingPart("the package document"))?;
    let package = xml::parse(&opf)?;

    // Hrefs in the package document are relative to *its* directory, which
    // is usually `EPUB/` or `OEBPS/` and occasionally the archive root.
    let base = opf_path.rsplit_once('/').map_or("", |(dir, _)| dir);
    let manifest = manifest(&package);
    let mut blocks = Vec::new();
    for item in spine(&package) {
        let Some(entry) = manifest.get(&item.idref) else { continue };
        // `linear="no"` is outside the reading order — a cover, or the
        // title page pandoc generates for a document with metadata — and
        // it contributes nothing at all, not even its anchor.
        if !item.linear {
            continue;
        }
        let name = basename(&entry.href);
        // The anchor marks where a file began.
        blocks.push(Block::Para(vec![Inline::Span(
            Box::new(Attr { identifier: name.clone(), ..Attr::default() }),
            Vec::new(),
        )]));
        let Some(source) = text(&join(base, &entry.path)) else { continue };
        let Ok(chapter) = ferrodoc_html::read_html_without_generated_identifiers(&source)
        else {
            continue;
        };
        let mut chapter = chapter.blocks;
        // Footnotes are reassembled *before* the identifiers move: the
        // reference still points at `#fn1` here, and prefixing would make
        // it name something that no longer exists.
        let notes = footnote_bodies(&source);
        inline_footnotes(&mut chapter, &notes);
        chapter.retain(|block| !is_footnotes_section(block));
        // A generated title page carries the title and byline again, as
        // page furniture rather than as content. Pandoc drops it and keeps
        // the anchor, so a `docx -> epub -> docx` trip does not grow a
        // duplicate heading each time.
        chapter.retain(|block| !is_titlepage(block));
        prefix_identifiers(&mut chapter, &name);
        // An href inside a chapter is relative to *that file*; the AST
        // names things relative to the package document. A picture two
        // directories in reaches its file as `../media/x.png`, and left
        // alone that is what the writer would be asked to resolve.
        let dir = entry.path.rsplit_once('/').map_or("", |(dir, _)| dir);
        resolve_targets(&mut chapter, dir);
        // A link into the book has to follow the identifiers, which have
        // just been prefixed: `#target` in ch001 now means
        // `#ch001.xhtml_target`, and `other.xhtml#target` means
        // `#other.xhtml_target`. Left alone, every cross-reference in the
        // book would point at nothing.
        rewrite_internal_links(&mut chapter, &name);
        blocks.append(&mut chapter);
    }

    let doc = Pandoc { meta: metadata(&package), blocks, ..Pandoc::default() };

    let mut media = Media::new();
    if want_media {
        let mut urls = Vec::new();
        collect_image_urls(&doc.blocks, &mut urls);
        for url in urls {
            if media.contains_key(&url) {
                continue;
            }
            // A part named but missing is not an error: the book is still
            // readable and the writer falls back to alt text.
            if let Ok(mut file) = archive.by_name(&join(base, &url)) {
                let mut bytes = Vec::new();
                if file.read_to_end(&mut bytes).is_ok() {
                    media.insert(url, bytes);
                }
            }
        }
    }
    Ok((doc, media))
}

/// The package document's path, from `META-INF/container.xml`.
fn root_file(container: &Node) -> Option<String> {
    container
        .child("rootfiles")?
        .children_named("rootfile")
        .find_map(|r| r.attr("full-path"))
        .map(str::to_owned)
}

/// One manifest entry: the href as written, and the archive entry it
/// names.
///
/// The two differ, and both are needed: a space is `%20` in the manifest
/// and a literal space in the zip, so the file is found by the decoded
/// path — while the anchor pandoc emits is named for the **raw** href.
struct Item {
    href: String,
    path: String,
}

/// One spine entry, in reading order.
struct SpineItem {
    idref: String,
    /// `linear="no"` marks something outside the reading order — a cover
    /// or a generated title page. It is skipped entirely.
    linear: bool,
}

fn manifest(package: &Node) -> HashMap<String, Item> {
    let mut out = HashMap::new();
    let Some(manifest) = package.child("manifest") else { return out };
    for item in manifest.children_named("item") {
        if let (Some(id), Some(href)) = (item.attr("id"), item.attr("href")) {
            out.insert(
                id.to_owned(),
                Item { href: href.to_owned(), path: percent_decode(href) },
            );
        }
    }
    out
}

fn spine(package: &Node) -> Vec<SpineItem> {
    package.child("spine").map_or_else(Vec::new, |spine| {
        spine
            .children_named("itemref")
            .filter_map(|r| {
                Some(SpineItem {
                    idref: r.attr("idref")?.to_owned(),
                    linear: r.attr("linear") != Some("no"),
                })
            })
            .collect()
    })
}

/// The Dublin Core metadata the package document carries.
///
/// Only the fields pandoc surfaces, under the names it gives them: an
/// EPUB's `dc:creator` is pandoc's `author`.
fn metadata(package: &Node) -> ferrodoc_ast::Meta {
    let mut meta = ferrodoc_ast::Meta::new();
    let Some(node) = package.child("metadata") else { return meta };
    for (element, field) in [
        ("title", "title"),
        ("creator", "author"),
        ("language", "language"),
        ("identifier", "identifier"),
        ("date", "date"),
        ("description", "description"),
        ("rights", "rights"),
        ("publisher", "publisher"),
    ] {
        let values: Vec<MetaValue> = node
            .children_named(element)
            .map(|e| MetaValue::MetaInlines(vec![Inline::Str(e.text())]))
            .filter(|v| !matches!(v, MetaValue::MetaInlines(i) if i == &[Inline::Str(String::new())]))
            .collect();
        match values.len() {
            0 => {}
            1 => {
                meta.insert(field.to_owned(), values.into_iter().next().expect("length one"));
            }
            // Several authors are a list — in **reverse** document order,
            // which is pandoc's, presumably from a fold that prepends.
            // Measured against a two-creator package rather than assumed.
            _ => {
                let reversed = values.into_iter().rev().collect();
                meta.insert(field.to_owned(), MetaValue::MetaList(reversed));
            }
        }
    }
    meta
}

/// Prefix every identifier in a chapter with the file it came from.
///
/// Two chapters may each define `#intro`, and once the spine is
/// concatenated the book is a single document in which they would collide.
fn prefix_identifiers(blocks: &mut [Block], file: &str) {
    fn attr(attr: &mut Attr, file: &str) {
        if !attr.identifier.is_empty() {
            attr.identifier = format!("{file}_{}", attr.identifier);
        }
    }
    fn inlines(list: &mut [Inline], file: &str) {
        for inline in list {
            match inline {
                Inline::Code(a, _) => attr(a, file),
                Inline::Span(a, inner)
                | Inline::Link(a, inner, _)
                | Inline::Image(a, inner, _) => {
                    attr(a, file);
                    inlines(inner, file);
                }
                Inline::Emph(inner)
                | Inline::Underline(inner)
                | Inline::Strong(inner)
                | Inline::Strikeout(inner)
                | Inline::Superscript(inner)
                | Inline::Subscript(inner)
                | Inline::SmallCaps(inner)
                | Inline::Quoted(_, inner)
                | Inline::Cite(_, inner) => inlines(inner, file),
                Inline::Note(blocks) => prefix_identifiers(blocks, file),
                _ => {}
            }
        }
    }
    for block in blocks {
        match block {
            Block::Header(_, a, list) => {
                attr(a, file);
                inlines(list, file);
            }
            Block::Div(a, inner) => {
                attr(a, file);
                prefix_identifiers(inner, file);
            }
            Block::CodeBlock(a, _) => attr(a, file),
            Block::Plain(list) | Block::Para(list) => inlines(list, file),
            Block::LineBlock(lines) => {
                for line in lines {
                    inlines(line, file);
                }
            }
            Block::BlockQuote(inner) => prefix_identifiers(inner, file),
            Block::BulletList(items) | Block::OrderedList(_, items) => {
                for item in items {
                    prefix_identifiers(item, file);
                }
            }
            Block::DefinitionList(entries) => {
                for (term, definitions) in entries {
                    inlines(term, file);
                    for definition in definitions {
                        prefix_identifiers(definition, file);
                    }
                }
            }
            Block::Figure(a, caption, inner) => {
                attr(a, file);
                prefix_identifiers(&mut caption.blocks, file);
                prefix_identifiers(inner, file);
            }
            Block::Table(table) => {
                attr(&mut table.attr, file);
                for row in table
                    .head
                    .rows
                    .iter_mut()
                    .chain(table.bodies.iter_mut().flat_map(|b| b.head.iter_mut().chain(&mut b.body)))
                    .chain(table.foot.rows.iter_mut())
                {
                    for cell in &mut row.cells {
                        prefix_identifiers(&mut cell.blocks, file);
                    }
                }
            }
            Block::HorizontalRule | Block::RawBlock(..) => {}
        }
    }
}

/// The body of every footnote in a chapter, by the identifier its
/// reference points at.
///
/// An EPUB keeps footnotes where the text is not: an `<aside
/// epub:type="footnote" id="fn1">` at the end of the file, with an `<a
/// epub:type="noteref" href="#fn1">` where the note belongs. Pandoc puts
/// them back together, so a note reads as a note rather than as a link to
/// the bottom of the chapter.
///
/// Scanned out of the source rather than taken from the parsed blocks,
/// because the HTML reader drops the `<aside>` wrapper — and with it the
/// identifier that says which note this is.
fn footnote_bodies(source: &str) -> HashMap<String, Vec<Block>> {
    let mut notes = HashMap::new();
    let mut rest = source;
    while let Some(start) = rest.find("<aside") {
        let after = &rest[start..];
        let Some(open_end) = after.find('>') else { break };
        let open = &after[..=open_end];
        let Some(inner_end) = matching_close(after, "aside") else {
            rest = &after[open_end..];
            continue;
        };
        let inner = &after[open_end + 1..inner_end];
        if open.contains(r#"epub:type="footnote""#)
            && let Some(id) = attribute_value(open, "id")
            && let Ok(body) = ferrodoc_html::read_html_without_generated_identifiers(inner)
        {
            let mut blocks = body.blocks;
            strip_backlinks(&mut blocks);
            notes.insert(id, blocks);
        }
        rest = &after[inner_end..];
    }
    notes
}

/// The offset of the `<` that opens `</name>` matching the element `text`
/// begins with, honouring nesting.
fn matching_close(text: &str, name: &str) -> Option<usize> {
    let open = format!("<{name}");
    let close = format!("</{name}");
    let (mut depth, mut at) = (0usize, 0usize);
    while at < text.len() {
        let next_open = text[at..].find(&open).map(|i| at + i);
        let next_close = text[at..].find(&close).map(|i| at + i);
        match (next_open, next_close) {
            (Some(o), Some(c)) if o < c => {
                depth += 1;
                at = o + open.len();
            }
            (_, Some(c)) => {
                depth -= 1;
                if depth == 0 {
                    return Some(c);
                }
                at = c + close.len();
            }
            _ => return None,
        }
    }
    None
}

/// The value of an attribute in a start tag, for the two spellings an
/// XHTML serializer uses.
fn attribute_value(tag: &str, name: &str) -> Option<String> {
    for quote in ['"', '\''] {
        let needle = format!("{name}={quote}");
        if let Some(at) = tag.find(&needle) {
            let rest = &tag[at + needle.len()..];
            if let Some(end) = rest.find(quote) {
                return Some(rest[..end].to_owned());
            }
        }
    }
    None
}

/// Drop the link back to the reference. It is navigation, not text, and
/// pandoc leaves it out of the note.
fn strip_backlinks(blocks: &mut [Block]) {
    fn inlines(list: &mut Vec<Inline>) {
        list.retain(|inline| !matches!(inline, Inline::Link(attr, ..)
            if attr.classes.iter().any(|c| c == "footnote-back")));
        for inline in list {
            if let Inline::Note(blocks) = inline {
                strip_backlinks(blocks);
            }
        }
    }
    for block in blocks {
        match block {
            Block::Plain(list) | Block::Para(list) => inlines(list),
            Block::BlockQuote(inner) | Block::Div(_, inner) => strip_backlinks(inner),
            Block::BulletList(items) | Block::OrderedList(_, items) => {
                for item in items {
                    strip_backlinks(item);
                }
            }
            _ => {}
        }
    }
}

/// Replace each note reference with the note itself.
fn inline_footnotes(blocks: &mut [Block], notes: &HashMap<String, Vec<Block>>) {
    fn inlines(list: &mut [Inline], notes: &HashMap<String, Vec<Block>>) {
        for inline in list {
            if let Inline::Link(attr, _, target) = inline
                && attr.classes.iter().any(|c| c == "footnote-ref")
                && let Some(body) = target.url.strip_prefix('#').and_then(|id| notes.get(id))
            {
                *inline = Inline::Note(body.clone());
                continue;
            }
            match inline {
                Inline::Emph(inner)
                | Inline::Underline(inner)
                | Inline::Strong(inner)
                | Inline::Strikeout(inner)
                | Inline::Superscript(inner)
                | Inline::Subscript(inner)
                | Inline::SmallCaps(inner)
                | Inline::Quoted(_, inner)
                | Inline::Cite(_, inner)
                | Inline::Span(_, inner)
                | Inline::Link(_, inner, _)
                | Inline::Image(_, inner, _) => inlines(inner, notes),
                Inline::Note(blocks) => inline_footnotes(blocks, notes),
                _ => {}
            }
        }
    }
    for block in blocks {
        match block {
            Block::Plain(list) | Block::Para(list) | Block::Header(_, _, list) => {
                inlines(list, notes);
            }
            Block::LineBlock(lines) => {
                for line in lines {
                    inlines(line, notes);
                }
            }
            Block::BlockQuote(inner) | Block::Div(_, inner) => inline_footnotes(inner, notes),
            Block::BulletList(items) | Block::OrderedList(_, items) => {
                for item in items {
                    inline_footnotes(item, notes);
                }
            }
            Block::DefinitionList(entries) => {
                for (term, definitions) in entries {
                    inlines(term, notes);
                    for definition in definitions {
                        inline_footnotes(definition, notes);
                    }
                }
            }
            Block::Figure(_, caption, inner) => {
                inline_footnotes(&mut caption.blocks, notes);
                inline_footnotes(inner, notes);
            }
            Block::Table(table) => {
                for row in table
                    .head
                    .rows
                    .iter_mut()
                    .chain(table.bodies.iter_mut().flat_map(|b| b.head.iter_mut().chain(&mut b.body)))
                    .chain(table.foot.rows.iter_mut())
                {
                    for cell in &mut row.cells {
                        inline_footnotes(&mut cell.blocks, notes);
                    }
                }
            }
            _ => {}
        }
    }
}

/// Whether a block is the end-of-chapter list of footnotes, which has been
/// folded into the notes themselves.
fn is_footnotes_section(block: &Block) -> bool {
    let Block::Div(attr, _) = block else { return false };
    attr.attributes.iter().any(|(k, v)| k == "epub:type" && v == "footnotes")
        || attr.classes.iter().any(|c| c == "footnotes")
}

/// Point every link inside the book at the identifier it actually got.
///
/// Concatenating the spine makes one document out of many files, so the
/// identifiers were prefixed with the file they came from. A link written
/// `#target` inside `ch001.xhtml` has to become `#ch001.xhtml_target`, and
/// one written `other.xhtml#target` has to name the other file's prefix.
fn rewrite_internal_links(blocks: &mut [Block], file: &str) {
    fn target(url: &mut String, file: &str) {
        let Some((path, fragment)) = url.split_once('#') else { return };
        if has_scheme(url) {
            return;
        }
        // An empty path is a link within this same file.
        let name = if path.is_empty() { file.to_owned() } else { basename(path) };
        *url = format!("#{name}_{fragment}");
    }
    fn inlines(list: &mut [Inline], file: &str) {
        for inline in list {
            match inline {
                Inline::Link(_, inner, t) => {
                    target(&mut t.url, file);
                    inlines(inner, file);
                }
                Inline::Emph(inner)
                | Inline::Underline(inner)
                | Inline::Strong(inner)
                | Inline::Strikeout(inner)
                | Inline::Superscript(inner)
                | Inline::Subscript(inner)
                | Inline::SmallCaps(inner)
                | Inline::Quoted(_, inner)
                | Inline::Cite(_, inner)
                | Inline::Span(_, inner)
                | Inline::Image(_, inner, _) => inlines(inner, file),
                Inline::Note(blocks) => rewrite_internal_links(blocks, file),
                _ => {}
            }
        }
    }
    for block in blocks {
        match block {
            Block::Plain(list) | Block::Para(list) | Block::Header(_, _, list) => inlines(list, file),
            Block::LineBlock(lines) => {
                for line in lines {
                    inlines(line, file);
                }
            }
            Block::BlockQuote(inner) | Block::Div(_, inner) => rewrite_internal_links(inner, file),
            Block::BulletList(items) | Block::OrderedList(_, items) => {
                for item in items {
                    rewrite_internal_links(item, file);
                }
            }
            Block::DefinitionList(entries) => {
                for (term, definitions) in entries {
                    inlines(term, file);
                    for definition in definitions {
                        rewrite_internal_links(definition, file);
                    }
                }
            }
            Block::Figure(_, caption, inner) => {
                rewrite_internal_links(&mut caption.blocks, file);
                rewrite_internal_links(inner, file);
            }
            Block::Table(table) => {
                for row in table
                    .head
                    .rows
                    .iter_mut()
                    .chain(table.bodies.iter_mut().flat_map(|b| b.head.iter_mut().chain(&mut b.body)))
                    .chain(table.foot.rows.iter_mut())
                {
                    for cell in &mut row.cells {
                        rewrite_internal_links(&mut cell.blocks, file);
                    }
                }
            }
            Block::HorizontalRule | Block::RawBlock(..) | Block::CodeBlock(..) => {}
        }
    }
}

/// Whether a block is the front-matter title page a reading system shows
/// instead of, not as part of, the text.
fn is_titlepage(block: &Block) -> bool {
    let Block::Div(attr, _) = block else { return false };
    attr.attributes
        .iter()
        .any(|(k, v)| k == "epub:type" && v == "titlepage")
        || attr.classes.iter().any(|c| c == "titlepage")
}

/// Rewrite every relative **image** target so it is relative to the
/// package document rather than to the chapter it appeared in.
///
/// Images only, and that asymmetry is pandoc's: its EPUB writer bundles
/// media at the package root and rewrites each `src` to reach it from the
/// chapter (`../moon.jpg`), so its reader undoes that one step. A link's
/// `href` is whatever the author wrote and is left exactly as found —
/// `./target.md` stays `./target.md`.
///
/// Absolute URLs and bare fragments are left alone either way: only a
/// path can be relative to anything.
fn resolve_targets(blocks: &mut [Block], dir: &str) {
    fn target(url: &mut String, dir: &str) {
        if url.is_empty() || url.starts_with('#') || has_scheme(url) || url.starts_with('/') {
            return;
        }
        let (path, fragment) = url.split_once('#').map_or((url.as_str(), ""), |(p, f)| (p, f));
        let resolved = join(dir, path);
        *url = if fragment.is_empty() { resolved } else { format!("{resolved}#{fragment}") };
    }
    fn inlines(list: &mut [Inline], dir: &str) {
        for inline in list {
            match inline {
                Inline::Image(_, inner, t) => {
                    target(&mut t.url, dir);
                    inlines(inner, dir);
                }
                // A link's href is the author's and is left as found.
                Inline::Link(_, inner, _)
                | Inline::Emph(inner)
                | Inline::Underline(inner)
                | Inline::Strong(inner)
                | Inline::Strikeout(inner)
                | Inline::Superscript(inner)
                | Inline::Subscript(inner)
                | Inline::SmallCaps(inner)
                | Inline::Quoted(_, inner)
                | Inline::Cite(_, inner)
                | Inline::Span(_, inner) => inlines(inner, dir),
                Inline::Note(blocks) => resolve_targets(blocks, dir),
                _ => {}
            }
        }
    }
    for block in blocks {
        match block {
            Block::Plain(list) | Block::Para(list) | Block::Header(_, _, list) => inlines(list, dir),
            Block::LineBlock(lines) => {
                for line in lines {
                    inlines(line, dir);
                }
            }
            Block::BlockQuote(inner) | Block::Div(_, inner) => resolve_targets(inner, dir),
            Block::BulletList(items) | Block::OrderedList(_, items) => {
                for item in items {
                    resolve_targets(item, dir);
                }
            }
            Block::DefinitionList(entries) => {
                for (term, definitions) in entries {
                    inlines(term, dir);
                    for definition in definitions {
                        resolve_targets(definition, dir);
                    }
                }
            }
            Block::Figure(_, caption, inner) => {
                resolve_targets(&mut caption.blocks, dir);
                resolve_targets(inner, dir);
            }
            Block::Table(table) => {
                for row in table
                    .head
                    .rows
                    .iter_mut()
                    .chain(table.bodies.iter_mut().flat_map(|b| b.head.iter_mut().chain(&mut b.body)))
                    .chain(table.foot.rows.iter_mut())
                {
                    for cell in &mut row.cells {
                        resolve_targets(&mut cell.blocks, dir);
                    }
                }
            }
            Block::HorizontalRule | Block::RawBlock(..) | Block::CodeBlock(..) => {}
        }
    }
}

/// Whether a URL begins with a scheme (`https:`, `mailto:`, …).
fn has_scheme(url: &str) -> bool {
    let Some(colon) = url.find(':') else { return false };
    let scheme = &url[..colon];
    scheme.starts_with(|c: char| c.is_ascii_alphabetic())
        && scheme.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
}

/// Every image URL the document names, in document order.
fn collect_image_urls(blocks: &[Block], out: &mut Vec<String>) {
    fn inlines(list: &[Inline], out: &mut Vec<String>) {
        for inline in list {
            match inline {
                Inline::Image(_, alt, target) => {
                    out.push(target.url.clone());
                    inlines(alt, out);
                }
                Inline::Emph(inner)
                | Inline::Underline(inner)
                | Inline::Strong(inner)
                | Inline::Strikeout(inner)
                | Inline::Superscript(inner)
                | Inline::Subscript(inner)
                | Inline::SmallCaps(inner)
                | Inline::Quoted(_, inner)
                | Inline::Cite(_, inner)
                | Inline::Span(_, inner)
                | Inline::Link(_, inner, _) => inlines(inner, out),
                Inline::Note(blocks) => collect_image_urls(blocks, out),
                _ => {}
            }
        }
    }
    for block in blocks {
        match block {
            Block::Plain(list) | Block::Para(list) | Block::Header(_, _, list) => inlines(list, out),
            Block::LineBlock(lines) => {
                for line in lines {
                    inlines(line, out);
                }
            }
            Block::BlockQuote(inner) | Block::Div(_, inner) => collect_image_urls(inner, out),
            Block::BulletList(items) | Block::OrderedList(_, items) => {
                for item in items {
                    collect_image_urls(item, out);
                }
            }
            Block::DefinitionList(entries) => {
                for (term, definitions) in entries {
                    inlines(term, out);
                    for definition in definitions {
                        collect_image_urls(definition, out);
                    }
                }
            }
            Block::Figure(_, caption, inner) => {
                collect_image_urls(&caption.blocks, out);
                collect_image_urls(inner, out);
            }
            Block::Table(table) => {
                for row in table
                    .head
                    .rows
                    .iter()
                    .chain(table.bodies.iter().flat_map(|b| b.head.iter().chain(&b.body)))
                    .chain(table.foot.rows.iter())
                {
                    for cell in &row.cells {
                        collect_image_urls(&cell.blocks, out);
                    }
                }
            }
            _ => {}
        }
    }
}

/// The last path segment of an href, with any fragment removed.
fn basename(href: &str) -> String {
    let path = href.split('#').next().unwrap_or(href);
    path.rsplit('/').next().unwrap_or(path).to_owned()
}

/// Resolve `href` against the package document's directory.
fn join(base: &str, href: &str) -> String {
    let href = href.split('#').next().unwrap_or(href);
    let mut segments: Vec<&str> = Vec::new();
    for segment in base.split('/').chain(href.split('/')) {
        match segment {
            "" | "." => {}
            ".." => {
                segments.pop();
            }
            segment => segments.push(segment),
        }
    }
    segments.join("/")
}

/// Decode `%XX` escapes, leaving anything malformed as written. An href in
/// a package document is a URL; the zip entry it names is not.
fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let hex = |b: u8| (b as char).to_digit(16);
    let mut i = 0;
    while i < bytes.len() {
        let escape = (bytes[i] == b'%')
            .then(|| Some((hex(*bytes.get(i + 1)?)?, hex(*bytes.get(i + 2)?)?)))
            .flatten();
        if let Some((high, low)) = escape {
            out.push(u8::try_from(high * 16 + low).unwrap_or(b'?'));
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write as _;

    /// Assemble a small book. `spine` is (id, href, linear).
    fn book(files: &[(&str, &str)], spine: &[(&str, &str, bool)], base: &str) -> Vec<u8> {
        let opf_path = if base.is_empty() {
            "book.opf".to_owned()
        } else {
            format!("{base}/book.opf")
        };
        let container = format!(
            concat!(
                r#"<?xml version="1.0" encoding="UTF-8"?>"#,
                r#"<container version="1.0""#,
                r#" xmlns="urn:oasis:names:tc:opendocument:xmlns:container">"#,
                r#"<rootfiles><rootfile full-path="{}""#,
                r#" media-type="application/oebps-package+xml"/></rootfiles></container>"#,
            ),
            opf_path
        );
        let mut items = String::new();
        let mut refs = String::new();
        for (id, href, linear) in spine {
            use std::fmt::Write as _;
            let _ = write!(
                items,
                r#"<item id="{id}" href="{href}" media-type="application/xhtml+xml"/>"#
            );
            let linear = if *linear { "" } else { r#" linear="no""# };
            let _ = write!(refs, r#"<itemref idref="{id}"{linear}/>"#);
        }
        let opf = format!(
            concat!(
                r#"<?xml version="1.0" encoding="UTF-8"?>"#,
                r#"<package xmlns="http://www.idpf.org/2007/opf" version="3.0""#,
                r#" unique-identifier="id">"#,
                r#"<metadata xmlns:dc="http://purl.org/dc/elements/1.1/">"#,
                "<dc:title>T</dc:title><dc:creator>One</dc:creator>",
                "<dc:creator>Two</dc:creator></metadata>",
                "<manifest>{}</manifest><spine>{}</spine></package>",
            ),
            items, refs
        );
        let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        let options = zip::write::SimpleFileOptions::default();
        let mut put = |name: &str, data: &str| {
            zip.start_file(name, options).unwrap();
            zip.write_all(data.as_bytes()).unwrap();
        };
        put("META-INF/container.xml", &container);
        put(&opf_path, &opf);
        for (name, body) in files {
            let path = if base.is_empty() {
                (*name).to_owned()
            } else {
                format!("{base}/{name}")
            };
            put(
                &path,
                &format!(
                    r#"<?xml version="1.0" encoding="UTF-8"?><html
                    xmlns="http://www.w3.org/1999/xhtml"><head><title>t</title>
                    </head><body>{body}</body></html>"#
                ),
            );
        }
        zip.finish().unwrap().into_inner()
    }

    #[test]
    fn reading_order_is_the_spine_and_not_the_file_order() {
        // A reader that walked the archive, or sorted by name, would put
        // these the other way round. Only the spine says which is which.
        let bytes = book(
            &[("z.xhtml", "<p>first</p>"), ("a.xhtml", "<p>second</p>")],
            &[("z", "z.xhtml", true), ("a", "a.xhtml", true)],
            "OEBPS",
        );
        let text = format!("{:?}", read_epub(&bytes).unwrap().blocks);
        let first = text.find("first").expect("first chapter");
        let second = text.find("second").expect("second chapter");
        assert!(first < second, "the spine order was not followed");
    }

    #[test]
    fn a_non_linear_item_contributes_nothing_at_all() {
        // Not even its anchor. It is a cover or a generated title page,
        // and pandoc leaves it out of the document entirely.
        let bytes = book(
            &[("cover.xhtml", "<p>cover art</p>"), ("one.xhtml", "<p>text</p>")],
            &[("cover", "cover.xhtml", false), ("one", "one.xhtml", true)],
            "OEBPS",
        );
        let text = format!("{:?}", read_epub(&bytes).unwrap().blocks);
        assert!(!text.contains("cover"), "the cover leaked into the book: {text}");
        assert!(text.contains("text"));
    }

    #[test]
    fn identifiers_are_prefixed_and_links_follow_them() {
        // Two chapters each defining `#intro` is ordinary, and once the
        // spine is concatenated the book is one document. Prefixing them
        // is only half the job: a link that still says `#intro` points at
        // whichever one won.
        let bytes = book(
            &[
                // `r##` because the markup contains `"#`, which would
                // otherwise end the raw string at the first fragment.
                ("one.xhtml", r#"<h2 id="intro">A</h2><p><a href="two.xhtml#intro">x</a></p>"#),
                ("two.xhtml", r##"<h2 id="intro">B</h2><p><a href="#intro">y</a></p>"##),
            ],
            &[("one", "one.xhtml", true), ("two", "two.xhtml", true)],
            "OEBPS",
        );
        let text = format!("{:?}", read_epub(&bytes).unwrap().blocks);
        assert!(text.contains("one.xhtml_intro"), "{text}");
        assert!(text.contains("two.xhtml_intro"), "{text}");
        assert!(!text.contains(r##""#intro""##), "an unprefixed link survived: {text}");
    }

    #[test]
    fn a_package_document_at_the_archive_root_resolves() {
        // The base directory is empty here, which is what a reader that
        // always strips a directory gets wrong.
        let bytes = book(
            &[("only.xhtml", "<p>at the root</p>")],
            &[("only", "only.xhtml", true)],
            "",
        );
        let text = format!("{:?}", read_epub(&bytes).unwrap().blocks);
        assert!(text.contains("root"), "{text}");
    }

    #[test]
    fn several_creators_arrive_in_reverse_order() {
        // Pandoc's order, measured against a two-creator package rather
        // than assumed.
        let bytes = book(&[("a.xhtml", "<p>x</p>")], &[("a", "a.xhtml", true)], "OEBPS");
        let meta = read_epub(&bytes).unwrap().meta;
        let Some(MetaValue::MetaList(authors)) = meta.get("author") else {
            panic!("expected a list of authors, got {:?}", meta.get("author"))
        };
        let names: Vec<String> = authors
            .iter()
            .map(|a| match a {
                MetaValue::MetaInlines(inlines) => format!("{inlines:?}"),
                other => format!("{other:?}"),
            })
            .collect();
        assert!(names[0].contains("Two"), "{names:?}");
        assert!(names[1].contains("One"), "{names:?}");
    }

    #[test]
    fn a_heading_keeps_only_the_identifier_the_markup_gave_it() {
        // Pandoc's EPUB reader generates none: chapters are one document
        // once the spine is concatenated, and an identifier invented per
        // chapter would be invented against the wrong namespace.
        let bytes = book(
            &[("a.xhtml", "<h1>Made Up</h1>")],
            &[("a", "a.xhtml", true)],
            "OEBPS",
        );
        let text = format!("{:?}", read_epub(&bytes).unwrap().blocks);
        assert!(!text.contains("made-up"), "an identifier was invented: {text}");
    }

    #[test]
    fn a_malformed_container_is_refused_not_guessed() {
        assert!(read_epub(b"not a zip").is_err());
        let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        zip.start_file("mimetype", zip::write::SimpleFileOptions::default()).unwrap();
        zip.write_all(b"application/epub+zip").unwrap();
        let empty = zip.finish().unwrap().into_inner();
        // A zip with no container is not a book.
        assert!(read_epub(&empty).is_err());
    }

    #[test]
    fn a_footnote_is_put_back_where_it_belongs() {
        // An EPUB keeps notes at the end of the file with a link to them.
        // Read literally that is a link to the bottom of the chapter; a
        // note has to read as a note.
        let body = concat!(
            r##"<p>Text<a href="#fn1" class="footnote-ref" id="fnref1""##,
            r#" epub:type="noteref">1</a>.</p>"#,
            r#"<section id="footnotes" class="footnotes" epub:type="footnotes">"#,
            r#"<aside epub:type="footnote" id="fn1"><p>"#,
            r##"<a href="#fnref1" class="footnote-back">1</a> The note.</p></aside>"##,
            "</section>",
        );
        let bytes = book(&[("a.xhtml", body)], &[("a", "a.xhtml", true)], "OEBPS");
        let text = format!("{:?}", read_epub(&bytes).unwrap().blocks);
        assert!(text.contains("Note("), "the reference stayed a link: {text}");
        assert!(text.contains("The"), "{text}");
        // The section it came from is gone, and so is the way back.
        assert!(!text.contains("footnote-back"), "{text}");
        assert!(!text.contains("footnotes"), "{text}");
    }
}
