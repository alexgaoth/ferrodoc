//! Table structure: captions, column specs, rows, cells.

use crate::{Attr, Block, Inline};
use serde::{Deserialize, Serialize};

/// A complete table: attributes, caption, column specs, head, bodies, foot.
///
/// Pandoc's `Table` constructor takes these six values positionally; this
/// struct serializes to that positional array (the `"c"` of a
/// [`crate::Block::Table`]).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(from = "TableRepr", into = "TableRepr")]
pub struct Table {
    /// The table's attributes.
    pub attr: Attr,
    /// The table's caption.
    pub caption: Caption,
    /// One spec per column: alignment and width.
    pub colspecs: Vec<ColSpec>,
    /// The table head.
    pub head: TableHead,
    /// The table bodies.
    pub bodies: Vec<TableBody>,
    /// The table foot.
    pub foot: TableFoot,
}

type TableRepr = (Attr, Caption, Vec<ColSpec>, TableHead, Vec<TableBody>, TableFoot);

impl From<TableRepr> for Table {
    fn from((attr, caption, colspecs, head, bodies, foot): TableRepr) -> Self {
        Table { attr, caption, colspecs, head, bodies, foot }
    }
}

impl From<Table> for TableRepr {
    fn from(t: Table) -> Self {
        (t.attr, t.caption, t.colspecs, t.head, t.bodies, t.foot)
    }
}

/// A table or figure caption: an optional short form plus the full caption.
///
/// Pandoc's `Caption` serializes as the array `[short_caption_or_null, blocks]`.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(from = "CaptionRepr", into = "CaptionRepr")]
pub struct Caption {
    /// The optional short caption.
    pub short: Option<Vec<Inline>>,
    /// The full caption.
    pub blocks: Vec<Block>,
}

type CaptionRepr = (Option<Vec<Inline>>, Vec<Block>);

impl From<CaptionRepr> for Caption {
    fn from((short, blocks): CaptionRepr) -> Self {
        Caption { short, blocks }
    }
}

impl From<Caption> for CaptionRepr {
    fn from(c: Caption) -> Self {
        (c.short, c.blocks)
    }
}

/// The alignment of a table column or cell.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "t", content = "c")]
pub enum Alignment {
    /// Left-aligned.
    AlignLeft,
    /// Right-aligned.
    AlignRight,
    /// Centered.
    AlignCenter,
    /// Alignment left to the writer.
    AlignDefault,
}

/// The width of a table column, as a fraction of the total width.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(tag = "t", content = "c")]
pub enum ColWidth {
    /// A fixed fraction of the text width.
    ColWidth(f64),
    /// Width left to the writer.
    ColWidthDefault,
}

/// The specification of a single table column: alignment plus width.
///
/// Pandoc's `ColSpec` is the tuple `(alignment, colwidth)`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(from = "ColSpecRepr", into = "ColSpecRepr")]
pub struct ColSpec {
    /// The column's alignment.
    pub alignment: Alignment,
    /// The column's width.
    pub width: ColWidth,
}

type ColSpecRepr = (Alignment, ColWidth);

impl From<ColSpecRepr> for ColSpec {
    fn from((alignment, width): ColSpecRepr) -> Self {
        ColSpec { alignment, width }
    }
}

impl From<ColSpec> for ColSpecRepr {
    fn from(c: ColSpec) -> Self {
        (c.alignment, c.width)
    }
}

/// A table row: attributes plus cells.
///
/// Pandoc's `Row` serializes as the array `[attr, cells]`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(from = "RowRepr", into = "RowRepr")]
pub struct Row {
    /// The row's attributes.
    pub attr: Attr,
    /// The row's cells.
    pub cells: Vec<Cell>,
}

type RowRepr = (Attr, Vec<Cell>);

impl From<RowRepr> for Row {
    fn from((attr, cells): RowRepr) -> Self {
        Row { attr, cells }
    }
}

impl From<Row> for RowRepr {
    fn from(r: Row) -> Self {
        (r.attr, r.cells)
    }
}

/// A table cell.
///
/// Pandoc's `Cell` serializes as the array
/// `[attr, alignment, rowspan, colspan, blocks]`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(from = "CellRepr", into = "CellRepr")]
pub struct Cell {
    /// The cell's attributes.
    pub attr: Attr,
    /// The cell's alignment (overrides the column's).
    pub alignment: Alignment,
    /// The number of rows the cell spans.
    pub row_span: i64,
    /// The number of columns the cell spans.
    pub col_span: i64,
    /// The cell's content.
    pub blocks: Vec<Block>,
}

type CellRepr = (Attr, Alignment, i64, i64, Vec<Block>);

impl From<CellRepr> for Cell {
    fn from((attr, alignment, row_span, col_span, blocks): CellRepr) -> Self {
        Cell { attr, alignment, row_span, col_span, blocks }
    }
}

impl From<Cell> for CellRepr {
    fn from(c: Cell) -> Self {
        (c.attr, c.alignment, c.row_span, c.col_span, c.blocks)
    }
}

/// The head of a table: attributes plus header rows.
///
/// Serializes as the array `[attr, rows]`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(from = "TableHeadRepr", into = "TableHeadRepr")]
pub struct TableHead {
    /// The head's attributes.
    pub attr: Attr,
    /// The header rows.
    pub rows: Vec<Row>,
}

type TableHeadRepr = (Attr, Vec<Row>);

impl From<TableHeadRepr> for TableHead {
    fn from((attr, rows): TableHeadRepr) -> Self {
        TableHead { attr, rows }
    }
}

impl From<TableHead> for TableHeadRepr {
    fn from(h: TableHead) -> Self {
        (h.attr, h.rows)
    }
}

/// A body of a table: attributes, the number of row-header columns, the
/// intermediate head rows, and the body rows.
///
/// Serializes as the array `[attr, row_head_columns, head, body]`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(from = "TableBodyRepr", into = "TableBodyRepr")]
pub struct TableBody {
    /// The body's attributes.
    pub attr: Attr,
    /// How many leading columns act as row headers.
    pub row_head_columns: i64,
    /// The intermediate head rows of this body.
    pub head: Vec<Row>,
    /// The body rows.
    pub body: Vec<Row>,
}

type TableBodyRepr = (Attr, i64, Vec<Row>, Vec<Row>);

impl From<TableBodyRepr> for TableBody {
    fn from((attr, row_head_columns, head, body): TableBodyRepr) -> Self {
        TableBody { attr, row_head_columns, head, body }
    }
}

impl From<TableBody> for TableBodyRepr {
    fn from(b: TableBody) -> Self {
        (b.attr, b.row_head_columns, b.head, b.body)
    }
}

/// The foot of a table: attributes plus footer rows.
///
/// Serializes as the array `[attr, rows]`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(from = "TableFootRepr", into = "TableFootRepr")]
pub struct TableFoot {
    /// The foot's attributes.
    pub attr: Attr,
    /// The footer rows.
    pub rows: Vec<Row>,
}

type TableFootRepr = (Attr, Vec<Row>);

impl From<TableFootRepr> for TableFoot {
    fn from((attr, rows): TableFootRepr) -> Self {
        TableFoot { attr, rows }
    }
}

impl From<TableFoot> for TableFootRepr {
    fn from(f: TableFoot) -> Self {
        (f.attr, f.rows)
    }
}
