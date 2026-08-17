//! EPUB writer: renders the ferrodoc AST to an e-book a reading system
//! opens.
//!
//! An EPUB is a zip of XHTML plus a package document, so the writer that
//! does the work already exists — this module is the packaging around
//! `ferrodoc-html`, exactly as the reader is packaging around
//! `ferrodoc-html`'s reader.
//!
//! Three decisions are worth stating, because none follows from the
//! specification:
//!
//! - **the document is split at level-1 headings**, one chapter per file,
//!   which is what pandoc does by default. A single-file book exercises
//!   neither the spine nor the per-file identifier prefixing, and a
//!   reading system paginates a huge file badly;
//! - **a heading's identifier belongs to its section, not to the heading.**
//!   The wrapping `<section id="…">` carries it and the `<hN>` carries
//!   none. Written the other way round, every identifier in the book
//!   arrives one level too deep and no cross-reference resolves;
//! - **a title page is always written**, and marked `linear="no"` when the
//!   document has no title. A reading system shows it as furniture, and
//!   the reader here skips a non-linear item entirely — so the two agree
//!   without either pretending the page is content.
//!
//! Output is deterministic: fixed zip timestamps and a fixed identifier
//! derived from the document, so the same AST produces the same bytes.
//! Pandoc's writer cannot say that — it stamps a UUID and the time.

use crate::Error;
use ferrodoc_ast::{Attr, Block, Inline, MetaValue, Pandoc};
use std::fmt::Write as _;
use std::io::Write as _;
use zip::write::SimpleFileOptions;

/// Render a document as an `.epub`, without embedding images.
///
/// Images survive as their alt text. Use [`write_epub_with_media`] to
/// supply their bytes.
///
/// # Errors
///
/// Only [`Error::Zip`], if the in-memory archive cannot be assembled.
pub fn write_epub(doc: &Pandoc) -> Result<Vec<u8>, Error> {
    write_epub_with_media(doc, &|_| None)
}

/// Render a document as an `.epub`, embedding every image whose bytes
/// `media` can supply for its URL.
///
/// # Errors
///
/// Only [`Error::Zip`], if the in-memory archive cannot be assembled.
pub fn write_epub_with_media(
    doc: &Pandoc,
    media: &dyn Fn(&str) -> Option<Vec<u8>>,
) -> Result<Vec<u8>, Error> {
    let title = meta_text(doc, "title");
    // The pictures are taken from the AST *before* it is rendered, so the
    // URL a chapter carries is already the one the manifest names.
    let mut blocks = doc.blocks.clone();
    let pictures = take_pictures(&mut blocks, media);
    let chapters = chapters(&blocks);

    let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let stamp = zip::DateTime::from_date_and_time(1980, 1, 1, 0, 0, 0).unwrap_or_default();
    let options = SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated)
        .last_modified_time(stamp);
    // `mimetype` first and stored, which is how a reading system
    // identifies the package without unzipping it.
    zip.start_file(
        "mimetype",
        SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored)
            .last_modified_time(stamp),
    )
    .map_err(|e| Error::Zip(e.to_string()))?;
    zip.write_all(MIMETYPE.as_bytes())
        .map_err(|e| Error::Zip(e.to_string()))?;

    let mut part = |name: &str, data: &str| -> Result<(), Error> {
        zip.start_file(name, options)
            .map_err(|e| Error::Zip(e.to_string()))?;
        zip.write_all(data.as_bytes())
            .map_err(|e| Error::Zip(e.to_string()))?;
        Ok(())
    };
    part("META-INF/container.xml", CONTAINER)?;
    part("EPUB/content.opf", &package(doc, &title, &chapters, &pictures))?;
    part("EPUB/nav.xhtml", &nav(&title, &chapters))?;
    part("EPUB/toc.ncx", &ncx(doc, &title, &chapters))?;
    part("EPUB/text/title_page.xhtml", &title_page(doc, &title))?;
    for chapter in &chapters {
        part(&format!("EPUB/text/{}", chapter.file), &chapter.xhtml)?;
    }
    #[expect(dropping_references, reason = "releases the borrow on `zip`")]
    drop(&part);
    for picture in &pictures {
        zip.start_file(format!("EPUB/{}", picture.path), options)
            .map_err(|e| Error::Zip(e.to_string()))?;
        zip.write_all(&picture.bytes)
            .map_err(|e| Error::Zip(e.to_string()))?;
    }
    let cursor = zip.finish().map_err(|e| Error::Zip(e.to_string()))?;
    Ok(cursor.into_inner())
}

const MIMETYPE: &str = "application/epub+zip";

const CONTAINER: &str = concat!(
    r#"<?xml version="1.0" encoding="UTF-8"?>"#,
    r#"<container version="1.0""#,
    r#" xmlns="urn:oasis:names:tc:opendocument:xmlns:container">"#,
    r#"<rootfiles><rootfile full-path="EPUB/content.opf""#,
    r#" media-type="application/oebps-package+xml"/></rootfiles></container>"#,
);

/// One content document.
struct Chapter {
    /// The file name inside `EPUB/text/`, and the manifest id it takes.
    file: String,
    /// The heading this chapter opens with, for the navigation document.
    title: String,
    /// The identifier that heading's section carries, so the navigation
    /// document can point *into* the chapter rather than at its start.
    anchor: String,
    xhtml: String,
}

/// One embedded picture.
struct Picture {
    /// Relative to the package document, which is where the manifest
    /// names it from.
    path: String,
    /// The URL the AST called it, so the chapter's `src` can be rewritten.
    url: String,
    media_type: &'static str,
    bytes: Vec<u8>,
}

/// Split the document into chapters at level-1 headings.
///
/// Content before the first level-1 heading becomes a chapter of its own,
/// opened by a **synthesized empty heading** — which is what pandoc does,
/// and dropping the content instead would be silent loss. A document with
/// no level-1 heading at all is therefore one chapter, not zero.
fn chapters(blocks: &[Block]) -> Vec<Chapter> {
    let mut source = blocks.to_vec();
    let opens_with_chapter =
        matches!(source.first(), Some(Block::Header(1, ..)));
    if !source.is_empty() && !opens_with_chapter {
        source.insert(
            0,
            Block::Header(
                1,
                Attr { classes: vec![UNNUMBERED.to_owned()], ..Attr::default() },
                Vec::new(),
            ),
        );
    }
    let mut ids = Vec::new();
    let sectioned = sections(&source, 1, &mut ids);
    let mut groups: Vec<Vec<Block>> = Vec::new();
    for block in sectioned {
        // A level-1 section starts a chapter; anything else joins the one
        // being built, or opens the first.
        let starts_chapter = matches!(&block, Block::Div(attr, _)
            if attr.classes.iter().any(|c| c == "level1"));
        if starts_chapter || groups.is_empty() {
            groups.push(vec![block]);
        } else {
            groups.last_mut().expect("just checked").push(block);
        }
    }
    groups
        .into_iter()
        .enumerate()
        .map(|(index, blocks)| {
            let (title, anchor) = opening(&blocks);
            Chapter {
                file: format!("ch{:03}.xhtml", index + 1),
                title,
                anchor,
                xhtml: chapter_xhtml(&blocks),
            }
        })
        .collect()
}

/// The class pandoc puts on a heading it invented, and on its section.
const UNNUMBERED: &str = "unnumbered";

/// The title and anchor a chapter opens with, for the navigation document.
fn opening(blocks: &[Block]) -> (String, String) {
    match blocks.first() {
        Some(Block::Div(attr, inner)) => {
            let title = match inner.first() {
                Some(Block::Header(_, _, list)) => plain_text(list),
                _ => String::new(),
            };
            (title, attr.identifier.clone())
        }
        _ => (String::new(), String::new()),
    }
}

/// Wrap each heading and the content that follows it in a section `Div`,
/// nesting deeper headings inside shallower ones.
///
/// Three rules, each measured against pandoc 3.8.2.1 rather than assumed,
/// because getting any of them wrong scores zero on `diff-epub-write`:
///
/// - **the heading's identifier moves to the section** and the heading
///   keeps none. Leaving it on the heading puts every anchor in the book
///   one level too deep;
/// - **a heading with no identifier is given one**, slugged from its text
///   and uniqued document-wide with `-1`, `-2` — so two chapters both
///   called `A` become `a` and `a-1`. An empty heading slugs to nothing
///   and takes `section`;
/// - **the section's classes are `section`, `level{N}`, then the
///   heading's own**, and the heading keeps its classes as well. Pandoc
///   writes a real `<section class="level1">` and its reader adds
///   `section` back from the element name; the HTML writer here emits a
///   `<div>`, so the class has to be written for the two to read back
///   equal. Dropping it scored 0/11 with every case differing in that one
///   string.
fn sections(blocks: &[Block], level: i64, ids: &mut Vec<String>) -> Vec<Block> {
    let mut out: Vec<Block> = Vec::new();
    let mut index = 0;
    while index < blocks.len() {
        let Block::Header(heading_level, attr, inlines) = &blocks[index] else {
            out.push(blocks[index].clone());
            index += 1;
            continue;
        };
        if *heading_level != level {
            // A heading deeper or shallower than the level being built is
            // not this level's business; the recursion below places it.
            out.push(blocks[index].clone());
            index += 1;
            continue;
        }
        // Everything up to the next heading at this level or shallower.
        let start = index + 1;
        let mut end = start;
        while end < blocks.len() {
            if let Block::Header(next, ..) = &blocks[end]
                && *next <= level
            {
                break;
            }
            end += 1;
        }
        let identifier = if attr.identifier.is_empty() {
            unique(&slug(&plain_text(inlines)), ids)
        } else {
            unique(&attr.identifier, ids)
        };
        let mut inner = vec![Block::Header(
            *heading_level,
            // The identifier moved to the section; the classes did not.
            Attr { classes: attr.classes.clone(), ..Attr::default() },
            inlines.clone(),
        )];
        inner.extend(sections(&blocks[start..end], level + 1, ids));
        let mut classes = vec!["section".to_owned(), format!("level{level}")];
        classes.extend(attr.classes.iter().cloned());
        out.push(Block::Div(
            Attr { identifier, classes, attributes: Vec::new() },
            inner,
        ));
        index = end;
    }
    out
}

/// Pandoc's `auto_identifiers`: keep alphanumerics, whitespace, `_`, `-`
/// and `.`, lowercase, and join the words with hyphens. A heading that
/// leaves nothing behind is called `section`.
fn slug(text: &str) -> String {
    let filtered: String = text
        .chars()
        .filter(|c| c.is_alphanumeric() || c.is_whitespace() || matches!(c, '_' | '-' | '.'))
        .flat_map(char::to_lowercase)
        .collect();
    let joined = filtered.split_whitespace().collect::<Vec<_>>().join("-");
    if joined.is_empty() { "section".to_owned() } else { joined }
}

/// Make `candidate` unique against the identifiers already handed out,
/// recording it. The `-N` search resumes rather than restarting: starting
/// over for every collision is quadratic, and it has shipped here before.
fn unique(candidate: &str, ids: &mut Vec<String>) -> String {
    let mut name = candidate.to_owned();
    let mut suffix = 0;
    while ids.contains(&name) {
        suffix += 1;
        name = format!("{candidate}-{suffix}");
    }
    ids.push(name.clone());
    name
}

/// One content document's XHTML.
fn chapter_xhtml(blocks: &[Block]) -> String {
    let body = ferrodoc_html::write_html(&Pandoc::new(blocks.to_vec()));
    xhtml("bodymatter", &body)
}

/// The XHTML skeleton every content document shares.
fn xhtml(kind: &str, body: &str) -> String {
    format!(
        concat!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n",
            "<!DOCTYPE html>\n",
            "<html xmlns=\"http://www.w3.org/1999/xhtml\"",
            " xmlns:epub=\"http://www.idpf.org/2007/ops\">\n",
            "<head>\n<meta charset=\"utf-8\" />\n<title>ferrodoc</title>\n</head>\n",
            "<body epub:type=\"{}\">\n{}</body>\n</html>\n",
        ),
        kind, body
    )
}

/// The title page: the document's own metadata, as page furniture.
///
/// Marked `titlepage` so a reader skips it rather than reading the title
/// twice — which is what the reader in this crate does, and pandoc's.
fn title_page(doc: &Pandoc, title: &str) -> String {
    let mut body = String::from("<section epub:type=\"titlepage\" class=\"titlepage\">\n");
    if !title.is_empty() {
        let _ = writeln!(body, "<h1 class=\"title\">{}</h1>", escape(title));
    }
    for author in meta_list(doc, "author") {
        let _ = writeln!(body, "<p class=\"author\">{}</p>", escape(&author));
    }
    body.push_str("</section>\n");
    xhtml("frontmatter", &body)
}

/// The manifest id a file takes. A `.` is not allowed in an XML id.
fn item_id(file: &str) -> String {
    file.replace('.', "_")
}

/// The package document: metadata, manifest and spine.
///
/// `dc:title` is written **always**, with `Untitled` when the document has
/// none: EPUB 3 requires exactly one, and pandoc omits it — `epubcheck`
/// rejects pandoc's book for precisely that. `dcterms:modified` is fixed
/// rather than stamped with the clock, which is what makes the same
/// document produce the same bytes.
fn package(doc: &Pandoc, title: &str, chapters: &[Chapter], pictures: &[Picture]) -> String {
    let mut out = String::from(concat!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n",
        "<package xmlns=\"http://www.idpf.org/2007/opf\" version=\"3.0\"",
        " unique-identifier=\"epub-id\">\n",
        "<metadata xmlns:dc=\"http://purl.org/dc/elements/1.1/\">\n",
    ));
    // Derived from the content rather than random, so the same AST always
    // produces the same book. Pandoc stamps a fresh UUID and the current
    // time, which is why its output is never byte-reproducible.
    let _ = writeln!(
        out,
        "<dc:identifier id=\"epub-id\">urn:uuid:{}</dc:identifier>",
        stable_uuid(doc)
    );
    let _ = writeln!(
        out,
        "<dc:title>{}</dc:title>",
        escape(if title.is_empty() { "Untitled" } else { title })
    );
    let language = meta_text(doc, "lang");
    let _ = writeln!(
        out,
        "<dc:language>{}</dc:language>",
        escape(if language.is_empty() { "en" } else { &language })
    );
    out.push_str("<meta property=\"dcterms:modified\">1980-01-01T00:00:00Z</meta>\n");
    out.push_str("</metadata>\n<manifest>\n");
    out.push_str(concat!(
        "<item id=\"nav\" href=\"nav.xhtml\"",
        " media-type=\"application/xhtml+xml\" properties=\"nav\"/>\n",
        "<item id=\"ncx\" href=\"toc.ncx\"",
        " media-type=\"application/x-dtbncx+xml\"/>\n",
        "<item id=\"title_page\" href=\"text/title_page.xhtml\"",
        " media-type=\"application/xhtml+xml\"/>\n",
    ));
    for chapter in chapters {
        let _ = writeln!(
            out,
            "<item id=\"{}\" href=\"text/{}\" media-type=\"application/xhtml+xml\"/>",
            item_id(&chapter.file),
            chapter.file
        );
    }
    for (index, picture) in pictures.iter().enumerate() {
        let _ = writeln!(
            out,
            "<item id=\"img{index}\" href=\"{}\" media-type=\"{}\"/>",
            picture.path, picture.media_type
        );
    }
    // `linear="no"`: the title page is furniture, and the reader in this
    // crate drops a non-linear item rather than reading the title twice.
    out.push_str("</manifest>\n<spine toc=\"ncx\">\n<itemref idref=\"title_page\" linear=\"no\"/>\n");
    for chapter in chapters {
        let _ = writeln!(out, "<itemref idref=\"{}\"/>", item_id(&chapter.file));
    }
    out.push_str("</spine>\n</package>\n");
    out
}

/// The EPUB 3 navigation document: the table of contents a reading system
/// shows, plus the landmarks that name the title page.
fn nav(title: &str, chapters: &[Chapter]) -> String {
    let mut body = String::from("<nav epub:type=\"toc\" id=\"toc\">\n<h1>Contents</h1>\n<ol>\n");
    for chapter in chapters {
        let label = if chapter.title.is_empty() {
            if title.is_empty() { "Untitled" } else { title }
        } else {
            &chapter.title
        };
        // Into the chapter's own section, not merely at its file: an
        // anchor is what lets a reader land on the heading.
        let fragment =
            if chapter.anchor.is_empty() { String::new() } else { format!("#{}", chapter.anchor) };
        let _ = writeln!(
            body,
            "<li><a href=\"text/{}{fragment}\">{}</a></li>",
            chapter.file,
            escape(label)
        );
    }
    body.push_str("</ol>\n</nav>\n");
    body.push_str(concat!(
        "<nav epub:type=\"landmarks\" id=\"landmarks\" hidden=\"hidden\">\n<ol>\n",
        "<li><a href=\"text/title_page.xhtml\" epub:type=\"titlepage\">Title Page</a></li>\n",
        "</ol>\n</nav>\n",
    ));
    xhtml("frontmatter", &body)
}

/// The EPUB 2 navigation document.
///
/// Written as well as `nav.xhtml`, not instead: a reading system old
/// enough to need it will not read the EPUB 3 one, and `epubcheck` wants
/// every spine item reachable from a table of contents.
fn ncx(doc: &Pandoc, title: &str, chapters: &[Chapter]) -> String {
    let mut out = String::from(concat!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n",
        "<ncx xmlns=\"http://www.daisy.org/z3986/2005/ncx/\" version=\"2005-1\">\n",
    ));
    let _ = writeln!(
        out,
        "<head><meta name=\"dtb:uid\" content=\"urn:uuid:{}\"/></head>",
        stable_uuid(doc)
    );
    let shown = if title.is_empty() { "Untitled" } else { title };
    let _ = writeln!(out, "<docTitle><text>{}</text></docTitle>", escape(shown));
    out.push_str("<navMap>\n");
    for (index, chapter) in chapters.iter().enumerate() {
        let label = if chapter.title.is_empty() { shown } else { &chapter.title };
        let order = index + 1;
        let _ = writeln!(
            out,
            "<navPoint id=\"nav{order}\" playOrder=\"{order}\"><navLabel><text>{}</text>\
             </navLabel><content src=\"text/{}\"/></navPoint>",
            escape(label),
            chapter.file
        );
    }
    out.push_str("</navMap>\n</ncx>\n");
    out
}

/// Repair the HTML comments in one raw HTML fragment.
///
/// A content document is parsed as XML, where HTML's tolerance runs out,
/// and two comment shapes are *fatal* there — the book will not open at
/// all, which is what `epubcheck` reported as `RSC-016`:
///
/// - an **unterminated** comment. `corpus/truncation-cases.md` has one;
///   the markdown reader keeps it verbatim, as it must, and HTML swallows
///   the rest of the file. It is *closed*, not dropped — which is what
///   pandoc does, and it keeps the author's text instead of throwing it
///   away to satisfy a parser;
/// - `--` **inside** a comment, which XML forbids outright.
///
/// A well-formed comment is left exactly alone: it is valid XHTML, pandoc
/// keeps it, and dropping it was a silent loss that cost a gate case.
///
/// **Per fragment, never over the rendered chapter.** An unterminated
/// comment runs to the end of whatever it is given, and given the whole
/// chapter that is the writer's own `</li></ul>` — which traded one fatal
/// for another.
fn repair_comments(body: &str) -> String {
    let mut out = String::with_capacity(body.len());
    let mut rest = body;
    while let Some(at) = rest.find("<!--") {
        out.push_str(&rest[..at]);
        let after = &rest[at + 4..];
        let Some(end) = after.find("-->") else {
            // Unterminated: in XML it would run to the end of the file and
            // the book would not open. Close it and keep the text.
            if !after.contains("--") {
                let _ = write!(out, "<!--{after}-->");
            }
            return out;
        };
        if after[..end].contains("--") {
            // Legal in HTML, forbidden in XML; the comment is dropped
            // rather than rewritten, because rewriting it would change
            // text a reader may be looking at in the source.
            rest = &after[end + 3..];
            continue;
        }
        out.push_str(&rest[at..at + 4 + end + 3]);
        rest = &after[end + 3..];
    }
    out.push_str(rest);
    out
}

/// Collect every picture whose bytes `media` can supply, rewrite each
/// image's URL to the path the manifest gives it, and **replace an image
/// whose bytes cannot be had with its alt text**.
///
/// Both halves matter and the second is the one that bites. An `<img>`
/// left pointing at a file that is not in the archive is not a cosmetic
/// flaw: `epubcheck` rejects the book (`RSC-007`), and a reading system
/// shows a hole. Pandoc leaves the reference and fails the same check;
/// this does what [`write_epub`]'s own documentation already promised,
/// which is that an image without bytes survives as its alt text.
///
/// A chapter lives in `text/` and the pictures in `media/`, so the URL
/// written is `../media/…` — the step the reader takes back off.
fn take_pictures(blocks: &mut Vec<Block>, media: &dyn Fn(&str) -> Option<Vec<u8>>) -> Vec<Picture> {
    let mut pictures: Vec<Picture> = Vec::new();
    walk_blocks(blocks, &mut |list: &mut Vec<Inline>| {
        let mut out = Vec::with_capacity(list.len());
        for inline in list.drain(..) {
            if let Inline::RawInline(format, text) = inline {
                let text = if format.0 == "html" { repair_comments(&text) } else { text };
                out.push(Inline::RawInline(format, text));
                continue;
            }
            // A link out of the book is a reference the book cannot
            // satisfy, exactly like a picture with no bytes: `epubcheck`
            // rejects it (`RSC-007`) and a reading system offers a tap
            // that goes nowhere. The text stays; only the dead href goes.
            // A fragment stays — that is a link *into* the book — and so
            // does anything with a scheme, which is the web.
            if let Inline::Link(_, inner, target) = &inline
                && !target.url.starts_with('#')
                && !has_scheme(&target.url)
            {
                out.extend(inner.clone());
                continue;
            }
            let Inline::Image(attr, alt, mut target) = inline else {
                out.push(inline);
                continue;
            };
            if let Some(picture) = pictures.iter().find(|p| p.url == target.url) {
                target.url = format!("../{}", picture.path);
                out.push(Inline::Image(attr, alt, target));
                continue;
            }
            match media(&target.url).and_then(|bytes| kind(&bytes).map(|k| (bytes, k))) {
                Some((bytes, (extension, media_type))) => {
                    let path = format!("media/image{}.{extension}", pictures.len());
                    pictures.push(Picture {
                        path: path.clone(),
                        url: std::mem::replace(&mut target.url, format!("../{path}")),
                        media_type,
                        bytes,
                    });
                    out.push(Inline::Image(attr, alt, target));
                }
                // No bytes, so no reference: the alt text is what is left
                // of the picture, and it is what pandoc's readers see.
                None => out.extend(alt),
            }
        }
        *list = out;
    });
    pictures
}

/// Whether a URL begins with a scheme (`https:`, `mailto:`, …).
fn has_scheme(url: &str) -> bool {
    let Some(colon) = url.find(':') else { return false };
    let scheme = &url[..colon];
    scheme.starts_with(|c: char| c.is_ascii_alphabetic())
        && scheme.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
}

/// Apply `f` to every run of inlines in the tree.
fn walk_blocks(blocks: &mut Vec<Block>, f: &mut impl FnMut(&mut Vec<Inline>)) {
    for block in blocks {
        match block {
            Block::Plain(list) | Block::Para(list) | Block::Header(_, _, list) => f(list),
            Block::LineBlock(lines) => {
                for line in lines {
                    f(line);
                }
            }
            Block::BlockQuote(inner) | Block::Div(_, inner) => walk_blocks(inner, f),
            Block::BulletList(items) | Block::OrderedList(_, items) => {
                for item in items {
                    walk_blocks(item, f);
                }
            }
            Block::DefinitionList(entries) => {
                for (term, definitions) in entries {
                    f(term);
                    for definition in definitions {
                        walk_blocks(definition, f);
                    }
                }
            }
            Block::Figure(_, caption, inner) => {
                walk_blocks(&mut caption.blocks, f);
                walk_blocks(inner, f);
            }
            Block::Table(table) => {
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
                        walk_blocks(&mut cell.blocks, f);
                    }
                }
            }
            Block::RawBlock(format, text) => {
                if format.0 == "html" {
                    *text = repair_comments(text);
                }
            }
            Block::HorizontalRule | Block::CodeBlock(..) => {}
        }
    }
}

/// What a picture is, from its first bytes. Only the formats an EPUB
/// reading system is required to display.
fn kind(bytes: &[u8]) -> Option<(&'static str, &'static str)> {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some(("png", "image/png"))
    } else if bytes.starts_with(b"\xff\xd8") {
        Some(("jpg", "image/jpeg"))
    } else if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        Some(("gif", "image/gif"))
    } else if bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"WEBP") {
        Some(("webp", "image/webp"))
    } else if bytes.starts_with(b"<svg") || bytes.starts_with(b"<?xml") {
        Some(("svg", "image/svg+xml"))
    } else {
        None
    }
}

/// A UUID derived from the document, so the same AST produces the same
/// book.
///
/// Not a cryptographic hash and it does not need to be: it distinguishes
/// documents, and being *stable* is the point. A random UUID would make
/// every build differ, which is what stops pandoc's output from being
/// byte-reproducible.
fn stable_uuid(doc: &Pandoc) -> String {
    let mut hash: u128 = 0xcbf2_9ce4_8422_2325;
    for byte in format!("{:?}", doc.blocks).bytes() {
        hash ^= u128::from(byte);
        hash = hash.wrapping_mul(0x100_0000_01b3);
    }
    let hex = format!("{hash:032x}");
    format!(
        "{}-{}-{}-{}-{}",
        &hex[0..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..32]
    )
}

fn meta_text(doc: &Pandoc, field: &str) -> String {
    meta_list(doc, field).join(" ")
}

/// A metadata field as one string per value.
fn meta_list(doc: &Pandoc, field: &str) -> Vec<String> {
    fn one(value: &MetaValue, out: &mut Vec<String>) {
        match value {
            MetaValue::MetaString(text) => out.push(text.clone()),
            MetaValue::MetaInlines(list) => out.push(plain_text(list)),
            MetaValue::MetaBlocks(blocks) => {
                for block in blocks {
                    if let Block::Plain(list) | Block::Para(list) = block {
                        out.push(plain_text(list));
                    }
                }
            }
            MetaValue::MetaList(values) => {
                for value in values {
                    one(value, out);
                }
            }
            MetaValue::MetaBool(flag) => out.push(flag.to_string()),
            MetaValue::MetaMap(_) => {}
        }
    }
    let mut out = Vec::new();
    if let Some(value) = doc.meta.get(field) {
        one(value, &mut out);
    }
    out.retain(|text| !text.is_empty());
    out
}

fn plain_text(inlines: &[Inline]) -> String {
    fn walk(list: &[Inline], out: &mut String) {
        for inline in list {
            match inline {
                Inline::Str(text) | Inline::Code(_, text) | Inline::Math(_, text) => {
                    out.push_str(text);
                }
                Inline::Space | Inline::SoftBreak | Inline::LineBreak => out.push(' '),
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
                | Inline::Image(_, inner, _) => walk(inner, out),
                _ => {}
            }
        }
    }
    let mut out = String::new();
    walk(inlines, &mut out);
    out
}

/// XML text and attribute escaping. The package document is XML, not HTML,
/// so an unescaped `&` in a title is a book no reader will open.
fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
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
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::read_epub;
    use ferrodoc_ast::Target;

    fn heading(level: i64, id: &str, text: &str) -> Block {
        Block::Header(
            level,
            Attr { identifier: id.to_owned(), ..Attr::default() },
            vec![Inline::Str(text.to_owned())],
        )
    }

    fn para(text: &str) -> Block {
        Block::Para(vec![Inline::Str(text.to_owned())])
    }

    /// Every heading in the document, in order, however deeply the
    /// section `Div`s nest it. The reader keeps those divs — that is what
    /// an EPUB's `<section>` elements are — so a flat scan of the top
    /// level finds nothing at all.
    fn headings(blocks: &[Block]) -> Vec<(String, String)> {
        fn walk(blocks: &[Block], id: &str, out: &mut Vec<(String, String)>) {
            for block in blocks {
                match block {
                    Block::Header(_, attr, list) => {
                        // The identifier is on the section, not the heading.
                        let owner = if attr.identifier.is_empty() { id } else { &attr.identifier };
                        out.push((plain_text(list), owner.to_owned()));
                    }
                    Block::Div(attr, inner) => walk(inner, &attr.identifier, out),
                    _ => {}
                }
            }
        }
        let mut out = Vec::new();
        walk(blocks, "", &mut out);
        out
    }

    fn paragraphs(blocks: &[Block]) -> Vec<String> {
        fn walk(blocks: &[Block], out: &mut Vec<String>) {
            for block in blocks {
                match block {
                    Block::Para(list) | Block::Plain(list) => {
                        let text = plain_text(list);
                        if !text.is_empty() {
                            out.push(text);
                        }
                    }
                    Block::Div(_, inner) => walk(inner, out),
                    _ => {}
                }
            }
        }
        let mut out = Vec::new();
        walk(blocks, &mut out);
        out
    }

    /// Every image URL in the tree, in order.
    fn image_urls(blocks: &[Block]) -> Vec<String> {
        fn walk(blocks: &[Block], urls: &mut Vec<String>) {
            for block in blocks {
                match block {
                    Block::Para(list) | Block::Plain(list) => {
                        for inline in list {
                            if let Inline::Image(_, _, target) = inline {
                                urls.push(target.url.clone());
                            }
                        }
                    }
                    Block::Div(_, inner) => walk(inner, urls),
                    _ => {}
                }
            }
        }
        let mut urls = Vec::new();
        walk(blocks, &mut urls);
        urls
    }

    /// The order the chapters appear in the archive says nothing; the
    /// spine is what a reading system follows, and it is what the reader
    /// in this crate concatenates.
    #[test]
    fn a_written_book_comes_back_in_spine_order() {
        let doc = Pandoc::new(vec![
            heading(1, "first", "First"),
            para("one"),
            heading(1, "second", "Second"),
            para("two"),
            heading(1, "third", "Third"),
            para("three"),
        ]);
        let back = read_epub(&write_epub(&doc).expect("writes")).expect("reads back");
        let titles: Vec<String> = headings(&back.blocks).into_iter().map(|(t, _)| t).collect();
        assert_eq!(titles, ["First", "Second", "Third"], "{back:?}");
        assert_eq!(paragraphs(&back.blocks), ["one", "two", "three"], "{back:?}");
    }

    /// A title page is furniture: it contributes its anchor and nothing
    /// else, so a title must not come back as a heading the document body
    /// never had.
    #[test]
    fn the_title_page_does_not_become_content() {
        let mut doc = Pandoc::new(vec![heading(1, "only", "Only"), para("body")]);
        doc.meta.insert(
            "title".to_owned(),
            MetaValue::MetaInlines(vec![Inline::Str("A Book".to_owned())]),
        );
        let back = read_epub(&write_epub(&doc).expect("writes")).expect("reads back");
        let titles: Vec<String> = headings(&back.blocks).into_iter().map(|(t, _)| t).collect();
        assert_eq!(titles, ["Only"], "the title page came back as content: {back:?}");
        // It survives as metadata, which is where a title belongs.
        assert!(back.meta.contains_key("title"), "{back:?}");
    }

    /// Every chapter's identifiers are prefixed by its file, so two
    /// chapters that both say `notes` do not collide once concatenated.
    #[test]
    fn identifiers_survive_the_split_into_chapters() {
        let doc = Pandoc::new(vec![
            heading(1, "alpha", "Alpha"),
            heading(2, "notes", "Notes"),
            heading(1, "beta", "Beta"),
            heading(2, "notes", "Notes"),
        ]);
        let back = read_epub(&write_epub(&doc).expect("writes")).expect("reads back");
        let ids: Vec<String> = headings(&back.blocks).into_iter().map(|(_, id)| id).collect();
        assert_eq!(ids.len(), 4, "{back:?}");
        let unique: std::collections::BTreeSet<&String> = ids.iter().collect();
        assert_eq!(unique.len(), 4, "two chapters' identifiers collided: {ids:?}");
    }

    /// The same AST twice is the same book, byte for byte. Pandoc cannot
    /// say this: it stamps a fresh UUID and the current time.
    #[test]
    fn the_same_document_writes_the_same_bytes() {
        let doc = Pandoc::new(vec![heading(1, "a", "A"), para("body")]);
        assert_eq!(write_epub(&doc).expect("writes"), write_epub(&doc).expect("writes"));
    }

    /// An image's bytes travel with the book under a name of the writer's
    /// choosing, and the chapter's `src` is rewritten to reach them — so
    /// the reader here finds the bytes again under the URL it reports.
    #[test]
    fn a_picture_is_embedded_and_its_src_rewritten() {
        let doc = Pandoc::new(vec![
            heading(1, "a", "A"),
            Block::Para(vec![Inline::Image(
                Box::default(),
                vec![Inline::Str("alt".to_owned())],
                Box::new(Target { url: "moon.png".to_owned(), title: String::new() }),
            )]),
        ]);
        let png = b"\x89PNG\r\n\x1a\n".to_vec();
        let bytes =
            write_epub_with_media(&doc, &|url| (url == "moon.png").then(|| png.clone()))
                .expect("writes");
        let (back, media) = crate::read_epub_with_media(&bytes).expect("reads back");
        assert_eq!(media.len(), 1, "the picture did not survive: {media:?}");
        // Whatever the writer called the file, the URL the reader reports
        // has to be the one the media map is keyed by, or no consumer can
        // pair them up.
        let urls = image_urls(&back.blocks);
        assert_eq!(urls.len(), 1, "{back:?}");
        assert!(media.contains_key(&urls[0]), "src {:?} is not in {media:?}", urls[0]);
    }

    fn archive_names(bytes: &[u8]) -> Vec<String> {
        let mut zip =
            zip::ZipArchive::new(std::io::Cursor::new(bytes.to_vec())).expect("a zip");
        (0..zip.len())
            .map(|i| zip.by_index(i).expect("entry").name().to_owned())
            .collect()
    }

    /// `mimetype` must be the first entry and stored uncompressed, which is
    /// how a reading system identifies the package without unzipping it.
    #[test]
    fn the_mimetype_is_first_and_stored() {
        let bytes = write_epub(&Pandoc::new(vec![para("body")])).expect("writes");
        assert_eq!(archive_names(&bytes).first().map(String::as_str), Some("mimetype"));
        let mut zip =
            zip::ZipArchive::new(std::io::Cursor::new(bytes.clone())).expect("a zip");
        let entry = zip.by_index(0).expect("entry");
        assert_eq!(entry.compression(), zip::CompressionMethod::Stored);
    }
}
