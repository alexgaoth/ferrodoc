//! Attributes, link targets, and raw-content format labels.

use serde::{Deserialize, Serialize};

/// Attributes attached to headers, code, spans, divs, tables, …
///
/// Pandoc's `Attr` is the tuple `(identifier, classes, key-value pairs)`;
/// this struct serializes to exactly that positional array.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(from = "AttrRepr", into = "AttrRepr")]
pub struct Attr {
    /// The element identifier (HTML `id`).
    pub identifier: String,
    /// The element's classes.
    pub classes: Vec<String>,
    /// Remaining key-value attributes.
    pub attributes: Vec<(String, String)>,
}

type AttrRepr = (String, Vec<String>, Vec<(String, String)>);

impl From<AttrRepr> for Attr {
    fn from((identifier, classes, attributes): AttrRepr) -> Self {
        Attr { identifier, classes, attributes }
    }
}

impl From<Attr> for AttrRepr {
    fn from(a: Attr) -> Self {
        (a.identifier, a.classes, a.attributes)
    }
}

/// A link or image target: URL plus title.
///
/// Pandoc's `Target` is the tuple `(url, title)`.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(from = "TargetRepr", into = "TargetRepr")]
pub struct Target {
    /// The URL the link points to (or the image source).
    pub url: String,
    /// The title attribute.
    pub title: String,
}

type TargetRepr = (String, String);

impl From<TargetRepr> for Target {
    fn from((url, title): TargetRepr) -> Self {
        Target { url, title }
    }
}

impl From<Target> for TargetRepr {
    fn from(t: Target) -> Self {
        (t.url, t.title)
    }
}

/// The format of raw content (e.g. `"html"`, `"tex"`).
///
/// Serializes transparently as a JSON string.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Format(pub String);
