//! Universal document converter: read markdown (`CommonMark` or GFM),
//! HTML, DOCX or the pandoc JSON AST, write any of those plus plain text.
//!
//! Everything goes through one document model — the same AST pandoc uses —
//! so any supported input can produce any supported output.
//!
//! ```
//! use ferrodoc::{Format, convert};
//!
//! let html = convert(b"# Title\n\nHello *world*.\n", Format::Markdown, Format::Html)?;
//! assert_eq!(
//!     String::from_utf8(html)?,
//!     "<h1>Title</h1>\n<p>Hello <em>world</em>.</p>\n"
//! );
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```
//!
//! To transform a document rather than just convert it, work with the AST:
//!
//! ```
//! use ferrodoc::{Format, ast::Block, parse, render};
//!
//! let mut doc = parse(b"# Title\n\ntext\n", Format::Markdown)?;
//! doc.blocks.retain(|block| !matches!(block, Block::Header(..)));
//! let html = render(&doc, Format::Html)?;
//! assert_eq!(String::from_utf8(html)?, "<p>text</p>\n");
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```

/// The document model: the pandoc-compatible AST.
pub mod ast {
    pub use ferrodoc_ast::*;
}

pub use ferrodoc_ast::Pandoc;

use std::fmt;

/// A document format.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    /// `CommonMark`. Readable and writable.
    Markdown,
    /// `GitHub Flavored Markdown`: `CommonMark` plus tables, task lists,
    /// strikethrough and extended autolinks. Readable and writable.
    Gfm,
    /// HTML. Readable and writable.
    Html,
    /// Office Open XML word processing documents. Readable and writable.
    Docx,
    /// The pandoc JSON AST. Readable and writable.
    Json,
    /// Unformatted text extraction. Writable.
    Plain,
}

impl Format {
    /// Every format name accepted on the command line, in help order.
    pub const NAMES: &'static [&'static str] = &[
        "markdown", "commonmark", "gfm", "html", "docx", "json", "plain",
    ];

    /// Parse a format name, accepting pandoc's spellings.
    pub fn parse(name: &str) -> Option<Self> {
        match name.to_ascii_lowercase().as_str() {
            "markdown" | "commonmark" | "md" => Some(Format::Markdown),
            "gfm" | "markdown_github" => Some(Format::Gfm),
            "html" | "htm" => Some(Format::Html),
            "docx" => Some(Format::Docx),
            "json" => Some(Format::Json),
            "plain" | "text" | "txt" => Some(Format::Plain),
            _ => None,
        }
    }

    /// Guess a format from a file name's extension.
    pub fn from_path(path: &std::path::Path) -> Option<Self> {
        Format::parse(path.extension()?.to_str()?)
    }

    /// Whether documents can be read from this format.
    pub fn readable(self) -> bool {
        !matches!(self, Format::Plain)
    }

    /// Whether writing this format embeds image bytes.
    ///
    /// Reading a document's media costs memory proportional to it — a
    /// `.docx` can hold a part that inflates a thousandfold — so
    /// [`convert`] asks for it only when the answer here is yes.
    pub fn embeds_media(self) -> bool {
        matches!(self, Format::Docx)
    }

    /// Whether documents can be written to this format.
    pub fn writable(self) -> bool {
        // Every format here has a writer; `Plain` is the only one-way one,
        // and it is one-way in the other direction.
        true
    }

    /// The name used in messages.
    pub fn name(self) -> &'static str {
        match self {
            Format::Markdown => "markdown",
            Format::Gfm => "gfm",
            Format::Html => "html",
            Format::Docx => "docx",
            Format::Json => "json",
            Format::Plain => "plain",
        }
    }
}

impl fmt::Display for Format {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// Why a conversion could not be performed.
#[derive(Debug)]
pub enum Error {
    /// The input format cannot be read (only written).
    NotReadable(Format),
    /// The output format cannot be written (only read).
    NotWritable(Format),
    /// The input was not valid for its format.
    Invalid {
        /// The format the input was supposed to be in.
        format: Format,
        /// What the underlying reader or writer reported.
        detail: String,
    },
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::NotReadable(format) => {
                write!(f, "cannot read {format}: it is an output-only format")
            }
            Error::NotWritable(format) => {
                write!(f, "cannot write {format}: it is an input-only format")
            }
            Error::Invalid { format, detail } => write!(f, "invalid {format} input: {detail}"),
        }
    }
}

impl std::error::Error for Error {}

/// The image bytes a document carries, keyed by the URL its AST refers to
/// them by. Only DOCX input supplies any today; every other reader
/// produces an empty bag.
pub type Media = std::collections::HashMap<String, Vec<u8>>;

/// Read a document together with the bytes of every image it embeds.
///
/// [`convert`] uses this, which is why `docx → docx` keeps its pictures.
/// Callers that go through [`parse`] and [`render`] separately need it
/// only when the images must survive.
///
/// # Errors
///
/// The same as [`parse`].
pub fn parse_with_media(input: &[u8], from: Format) -> Result<(Pandoc, Media), Error> {
    match from {
        Format::Docx => ferrodoc_docx::read_docx_with_media(input).map_err(|e| Error::Invalid {
            format: from,
            detail: e.to_string(),
        }),
        _ => Ok((parse(input, from)?, Media::new())),
    }
}

/// Read a document.
pub fn parse(input: &[u8], from: Format) -> Result<Pandoc, Error> {
    let text = |input: &[u8]| -> Result<String, Error> {
        String::from_utf8(input.to_vec()).map_err(|e| Error::Invalid {
            format: from,
            detail: format!("not UTF-8: {e}"),
        })
    };
    match from {
        Format::Markdown => ferrodoc_markdown::read_commonmark(&text(input)?).map_err(|e| Error::Invalid {
            format: from,
            detail: e.to_string(),
        }),
        Format::Gfm => ferrodoc_markdown::read_gfm(&text(input)?).map_err(|e| Error::Invalid {
            format: from,
            detail: e.to_string(),
        }),
        Format::Docx => ferrodoc_docx::read_docx(input).map_err(|e| Error::Invalid {
            format: from,
            detail: e.to_string(),
        }),
        Format::Json => serde_json::from_slice(input).map_err(|e| Error::Invalid {
            format: from,
            detail: e.to_string(),
        }),
        Format::Html => ferrodoc_html::read_html(&text(input)?).map_err(|e| Error::Invalid {
            format: from,
            detail: e.to_string(),
        }),
        Format::Plain => Err(Error::NotReadable(Format::Plain)),
    }
}

/// Write a document. Images are not embedded; see [`render_with_media`].
pub fn render(doc: &Pandoc, to: Format) -> Result<Vec<u8>, Error> {
    render_with_media(doc, to, &|_| None)
}

/// Write a document, embedding every image whose bytes `media` can supply
/// for its URL.
///
/// Resolving a URL is the caller's job: it may name a file on disk, a
/// cache, or nothing at all, and this crate has no business guessing. Only
/// DOCX output embeds media today; other formats ignore the resolver.
pub fn render_with_media(
    doc: &Pandoc,
    to: Format,
    media: &dyn Fn(&str) -> Option<Vec<u8>>,
) -> Result<Vec<u8>, Error> {
    match to {
        Format::Markdown => Ok(ferrodoc_markdown::write_markdown(doc).into_bytes()),
        Format::Gfm => Ok(ferrodoc_markdown::write_gfm(doc).into_bytes()),
        Format::Html => Ok(ferrodoc_html::write_html(doc).into_bytes()),
        Format::Plain => Ok(ferrodoc_text::write_text(doc).into_bytes()),
        Format::Docx => {
            ferrodoc_docx::write_docx_with_media(doc, media).map_err(|e| Error::Invalid {
                format: to,
                detail: e.to_string(),
            })
        }
        Format::Json => {
            let mut json = serde_json::to_vec(doc).map_err(|e| Error::Invalid {
                format: to,
                detail: e.to_string(),
            })?;
            json.push(b'\n');
            Ok(json)
        }
    }
}

/// Convert a document from one format to another.
///
/// Images the input embeds are carried through, so `docx → docx` keeps
/// its pictures. Images the input only *names* — a markdown `![](x.png)`
/// — are still the caller's to resolve; see [`render_with_media`].
pub fn convert(input: &[u8], from: Format, to: Format) -> Result<Vec<u8>, Error> {
    if !to.embeds_media() {
        return render(&parse(input, from)?, to);
    }
    let (doc, media) = parse_with_media(input, from)?;
    render_with_media(&doc, to, &|url| media.get(url).cloned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn markdown_to_html() {
        let out = convert(b"*hi*\n", Format::Markdown, Format::Html).unwrap();
        assert_eq!(out, b"<p><em>hi</em></p>\n");
    }

    #[test]
    fn docx_round_trips_through_the_facade() {
        let docx = convert(b"# Title\n\nBody.\n", Format::Markdown, Format::Docx).unwrap();
        let html = convert(&docx, Format::Docx, Format::Html).unwrap();
        let html = String::from_utf8(html).unwrap();
        assert!(html.contains("<h1"), "{html}");
        assert!(html.contains("Body."), "{html}");
    }

    #[test]
    fn only_docx_output_embeds_media() {
        // What guards the memory: reading a document's images costs peak
        // RSS proportional to them, and a `.docx` can hold a part that
        // inflates a thousandfold. Widen this and `docx -> markdown`
        // starts paying for pictures it will never write.
        assert!(Format::Docx.embeds_media());
        for format in [Format::Markdown, Format::Gfm, Format::Html, Format::Json, Format::Plain] {
            assert!(!format.embeds_media(), "{format} does not embed image bytes");
        }
    }

    #[test]
    fn formats_are_named_and_guessed() {
        assert_eq!(Format::parse("CommonMark"), Some(Format::Markdown));
        assert_eq!(Format::from_path(std::path::Path::new("a/b.docx")), Some(Format::Docx));
        assert_eq!(Format::parse("pdf"), None);
    }

    #[test]
    fn unsupported_directions_are_errors_not_panics() {
        assert!(matches!(
            convert(b"x", Format::Plain, Format::Json),
            Err(Error::NotReadable(Format::Plain))
        ));
    }

    #[test]
    fn html_converts_to_markdown() {
        let out = convert(b"<h1>T</h1><ul><li>a</li></ul>", Format::Html, Format::Markdown)
            .expect("html is readable");
        assert_eq!(String::from_utf8(out).unwrap(), "# T\n\n- a\n");
    }

    #[test]
    fn docx_converts_to_markdown() {
        let docx = convert(b"# Title\n\nBody *text*.\n", Format::Markdown, Format::Docx)
            .expect("writable");
        let md = convert(&docx, Format::Docx, Format::Markdown).expect("convertible");
        let md = String::from_utf8(md).expect("utf8");
        assert!(md.contains("# Title"), "{md}");
        assert!(md.contains("*text*"), "{md}");
    }

    #[test]
    fn a_table_survives_docx_to_gfm() {
        // The workflow the README sells. `markdown` has no table syntax,
        // so the same conversion loses the grid there and `gfm` keeps it.
        let source = b"| Region | Q1 |\n|--------|---:|\n| North  | 120 |\n";
        let docx = convert(source, Format::Gfm, Format::Docx).expect("writable");
        let gfm = String::from_utf8(convert(&docx, Format::Docx, Format::Gfm).unwrap()).unwrap();
        assert!(gfm.contains("| Region | Q1 |"), "{gfm}");
        assert!(gfm.contains("| North | 120 |"), "{gfm}");
        let md = String::from_utf8(convert(&docx, Format::Docx, Format::Markdown).unwrap()).unwrap();
        assert!(!md.contains('|'), "{md}");
    }

    #[test]
    fn images_survive_docx_to_docx() {
        // A `.docx` is the one input that carries its pictures inside
        // itself, so it is the one conversion that can keep them without
        // the caller resolving anything.
        let png: &[u8] = &[
            0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a, 0, 0, 0, 13, b'I', b'H', b'D', b'R',
            0, 0, 0, 1, 0, 0, 0, 1, 8, 6, 0, 0, 0, 0x1f, 0x15, 0xc4, 0x89, 0, 0, 0, 0, b'I',
            b'E', b'N', b'D', 0xae, 0x42, 0x60, 0x82,
        ];
        let doc = parse(b"![alt](pic.png)\n", Format::Markdown).unwrap();
        let first = render_with_media(&doc, Format::Docx, &|url| {
            (url == "pic.png").then(|| png.to_vec())
        })
        .unwrap();

        let (_, media) = parse_with_media(&first, Format::Docx).unwrap();
        assert_eq!(media.len(), 1, "the reader hands back what the package holds");

        let second = convert(&first, Format::Docx, Format::Docx).unwrap();
        let (_, again) = parse_with_media(&second, Format::Docx).unwrap();
        assert_eq!(again.values().next().map(Vec::as_slice), Some(png));

        // A reader with nothing to carry hands back an empty bag rather
        // than making the caller ask which formats have one.
        assert!(parse_with_media(b"# t\n", Format::Markdown).unwrap().1.is_empty());
    }

    #[test]
    fn malformed_input_is_reported_not_panicked() {
        assert!(convert(b"not a zip", Format::Docx, Format::Html).is_err());
        assert!(convert(b"{oops", Format::Json, Format::Html).is_err());
    }
}
