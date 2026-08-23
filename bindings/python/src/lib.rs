//! Python bindings for ferrodoc.
//!
//! The surface is deliberately one function. Everything the CLI can do is
//! a pair of format names, and the pandoc AST is reachable through the
//! `json` format rather than through a second API that would have to be
//! kept in step with it.

use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use pyo3::types::{PyBytes, PyString};

// The exception's own docstring is the fourth argument below; a `///`
// here would be attached to the macro call and dropped.
//
// Named for the `ferrodoc` package rather than the `_ferrodoc` extension
// inside it, because that name has to be importable: pickle resolves an
// exception by `__module__` and `__qualname__`, and a batch running over
// a `ProcessPoolExecutor` can only receive this error if it pickles. One
// corrupt document would otherwise break the pool rather than raise.
pyo3::create_exception!(
    ferrodoc,
    ConversionError,
    PyValueError,
    "A document could not be read as the format it was said to be, or \
     could not be written as the format asked for."
);

/// Whether a format's bytes are text a caller would rather have as `str`.
fn is_text(format: ferrodoc::Format) -> bool {
    // The three zip formats, and only those. This said "everything but
    // `docx`" from the version where `docx` was the only one, so
    // `convert(doc, "gfm", "odt")` raised "invalid utf-8 sequence" and
    // the binding could not produce an ODT or an EPUB **at all** — the
    // wheel could not be built while `ferrodoc 0.2` was unpublished, so
    // no test could reach it.
    //
    // `ipynb` is *not* here on purpose: a notebook is JSON, so it comes
    // back as `str` like every other text format. `Format::embeds_media`
    // is the wrong predicate to reuse — it counts `ipynb` because a
    // notebook carries base64 pictures, which is a different question.
    !matches!(
        format,
        ferrodoc::Format::Docx | ferrodoc::Format::Odt | ferrodoc::Format::Epub
    )
}

fn format(name: &str, role: &str) -> PyResult<ferrodoc::Format> {
    ferrodoc::Format::parse(name).ok_or_else(|| {
        PyValueError::new_err(format!(
            "unknown {role} format {name:?}; known formats: {}",
            ferrodoc::Format::NAMES.join(", ")
        ))
    })
}

/// Convert a document from one format to another.
///
/// `data` may be `str` or `bytes`; DOCX input has to be `bytes`. The
/// result is `str` for a text format and `bytes` for DOCX, which is what
/// a caller wants to write to a file or hand to a model.
#[pyfunction]
#[pyo3(signature = (data, from_format, to_format), text_signature = "(data, from_format, to_format)")]
fn convert<'py>(
    python: Python<'py>,
    data: &Bound<'py, PyAny>,
    from_format: &str,
    to_format: &str,
) -> PyResult<Bound<'py, PyAny>> {
    let from = format(from_format, "input")?;
    let to = format(to_format, "output")?;

    // `bytearray` and `memoryview` are what a caller holding an mmap or a
    // socket read has, and asking them to copy first would be a rule with
    // no reason behind it.
    let owned_text;
    let owned_bytes;
    let bytes: &[u8] = if let Ok(text) = data.downcast::<PyString>() {
        owned_text = text.to_cow()?.into_owned();
        owned_text.as_bytes()
    } else if let Ok(buffer) = data.downcast::<PyBytes>() {
        buffer.as_bytes()
    } else if let Ok(copied) = data.extract::<Vec<u8>>() {
        owned_bytes = copied;
        &owned_bytes
    } else {
        return Err(PyValueError::new_err(format!(
            "data must be str, bytes, bytearray or a buffer, not {}; \
             a DOCX document has to be bytes",
            data.get_type().name()?,
        )));
    };

    // The conversion is pure computation on bytes we already hold, so the
    // GIL is released for it: a caller converting a directory of
    // documents across a thread pool gets the parallelism they asked for.
    let converted = python
        .detach(|| ferrodoc::convert(bytes, from, to))
        .map_err(|error| ConversionError::new_err(error.to_string()))?;

    if is_text(to) {
        let text = String::from_utf8(converted)
            .map_err(|error| ConversionError::new_err(error.to_string()))?;
        return Ok(PyString::new(python, &text).into_any());
    }
    Ok(PyBytes::new(python, &converted).into_any())
}

/// Every format name `convert` knows, in either direction.
///
/// **Not every one of these can be both read and written**, which is why
/// [`read_formats`] and [`write_formats`] exist beside it: `pandoc_markdown`
/// is read-only and `plain`, `latex`, `rst` and `asciidoc` are write-only.
/// Saying "every format `convert` accepts" and meaning the union is what
/// made the test that iterated this fail the moment a read-only format
/// existed.
#[pyfunction]
fn formats() -> Vec<String> {
    named(|_| true)
}

/// The formats a document can be converted **from**.
#[pyfunction]
fn read_formats() -> Vec<String> {
    named(ferrodoc::Format::readable)
}

/// The formats a document can be converted **to**.
#[pyfunction]
fn write_formats() -> Vec<String> {
    named(ferrodoc::Format::writable)
}

fn named(keep: impl Fn(ferrodoc::Format) -> bool) -> Vec<String> {
    ferrodoc::Format::NAMES
        .iter()
        .filter(|name| {
            ferrodoc::Format::parse(name).is_some_and(|format| format.compiled() && keep(format))
        })
        .map(|name| (*name).to_owned())
        .collect()
}

#[pymodule]
fn _ferrodoc(module: &Bound<'_, PyModule>) -> PyResult<()> {
    module.add_function(wrap_pyfunction!(convert, module)?)?;
    module.add_function(wrap_pyfunction!(formats, module)?)?;
    module.add_function(wrap_pyfunction!(read_formats, module)?)?;
    module.add_function(wrap_pyfunction!(write_formats, module)?)?;
    module.add("ConversionError", module.py().get_type::<ConversionError>())?;
    module.add("__version__", env!("CARGO_PKG_VERSION"))?;
    Ok(())
}
