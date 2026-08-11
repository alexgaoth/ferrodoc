//! Differential test harness: compares ferrodoc's AST against
//! `pandoc -f commonmark -t json` over a corpus or the `CommonMark` spec.
//!
//! Usage:
//!   ferrodoc-harness diff-ast [--verbose] [--fail-under PCT] <file-or-dir>...
//!   ferrodoc-harness diff-spec [--verbose] [--fail-under PCT] <spec.json>

use anyhow::{Context, Result, bail};
use serde_json::Value;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

fn main() -> Result<()> {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let verbose = take_flag(&mut args, "--verbose");
    let fail_under = take_option(&mut args, "--fail-under")?
        .map(|v| v.parse::<f64>().context("--fail-under expects a number"))
        .transpose()?;
    match args.first().map(String::as_str) {
        Some("diff-ast") => diff_ast(&args[1..], verbose, fail_under),
        Some("diff-spec") => diff_spec(&args[1..], verbose, fail_under),
        _ => bail!("usage: ferrodoc-harness <diff-ast|diff-spec> [--verbose] [--fail-under PCT] <paths>"),
    }
}

fn take_flag(args: &mut Vec<String>, flag: &str) -> bool {
    let before = args.len();
    args.retain(|a| a != flag);
    args.len() != before
}

fn take_option(args: &mut Vec<String>, opt: &str) -> Result<Option<String>> {
    if let Some(i) = args.iter().position(|a| a == opt) {
        if i + 1 >= args.len() {
            bail!("{opt} expects a value");
        }
        let v = args.remove(i + 1);
        args.remove(i);
        return Ok(Some(v));
    }
    Ok(None)
}

/// One named markdown input to compare.
struct Case {
    name: String,
    markdown: String,
}

fn diff_ast(paths: &[String], verbose: bool, fail_under: Option<f64>) -> Result<()> {
    if paths.is_empty() {
        bail!("diff-ast expects at least one file or directory");
    }
    let mut cases = Vec::new();
    for p in paths {
        collect_markdown_files(Path::new(p), &mut cases)?;
    }
    run_cases(&cases, verbose, fail_under)
}

fn collect_markdown_files(path: &Path, cases: &mut Vec<Case>) -> Result<()> {
    if path.is_dir() {
        let mut entries: Vec<PathBuf> = std::fs::read_dir(path)
            .with_context(|| format!("reading {}", path.display()))?
            .map(|e| e.map(|e| e.path()))
            .collect::<std::io::Result<_>>()?;
        entries.sort();
        for entry in entries {
            if entry.is_dir() || entry.extension().is_some_and(|e| e == "md") {
                collect_markdown_files(&entry, cases)?;
            }
        }
    } else {
        cases.push(Case {
            name: path.display().to_string(),
            markdown: std::fs::read_to_string(path)
                .with_context(|| format!("reading {}", path.display()))?,
        });
    }
    Ok(())
}

fn diff_spec(paths: &[String], verbose: bool, fail_under: Option<f64>) -> Result<()> {
    let [spec_path] = paths else {
        bail!("diff-spec expects exactly one spec.json path");
    };
    let raw = std::fs::read_to_string(spec_path)
        .with_context(|| format!("reading {spec_path}"))?;
    let examples: Vec<Value> = serde_json::from_str(&raw).context("parsing spec.json")?;
    let cases: Vec<Case> = examples
        .iter()
        .map(|ex| {
            let number = ex["example"].as_i64().unwrap_or(0);
            let section = ex["section"].as_str().unwrap_or("?");
            Ok(Case {
                name: format!("example {number} ({section})"),
                markdown: ex["markdown"]
                    .as_str()
                    .context("spec example without markdown")?
                    .to_owned(),
            })
        })
        .collect::<Result<_>>()?;
    run_cases(&cases, verbose, fail_under)
}

fn run_cases(cases: &[Case], verbose: bool, fail_under: Option<f64>) -> Result<()> {
    if cases.is_empty() {
        bail!("no markdown inputs found");
    }
    let mut matched = 0usize;
    let mut failures: Vec<(&Case, String)> = Vec::new();
    for case in cases {
        let ours = serde_json::to_value(ferrodoc_markdown::read_commonmark(&case.markdown))?;
        let theirs = pandoc_json(&case.markdown)
            .with_context(|| format!("pandoc failed on {}", case.name))?;
        if ours == theirs {
            matched += 1;
        } else {
            let path = first_divergence(&ours, &theirs, "");
            failures.push((case, path));
        }
    }
    let total = cases.len();
    #[allow(clippy::cast_precision_loss)]
    let pct = 100.0 * matched as f64 / total as f64;
    for (case, path) in &failures {
        println!("MISMATCH {} at {path}", case.name);
        if verbose {
            println!("  input: {:?}", case.markdown);
        }
    }
    println!("{matched}/{total} identical ASTs ({pct:.1}%)");
    if let Some(threshold) = fail_under
        && pct < threshold
    {
        bail!("conformance {pct:.1}% is below threshold {threshold}%");
    }
    Ok(())
}

/// Run `pandoc -f commonmark -t json` on the given markdown.
fn pandoc_json(markdown: &str) -> Result<Value> {
    let mut child = Command::new("pandoc")
        .args(["-f", "commonmark", "-t", "json"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .context("spawning pandoc (is it on PATH?)")?;
    child
        .stdin
        .take()
        .expect("stdin was piped")
        .write_all(markdown.as_bytes())?;
    let output = child.wait_with_output()?;
    if !output.status.success() {
        bail!("pandoc exited with {}: {}", output.status, String::from_utf8_lossy(&output.stderr));
    }
    Ok(serde_json::from_slice(&output.stdout)?)
}

/// Return a JSON-pointer-ish path to the first structural divergence,
/// with both sides' values (truncated) for context.
fn first_divergence(ours: &Value, theirs: &Value, path: &str) -> String {
    match (ours, theirs) {
        (Value::Object(a), Value::Object(b)) => {
            let mut keys: Vec<&String> = a.keys().chain(b.keys()).collect();
            keys.sort();
            keys.dedup();
            for k in keys {
                match (a.get(k), b.get(k)) {
                    (Some(x), Some(y)) if x != y => {
                        return first_divergence(x, y, &format!("{path}/{k}"));
                    }
                    (Some(_), Some(_)) => {}
                    _ => return format!("{path}/{k} (present on one side only)"),
                }
            }
            unreachable!("objects compared equal but were not")
        }
        (Value::Array(a), Value::Array(b)) => {
            for (i, (x, y)) in a.iter().zip(b.iter()).enumerate() {
                if x != y {
                    return first_divergence(x, y, &format!("{path}/{i}"));
                }
            }
            format!("{path} (array lengths {} vs {})", a.len(), b.len())
        }
        (a, b) => format!("{path}: ours={} theirs={}", truncate(a), truncate(b)),
    }
}

fn truncate(v: &Value) -> String {
    let s = v.to_string();
    if s.len() > 120 {
        format!("{}…", s.chars().take(120).collect::<String>())
    } else {
        s
    }
}
