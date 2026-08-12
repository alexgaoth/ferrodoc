//! Universal document converter: read markdown, DOCX or the pandoc JSON
//! AST, write HTML, DOCX, plain text or the pandoc JSON AST.
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
    /// `CommonMark`. Readable.
    Markdown,
    /// HTML. Writable.
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
        "markdown", "commonmark", "html", "docx", "json", "plain",
    ];

    /// Parse a format name, accepting pandoc's spellings.
    pub fn parse(name: &str) -> Option<Self> {
        match name.to_ascii_lowercase().as_str() {
            "markdown" | "commonmark" | "md" => Some(Format::Markdown),
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
        matches!(self, Format::Markdown | Format::Docx | Format::Json)
    }

    /// Whether documents can be written to this format.
    pub fn writable(self) -> bool {
        matches!(self, Format::Html | Format::Docx | Format::Json | Format::Plain)
    }

    /// The name used in messages.
    pub fn name(self) -> &'static str {
        match self {
            Format::Markdown => "markdown",
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
        Format::Docx => ferrodoc_docx::read_docx(input).map_err(|e| Error::Invalid {
            format: from,
            detail: e.to_string(),
        }),
        Format::Json => serde_json::from_slice(input).map_err(|e| Error::Invalid {
            format: from,
            detail: e.to_string(),
        }),
        other @ (Format::Html | Format::Plain) => Err(Error::NotReadable(other)),
    }
}

/// Write a document.
pub fn render(doc: &Pandoc, to: Format) -> Result<Vec<u8>, Error> {
    match to {
        Format::Html => Ok(ferrodoc_html::write_html(doc).into_bytes()),
        Format::Plain => Ok(ferrodoc_text::write_text(doc).into_bytes()),
        Format::Docx => ferrodoc_docx::write_docx(doc).map_err(|e| Error::Invalid {
            format: to,
            detail: e.to_string(),
        }),
        Format::Json => {
            let mut json = serde_json::to_vec(doc).map_err(|e| Error::Invalid {
                format: to,
                detail: e.to_string(),
            })?;
            json.push(b'\n');
            Ok(json)
        }
        other @ Format::Markdown => Err(Error::NotWritable(other)),
    }
}

/// Convert a document from one format to another.
pub fn convert(input: &[u8], from: Format, to: Format) -> Result<Vec<u8>, Error> {
    render(&parse(input, from)?, to)
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
    fn formats_are_named_and_guessed() {
        assert_eq!(Format::parse("CommonMark"), Some(Format::Markdown));
        assert_eq!(Format::from_path(std::path::Path::new("a/b.docx")), Some(Format::Docx));
        assert_eq!(Format::parse("pdf"), None);
    }

    #[test]
    fn unsupported_directions_are_errors_not_panics() {
        assert!(matches!(
            convert(b"x", Format::Html, Format::Json),
            Err(Error::NotReadable(Format::Html))
        ));
        assert!(matches!(
            convert(b"x", Format::Markdown, Format::Markdown),
            Err(Error::NotWritable(Format::Markdown))
        ));
    }

    #[test]
    fn malformed_input_is_reported_not_panicked() {
        assert!(convert(b"not a zip", Format::Docx, Format::Html).is_err());
        assert!(convert(b"{oops", Format::Json, Format::Html).is_err());
    }
}
