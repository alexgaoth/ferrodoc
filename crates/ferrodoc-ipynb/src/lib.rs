//! Jupyter notebook (`.ipynb`) reader and writer producing the ferrodoc
//! (pandoc-compatible) AST.
//!
//! [`read_ipynb`] maps a notebook to the same AST `pandoc -f ipynb -t json`
//! produces (differentially verified by `ferrodoc-harness diff-ipynb`);
//! [`write_ipynb`] emits nbformat 4.5 that Jupyter, `nbformat.validate` and
//! `pandoc -f ipynb` all accept.
//!
//! A notebook is JSON whose markdown cells are markdown, so **the reader
//! that does the work already exists** — this crate is the notebook around
//! `ferrodoc-markdown`. What it adds is the cell and output structure, and
//! every rule below was measured against the pandoc 3.8.2.1 binary rather
//! than guessed:
//!
//! - **one `Div` per cell**, identified by the cell's `id`, classed
//!   `cell markdown` / `cell code` / `cell raw`, carrying the cell's
//!   metadata as key-value attributes with `execution_count` folded in
//!   beside them and the whole set sorted by key;
//! - **each output is a nested `Div`** classed `output <output_type>`, with
//!   the stream name appended (`output stream stdout`), `execution_count`
//!   on an `execute_result` and `ename`/`evalue` on an `error`;
//! - a **mime bundle contributes exactly one block**, chosen by a fixed
//!   preference — image, then JSON, then plain text, then HTML, LaTeX and
//!   markdown. `text/plain` beating `text/html` is pandoc's order, not an
//!   obvious one, and it is what the binary does;
//! - an **image output is extracted, not inlined**: the AST names
//!   `<sha1 of the bytes>.<ext>` and the bytes go in the media bag, which
//!   is how pandoc's media bag names them too;
//! - **a raw cell becomes `RawBlock`**, in the format its `format` or
//!   `raw_mimetype` metadata names, and `ipynb` when it names none;
//! - notebook metadata, `nbformat` and `nbformat_minor` land under a
//!   single `jupyter` metadata key.
//!
//! Known gaps, deliberate and unfixed:
//!
//! - a markdown cell is read as GFM, which is the closest reader this
//!   project has to pandoc's ipynb extension set. Four constructs differ
//!   and are listed in `COMPATIBILITY.md`;
//! - the writer derives each missing cell `id` from the cell's own content
//!   rather than drawing it at random, so a notebook written twice from
//!   one AST is byte-identical. Pandoc's is random, so neither can match
//!   the other.

mod read;
mod sha1;
mod write;

pub use read::{read_ipynb, read_ipynb_with_media};
pub use write::{write_ipynb, write_ipynb_with_media, write_ipynb_wrapped};

use std::collections::HashMap;

/// An error reading or writing a notebook.
#[derive(Debug)]
pub enum Error {
    /// The bytes are not JSON, or are nested deeper than the reader will go.
    Json(String),
    /// The JSON is well-formed but is not a notebook.
    NotANotebook(&'static str),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Json(e) => write!(f, "not a readable notebook: {e}"),
            Error::NotANotebook(what) => write!(f, "not a notebook: {what}"),
        }
    }
}

impl std::error::Error for Error {}

/// The image bytes a notebook carries, keyed by the URL its AST names them
/// by.
pub type Media = HashMap<String, Vec<u8>>;

/// How deep a metadata value may nest before the reader refuses it.
///
/// Notebook metadata is arbitrary JSON, and converting it to `MetaValue`
/// is the one recursive walk in this crate. A test thread gets 2 MiB, so
/// the bound is kept low the way every reader here keeps it.
const MAX_META_DEPTH: usize = 100;

/// Decode base64, ignoring the line breaks nbformat wraps it at.
fn from_base64(text: &str) -> Option<Vec<u8>> {
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

/// Encode base64, wrapped at 76 characters with a trailing newline.
///
/// The wrapping is not cosmetic: it is what pandoc and `nbconvert` both
/// emit, so a notebook written here diffs against one written there in
/// content rather than in line length.
fn to_base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut raw = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let mut buf = [0u8; 3];
        buf[..chunk.len()].copy_from_slice(chunk);
        let n = u32::from(buf[0]) << 16 | u32::from(buf[1]) << 8 | u32::from(buf[2]);
        for i in 0..4 {
            if i <= chunk.len() {
                raw.push(char::from(ALPHABET[(n >> (18 - 6 * i) & 0x3f) as usize]));
            } else {
                raw.push('=');
            }
        }
    }
    let mut out = String::with_capacity(raw.len() + raw.len() / 76 + 1);
    let mut rest = raw.as_str();
    while rest.len() > 76 {
        let (line, tail) = rest.split_at(76);
        out.push_str(line);
        out.push('\n');
        rest = tail;
    }
    out.push_str(rest);
    out.push('\n');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_round_trips_every_tail_length() {
        for len in 0..40usize {
            let bytes: Vec<u8> = (0..len).map(|i| u8::try_from(i * 7 % 251).unwrap()).collect();
            let encoded = to_base64(&bytes);
            assert_eq!(from_base64(&encoded).as_deref(), Some(&bytes[..]), "len {len}");
        }
    }

    #[test]
    fn base64_wraps_at_seventy_six() {
        let encoded = to_base64(&[0u8; 120]);
        for line in encoded.trim_end().split('\n') {
            assert!(line.len() <= 76, "line of {} chars", line.len());
        }
        assert!(encoded.ends_with('\n'));
    }
}
