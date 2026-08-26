//! The notebook writer.
//!
//! Emits nbformat **4.5**: every cell carries an `id`, which is what
//! Jupyter, `nbformat.validate` and pandoc's own writer all produce. The
//! shape of every field was read off `pandoc -f json -t ipynb`, because
//! `diff-ipynb-write` compares our notebook and pandoc's *through pandoc's
//! reader* and a field spelled differently comes back differently.

use crate::{Error, sha1, to_base64};
use ferrodoc_ast::{Attr, Block, Inline, MetaValue, Pandoc};
use serde_json::{Map, Value, json};

/// Write a notebook. Images are named but not embedded; see
/// [`write_ipynb_with_media`].
///
/// # Errors
///
/// Never today; the signature matches the other writers so that a failure
/// mode can be added without a breaking change.
pub fn write_ipynb(doc: &Pandoc) -> Result<Vec<u8>, Error> {
    write_ipynb_with_media(doc, &|_| None)
}

/// Write a notebook, embedding every image whose bytes `media` can supply
/// for its URL.
///
/// An image the resolver cannot answer for is written as an
/// `attachment:`-less markdown image, the way pandoc leaves one it cannot
/// find — the notebook stays valid rather than carrying an empty picture.
///
/// # Errors
///
/// The same as [`write_ipynb`].
pub fn write_ipynb_with_media(
    doc: &Pandoc,
    media: &dyn Fn(&str) -> Option<Vec<u8>>,
) -> Result<Vec<u8>, Error> {
    write_ipynb_wrapped(doc, media, None)
}

/// Write a notebook whose markdown cells are filled to `columns`.
///
/// **A notebook's markdown cell is markdown, and pandoc lays it out.**
/// `--wrap` was ignored here — `auto`, `none` and `preserve` produced the
/// same bytes — because `Format::Ipynb` was classified with DOCX and ODT,
/// where pandoc ignores it too because there are no lines to lay out.
/// There are lines in a markdown cell, and all three of pandoc's modes
/// change them.
///
/// `None` keeps the document's own breaks, which is `--wrap=preserve`.
///
/// # Errors
///
/// The same as [`write_ipynb`].
pub fn write_ipynb_wrapped(
    doc: &Pandoc,
    media: &dyn Fn(&str) -> Option<Vec<u8>>,
    columns: Option<usize>,
) -> Result<Vec<u8>, Error> {
    let mut cells = Vec::new();
    let mut loose: Vec<Block> = Vec::new();
    for block in &doc.blocks {
        match block {
            Block::Div(attr, blocks) if attr.classes.first().is_some_and(|c| c == "cell") => {
                // Blocks outside any cell become one markdown cell, in
                // the order they appeared: that is what makes
                // `ferrodoc notes.md -o notes.ipynb` produce a notebook
                // rather than an empty one.
                if !loose.is_empty() {
                    cells.push(cell(&Attr::default(), "markdown", &std::mem::take(&mut loose), media, columns));
                }
                let kind = attr.classes.get(1).map_or("markdown", String::as_str);
                cells.push(cell(attr, kind, blocks, media, columns));
            }
            other => loose.push(other.clone()),
        }
    }
    if !loose.is_empty() {
        cells.push(cell(&Attr::default(), "markdown", &loose, media, columns));
    }

    let notebook = json!({
        "cells": cells,
        // Always 4.5, and always minor 5 even when the document says
        // otherwise: pandoc forces it too, because a cell `id` — which
        // this writer always emits — is invalid before 4.5.
        "nbformat": 4,
        "nbformat_minor": 5,
        "metadata": notebook_metadata(doc),
    });
    let mut out = serde_json::to_vec_pretty(&notebook).map_err(|e| Error::Json(e.to_string()))?;
    out.push(b'\n');
    Ok(out)
}

/// `meta.jupyter` becomes the notebook's metadata, less the two fields
/// that are not metadata at all.
fn notebook_metadata(doc: &Pandoc) -> Value {
    let Some(MetaValue::MetaMap(fields)) = doc.meta.get("jupyter") else {
        return json!({});
    };
    let mut out = Map::new();
    for (key, value) in fields {
        if key == "nbformat" || key == "nbformat_minor" {
            continue;
        }
        out.insert(key.clone(), meta_json(value));
    }
    Value::Object(out)
}

fn meta_json(value: &MetaValue) -> Value {
    match value {
        MetaValue::MetaMap(fields) => {
            Value::Object(fields.iter().map(|(k, v)| (k.clone(), meta_json(v))).collect())
        }
        MetaValue::MetaList(items) => Value::Array(items.iter().map(meta_json).collect()),
        MetaValue::MetaBool(b) => Value::Bool(*b),
        MetaValue::MetaString(s) => scalar(s),
        // Inlines and blocks have no notebook spelling; nothing this
        // crate's reader produces reaches here.
        MetaValue::MetaInlines(_) | MetaValue::MetaBlocks(_) => Value::Null,
    }
}

fn cell(
    attr: &Attr,
    kind: &str,
    blocks: &[Block],
    media: &dyn Fn(&str) -> Option<Vec<u8>>,
    columns: Option<usize>,
) -> Value {
    let mut metadata = Map::new();
    let mut execution_count = Value::Null;
    for (key, value) in &attr.attributes {
        if key == "execution_count" {
            execution_count = scalar(value);
        } else {
            metadata.insert(key.clone(), scalar(value));
        }
    }

    let mut fields = Map::new();
    fields.insert("cell_type".to_owned(), Value::String(kind.to_owned()));
    let mut attachments = Map::new();
    let source = match kind {
        "code" => blocks
            .iter()
            .find_map(|b| match b {
                Block::CodeBlock(_, text) => Some(text.clone()),
                _ => None,
            })
            .unwrap_or_default(),
        "raw" => {
            // A raw cell's *format* lives in the block, not in the cell
            // metadata that produced it: pandoc rewrites whichever of
            // `format` and `raw_mimetype` the cell had into one
            // `raw_mimetype`, and writes neither when the block is
            // `ipynb` — content no other format claims.
            metadata.remove("format");
            metadata.remove("raw_mimetype");
            if let Some(Block::RawBlock(format, _)) = blocks.first()
                && format.0 != "ipynb"
            {
                metadata.insert("raw_mimetype".to_owned(), Value::String(raw_mimetype(&format.0)));
            }
            blocks
                .iter()
                .map(|b| match b {
                    Block::RawBlock(_, text) => text.clone(),
                    _ => String::new(),
                })
                .collect()
        }
        // GFM, not CommonMark, and the reason is a table: `write_markdown`
        // targets CommonMark, which has no table syntax at all, so a
        // table comes out as one paragraph per cell. Pandoc's ipynb
        // markdown *does* have `pipe_tables` — and `task_lists` and
        // `strikeout` — so GFM is the closer of the two on both sides,
        // matching `read_gfm` in the reader.
        //
        // The markdown writer ends its output with a newline; the last
        // source line does not carry one.
        _ => {
            let mut blocks = blocks.to_vec();
            attachments = detach_images(&mut blocks, media);
            {
                let cell = Pandoc { blocks, ..Pandoc::default() };
                match columns {
                    Some(columns) => ferrodoc_markdown::write_gfm_wrapped(&cell, columns),
                    None => ferrodoc_markdown::write_gfm(&cell),
                }
            }
                .trim_end_matches('\n')
                .to_owned()
        }
    };
    if kind == "code" {
        fields.insert("execution_count".to_owned(), execution_count);
    }
    fields.insert("metadata".to_owned(), Value::Object(metadata));
    if kind == "code" {
        let outputs: Vec<Value> = blocks
            .iter()
            .filter_map(|b| match b {
                Block::Div(attr, inner) if attr.classes.first().is_some_and(|c| c == "output") => {
                    Some(output(attr, inner, media))
                }
                _ => None,
            })
            .collect();
        fields.insert("outputs".to_owned(), Value::Array(outputs));
    }
    fields.insert("source".to_owned(), lines(&source));
    if !attachments.is_empty() {
        fields.insert("attachments".to_owned(), Value::Object(attachments));
    }
    // nbformat 4.5 requires an id on every cell. Pandoc draws one at
    // random; this derives it from the cell's own content, so writing the
    // same AST twice gives the same notebook and a diff means a change.
    let id = if attr.identifier.is_empty() {
        derived_id(&Value::Object(fields.clone()))
    } else {
        attr.identifier.clone()
    };
    fields.insert("id".to_owned(), Value::String(id));
    Value::Object(fields)
}

/// A UUID-shaped identifier derived from the cell, so it is stable across
/// runs. It is deliberately in the same *shape* pandoc's random one has:
/// the gate drops that shape from both sides and nothing else, so a cell
/// that loses a real id still fails.
fn derived_id(cell: &Value) -> String {
    let digest = sha1::hex(cell.to_string().as_bytes());
    format!(
        "{}-{}-4{}-8{}-{}",
        &digest[0..8],
        &digest[8..12],
        &digest[12..15],
        &digest[15..18],
        &digest[18..30]
    )
}

fn output(attr: &Attr, blocks: &[Block], media: &dyn Fn(&str) -> Option<Vec<u8>>) -> Value {
    let kind = attr.classes.get(1).map_or("display_data", String::as_str);
    let mut fields = Map::new();
    fields.insert("output_type".to_owned(), Value::String(kind.to_owned()));
    let text = |blocks: &[Block]| -> String {
        blocks
            .iter()
            .map(|b| match b {
                Block::CodeBlock(_, t) | Block::RawBlock(_, t) => t.clone(),
                _ => String::new(),
            })
            .collect()
    };
    match kind {
        "stream" => {
            fields.insert(
                "name".to_owned(),
                Value::String(attr.classes.get(2).map_or("stdout", String::as_str).to_owned()),
            );
            fields.insert("text".to_owned(), lines(&text(blocks)));
        }
        "error" => {
            for key in ["ename", "evalue"] {
                let value = attr.attributes.iter().find(|(k, _)| k == key).map_or("", |(_, v)| v);
                fields.insert(key.to_owned(), Value::String(value.to_owned()));
            }
            fields.insert("traceback".to_owned(), lines(&text(blocks)));
        }
        _ => {
            if kind == "execute_result" {
                let count = attr
                    .attributes
                    .iter()
                    .find(|(k, _)| k == "execution_count")
                    .map_or(Value::Null, |(_, v)| scalar(v));
                fields.insert("execution_count".to_owned(), count);
            }
            let (data, metadata) = data(blocks, media);
            fields.insert("metadata".to_owned(), metadata);
            fields.insert("data".to_owned(), data);
        }
    }
    Value::Object(fields)
}

/// The mime bundle for one output, and the output metadata beside it.
///
/// An image's `Attr` becomes that metadata **unkeyed** — `{"width": 100}`
/// rather than `{"image/png": {"width": 100}}` — which is what pandoc's
/// writer emits even though its own reader looks for the keyed form. The
/// gate compares the two readbacks, so matching the writer is what counts.
fn data(blocks: &[Block], media: &dyn Fn(&str) -> Option<Vec<u8>>) -> (Value, Value) {
    let mut bundle = Map::new();
    let mut metadata = Map::new();
    for block in blocks {
        match block {
            Block::Para(inlines) | Block::Plain(inlines) => {
                for inline in inlines {
                    if let Inline::Image(attr, _, target) = inline
                        && let Some(bytes) = media(&target.url)
                    {
                        bundle.insert(
                            image_mime(&target.url).to_owned(),
                            Value::String(to_base64(&bytes)),
                        );
                        for (key, value) in &attr.attributes {
                            metadata.insert(key.clone(), scalar(value));
                        }
                    }
                }
            }
            Block::CodeBlock(attr, text) if attr.classes.iter().any(|c| c == "json") => {
                let value = serde_json::from_str(text).unwrap_or_else(|_| Value::String(text.clone()));
                bundle.insert("application/json".to_owned(), value);
            }
            Block::CodeBlock(_, text) => {
                bundle.insert("text/plain".to_owned(), lines(text));
            }
            Block::RawBlock(format, text) => {
                let mime = match format.0.as_str() {
                    "html" => "text/html",
                    "latex" => "text/latex",
                    _ => "text/markdown",
                };
                bundle.insert(mime.to_owned(), lines(text));
            }
            _ => {}
        }
    }
    (Value::Object(bundle), Value::Object(metadata))
}

/// A `RawBlock` format name back to the mime type a raw cell names it by.
fn raw_mimetype(format: &str) -> String {
    match format {
        "html" => "text/html",
        "latex" => "text/latex",
        "markdown" => "text/markdown",
        "rst" => "text/restructuredtext",
        "asciidoc" => "text/asciidoc",
        other => other,
    }
    .to_owned()
}

/// Move every image a markdown cell names, and whose bytes the resolver
/// can supply, into the cell's `attachments` — rewriting its URL to
/// `attachment:<url>`, which is the only reference a notebook can resolve
/// on its own. Pandoc keys the attachment by the whole URL, not by its
/// basename, so a picture that arrived as `<cell>-logo.png` leaves as
/// `attachment:<cell>-logo.png`.
fn detach_images(blocks: &mut [Block], media: &dyn Fn(&str) -> Option<Vec<u8>>) -> Map<String, Value> {
    let mut attachments = Map::new();
    // Iterative, so no input can drive this deeper than the markdown
    // reader already allowed.
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
            Block::BulletList(items) | Block::OrderedList(_, items) => {
                stack.extend(items.iter_mut().flatten());
            }
            _ => {}
        }
    }
    while let Some(inline) = inlines.pop() {
        match inline {
            Inline::Image(_, alt, target) => {
                if let Some(bytes) = media(&target.url) {
                    attachments.insert(
                        target.url.clone(),
                        serde_json::json!({ image_mime(&target.url): to_base64(&bytes) }),
                    );
                    target.url = format!("attachment:{}", target.url);
                }
                inlines.extend(alt.iter_mut());
            }
            Inline::Emph(items)
            | Inline::Strong(items)
            | Inline::Strikeout(items)
            | Inline::Underline(items)
            | Inline::Span(_, items)
            | Inline::Link(_, items, _) => inlines.extend(items.iter_mut()),
            _ => {}
        }
    }
    attachments
}

fn image_mime(url: &str) -> &'static str {
    match url.rsplit('.').next().unwrap_or_default() {
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "svg" => "image/svg+xml",
        "pdf" => "application/pdf",
        _ => "image/png",
    }
}

/// Text as nbformat spells it: a list of lines each keeping its newline.
fn lines(text: &str) -> Value {
    let mut out = Vec::new();
    let mut rest = text;
    while let Some(end) = rest.find('\n') {
        out.push(Value::String(rest[..=end].to_owned()));
        rest = &rest[end + 1..];
    }
    if !rest.is_empty() {
        out.push(Value::String(rest.to_owned()));
    }
    Value::Array(out)
}

/// An attribute value back to the JSON it was read from, when it spells
/// one: `true` becomes a boolean and `["a"]` a list, and anything that is
/// not JSON stays the string it is.
fn scalar(text: &str) -> Value {
    serde_json::from_str(text).unwrap_or_else(|_| Value::String(text.to_owned()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn written(doc: &Pandoc) -> Value {
        serde_json::from_slice(&write_ipynb(doc).expect("writes")).expect("valid JSON")
    }

    #[test]
    fn every_cell_carries_an_id_and_the_notebook_is_four_five() {
        let doc = Pandoc {
            blocks: vec![Block::Para(vec![Inline::Str("hi".to_owned())])],
            ..Pandoc::default()
        };
        let nb = written(&doc);
        assert_eq!(nb["nbformat"], 4);
        assert_eq!(nb["nbformat_minor"], 5);
        assert_eq!(nb["cells"][0]["cell_type"], "markdown");
        assert_eq!(nb["cells"][0]["source"], json!(["hi"]));
        assert!(nb["cells"][0]["id"].as_str().is_some_and(|id| id.len() == 36));
    }

    #[test]
    fn the_same_ast_writes_the_same_bytes_twice() {
        let doc = Pandoc {
            blocks: vec![Block::Para(vec![Inline::Str("hi".to_owned())])],
            ..Pandoc::default()
        };
        assert_eq!(write_ipynb(&doc).unwrap(), write_ipynb(&doc).unwrap());
    }

    #[test]
    fn a_cell_identifier_is_kept_rather_than_derived() {
        let doc = Pandoc {
            blocks: vec![Block::Div(
                Attr {
                    identifier: "a1b2c3d4".to_owned(),
                    classes: vec!["cell".to_owned(), "code".to_owned()],
                    attributes: vec![("execution_count".to_owned(), "2".to_owned())],
                },
                vec![Block::CodeBlock(Attr::default(), "x = 1".to_owned())],
            )],
            ..Pandoc::default()
        };
        let nb = written(&doc);
        assert_eq!(nb["cells"][0]["id"], "a1b2c3d4");
        assert_eq!(nb["cells"][0]["execution_count"], 2);
        assert_eq!(nb["cells"][0]["outputs"], json!([]));
    }

    #[test]
    fn lines_keep_their_newlines_and_a_trailing_one_adds_no_empty_line() {
        assert_eq!(lines("a\nb\n"), json!(["a\n", "b\n"]));
        assert_eq!(lines("a\nb"), json!(["a\n", "b"]));
        assert_eq!(lines(""), json!([]));
    }
}
