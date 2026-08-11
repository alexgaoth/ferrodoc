//! Print a DOCX file's ferrodoc AST blocks as JSON, for eyeballing
//! divergences next to `pandoc -f docx -t json`.

use anyhow::{Context, Result};

fn main() -> Result<()> {
    let path = std::env::args().nth(1).context("usage: dump_docx <file.docx>")?;
    let bytes = std::fs::read(&path).with_context(|| format!("reading {path}"))?;
    let doc = ferrodoc_docx::read_docx(&bytes).map_err(|e| anyhow::anyhow!("{e}"))?;
    println!("{}", serde_json::to_string(&doc.blocks)?);
    Ok(())
}
