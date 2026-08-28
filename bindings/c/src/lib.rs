//! The C ABI for ferrodoc.
//!
//! One entry point, mirroring the Python and JavaScript bindings: bytes
//! and two format names in, a result out. Everything the CLI can do is a
//! pair of format names, and the pandoc AST is reachable through the
//! `json` format rather than through a second API.
//!
//! This exists so that Go, Java, C#, Ruby and Julia can link ferrodoc
//! rather than spawn it — one ABI unlocks all of them at once, which is
//! the highest multiplier left after the wheel and the npm package.
//!
//! # The two rules this file lives by
//!
//! **Every `unsafe` block is one dereference wide**, immediately preceded
//! by the invariant the *caller* has to have met. A C ABI cannot avoid
//! `unsafe` — the pointers come from another language — but it can keep
//! each use small enough to check by eye. A block spanning logic is a
//! block nobody audits.
//!
//! **No panic may cross the boundary.** Unwinding into C is undefined
//! behaviour, not an error, so every entry point catches. A conversion
//! that fails returns a result carrying the message; a conversion that
//! *panics* — which would be a bug here — returns one too, rather than
//! taking the host process down.

use std::ffi::{CStr, CString, c_char, c_int};
use std::panic::{AssertUnwindSafe, catch_unwind};

/// The outcome of a conversion: either the converted document or a
/// message saying why not.
///
/// Opaque to C. The bytes stay owned by this library until
/// [`ferrodoc_result_free`] is called, which is what makes
/// [`ferrodoc_result_data`] safe to read.
pub struct FerrodocResult {
    bytes: Vec<u8>,
    ok: bool,
}

/// Convert a document.
///
/// `data`/`len` are the input; `from` and `to` are NUL-terminated format
/// names. The result is never null and must be released with
/// [`ferrodoc_result_free`].
///
/// # Safety
///
/// `data` must point to `len` readable bytes, and `from` and `to` must be
/// NUL-terminated strings. `len` may be zero, in which case `data` is not
/// read and may be null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ferrodoc_convert(
    data: *const u8,
    len: usize,
    from: *const c_char,
    to: *const c_char,
) -> *mut FerrodocResult {
    let result = catch_unwind(AssertUnwindSafe(|| {
        let input: &[u8] = if len == 0 || data.is_null() {
            &[]
        } else {
            // SAFETY: the caller guarantees `len` readable bytes at
            // `data`; the slice does not outlive this call.
            unsafe { std::slice::from_raw_parts(data, len) }
        };
        let Some(from) = (unsafe { name(from) }) else {
            return failure("the input format name is not valid UTF-8");
        };
        let Some(to) = (unsafe { name(to) }) else {
            return failure("the output format name is not valid UTF-8");
        };
        let parse = |name: &str, role: &str| {
            ferrodoc::Format::parse(name).ok_or_else(|| {
                format!(
                    "unknown {role} format {name:?}; known formats: {}",
                    ferrodoc::Format::NAMES.join(", ")
                )
            })
        };
        match parse(&from, "input")
            .and_then(|from| parse(&to, "output").map(|to| (from, to)))
            .and_then(|(from, to)| {
                ferrodoc::convert(input, from, to).map_err(|e| e.to_string())
            }) {
            Ok(bytes) => FerrodocResult { bytes, ok: true },
            Err(message) => failure(&message),
        }
    }));
    // A panic here would be a bug in ferrodoc, and unwinding into C is
    // undefined behaviour rather than a crash you can debug. Reporting it
    // as a failed conversion keeps the host process alive.
    let result = result.unwrap_or_else(|_| failure("ferrodoc panicked; this is a bug"));
    Box::into_raw(Box::new(result))
}

/// Whether a result holds a converted document (1) or a message (0).
///
/// # Safety
///
/// `result` must come from [`ferrodoc_convert`] and not yet be freed. A
/// null pointer answers 0.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ferrodoc_result_ok(result: *const FerrodocResult) -> c_int {
    // SAFETY: the caller guarantees a live result or null.
    match unsafe { result.as_ref() } {
        Some(result) => c_int::from(result.ok),
        None => 0,
    }
}

/// The result's bytes. Valid until the result is freed, and never null
/// for a live result — an empty document answers a valid pointer and a
/// length of zero.
///
/// # Safety
///
/// `result` must come from [`ferrodoc_convert`] and not yet be freed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ferrodoc_result_data(result: *const FerrodocResult) -> *const u8 {
    // SAFETY: the caller guarantees a live result or null.
    match unsafe { result.as_ref() } {
        Some(result) => result.bytes.as_ptr(),
        None => std::ptr::null(),
    }
}

/// How many bytes the result holds.
///
/// # Safety
///
/// `result` must come from [`ferrodoc_convert`] and not yet be freed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ferrodoc_result_len(result: *const FerrodocResult) -> usize {
    // SAFETY: the caller guarantees a live result or null.
    match unsafe { result.as_ref() } {
        Some(result) => result.bytes.len(),
        None => 0,
    }
}

/// Release a result. Freeing null does nothing; freeing twice is the
/// caller's error, as it is for `free`.
///
/// # Safety
///
/// `result` must come from [`ferrodoc_convert`] and must not be used
/// afterwards.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ferrodoc_result_free(result: *mut FerrodocResult) {
    if result.is_null() {
        return;
    }
    // SAFETY: the caller guarantees the pointer came from
    // `Box::into_raw` in `ferrodoc_convert` and is freed once.
    drop(unsafe { Box::from_raw(result) });
}

/// The library version, as a static NUL-terminated string. Never null,
/// and never needs freeing.
#[unsafe(no_mangle)]
pub extern "C" fn ferrodoc_version() -> *const c_char {
    // A `\0` in the literal, so the pointer is valid for C without an
    // allocation the caller would have to release.
    concat!(env!("CARGO_PKG_VERSION"), "\0").as_ptr().cast()
}

/// Every format name the library accepts, comma-separated. Never null,
/// and never needs freeing.
#[unsafe(no_mangle)]
pub extern "C" fn ferrodoc_formats() -> *const c_char {
    use std::sync::OnceLock;
    static NAMES: OnceLock<CString> = OnceLock::new();
    NAMES
        .get_or_init(|| {
            CString::new(ferrodoc::Format::NAMES.join(","))
                .unwrap_or_else(|_| CString::default())
        })
        .as_ptr()
}

fn failure(message: &str) -> FerrodocResult {
    FerrodocResult { bytes: message.as_bytes().to_vec(), ok: false }
}

/// A NUL-terminated C string as a Rust one.
///
/// # Safety
///
/// `text` must be null or point to a NUL-terminated string.
unsafe fn name(text: *const c_char) -> Option<String> {
    if text.is_null() {
        return None;
    }
    // SAFETY: the caller guarantees a NUL terminator; the borrow ends
    // before this function returns.
    unsafe { CStr::from_ptr(text) }.to_str().ok().map(str::to_owned)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn convert(input: &[u8], from: &str, to: &str) -> (bool, Vec<u8>) {
        let from = CString::new(from).expect("no interior nul");
        let to = CString::new(to).expect("no interior nul");
        // SAFETY: the slice and the two strings are live for the call.
        let result = unsafe {
            ferrodoc_convert(input.as_ptr(), input.len(), from.as_ptr(), to.as_ptr())
        };
        // SAFETY: `result` is what the call just returned.
        let ok = unsafe { ferrodoc_result_ok(result) } == 1;
        // SAFETY: one dereference each, on the live result above.
        let data = unsafe { ferrodoc_result_data(result) };
        let len = unsafe { ferrodoc_result_len(result) };
        // SAFETY: `data` points to `len` bytes the result still owns.
        let bytes = unsafe { std::slice::from_raw_parts(data, len) }.to_vec();
        unsafe { ferrodoc_result_free(result) };
        (ok, bytes)
    }

    #[test]
    fn a_document_converts() {
        let (ok, bytes) = convert(b"# T\n", "markdown", "html");
        assert!(ok);
        // Pandoc's dialect, so pandoc's heading identifier — see the
        // same assertion in `bindings/wasm`.
        assert_eq!(String::from_utf8_lossy(&bytes), "<h1 id=\"t\">T</h1>\n");
    }

    #[test]
    fn a_bad_document_is_a_message_not_a_crash() {
        let (ok, bytes) = convert(b"not a zip", "docx", "gfm");
        assert!(!ok);
        assert!(String::from_utf8_lossy(&bytes).contains("docx"));
    }

    #[test]
    fn an_unknown_format_names_the_ones_that_exist() {
        let (ok, bytes) = convert(b"x", "markdown", "pdf");
        assert!(!ok);
        let message = String::from_utf8_lossy(&bytes).into_owned();
        assert!(message.contains("pdf"), "{message}");
        assert!(message.contains("docx"), "{message}");
    }

    #[test]
    fn an_empty_input_is_an_empty_document() {
        // And `data` may be null when `len` is zero, which is what a C
        // caller with an empty buffer will pass.
        let from = CString::new("markdown").expect("no nul");
        let to = CString::new("html").expect("no nul");
        // SAFETY: len is zero, so `data` is never read.
        let result =
            unsafe { ferrodoc_convert(std::ptr::null(), 0, from.as_ptr(), to.as_ptr()) };
        assert_eq!(unsafe { ferrodoc_result_ok(result) }, 1);
        unsafe { ferrodoc_result_free(result) };
    }

    #[test]
    fn null_is_answered_rather_than_dereferenced() {
        // A C caller *will* pass null eventually; every accessor has to
        // survive it.
        assert_eq!(unsafe { ferrodoc_result_ok(std::ptr::null()) }, 0);
        assert!(unsafe { ferrodoc_result_data(std::ptr::null()) }.is_null());
        assert_eq!(unsafe { ferrodoc_result_len(std::ptr::null()) }, 0);
        // Freeing null is a no-op, as it is for `free`.
        unsafe { ferrodoc_result_free(std::ptr::null_mut()) };
    }

    #[test]
    fn a_null_format_name_is_refused() {
        let to = CString::new("html").expect("no nul");
        // SAFETY: `from` is null, which `name` checks for.
        let result = unsafe { ferrodoc_convert(b"x".as_ptr(), 1, std::ptr::null(), to.as_ptr()) };
        assert_eq!(unsafe { ferrodoc_result_ok(result) }, 0);
        unsafe { ferrodoc_result_free(result) };
    }

    #[test]
    fn every_unsafe_block_is_one_line_wide() {
        // The rule this file lives by: a block spanning logic is a block
        // nobody audits. Assembled so the check does not match itself.
        let opener = concat!("unsafe", " {");
        let mut wide = Vec::new();
        let mut lines = include_str!("lib.rs").lines().enumerate().peekable();
        while let Some((number, line)) = lines.next() {
            let code = line.trim_start();
            if code.starts_with("//") || !code.contains(opener) {
                continue;
            }
            // A block that closes on its own line is one expression wide.
            // Anything longer than three lines is doing work inside.
            if !line.contains('}') {
                let mut depth = 1;
                let mut span = 1;
                while depth > 0 && span < 4 {
                    let Some((_, next)) = lines.next() else { break };
                    depth += next.matches('{').count();
                    depth -= next.matches('}').count();
                    span += 1;
                }
                if depth > 0 {
                    wide.push(number + 1);
                }
            }
        }
        assert!(wide.is_empty(), "unsafe blocks spanning logic at lines {wide:?}");
    }
}
