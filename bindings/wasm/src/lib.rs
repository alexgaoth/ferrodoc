//! WebAssembly bindings for ferrodoc, hand-written.
//!
//! There is no `wasm-bindgen` here, and that is deliberate: the whole API
//! is bytes in and bytes out, the generated glue would be larger than the
//! glue it replaces, and the size of this module is a published claim.
//!
//! # Why there is no `unsafe` block
//!
//! The usual shape of a hand-written wasm boundary hands JavaScript a raw
//! pointer and rebuilds a slice from it with `slice::from_raw_parts`,
//! which is `unsafe` and easy to get wrong. This module never does that.
//!
//! Instead every buffer stays owned by Rust in a **handle table**.
//! JavaScript is told a handle and, separately, the address to write into;
//! it writes bytes there through its own `Uint8Array` view of the linear
//! memory, and Rust afterwards reads *its own* `Vec<u8>` by handle. Bytes
//! are `u8`, which has no invalid bit pattern, so a buffer whose contents
//! changed underneath is still a perfectly valid `Vec<u8>`.
//!
//! The only concession is `#[unsafe(no_mangle)]` on the exports, which is
//! an attribute rather than a block. `no_unsafe_blocks_in_this_crate`
//! checks the distinction is still true.

use std::cell::RefCell;
use std::collections::HashMap;

/// A buffer owned by Rust and addressed by JavaScript.
struct Buffer {
    bytes: Vec<u8>,
    /// Whether a conversion produced this, or an error message did. The
    /// JavaScript side turns the second into a thrown `ConversionError`.
    ok: bool,
}

thread_local! {
    /// Every live buffer. `thread_local` rather than a `Mutex`: wasm32
    /// without threads has one, and a lock here would be a lie about
    /// what is protecting what.
    static BUFFERS: RefCell<HashMap<u32, Buffer>> = RefCell::new(HashMap::new());
    /// The next handle. Handles are never reused within a session, so a
    /// double free is a no-op rather than a buffer someone else now owns.
    static NEXT: RefCell<u32> = const { RefCell::new(1) };
}

fn insert(bytes: Vec<u8>, ok: bool) -> u32 {
    let handle = NEXT.with(|n| {
        let mut n = n.borrow_mut();
        let handle = *n;
        *n = n.wrapping_add(1).max(1);
        handle
    });
    BUFFERS.with(|b| b.borrow_mut().insert(handle, Buffer { bytes, ok }));
    handle
}

fn with_bytes<T>(handle: u32, f: impl FnOnce(&Buffer) -> T) -> Option<T> {
    BUFFERS.with(|b| b.borrow().get(&handle).map(f))
}

/// Reserve `len` bytes for JavaScript to fill, returning a handle.
///
/// The buffer is zeroed, which is what makes writing into it from the
/// other side sound: there is no uninitialized memory to observe.
#[unsafe(no_mangle)]
pub extern "C" fn ferrodoc_alloc(len: u32) -> u32 {
    insert(vec![0u8; len as usize], true)
}

/// The address of a handle's bytes in linear memory, for JavaScript to
/// build a `Uint8Array` over. Zero if the handle is unknown.
#[unsafe(no_mangle)]
pub extern "C" fn ferrodoc_address(handle: u32) -> u32 {
    with_bytes(handle, |b| b.bytes.as_ptr() as u32).unwrap_or(0)
}

/// How many bytes a handle holds. Zero if the handle is unknown.
#[unsafe(no_mangle)]
pub extern "C" fn ferrodoc_length(handle: u32) -> u32 {
    with_bytes(handle, |b| u32::try_from(b.bytes.len()).unwrap_or(u32::MAX)).unwrap_or(0)
}

/// Whether a handle holds a converted document (1) or an error message
/// (0). An unknown handle is an error.
#[unsafe(no_mangle)]
pub extern "C" fn ferrodoc_ok(handle: u32) -> u32 {
    u32::from(with_bytes(handle, |b| b.ok).unwrap_or(false))
}

/// Release a handle. Releasing one twice, or one that never existed, does
/// nothing rather than corrupting anything.
#[unsafe(no_mangle)]
pub extern "C" fn ferrodoc_free(handle: u32) {
    BUFFERS.with(|b| b.borrow_mut().remove(&handle));
}

/// Convert `input` from one named format to another.
///
/// All three arguments are handles: the document, and the two format names
/// as UTF-8. The result is a new handle, holding either the converted
/// document or an error message — [`ferrodoc_ok`] says which. The caller
/// frees all four.
///
/// Every failure arrives this way rather than as a trap: a panicking wasm
/// instance is poisoned, and every later call against it fails too, so a
/// single bad document would take the page's converter down with it.
#[unsafe(no_mangle)]
pub extern "C" fn ferrodoc_convert(input: u32, from: u32, to: u32) -> u32 {
    let Some(document) = with_bytes(input, |b| b.bytes.clone()) else {
        return insert(b"no such input handle".to_vec(), false);
    };
    let name = |handle| {
        with_bytes(handle, |b| String::from_utf8(b.bytes.clone()).ok())
            .flatten()
    };
    let (Some(from_name), Some(to_name)) = (name(from), name(to)) else {
        return insert(b"format names must be UTF-8".to_vec(), false);
    };
    let parse = |name: &str, role: &str| {
        ferrodoc::Format::parse(name).ok_or_else(|| {
            format!(
                "unknown {role} format {name:?}; known formats: {}",
                ferrodoc::Format::NAMES.join(", ")
            )
        })
    };
    let result = parse(&from_name, "input")
        .and_then(|from| parse(&to_name, "output").map(|to| (from, to)))
        .and_then(|(from, to)| {
            ferrodoc::convert(&document, from, to).map_err(|e| e.to_string())
        });
    match result {
        Ok(bytes) => insert(bytes, true),
        Err(message) => insert(message.into_bytes(), false),
    }
}

/// Whether a format's bytes are text a caller would rather have as a
/// string. Mirrors the Python binding's rule, so the two agree on what
/// `docx` returns.
#[unsafe(no_mangle)]
pub extern "C" fn ferrodoc_is_text(name: u32) -> u32 {
    let text = with_bytes(name, |b| {
        String::from_utf8(b.bytes.clone())
            .ok()
            .and_then(|n| ferrodoc::Format::parse(&n))
            .is_some_and(|f| !matches!(f, ferrodoc::Format::Docx | ferrodoc::Format::Odt))
    });
    u32::from(text.unwrap_or(false))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn handle_of(bytes: &[u8]) -> u32 {
        let handle = ferrodoc_alloc(u32::try_from(bytes.len()).expect("small"));
        // The test stands in for JavaScript, which writes through its own
        // view of linear memory; here the buffer is filled directly.
        BUFFERS.with(|b| {
            b.borrow_mut()
                .get_mut(&handle)
                .expect("just allocated")
                .bytes
                .copy_from_slice(bytes);
        });
        handle
    }

    fn text_of(handle: u32) -> String {
        with_bytes(handle, |b| String::from_utf8_lossy(&b.bytes).into_owned())
            .expect("live handle")
    }

    #[test]
    fn a_document_converts_and_the_handles_free() {
        let input = handle_of(b"# Title\n");
        let from = handle_of(b"markdown");
        let to = handle_of(b"html");
        let out = ferrodoc_convert(input, from, to);
        assert_eq!(ferrodoc_ok(out), 1);
        assert_eq!(text_of(out), "<h1>Title</h1>\n");
        for handle in [input, from, to, out] {
            ferrodoc_free(handle);
            assert_eq!(ferrodoc_length(handle), 0, "handle {handle} outlived its free");
        }
    }

    #[test]
    fn a_bad_document_is_an_error_handle_not_a_panic() {
        // The whole reason for the ok flag: a wasm instance that traps is
        // poisoned, so one corrupt document would break every later call.
        let input = handle_of(b"this is not a zip archive");
        let from = handle_of(b"docx");
        let to = handle_of(b"gfm");
        let out = ferrodoc_convert(input, from, to);
        assert_eq!(ferrodoc_ok(out), 0);
        assert!(text_of(out).contains("docx"), "{}", text_of(out));

        // And the instance still works afterwards.
        let good = handle_of(b"hi");
        let md = handle_of(b"markdown");
        let html = handle_of(b"html");
        assert_eq!(ferrodoc_ok(ferrodoc_convert(good, md, html)), 1);
    }

    #[test]
    fn an_unknown_format_names_the_ones_that_exist() {
        let input = handle_of(b"x");
        let from = handle_of(b"markdown");
        let to = handle_of(b"pdf");
        let out = ferrodoc_convert(input, from, to);
        assert_eq!(ferrodoc_ok(out), 0);
        let message = text_of(out);
        assert!(message.contains("pdf"), "{message}");
        assert!(message.contains("docx"), "{message}");
    }

    #[test]
    fn an_unknown_handle_is_refused_rather_than_guessed() {
        let out = ferrodoc_convert(9999, 9999, 9999);
        assert_eq!(ferrodoc_ok(out), 0);
        assert_eq!(ferrodoc_address(9999), 0);
        assert_eq!(ferrodoc_length(9999), 0);
        // Freeing something that never existed is a no-op, not a crash.
        ferrodoc_free(9999);
    }

    #[test]
    fn binary_output_is_flagged_binary() {
        for (name, text) in [("html", 1), ("gfm", 1), ("json", 1), ("docx", 0), ("odt", 0)] {
            assert_eq!(ferrodoc_is_text(handle_of(name.as_bytes())), text, "{name}");
        }
    }

    #[test]
    fn no_unsafe_blocks_in_this_crate() {
        // The crate allows `unsafe_code` only for `#[unsafe(no_mangle)]`
        // on its exports. An `unsafe {}` block would mean the handle table
        // stopped being what keeps this boundary sound.
        // Assembled rather than written out, so this line does not match
        // itself — and comments are skipped, because the documentation
        // above discusses the very thing being searched for.
        let needle = concat!("unsafe", " {");
        let offenders: Vec<&str> = include_str!("lib.rs")
            .lines()
            .filter(|line| !line.trim_start().starts_with("//"))
            .filter(|line| line.contains(needle))
            .collect();
        assert!(offenders.is_empty(), "unsafe block in the wasm binding: {offenders:?}");
    }
}
