//! Differential test harness: compares ferrodoc's AST (and HTML output)
//! against pandoc over a corpus or the `CommonMark` spec, and benchmarks
//! the in-process pipeline against the pandoc subprocess.
//!
//! Usage:
//!   ferrodoc-harness diff-ast  [--verbose] [--fail-under PCT] <file-or-dir>...
//!   ferrodoc-harness diff-spec [--verbose] [--fail-under PCT] <spec.json>
//!   ferrodoc-harness diff-html [--verbose] [--fail-under PCT] <file-dir-or-spec.json>...
//!   ferrodoc-harness diff-odt   [--verbose] [--fail-under PCT] <file-or-dir>...
//!   ferrodoc-harness diff-write [--verbose] [--fail-under PCT] <file-dir-or-spec.json>...
//!   ferrodoc-harness diff-md [--verbose] [--fail-under PCT] <file-dir-or-spec.json>...
//!   ferrodoc-harness diff-gfm [--verbose] [--fail-under PCT] <dir-or-spec.json>...
//!   ferrodoc-harness diff-gfm-md [--verbose] [--fail-under PCT] <dir-or-spec.json>...
//!   ferrodoc-harness diff-html-read [--verbose] [--fail-under PCT] <file-dir-or-spec.json>...
//!   ferrodoc-harness fuzz [--iters N] [--seed N] <file-or-dir>...
//!   ferrodoc-harness bench [--iters N] <file>...
//!   ferrodoc-harness bench-docx [--iters N] <file>...
//!   ferrodoc-harness bench-rss [--max-rss-ratio N] <file>...

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
    let max_rss_ratio = take_option(&mut args, "--max-rss-ratio")?
        .map(|v| v.parse::<f64>().context("--max-rss-ratio expects a number"))
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
        Some("diff-odt") => diff_odt(&args[1..], verbose, fail_under),
        Some("diff-epub") => diff_epub(&args[1..], verbose, fail_under),
        Some("diff-write") => diff_write(&args[1..], verbose, fail_under),
        Some("diff-odt-write") => diff_odt_write(&args[1..], verbose, fail_under),
        Some("diff-md") => diff_md(&args[1..], verbose, fail_under),
        Some("diff-gfm") => diff_gfm(&args[1..], verbose, fail_under),
        Some("diff-gfm-md") => diff_gfm_md(&args[1..], verbose, fail_under),
        Some("diff-html-read") => diff_html_read(&args[1..], verbose, fail_under),
        Some("bench") => bench(&args[1..], iters),
        Some("bench-sizes") => bench_sizes(&args[1..], iters),
        Some("bench-rss") => bench_rss(&args[1..], max_rss_ratio),
        Some("rss-one") => rss_one(&args[1..]),
        Some("fuzz") => fuzz(&args[1..], iters),
        Some("bench-docx") => bench_docx(&args[1..], iters),
        _ => bail!(
            "usage: ferrodoc-harness <diff-ast|diff-spec|diff-html|diff-html-read|diff-docx|diff-odt|diff-epub|diff-write|diff-odt-write|diff-md|diff-gfm|diff-gfm-md|bench|bench-sizes|bench-rss|bench-docx|fuzz> [--verbose] [--fail-under PCT] [--iters N] <paths>"
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

/// Compare our GFM *reader* against pandoc's `-f gfm`.
///
/// Pandoc's `gfm` bundles extensions the GFM specification does not define
/// — emoji, footnotes, alerts, `$math$`, YAML metadata — which this reader
/// deliberately does not implement, so a corpus exercising those is
/// expected to diverge and `COMPATIBILITY.md` says so.
fn diff_gfm(paths: &[String], verbose: bool, fail_under: Option<f64>) -> Result<()> {
    let cases = collect_gfm(paths)?;
    let mut matched = 0usize;
    let mut failures = Vec::new();
    for case in &cases {
        let ours = serde_json::to_value(
            ferrodoc_markdown::read_gfm(&case.markdown).map_err(|e| anyhow::anyhow!("{e}"))?,
        )?;
        let theirs = gfm_json(&case.markdown)
            .with_context(|| format!("pandoc failed on {}", case.name))?;
        if ours == theirs {
            matched += 1;
        } else {
            failures.push((case, first_divergence(&ours, &theirs, "")));
        }
    }
    report(&cases, matched, &failures, verbose, fail_under)
}

/// Measure the GFM *writer* by fidelity, exactly as `diff-md` does for
/// `CommonMark`: write the AST back out, have pandoc read the result, and
/// require the document that comes back to be the one we started from.
/// Pandoc's own `-t gfm` is scored on the same corpus beside it.
fn diff_gfm_md(paths: &[String], verbose: bool, fail_under: Option<f64>) -> Result<()> {
    let cases = collect_gfm(paths)?;
    let mut matched = 0usize;
    let mut pandoc_matched = 0usize;
    let mut failures = Vec::new();
    for case in &cases {
        let ast = ferrodoc_markdown::read_gfm(&case.markdown).map_err(|e| anyhow::anyhow!("{e}"))?;
        let original = serde_json::to_value(&ast)?;
        let ast_json = serde_json::to_string(&ast)?;

        let ours_md = ferrodoc_markdown::write_gfm(&ast);
        let ours = gfm_json(&ours_md)
            .with_context(|| format!("pandoc could not read our gfm for {}", case.name))?;

        let theirs_md =
            run_pandoc_input(&ast_json, &["-f", "json", "-t", "gfm", "--wrap=preserve"])?;
        let theirs_md = String::from_utf8(theirs_md).context("pandoc emitted invalid UTF-8")?;
        if gfm_json(&theirs_md)? == original {
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

/// Collect GFM cases: `.json` files contribute each spec example's
/// markdown, everything else is a `.gfm` file. The extension is not `.md`
/// on purpose — those belong to the `CommonMark` gates, which walk the
/// same corpus directory.
fn collect_gfm(paths: &[String]) -> Result<Vec<Case>> {
    let mut cases = Vec::new();
    for p in paths {
        let path = Path::new(p);
        if path.extension().is_some_and(|e| e.eq_ignore_ascii_case("json")) {
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
            collect_by_extension(path, "gfm", &mut cases)?;
        }
    }
    if cases.is_empty() {
        bail!("no gfm inputs found");
    }
    Ok(cases)
}

/// Every file under `path` whose extension is `extension`.
fn collect_by_extension(path: &Path, extension: &str, cases: &mut Vec<Case>) -> Result<()> {
    if path.is_dir() {
        let mut entries: Vec<PathBuf> = std::fs::read_dir(path)
            .with_context(|| format!("reading {}", path.display()))?
            .map(|e| e.map(|e| e.path()))
            .collect::<std::io::Result<_>>()?;
        entries.sort();
        for entry in entries {
            if entry.is_dir() || entry.extension().is_some_and(|e| e == extension) {
                collect_by_extension(&entry, extension, cases)?;
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

fn gfm_json(markdown: &str) -> Result<Value> {
    let out = run_pandoc(markdown, &["-f", "gfm", "-t", "json"])?;
    Ok(serde_json::from_slice(&out)?)
}

/// Compare our HTML *reader* against pandoc's: both read the same page,
/// and the two documents must be identical.
///
/// The corpus is HTML, so a `.json` argument contributes the spec's `html`
/// field rather than its `markdown` — those are 651 real fragments
/// covering every construct the spec has, which is a far better reader
/// corpus than anything written by hand.
fn diff_html_read(paths: &[String], verbose: bool, fail_under: Option<f64>) -> Result<()> {
    let cases = collect_html(paths)?;
    if cases.is_empty() {
        bail!("no html inputs found");
    }
    let mut matched = 0usize;
    let mut failures = Vec::new();
    for case in &cases {
        // A refusal is one case's mismatch, not a reason to abandon the
        // run: a corpus is allowed to contain input the reader rejects.
        let ours = match ferrodoc_html::read_html(&case.markdown) {
            Ok(doc) => serde_json::to_value(&doc)?,
            Err(e) => {
                failures.push((case, format!("refused: {e}")));
                continue;
            }
        };
        let theirs = run_pandoc(&case.markdown, &["-f", "html", "-t", "json"])
            .with_context(|| format!("pandoc failed on {}", case.name))?;
        let theirs: Value = serde_json::from_slice(&theirs)?;
        if ours == theirs {
            matched += 1;
        } else {
            failures.push((case, first_divergence(&ours, &theirs, "")));
        }
    }
    report(&cases, matched, &failures, verbose, fail_under)
}

/// Collect HTML cases: `.json` files contribute each spec example's
/// expected HTML, everything else is read as an HTML file.
fn collect_html(paths: &[String]) -> Result<Vec<Case>> {
    let mut cases = Vec::new();
    for p in paths {
        let path = Path::new(p);
        if path.extension().is_some_and(|e| e.eq_ignore_ascii_case("json")) {
            let raw = std::fs::read_to_string(p).with_context(|| format!("reading {p}"))?;
            let examples: Vec<Value> = serde_json::from_str(&raw).context("parsing spec.json")?;
            for ex in &examples {
                let Some(html) = ex["html"].as_str().filter(|h| !h.is_empty()) else {
                    continue;
                };
                cases.push(Case {
                    name: format!(
                        "example {} ({})",
                        ex["example"].as_i64().unwrap_or(0),
                        ex["section"].as_str().unwrap_or("?")
                    ),
                    markdown: html.to_owned(),
                });
            }
        } else {
            collect_html_files(path, &mut cases)?;
        }
    }
    Ok(cases)
}

fn collect_html_files(path: &Path, cases: &mut Vec<Case>) -> Result<()> {
    if path.is_dir() {
        let mut entries: Vec<PathBuf> = std::fs::read_dir(path)
            .with_context(|| format!("reading {}", path.display()))?
            .map(|e| e.map(|e| e.path()))
            .collect::<std::io::Result<_>>()?;
        entries.sort();
        for entry in entries {
            // A `src/` directory holds the inputs another corpus's
            // generator converts — `corpus/docx-libreoffice/src` is HTML
            // that becomes `.docx`. Walking into it would score those as
            // HTML-reader fixtures, so this gate's scope would widen
            // every time someone added a DOCX case, and drop for a reason
            // that has nothing to do with the HTML reader.
            if entry.file_name().is_some_and(|name| name == "src") {
                continue;
            }
            if entry.is_dir() || entry.extension().is_some_and(|e| e == "html") {
                collect_html_files(&entry, cases)?;
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

/// Mutate real documents and require every reader to survive.
///
/// Not a coverage-guided fuzzer — that needs nightly and a toolchain a
/// contributor may not have. This is the part that pays: take inputs that
/// are already interesting, corrupt them, and insist the readers return
/// `Err` rather than panic, overflow or hang. Several of the worst bugs in
/// this project were found exactly this way, by hand; this runs it in CI.
fn fuzz(paths: &[String], iters: u32) -> Result<()> {
    let seeds = fuzz_seeds(paths)?;
    if seeds.is_empty() {
        bail!("no seed documents found");
    }
    let seed = std::env::var("FERRODOC_FUZZ_SEED")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(0x5eed_1234_9876_abcd);
    let start = std::time::Instant::now();
    let outcomes = fuzz_run(&seeds, iters, seed);
    println!(
        "{iters} mutations of {} seeds in {:.1?}: {} read, {} refused — no panics, no hangs",
        seeds.len(),
        start.elapsed(),
        outcomes.0,
        outcomes.1
    );
    Ok(())
}

/// Every corpus document, as bytes, whatever its format.
fn fuzz_seeds(paths: &[String]) -> Result<Vec<Vec<u8>>> {
    let mut seeds = Vec::new();
    for p in paths {
        collect_bytes(Path::new(p), &mut seeds)?;
    }
    Ok(seeds)
}

fn collect_bytes(path: &Path, out: &mut Vec<Vec<u8>>) -> Result<()> {
    if path.is_dir() {
        let mut entries: Vec<PathBuf> = std::fs::read_dir(path)
            .with_context(|| format!("reading {}", path.display()))?
            .map(|e| e.map(|e| e.path()))
            .collect::<std::io::Result<_>>()?;
        entries.sort();
        for entry in entries {
            collect_bytes(&entry, out)?;
        }
    } else if path
        .extension()
        .is_some_and(|e| matches!(e.to_str(), Some("md" | "gfm" | "html" | "docx" | "odt")))
    {
        out.push(std::fs::read(path).with_context(|| format!("reading {}", path.display()))?);
    }
    Ok(())
}

/// Run the mutation loop, returning (documents read, documents refused).
///
/// A panic or an abort ends the process, which is the whole signal: every
/// reader here promises to return `Err` on input it cannot handle.
#[expect(
    clippy::cast_possible_truncation,
    reason = "indices are taken modulo a length that is already a usize"
)]
fn fuzz_run(seeds: &[Vec<u8>], iters: u32, seed: u64) -> (u32, u32) {
    let mut rng = seed | 1;
    let mut next = move || {
        // xorshift64*: no dependency, and reproducible from the seed.
        rng ^= rng >> 12;
        rng ^= rng << 25;
        rng ^= rng >> 27;
        rng.wrapping_mul(0x2545_f491_4f6c_dd1d)
    };
    let (mut ok, mut refused) = (0, 0);
    let mut input = Vec::new();
    for _ in 0..iters {
        input.clear();
        input.extend_from_slice(&seeds[(next() % seeds.len() as u64) as usize]);
        mutate(&mut input, &mut next);

        // Bytes first: the DOCX reader takes a zip, and a corrupted zip is
        // its own family of hostile input.
        if ferrodoc_docx::read_docx(&input).is_ok() {
            ok += 1;
        } else {
            refused += 1;
        }
        // The media path decompresses parts the plain read never touches.
        if ferrodoc_docx::read_docx_with_media(&input).is_ok() {
            ok += 1;
        } else {
            refused += 1;
        }
        // ODT is a zip of XML parts too, and its own reader.
        if ferrodoc_odt::read_odt(&input).is_ok() {
            ok += 1;
        } else {
            refused += 1;
        }
        if ferrodoc_odt::read_odt_with_media(&input).is_ok() {
            ok += 1;
        } else {
            refused += 1;
        }
        // Then as text, for the readers that take one.
        if let Ok(text) = std::str::from_utf8(&input) {
            if ferrodoc_markdown::read_commonmark(text).is_ok() {
                ok += 1;
            } else {
                refused += 1;
            }
            if ferrodoc_markdown::read_gfm(text).is_ok() {
                ok += 1;
            } else {
                refused += 1;
            }
            if ferrodoc_html::read_html(text).is_ok() {
                ok += 1;
            } else {
                refused += 1;
            }
        }
    }
    (ok, refused)
}

/// Corrupt a document in one of the ways that has actually found a bug
/// here: flip bytes, truncate, repeat a region, or splice in nesting.
#[expect(
    clippy::cast_possible_truncation,
    reason = "offsets are taken modulo the input length, itself a usize"
)]
fn mutate(input: &mut Vec<u8>, next: &mut impl FnMut() -> u64) {
    let rounds = 1 + next() % 8;
    for _ in 0..rounds {
        if input.is_empty() {
            input.push(b'x');
        }
        let len = input.len() as u64;
        match next() % 6 {
            // Flip a byte.
            0 => {
                let at = (next() % len) as usize;
                input[at] ^= 1 << (next() % 8);
            }
            // Truncate: unterminated everything.
            1 => input.truncate((next() % len) as usize),
            // Repeat a slice, which is how nesting gets deep.
            2 => {
                let at = (next() % len) as usize;
                let take = ((next() % 64) as usize).min(input.len() - at);
                let piece = input[at..at + take].to_vec();
                let times = 1 + next() % 200;
                for _ in 0..times {
                    input.extend_from_slice(&piece);
                }
            }
            // Splice in a structural token.
            3 => {
                let tokens: [&[u8]; 8] = [
                    b"<div>", b"<pre>", b"<table><tr><td>", b"</p>", b"- ", b"> ", b"```",
                    b"<w:p>",
                ];
                let at = (next() % len) as usize;
                let token = tokens[(next() % tokens.len() as u64) as usize];
                input.splice(at..at, token.iter().copied());
            }
            // Delete a run.
            4 => {
                let at = (next() % len) as usize;
                let take = ((next() % 128) as usize).min(input.len() - at);
                input.drain(at..at + take);
            }
            // Insert a byte that often ends a token.
            _ => {
                let at = (next() % len) as usize;
                let byte = [b'<', b'>', b'&', b'"', 0, 0xff, b'\n', b'\t'][(next() % 8) as usize];
                input.insert(at, byte);
            }
        }
        // Keep the loop quick; a mutation that runs away helps nobody.
        input.truncate(4 << 20);
    }
}

/// Report latency and throughput for each conversion path, in absolute
/// terms, at every document size given.
///
/// Absolute numbers are what a user planning a pipeline needs; the ratios
/// elsewhere are for comparing builds. They are not interchangeable — this
/// machine drifts within a session, so treat these as an order of
/// magnitude, not a benchmark score. Peak memory is measured from outside,
/// with `/usr/bin/time -v` on the CLI: an in-process figure would need a
/// custom global allocator, and `unsafe` is forbidden across this
/// workspace for a better reason than a benchmark.
fn bench_sizes(paths: &[String], iters: u32) -> Result<()> {
    if paths.is_empty() {
        bail!("bench-sizes expects at least one markdown file");
    }
    println!("{:>10}  {:<18}  {:>11}  {:>9}", "input", "path", "latency", "MB/s");
    for p in paths {
        let markdown = std::fs::read_to_string(p).with_context(|| format!("reading {p}"))?;
        let bytes = markdown.len();
        let ast = ferrodoc_markdown::read_commonmark(&markdown).map_err(|e| anyhow::anyhow!("{e}"))?;
        let html = ferrodoc_html::write_html(&ast);
        let docx = ferrodoc_docx::write_docx(&ast).map_err(|e| anyhow::anyhow!("{e}"))?;

        // Every read runs once for real before anything is timed. The
        // loops below discard the result, and the fastest way to "read" a
        // document is to refuse it: a fixture whose HTML nests past the
        // reader's depth bound was published for months as 4.12 s of
        // throughput, when it was 4.12 s of rejecting the input.
        ferrodoc_html::read_html(&html)
            .map_err(|e| anyhow::anyhow!("{p}: the HTML this writes cannot be read back: {e}"))?;
        ferrodoc_docx::read_docx(&docx)
            .map_err(|e| anyhow::anyhow!("{p}: the docx this writes cannot be read back: {e}"))?;

        // Big documents are slow enough that a fixed iteration count would
        // take minutes; scale it down but never below one.
        let scaled = (iters as usize).saturating_mul(100_000) / bytes.max(1);
        let runs = u32::try_from(scaled.clamp(1, iters as usize)).unwrap_or(1);
        let label = human(bytes);
        measure(&label, "markdown -> AST", bytes, runs, || {
            let _ = ferrodoc_markdown::read_commonmark(&markdown);
        });
        measure(&label, "AST -> HTML", bytes, runs, || {
            let _ = ferrodoc_html::write_html(&ast);
        });
        measure(&label, "AST -> markdown", bytes, runs, || {
            let _ = ferrodoc_markdown::write_markdown(&ast);
        });
        measure(&label, "AST -> docx", bytes, runs, || {
            let _ = ferrodoc_docx::write_docx(&ast);
        });
        measure(&label, "docx -> AST", bytes, runs, || {
            let _ = ferrodoc_docx::read_docx(&docx);
        });
        measure(&label, "HTML -> AST", bytes, runs, || {
            let _ = ferrodoc_html::read_html(&html);
        });
    }
    Ok(())
}


/// The conversion paths `bench-rss` measures, by the name it takes on the
/// command line. Kept beside `bench_sizes`' list on purpose: a path that is
/// timed but never weighed is how `docx -> markdown` reached 1.12 GB
/// unnoticed.
const RSS_PATHS: &[&str] = &[
    "markdown-to-ast",
    "markdown-to-html",
    "markdown-to-docx",
    "markdown-to-odt",
    "docx-to-markdown",
    "odt-to-markdown",
    "html-to-ast",
];

/// Peak resident memory of each conversion path, as a multiple of the input.
///
/// Every measurement runs in a **child process**, because peak RSS is a
/// high-water mark the kernel never lowers: measuring two paths in one
/// process reports the larger for both. The child converts once and prints
/// its own `VmHWM`.
///
/// The ratio is only meaningful once the document dominates the process, so
/// inputs below [`RSS_FLOOR`] are measured and printed but not gated — at
/// 10 KB the interpreter and the binary itself are most of the resident set
/// and every path would "exceed" any sane bound.
fn bench_rss(paths: &[String], max_ratio: Option<f64>) -> Result<()> {
    if paths.is_empty() {
        bail!("bench-rss expects at least one markdown file");
    }
    let exe = std::env::current_exe().context("finding this executable")?;
    println!("{:>10}  {:<18}  {:>10}  {:>8}  gated", "input", "path", "peak RSS", "ratio");
    let mut worst: Option<(String, String, f64)> = None;
    for p in paths {
        let bytes = std::fs::metadata(p).with_context(|| format!("reading {p}"))?.len();
        let label = human(usize::try_from(bytes).unwrap_or(usize::MAX));
        for path in RSS_PATHS {
            let output = Command::new(&exe)
                .args(["rss-one", path])
                .arg(p)
                .output()
                .context("running the measuring child")?;
            if !output.status.success() {
                bail!(
                    "{path} on {p}: {}",
                    String::from_utf8_lossy(&output.stderr).trim()
                );
            }
            let peak: u64 = String::from_utf8_lossy(&output.stdout)
                .trim()
                .parse()
                .with_context(|| format!("{path} reported no peak RSS"))?;
            #[expect(clippy::cast_precision_loss, reason = "reporting, not arithmetic")]
            let ratio = peak as f64 / bytes.max(1) as f64;
            let gated = bytes >= RSS_FLOOR;
            println!(
                "{label:>10}  {path:<18}  {:>10}  {ratio:>7.1}x  {}",
                human(usize::try_from(peak).unwrap_or(usize::MAX)),
                if gated { "yes" } else { "no (too small)" }
            );
            if gated && worst.as_ref().is_none_or(|(_, _, w)| ratio > *w) {
                worst = Some(((*path).to_owned(), p.clone(), ratio));
            }
        }
    }
    if let (Some(limit), Some((path, file, ratio))) = (max_ratio, worst) {
        if ratio > limit {
            bail!(
                "{path} on {file} peaked at {ratio:.1}x its input, above the {limit:.1}x bound"
            );
        }
        println!("worst gated path: {path} at {ratio:.1}x, within the {limit:.1}x bound");
    }
    Ok(())
}

/// Below this an input does not dominate the process and the ratio measures
/// the binary rather than the conversion.
const RSS_FLOOR: u64 = 1 << 20;

/// One conversion, in this process, reporting this process's peak RSS.
///
/// The whole point of the child: `VmHWM` never falls, so each measurement
/// needs a process of its own.
fn rss_one(args: &[String]) -> Result<()> {
    let [path, file] = args else {
        bail!("rss-one expects a path name and a file");
    };
    let markdown = std::fs::read_to_string(file).with_context(|| format!("reading {file}"))?;
    let ast = || ferrodoc_markdown::read_commonmark(&markdown).map_err(|e| anyhow::anyhow!("{e}"));
    // Each arm ends holding only what that path produces, so the peak is
    // the path's own cost and not the setup's.
    match path.as_str() {
        "markdown-to-ast" => {
            let _ = ast()?;
        }
        "markdown-to-html" => {
            let _ = ferrodoc_html::write_html(&ast()?);
        }
        "markdown-to-docx" => {
            let _ = ferrodoc_docx::write_docx(&ast()?).map_err(|e| anyhow::anyhow!("{e}"))?;
        }
        "markdown-to-odt" => {
            let _ = ferrodoc_odt::write_odt(&ast()?).map_err(|e| anyhow::anyhow!("{e}"))?;
        }
        "docx-to-markdown" => {
            let docx = ferrodoc_docx::write_docx(&ast()?).map_err(|e| anyhow::anyhow!("{e}"))?;
            let back = ferrodoc_docx::read_docx(&docx).map_err(|e| anyhow::anyhow!("{e}"))?;
            let _ = ferrodoc_markdown::write_markdown(&back);
        }
        "odt-to-markdown" => {
            let odt = ferrodoc_odt::write_odt(&ast()?).map_err(|e| anyhow::anyhow!("{e}"))?;
            let back = ferrodoc_odt::read_odt(&odt).map_err(|e| anyhow::anyhow!("{e}"))?;
            let _ = ferrodoc_markdown::write_markdown(&back);
        }
        "html-to-ast" => {
            let html = ferrodoc_html::write_html(&ast()?);
            let _ = ferrodoc_html::read_html(&html).map_err(|e| anyhow::anyhow!("{e}"))?;
        }
        other => bail!("unknown path {other}; known: {}", RSS_PATHS.join(", ")),
    }
    println!("{}", peak_rss()?);
    Ok(())
}

/// This process's high-water resident set, in bytes.
///
/// Linux only: `VmHWM` is what makes a peak measurable at all without a
/// sampling thread that would itself perturb the number. Elsewhere the gate
/// reports that it cannot measure rather than guessing.
fn peak_rss() -> Result<u64> {
    let status = std::fs::read_to_string("/proc/self/status")
        .context("peak RSS needs /proc/self/status, which this platform has not got")?;
    let line = status
        .lines()
        .find(|l| l.starts_with("VmHWM:"))
        .context("no VmHWM in /proc/self/status")?;
    let kb: u64 = line
        .split_whitespace()
        .nth(1)
        .and_then(|v| v.parse().ok())
        .context("VmHWM was not a number")?;
    Ok(kb * 1024)
}

/// Time `body` over `runs` iterations and report the per-run cost.
fn measure(label: &str, path: &str, bytes: usize, runs: u32, mut body: impl FnMut()) {
    body(); // warm up
    let start = std::time::Instant::now();
    for _ in 0..runs {
        body();
    }
    let each = start.elapsed() / runs;
    #[expect(clippy::cast_precision_loss, reason = "reporting, not arithmetic")]
    let throughput = bytes as f64 / each.as_secs_f64() / 1_048_576.0;
    println!(
        "{label:>10}  {path:<18}  {:>11}  {throughput:>9.0}",
        format!("{each:.2?}")
    );
}

fn human(bytes: usize) -> String {
    #[expect(clippy::cast_precision_loss, reason = "reporting, not arithmetic")]
    let n = bytes as f64;
    if bytes >= 1 << 20 {
        format!("{:.1} MB", n / 1_048_576.0)
    } else if bytes >= 1 << 10 {
        format!("{:.0} KB", n / 1024.0)
    } else {
        format!("{bytes} B")
    }
}

/// Run pandoc with the given arguments over stdin text.
fn run_pandoc_input(input: &str, args: &[&str]) -> Result<Vec<u8>> {
    run_pandoc(input, args)
}

/// Rewrite the names of embedded media parts to their order of first
/// appearance.
///
/// Reading a `.docx` back reports each image's URL as the package path it
/// was stored at, and the two writers number those parts differently —
/// ferrodoc counts images, pandoc reuses relationship ids. That is zip
/// layout, not document content, and `diff-write` exists to compare
/// content. Distinctness and the file extension are kept, so two images
/// sharing one part, or an image landing in the wrong format, still fail.
fn normalize_media(value: &mut Value, seen: &mut Vec<String>) {
    match value {
        Value::String(text) if text.starts_with("media/") => {
            let extension = text.rsplit_once('.').map_or(String::new(), |(_, e)| format!(".{e}"));
            let index = seen.iter().position(|s| s == text).unwrap_or_else(|| {
                seen.push(text.clone());
                seen.len() - 1
            });
            *text = format!("media/{index}{extension}");
        }
        Value::Array(items) => {
            for item in items {
                normalize_media(item, seen);
            }
        }
        Value::Object(fields) => {
            for (_, field) in fields {
                normalize_media(field, seen);
            }
        }
        _ => {}
    }
}

/// Compare our DOCX *writer* against pandoc's, semantically: both engines
/// write the same AST to a `.docx`, pandoc reads both back, and the two
/// resulting documents must be identical. Comparing the zip bytes would be
/// meaningless; comparing what the format actually preserves is the real
/// contract.
fn diff_write(paths: &[String], verbose: bool, fail_under: Option<f64>) -> Result<()> {
    diff_round_trip(paths, "docx", &|ast, base| {
        ferrodoc_docx::write_docx_with_media(ast, &|url| std::fs::read(base.join(url)).ok())
            .map_err(|e| anyhow::anyhow!("{e}"))
    }, verbose, fail_under)
}

fn diff_odt_write(paths: &[String], verbose: bool, fail_under: Option<f64>) -> Result<()> {
    diff_round_trip(paths, "odt", &|ast, base| {
        ferrodoc_odt::write_odt_with_media(ast, &|url| std::fs::read(base.join(url)).ok())
            .map_err(|e| anyhow::anyhow!("{e}"))
    }, verbose, fail_under)
}

/// Score a binary writer by round trip: ours through pandoc's reader
/// against pandoc's own output through the same reader.
///
/// Comparing the two *readbacks* rather than either against the source AST
/// is what isolates the writer. Both office formats lose things on the way
/// through — a code block has no ODT spelling pandoc's reader knows — and a
/// loss both writers share is the format's, not ours.
fn diff_round_trip(
    paths: &[String],
    format: &str,
    write: &dyn Fn(&ferrodoc_ast::Pandoc, &Path) -> Result<Vec<u8>>,
    verbose: bool,
    fail_under: Option<f64>,
) -> Result<()> {
    let cases = collect_mixed(paths)?;
    if cases.is_empty() {
        bail!("no inputs found");
    }
    let dir = std::env::temp_dir().join(format!("ferrodoc-diff-{format}-write"));
    std::fs::create_dir_all(&dir)?;
    let mut matched = 0usize;
    let mut failures = Vec::new();
    for case in &cases {
        let ast = ferrodoc_markdown::read_commonmark(&case.markdown).map_err(|e| anyhow::anyhow!("{e}"))?;
        let ast_json = serde_json::to_string(&ast)?;

        // Ours: ferrodoc writes the file, pandoc reads it back. Images
        // resolve against the case's own directory, which is how pandoc
        // resolves them too — otherwise nothing here would embed one.
        let base = Path::new(&case.name).parent().unwrap_or(Path::new(".")).to_owned();
        let ours_bytes = write(&ast, &base)
            .with_context(|| format!("ferrodoc failed to write {}", case.name))?;
        let ours_path = dir.join(format!("ours.{format}"));
        std::fs::write(&ours_path, &ours_bytes)?;
        let ours = pandoc_file(&ours_path, format)
            .with_context(|| format!("pandoc could not read our {format} for {}", case.name))?;

        // Theirs: pandoc writes the file and reads it back.
        let theirs_path = dir.join(format!("theirs.{format}"));
        let status = Command::new("pandoc")
            .args(["-f", "json", "-t", format, "--resource-path"])
            .arg(&base)
            .arg("-o")
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
        let mut theirs = pandoc_file(&theirs_path, format)?;

        let mut ours = ours;
        normalize_media(&mut ours, &mut Vec::new());
        normalize_media(&mut theirs, &mut Vec::new());

        if ours == theirs {
            matched += 1;
        } else {
            failures.push((case, first_divergence(&ours, &theirs, "")));
        }
    }
    report(&cases, matched, &failures, verbose, fail_under)
}

/// Read a file with `pandoc -f <format> -t json`.
fn pandoc_file(path: &Path, format: &str) -> Result<Value> {
    let output = Command::new("pandoc")
        .args(["-f", format, "-t", "json"])
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
    diff_binary(paths, "docx", &|bytes| {
        Ok(serde_json::to_value(ferrodoc_docx::read_docx(bytes)?)?)
    }, verbose, fail_under)
}

fn diff_epub(paths: &[String], verbose: bool, fail_under: Option<f64>) -> Result<()> {
    diff_binary(paths, "epub", &|bytes| {
        Ok(serde_json::to_value(ferrodoc_epub::read_epub(bytes)?)?)
    }, verbose, fail_under)
}

fn diff_odt(paths: &[String], verbose: bool, fail_under: Option<f64>) -> Result<()> {
    diff_binary(paths, "odt", &|bytes| {
        Ok(serde_json::to_value(ferrodoc_odt::read_odt(bytes)?)?)
    }, verbose, fail_under)
}

/// Compare a binary reader against pandoc's, document by document.
///
/// One function for both office formats: the extension is also pandoc's
/// name for the format, so nothing else differs between the two gates.
fn diff_binary(
    paths: &[String],
    format: &str,
    read: &dyn Fn(&[u8]) -> Result<Value>,
    verbose: bool,
    fail_under: Option<f64>,
) -> Result<()> {
    let mut files = Vec::new();
    for p in paths {
        collect_files_with_ext(Path::new(p), format, &mut files)?;
    }
    if files.is_empty() {
        bail!("no .{format} inputs found");
    }
    let mut matched = 0usize;
    let mut failures = Vec::new();
    let cases: Vec<Case> = files
        .iter()
        .map(|f| Case { name: f.display().to_string(), markdown: String::new() })
        .collect();
    for (file, case) in files.iter().zip(&cases) {
        let bytes = std::fs::read(file).with_context(|| format!("reading {}", file.display()))?;
        let ours = read(&bytes).with_context(|| format!("ferrodoc failed on {}", file.display()))?;
        let output = Command::new("pandoc")
            .args(["-f", format, "-t", "json"])
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

#[cfg(test)]
mod tests {
    use super::*;

    /// A short, fixed-seed fuzz run, so the readers' promise — refuse, do
    /// not panic — is checked on every `cargo test` rather than only when
    /// somebody remembers. The long campaigns run from the CLI.
    #[test]
    fn readers_refuse_mutated_documents_without_panicking() {
        // `cargo test` runs with the crate as the working directory.
        let corpus = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../corpus")
            .display()
            .to_string();
        let seeds = fuzz_seeds(&[corpus]).expect("corpus is readable");
        assert!(!seeds.is_empty(), "no seed documents found");
        for seed in [0x5eed_1234_9876_abcd, 1, 0xdead_beef] {
            let (read, refused) = fuzz_run(&seeds, 400, seed);
            assert!(read + refused > 0, "the loop did nothing");
        }
    }
}
