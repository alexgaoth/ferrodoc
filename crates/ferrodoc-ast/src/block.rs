//! Block-level document elements.

use crate::table::{Caption, Table};
use crate::{Attr, Format, Inline};
use serde::{Deserialize, Serialize};

/// A block element of a document.
///
/// Mirrors pandoc-types `Block`; serializes as `{"t": <constructor>}` or
/// `{"t": <constructor>, "c": <contents>}`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "t", content = "c")]
pub enum Block {
    /// Plain text, not a paragraph (e.g. tight list items).
    Plain(Vec<Inline>),
    /// A paragraph.
    Para(Vec<Inline>),
    /// Multiple non-breaking lines, each a sequence of inlines.
    LineBlock(Vec<Vec<Inline>>),
    /// A code block with attributes.
    CodeBlock(Attr, String),
    /// Raw content in the given format, passed through verbatim.
    RawBlock(Format, String),
    /// A block quotation.
    BlockQuote(Vec<Block>),
    /// An ordered list: numbering attributes plus items.
    OrderedList(ListAttributes, Vec<Vec<Block>>),
    /// A bullet list.
    BulletList(Vec<Vec<Block>>),
    /// A definition list: each item is a term plus one or more definitions.
    DefinitionList(Vec<(Vec<Inline>, Vec<Vec<Block>>)>),
    /// A section heading: level, attributes, text.
    Header(i64, Attr, Vec<Inline>),
    /// A horizontal rule.
    HorizontalRule,
    /// A table (boxed to keep the enum small; JSON shape is unchanged).
    Table(Box<Table>),
    /// A figure: attributes, caption, content.
    Figure(Attr, Caption, Vec<Block>),
    /// A generic block container with attributes.
    Div(Attr, Vec<Block>),
}

/// Numbering attributes of an [`Block::OrderedList`]: start number, style,
/// delimiter.
///
/// Pandoc's `ListAttributes` is the tuple `(start, style, delim)`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(from = "ListAttributesRepr", into = "ListAttributesRepr")]
pub struct ListAttributes {
    /// The number of the first list item.
    pub start: i64,
    /// The numbering style.
    pub style: ListNumberStyle,
    /// The delimiter after the number.
    pub delim: ListNumberDelim,
}

type ListAttributesRepr = (i64, ListNumberStyle, ListNumberDelim);

impl From<ListAttributesRepr> for ListAttributes {
    fn from((start, style, delim): ListAttributesRepr) -> Self {
        ListAttributes { start, style, delim }
    }
}

impl From<ListAttributes> for ListAttributesRepr {
    fn from(a: ListAttributes) -> Self {
        (a.start, a.style, a.delim)
    }
}

/// The numbering style of an ordered list.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "t", content = "c")]
pub enum ListNumberStyle {
    /// Style left to the writer.
    DefaultStyle,
    /// Numbered examples (pandoc's `@` lists).
    Example,
    /// Decimal numbers.
    Decimal,
    /// Lowercase roman numerals.
    LowerRoman,
    /// Uppercase roman numerals.
    UpperRoman,
    /// Lowercase letters.
    LowerAlpha,
    /// Uppercase letters.
    UpperAlpha,
}

/// The delimiter after an ordered-list number.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "t", content = "c")]
pub enum ListNumberDelim {
    /// Delimiter left to the writer.
    DefaultDelim,
    /// A period, as in `1.`.
    Period,
    /// A closing parenthesis, as in `1)`.
    OneParen,
    /// Enclosing parentheses, as in `(1)`.
    TwoParens,
}
