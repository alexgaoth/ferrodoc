//! Placeholder; the writer lands once the reader is gated.
use crate::Error;
use ferrodoc_ast::Pandoc;

/// Render a document as an `.epub` package.
///
/// # Errors
///
/// Only [`Error::Zip`], if the in-memory archive cannot be assembled.
pub fn write_epub(doc: &Pandoc) -> Result<Vec<u8>, Error> {
    write_epub_with_media(doc, &|_| None)
}

/// Render a document as an `.epub`, embedding what `media` supplies.
///
/// # Errors
///
/// Only [`Error::Zip`], if the in-memory archive cannot be assembled.
pub fn write_epub_with_media(
    _doc: &Pandoc,
    _media: &dyn Fn(&str) -> Option<Vec<u8>>,
) -> Result<Vec<u8>, Error> {
    Ok(Vec::new())
}
