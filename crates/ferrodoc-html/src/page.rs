//! A standalone HTML page, through pandoc's own template.
//!
//! The fragment writer has matched pandoc for a long time; the *page*
//! around it did not, and it is what a reader sees first. 174 of the 176
//! lines by which `-s` output differed were pandoc's default template and
//! its default stylesheet, so those two files are vendored in
//! `templates/` under the BSD-3 option pandoc's `COPYRIGHT` offers for
//! `data/templates`, and this module fills them in.
//!
//! Every variable below was derived by running the pinned binary and
//! comparing bytes, not from the manual.

use crate::template::{Context, Value, render};
use crate::{escape_text, meta_text, meta_texts, toc_list_to_depth, write_html};
use ferrodoc_ast::Pandoc;

/// Pandoc's default HTML template, verbatim. See `templates/LICENSE`.
const DEFAULT_TEMPLATE: &str = include_str!("../templates/html5.html");
/// Pandoc's default stylesheet, verbatim. See `templates/LICENSE`.
const DEFAULT_STYLES: &str = include_str!("../templates/styles.html");

/// What goes into the page besides the document.
#[derive(Debug, Default)]
pub struct Page<'a> {
    /// Stylesheet URLs, one `<link>` each — **not** inlined. Pandoc's
    /// `--css` takes a URL and links it; inlining the file was this
    /// project's own invention and made every `-s -c` command line
    /// differ.
    pub css: Vec<String>,
    /// A table of contents before the body.
    pub toc: bool,
    /// How deep it goes. Pandoc's default is 3.
    pub toc_depth: i64,
    /// `--include-in-header`, verbatim into `<head>`.
    pub header_includes: Vec<String>,
    /// `--include-before-body`, verbatim after `<body>`.
    pub include_before: Vec<String>,
    /// `--include-after-body`, verbatim before `</body>`.
    pub include_after: Vec<String>,
    /// `-V key=value`, which wins over anything derived from the
    /// document — that is pandoc's precedence.
    pub variables: Vec<(String, String)>,
    /// A `--template` to use instead of pandoc's default.
    pub template: Option<&'a str>,
    /// `--id-prefix`, which the template also puts on the contents'
    /// own `<nav id>`.
    pub id_prefix: String,
    /// What `<title>` says when the document has no title. Pandoc uses
    /// the **input file's name**, which only the caller knows.
    pub pagetitle: Option<&'a str>,
}

impl Page<'_> {
    /// Pandoc's defaults: no CSS, no contents, contents three deep.
    pub fn new() -> Self {
        Page { toc_depth: 3, ..Page::default() }
    }
}

/// Render `doc` as a complete page.
///
/// # Errors
///
/// A `--template` using a construct outside the supported subset, named
/// in the message rather than left as a hole in the page.
pub fn write_page(doc: &Pandoc, page: &Page<'_>) -> Result<String, String> {
    let mut context = Context::new();
    let mut set = |name: &str, value: Value| {
        context.insert(name.to_owned(), value);
    };

    // `lang` is always *set* and often empty: the template writes
    // `lang=""` for a document with no language, and `$if(dir)$` is false
    // for the same emptiness. That is why `Value::Text("")` is not the
    // same as absent.
    set("lang", Value::Text(meta_text(doc, "lang").unwrap_or_default()));
    if let Some(dir) = meta_text(doc, "dir") {
        set("dir", Value::Text(dir));
    }

    let authors = meta_texts(doc, "author");
    if !authors.is_empty() {
        set("author-meta", Value::List(authors.clone()));
        set("author", Value::List(authors.iter().map(|a| escaped(a)).collect()));
    }
    if let Some(date) = meta_text(doc, "date") {
        set("date-meta", Value::Text(date.clone()));
        set("date", Value::Text(escaped(&date)));
    }
    let keywords = meta_texts(doc, "keywords");
    if !keywords.is_empty() {
        set("keywords", Value::List(keywords));
    }
    if let Some(description) = meta_text(doc, "description") {
        set("description-meta", Value::Text(description));
    }
    if let Some(subtitle) = meta_text(doc, "subtitle") {
        set("subtitle", Value::Text(escaped(&subtitle)));
    }

    // `title` is the rendered heading and `pagetitle` is what `<title>`
    // says. They differ: a document with no title still needs a
    // `<title>`, and pandoc puts the **input file's name** there.
    let title = meta_text(doc, "title");
    if let Some(title) = &title {
        set("title", Value::Text(escaped(title)));
    }
    let pagetitle = title.unwrap_or_else(|| page.pagetitle.unwrap_or_default().to_owned());
    set("pagetitle", Value::Text(escaped(&pagetitle)));

    if !page.id_prefix.is_empty() {
        set("idprefix", Value::Text(page.id_prefix.clone()));
    }
    if !page.css.is_empty() {
        set("css", Value::List(page.css.clone()));
    }
    if !page.header_includes.is_empty() {
        set("header-includes", Value::List(page.header_includes.clone()));
    }
    if !page.include_before.is_empty() {
        set("include-before", Value::List(page.include_before.clone()));
    }
    if !page.include_after.is_empty() {
        set("include-after", Value::List(page.include_after.clone()));
    }

    // `--toc` on a document with no heading writes **nothing** — not an
    // empty `<nav>`. Pandoc leaves the variable unset rather than setting
    // it to an empty list, so the template's `$if(toc)$` is false.
    if page.toc {
        let contents = trimmed(&toc_list_to_depth(doc, page.toc_depth, &page.id_prefix));
        if !contents.is_empty() {
            // **Both names, and both holding the contents.** Pandoc's own
            // template tests `$if(toc)$` and interpolates
            // `$table-of-contents$`, so a flag would have done — but
            // templates in the wild write `$toc$` for the contents
            // themselves, and one of them is in this repository's own
            // corpus. A flag there renders an empty `<nav>`.
            set("toc", Value::Text(contents.clone()));
            set("table-of-contents", Value::Text(contents));
        }
    }

    // Only with the default template: pandoc sets `document-css` when it
    // is using its own, and a custom template that wants the stylesheet
    // asks for it by name.
    //
    // `displaymath-css` rides with it, and **unconditionally** — pandoc
    // emits that rule for a document with no mathematics at all, and
    // drops it only when a math *method* like `--mathml` takes over the
    // rendering. Checked on three documents without any before it was
    // believed.
    //
    // **`--css` turns the default stylesheet off**, which is the rule
    // that took 145 lines out of every `-s -c` comparison: a caller who
    // brought their own stylesheet did not ask for pandoc's on top of it.
    // Only the comment at the head of `styles.html` survives, because it
    // sits outside the `$if(document-css)$`.
    if page.template.is_none() {
        if page.css.is_empty() {
            set("document-css", Value::Flag);
        }
        set("displaymath-css", Value::Flag);
    }

    set("body", Value::Text(trimmed(&write_html(doc))));

    // `-V` last, because it wins. `pandoc -V lang=fr` overrides a
    // document that says otherwise, and a caller who passes one has said
    // something more specific than the document did.
    for (name, value) in &page.variables {
        context.insert(name.clone(), Value::Text(value.clone()));
    }

    let template = page.template.unwrap_or(DEFAULT_TEMPLATE);
    let partials = |name: &str| match name {
        "styles.html" => Some(DEFAULT_STYLES.to_owned()),
        _ => None,
    };
    render(template, &context, &partials)
}

/// A fragment as a template value: no trailing newline, because the
/// template supplies the one that ends its line.
fn trimmed(html: &str) -> String {
    html.trim_end_matches('\n').to_owned()
}

fn escaped(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    escape_text(&mut out, text);
    out
}
