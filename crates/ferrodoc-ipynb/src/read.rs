//! The notebook reader.
//!
//! Every rule here was probed against pandoc 3.8.2.1 with `-t json` and is
//! commented with what the binary does, not with what the nbformat schema
//! permits — the two disagree in several places and the binary is the
//! oracle `diff-ipynb` scores against.

use crate::{Error, MAX_META_DEPTH, Media, from_base64, sha1};
use ferrodoc_ast::{Attr, Block, Inline, Meta, MetaValue, Pandoc, Target};
use serde_json::{Map, Value};
use std::collections::BTreeMap;

/// Read a notebook into a [`Pandoc`] AST equivalent to pandoc's ipynb
/// reader output.
///
/// # Errors
///
/// [`Error::Json`] if the input is not JSON or nests deeper than the
/// reader will go, [`Error::NotANotebook`] if it is JSON without a `cells`
/// array.
pub fn read_ipynb(input: &str) -> Result<Pandoc, Error> {
    read(input).map(|(doc, _)| doc)
}

/// Read a notebook together with the bytes of every image it embeds.
///
/// The keys are the URLs the AST names — `<sha1>.png` for an output image,
/// `<cell id>-<name>` for a markdown-cell attachment.
///
/// # Errors
///
/// The same as [`read_ipynb`]. An output whose base64 does not decode is
/// left out of the bag rather than failing the read.
pub fn read_ipynb_with_media(input: &str) -> Result<(Pandoc, Media), Error> {
    read(input)
}

fn read(input: &str) -> Result<(Pandoc, Media), Error> {
    // serde_json refuses input nested past its own recursion limit with an
    // error rather than a stack overflow, which is this reader's outer
    // bound; `MAX_META_DEPTH` is the inner one, over the value tree.
    let notebook: Value = serde_json::from_str(input).map_err(|e| Error::Json(e.to_string()))?;
    let notebook = notebook.as_object().ok_or(Error::NotANotebook("not a JSON object"))?;
    let cells = notebook
        .get("cells")
        .and_then(Value::as_array)
        .ok_or(Error::NotANotebook("no cells array"))?;

    let metadata = notebook.get("metadata").and_then(Value::as_object);
    let language = metadata
        .and_then(|m| m.get("kernelspec"))
        .and_then(|k| k.get("language"))
        .and_then(Value::as_str)
        // `kernelspec.language` wins over `language_info.name`: a notebook
        // giving `kernelspec.language = R` and `language_info.name =
        // python` reads as R.
        .or_else(|| {
            metadata
                .and_then(|m| m.get("language_info"))
                .and_then(|l| l.get("name"))
                .and_then(Value::as_str)
        })
        // And a notebook with no metadata at all still reads its code
        // blocks as Python, which is pandoc's default rather than an
        // empty class list.
        .unwrap_or("python")
        .to_owned();

    let mut media = Media::new();
    let mut blocks = Vec::with_capacity(cells.len());
    for cell in cells {
        if let Some(cell) = cell.as_object() {
            blocks.push(read_cell(cell, &language, &mut media)?);
        }
    }

    Ok((Pandoc { meta: read_meta(notebook, metadata)?, blocks, ..Pandoc::default() }, media))
}

/// Notebook metadata lands whole under one `jupyter` key, with `nbformat`
/// and `nbformat_minor` folded in beside it.
fn read_meta(notebook: &Map<String, Value>, metadata: Option<&Map<String, Value>>) -> Result<Meta, Error> {
    let mut jupyter: BTreeMap<String, MetaValue> = BTreeMap::new();
    if let Some(metadata) = metadata {
        for (key, value) in metadata {
            jupyter.insert(key.clone(), meta_value(value, 0)?);
        }
    }
    for key in ["nbformat", "nbformat_minor"] {
        if let Some(value) = notebook.get(key) {
            jupyter.insert(key.to_owned(), meta_value(value, 0)?);
        }
    }
    Ok(Meta::from([("jupyter".to_owned(), MetaValue::MetaMap(jupyter))]))
}

/// JSON to `MetaValue`, pandoc's way: only booleans keep their type. A
/// number becomes the string of its own spelling and **null becomes the
/// empty string**, which is not what the same value becomes in a cell
/// attribute (there it is the four characters `null`).
fn meta_value(value: &Value, depth: usize) -> Result<MetaValue, Error> {
    if depth > MAX_META_DEPTH {
        return Err(Error::Json(format!("metadata nested deeper than {MAX_META_DEPTH}")));
    }
    Ok(match value {
        Value::Object(fields) => MetaValue::MetaMap(
            fields
                .iter()
                .map(|(k, v)| Ok((k.clone(), meta_value(v, depth + 1)?)))
                .collect::<Result<BTreeMap<_, _>, Error>>()?,
        ),
        Value::Array(items) => MetaValue::MetaList(
            items.iter().map(|v| meta_value(v, depth + 1)).collect::<Result<Vec<_>, Error>>()?,
        ),
        Value::Bool(b) => MetaValue::MetaBool(*b),
        Value::Null => MetaValue::MetaString(String::new()),
        Value::Number(n) => MetaValue::MetaString(n.to_string()),
        Value::String(s) => MetaValue::MetaString(s.clone()),
    })
}

fn read_cell(cell: &Map<String, Value>, language: &str, media: &mut Media) -> Result<Block, Error> {
    let cell_type = cell.get("cell_type").and_then(Value::as_str).unwrap_or("markdown");
    let identifier = cell.get("id").and_then(Value::as_str).unwrap_or_default().to_owned();
    let source = text_of(cell.get("source"));

    // The cell's own metadata becomes its key-value attributes, sorted by
    // key — and `execution_count` leads them rather than sorting in among
    // them, which is why a cell with an `arr` key still lists
    // `execution_count` first.
    let mut attributes: Vec<(String, String)> = Vec::new();
    if let Some(count) = cell.get("execution_count").and_then(Value::as_i64) {
        attributes.push(("execution_count".to_owned(), count.to_string()));
    }
    if let Some(metadata) = cell.get("metadata").and_then(Value::as_object) {
        let sorted: BTreeMap<&String, &Value> = metadata.iter().collect();
        attributes.extend(sorted.into_iter().map(|(k, v)| (k.clone(), attribute_value(v))));
    }
    let attr = Attr {
        identifier: identifier.clone(),
        classes: vec!["cell".to_owned(), cell_type.to_owned()],
        attributes,
    };

    let blocks = match cell_type {
        "code" => {
            let mut blocks = vec![Block::CodeBlock(
                Attr { classes: vec![language.to_owned()], ..Attr::default() },
                source,
            )];
            if let Some(outputs) = cell.get("outputs").and_then(Value::as_array) {
                for output in outputs {
                    if let Some(output) = output.as_object() {
                        blocks.push(read_output(output, media));
                    }
                }
            }
            blocks
        }
        "raw" => {
            // A raw cell names its format in `format` or `raw_mimetype`;
            // the well-known mime types get pandoc's short name and
            // anything else is carried through verbatim. With neither, the
            // format is `ipynb` — content only this notebook understands.
            let format = cell
                .get("metadata")
                .and_then(|m| m.get("format").or_else(|| m.get("raw_mimetype")))
                // The *attribute* spelling, not the raw string: a `format`
                // of `""` labels the block `""`, the two characters, and
                // not the empty format.
                .map(attribute_value)
                .map_or_else(|| "ipynb".to_owned(), |mime| raw_format(&mime).to_owned());
            vec![Block::RawBlock(ferrodoc_ast::Format(format), source)]
        }
        // Everything else is markdown, which is also what pandoc does with
        // a cell type it does not know.
        _ => {
            let mut blocks = ferrodoc_markdown::read_gfm(&source)
                .map_err(|e| Error::Json(e.to_string()))?
                .blocks;
            let attachments = cell.get("attachments").and_then(Value::as_object);
            fixup_markdown(&mut blocks, &identifier, attachments, media);
            blocks
        }
    };
    Ok(Block::Div(attr, blocks))
}

/// A mime type to the format name pandoc labels a `RawBlock` with.
fn raw_format(mime: &str) -> &str {
    match mime {
        "text/html" => "html",
        "text/latex" => "latex",
        "text/markdown" => "markdown",
        "text/restructuredtext" => "rst",
        "text/asciidoc" => "asciidoc",
        other => other,
    }
}

/// A cell metadata value as an attribute string: a non-empty string is
/// used as it stands, and everything else — including the empty string —
/// is its compact JSON spelling.
fn attribute_value(value: &Value) -> String {
    match value {
        Value::String(s) if !s.is_empty() => s.clone(),
        other => other.to_string(),
    }
}

fn read_output(output: &Map<String, Value>, media: &mut Media) -> Block {
    let output_type = output.get("output_type").and_then(Value::as_str).unwrap_or_default();
    let mut classes = vec!["output".to_owned(), output_type.to_owned()];
    let mut attributes = Vec::new();
    let blocks = match output_type {
        "stream" => {
            // The stream's name is a third class, whatever it is: `stdout`
            // and `stderr` are not special-cased.
            classes.push(output.get("name").and_then(Value::as_str).unwrap_or("stdout").to_owned());
            vec![Block::CodeBlock(Attr::default(), text_of(output.get("text")))]
        }
        "error" => {
            for key in ["ename", "evalue"] {
                if let Some(value) = output.get(key).and_then(Value::as_str) {
                    attributes.push((key.to_owned(), value.to_owned()));
                }
            }
            let traceback: String = output
                .get("traceback")
                .and_then(Value::as_array)
                .map(|lines| {
                    lines.iter().filter_map(Value::as_str).fold(String::new(), |mut out, line| {
                        out.push_str(line);
                        out.push('\n');
                        out
                    })
                })
                .unwrap_or_default();
            vec![Block::CodeBlock(Attr::default(), strip_ansi(&traceback))]
        }
        _ => {
            if let Some(count) = output.get("execution_count").and_then(Value::as_i64) {
                attributes.push(("execution_count".to_owned(), count.to_string()));
            }
            let data = output.get("data").and_then(Value::as_object);
            let metadata = output.get("metadata").and_then(Value::as_object);
            data.map(|data| read_data(data, metadata, media)).unwrap_or_default()
        }
    };
    Block::Div(Attr { classes, attributes, ..Attr::default() }, blocks)
}

/// A mime bundle contributes **exactly one** block, not one per entry.
///
/// The order is pandoc's and it is not the obvious one: `text/plain` beats
/// `text/html`, so a `DataFrame` that renders as both comes out as its
/// repr rather than its table. Probed each way against the binary.
fn read_data(
    data: &Map<String, Value>,
    metadata: Option<&Map<String, Value>>,
    media: &mut Media,
) -> Vec<Block> {
    // An image is extracted rather than inlined, and the name is the
    // sha1 of the bytes — which is how pandoc's media bag names it, so
    // two notebooks embedding one picture name it once.
    if let Some((mime, value)) = data
        .iter()
        .find(|(mime, _)| mime.starts_with("image/") || *mime == "application/pdf")
        && let Some(bytes) = from_base64(&text_of(Some(value)))
    {
        let url = format!("{}.{}", sha1::hex(&bytes), image_extension(mime));
        // An output's `metadata` is keyed by mime type, and the entry for
        // *this* mime becomes the image's attributes — which is where
        // `width` and `height` come from.
        let attributes = metadata
            .and_then(|m| m.get(mime))
            .and_then(Value::as_object)
            .map(|fields| {
                fields
                    .iter()
                    .map(|(k, v)| (k.clone(), attribute_value(v)))
                    .collect::<BTreeMap<_, _>>()
                    .into_iter()
                    .collect()
            })
            .unwrap_or_default();
        media.insert(url.clone(), bytes);
        return vec![Block::Para(vec![Inline::Image(
            Box::new(Attr { attributes, ..Attr::default() }),
            Vec::new(),
            Box::new(Target { url, title: String::new() }),
        )])];
    }
    // Any `+json` flavour, whose value is JSON rather than lines of text:
    // a list stays a list here instead of being joined.
    if let Some((_, value)) = data.iter().find(|(mime, _)| mime.ends_with("json")) {
        return vec![Block::CodeBlock(
            Attr { classes: vec!["json".to_owned()], ..Attr::default() },
            value.to_string(),
        )];
    }
    if let Some(value) = data.get("text/plain") {
        return vec![Block::CodeBlock(Attr::default(), text_of(Some(value)))];
    }
    for (mime, format) in [("text/html", "html"), ("text/latex", "latex"), ("text/markdown", "markdown")] {
        if let Some(value) = data.get(mime) {
            return vec![Block::RawBlock(ferrodoc_ast::Format(format.to_owned()), text_of(Some(value)))];
        }
    }
    Vec::new()
}

fn image_extension(mime: &str) -> &str {
    match mime {
        "image/png" => "png",
        "image/jpeg" => "jpg",
        "image/gif" => "gif",
        "image/svg+xml" => "svg",
        "application/pdf" => "pdf",
        other => other.rsplit('/').next().unwrap_or("bin"),
    }
}

/// Two corrections a markdown cell needs after the GFM reader has run.
///
/// **Ordered lists lose their numbering style.** Pandoc's ipynb markdown
/// has neither `fancy_lists` nor `startnum`, so the only ordered list it
/// can parse is `N.` and every one of them comes back as
/// `(1, DefaultStyle, DefaultDelim)` — a list starting at 3 starts at 1.
/// GFM keeps both, so they are flattened here.
///
/// **`![](attachment:name)` names a media-bag key**, which is the cell's
/// own id and the attachment name. An attachment the cell does not
/// actually carry keeps its name with the scheme removed: pandoc does not
/// invent a key for it, and a percent-encoded reference to a name with a
/// space is exactly that case.
fn fixup_markdown(
    blocks: &mut [Block],
    cell_id: &str,
    attachments: Option<&Map<String, Value>>,
    media: &mut Media,
) {
    let mut resolve = |target: &mut Target| {
        let Some(name) = target.url.strip_prefix("attachment:") else { return };
        let name = name.to_owned();
        match attachments.and_then(|a| a.get(&name)).and_then(Value::as_object) {
            Some(bundle) => {
                let url = format!("{cell_id}-{name}");
                if let Some(bytes) = bundle.values().next().and_then(|v| from_base64(&text_of(Some(v)))) {
                    media.insert(url.clone(), bytes);
                }
                target.url = url;
            }
            None => target.url = name,
        }
    };
    // Iterative rather than recursive: the walk is over a tree the
    // markdown reader already bounded, but a bound a walk can bypass
    // protects nothing, so this one does not recurse at all.
    let mut stack: Vec<&mut Block> = blocks.iter_mut().collect();
    let mut inlines: Vec<&mut Inline> = Vec::new();
    while let Some(block) = stack.pop() {
        match block {
            Block::Plain(items) | Block::Para(items) | Block::Header(_, _, items) => {
                inlines.extend(items.iter_mut());
            }
            Block::BlockQuote(children) | Block::Div(_, children) => {
                stack.extend(children.iter_mut());
            }
            Block::OrderedList(list, items) => {
                *list = ferrodoc_ast::ListAttributes {
                    start: 1,
                    style: ferrodoc_ast::ListNumberStyle::DefaultStyle,
                    delim: ferrodoc_ast::ListNumberDelim::DefaultDelim,
                };
                stack.extend(items.iter_mut().flatten());
            }
            Block::BulletList(items) => stack.extend(items.iter_mut().flatten()),
            Block::DefinitionList(items) => {
                for (term, definitions) in items {
                    inlines.extend(term.iter_mut());
                    stack.extend(definitions.iter_mut().flatten());
                }
            }
            Block::Figure(_, _, children) => stack.extend(children.iter_mut()),
            _ => {}
        }
    }
    while let Some(inline) = inlines.pop() {
        match inline {
            Inline::Image(_, alt, target) => {
                resolve(target);
                inlines.extend(alt.iter_mut());
            }
            Inline::Link(attr, items, target) => {
                autolink_class(attr, items, target);
                inlines.extend(items.iter_mut());
            }
            Inline::Emph(items)
            | Inline::Strong(items)
            | Inline::Strikeout(items)
            | Inline::Underline(items)
            | Inline::Span(_, items) => inlines.extend(items.iter_mut()),
            _ => {}
        }
    }
}

/// Pandoc's ipynb reader runs markdown with `autolink_bare_uris`, which
/// classes an autolink: a bare or `<…>` URI becomes `Link ("",["uri"],[])`
/// and a bare address `Link ("",["email"],[])` with a `mailto:` target. An
/// explicit `[text](url)` gets no class. Probed against 3.8.2.1; note its
/// *gfm* reader adds neither class, which is why this lives here and not in
/// `read_gfm`.
///
/// Comrak does not record whether a link was written as an autolink, so the
/// test is the one that distinguishes them in the source: an autolink's text
/// *is* its target. `[https://x](https://x)`, written out in full, is
/// therefore classed here where pandoc leaves it bare — recorded in
/// `COMPATIBILITY.md` rather than papered over.
fn autolink_class(attr: &mut Attr, items: &[Inline], target: &Target) {
    if !attr.classes.is_empty() {
        return;
    }
    let [Inline::Str(text)] = items else { return };
    if let Some(address) = target.url.strip_prefix("mailto:") {
        if address == text {
            attr.classes.push("email".to_owned());
        }
    } else if target.url == *text {
        attr.classes.push("uri".to_owned());
    }
}

/// `source`, `text` and a mime-bundle entry are all "a string or a list of
/// strings", and the list is concatenated with nothing between: the lines
/// already carry their own newlines.
fn text_of(value: Option<&Value>) -> String {
    match value {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Array(items)) => items.iter().filter_map(Value::as_str).collect(),
        _ => String::new(),
    }
}

/// Remove ANSI colour from a traceback, the way pandoc does: from each
/// `ESC` up to and including the next `m`, **and to the end of the text
/// when there is no `m` left**.
///
/// That last clause is why `x\x1b[K y\n` becomes `x` and not `x y\n` — a
/// terminal control sequence that is not a colour swallows the rest. It is
/// a quirk, and the gate scores against the quirk.
fn strip_ansi(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(escape) = rest.find('\u{1b}') {
        out.push_str(&rest[..escape]);
        match rest[escape..].find('m') {
            Some(end) => rest = &rest[escape + end + 1..],
            None => return out,
        }
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ansi_colour_goes_and_an_unterminated_sequence_eats_the_tail() {
        assert_eq!(strip_ansi("\u{1b}[0;31ma\u{1b}[0m\n"), "a\n");
        // No ESC, so nothing is stripped even though it looks like colour.
        assert_eq!(strip_ansi("[1mb[0m"), "[1mb[0m");
        assert_eq!(strip_ansi("x\u{1b}[K y\n"), "x");
    }

    #[test]
    fn a_cell_becomes_a_div_with_sorted_attributes() {
        let doc = read_ipynb(
            r#"{"cells":[{"cell_type":"code","execution_count":3,"id":"a1",
                 "metadata":{"tags":["t"],"scrolled":true},"outputs":[],"source":["x"]}],
                 "metadata":{},"nbformat":4,"nbformat_minor":5}"#,
        )
        .expect("reads");
        let Block::Div(attr, blocks) = &doc.blocks[0] else { panic!("not a Div") };
        assert_eq!(attr.identifier, "a1");
        assert_eq!(attr.classes, ["cell", "code"]);
        assert_eq!(
            attr.attributes,
            [
                ("execution_count".to_owned(), "3".to_owned()),
                ("scrolled".to_owned(), "true".to_owned()),
                ("tags".to_owned(), "[\"t\"]".to_owned()),
            ]
        );
        assert_eq!(blocks.len(), 1);
    }

    #[test]
    fn an_output_image_is_named_for_its_sha1() {
        // One transparent pixel, base64 as a notebook writes it.
        let png = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAACklEQVR4nGMAAQAABQABAQ0KLbQAAAAASUVORK5CYII=";
        let (doc, media) = read_ipynb_with_media(&format!(
            r#"{{"cells":[{{"cell_type":"code","execution_count":1,"id":"a1","metadata":{{}},
               "outputs":[{{"output_type":"display_data","metadata":{{}},
                          "data":{{"image/png":"{png}"}}}}],"source":["p()"]}}],
               "metadata":{{}},"nbformat":4,"nbformat_minor":5}}"#
        ))
        .expect("reads");
        let Block::Div(_, blocks) = &doc.blocks[0] else { panic!("not a Div") };
        let Block::Div(attr, output) = &blocks[1] else { panic!("no output Div") };
        assert_eq!(attr.classes, ["output", "display_data"]);
        let Block::Para(inlines) = &output[0] else { panic!("not a Para") };
        let Inline::Image(_, _, target) = &inlines[0] else { panic!("not an Image") };
        assert_eq!(target.url, "f826428407bea07427c15fb8efd5ad4fdd22ad86.png");
        assert!(media.contains_key(&target.url));
    }

    #[test]
    fn metadata_nested_past_the_bound_is_refused_rather_than_overflowing() {
        let deep = format!(
            r#"{{"cells":[],"metadata":{{"a":{}{}}},"nbformat":4,"nbformat_minor":5}}"#,
            "[".repeat(120),
            "]".repeat(120)
        );
        assert!(read_ipynb(&deep).is_err());
    }

    #[test]
    fn json_that_is_not_a_notebook_is_an_error_not_an_empty_document() {
        assert!(read_ipynb("{}").is_err());
        assert!(read_ipynb("[]").is_err());
        assert!(read_ipynb("not json").is_err());
    }
}
