//! Differential test harness: compares ferrodoc's AST (and HTML output)
//! against pandoc over a corpus or the `CommonMark` spec, and benchmarks
//! the in-process pipeline against the pandoc subprocess.
//!
//! Usage:
//!   ferrodoc-harness diff-ast  [--verbose] [--fail-under PCT] <file-or-dir>...
//!   ferrodoc-harness diff-spec [--verbose] [--fail-under PCT] <spec.json>
//!   ferrodoc-harness diff-html [--verbose] [--fail-under PCT] <file-dir-or-spec.json>...
//!   ferrodoc-harness diff-write [--verbose] [--fail-under PCT] <file-dir-or-spec.json>...
//!   ferrodoc-harness diff-md [--verbose] [--fail-under PCT] <file-dir-or-spec.json>...
//!   ferrodoc-harness bench [--iters N] <file>...
//!   ferrodoc-harness bench-docx [--iters N] <file>...

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
    let iters = take_option(&mut args, "--iters")?
        .map(|v| v.parse::<u32>().context("--iters expects a number"))
        .transpose()?
        .unwrap_or(50)
        .max(1);
    match args.first().map(String::as_str) {
        Some("diff-ast") => diff_ast(&args[1..], verbose, fail_under),
        Some("diff-spec") => diff_spec(&args[1..], verbose, fail_under),
        Some("diff-html") => diff_html(&args[1..], verbose, fail_under),
        Some("diff-docx") => diff_docx(&args[1..], verbose, fail_under),
        Some("diff-write") => diff_write(&args[1..], verbose, fail_under),
        Some("diff-md") => diff_md(&args[1..], verbose, fail_under),
        Some("bench") => bench(&args[1..], iters),
        Some("bench-docx") => bench_docx(&args[1..], iters),
        _ => bail!(
            "usage: ferrodoc-harness <diff-ast|diff-spec|diff-html|diff-docx|diff-write|diff-md|bench|bench-docx> [--verbose] [--fail-under PCT] [--iters N] <paths>"
        ),
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
        let ours = serde_json::to_value(ferrodoc_markdown::read_commonmark(&case.markdown).map_err(|e| anyhow::anyhow!("{e}"))?)?;
        let theirs = pandoc_json(&case.markdown)
            .with_context(|| format!("pandoc failed on {}", case.name))?;
        if ours == theirs {
            matched += 1;
        } else {
            let path = first_divergence(&ours, &theirs, "");
            failures.push((case, path));
        }
    }
    report(cases, matched, &failures, verbose, fail_under)
}

/// Print mismatches and the conformance summary; gate the exit code on
/// `--fail-under`.
fn report(
    cases: &[Case],
    matched: usize,
    failures: &[(&Case, String)],
    verbose: bool,
    fail_under: Option<f64>,
) -> Result<()> {
    let total = cases.len();
    #[allow(clippy::cast_precision_loss)]
    let pct = 100.0 * matched as f64 / total as f64;
    for (case, place) in failures {
        println!("MISMATCH {} at {place}", case.name);
        if verbose {
            println!("  input: {:?}", case.markdown);
        }
    }
    println!("{matched}/{total} identical ({pct:.1}%)");
    if let Some(threshold) = fail_under
        && pct < threshold
    {
        bail!("conformance {pct:.1}% is below threshold {threshold}%");
    }
    Ok(())
}

/// Collect cases from paths, treating `.json` files as `CommonMark` spec
/// files (one case per example) and everything else as markdown.
fn collect_mixed(paths: &[String]) -> Result<Vec<Case>> {
    let mut cases = Vec::new();
    for p in paths {
        if std::path::Path::new(p).extension().is_some_and(|e| e.eq_ignore_ascii_case("json")) {
            let raw = std::fs::read_to_string(p).with_context(|| format!("reading {p}"))?;
            let examples: Vec<Value> = serde_json::from_str(&raw).context("parsing spec.json")?;
            for ex in &examples {
                cases.push(Case {
                    name: format!(
                        "example {} ({})",
                        ex["example"].as_i64().unwrap_or(0),
                        ex["section"].as_str().unwrap_or("?")
                    ),
                    markdown: ex["markdown"]
                        .as_str()
                        .context("spec example without markdown")?
                        .to_owned(),
                });
            }
        } else {
            collect_markdown_files(Path::new(p), &mut cases)?;
        }
    }
    Ok(cases)
}

/// Measure the markdown *writer* by fidelity: write the AST back to
/// markdown, have pandoc read the result, and require the document that
/// comes back to be the one we started from.
///
/// This deliberately does **not** compare against pandoc's own markdown
/// output. `CommonMark` is a lossy target and pandoc's writer loses
/// documents ferrodoc keeps (escaped punctuation, autolinks containing
/// backslashes); demanding sameness would mean reproducing those losses.
/// Round-tripping is the property a user actually depends on, so it is
/// the gate — and pandoc's score on the identical corpus is printed
/// beside it so the comparison stays honest in both directions.
fn diff_md(paths: &[String], verbose: bool, fail_under: Option<f64>) -> Result<()> {
    let cases = collect_mixed(paths)?;
    if cases.is_empty() {
        bail!("no inputs found");
    }
    let mut matched = 0usize;
    let mut pandoc_matched = 0usize;
    let mut failures = Vec::new();
    for case in &cases {
        let ast = ferrodoc_markdown::read_commonmark(&case.markdown)
            .map_err(|e| anyhow::anyhow!("{e}"))?;
        let original = serde_json::to_value(&ast)?;
        let ast_json = serde_json::to_string(&ast)?;

        let ours_md = ferrodoc_markdown::write_markdown(&ast);
        let ours = pandoc_json(&ours_md)
            .with_context(|| format!("pandoc could not read our markdown for {}", case.name))?;

        // `--wrap=preserve`, not `--wrap=none`: `none` collapses every
        // soft break into a space, and scoring pandoc on a setting that
        // throws away line structure would flatter ferrodoc for free.
        let theirs_md = run_pandoc_input(
            &ast_json,
            &["-f", "json", "-t", "commonmark", "--wrap=preserve"],
        )?;
        let theirs_md = String::from_utf8(theirs_md).context("pandoc emitted invalid UTF-8")?;
        if pandoc_json(&theirs_md)? == original {
            pandoc_matched += 1;
        }

        if ours == original {
            matched += 1;
        } else {
            failures.push((case, first_divergence(&ours, &original, "")));
        }
    }
    #[allow(clippy::cast_precision_loss)]
    let pandoc_pct = 100.0 * pandoc_matched as f64 / cases.len() as f64;
    println!(
        "pandoc round-trips {pandoc_matched}/{} of the same corpus ({pandoc_pct:.1}%)",
        cases.len()
    );
    report(&cases, matched, &failures, verbose, fail_under)
}

/// Run pandoc with the given arguments over stdin text.
fn run_pandoc_input(input: &str, args: &[&str]) -> Result<Vec<u8>> {
    run_pandoc(input, args)
}

/// Compare our DOCX *writer* against pandoc's, semantically: both engines
/// write the same AST to a `.docx`, pandoc reads both back, and the two
/// resulting documents must be identical. Comparing the zip bytes would be
/// meaningless; comparing what the format actually preserves is the real
/// contract.
fn diff_write(paths: &[String], verbose: bool, fail_under: Option<f64>) -> Result<()> {
    let cases = collect_mixed(paths)?;
    if cases.is_empty() {
        bail!("no inputs found");
    }
    let dir = std::env::temp_dir().join("ferrodoc-diff-write");
    std::fs::create_dir_all(&dir)?;
    let mut matched = 0usize;
    let mut failures = Vec::new();
    for case in &cases {
        let ast = ferrodoc_markdown::read_commonmark(&case.markdown).map_err(|e| anyhow::anyhow!("{e}"))?;
        let ast_json = serde_json::to_string(&ast)?;

        // Ours: ferrodoc writes the docx, pandoc reads it back.
        let ours_docx = ferrodoc_docx::write_docx(&ast)
            .map_err(|e| anyhow::anyhow!("{e}"))
            .with_context(|| format!("ferrodoc failed to write {}", case.name))?;
        let ours_path = dir.join("ours.docx");
        std::fs::write(&ours_path, &ours_docx)?;
        let ours = pandoc_file(&ours_path)
            .with_context(|| format!("pandoc could not read our docx for {}", case.name))?;

        // Theirs: pandoc writes the docx and reads it back.
        let theirs_path = dir.join("theirs.docx");
        let status = Command::new("pandoc")
            .args(["-f", "json", "-t", "docx", "-o"])
            .arg(&theirs_path)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .and_then(|mut child| {
                child
                    .stdin
                    .take()
                    .expect("stdin was piped")
                    .write_all(ast_json.as_bytes())?;
                child.wait_with_output()
            })?;
        if !status.status.success() {
            bail!(
                "pandoc failed to write {}: {}",
                case.name,
                String::from_utf8_lossy(&status.stderr)
            );
        }
        let theirs = pandoc_file(&theirs_path)?;

        if ours == theirs {
            matched += 1;
        } else {
            failures.push((case, first_divergence(&ours, &theirs, "")));
        }
    }
    report(&cases, matched, &failures, verbose, fail_under)
}

/// Read a file with `pandoc -f docx -t json`.
fn pandoc_file(path: &Path) -> Result<Value> {
    let output = Command::new("pandoc")
        .args(["-f", "docx", "-t", "json"])
        .arg(path)
        .output()
        .context("running pandoc")?;
    if !output.status.success() {
        bail!(
            "pandoc failed on {}: {}",
            path.display(),
            String::from_utf8_lossy(&output.stderr)
        );
    }
    Ok(serde_json::from_slice(&output.stdout)?)
}

/// Compare our DOCX reader against `pandoc -f docx -t json` per file.
fn diff_docx(paths: &[String], verbose: bool, fail_under: Option<f64>) -> Result<()> {
    let mut files = Vec::new();
    for p in paths {
        collect_files_with_ext(Path::new(p), "docx", &mut files)?;
    }
    if files.is_empty() {
        bail!("no .docx inputs found");
    }
    let mut matched = 0usize;
    let mut failures = Vec::new();
    let cases: Vec<Case> = files
        .iter()
        .map(|f| Case { name: f.display().to_string(), markdown: String::new() })
        .collect();
    for (file, case) in files.iter().zip(&cases) {
        let bytes = std::fs::read(file).with_context(|| format!("reading {}", file.display()))?;
        let ours = serde_json::to_value(
            ferrodoc_docx::read_docx(&bytes)
                .with_context(|| format!("ferrodoc failed on {}", file.display()))?,
        )?;
        let output = Command::new("pandoc")
            .args(["-f", "docx", "-t", "json"])
            .arg(file)
            .output()
            .context("running pandoc")?;
        if !output.status.success() {
            bail!(
                "pandoc failed on {}: {}",
                file.display(),
                String::from_utf8_lossy(&output.stderr)
            );
        }
        let theirs: Value = serde_json::from_slice(&output.stdout)?;
        if ours == theirs {
            matched += 1;
        } else {
            failures.push((case, first_divergence(&ours, &theirs, "")));
        }
    }
    report(&cases, matched, &failures, verbose, fail_under)
}

/// Recursively collect files with the given extension.
fn collect_files_with_ext(path: &Path, ext: &str, out: &mut Vec<PathBuf>) -> Result<()> {
    if path.is_dir() {
        let mut entries: Vec<PathBuf> = std::fs::read_dir(path)
            .with_context(|| format!("reading {}", path.display()))?
            .map(|e| e.map(|e| e.path()))
            .collect::<std::io::Result<_>>()?;
        entries.sort();
        for entry in entries {
            if entry.is_dir() || entry.extension().is_some_and(|e| e == ext) {
                collect_files_with_ext(&entry, ext, out)?;
            }
        }
    } else {
        out.push(path.to_owned());
    }
    Ok(())
}

/// Compare our HTML writer against `pandoc -t html` per case.
fn diff_html(paths: &[String], verbose: bool, fail_under: Option<f64>) -> Result<()> {
    let cases = collect_mixed(paths)?;
    if cases.is_empty() {
        bail!("no inputs found");
    }
    let mut matched = 0usize;
    let mut failures = Vec::new();
    for case in &cases {
        let ours = ferrodoc_html::write_html(&ferrodoc_markdown::read_commonmark(&case.markdown).map_err(|e| anyhow::anyhow!("{e}"))?);
        let theirs = run_pandoc(&case.markdown, &["-f", "commonmark", "-t", "html", "--syntax-highlighting=none", "--wrap=none"])
            .with_context(|| format!("pandoc failed on {}", case.name))?;
        let theirs = String::from_utf8(theirs).context("pandoc emitted invalid UTF-8")?;
        if ours == theirs {
            matched += 1;
        } else {
            failures.push((case, first_line_divergence(&ours, &theirs)));
        }
    }
    report(&cases, matched, &failures, verbose, fail_under)
}

/// First differing line of two texts, with both sides shown.
fn first_line_divergence(ours: &str, theirs: &str) -> String {
    for (i, (a, b)) in ours.lines().zip(theirs.lines()).enumerate() {
        if a != b {
            return format!("line {}: ours={a:?} theirs={b:?}", i + 1);
        }
    }
    format!(
        "line counts differ ({} vs {}); ours ends {:?}, theirs ends {:?}",
        ours.lines().count(),
        theirs.lines().count(),
        char_safe_tail(ours, 60),
        char_safe_tail(theirs, 60),
    )
}

/// The last `n` bytes of `s`, moved forward to a UTF-8 char boundary.
fn char_safe_tail(s: &str, n: usize) -> &str {
    let mut start = s.len().saturating_sub(n);
    while !s.is_char_boundary(start) {
        start += 1;
    }
    &s[start..]
}

/// Benchmark the in-process pipeline against the pandoc subprocess.
fn bench(paths: &[String], iters: u32) -> Result<()> {
    if paths.is_empty() {
        bail!("bench expects at least one markdown file");
    }
    for p in paths {
        let markdown = std::fs::read_to_string(p).with_context(|| format!("reading {p}"))?;
        let bytes = markdown.len();

        // Warm up, then time ferrodoc in-process (parse + write HTML).
        let mut sink = 0usize;
        sink += ferrodoc_html::write_html(&ferrodoc_markdown::read_commonmark(&markdown).map_err(|e| anyhow::anyhow!("{e}"))?).len();
        let start = std::time::Instant::now();
        for _ in 0..iters {
            sink += ferrodoc_html::write_html(&ferrodoc_markdown::read_commonmark(&markdown).map_err(|e| anyhow::anyhow!("{e}"))?).len();
        }
        let ours = start.elapsed() / iters;

        // Time the pandoc subprocess (what a pipeline shelling out pays).
        let pandoc_iters = iters.clamp(3, 10);
        let start = std::time::Instant::now();
        for _ in 0..pandoc_iters {
            sink += run_pandoc(&markdown, &["-f", "commonmark", "-t", "html", "--syntax-highlighting=none", "--wrap=none"])?.len();
        }
        let theirs = start.elapsed() / pandoc_iters;

        #[allow(clippy::cast_precision_loss)]
        let speedup = theirs.as_secs_f64() / ours.as_secs_f64();
        println!(
            "{p} ({bytes} bytes): ferrodoc {ours:?}/doc vs pandoc subprocess {theirs:?}/doc — {speedup:.1}x (sink {sink})"
        );

    }
    Ok(())
}

/// Time the DOCX writer and reader. Kept apart from `bench` so that the
/// markdown numbers there stay comparable between builds: adding code to a
/// benchmark changes inlining and code layout, which moves its timings even
/// when the measured library has not changed at all.
fn bench_docx(paths: &[String], iters: u32) -> Result<()> {
    if paths.is_empty() {
        bail!("bench-docx expects at least one markdown file");
    }
    for path in paths {
        let markdown =
            std::fs::read_to_string(path).with_context(|| format!("reading {path}"))?;
        let ast = ferrodoc_markdown::read_commonmark(&markdown).map_err(|e| anyhow::anyhow!("{e}"))?;
        let docx = ferrodoc_docx::write_docx(&ast).map_err(|e| anyhow::anyhow!("{e}"))?;
        let mut sink = docx.len();

        let start = std::time::Instant::now();
        for _ in 0..iters {
            sink += ferrodoc_docx::write_docx(&ast)
                .map_err(|e| anyhow::anyhow!("{e}"))?
                .len();
        }
        let write = start.elapsed() / iters;

        let start = std::time::Instant::now();
        for _ in 0..iters {
            sink += ferrodoc_docx::read_docx(&docx)
                .map_err(|e| anyhow::anyhow!("{e}"))?
                .blocks
                .len();
        }
        let read = start.elapsed() / iters;

        println!(
            "{path}: docx write {write:?}/doc, read {read:?}/doc ({} docx bytes, sink {sink})",
            docx.len()
        );
    }
    Ok(())
}

/// Run `pandoc -f commonmark -t json` on the given markdown.
fn pandoc_json(markdown: &str) -> Result<Value> {
    let out = run_pandoc(markdown, &["-f", "commonmark", "-t", "json"])?;
    Ok(serde_json::from_slice(&out)?)
}

/// Run pandoc with the given arguments, feeding markdown on stdin.
fn run_pandoc(markdown: &str, args: &[&str]) -> Result<Vec<u8>> {
    let mut child = Command::new("pandoc")
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .context("spawning pandoc (is it on PATH?)")?;
    // Write stdin from a separate thread so a large document cannot
    // deadlock against an already-full stdout pipe.
    let mut stdin = child.stdin.take().expect("stdin was piped");
    let bytes = markdown.as_bytes().to_vec();
    let writer = std::thread::spawn(move || stdin.write_all(&bytes));
    let output = child.wait_with_output()?;
    writer
        .join()
        .expect("stdin writer thread does not panic")
        .context("writing to pandoc stdin")?;
    if !output.status.success() {
        bail!("pandoc exited with {}: {}", output.status, String::from_utf8_lossy(&output.stderr));
    }
    Ok(output.stdout)
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
            format!(
                "{path} (array lengths {} vs {}): ours={} theirs={}",
                a.len(),
                b.len(),
                truncate(&Value::Array(a.clone())),
                truncate(&Value::Array(b.clone())),
            )
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
