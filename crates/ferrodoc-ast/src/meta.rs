//! Document metadata.

use crate::{Block, Inline};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Document metadata: a map from field names to [`MetaValue`]s.
///
/// Mirrors pandoc's `Meta` (a sorted map); serializes as a JSON object.
pub type Meta = BTreeMap<String, MetaValue>;

/// A metadata value.
///
/// Mirrors pandoc-types `MetaValue`; serializes as
/// `{"t": <constructor>, "c": <contents>}`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "t", content = "c")]
pub enum MetaValue {
    /// A nested map of metadata values.
    MetaMap(BTreeMap<String, MetaValue>),
    /// A list of metadata values.
    MetaList(Vec<MetaValue>),
    /// A boolean.
    MetaBool(bool),
    /// A plain string.
    MetaString(String),
    /// A sequence of inlines (e.g. a parsed title).
    MetaInlines(Vec<Inline>),
    /// A sequence of blocks.
    MetaBlocks(Vec<Block>),
}
