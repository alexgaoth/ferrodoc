//! What an archive is allowed to decompress to.
//!
//! DOCX, ODT and EPUB are all zips of XML, and a zip states each entry's
//! uncompressed size in its own header — so the question "how big does
//! this become" is answerable **before a byte is decompressed**, which is
//! where it has to be answered. Reading first and measuring afterwards is
//! how a 457 KB file becomes 1.28 GB resident.
//!
//! Measured 2026-08-27, `bash corpus/bombs/generate.sh` beside every real
//! archive on hand — ten Project Gutenberg EPUBs, this repository's DOCX
//! and ODT corpora:
//!
//! ```text
//! real archives          ratio up to    16x
//! corpus/bombs/ratio.epub              342x
//! corpus/bombs/ratio.docx              294x
//! ```
//!
//! An order of magnitude apart, which is what makes a limit possible at
//! all. [`MAX_RATIO`] sits at 100 — six times the largest real archive
//! measured and a third of the smaller bomb.
//!
//! **Pandoc does not bound this**, and on the same fixtures it uses three
//! to eleven times more memory than ferrodoc does. There is no oracle to
//! copy here; the reason to have a limit anyway is that pandoc is a
//! process you can kill and this is a library inside somebody's request
//! handler. The same hostile document that inconveniences pandoc takes
//! down the service that linked this.

/// The most an archive may decompress to, as a multiple of its own size.
///
/// Real archives measured up to **16×**; the two fixtures in
/// `corpus/bombs/` are 294× and 342×.
pub const MAX_RATIO: u64 = 100;

/// The smallest budget any archive gets, whatever its size.
///
/// Without it a 10 KB archive would be held to 1 MB, and a small file
/// that legitimately expands a great deal — a short document with a large
/// embedded font — would be refused for no gain. At 64 MB the floor is
/// far above every real document measured and far below both bombs.
pub const MIN_BUDGET: u64 = 64 * 1024 * 1024;

/// Whether `declared` bytes of decompressed content are within what an
/// archive of `archive_bytes` is allowed, and what the budget was.
///
/// Returns `Err(budget)` when it is not, so the caller can name the
/// number it crossed rather than reporting a generic failure.
///
/// # Errors
///
/// When the archive's own headers declare more than the budget allows.
pub fn within_budget(declared: u64, archive_bytes: usize) -> Result<(), u64> {
    let budget = MIN_BUDGET.max(MAX_RATIO.saturating_mul(archive_bytes as u64));
    if declared > budget { Err(budget) } else { Ok(()) }
}
