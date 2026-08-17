//! Inline-level document elements.

use crate::{Attr, Block, Format, Target};
use serde::{Deserialize, Serialize};

/// An inline element of a document.
///
/// Mirrors pandoc-types `Inline`; serializes as `{"t": <constructor>}` or
/// `{"t": <constructor>, "c": <contents>}`.
///
/// **[`Attr`] and [`Target`] are boxed wherever they appear**, and that is
/// not a style choice. An enum is as wide as its widest variant, and every
/// `Str` and `Space` in a document pays that width: unboxed, `Link` made
/// this type 152 bytes, so a 10 MB document spent 517 MB on the slots
/// alone and peaked at 718 MB — 72× its own size. Boxing the two big
/// payloads takes the type to 56 bytes. `Block::Table` is boxed for the
/// same reason.
///
/// The boxes are invisible in JSON: `serde` writes through them, so the
/// wire format is still exactly pandoc's, which `diff-ast` proves on every
/// run.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "t", content = "c")]
pub enum Inline {
    /// Literal text.
    Str(String),
    /// Emphasized text (usually italics).
    Emph(Vec<Inline>),
    /// Underlined text.
    Underline(Vec<Inline>),
    /// Strongly emphasized text (usually bold).
    Strong(Vec<Inline>),
    /// Struck-out text.
    Strikeout(Vec<Inline>),
    /// Superscripted text.
    Superscript(Vec<Inline>),
    /// Subscripted text.
    Subscript(Vec<Inline>),
    /// Small-caps text.
    SmallCaps(Vec<Inline>),
    /// Quoted text with quote type.
    Quoted(QuoteType, Vec<Inline>),
    /// A citation: the citations plus their textual rendering.
    Cite(Vec<Citation>, Vec<Inline>),
    /// Inline code with attributes.
    ///
    /// The attributes are boxed, as they are on every variant that carries
    /// them. See the note on [`Inline`] itself.
    Code(Box<Attr>, String),
    /// An inter-word space.
    Space,
    /// A soft line break (reflowable).
    SoftBreak,
    /// A hard line break.
    LineBreak,
    /// A TeX math fragment.
    Math(MathType, String),
    /// Raw content in the given format, passed through verbatim.
    RawInline(Format, String),
    /// A hyperlink: attributes, link text, target.
    Link(Box<Attr>, Vec<Inline>, Box<Target>),
    /// An image: attributes, alt text, source.
    Image(Box<Attr>, Vec<Inline>, Box<Target>),
    /// A footnote or endnote.
    Note(Vec<Block>),
    /// A generic inline container with attributes.
    Span(Box<Attr>, Vec<Inline>),
}

/// The style of quotation marks around [`Inline::Quoted`] text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "t", content = "c")]
pub enum QuoteType {
    /// ‘Single’ quotes.
    SingleQuote,
    /// “Double” quotes.
    DoubleQuote,
}

/// Whether a [`Inline::Math`] fragment is display or inline math.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "t", content = "c")]
pub enum MathType {
    /// Display (block-level) math.
    DisplayMath,
    /// Inline math.
    InlineMath,
}

/// A single citation within an [`Inline::Cite`].
///
/// Serializes as an object with pandoc's `citationId`, `citationPrefix`, …
/// field names.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Citation {
    /// The citation key.
    pub citation_id: String,
    /// Inlines preceding the citation.
    pub citation_prefix: Vec<Inline>,
    /// Inlines following the citation (e.g. a locator).
    pub citation_suffix: Vec<Inline>,
    /// How the citation is rendered.
    pub citation_mode: CitationMode,
    /// The note number, when rendered as a note.
    pub citation_note_num: i64,
    /// A hash used internally by citeproc.
    pub citation_hash: i64,
}

/// How a [`Citation`] is rendered.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "t", content = "c")]
pub enum CitationMode {
    /// Author name in text, year in parentheses.
    AuthorInText,
    /// Suppress the author name.
    SuppressAuthor,
    /// The normal parenthetical citation.
    NormalCitation,
}
