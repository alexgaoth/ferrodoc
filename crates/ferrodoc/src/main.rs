//! The `ferrodoc` command-line converter.

use ferrodoc::Format;
use ferrodoc::ast::{Block, Inline};
use std::fmt::Write as _;
use std::io::{Read as _, Write as _};
use std::path::PathBuf;
use std::process::ExitCode;

const USAGE_HEAD: &str = "\
ferrodoc — convert documents between markdown, HTML, DOCX and the pandoc AST

USAGE:
    ferrodoc [OPTIONS] [INPUT]

    With no INPUT, or with `-`, the document is read from standard input.
    With no --output, the result is written to standard output.

OPTIONS:
    -f, --from <FORMAT>     Input format   [inferred from INPUT's extension]
    -t, --to <FORMAT>       Output format  [inferred from --output's extension]
    -o, --output <FILE>     Write to FILE instead of standard output
    -s, --standalone        Wrap HTML in a page, or LaTeX in a whole document
        --css <FILE>        Inline a stylesheet into that page
        --wrap <MODE>       preserve (default) | none | auto. `auto` fills
                            text output to --columns, which is what pandoc
                            does by default; ferrodoc leaves lines where
                            they fall unless asked. Only the markdown
                            writers fill.
        --columns <N>       Fill width for --wrap=auto [72]
        --extract-media <DIR>
                            Write the input's embedded images under DIR and
                            point the output at them. Without it a
                            `docx -> markdown` conversion names pictures it
                            never writes, so they cannot be recovered.
    -h, --help              Print this help
    -V, --version           Print the version

FORMATS:
";

/// Every readable format's help spelling, in help order.
///
/// The list is data rather than prose because a build can be trimmed with
/// cargo features — `--no-default-features --features markdown,html` links
/// two format crates instead of eleven — and a help text that named formats
/// the binary cannot convert would be lying about itself. With the default
/// features every entry is compiled and the two lines below come out
/// exactly as they always did.
const HELP_READABLE: &[(Format, &str)] = &[
    (Format::Markdown, "markdown (commonmark, md)"),
    (Format::Gfm, "gfm"),
    (Format::Html, "html"),
    (Format::Docx, "docx"),
    (Format::Odt, "odt"),
    (Format::Epub, "epub"),
    (Format::Ipynb, "ipynb"),
    (Format::Json, "json"),
];

/// The write-only formats, in help order.
const HELP_WRITE_ONLY: &[(Format, &str)] = &[
    (Format::Latex, "latex (tex)"),
    (Format::Rst, "rst"),
    (Format::Asciidoc, "asciidoc (adoc)"),
    (Format::Plain, "plain (text)"),
];

// The `\<newline>` continuation used above would strip this line's indent,
// so the first line sits on the assignment.
const USAGE_TAIL: &str = "    `markdown` here is **CommonMark**, which is not what `pandoc -f markdown`
    means. Pandoc's own dialect adds YAML metadata blocks, header attributes
    (`# H {#id .class}`), definition lists and superscript/subscript, and
    none of those are read: they come through as the literal text they are
    written with. Footnotes are read by `gfm` and not by `markdown`, which
    is also how pandoc has it.

    `gfm` is GitHub Flavored Markdown: tables, task lists, strikethrough
    and bare-URL links. Prefer it over `markdown` for anything with a
    table — CommonMark has no table syntax, so a table degrades there to
    one paragraph per cell.

EXAMPLES:
    ferrodoc README.md -o readme.html
    ferrodoc README.md -s -o readme.html    # a page a browser can open
    ferrodoc notes.md -s --css site.css -o notes.html
    ferrodoc report.docx -t gfm             # DOCX in, GitHub markdown out
    ferrodoc report.docx -t markdown        # DOCX in, CommonMark out
    ferrodoc page.html -t markdown          # HTML in, markdown out
    ferrodoc report.docx -t plain
    ferrodoc report.docx -t gfm --extract-media out  # and keep the pictures
    ferrodoc minutes.odt -t gfm             # LibreOffice in, markdown out
    ferrodoc book.epub -t gfm               # an e-book in, markdown out
    ferrodoc manual.md -o manual.epub       # markdown in, an e-book out
    ferrodoc report.docx -t latex | pdflatex # DOCX in, PDF out, via TeX
    ferrodoc analysis.ipynb -t docx        # a notebook in, Word out
    ferrodoc README.md -o readme.odt        # markdown in, LibreOffice out
    cat notes.md | ferrodoc -f markdown -t docx -o notes.docx
";

/// The `FORMATS:` block, listing what this build actually has code for.
fn formats_block() -> String {
    let listed = |table: &[(Format, &'static str)]| -> Vec<&'static str> {
        table
            .iter()
            .filter(|(format, _)| format.compiled())
            .map(|(_, spelling)| *spelling)
            .collect()
    };
    let write_only = listed(HELP_WRITE_ONLY);
    // "a, b and c" — an Oxford-less list is what the help has always read.
    let joined = match write_only.split_last() {
        None => String::new(),
        Some((last, [])) => (*last).to_owned(),
        Some((last, rest)) => format!("{} and {last}", rest.join(", ")),
    };
    let mut block = format!("    input:   {}\n", listed(HELP_READABLE).join(", "));
    if joined.is_empty() {
        block.push_str("    output:  those\n");
    } else {
        let _ = writeln!(block, "    output:  those, plus {joined}");
    }
    block
}

/// The whole `--help` text.
fn usage() -> String {
    format!("{USAGE_HEAD}{}\n{USAGE_TAIL}", formats_block())
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("ferrodoc: {message}");
            ExitCode::FAILURE
        }
    }
}

/// Everything the command line asked for.
struct Options {
    from: Option<Format>,
    to: Option<Format>,
    output: Option<PathBuf>,
    input: Option<PathBuf>,
    standalone: bool,
    css: Option<PathBuf>,
    extract_media: Option<PathBuf>,
    /// The width to fill to, or `None` to leave lines where they fall.
    wrap_columns: Option<usize>,
}

/// Parse the command line. `Ok(None)` means `--help` or `--version`
/// already printed what was asked for and there is nothing left to do.
fn parse_args(args: &[String]) -> Result<Option<Options>, String> {
    let mut from: Option<Format> = None;
    let mut to: Option<Format> = None;
    let mut output: Option<PathBuf> = None;
    let mut input: Option<PathBuf> = None;
    let mut stdin_requested = false;
    let mut standalone = false;
    let mut css: Option<PathBuf> = None;
    let mut extract_media: Option<PathBuf> = None;
    // `preserve` is the default and `none` is the same thing for this
    // writer, which never inserted a break of its own: both leave every
    // line where the document put it. Only `auto` fills.
    let mut wrap_columns: Option<usize> = None;
    let mut columns = 72usize;

    let mut i = 0;
    while i < args.len() {
        // `--opt=value` as well as `--opt value`, because that is how
        // pandoc is written in everybody's existing Makefile.
        let (arg, attached) = match args[i].split_once('=') {
            Some((name, value)) if name.starts_with("--") => (name, Some(value.to_owned())),
            _ => (args[i].as_str(), None),
        };
        let mut value = |name: &str| -> Result<String, String> {
            if let Some(attached) = attached.clone() {
                return Ok(attached);
            }
            i += 1;
            args.get(i)
                .cloned()
                .ok_or_else(|| format!("{name} needs a value"))
        };
        match arg {
            "-h" | "--help" => {
                print!("{}", usage());
                return Ok(None);
            }
            "-V" | "--version" => {
                println!("ferrodoc {}", env!("CARGO_PKG_VERSION"));
                return Ok(None);
            }
            "-f" | "--from" => {
                let name = value("--from")?;
                from = Some(format(&name)?);
            }
            "-t" | "--to" => {
                let name = value("--to")?;
                to = Some(format(&name)?);
            }
            "-o" | "--output" => output = Some(PathBuf::from(value("--output")?)),
            "-s" | "--standalone" => standalone = true,
            "--css" => css = Some(PathBuf::from(value("--css")?)),
            "--extract-media" => {
                extract_media = Some(PathBuf::from(value("--extract-media")?));
            }
            "--wrap" => {
                let mode = value("--wrap")?;
                match mode.as_str() {
                    "auto" => wrap_columns = Some(0), // resolved after --columns
                    "none" | "preserve" => wrap_columns = None,
                    other => {
                        return Err(format!(
                            "unknown --wrap {other:?}; expected auto, none or preserve"
                        ));
                    }
                }
            }
            "--columns" => {
                let raw = value("--columns")?;
                columns = raw
                    .parse::<usize>()
                    .ok()
                    .filter(|n| *n > 0)
                    .ok_or_else(|| format!("--columns needs a positive number, not {raw:?}"))?;
            }
            // An explicit "-" means stdin, and cannot be combined with a
            // named file — silently ignoring one of them would convert the
            // wrong document.
            "-" => {
                if input.is_some() {
                    return Err("more than one input given".to_owned());
                }
                stdin_requested = true;
            }
            other if other.starts_with('-') && other.len() > 1 => {
                return Err(format!("unknown option {other} (try --help)"));
            }
            other => {
                if input.is_some() || stdin_requested {
                    return Err("more than one input given".to_owned());
                }
                input = Some(PathBuf::from(other));
            }
        }
        i += 1;
    }

    // Formats not given explicitly come from the file extensions; that
    // resolution needs the input path, so it happens in `run`.
    //
    // `--columns` may appear either side of `--wrap`, so the width is
    // read once both are known rather than as `--wrap` is parsed.
    Ok(Some(Options {
        from,
        to,
        output,
        input,
        standalone,
        css,
        extract_media,
        wrap_columns: wrap_columns.map(|_| columns),
    }))
}

fn run() -> Result<(), String> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(Options {
        from,
        to,
        output,
        input,
        standalone,
        css,
        extract_media,
        wrap_columns,
    }) = parse_args(&args)?
    else {
        return Ok(());
    };

    // Formats not given explicitly come from the file extensions.
    let Some(from) = from.or_else(|| input.as_deref().and_then(Format::from_path)) else {
        return Err(
            "cannot tell the input format: pass --from, or name a file with a known extension"
                .to_owned(),
        );
    };
    let Some(to) = to.or_else(|| output.as_deref().and_then(Format::from_path)) else {
        return Err(
            "cannot tell the output format: pass --to, or name an output file with a known extension"
                .to_owned(),
        );
    };

    let bytes = read_input(input.as_deref())?;
    if matches!(from, Format::Markdown | Format::Gfm) && opens_with_metadata_block(&bytes) {
        eprintln!(
            "ferrodoc: this document opens with what pandoc would read as a YAML \
metadata block; ferrodoc reads CommonMark, where it is a thematic break and \
a heading in the body"
        );
    }

    // Image paths in a document are relative to the document, the way
    // every editor that wrote one meant them.
    let base = input
        .as_deref()
        .and_then(std::path::Path::parent)
        .unwrap_or_else(|| std::path::Path::new("."))
        .to_owned();
    // Only when the output can hold them, or when asked for them: a
    // document's images can be far larger than its text, and reading them
    // to throw them away is how a `docx -> markdown` conversion runs a
    // machine out of memory.
    let (mut doc, embedded) = if to.embeds_media() || extract_media.is_some() {
        ferrodoc::parse_with_media(&bytes, from).map_err(|e| e.to_string())?
    } else {
        (ferrodoc::parse(&bytes, from).map_err(|e| e.to_string())?, ferrodoc::Media::new())
    };
    if let Some(dir) = extract_media.as_deref() {
        extract(&mut doc, &embedded, dir)?;
    }
    let converted = if standalone {
        render_page(&doc, to, css.as_deref())?
    } else {
        if css.is_some() {
            return Err("--css needs --standalone: a fragment has no <head>".to_owned());
        }
        match wrap_columns {
            Some(columns) => {
                ferrodoc::render_wrapped(&doc, to, columns).map_err(|e| e.to_string())?
            }
            None => ferrodoc::render_with_media(&doc, to, &resolve(&embedded, &base))
                .map_err(|e| e.to_string())?,
        }
    };

    write_output(output.as_deref(), &converted)
}

/// The document's bytes, from a named file or standard input.
fn read_input(input: Option<&std::path::Path>) -> Result<Vec<u8>, String> {
    let Some(path) = input else {
        let mut bytes = Vec::new();
        std::io::stdin()
            .read_to_end(&mut bytes)
            .map_err(|e| format!("cannot read standard input: {e}"))?;
        return Ok(bytes);
    };
    std::fs::read(path).map_err(|e| format!("cannot read {}: {e}", path.display()))
}

fn write_output(output: Option<&std::path::Path>, bytes: &[u8]) -> Result<(), String> {
    let Some(path) = output else {
        return std::io::stdout()
            .write_all(bytes)
            .map_err(|e| format!("cannot write to standard output: {e}"));
    };
    std::fs::write(path, bytes).map_err(|e| format!("cannot write {}: {e}", path.display()))
}

/// A complete document rather than a fragment: an HTML page with `css`
/// inlined, or a LaTeX file with a preamble and `\begin{document}`.
///
/// Only those two have a fragment/whole distinction; saying so beats
/// writing the fragment the flag was meant to prevent.
fn render_page(
    doc: &ferrodoc::Pandoc,
    to: Format,
    css: Option<&std::path::Path>,
) -> Result<Vec<u8>, String> {
    if to == Format::Latex {
        if css.is_some() {
            return Err("--css applies to html output, not latex".to_owned());
        }
        // Without this, `-s` on LaTeX would hand pdflatex a fragment with
        // no preamble — which is the exact mistake the flag prevents for
        // HTML.
        #[cfg(feature = "latex")]
        return Ok(ferrodoc::render_latex_standalone(doc).into_bytes());
        // Unreachable in any build that has the format at all: `format()`
        // refuses the name before a document is read.
        #[cfg(not(feature = "latex"))]
        return Err(format!("{to} support was not compiled into this build"));
    }
    if to != Format::Html {
        return Err(format!("--standalone applies to html or latex output, not {to}"));
    }
    let css = css
        .map(|path| {
            std::fs::read_to_string(path)
                .map_err(|e| format!("cannot read {}: {e}", path.display()))
        })
        .transpose()?;
    #[cfg(feature = "html")]
    return Ok(ferrodoc::render_html_standalone(doc, css.as_deref()));
    #[cfg(not(feature = "html"))]
    return Err(format!("{to} support was not compiled into this build"));
}

/// Whether the document opens with what pandoc would read as a YAML
/// metadata block.
///
/// This is the one place the dialect difference is not merely narrower
/// but **wrong**: pandoc lifts the block into metadata, and `CommonMark`
/// reads it as a thematic break followed by a setext heading, so the
/// title and author appear in the body. Nothing else here is worth a
/// warning — an unread `[^1]` or `{#id}` shows up as itself.
///
/// The rule is pandoc's, probed against 3.8.2.1: the first line is exactly
/// `---`, the line after it is **not** blank (`---\n\ntext\n\n---` is two
/// thematic breaks to pandoc as well), and a later line is exactly `---`
/// or `...`.
fn opens_with_metadata_block(bytes: &[u8]) -> bool {
    let Ok(text) = std::str::from_utf8(bytes) else { return false };
    let mut lines = text.lines();
    if lines.next().map(str::trim_end) != Some("---") {
        return false;
    }
    let Some(second) = lines.next() else { return false };
    if second.trim().is_empty() {
        return false;
    }
    std::iter::once(second)
        .chain(lines)
        .any(|line| matches!(line.trim_end(), "---" | "..."))
}

/// Write every embedded image under `dir` and repoint the document at it.
///
/// Pandoc's `--extract-media` is the behaviour being matched, probed
/// against 3.8.2.1: a picture the AST calls `media/rId10.png` is written
/// to `<dir>/media/rId10.png`, and the reference becomes that same joined
/// path — so a relative `--extract-media out` yields `out/media/rId10.png`
/// and an absolute one an absolute reference. Without this the reference
/// names a file nothing ever writes, and the picture is unrecoverable
/// from the command line although the library has held it all along.
///
/// # Errors
///
/// If a key escapes `dir`, or the bytes cannot be written.
fn extract(
    doc: &mut ferrodoc::Pandoc,
    embedded: &ferrodoc::Media,
    dir: &std::path::Path,
) -> Result<(), String> {
    let mut written = std::collections::HashMap::new();
    for (url, bytes) in embedded {
        // The key comes out of somebody's zip, so it is untrusted input.
        // A component that walks upward would place a file anywhere the
        // process can write; refusing beats sanitizing, which invites a
        // second guess about what the sanitized name now collides with.
        let relative = std::path::Path::new(url);
        if relative.is_absolute()
            || relative
                .components()
                .any(|c| !matches!(c, std::path::Component::Normal(_)))
        {
            return Err(format!("refusing to extract {url:?}: it escapes {}", dir.display()));
        }
        let path = dir.join(relative);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("cannot create {}: {e}", parent.display()))?;
        }
        std::fs::write(&path, bytes).map_err(|e| format!("cannot write {}: {e}", path.display()))?;
        written.insert(url.clone(), path.to_string_lossy().into_owned());
    }
    repoint(&mut doc.blocks, &written);
    Ok(())
}

/// Rewrite every image target that names an extracted file.
///
/// Recursive, unlike the readers: the tree here has already been through
/// one, so its depth is bounded by whichever bound accepted it — 200 for
/// this project's readers, `serde_json`'s own limit for `-f json`. The
/// alternative, a hand-rolled worklist over two node kinds, is what makes
/// a walk miss a container, and a missed container is a picture silently
/// left pointing at a file that is not there.
fn repoint(blocks: &mut [Block], written: &std::collections::HashMap<String, String>) {
    for block in blocks {
        match block {
            Block::Plain(inlines) | Block::Para(inlines) | Block::Header(_, _, inlines) => {
                repoint_inlines(inlines, written);
            }
            Block::LineBlock(lines) => {
                for line in lines {
                    repoint_inlines(line, written);
                }
            }
            Block::BlockQuote(inner) | Block::Div(_, inner) => repoint(inner, written),
            Block::BulletList(items) | Block::OrderedList(_, items) => {
                for item in items {
                    repoint(item, written);
                }
            }
            Block::DefinitionList(entries) => {
                for (term, definitions) in entries {
                    repoint_inlines(term, written);
                    for definition in definitions {
                        repoint(definition, written);
                    }
                }
            }
            Block::Figure(_, caption, inner) => {
                repoint_caption(caption, written);
                repoint(inner, written);
            }
            Block::Table(table) => {
                repoint_caption(&mut table.caption, written);
                for row in table
                    .head
                    .rows
                    .iter_mut()
                    .chain(table.bodies.iter_mut().flat_map(|b| {
                        b.head.iter_mut().chain(b.body.iter_mut())
                    }))
                    .chain(table.foot.rows.iter_mut())
                {
                    for cell in &mut row.cells {
                        repoint(&mut cell.blocks, written);
                    }
                }
            }
            Block::CodeBlock(..) | Block::RawBlock(..) | Block::HorizontalRule => {}
        }
    }
}

fn repoint_caption(
    caption: &mut ferrodoc::ast::Caption,
    written: &std::collections::HashMap<String, String>,
) {
    if let Some(short) = caption.short.as_mut() {
        repoint_inlines(short, written);
    }
    repoint(&mut caption.blocks, written);
}

fn repoint_inlines(inlines: &mut [Inline], written: &std::collections::HashMap<String, String>) {
    for inline in inlines {
        match inline {
            Inline::Image(_, alt, target) => {
                if let Some(path) = written.get(&target.url) {
                    target.url.clone_from(path);
                }
                repoint_inlines(alt, written);
            }
            Inline::Emph(inner)
            | Inline::Underline(inner)
            | Inline::Strong(inner)
            | Inline::Strikeout(inner)
            | Inline::Superscript(inner)
            | Inline::Subscript(inner)
            | Inline::SmallCaps(inner)
            | Inline::Quoted(_, inner)
            | Inline::Link(_, inner, _)
            | Inline::Span(_, inner)
            | Inline::Cite(_, inner) => repoint_inlines(inner, written),
            // A picture inside a footnote is not hypothetical here: one
            // was once replaced by the body's, and `corpus/docx` still
            // carries the document that found it.
            Inline::Note(blocks) => repoint(blocks, written),
            Inline::Str(_)
            | Inline::Code(..)
            | Inline::Math(..)
            | Inline::RawInline(..)
            | Inline::Space
            | Inline::SoftBreak
            | Inline::LineBreak => {}
        }
    }
}

/// Where an image's bytes come from: the input package first, then a file
/// of that name beside the document.
///
/// That order matters. A `.docx` names its pictures by part path, so a
/// file called `media/image1.png` sitting next to the document is a
/// different picture entirely — preferring it would silently swap one
/// image for another.
fn resolve<'a>(
    embedded: &'a ferrodoc::Media,
    base: &'a std::path::Path,
) -> impl Fn(&str) -> Option<Vec<u8>> + 'a {
    move |url| {
        embedded
            .get(url)
            .cloned()
            .or_else(|| std::fs::read(base.join(url)).ok())
    }
}

fn format(name: &str) -> Result<Format, String> {
    let known = || -> String {
        Format::NAMES
            .iter()
            .copied()
            .filter(|name| Format::parse(name).is_some_and(Format::compiled))
            .collect::<Vec<_>>()
            .join(", ")
    };
    let format = Format::parse(name)
        .ok_or_else(|| format!("unknown format {name:?}; known formats: {}", known()))?;
    // Only a build trimmed with cargo features can reach this: the name is
    // real, the code for it was not compiled in. Saying so beats "unknown
    // format", which would send someone looking for a typo.
    if !format.compiled() {
        return Err(format!(
            "{format} support was not compiled into this build; known formats: {}",
            known()
        ));
    }
    Ok(format)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The help text is what a trimmed build tells the truth with, and it
    /// is generated rather than written for exactly that reason. Enabling
    /// a format without listing it — or listing one this build cannot
    /// convert — fails here.
    #[test]
    fn help_lists_exactly_the_formats_this_build_has() {
        let help = usage();
        let block = help
            .split("FORMATS:\n")
            .nth(1)
            .expect("a FORMATS block")
            .split("\n\n")
            .next()
            .expect("the list ends at a blank line");
        for (format, spelling) in HELP_READABLE.iter().chain(HELP_WRITE_ONLY) {
            assert_eq!(
                block.contains(spelling),
                format.compiled(),
                "{format}: compiled = {}, listed in --help = {}\n{block}",
                format.compiled(),
                block.contains(spelling),
            );
        }
    }

    /// Probed against pandoc 3.8.2.1: every `true` here is a document
    /// whose `meta` pandoc fills and ferrodoc leaves in the body, and
    /// every `false` one pandoc also reads as thematic breaks.
    #[test]
    fn a_metadata_block_is_recognised_and_its_near_misses_are_not() {
        let opens = |s: &str| opens_with_metadata_block(s.as_bytes());
        assert!(opens("---\ntitle: A\n---\n\nBody.\n"));
        assert!(opens("---\ntitle: A\n...\n\nBody.\n"), "`...` closes one too");
        assert!(opens("---\r\ntitle: A\r\n---\r\n"), "CRLF");

        // A blank line after the opener makes it a thematic break for
        // pandoc as well, which is the false positive worth avoiding.
        assert!(!opens("---\n\nSome text\n\n---\n"));
        assert!(!opens("---\ntitle: A\n\nBody.\n"), "no closing fence");
        assert!(!opens("***\n\nBody.\n\n---\n"), "a different break character");
        assert!(!opens("Body.\n\n---\ntitle: A\n---\n"), "not at the start");
        assert!(!opens(""));
        assert!(!opens("---\n"));
    }

    /// A key out of somebody's zip is untrusted: a component that walks
    /// upward would place a file anywhere the process can write.
    #[test]
    fn extraction_refuses_a_key_that_escapes_the_directory() {
        let dir = std::env::temp_dir().join("ferrodoc-extract-escape");
        std::fs::create_dir_all(&dir).expect("a writable temp dir");
        for hostile in ["../escaped.png", "a/../../escaped.png", "/etc/escaped.png"] {
            let mut media = ferrodoc::Media::new();
            media.insert(hostile.to_owned(), b"bytes".to_vec());
            let mut doc = ferrodoc::Pandoc::new(Vec::new());
            let err = extract(&mut doc, &media, &dir).expect_err(hostile);
            assert!(err.contains("escapes"), "{err}");
        }
        assert!(!dir.join("../escaped.png").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Every container, because a walk that misses one leaves a picture
    /// pointing at a file that is not there — and nothing fails loudly.
    #[test]
    fn extraction_repoints_a_picture_wherever_it_sits() {
        let dir = std::env::temp_dir().join("ferrodoc-extract-walk");
        std::fs::create_dir_all(&dir).expect("a writable temp dir");
        let mut media = ferrodoc::Media::new();
        media.insert("media/p.png".to_owned(), b"bytes".to_vec());

        let image = || {
            Inline::Image(
                Box::default(),
                Vec::new(),
                Box::new(ferrodoc::ast::Target { url: "media/p.png".to_owned(), title: String::new() }),
            )
        };
        let para = || Block::Para(vec![image()]);
        let mut doc = ferrodoc::Pandoc::new(vec![
            para(),
            Block::BlockQuote(vec![para()]),
            Block::BulletList(vec![vec![para()]]),
            Block::DefinitionList(vec![(vec![image()], vec![vec![para()]])]),
            Block::Para(vec![Inline::Note(vec![para()])]),
            Block::Para(vec![Inline::Emph(vec![image()])]),
        ]);
        extract(&mut doc, &media, &dir).expect("extractable");

        let json = serde_json::to_string(&doc).expect("serializable");
        assert!(!json.contains("\"media/p.png\""), "a picture was left unrepointed: {json}");
        let expected = dir.join("media/p.png").to_string_lossy().into_owned();
        assert_eq!(json.matches(&expected).count(), 7, "{json}");
        assert_eq!(std::fs::read(dir.join("media/p.png")).expect("written"), b"bytes");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The package wins over a same-named file on disk. Nothing else in
    /// the workspace exercises `main.rs`, and swapping the two silently
    /// embeds the wrong picture.
    #[test]
    fn the_package_outranks_a_file_of_the_same_name() {
        let dir = std::env::temp_dir().join("ferrodoc-resolve-test");
        std::fs::create_dir_all(&dir).expect("a writable temp dir");
        std::fs::write(dir.join("pic.png"), b"from disk").expect("writable");

        let mut embedded = ferrodoc::Media::new();
        embedded.insert("pic.png".to_owned(), b"from the package".to_vec());
        assert_eq!(resolve(&embedded, &dir)("pic.png").as_deref(), Some(&b"from the package"[..]));

        // ...and the disk is still the fallback for what the package
        // never held, which is how `![](x.png)` in markdown resolves.
        assert_eq!(
            resolve(&ferrodoc::Media::new(), &dir)("pic.png").as_deref(),
            Some(&b"from disk"[..])
        );
        assert!(resolve(&ferrodoc::Media::new(), &dir)("absent.png").is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
