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
                            `auto` fills to --columns, `none` puts each
                            block on one line, `preserve` keeps the
                            document's own breaks. Every text writer does
                            all three, and the default is `auto` — which
                            is pandoc's [auto]
        --columns <N>       Fill width for --wrap=auto [72]
    -d, --defaults <FILE>   Read flags from a defaults file. Applied where
                            the flag appears, so a later option overrides
                            it and an earlier one does not, which is what
                            pandoc does. A key this build has no flag for
                            is an error naming the key.
        --shift-heading-level-by N   Shift every heading; a heading pushed
                            above level 1 becomes a paragraph, and `-1` on
                            a leading `#` makes it the title
        --strip-comments    Drop HTML comments
        --ascii             Escape every non-ASCII character (html only)
        --id-prefix PREFIX  Prefix every identifier and internal link
        --metadata-file FILE   Metadata as a flat `key: value` file
        --resource-path DIR[:DIR]  Where to look for a picture the
                            document names, after its own directory
        --reference-doc FILE   Take the styles from this .docx
        --data-dir DIR      Where `templates/default.html5` and a
                            `--template` named rather than pathed live
        --eol crlf|lf|native  What ends a line in text output
        --quiet             Say nothing on stderr but errors
        --fail-if-warnings  Exit 3 if anything warned
        --verbose           Accepted; this build has no extra diagnostics
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
    -v, --version           Print the version
    -V, --variable KEY=VAL  A template variable, which wins over the
                            document's own metadata
        --template FILE     Use this template instead of pandoc's default
        --toc-depth N       How deep the contents go [3]
        --no-highlight      Do not colour code (see COMPATIBILITY.md for
                            the languages that are coloured)
    -H, --include-in-header FILE    Verbatim into <head>   } each implies
    -B, --include-before-body FILE  Verbatim after <body>  } --standalone,
    -A, --include-after-body FILE   Verbatim before </body>} as in pandoc

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
    table — CommonMark has no table syntax, so a table is written there
    as the raw `<table>`, which keeps it but is not pretty.

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
        // `--fail-if-warnings` turns a warning into a non-zero exit, and
        // pandoc's is **3** with a line saying why. A build script that
        // asked to be told is one that wanted to stop.
        Ok(()) if FAIL_ON_WARNING.load(std::sync::atomic::Ordering::Relaxed)
            && WARNED.load(std::sync::atomic::Ordering::Relaxed) =>
        {
            eprintln!("Failing because there were warnings.");
            ExitCode::from(3)
        }
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
    /// Everything that only shapes a standalone page.
    page: PageFlags,
    /// The flags that reshape the document or its bytes.
    shaping: Shaping,
    /// `--resource-path`, searched after the document's own directory.
    resource_path: Vec<PathBuf>,
    /// `--reference-doc`, already read.
    reference: Option<Vec<u8>>,
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

/// A `--metadata-file`, as the pairs it sets.
///
/// The same flat `key: value` YAML the defaults reader accepts — a
/// nested map or a list is refused by name rather than skipped, because
/// metadata silently dropped is a title that never appears in the
/// output to be noticed.
fn metadata_file(path: &str) -> Result<Vec<(String, String)>, String> {
    let text = slurp(path)?;
    let mut pairs = Vec::new();
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') || trimmed == "---" {
            continue;
        }
        if line.starts_with([' ', '\t']) || trimmed.starts_with('-') {
            return Err(format!(
                "{path}: cannot read {trimmed:?}: this reads a flat `key: value` file"
            ));
        }
        let (key, raw) = trimmed
            .split_once(':')
            .ok_or_else(|| format!("{path}: cannot read {trimmed:?}: expected `key: value`"))?;
        pairs.push((key.to_owned(), raw.trim().trim_matches(['"', '\'']).to_owned()));
    }
    Ok(pairs)
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

/// Everything the argument loop accumulates, so that the flags can live
/// in a function of their own.
///
/// This started as a dozen locals in `parse_args`. It stopped being one
/// once the CLI had thirty flags: the loop is the *shape* of parsing —
/// `--opt=value`, stdin, positional input — and the flag table is a
/// table.
#[derive(Default)]
struct Parsed {
    from: Option<Format>,
    to: Option<Format>,
    /// The names those two were spelled with, kept so a deprecated
    /// spelling is reported for the format actually used — a later
    /// `-f gfm` replaces an earlier `-f markdown_github`, and pandoc
    /// says nothing about the one it did not use.
    deprecated: Vec<String>,
    output: Option<PathBuf>,
    input: Option<PathBuf>,
    stdin_requested: bool,
    framing: Framing,
    page: PageFlags,
    shaping: Shaping,
    resource_path: Vec<PathBuf>,
    data_dir: Option<PathBuf>,
    reference: Option<Vec<u8>>,
    extract_media: Option<PathBuf>,
    wrap: Option<Wrap>,
    columns: usize,
    metadata: Vec<(String, Option<String>)>,
}

/// The three switches that frame the output document rather than change
/// its content.
#[derive(Default)]
struct Framing {
    standalone: bool,
    toc: bool,
    number_sections: bool,
}

/// One flag, or `Ok(false)` if it is not a flag this knows — which is how
/// the caller tells a positional input from a typo.
///
/// `value` is the caller's, because whether a value is attached with `=`
/// or is the next argument is the loop's business rather than the flag's.
fn take_flag(
    arg: &str,
    value: &mut dyn FnMut(&str) -> Result<String, String>,
    out: &mut Parsed,
) -> Result<bool, String> {
    match arg {
        "-f" | "--from" | "-t" | "--to" => {
            let name = value(arg)?;
            let parsed = format(&name)?;
            let slot = usize::from(matches!(arg, "-t" | "--to"));
            if out.deprecated.len() <= slot {
                out.deprecated.resize(slot + 1, String::new());
            }
            // The base name, because `markdown_github-hard_line_breaks`
            // is the same deprecated format wearing an extension.
            let base = name.split(['+', '-']).next().unwrap_or(&name);
            out.deprecated[slot] =
                if base.eq_ignore_ascii_case("markdown_github") { name } else { String::new() };
            if slot == 0 {
                out.from = Some(parsed);
            } else {
                out.to = Some(parsed);
            }
        }
        "-o" | "--output" => out.output = Some(PathBuf::from(value(arg)?)),
        // `--quiet` and `--fail-if-warnings` are read before the loop
        // starts — see `diagnostics`. `--verbose` adds `[INFO]` lines in
        // pandoc and there is nothing here that would say one; refusing
        // it would fail a command line pandoc runs, so its absence is a
        // row in COMPATIBILITY.md instead.
        "--quiet" | "--fail-if-warnings" | "--verbose" => {}
        // Pandoc colours code by default and so does this, for the
        // languages `COMPATIBILITY.md` names. `none` is the only value
        // reproducible here, so any other is refused **by name** rather
        // than accepted and ignored — a style that silently does nothing
        // is worse than one that says so.
        "--no-highlight" => out.page.highlighting = ferrodoc::Highlighting::None,
        "--syntax-highlighting" | "--highlight-style" => {
            let given = value(arg)?;
            if given != "none" {
                return Err(format!(
                    "{arg}={given}: only `none` is available here; \
                     COMPATIBILITY.md names the languages that are highlighted"
                ));
            }
            out.page.highlighting = ferrodoc::Highlighting::None;
        }
        "-s" | "--standalone" => out.framing.standalone = true,
        "--toc" | "--table-of-contents" => out.framing.toc = true,
        "-N" | "--number-sections" => out.framing.number_sections = true,
        "--strip-comments" => out.shaping.strip_comments = true,
        "--ascii" => out.shaping.ascii = true,
        "--shift-heading-level-by" | "--eol" => {
            let given = value(arg)?;
            shaping_option(arg, &given, &mut out.shaping)?;
        }
        // `DIR:DIR` on Unix, which is how a Makefile writes it.
        "--resource-path" => {
            out.resource_path.extend(value(arg)?.split(':').map(PathBuf::from));
        }
        "--data-dir" => out.data_dir = Some(PathBuf::from(value(arg)?)),
        // Read now: a missing reference should fail before the document
        // is converted rather than after.
        "--reference-doc" | "--reference-docx" => {
            let path = value(arg)?;
            out.reference =
                Some(std::fs::read(&path).map_err(|e| format!("cannot read {path}: {e}"))?);
        }
        "-M" | "--metadata" => out.metadata.push(metadata_pair(value(arg)?)),
        // The same flat `key: value` subset a `--defaults` file uses, and
        // refusing an unreadable key by name for the same reason:
        // metadata quietly dropped is a title that never appears.
        "--metadata-file" => {
            for (key, given) in metadata_file(&value(arg)?)? {
                out.metadata.push((key, Some(given)));
            }
        }
        "--extract-media" => out.extract_media = Some(PathBuf::from(value(arg)?)),
        "--wrap" => out.wrap = Some(wrap_mode(&value(arg)?)?),
        "--columns" => {
            let raw = value(arg)?;
            out.columns = raw
                .parse::<usize>()
                .ok()
                .filter(|n| *n > 0)
                .ok_or_else(|| format!("--columns needs a positive number, not {raw:?}"))?;
        }
        name if page_flag(name) => {
            let given = value(name)?;
            // The three include flags imply `--standalone` in pandoc,
            // and nothing else here does — `--css`, `--template`,
            // `--toc` and `-V` all leave a fragment a fragment,
            // measured one flag at a time. Without this a Makefile that
            // says `-H header.html` got a fragment where pandoc writes
            // a page (`dropin-013`).
            if matches!(
                name,
                "-H" | "--include-in-header"
                    | "-B"
                    | "--include-before-body"
                    | "-A"
                    | "--include-after-body"
            ) {
                out.framing.standalone = true;
            }
            page_option(name, given, &mut out.page)?;
        }
        _ => return Ok(false),
    }
    Ok(true)
}

fn parse_args(args: &[String]) -> Result<Option<Options>, String> {
    let args = &expand_defaults(args, 0)?;
    diagnostics(args);
    let mut out = Parsed { columns: 72, ..Parsed::default() };

    let mut i = 0;
    while i < args.len() {
        let (arg, attached) = split_attached(&args[i]);
        // `-v` is the version and `-V` is a variable, which is pandoc's
        // assignment and the opposite of what this had: `ferrodoc -s -V
        // lang=fr` printed a version string and converted nothing.
        if matches!(arg, "-h" | "--help" | "-v" | "--version") {
            print!("{}", if arg.contains('h') { usage() } else { version() });
            return Ok(None);
        }
        let mut value = |name: &str| -> Result<String, String> {
            if let Some(attached) = attached.clone() {
                return Ok(attached);
            }
            i += 1;
            args.get(i).cloned().ok_or_else(|| format!("{name} needs a value"))
        };
        if !take_flag(arg, &mut value, &mut out)? {
            match arg {
                // An explicit "-" means stdin, and cannot be combined
                // with a named file — silently ignoring one of them would
                // convert the wrong document.
                "-" => {
                    if out.input.is_some() {
                        return Err("more than one input given".to_owned());
                    }
                    out.stdin_requested = true;
                }
                other if other.starts_with('-') && other.len() > 1 => {
                    return Err(format!("unknown option {other} (try --help)"));
                }
                other => {
                    if out.input.is_some() || out.stdin_requested {
                        return Err("more than one input given".to_owned());
                    }
                    out.input = Some(PathBuf::from(other));
                }
            }
        }
        i += 1;
    }

    out.page.template = read_template(out.page.template.take(), out.data_dir.as_deref())?;

    // Pandoc's own wording, byte for byte, because a Makefile that has
    // said `markdown_github` for ten years is where this came from and a
    // silent acceptance is the one answer that helps nobody. `dropin/`
    // found it: two rows differ in nothing but this line. It is said
    // **after** parsing, once per side, for the spelling that survived —
    // `-f markdown_github -f gfm` uses `gfm` and pandoc says nothing.
    for _ in out.deprecated.iter().filter(|name| !name.is_empty()) {
        warn("[WARNING] Deprecated: markdown_github. Use gfm instead.");
    }

    // Formats not given explicitly come from the file extensions; that
    // resolution needs the input path, so it happens in `run`.
    Ok(Some(Options {
        from: out.from,
        to: out.to,
        output: out.output,
        input: out.input,
        standalone: out.framing.standalone,
        page: out.page,
        shaping: out.shaping,
        resource_path: out.resource_path,
        reference: out.reference,
        extract_media: out.extract_media,
        // `--columns` is only read when `--wrap=auto` asked for it, and
        // may have been given either side of it.
        //
        // **The default is `auto`, which is pandoc's.** It was each
        // writer's own layout until 2026-08-24, and no two writers
        // agreed: `html` and `plain` joined, the other five kept the
        // document's lines. That was chosen so a migration diff would be
        // readable, and it made every text conversion differ from the
        // same conversion through pandoc.
        wrap: Some(widened(out.wrap.unwrap_or(Wrap::Auto(0)), out.columns)),
        toc: out.framing.toc,
        number_sections: out.framing.number_sections,
        metadata: out.metadata,
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
        page,
        shaping,
        resource_path,
        reference,
        extract_media,
        wrap,
        toc,
        number_sections,
        metadata,
    }) = parse_args(&args)?
    else {
        return Ok(());
    };

    let from = inferred(from, input.as_deref(), "input", "--from", "a file")?;
    let to = inferred(to, output.as_deref(), "output", "--to", "an output file")?;

    let bytes = read_input(input.as_deref())?;
    if matches!(from, Format::Markdown | Format::Gfm) && opens_with_metadata_block(&bytes) {
        warn(
            "ferrodoc: this document opens with what pandoc would read as a YAML \
metadata block; ferrodoc reads CommonMark, where it is a thematic break and \
a heading in the body",
        );
    }

    // Image paths in a document are relative to the document, the way
    // every editor that wrote one meant them.
    let base = input
        .as_deref()
        .and_then(std::path::Path::parent)
        .unwrap_or_else(|| std::path::Path::new("."))
        .to_owned();
    let (mut doc, embedded) = read_document(&bytes, from, to, extract_media.is_some())?;
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
            warn(&format!(
                "ferrodoc: --number-sections is HTML-only here; the {to} output is unnumbered"
            ));
        }
    }
    // `--toc` without `-s` is accepted and emits nothing, because there is
    // no page to put the contents in — probed, pandoc does the same, and
    // erroring would fail a Makefile that pandoc runs happily.
    if toc && to != Format::Html {
        warn(&format!("ferrodoc: --toc is HTML-only here; the {to} output has no contents"));
    }
    if shaping.strip_comments {
        ferrodoc::strip_comments(&mut doc);
    }
    // Before `--number-sections`, which counts levels: shifting after it
    // would number the document that was, not the one being written.
    if shaping.shift_headings != 0 {
        ferrodoc::shift_heading_level(&mut doc, shaping.shift_headings);
    }
    ferrodoc::prefix_identifiers(&mut doc, &page.id_prefix);

    // Pandoc puts the **input file's name** in `<title>` when the
    // document has no title of its own; only the caller knows it.
    let stem = input
        .as_deref()
        .and_then(|path| path.file_stem())
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_default();
    let page = page.as_page(toc, &stem, wrap);
    if reference.is_some() && !matches!(to, Format::Docx | Format::Odt) {
        // The library answers this with `NotWritable`, which reads as "it
        // is an input-only format" — true of nothing here and wrong about
        // latex, which is output-only. The flag's own message belongs
        // where the flag is.
        return Err(format!("--reference-doc applies to docx and odt output, not {to}"));
    }
    let converted = if let Some(reference) = &reference {
        // `--reference-doc` replaces the whole render: the package it
        // produces is the reference's, with this document in it.
        ferrodoc::render_with_reference(
            &doc,
            to,
            reference,
            &resolve(&embedded, &base, &resource_path),
        )
        // The library says "invalid odt input", because to it the
        // reference *is* an input. Here the input is the document, and
        // blaming it would send someone to the wrong file.
        .map_err(|e| format!("--reference-doc: {e}"))?
    } else if wants_page(standalone, to, &doc)? {
        render_page(&doc, to, &page)?
    } else {
        render_fragment(&doc, to, wrap, &page, &resolve(&embedded, &base, &resource_path))?
    };

    write_output(output.as_deref(), &reshaped(converted, &shaping, to)?)
}

/// The document without a page around it.
fn render_fragment(
    doc: &ferrodoc::Pandoc,
    to: Format,
    wrap: Option<ferrodoc::Wrap>,
    page: &ferrodoc::Page<'_>,
    media: &dyn Fn(&str) -> Option<Vec<u8>>,
) -> Result<Vec<u8>, String> {
    if !page.css.is_empty() {
        return Err("--css needs --standalone: a fragment has no <head>".to_owned());
    }
    // Highlighting is a property of the HTML writer, and a fragment
    // reaches it through neither the page nor the wrap resolver — so
    // `--no-highlight` on a fragment went through this function and did
    // nothing at all until it was asked here.
    if to == Format::Html && page.highlighting == ferrodoc::Highlighting::None {
        return Ok(ferrodoc::render_html_unhighlighted(
            doc,
            &page.id_prefix,
            wrap.unwrap_or(ferrodoc::Wrap::Auto(72)),
        ));
    }
    // A fragment with `--id-prefix` needs the prefix on the footnote
    // identifiers too, and those are invented by the writer rather than
    // carried by the tree.
    if to == Format::Html && !page.id_prefix.is_empty() {
        return Ok(ferrodoc::render_html_with_id_prefix(
            doc,
            &page.id_prefix,
            wrap.unwrap_or(ferrodoc::Wrap::Auto(72)),
        ));
    }
    match wrap {
        // The resolver goes to both arms now. It did not, so
        // `--wrap=auto -o out.docx` dropped every embedded picture: the
        // wrapped path called the writer that takes no media.
        Some(wrap) => ferrodoc::render_wrapped_with_media(doc, to, wrap, media),
        None => ferrodoc::render_with_media(doc, to, media),
    }
    .map_err(|e| e.to_string())
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

/// Whether `-s` writes a page here, is pandoc's no-op, or is refused.
///
/// Pandoc accepts `-s` for every format, and for one with no page form it
/// is a **no-op only while the document carries no metadata**: with any at
/// all it writes a title block, and for `plain` that is two blank lines
/// even for a key no title block would show. Measured key by key against
/// 3.8.2.1.
///
/// So accept and ignore where the bytes are identical, and refuse by name
/// where they would not be. Erroring on both was worse than either:
/// `pandoc --standalone --to man x.md`, a real line from a real Makefile,
/// wrote nothing here at all.
fn wants_page(standalone: bool, to: Format, doc: &ferrodoc::Pandoc) -> Result<bool, String> {
    match (standalone, to) {
        (false, _) => Ok(false),
        (true, Format::Html | Format::Latex) => Ok(true),
        (true, _) if doc.meta.is_empty() => Ok(false),
        (true, _) => Err(format!(
            "--standalone for {to} would write a title block from this document's \
             metadata, which this build does not write; drop -s, or convert to \
             html or latex, which have one"
        )),
    }
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
    page: &ferrodoc::Page<'_>,
) -> Result<Vec<u8>, String> {
    if to == Format::Latex {
        if !page.css.is_empty() {
            return Err("--css applies to html output, not latex".to_owned());
        }
        if page.toc {
            warn("ferrodoc: --toc is HTML-only here; the latex output has no contents");
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
        // `run` decides before this is reached: `-s` for a format with no
        // page form is either ignored or refused there. Kept as a guard,
        // because a third page format added later should not silently get
        // a fragment out of this function.
        return Err(format!("--standalone applies to html or latex output, not {to}"));
    }
    #[cfg(feature = "html")]
    return ferrodoc::render_page(doc, page);
    #[cfg(not(feature = "html"))]
    return Err(format!("{to} support was not compiled into this build"));
}

/// Check a `base+ext-ext` spelling and return the base.
///
/// Accepted when every `+ext` is one the dialect already has and every
/// `-ext` is one it already lacks — the request is then the conversion
/// that would have happened anyway. Anything else names what it cannot
/// do, and which of this build's dialects could:
///
/// ```console
/// $ ferrodoc x.md -f markdown+footnotes -t html
/// ferrodoc: `markdown` does not read `footnotes` here, and this build
/// cannot turn one on: `gfm` and `pandoc_markdown` read it
/// ```
fn extensions(name: &str) -> Result<String, String> {
    let Some(cut) = name.find(['+', '-']) else {
        return Ok(name.to_owned());
    };
    let (base, mut rest) = name.split_at(cut);
    // A dialect whose *own name* has a dash — none today, but `Format`
    // decides that, not this function.
    if Format::parse(name).is_some() {
        return Ok(name.to_owned());
    }
    let format = Format::parse(base)
        .ok_or_else(|| format!("unknown format {base:?} in {name:?}"))?;
    let has = |extension: &str| format.extensions().contains(&extension);
    while !rest.is_empty() {
        let on = rest.starts_with('+');
        let end = rest[1..].find(['+', '-']).map_or(rest.len(), |at| at + 1);
        let extension = &rest[1..end];
        rest = &rest[end..];
        if extension.is_empty() {
            return Err(format!("{name:?} has a `+` or `-` with no extension after it"));
        }
        // The name is checked **before** asking whether it is a no-op:
        // `-nothing` is not "a extension this dialect already lacks", it
        // is a typo, and treating it as a no-op accepted it silently.
        if !ferrodoc::EXTENSIONS.contains(&extension) {
            return Err(format!(
                "no extension named {extension:?}; `pandoc --list-extensions` has the names"
            ));
        }
        if has(extension) == on {
            // Already how this dialect reads: nothing is being asked for.
            continue;
        }
        let elsewhere: Vec<&str> = [Format::Markdown, Format::Gfm, Format::PandocMarkdown]
            .into_iter()
            .filter(|other| *other != format && other.extensions().contains(&extension))
            .map(Format::name)
            .collect();
        let instead = if elsewhere.is_empty() {
            "no dialect here reads it".to_owned()
        } else {
            format!("{} reads it", elsewhere.join(" and "))
        };
        return Err(if on {
            format!("`{base}` does not read `{extension}` here, and this build cannot turn one on: {instead}")
        } else {
            format!("`{base}` reads `{extension}` here and this build cannot turn it off: {instead}")
        });
    }
    Ok(base.to_owned())
}

fn version() -> String {
    format!("ferrodoc {}\n", env!("CARGO_PKG_VERSION"))
}

/// `--opt=value` as well as `--opt value`, because that is how pandoc is
/// written in everybody's existing Makefile.
fn split_attached(arg: &str) -> (&str, Option<String>) {
    match arg.split_once('=') {
        Some((name, value)) if name.starts_with("--") => (name, Some(value.to_owned())),
        _ => (arg, None),
    }
}

/// Read `--quiet` and `--fail-if-warnings` before anything else.
///
/// **They are position-independent**, which an ordinary match arm cannot
/// be: `-f markdown_github --quiet` warns *while the first flag is being
/// parsed*, so a `--quiet` reached in turn arrives too late to silence
/// it. Pandoc silences it.
fn diagnostics(args: &[String]) {
    use std::sync::atomic::Ordering::Relaxed;
    for arg in args {
        match arg.as_str() {
            "--quiet" => QUIET.store(true, Relaxed),
            "--fail-if-warnings" => FAIL_ON_WARNING.store(true, Relaxed),
            _ => {}
        }
    }
}

/// Whether warnings are printed, and whether any has been.
///
/// A CLI process, so two atomics rather than a value threaded through
/// every function that might have something to say. `--quiet` and
/// `--fail-if-warnings` are pandoc's, and both are position-independent
/// there, so a sink the whole run shares is the shape that matches.
static QUIET: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
static WARNED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
/// `--fail-if-warnings`: read once, in `main`, after the run.
static FAIL_ON_WARNING: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

/// Say something on stderr that is not an error, unless `--quiet`.
///
/// Every warning goes through here so that `--quiet` silences all of
/// them and `--fail-if-warnings` counts all of them — a warning printed
/// with `eprintln!` beside this is one neither flag can see.
fn warn(message: &str) {
    use std::sync::atomic::Ordering::Relaxed;
    WARNED.store(true, Relaxed);
    if !QUIET.load(Relaxed) {
        eprintln!("{message}");
    }
}

/// A format not given explicitly comes from a file extension — and when
/// there is no file either, saying which flag would have answered it is
/// the whole of the message.
fn inferred(
    given: Option<Format>,
    path: Option<&std::path::Path>,
    role: &str,
    flag: &str,
    what: &str,
) -> Result<Format, String> {
    given.or_else(|| path.and_then(Format::from_path)).ok_or_else(|| {
        format!("cannot tell the {role} format: pass {flag}, or name {what} with a known extension")
    })
}

/// `--columns` is only read when `--wrap=auto` asked for it, and may
/// have been given either side of it — so the width is applied once both
/// are known rather than as `--wrap` is parsed.
fn widened(wrap: Wrap, columns: usize) -> Wrap {
    match wrap {
        Wrap::Auto(_) => Wrap::Auto(columns),
        other => other,
    }
}

/// Read the document, and its media only when something will hold it.
///
/// A document's images can be far larger than its text, and reading them
/// to throw them away is how a `docx -> markdown` conversion runs a
/// machine out of memory.
fn read_document(
    bytes: &[u8],
    from: Format,
    to: Format,
    extracting: bool,
) -> Result<(ferrodoc::Pandoc, ferrodoc::Media), String> {
    if to.embeds_media() || extracting {
        return ferrodoc::parse_with_media(bytes, from).map_err(|e| e.to_string());
    }
    let doc = ferrodoc::parse(bytes, from).map_err(|e| e.to_string())?;
    Ok((doc, ferrodoc::Media::new()))
}

/// The flags that reshape the document or the bytes it is written as.
/// Grouped for the same reason as [`PageFlags`]: separately they are four
/// near-identical arms in a function that has enough of them.
#[derive(Default)]
struct Shaping {
    /// `--shift-heading-level-by`.
    shift_headings: i64,
    /// `--strip-comments`.
    strip_comments: bool,
    /// `--eol`: what ends a line in text output.
    eol: Option<&'static str>,
    /// `--ascii`, which this build has for HTML alone.
    ascii: bool,
}

fn shaping_option(name: &str, given: &str, shaping: &mut Shaping) -> Result<(), String> {
    match name {
        "--shift-heading-level-by" => {
            shaping.shift_headings = given
                .parse()
                .map_err(|_| format!("--shift-heading-level-by wants a number, not {given:?}"))?;
        }
        "--eol" => {
            shaping.eol = match given {
                "crlf" => Some("\r\n"),
                "lf" => Some("\n"),
                // Pandoc's `native` is the platform's, and this builds
                // for one platform at a time.
                "native" => Some(if cfg!(windows) { "\r\n" } else { "\n" }),
                other => {
                    return Err(format!("unknown --eol {other:?}; expected crlf, lf or native"));
                }
            };
        }
        other => return Err(format!("{other} is not a shaping flag")),
    }
    Ok(())
}

/// The flags that only shape a standalone page, gathered so
/// `parse_args` does not carry seven near-identical arms.
struct PageFlags {
    /// `--css`/`-c`, as **URLs to link**, which is what pandoc's flag
    /// means. This inlined the file's contents until 0.3.
    css: Vec<String>,
    /// `--toc-depth`, pandoc's default of 3 when not given.
    toc_depth: i64,
    /// `-H`, `-B`, `-A` — read at parse time so a missing file fails
    /// before the document is converted rather than after.
    header_includes: Vec<String>,
    include_before: Vec<String>,
    include_after: Vec<String>,
    /// `-V key=value`, which wins over the document's own metadata.
    variables: Vec<(String, String)>,
    /// `--template`, already read.
    template: Option<String>,
    /// `--id-prefix`.
    id_prefix: String,
    /// `--no-highlight` / `--syntax-highlighting=none`.
    highlighting: ferrodoc::Highlighting,
}

impl Default for PageFlags {
    fn default() -> Self {
        PageFlags {
            css: Vec::new(),
            // Pandoc's default, and the only field whose zero value would
            // be wrong.
            toc_depth: 3,
            header_includes: Vec::new(),
            include_before: Vec::new(),
            include_after: Vec::new(),
            variables: Vec::new(),
            template: None,
            id_prefix: String::new(),
            highlighting: ferrodoc::Highlighting::default(),
        }
    }
}

/// Whether this flag is one of the page flags, all of which take a value.
impl PageFlags {
    /// As the library sees them. `pagetitle` is the input file's name,
    /// which is what pandoc puts in `<title>` when the document has no
    /// title and which only the caller knows.
    fn as_page<'a>(&'a self, toc: bool, stem: &'a str, wrap: Option<Wrap>) -> ferrodoc::Page<'a> {
        ferrodoc::Page {
            css: self.css.clone(),
            toc,
            toc_depth: self.toc_depth,
            header_includes: self.header_includes.clone(),
            include_before: self.include_before.clone(),
            include_after: self.include_after.clone(),
            variables: self.variables.clone(),
            template: self.template.as_deref(),
            id_prefix: self.id_prefix.clone(),
            pagetitle: Some(stem),
            highlighting: self.highlighting,
            // A page's body fills like a fragment's; the template around
            // it never does, because it is not the document.
            wrap: match wrap {
                Some(Wrap::Preserve) => ferrodoc::HtmlWrap::Preserve,
                Some(Wrap::Auto(columns)) => ferrodoc::HtmlWrap::Fill(columns),
                Some(Wrap::None) | None => ferrodoc::HtmlWrap::None,
            },
        }
    }
}

fn page_flag(name: &str) -> bool {
    matches!(
        name,
        "-c" | "--css"
            | "--toc-depth"
            | "-H"
            | "--include-in-header"
            | "-B"
            | "--include-before-body"
            | "-A"
            | "--include-after-body"
            | "-V"
            | "--variable"
            | "--template"
            | "--id-prefix"
    )
}

fn page_option(name: &str, given: String, page: &mut PageFlags) -> Result<(), String> {
    match name {
        "-c" | "--css" => page.css.push(given),
        "--toc-depth" => {
            page.toc_depth = given
                .parse()
                .map_err(|_| format!("--toc-depth wants a number, not {given:?}"))?;
        }
        "-H" | "--include-in-header" => page.header_includes.push(slurp_value(&given)?),
        "-B" | "--include-before-body" => page.include_before.push(slurp_value(&given)?),
        "-A" | "--include-after-body" => page.include_after.push(slurp_value(&given)?),
        "-V" | "--variable" => {
            let (key, value) = metadata_pair(given);
            // `-V draft` with no value is `true`, as `-M draft` is.
            page.variables.push((key, value.unwrap_or_else(|| "true".to_owned())));
        }
        // The *path*, read after parsing rather than here: `--data-dir`
        // may come after `--template` on the command line, and it is
        // where a template named rather than pathed is found.
        "--template" => page.template = Some(given),
        "--id-prefix" => page.id_prefix = given,
        // `page_flag` is the list; the two cannot disagree without this
        // arm being reached.
        other => return Err(format!("{other} is not a page flag")),
    }
    Ok(())
}

/// Read the `--template`, looking in `--data-dir` for one named rather
/// than pathed — and take the data directory's *default* template when
/// there is one and no `--template` was given.
///
/// Pandoc's naming, measured: the override is `templates/default.html5`,
/// not `templates/html5.html`. A data directory with the second in it is
/// ignored by pandoc, so it is ignored here.
fn read_template(
    template: Option<String>,
    data_dir: Option<&std::path::Path>,
) -> Result<Option<String>, String> {
    if let Some(named) = template {
        // Verbatim: a template's trailing newline is its own, and
        // trimming it the way an include is trimmed costs the page its
        // last line.
        if let Some(dir) = data_dir
            && !std::path::Path::new(&named).is_file()
        {
            return slurp(&dir.join("templates").join(&named).to_string_lossy()).map(Some);
        }
        return slurp(&named).map(Some);
    }
    let Some(dir) = data_dir else { return Ok(None) };
    let default = dir.join("templates").join("default.html5");
    if default.is_file() {
        return slurp(&default.to_string_lossy()).map(Some);
    }
    Ok(None)
}

/// The two flags that rewrite the finished bytes.
fn reshaped(converted: Vec<u8>, shaping: &Shaping, to: Format) -> Result<Vec<u8>, String> {
    // `--ascii` is HTML's here: every other writer spells the escape
    // differently — `&eacute;` in markdown, `\'{e}` in LaTeX, nothing at
    // all in RST — and inventing one would be a flag that looks honoured
    // and writes something pandoc does not.
    let converted = if shaping.ascii {
        if to != Format::Html {
            return Err(format!(
                "--ascii is html-only here; the {to} writer has its own spelling for a \
                 non-ASCII character and this build does not write it"
            ));
        }
        ferrodoc::ascii_only(&String::from_utf8_lossy(&converted)).into_bytes()
    } else {
        converted
    };
    // `--eol` rewrites text output only: a `.docx` is a zip, and
    // rewriting bytes inside one would corrupt it. Pandoc applies it to
    // text alone for the same reason.
    Ok(match shaping.eol {
        Some(ending) if ending != "\n" && !to.embeds_media() => {
            String::from_utf8_lossy(&converted).replace('\n', ending).into_bytes()
        }
        _ => converted,
    })
}

/// A file the command line named, read now rather than at conversion
/// time: `-H missing.html` should fail before the document is converted,
/// not after.
fn slurp(path: &str) -> Result<String, String> {
    std::fs::read_to_string(path).map_err(|e| format!("cannot read {path}: {e}"))
}

/// An `-H`/`-B`/`-A` include, which is a template *value* rather than a
/// template: its trailing newline goes, because the template supplies the
/// one that ends the line it is interpolated on. Keeping the file's own
/// put a blank line after every include — and trimming a `--template`
/// the same way cost its last newline, so the two cannot share a reader.
fn slurp_value(path: &str) -> Result<String, String> {
    slurp(path).map(|text| text.trim_end_matches('\n').to_owned())
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
    search: &'a [PathBuf],
) -> impl Fn(&str) -> Option<Vec<u8>> + 'a {
    move |url| {
        if let Some(bytes) = embedded.get(url) {
            return Some(bytes.clone());
        }
        // The document's own directory first, then `--resource-path` in
        // the order it was given — pandoc's order, and the reason the
        // flag exists is a build that keeps its pictures somewhere else.
        std::iter::once(base)
            .chain(search.iter().map(PathBuf::as_path))
            .find_map(|dir| std::fs::read(dir.join(url)).ok())
    }
}

fn format(name: &str) -> Result<Format, String> {
    // `markdown+footnotes-pipe_tables` is pandoc's extension syntax. What
    // is accepted here is what the named dialect **already does**: a
    // request that asks for nothing new is the same conversion, and one
    // that asks for a change this build cannot make is refused by name.
    //
    // The alternative — accepting the syntax and ignoring it — is a flag
    // that looks honoured and changes nothing, which is the failure this
    // project keeps finding in its own gates.
    let name = &extensions(name)?;
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

    /// Which flags imply `--standalone`, measured one at a time against
    /// pandoc: the three include flags do and nothing else here does.
    /// A `-H header.html` that wrote a fragment is `dropin-013`.
    #[test]
    fn the_include_flags_imply_standalone() {
        let include = std::env::temp_dir().join("ferrodoc-include.html");
        std::fs::write(&include, "<!-- x -->\n").expect("a writable temp dir");
        let path = include.to_string_lossy().into_owned();
        let parse = |flag: &str, value: &str| {
            let argv = [flag, value, "x.md"].map(str::to_owned);
            parse_args(&argv).expect("parsed").expect("options").standalone
        };
        for flag in ["-H", "--include-in-header", "-B", "--include-before-body", "-A"] {
            assert!(parse(flag, &path), "{flag} should imply --standalone");
        }
        for (flag, value) in [("-c", "x.css"), ("--toc-depth", "2"), ("-V", "k=v")] {
            assert!(!parse(flag, value), "{flag} should not imply --standalone");
        }
        let _ = std::fs::remove_file(&include);
    }

    /// Every container, because a walk that misses one leaves a picture
    /// pointing at a file that is not there — and nothing fails loudly.
    #[test]
    fn standalone_is_pandocs_no_op_until_the_document_has_metadata() {
        // `pandoc --standalone --to man x.md` is a real Makefile line, and
        // erroring on it wrote nothing at all where pandoc writes the
        // document.
        let plain = ferrodoc::Pandoc::new(Vec::new());
        assert!(!wants_page(true, Format::Plain, &plain).expect("no-op"));
        assert!(!wants_page(true, Format::Rst, &plain).expect("no-op"));
        assert!(wants_page(true, Format::Html, &plain).expect("a page"));
        assert!(wants_page(true, Format::Latex, &plain).expect("a page"));
        assert!(!wants_page(false, Format::Html, &plain).expect("a fragment"));

        // With metadata it is not a no-op — pandoc writes a title block —
        // so it is refused by name rather than quietly writing the
        // document without one.
        let mut titled = ferrodoc::Pandoc::new(Vec::new());
        titled.meta.insert(
            "title".to_owned(),
            ferrodoc_ast::MetaValue::MetaString("X".to_owned()),
        );
        let error = wants_page(true, Format::Plain, &titled).expect_err("a title block");
        assert!(error.contains("title block"), "{error}");
    }

    #[test]
    fn extension_syntax_is_accepted_when_it_asks_for_nothing() {
        // What the dialect already does is the same conversion, so the
        // spelling is accepted and the base comes back.
        for accepted in [
            "gfm+footnotes",
            "gfm+pipe_tables+strikeout",
            "markdown-footnotes",
            "pandoc_markdown+yaml_metadata_block",
        ] {
            let base = accepted.split(['+', '-']).next().expect("a base");
            assert_eq!(extensions(accepted).as_deref(), Ok(base), "{accepted}");
        }

        // Anything that asks for a change this build cannot make names
        // the extension, and where it does exist.
        let off = extensions("gfm-pipe_tables").expect_err("cannot disable");
        assert!(off.contains("pipe_tables") && off.contains("cannot turn it off"), "{off}");
        let on = extensions("markdown+footnotes").expect_err("cannot enable");
        assert!(on.contains("gfm"), "{on}");

        // A name pandoc does not have is a typo, and saying "this dialect
        // lacks it" would send someone looking for the wrong thing. It is
        // checked before the no-op test, which accepted `-nothing`.
        let typo = extensions("gfm+fotnotes").expect_err("typo");
        assert!(typo.contains("no extension named"), "{typo}");
        let also = extensions("gfm-nothing").expect_err("typo the other way");
        assert!(also.contains("no extension named"), "{also}");

        // A plain name is untouched.
        assert_eq!(extensions("gfm").as_deref(), Ok("gfm"));
    }

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
        let none: &[PathBuf] = &[];
        assert_eq!(
            resolve(&embedded, &dir, none)("pic.png").as_deref(),
            Some(&b"from the package"[..])
        );

        // ...and the disk is still the fallback for what the package
        // never held, which is how `![](x.png)` in markdown resolves.
        let empty = ferrodoc::Media::new();
        assert_eq!(resolve(&empty, &dir, none)("pic.png").as_deref(), Some(&b"from disk"[..]));
        assert!(resolve(&empty, &dir, none)("absent.png").is_none());

        // `--resource-path` is searched **after** the document's own
        // directory, so a picture beside the document still wins.
        let elsewhere = dir.join("pics");
        std::fs::create_dir_all(&elsewhere).expect("a writable temp dir");
        std::fs::write(elsewhere.join("pic.png"), b"from the path").expect("writable");
        std::fs::write(elsewhere.join("only.png"), b"only here").expect("writable");
        let search = vec![elsewhere.clone()];
        assert_eq!(
            resolve(&empty, &dir, &search)("pic.png").as_deref(),
            Some(&b"from disk"[..])
        );
        assert_eq!(
            resolve(&empty, &dir, &search)("only.png").as_deref(),
            Some(&b"only here"[..])
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
