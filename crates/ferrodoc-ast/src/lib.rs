//! Pandoc-compatible document AST with lossless pandoc-JSON interop.
//!
//! The types in this crate mirror [pandoc-types] 1.23.x exactly, and their
//! serde implementations produce and consume the JSON emitted by
//! `pandoc -t json` (pandoc 3.x). Any document pandoc can represent,
//! this crate can represent, byte-for-byte at the JSON value level.
//!
//! One deliberate deviation from a literal transcription of the Haskell
//! types: tuples-with-meaning (`Attr`, `Target`, `ListAttributes`, …) are
//! named structs here for API ergonomics; their serde form is still the
//! positional array pandoc emits.
//!
//! Known limitations: JSON numbers that are integral floats (a `ColWidth`
//! of `1`) re-serialize as `1.0`, which is numerically equal but not
//! value-equal under `serde_json`; and documents nested deeper than
//! `serde_json`'s recursion limit (~128 levels) fail to deserialize —
//! `serde_json::Value` itself cannot represent such documents either.
//!
//! [pandoc-types]: https://hackage.haskell.org/package/pandoc-types

mod attr;
mod block;
mod inline;
mod math;
mod meta;
mod table;

pub use attr::{Attr, Format, Target};
pub use block::{Block, ListAttributes, ListNumberDelim, ListNumberStyle};
pub use inline::{Citation, CitationMode, Inline, MathType, QuoteType};
/// TeX rendered as inlines, exposed for the **three writers that render
/// math** — `commonmark`, `html` and `plain`.
///
/// Not part of the supported surface — `#[doc(hidden)]`, like
/// `ferrodoc-html`'s `highlight`, which the LaTeX writer is built on for
/// the same reason: it is machinery two crates share, not a promise
/// about this one's API.
#[doc(hidden)]
pub use math::tex_inlines;
pub use meta::{Meta, MetaValue};
pub use table::{
    Alignment, Caption, Cell, ColSpec, ColWidth, Row, Table, TableBody, TableFoot, TableHead,
};

use serde::{Deserialize, Serialize};

/// The pandoc-types API version this crate targets, emitted as
/// `"pandoc-api-version"` in the JSON representation.
pub const API_VERSION: [u32; 3] = [1, 23, 1];

/// A complete document: metadata plus a sequence of blocks.
///
/// Serializes to the top-level object of `pandoc -t json` output:
/// `{"pandoc-api-version":[1,23,1],"meta":{…},"blocks":[…]}`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Pandoc {
    /// The pandoc-types version of the JSON representation.
    #[serde(rename = "pandoc-api-version")]
    pub api_version: Vec<u32>,
    /// Document metadata (title, authors, arbitrary fields).
    pub meta: Meta,
    /// The document body.
    pub blocks: Vec<Block>,
}

impl Pandoc {
    /// Create a document with the given blocks, empty metadata, and the
    /// current [`API_VERSION`].
    pub fn new(blocks: Vec<Block>) -> Self {
        Pandoc {
            api_version: API_VERSION.to_vec(),
            meta: Meta::default(),
            blocks,
        }
    }
}

impl Default for Pandoc {
    fn default() -> Self {
        Pandoc::new(Vec::new())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn nullary_inline_serializes_without_content() {
        assert_eq!(serde_json::to_value(Inline::Space).unwrap(), json!({"t": "Space"}));
    }

    #[test]
    fn str_serializes_with_content() {
        assert_eq!(
            serde_json::to_value(Inline::Str("hi".into())).unwrap(),
            json!({"t": "Str", "c": "hi"})
        );
    }

    #[test]
    fn attr_serializes_as_positional_array() {
        let attr = Attr {
            identifier: "id".into(),
            classes: vec!["a".into()],
            attributes: vec![("k".into(), "v".into())],
        };
        assert_eq!(
            serde_json::to_value(Inline::Code(Box::new(attr), "x".into())).unwrap(),
            json!({"t": "Code", "c": [["id", ["a"], [["k", "v"]]], "x"]})
        );
    }

    #[test]
    fn empty_document_round_trips() {
        let doc = Pandoc::default();
        let v = serde_json::to_value(&doc).unwrap();
        assert_eq!(
            v,
            json!({"pandoc-api-version": [1, 23, 1], "meta": {}, "blocks": []})
        );
        let back: Pandoc = serde_json::from_value(v).unwrap();
        assert_eq!(back, doc);
    }
}
