//! The `ferrodoc` command-line converter.

use ferrodoc::{Format, convert};
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
    input:   markdown (commonmark, md), docx, json
    output:  html, docx, json, plain (text)

EXAMPLES:
    ferrodoc README.md -o readme.html
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

    let converted = convert(&bytes, from, to).map_err(|e| e.to_string())?;

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

fn format(name: &str) -> Result<Format, String> {
    Format::parse(name).ok_or_else(|| {
        format!(
            "unknown format {name:?}; known formats: {}",
            Format::NAMES.join(", ")
        )
    })
}
