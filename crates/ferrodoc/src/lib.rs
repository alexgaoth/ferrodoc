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
    /// **Pandoc's** markdown, which is not `CommonMark`: a YAML metadata
    /// block, header attributes, definition lists and
    /// superscript/subscript on top of what [`Format::Gfm`] reads.
    ///
    /// A separate name rather than a change to [`Format::Markdown`],
    /// because the two disagree on documents valid in both and a silent
    /// change of meaning is worse than a flag someone has to type.
    /// Readable only: writing it would be a second markdown writer, and
    /// [`Format::Markdown`] already round-trips what the AST can hold.
    PandocMarkdown,
    /// HTML. Readable and writable.
    Html,
    /// Office Open XML word processing documents. Readable and writable.
    Docx,
    /// `OpenDocument` text, what `LibreOffice` and `OpenOffice` write.
    /// Readable and writable.
    Odt,
    /// EPUB, the e-book format. Readable and writable.
    Epub,
    /// Jupyter notebooks (`.ipynb`). Readable and writable.
    Ipynb,
    /// LaTeX. Writable — `ferrodoc x.docx -t latex | pdflatex` is PDF
    /// output for anyone with TeX. Deliberately never readable: a `.tex`
    /// file expands user-defined macros, which is a language, not a
    /// format.
    Latex,
    /// reStructuredText. Writable — it feeds Sphinx. Not readable, and
    /// deliberately: people write RST by hand and convert *out* of it.
    Rst,
    /// `AsciiDoc`. Writable — it feeds Asciidoctor and Antora. Not
    /// readable, for the same reason as RST; pandoc does not read it
    /// either.
    Asciidoc,
    /// The pandoc JSON AST. Readable and writable.
    Json,
    /// Unformatted text extraction. Writable.
    Plain,
}

impl Format {
    /// Every format name accepted on the command line, in help order.
    pub const NAMES: &'static [&'static str] = &[
        "markdown", "commonmark", "gfm", "pandoc_markdown", "html", "docx", "odt", "epub",
        "ipynb", "latex", "rst", "asciidoc", "json", "plain",
    ];

    /// Parse a format name, accepting pandoc's spellings.
    pub fn parse(name: &str) -> Option<Self> {
        match name.to_ascii_lowercase().as_str() {
            "markdown" | "commonmark" | "md" => Some(Format::Markdown),
            "gfm" | "markdown_github" => Some(Format::Gfm),
            "pandoc_markdown" | "pandoc-markdown" => Some(Format::PandocMarkdown),
            // `html5` is pandoc's own spelling and produces identical
            // bytes there; a Makefile writing `-t html5` was refused for
            // a name. `html4` is *not* an alias — pandoc's html4 writer
            // differs on real constructs, so it stays refused by name
            // rather than answered wrongly.
            "html" | "htm" | "html5" => Some(Format::Html),
            "docx" => Some(Format::Docx),
            "odt" => Some(Format::Odt),
            "epub" | "epub2" | "epub3" => Some(Format::Epub),
            "ipynb" | "jupyter" | "notebook" => Some(Format::Ipynb),
            "latex" | "tex" => Some(Format::Latex),
            "rst" | "rest" | "restructuredtext" => Some(Format::Rst),
            "asciidoc" | "adoc" | "asciidoctor" => Some(Format::Asciidoc),
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
        !matches!(self, Format::Plain | Format::Latex | Format::Rst | Format::Asciidoc)
    }


    /// Whether this build has the code for this format at all.
    ///
    /// Every format is compiled in by default. A caller who trimmed the
    /// feature list to pay for less gets `false` here — and a conversion
    /// that asks for one anyway fails with [`Error::NotCompiled`] rather
    /// than with a wrong answer. The CLI's `--help` lists exactly the
    /// formats this returns `true` for.
    pub fn compiled(self) -> bool {
        match self {
            Format::Markdown | Format::Gfm | Format::PandocMarkdown => cfg!(feature = "markdown"),
            Format::Html => cfg!(feature = "html"),
            Format::Docx => cfg!(feature = "docx"),
            Format::Odt => cfg!(feature = "odt"),
            Format::Epub => cfg!(feature = "epub"),
            Format::Ipynb => cfg!(feature = "ipynb"),
            Format::Latex => cfg!(feature = "latex"),
            Format::Rst => cfg!(feature = "rst"),
            Format::Asciidoc => cfg!(feature = "asciidoc"),
            Format::Plain => cfg!(feature = "text"),
            // The AST already serializes, so JSON costs nothing to keep.
            Format::Json => true,
        }
    }

    /// Whether writing this format embeds image bytes.
    ///
    /// Reading a document's media costs memory proportional to it — a
    /// `.docx` can hold a part that inflates a thousandfold — so
    /// [`convert`] asks for it only when the answer here is yes.
    pub fn embeds_media(self) -> bool {
        matches!(self, Format::Docx | Format::Odt | Format::Epub | Format::Ipynb)
    }

    /// Whether documents can be written to this format.
    ///
    /// All but one: `pandoc_markdown` is read only. Writing it would be a
    /// second markdown writer for constructs [`Format::Markdown`] already
    /// round-trips, and a writer nothing gates is worth less than the
    /// error saying it does not exist.
    pub fn writable(self) -> bool {
        self != Format::PandocMarkdown
    }

    /// The name used in messages.
    pub fn name(self) -> &'static str {
        match self {
            Format::Markdown => "markdown",
            Format::Gfm => "gfm",
            Format::PandocMarkdown => "pandoc_markdown",
            Format::Html => "html",
            Format::Docx => "docx",
            Format::Odt => "odt",
            Format::Epub => "epub",
            Format::Ipynb => "ipynb",
            Format::Latex => "latex",
            Format::Rst => "rst",
            Format::Asciidoc => "asciidoc",
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
    /// The format is supported, but this build was compiled without it.
    /// Only a build that trimmed the default feature set can produce
    /// this; see [`Format::compiled`].
    NotCompiled(Format),
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
            Error::NotCompiled(format) => {
                write!(f, "cannot handle {format}: this build was compiled without it")
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
        #[cfg(feature = "docx")]
        Format::Docx => ferrodoc_docx::read_docx_with_media(input).map_err(|e| Error::Invalid {
            format: from,
            detail: e.to_string(),
        }),
        #[cfg(feature = "odt")]
        Format::Odt => ferrodoc_odt::read_odt_with_media(input).map_err(|e| Error::Invalid {
            format: from,
            detail: e.to_string(),
        }),
        #[cfg(feature = "epub")]
        Format::Epub => ferrodoc_epub::read_epub_with_media(input).map_err(|e| Error::Invalid {
            format: from,
            detail: e.to_string(),
        }),
        #[cfg(feature = "ipynb")]
        Format::Ipynb => {
            let text = String::from_utf8(input.to_vec()).map_err(|e| Error::Invalid {
                format: from,
                detail: format!("not UTF-8: {e}"),
            })?;
            ferrodoc_ipynb::read_ipynb_with_media(&text).map_err(|e| Error::Invalid {
                format: from,
                detail: e.to_string(),
            })
        }
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
        #[cfg(feature = "markdown")]
        Format::Markdown => ferrodoc_markdown::read_commonmark(&text(input)?).map_err(|e| Error::Invalid {
            format: from,
            detail: e.to_string(),
        }),
        #[cfg(not(feature = "markdown"))]
        Format::Markdown => Err(Error::NotCompiled(from)),
        #[cfg(feature = "markdown")]
        Format::Gfm => ferrodoc_markdown::read_gfm(&text(input)?).map_err(|e| Error::Invalid {
            format: from,
            detail: e.to_string(),
        }),
        #[cfg(not(feature = "markdown"))]
        Format::Gfm => Err(Error::NotCompiled(from)),
        #[cfg(feature = "markdown")]
        Format::PandocMarkdown => {
            ferrodoc_markdown::read_pandoc_markdown(&text(input)?).map_err(|e| Error::Invalid {
                format: from,
                detail: e.to_string(),
            })
        }
        #[cfg(not(feature = "markdown"))]
        Format::PandocMarkdown => Err(Error::NotCompiled(from)),
        #[cfg(feature = "docx")]
        Format::Docx => ferrodoc_docx::read_docx(input).map_err(|e| Error::Invalid {
            format: from,
            detail: e.to_string(),
        }),
        #[cfg(not(feature = "docx"))]
        Format::Docx => Err(Error::NotCompiled(from)),
        #[cfg(feature = "odt")]
        Format::Odt => ferrodoc_odt::read_odt(input).map_err(|e| Error::Invalid {
            format: from,
            detail: e.to_string(),
        }),
        #[cfg(not(feature = "odt"))]
        Format::Odt => Err(Error::NotCompiled(from)),
        #[cfg(feature = "epub")]
        Format::Epub => ferrodoc_epub::read_epub(input).map_err(|e| Error::Invalid {
            format: from,
            detail: e.to_string(),
        }),
        #[cfg(not(feature = "epub"))]
        Format::Epub => Err(Error::NotCompiled(from)),
        #[cfg(feature = "ipynb")]
        Format::Ipynb => ferrodoc_ipynb::read_ipynb(&text(input)?).map_err(|e| Error::Invalid {
            format: from,
            detail: e.to_string(),
        }),
        #[cfg(not(feature = "ipynb"))]
        Format::Ipynb => Err(Error::NotCompiled(from)),
        Format::Json => serde_json::from_slice(input).map_err(|e| Error::Invalid {
            format: from,
            detail: e.to_string(),
        }),
        #[cfg(feature = "html")]
        Format::Html => ferrodoc_html::read_html(&text(input)?).map_err(|e| Error::Invalid {
            format: from,
            detail: e.to_string(),
        }),
        #[cfg(not(feature = "html"))]
        Format::Html => Err(Error::NotCompiled(from)),
        Format::Plain => Err(Error::NotReadable(Format::Plain)),
        Format::Latex => Err(Error::NotReadable(Format::Latex)),
        Format::Rst => Err(Error::NotReadable(Format::Rst)),
        Format::Asciidoc => Err(Error::NotReadable(Format::Asciidoc)),
    }
}

// The three helpers below decode a `data:` URL for the writers that embed
// image bytes. No other format asks, so a build without one of those has no
// use for them and would warn that they are dead.
/// The bytes a `data:` URL carries, or `None` if it is not one.
///
/// Both spellings, because both appear in real pages: base64, which is
/// what this crate's own HTML reader writes for an inline `<svg>`, and
/// percent-encoded, which is what a hand-written SVG data URL usually is.
#[cfg(any(feature = "docx", feature = "odt", feature = "epub", feature = "ipynb"))]
fn data_url(url: &str) -> Option<Vec<u8>> {
    let rest = url.strip_prefix("data:")?;
    let (media_type, data) = rest.split_once(',')?;
    // The token is case-insensitive; RFC 2397 spells it lowercase and
    // real pages do not always agree.
    if media_type
        .rsplit(';')
        .next()
        .is_some_and(|token| token.eq_ignore_ascii_case("base64"))
    {
        return base64(data);
    }
    percent_decode(data)
}

/// Standard base64, with or without padding.
///
/// ASCII whitespace is skipped rather than refused: a `data:` URL long
/// enough to hold a picture is routinely wrapped across lines, a newline
/// inside an attribute value is legal HTML, and both the WHATWG's
/// "forgiving base64" and RFC 2397 strip it before decoding. Refusing it
/// lost the picture on exactly the generated pages this is for.
///
/// Any *other* character outside the alphabet makes the whole thing not an
/// image. A payload that simply stops early still yields what it held, as
/// it does for pandoc — there is no way to tell a truncated picture from a
/// short one.
#[cfg(any(feature = "docx", feature = "odt", feature = "epub", feature = "ipynb"))]
fn base64(text: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(text.len() / 4 * 3);
    let (mut bits, mut held) = (0u32, 0u32);
    for byte in text.bytes() {
        let value = match byte {
            b'A'..=b'Z' => u32::from(byte - b'A'),
            b'a'..=b'z' => u32::from(byte - b'a') + 26,
            b'0'..=b'9' => u32::from(byte - b'0') + 52,
            b'+' => 62,
            b'/' => 63,
            b'=' => break,
            byte if byte.is_ascii_whitespace() => continue,
            _ => return None,
        };
        bits = bits << 6 | value;
        held += 6;
        if held >= 8 {
            held -= 8;
            out.push(u8::try_from(bits >> held & 0xff).ok()?);
        }
    }
    Some(out)
}

/// Percent-decoding, for a `data:` URL that spells its payload out.
#[cfg(any(feature = "docx", feature = "odt", feature = "epub", feature = "ipynb"))]
fn percent_decode(text: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(text.len());
    let mut bytes = text.bytes();
    while let Some(byte) = bytes.next() {
        if byte == b'%' {
            let mut digits = [0u8; 2];
            for digit in &mut digits {
                *digit = bytes.next()?;
            }
            // `from_str_radix` would take a leading `+`, turning the
            // malformed `%+1` into a byte rather than a refusal.
            if !digits.iter().all(u8::is_ascii_hexdigit) {
                return None;
            }
            let text = std::str::from_utf8(&digits).ok()?;
            out.push(u8::from_str_radix(text, 16).ok()?);
        } else {
            out.push(byte);
        }
    }
    Some(out)
}

/// Write a document. Images are not embedded; see [`render_with_media`].
pub fn render(doc: &Pandoc, to: Format) -> Result<Vec<u8>, Error> {
    render_with_media(doc, to, &|_| None)
}

/// Render, filling text to `columns` where the writer can fill.
///
/// This is pandoc's `--wrap=auto --columns N`. Only the markdown writers
/// fill today; every other format renders exactly as [`render`] would, so
/// the flag is accepted rather than refused for them and simply has no
/// effect. Nothing here embeds media, because no format that fills does.
///
/// # Errors
///
/// The same as [`render`].
pub fn render_wrapped(doc: &Pandoc, to: Format, columns: usize) -> Result<Vec<u8>, Error> {
    match to {
        #[cfg(feature = "markdown")]
        Format::Markdown => Ok(ferrodoc_markdown::write_markdown_wrapped(doc, columns).into_bytes()),
        #[cfg(feature = "markdown")]
        Format::Gfm => Ok(ferrodoc_markdown::write_gfm_wrapped(doc, columns).into_bytes()),
        _ => render(doc, to),
    }
}

/// Write a document, embedding every image whose bytes `media` can supply
/// for its URL.
///
/// Resolving a URL is the caller's job: it may name a file on disk, a
/// cache, or nothing at all, and this crate has no business guessing. Only
/// DOCX output embeds media today; other formats ignore the resolver.
#[cfg_attr(not(any(feature = "docx", feature = "odt", feature = "epub", feature = "ipynb")), allow(unused_variables))]
pub fn render_with_media(
    doc: &Pandoc,
    to: Format,
    media: &dyn Fn(&str) -> Option<Vec<u8>>,
) -> Result<Vec<u8>, Error> {
    match to {
        #[cfg(feature = "markdown")]
        Format::Markdown => Ok(ferrodoc_markdown::write_markdown(doc).into_bytes()),
        #[cfg(not(feature = "markdown"))]
        Format::Markdown => Err(Error::NotCompiled(to)),
        #[cfg(feature = "markdown")]
        Format::Gfm => Ok(ferrodoc_markdown::write_gfm(doc).into_bytes()),
        #[cfg(not(feature = "markdown"))]
        Format::Gfm => Err(Error::NotCompiled(to)),
        Format::PandocMarkdown => Err(Error::NotWritable(to)),
        #[cfg(feature = "html")]
        Format::Html => Ok(ferrodoc_html::write_html(doc).into_bytes()),
        #[cfg(not(feature = "html"))]
        Format::Html => Err(Error::NotCompiled(to)),
        #[cfg(feature = "text")]
        Format::Plain => Ok(ferrodoc_text::write_text(doc).into_bytes()),
        #[cfg(not(feature = "text"))]
        Format::Plain => Err(Error::NotCompiled(to)),
        #[cfg(feature = "latex")]
        Format::Latex => Ok(ferrodoc_latex::write_latex(doc).into_bytes()),
        #[cfg(not(feature = "latex"))]
        Format::Latex => Err(Error::NotCompiled(to)),
        #[cfg(feature = "rst")]
        Format::Rst => Ok(ferrodoc_rst::write_rst(doc).into_bytes()),
        #[cfg(not(feature = "rst"))]
        Format::Rst => Err(Error::NotCompiled(to)),
        #[cfg(feature = "asciidoc")]
        Format::Asciidoc => Ok(ferrodoc_asciidoc::write_asciidoc(doc).into_bytes()),
        #[cfg(not(feature = "asciidoc"))]
        Format::Asciidoc => Err(Error::NotCompiled(to)),
        #[cfg(feature = "docx")]
        Format::Docx => {
            // A `data:` URL carries its own bytes, so it is answered here
            // rather than passed to a resolver that would look for a file
            // of that name. Without this an inline `<svg>`, or any image
            // a page embeds rather than links, reaches the DOCX writer as
            // a URL nothing can resolve and comes out as alt text.
            let resolve = |url: &str| data_url(url).or_else(|| media(url));
            ferrodoc_docx::write_docx_with_media(doc, &resolve).map_err(|e| Error::Invalid {
                format: to,
                detail: e.to_string(),
            })
        }
        #[cfg(not(feature = "docx"))]
        Format::Docx => Err(Error::NotCompiled(to)),
        #[cfg(feature = "odt")]
        Format::Odt => {
            let resolve = |url: &str| data_url(url).or_else(|| media(url));
            ferrodoc_odt::write_odt_with_media(doc, &resolve).map_err(|e| Error::Invalid {
                format: to,
                detail: e.to_string(),
            })
        }
        #[cfg(not(feature = "odt"))]
        Format::Odt => Err(Error::NotCompiled(to)),
        #[cfg(feature = "epub")]
        Format::Epub => {
            let resolve = |url: &str| data_url(url).or_else(|| media(url));
            ferrodoc_epub::write_epub_with_media(doc, &resolve).map_err(|e| Error::Invalid {
                format: to,
                detail: e.to_string(),
            })
        }
        #[cfg(not(feature = "epub"))]
        Format::Epub => Err(Error::NotCompiled(to)),
        #[cfg(feature = "ipynb")]
        Format::Ipynb => {
            let resolve = |url: &str| data_url(url).or_else(|| media(url));
            ferrodoc_ipynb::write_ipynb_with_media(doc, &resolve).map_err(|e| Error::Invalid {
                format: to,
                detail: e.to_string(),
            })
        }
        #[cfg(not(feature = "ipynb"))]
        Format::Ipynb => Err(Error::NotCompiled(to)),
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

/// Render a document as a complete LaTeX file rather than a fragment:
/// preamble, `\begin{document}`, and the title block if the document
/// carries one.
///
/// This is what `pdflatex` can compile on its own; [`render`] to
/// [`Format::Latex`] gives the body alone, for someone else's template.
#[cfg(feature = "latex")]
pub fn render_latex_standalone(doc: &Pandoc) -> String {
    ferrodoc_latex::write_latex_standalone(doc)
}

/// Render a document as a complete HTML page rather than a fragment.
///
/// [`render`] to [`Format::Html`] emits the body only, which is what a
/// template engine wants and what a browser does not. `css`, if given, is
/// inlined into a `<style>` element; reading it from a file is the
/// caller's job, because no crate below this one does IO. With `toc`, the
/// page opens with pandoc's `<nav id="TOC" role="doc-toc">`.
#[cfg(feature = "html")]
pub fn render_html_standalone(doc: &Pandoc, css: Option<&str>, toc: bool) -> Vec<u8> {
    ferrodoc_html::write_html_standalone(doc, css, toc).into_bytes()
}

/// Number a document's headings, as pandoc's `--number-sections` does.
///
/// The markup is HTML's — a `data-number` attribute and a
/// `header-section-number` span — so this is an HTML-output transform, and
/// every other writer will render the span as whatever it makes of one.
#[cfg(feature = "html")]
pub fn number_sections(doc: &mut Pandoc) {
    ferrodoc_html::number_sections(doc);
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

// Each test below is gated on the formats it converts: a build trimmed with
// cargo features still runs its own tests, and skips only what it does not
// contain.
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[cfg(all(feature = "markdown", feature = "html"))]
    fn markdown_to_html() {
        let out = convert(b"*hi*\n", Format::Markdown, Format::Html).unwrap();
        assert_eq!(out, b"<p><em>hi</em></p>\n");
    }

    #[test]
    #[cfg(all(feature = "markdown", feature = "docx", feature = "html"))]
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
        // The spellings a real Makefile uses. `dropin/` found `html5`
        // being refused for its name alone; `pandoc -t html5` and
        // `pandoc -t html` write the same bytes.
        assert_eq!(Format::parse("html5"), Some(Format::Html));
        assert_eq!(Format::parse("markdown_github"), Some(Format::Gfm));
        // Not aliases: pandoc's html4 writer differs on real constructs
        // and `markdown_strict` is a different dialect, so both are
        // refused by name rather than answered wrongly.
        assert_eq!(Format::parse("html4"), None);
        assert_eq!(Format::parse("markdown_strict"), None);
    }

    #[test]
    fn unsupported_directions_are_errors_not_panics() {
        assert!(matches!(
            convert(b"x", Format::Plain, Format::Json),
            Err(Error::NotReadable(Format::Plain))
        ));
    }

    #[test]
    #[cfg(all(feature = "markdown", feature = "html"))]
    fn html_converts_to_markdown() {
        let out = convert(b"<h1>T</h1><ul><li>a</li></ul>", Format::Html, Format::Markdown)
            .expect("html is readable");
        assert_eq!(String::from_utf8(out).unwrap(), "# T\n\n- a\n");
    }

    #[test]
    #[cfg(all(feature = "markdown", feature = "docx"))]
    fn docx_converts_to_markdown() {
        let docx = convert(b"# Title\n\nBody *text*.\n", Format::Markdown, Format::Docx)
            .expect("writable");
        let md = convert(&docx, Format::Docx, Format::Markdown).expect("convertible");
        let md = String::from_utf8(md).expect("utf8");
        assert!(md.contains("# Title"), "{md}");
        assert!(md.contains("*text*"), "{md}");
    }

    #[test]
    #[cfg(all(feature = "markdown", feature = "docx"))]
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
    #[cfg(all(feature = "markdown", feature = "docx"))]
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

    /// A picture a page carries rather than links has no file behind it,
    /// so a resolver has nothing to look up and the DOCX writer used to
    /// fall back to alt text. It is the whole reason an inline `<svg>`
    /// reaches a `.docx` at all.
    #[test]
    #[cfg(all(feature = "html", feature = "docx"))]
    fn a_data_url_carries_its_own_picture_into_a_package() {
        let html = concat!(
            r#"<svg xmlns="http://www.w3.org/2000/svg" width="9" height="9">"#,
            r#"<circle cx="4" cy="4" r="4"></circle></svg>"#,
        );
        // Nothing resolves a URL here: the bytes are in the document.
        let docx = convert(html.as_bytes(), Format::Html, Format::Docx).unwrap();
        let (_, media) = parse_with_media(&docx, Format::Docx).unwrap();
        let bytes = media.values().next().expect("the package holds the picture");
        assert_eq!(String::from_utf8_lossy(bytes), html);
    }

    #[test]
    #[cfg(any(feature = "docx", feature = "odt", feature = "epub", feature = "ipynb"))]
    fn a_data_url_is_read_in_both_spellings() {
        assert_eq!(data_url("data:image/svg+xml;base64,Zm9vYmFy").as_deref(), Some(&b"foobar"[..]));
        assert_eq!(data_url("data:text/plain,a%20b%2Fc").as_deref(), Some(&b"a b/c"[..]));
        // The token is case-insensitive, and padding is optional.
        assert_eq!(data_url("data:image/png;BASE64,Zm9vYmE=").as_deref(), Some(&b"fooba"[..]));
        assert_eq!(data_url("data:image/png;base64,Zm9vYmE").as_deref(), Some(&b"fooba"[..]));
        // A URL long enough to hold a picture is routinely wrapped, and a
        // newline inside an attribute value is legal HTML. Refusing it
        // lost the picture on exactly the pages this is for.
        assert_eq!(
            data_url("data:image/png;base64,Zm9v\n  YmFy").as_deref(),
            Some(&b"foobar"[..])
        );
        // Not a data URL, a payload that is not base64 at all, no comma,
        // and a percent escape that is not one.
        assert_eq!(data_url("pic.png"), None);
        assert_eq!(data_url("data:image/png;base64,not_base64!"), None);
        assert_eq!(data_url("data:image/png;base64"), None);
        assert_eq!(data_url("data:text/plain,a%+1b"), None);
        assert_eq!(data_url("data:text/plain,a%2"), None);
    }

    #[test]
    #[cfg(all(feature = "docx", feature = "html"))]
    fn malformed_input_is_reported_not_panicked() {
        assert!(convert(b"not a zip", Format::Docx, Format::Html).is_err());
        assert!(convert(b"{oops", Format::Json, Format::Html).is_err());
    }
}
