//! The `ferrodoc` command-line converter.

use ferrodoc::Format;
use std::io::{Read as _, Write as _};
use std::path::PathBuf;
use std::process::ExitCode;

const USAGE: &str = "\
ferrodoc — convert documents between markdown, HTML, DOCX and the pandoc AST

USAGE:
    ferrodoc [OPTIONS] [INPUT]

    With no INPUT, or with `-`, the document is read from standard input.
    With no --output, the result is written to standard output.

OPTIONS:
    -f, --from <FORMAT>     Input format   [inferred from INPUT's extension]
    -t, --to <FORMAT>       Output format  [inferred from --output's extension]
    -o, --output <FILE>     Write to FILE instead of standard output
    -h, --help              Print this help
    -V, --version           Print the version

FORMATS:
    input:   markdown (commonmark, md), gfm, html, docx, json
    output:  those, plus plain (text)

    `gfm` is GitHub Flavored Markdown: tables, task lists, strikethrough
    and bare-URL links. Prefer it over `markdown` for anything with a
    table — CommonMark has no table syntax, so a table degrades there to
    one paragraph per cell.

EXAMPLES:
    ferrodoc README.md -o readme.html
    ferrodoc report.docx -t gfm             # DOCX in, GitHub markdown out
    ferrodoc report.docx -t markdown        # DOCX in, CommonMark out
    ferrodoc page.html -t markdown          # HTML in, markdown out
    ferrodoc report.docx -t plain
    cat notes.md | ferrodoc -f markdown -t docx -o notes.docx
";

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("ferrodoc: {message}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), String> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut from: Option<Format> = None;
    let mut to: Option<Format> = None;
    let mut output: Option<PathBuf> = None;
    let mut input: Option<PathBuf> = None;
    let mut stdin_requested = false;

    let mut i = 0;
    while i < args.len() {
        let arg = args[i].as_str();
        let mut value = |name: &str| -> Result<String, String> {
            i += 1;
            args.get(i)
                .cloned()
                .ok_or_else(|| format!("{name} needs a value"))
        };
        match arg {
            "-h" | "--help" => {
                print!("{USAGE}");
                return Ok(());
            }
            "-V" | "--version" => {
                println!("ferrodoc {}", env!("CARGO_PKG_VERSION"));
                return Ok(());
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

    let bytes = if let Some(path) = &input {
        std::fs::read(path).map_err(|e| format!("cannot read {}: {e}", path.display()))?
    } else {
        let mut bytes = Vec::new();
        std::io::stdin()
            .read_to_end(&mut bytes)
            .map_err(|e| format!("cannot read standard input: {e}"))?;
        bytes
    };

    // Image paths in a document are relative to the document, the way
    // every editor that wrote one meant them.
    let base = input
        .as_deref()
        .and_then(std::path::Path::parent)
        .unwrap_or_else(|| std::path::Path::new("."))
        .to_owned();
    // Only when the output can hold them: a document's images can be far
    // larger than its text, and reading them to throw them away is how a
    // `docx -> markdown` conversion runs a machine out of memory.
    let (doc, embedded) = if to.embeds_media() {
        ferrodoc::parse_with_media(&bytes, from).map_err(|e| e.to_string())?
    } else {
        (ferrodoc::parse(&bytes, from).map_err(|e| e.to_string())?, ferrodoc::Media::new())
    };
    let converted = ferrodoc::render_with_media(&doc, to, &resolve(&embedded, &base))
        .map_err(|e| e.to_string())?;

    if let Some(path) = &output {
        std::fs::write(path, &converted)
            .map_err(|e| format!("cannot write {}: {e}", path.display()))?;
    } else {
        std::io::stdout()
            .write_all(&converted)
            .map_err(|e| format!("cannot write to standard output: {e}"))?;
    }
    Ok(())
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
    Format::parse(name).ok_or_else(|| {
        format!(
            "unknown format {name:?}; known formats: {}",
            Format::NAMES.join(", ")
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

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
