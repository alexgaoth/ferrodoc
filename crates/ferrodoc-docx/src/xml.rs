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

/// The children of the first `<w:body>`, one subtree at a time.
///
/// The tree of a whole `word/document.xml` costs about twenty times the
/// XML — it is millions of small allocations — and nothing needs it: the
/// reader scans the body forwards with one element of lookahead. Yielding
/// one child at a time keeps peak memory proportional to the largest
/// paragraph rather than to the document.
///
/// # Errors
///
/// The same malformed-XML errors [`parse`] reports, raised as the reader
/// reaches them rather than up front.
pub fn body_children(xml: &str) -> Result<BodyChildren<'_>, Error> {
    let mut reader = Reader::from_str(xml);
    // Everything up to the body's first child is skipped, but neither a
    // malformed prologue nor a missing body may pass silently: an empty
    // document is a wrong answer where an error is a true one.
    loop {
        match reader.read_event().map_err(|e| Error::Xml(e.to_string()))? {
            Event::Start(e) if local_name(e.name().as_ref()) == "body" => {
                // Parsed and thrown away: the body's own attributes carry
                // nothing, but a malformed one is malformed XML and the
                // whole-tree parser this replaced said so.
                node_from_start(&e)?;
                return Ok(BodyChildren { reader, done: false });
            }
            // A self-closing body is a document with nothing in it.
            Event::Empty(e) if local_name(e.name().as_ref()) == "body" => {
                node_from_start(&e)?;
                return Ok(BodyChildren { reader, done: true });
            }
            Event::Eof => return Err(Error::MissingPart("w:body")),
            _ => {}
        }
    }
}

/// The iterator [`body_children`] returns.
pub struct BodyChildren<'a> {
    reader: Reader<&'a [u8]>,
    done: bool,
}

impl Iterator for BodyChildren<'_> {
    type Item = Result<Node, Error>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.done {
            return None;
        }
        // One child of the body: everything from its start tag to the
        // matching end tag, and nothing else stays alive.
        let mut stack: Vec<Node> = Vec::new();
        let mut finished = None;
        while finished.is_none() {
            let event = match self.reader.read_event() {
                Ok(event) => event,
                Err(e) => {
                    self.done = true;
                    return Some(Err(Error::Xml(e.to_string())));
                }
            };
            match consume(event, &mut stack, &mut finished) {
                Ok(Consumed::More) => {}
                // The body's own end tag, or the end of input.
                Ok(Consumed::Done) => {
                    self.done = true;
                    return None;
                }
                Err(e) => {
                    self.done = true;
                    return Some(Err(e));
                }
            }
        }
        finished.map(Ok)
    }
}

enum Consumed {
    More,
    Done,
}

/// Fold one event into the subtree being built. `finished` receives the
/// subtree once its outermost element closes.
fn consume(
    event: Event<'_>,
    stack: &mut Vec<Node>,
    finished: &mut Option<Node>,
) -> Result<Consumed, Error> {
    match event {
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
            if let Some(parent) = stack.last_mut() {
                parent.children.push(Child::Elem(node));
            } else {
                *finished = Some(node);
            }
        }
        Event::End(_) => {
            let Some(node) = stack.pop() else {
                // The body's own end tag: there is nothing after it.
                return Ok(Consumed::Done);
            };
            if let Some(parent) = stack.last_mut() {
                parent.children.push(Child::Elem(node));
            } else {
                *finished = Some(node);
            }
        }
        Event::Text(t) => {
            if let Some(parent) = stack.last_mut() {
                let text = t
                    .xml_content(quick_xml::XmlVersion::default())
                    .map_err(|e| Error::Xml(e.to_string()))?;
                parent.children.push(Child::Text(text.into_owned()));
            }
        }
        Event::GeneralRef(r) => {
            if let Some(parent) = stack.last_mut() {
                parent.children.push(Child::Text(entity(&r)?));
            }
        }
        // The body's own end tag stops the walk through the `End` arm
        // above, so reaching the end of input here means the document was
        // cut off mid-element. Truncating quietly would answer with half
        // a document; erring answers with the truth.
        Event::Eof => {
            return Err(Error::Xml(
                "document ended inside the body: an element is unclosed".into(),
            ));
        }
        _ => {}
    }
    Ok(Consumed::More)
}

/// The text an entity reference stands for.
fn entity(r: &quick_xml::events::BytesRef) -> Result<String, Error> {
    if let Some(ch) = r
        .resolve_char_ref()
        .map_err(|e| Error::Xml(e.to_string()))?
    {
        return Ok(ch.to_string());
    }
    let name: &[u8] = r;
    match name {
        b"lt" => Ok("<".to_owned()),
        b"gt" => Ok(">".to_owned()),
        b"amp" => Ok("&".to_owned()),
        b"apos" => Ok("'".to_owned()),
        b"quot" => Ok("\"".to_owned()),
        other => Err(Error::Xml(format!(
            "unknown entity reference: &{};",
            String::from_utf8_lossy(other)
        ))),
    }
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
