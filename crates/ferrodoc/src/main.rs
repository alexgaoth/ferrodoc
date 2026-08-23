//! The `ferrodoc` command-line converter.

use ferrodoc::{Format, Wrap};
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
    -c, --css <FILE>        Inline a stylesheet into that page
        --wrap <MODE>       auto | none | preserve, as pandoc means them:
                            `auto` fills to --columns (pandoc's default),
                            `none` puts each block on one line, `preserve`
                            keeps the document's own breaks. A writer that
                            cannot lay lines out that way says so rather
                            than ignoring the flag: markdown and gfm do
                            all three, html and plain are `none`, latex,
                            rst and asciidoc are `preserve`.
        --columns <N>       Fill width for --wrap=auto [72]
    -d, --defaults <FILE>   Read flags from a defaults file. Applied where
                            the flag appears, so a later option overrides
                            it and an earlier one does not, which is what
                            pandoc does. A key this build has no flag for
                            is an error naming the key.
        --toc               Put a table of contents at the top of the page.
                            HTML output with --standalone; accepted and
                            ignored otherwise, which is what pandoc does.
                            Three levels deep, pandoc's --toc-depth default.
    -N, --number-sections   Number the headings, and the contents entries
                            with them. HTML output; a heading whose classes
                            include `unnumbered` keeps its place and takes
                            no number.
    -M, --metadata <K[=V]>  Set a metadata field: `-M title=Report` names an
                            HTML page or a DOCX. A bare `-M draft` sets it
                            to true. Nothing is invented in the page head
                            beyond title, author and lang.
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
    (Format::PandocMarkdown, "pandoc_markdown"),
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
    ferrodoc manual.md -s --toc -N -o manual.html   # contents and numbering
    ferrodoc notes.md -M title='Q3 review' -o notes.docx
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
    // One readable format is not writable, so "those" would be a lie.
    let read_only: Vec<&str> = HELP_READABLE
        .iter()
        .filter(|(format, _)| format.compiled() && !format.writable())
        .map(|(_, spelling)| *spelling)
        .collect();
    let those = if read_only.is_empty() {
        "those".to_owned()
    } else {
        format!("those except {}", read_only.join(", "))
    };
    let mut block = format!("    input:   {}\n", listed(HELP_READABLE).join(", "));
    if joined.is_empty() {
        let _ = writeln!(block, "    output:  {those}");
    } else {
        let _ = writeln!(block, "    output:  {those}, plus {joined}");
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
    /// The layout asked for, or `None` for the writer's own — which is
    /// not the same for all of them; see `Format::wrapping`.
    wrap: Option<Wrap>,
    toc: bool,
    number_sections: bool,
    /// `-M key=value`, in the order given: a later one wins.
    metadata: Vec<(String, Option<String>)>,
}

/// `--wrap=auto|none|preserve`, pandoc's three, all now meaning what
/// they mean there.
///
/// `none` and `preserve` were the same value here, and they are not the
/// same thing: `none` joins every soft break into a space and `preserve`
/// keeps them. Measured against the binary, which is how a flag that had
/// been accepted and ignored for two versions was found.
///
/// `Auto(0)` is a placeholder the caller replaces once `--columns` has
/// been seen — the two flags may come in either order.
fn wrap_mode(mode: &str) -> Result<Wrap, String> {
    match mode {
        "auto" => Ok(Wrap::Auto(0)),
        "none" => Ok(Wrap::None),
        "preserve" => Ok(Wrap::Preserve),
        other => Err(format!("unknown --wrap {other:?}; expected auto, none or preserve")),
    }
}

/// `-M key=value` is a string; a bare `-M key` is `true`, which is what
/// `pandoc -M draft -t json` shows in `meta`.
fn metadata_pair(raw: String) -> (String, Option<String>) {
    match raw.split_once('=') {
        Some((key, value)) => (key.to_owned(), Some(value.to_owned())),
        None => (raw, None),
    }
}

/// Parse the command line. `Ok(None)` means `--help` or `--version`
/// already printed what was asked for and there is nothing left to do.
/// Replace every `--defaults FILE` with the flags that file stands for,
/// where it stood.
///
/// **Position is the precedence, and that is pandoc's rule rather than a
/// choice made here.** `pandoc -t plain --defaults d.yaml` takes `to`
/// from the file and `pandoc --defaults d.yaml -t plain` takes it from
/// the flag — measured both ways round. Splicing the expansion in at the
/// point the flag appeared gets that for free, where applying the file
/// before or after the command line would get it wrong half the time.
fn expand_defaults(args: &[String], depth: usize) -> Result<Vec<String>, String> {
    // A defaults file may name another. Bounded like every other
    // recursion here, and low: a chain this long is a mistake, not a
    // configuration.
    if depth > 8 {
        return Err("--defaults files are nested more than 8 deep".to_owned());
    }
    let mut out = Vec::with_capacity(args.len());
    let mut i = 0;
    while i < args.len() {
        let (name, attached) = match args[i].split_once('=') {
            Some((name, value)) if name.starts_with("--") => (name, Some(value.to_owned())),
            _ => (args[i].as_str(), None),
        };
        if name != "-d" && name != "--defaults" {
            out.push(args[i].clone());
            i += 1;
            continue;
        }
        let path = if let Some(value) = attached {
            value
        } else {
            i += 1;
            args.get(i)
                .cloned()
                .ok_or_else(|| "--defaults needs a value".to_owned())?
        };
        let text = std::fs::read_to_string(&path)
            .map_err(|e| format!("cannot read --defaults {path}: {e}"))?;
        out.extend(expand_defaults(&defaults_to_args(&text, &path)?, depth + 1)?);
        i += 1;
    }
    Ok(out)
}

/// The keys of a defaults file, as the flags they stand for.
///
/// The YAML read here is the same subset the pandoc-markdown metadata
/// reader accepts — `key: value`, `key:` with `- item` lines, `#`
/// comments — and **a key it does not know is an error naming that key**,
/// never a line skipped. A defaults file whose `filters:` were silently
/// dropped would convert the document and quietly leave out what the file
/// was written to do.
fn defaults_to_args(text: &str, path: &str) -> Result<Vec<String>, String> {
    let mut out = Vec::new();
    let mut lines = text.lines().peekable();
    while let Some(line) = lines.next() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') || trimmed == "---" {
            continue;
        }
        if line.starts_with([' ', '\t']) || trimmed.starts_with('-') {
            return Err(format!("{path}: cannot read {trimmed:?}: this reads a flat `key: value` file"));
        }
        let (key, raw) = trimmed
            .split_once(':')
            .ok_or_else(|| format!("{path}: cannot read {trimmed:?}: expected `key: value`"))?;
        let value = raw.trim().trim_matches(['"', '\'']).to_owned();
        // A key whose value is a list or a map: gather the `- item`
        // lines that follow so the message can name what was found.
        let mut items = Vec::new();
        while lines.peek().is_some_and(|l| l.starts_with([' ', '\t'])) {
            let item = lines.next().unwrap_or_default();
            items.push(item.trim().trim_start_matches("- ").to_owned());
        }
        let flags = defaults_key(key, &value, &items)
            .ok_or_else(|| format!("{path}: `{key}` is not a defaults key ferrodoc reads"))?;
        out.extend(flags);
    }
    Ok(out)
}

/// One defaults key, as flags. `None` means this build has no flag for
/// it — which is refused by name rather than ignored.
fn defaults_key(key: &str, value: &str, items: &[String]) -> Option<Vec<String>> {
    // `false` is the absence of a switch, not an unknown value.
    let switch = |flag: &str| match value {
        "true" | "yes" => Some(vec![flag.to_owned()]),
        "false" | "no" => Some(Vec::new()),
        _ => None,
    };
    match key {
        "from" | "reader" => Some(vec!["--from".to_owned(), value.to_owned()]),
        "to" | "writer" => Some(vec!["--to".to_owned(), value.to_owned()]),
        "output-file" => Some(vec!["--output".to_owned(), value.to_owned()]),
        "input-file" => Some(vec![value.to_owned()]),
        "standalone" => switch("--standalone"),
        "table-of-contents" | "toc" => switch("--toc"),
        "number-sections" => switch("--number-sections"),
        "wrap" => Some(vec!["--wrap".to_owned(), value.to_owned()]),
        "columns" => Some(vec!["--columns".to_owned(), value.to_owned()]),
        "css" => {
            // Pandoc takes a list here; this build inlines one
            // stylesheet, so more than one is refused rather than
            // silently reduced to the first.
            match (value.is_empty(), items) {
                (true, [only]) => Some(vec!["--css".to_owned(), only.clone()]),
                (false, []) => Some(vec!["--css".to_owned(), value.to_owned()]),
                _ => None,
            }
        }
        "extract-media" => Some(vec!["--extract-media".to_owned(), value.to_owned()]),
        "metadata" => Some(
            items
                .iter()
                .flat_map(|item| ["--metadata".to_owned(), item.replace(": ", "=")])
                .collect(),
        ),
        _ => None,
    }
}

fn parse_args(args: &[String]) -> Result<Option<Options>, String> {
    let args = &expand_defaults(args, 0)?;
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
    let mut wrap: Option<Wrap> = None;
    let mut columns = 72usize;
    let mut toc = false;
    let mut number_sections = false;
    let mut metadata: Vec<(String, Option<String>)> = Vec::new();

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
            // `-c` is pandoc's short form and appears in real Makefiles
            // more often than the long one.
            "-c" | "--css" => css = Some(PathBuf::from(value("--css")?)),
            "--toc" | "--table-of-contents" => toc = true,
            "-N" | "--number-sections" => number_sections = true,
            "-M" | "--metadata" => metadata.push(metadata_pair(value("--metadata")?)),
            "--extract-media" => {
                extract_media = Some(PathBuf::from(value("--extract-media")?));
            }
            "--wrap" => wrap = Some(wrap_mode(&value("--wrap")?)?),
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
        // `--columns` is only read when `--wrap=auto` asked for it, and
        // may have been given before or after it.
        wrap: wrap.map(|wrap| match wrap {
            Wrap::Auto(_) => Wrap::Auto(columns),
            other => other,
        }),
        toc,
        number_sections,
        metadata,
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
        wrap,
        toc,
        number_sections,
        metadata,
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
    for (key, value) in metadata {
        let value = match value {
            Some(value) => ferrodoc::ast::MetaValue::MetaString(value),
            // A bare `-M draft` is `true`, not the empty string: probed
            // with `pandoc -M draft -t json`.
            None => ferrodoc::ast::MetaValue::MetaBool(true),
        };
        doc.meta.insert(key, value);
    }
    if number_sections {
        #[cfg(feature = "html")]
        if to == Format::Html {
            ferrodoc::number_sections(&mut doc);
        }
        if to != Format::Html {
            // Pandoc numbers headings for LaTeX, DOCX and the rest as
            // well. Here it is an HTML transform, so saying nothing would
            // be a silent loss — the one thing the dialect warning in
            // 7c06bb3 exists to avoid.
            eprintln!("ferrodoc: --number-sections is HTML-only here; the {to} output is unnumbered");
        }
    }
    // `--toc` without `-s` is accepted and emits nothing, because there is
    // no page to put the contents in — probed, pandoc does the same, and
    // erroring would fail a Makefile that pandoc runs happily.
    if toc && to != Format::Html {
        eprintln!("ferrodoc: --toc is HTML-only here; the {to} output has no contents");
    }
    let converted = if standalone {
        render_page(&doc, to, css.as_deref(), toc)?
    } else {
        if css.is_some() {
            return Err("--css needs --standalone: a fragment has no <head>".to_owned());
        }
        match wrap {
            // The resolver goes to both arms now. It did not, so
            // `--wrap=auto -o out.docx` dropped every embedded picture:
            // the wrapped path called the writer that takes no media.
            Some(wrap) => {
                ferrodoc::render_wrapped_with_media(&doc, to, wrap, &resolve(&embedded, &base))
                    .map_err(|e| e.to_string())?
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
#[cfg_attr(not(any(feature = "html", feature = "latex")), allow(unused_variables))]
fn render_page(
    doc: &ferrodoc::Pandoc,
    to: Format,
    css: Option<&std::path::Path>,
    toc: bool,
) -> Result<Vec<u8>, String> {
    if to == Format::Latex {
        if css.is_some() {
            return Err("--css applies to html output, not latex".to_owned());
        }
        if toc {
            eprintln!("ferrodoc: --toc is HTML-only here; the latex output has no contents");
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
    return Ok(ferrodoc::render_html_standalone(doc, css.as_deref(), toc));
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
    // `markdown+footnotes-tables` is pandoc's extension syntax, and this
    // reads none of it. Refusing by name beats the alternative: a flag
    // that looks accepted and changes nothing is the failure mode this
    // project keeps finding in its own gates.
    if let Some((base, _)) = name.split_once(['+', '-'])
        && Format::parse(base).is_some()
        && Format::parse(name).is_none()
    {
        return Err(format!(
            "extension syntax is not supported: {name:?}. `markdown` is CommonMark, \
             `gfm` adds tables, task lists and footnotes, and `pandoc_markdown` adds \
             YAML metadata, header attributes, definition lists and super/subscript"
        ));
    }
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
    // Pandoc's own wording, byte for byte, because a Makefile that has
    // said `markdown_github` for ten years is where this came from and a
    // silent acceptance is the one answer that helps nobody. `dropin/`
    // found it: two rows differ in nothing but this line.
    if name.eq_ignore_ascii_case("markdown_github") {
        eprintln!("[WARNING] Deprecated: markdown_github. Use gfm instead.");
    }
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
    fn a_defaults_file_is_the_flags_it_stands_for() {
        let flags = |yaml: &str| defaults_to_args(yaml, "d.yaml");

        assert_eq!(
            flags("from: gfm\nto: html\nstandalone: true\n").expect("read"),
            ["--from", "gfm", "--to", "html", "--standalone"]
        );
        // `false` is the absence of a switch, not an unknown value.
        assert_eq!(flags("standalone: false\n").expect("read"), Vec::<String>::new());
        // Comments, the document marker and blank lines are not keys.
        assert_eq!(flags("---\n# a comment\n\nto: rst\n").expect("read"), ["--to", "rst"]);

        // A key with no flag behind it is refused **by name**: a
        // `filters:` quietly dropped would convert the document and leave
        // out what the file was written to do.
        let refused = flags("to: html\nfilters:\n  - x.lua\n").expect_err("filters");
        assert!(refused.contains("filters"), "{refused}");

        // Position is the precedence, which is pandoc's rule: measured
        // both ways round with `pandoc -t plain --defaults d.yaml` and
        // the same two the other way.
        let dir = std::env::temp_dir().join("ferrodoc-defaults-order");
        std::fs::create_dir_all(&dir).expect("a writable temp dir");
        let file = dir.join("d.yaml");
        std::fs::write(&file, "to: html\n").expect("written");
        let path = file.display().to_string();
        let expand = |args: &[&str]| {
            expand_defaults(
                &args.iter().map(|a| (*a).to_owned()).collect::<Vec<_>>(),
                0,
            )
            .expect("expanded")
        };
        assert_eq!(expand(&["-t", "plain", "--defaults", &path]), ["-t", "plain", "--to", "html"]);
        assert_eq!(expand(&["--defaults", &path, "-t", "plain"]), ["--to", "html", "-t", "plain"]);
        let _ = std::fs::remove_dir_all(&dir);
    }

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
        // Serialized, not the raw path: on Windows the separator is a
        // backslash and JSON doubles it, so matching the path as it comes
        // off the filesystem found nothing and this test failed there for
        // two days while passing everywhere else.
        let written = dir.join("media/p.png").to_string_lossy().into_owned();
        let expected = serde_json::to_string(&written).expect("serializable");
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
