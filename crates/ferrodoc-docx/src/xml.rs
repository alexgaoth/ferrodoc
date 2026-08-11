//! Minimal XML tree used to navigate OOXML parts.
//!
//! Element names are stored without their namespace prefix (`w:p` → `p`);
//! attribute names keep the full qualified form (`w:val`, `r:id`) because
//! different namespaces carry different meanings on one element.

use crate::Error;
use quick_xml::Reader;
use quick_xml::events::Event;

/// An XML element.
#[derive(Debug, Default)]
pub struct Node {
    /// Local element name, namespace prefix stripped.
    pub name: String,
    /// Attributes as (qualified name, value).
    pub attrs: Vec<(String, String)>,
    /// Child elements and text, in document order.
    pub children: Vec<Child>,
}

/// A child of an element.
#[derive(Debug)]
pub enum Child {
    /// A nested element.
    Elem(Node),
    /// A text node.
    Text(String),
}

impl Node {
    /// The value of the attribute with the given qualified name.
    pub fn attr(&self, name: &str) -> Option<&str> {
        self.attrs
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.as_str())
    }

    /// Child elements.
    pub fn elems(&self) -> impl Iterator<Item = &Node> {
        self.children.iter().filter_map(|c| match c {
            Child::Elem(n) => Some(n),
            Child::Text(_) => None,
        })
    }

    /// The first child element with the given local name.
    pub fn child(&self, name: &str) -> Option<&Node> {
        self.elems().find(|n| n.name == name)
    }

    /// All child elements with the given local name.
    pub fn children_named<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a Node> {
        self.elems().filter(move |n| n.name == name)
    }

    /// Concatenated text of this element's direct text children.
    pub fn text(&self) -> String {
        let mut out = String::new();
        for c in &self.children {
            if let Child::Text(t) = c {
                out.push_str(t);
            }
        }
        out
    }
}

fn local_name(qname: &[u8]) -> String {
    let name = qname
        .iter()
        .position(|&b| b == b':')
        .map_or(qname, |i| &qname[i + 1..]);
    String::from_utf8_lossy(name).into_owned()
}

/// The deepest element nesting accepted. Real OOXML nests a few dozen
/// levels at most; a deeper document is malformed or hostile, and the
/// recursive walks over [`Node`] (including its drop glue) would otherwise
/// overflow the stack, which aborts the process instead of erroring.
const MAX_DEPTH: usize = 256;

/// Parse an XML document into a tree rooted at its document element.
pub fn parse(xml: &str) -> Result<Node, Error> {
    let mut reader = Reader::from_str(xml);
    let mut stack: Vec<Node> = Vec::new();
    let mut root = None;
    loop {
        match reader.read_event().map_err(|e| Error::Xml(e.to_string()))? {
            Event::Start(e) => {
                if stack.len() >= MAX_DEPTH {
                    return Err(Error::Xml(format!(
                        "element nesting deeper than {MAX_DEPTH} levels"
                    )));
                }
                stack.push(node_from_start(&e)?);
            }
            Event::Empty(e) => {
                let node = node_from_start(&e)?;
                attach(&mut stack, &mut root, node);
            }
            Event::End(_) => {
                let node = stack.pop().expect("well-formed XML nests correctly");
                attach(&mut stack, &mut root, node);
            }
            Event::Text(t) => {
                if let Some(parent) = stack.last_mut() {
                    let text = t
                        .xml_content(quick_xml::XmlVersion::default())
                        .map_err(|e| Error::Xml(e.to_string()))?;
                    parent.children.push(Child::Text(text.into_owned()));
                }
            }
            // Entity references arrive as separate events; dropping them
            // silently loses text like `&lt;` or `&quot;`.
            Event::GeneralRef(r) => {
                if let Some(parent) = stack.last_mut() {
                    let resolved = if let Some(ch) = r
                        .resolve_char_ref()
                        .map_err(|e| Error::Xml(e.to_string()))?
                    {
                        ch.to_string()
                    } else {
                        let name: &[u8] = &r;
                        match name {
                            b"lt" => "<".to_owned(),
                            b"gt" => ">".to_owned(),
                            b"amp" => "&".to_owned(),
                            b"apos" => "'".to_owned(),
                            b"quot" => "\"".to_owned(),
                            other => {
                                return Err(Error::Xml(format!(
                                    "unknown entity reference: &{};",
                                    String::from_utf8_lossy(other)
                                )));
                            }
                        }
                    };
                    parent.children.push(Child::Text(resolved));
                }
            }
            Event::Eof => break,
            _ => {}
        }
    }
    root.ok_or_else(|| Error::Xml("document has no root element".into()))
}

fn node_from_start(e: &quick_xml::events::BytesStart) -> Result<Node, Error> {
    let mut node = Node {
        name: local_name(e.name().as_ref()),
        ..Node::default()
    };
    for attr in e.attributes() {
        let attr = attr.map_err(|e| Error::Xml(e.to_string()))?;
        node.attrs.push((
            String::from_utf8_lossy(attr.key.as_ref()).into_owned(),
            attr.normalized_value(quick_xml::XmlVersion::default())
                .map_err(|e| Error::Xml(e.to_string()))?
                .into_owned(),
        ));
    }
    Ok(node)
}

fn attach(stack: &mut [Node], root: &mut Option<Node>, node: Node) {
    if let Some(parent) = stack.last_mut() {
        parent.children.push(Child::Elem(node));
    } else {
        *root = Some(node);
    }
}
