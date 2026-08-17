//! Just enough image inspection to embed a picture in an OOXML package.
//!
//! A media part needs three things the bytes themselves carry: the file
//! extension, the content type to declare for it, and the intrinsic size
//! that becomes the drawing's extent when the document does not say how
//! big the image should be. Formats not listed here are not embedded —
//! the writer falls back to the alt text rather than produce a package
//! Word would reject.
//!
//! Intrinsic size is a pixel count *and* the resolution it is counted at,
//! because half these formats have no pixels at all: an EMF states a
//! physical size, and pandoc reads it as whole screen pixels. Reporting
//! the count alone would put a 300-dpi scan on the page at four times its
//! size.

/// What the writer needs to know about an image it is about to embed.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Image {
    /// The file extension the media part should carry.
    pub extension: &'static str,
    /// The MIME type to declare for it.
    pub content_type: &'static str,
    /// The size the file itself states.
    pub size: Size,
}

/// An image's intrinsic size: a pixel count, and the resolution those
/// pixels are counted at.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Size {
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// Horizontal resolution those pixels are counted at.
    pub dpi_x: u32,
    /// Vertical resolution those pixels are counted at.
    pub dpi_y: u32,
}

/// What a format that states no resolution is counted at. Pandoc's
/// default too, so a plain PNG lands on the page at one pixel per point.
const DEFAULT_DPI: u32 = 72;

/// What a format with no pixels of its own is measured in. A vector image
/// has a physical size; pandoc converts it at this resolution, so an EMF
/// one inch wide is 96 pixels before it is 72 points.
const SCREEN_DPI: u32 = 96;

/// Hundredths of a millimetre to the inch, which is the unit an EMF
/// states its size in.
const MM100_PER_INCH: u32 = 2540;

/// How far into a file an SVG root element may start. A licence comment
/// ahead of the root is ordinary; a megabyte of one is not.
const SVG_HEAD: usize = 65_536;

/// Identify an image and read its intrinsic size, or `None` if the bytes
/// are not a format this writer can embed.
pub fn inspect(bytes: &[u8]) -> Option<Image> {
    let (size, extension, content_type) = if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        (png_size(bytes)?, "png", "image/png")
    } else if bytes.starts_with(b"\xff\xd8") {
        (jpeg_size(bytes)?, "jpeg", "image/jpeg")
    } else if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        (gif_size(bytes)?, "gif", "image/gif")
    } else if bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"WEBP") {
        (webp_size(bytes)?, "webp", "image/webp")
    } else if bytes.starts_with(b"II*\x00") || bytes.starts_with(b"MM\x00*") {
        (tiff_size(bytes)?, "tiff", "image/tiff")
    } else if bytes.starts_with(&[1, 0, 0, 0]) && bytes.get(40..44) == Some(&b" EMF"[..]) {
        (emf_size(bytes)?, "emf", "image/x-emf")
    } else if bytes.starts_with(b"\xd7\xcd\xc6\x9a") {
        (wmf_size(bytes)?, "wmf", "image/x-wmf")
    } else {
        // Last, because it is the one format identified by parsing it
        // rather than by a signature.
        (svg_size(bytes)?, "svg", "image/svg+xml")
    };
    // A zero dimension would give the drawing a zero extent, which Word
    // renders as an invisible picture; a zero resolution has no size at
    // all to compute.
    if size.width == 0 || size.height == 0 || size.dpi_x == 0 || size.dpi_y == 0 {
        return None;
    }
    Some(Image { extension, content_type, size })
}

/// PNG: the IHDR chunk is required to come first, so the size sits at a
/// fixed offset; the optional pHYs chunk states the resolution.
fn png_size(bytes: &[u8]) -> Option<Size> {
    if bytes.get(12..16)? != b"IHDR" {
        return None;
    }
    let (width, height) = (be32(bytes, 16)?, be32(bytes, 20)?);
    let (mut dpi_x, mut dpi_y) = (DEFAULT_DPI, DEFAULT_DPI);
    // Chunks are a length, a four-byte type, that many bytes and a CRC.
    let mut at = 8usize;
    while let (Some(length), Some(kind)) = (
        be32(bytes, at),
        at.checked_add(4).and_then(|at| bytes.get(at..at.checked_add(4)?)),
    ) {
        // pHYs is required to precede the pixel data, so there is no
        // reason to walk an entire image looking for one.
        if kind == b"IDAT" {
            break;
        }
        if kind == b"pHYs" {
            // Unit 1 is metres; any other states an aspect ratio only.
            if bytes.get(at + 16) == Some(&1)
                && let (Some(x), Some(y)) = (be32(bytes, at + 8), be32(bytes, at + 12))
            {
                dpi_x = per_metre(x);
                dpi_y = per_metre(y);
            }
            break;
        }
        // A chunk length that runs off the end of the address space ends
        // the search for a resolution; it does not discard the size IHDR
        // already gave. Failing here instead would refuse on a 32-bit
        // target a file a 64-bit one embeds.
        let next = at
            .checked_add(12)
            .and_then(|at| at.checked_add(usize::try_from(length).ok()?));
        let Some(next) = next else { break };
        at = next;
    }
    Some(Size { width, height, dpi_x, dpi_y })
}

/// Pixels per metre as whole dots per inch, truncated exactly where
/// pandoc truncates: a 300-dpi PNG records 11811 ppm and reads back 299.
fn per_metre(ppm: u32) -> u32 {
    whole_dpi(u64::from(ppm) * 254 / 10_000)
}

/// JPEG: walk the marker segments to the frame header, which is the only
/// place the size is recorded, noting the resolution on the way.
///
/// There are two places that resolution can be, and they do not tie: JFIF
/// in APP0, and Exif in APP1. Exif wins where it says something usable —
/// a camera or a scanner writes one, and often no JFIF density at all —
/// and JFIF answers otherwise. Measured both ways: JFIF 150 beside an
/// Exif segment stating 300 dpi is 300 to pandoc, and beside an Exif
/// segment carrying no resolution tags it is still 150, not the default.
fn jpeg_size(bytes: &[u8]) -> Option<Size> {
    let (mut jfif, mut exif) = (None, None);
    let mut at = 2usize;
    loop {
        // Segments are `0xff`, a marker, then a big-endian length that
        // counts itself. Fill bytes (`0xff` runs) are skipped.
        while bytes.get(at) == Some(&0xff) && bytes.get(at + 1) == Some(&0xff) {
            at += 1;
        }
        if *bytes.get(at)? != 0xff {
            return None;
        }
        let marker = *bytes.get(at + 1)?;
        let length = usize::from(be16(bytes, at + 2)?);
        // Both headers precede the frame, so both are read before the
        // size is returned.
        if marker == 0xe0
            && bytes.get(at + 4..at + 9) == Some(&b"JFIF\x00"[..])
            && let (Some(unit), Some(x), Some(y)) =
                (bytes.get(at + 11), be16(bytes, at + 12), be16(bytes, at + 14))
        {
            jfif = Some((density(*unit, x), density(*unit, y)));
        }
        // Exif holds a TIFF image file directory, resolution tags and
        // all, six bytes into the segment's payload. A segment that
        // states no usable resolution — most of them — leaves JFIF to
        // answer, so nothing is recorded for it here.
        if marker == 0xe1 && bytes.get(at + 4..at + 10) == Some(&b"Exif\x00\x00"[..]) {
            exif = bytes
                .get(at + 10..)
                .and_then(directory)
                .and_then(|ifd| Some((ifd.dpi_x?, ifd.dpi_y?)));
        }
        // SOF0..SOF15, excluding the four markers in that range that are
        // not frame headers, carry the dimensions at a fixed offset.
        if (0xc0..=0xcf).contains(&marker) && !matches!(marker, 0xc4 | 0xc8 | 0xcc) {
            let (dpi_x, dpi_y) = exif.or(jfif).unwrap_or((DEFAULT_DPI, DEFAULT_DPI));
            return Some(Size {
                width: u32::from(be16(bytes, at + 7)?),
                height: u32::from(be16(bytes, at + 5)?),
                dpi_x,
                dpi_y,
            });
        }
        // Entropy-coded data follows the scan header and is not a segment.
        if marker == 0xda {
            return None;
        }
        at = at.checked_add(2)?.checked_add(length)?;
    }
}

/// A JFIF density in whole dots per inch. Unit 1 is already per inch,
/// unit 2 is per centimetre, and unit 0 states an aspect ratio only.
fn density(unit: u8, value: u16) -> u32 {
    match unit {
        1 => whole_dpi(u64::from(value)),
        2 => whole_dpi(u64::from(value) * 254 / 100),
        _ => DEFAULT_DPI,
    }
}

/// GIF: the logical screen descriptor follows the six-byte signature.
/// There is nowhere in the format to record a resolution.
fn gif_size(bytes: &[u8]) -> Option<Size> {
    Some(Size {
        width: u32::from(le16(bytes, 6)?),
        height: u32::from(le16(bytes, 8)?),
        dpi_x: DEFAULT_DPI,
        dpi_y: DEFAULT_DPI,
    })
}

/// WebP: a RIFF container whose first chunk states the size, spelled
/// differently by each of the three codings.
///
/// Pandoc mis-reads the lossless one — a 7x11 image comes back 1x6 — so
/// this diverges there rather than reproduce a size nobody meant.
fn webp_size(bytes: &[u8]) -> Option<Size> {
    let (width, height) = match bytes.get(12..16)? {
        // Lossy: the key frame header, past a three-byte start code.
        // Each dimension is 14 bits; the top two are a scaling hint.
        b"VP8 " => {
            if bytes.get(23..26)? != b"\x9d\x01\x2a" {
                return None;
            }
            (
                u32::from(le16(bytes, 26)? & 0x3fff),
                u32::from(le16(bytes, 28)? & 0x3fff),
            )
        }
        // Lossless: 14 bits each in one little-endian run, each one
        // less than the real size.
        b"VP8L" => {
            if bytes.get(20) != Some(&0x2f) {
                return None;
            }
            let bits = le32(bytes, 21)?;
            ((bits & 0x3fff) + 1, ((bits >> 14) & 0x3fff) + 1)
        }
        // Extended: a 24-bit canvas size, also one less.
        b"VP8X" => (le24(bytes, 24)? + 1, le24(bytes, 27)? + 1),
        _ => return None,
    };
    Some(Size { width, height, dpi_x: SCREEN_DPI, dpi_y: SCREEN_DPI })
}

/// TIFF: the first image file directory holds the size and, usually, the
/// resolution — which for a scan is rarely 72, so reading it is not
/// optional.
fn tiff_size(bytes: &[u8]) -> Option<Size> {
    let directory = directory(bytes)?;
    Some(Size {
        width: directory.width?,
        height: directory.height?,
        dpi_x: directory.dpi_x.unwrap_or(DEFAULT_DPI),
        dpi_y: directory.dpi_y.unwrap_or(DEFAULT_DPI),
    })
}

/// What one image file directory states. A TIFF's names the picture's
/// size; a JPEG's Exif segment holds the same structure in the same two
/// byte orders with the same three resolution tags, and names no size —
/// which is why one function reads both.
struct Directory {
    width: Option<u32>,
    height: Option<u32>,
    /// `None` unless the directory states a resolution *and* the unit it
    /// is in. Pandoc reads one only when all three tags are there, and a
    /// JPEG's Exif has to be able to say "nothing usable here" so that
    /// its JFIF header can answer instead.
    dpi_x: Option<u32>,
    dpi_y: Option<u32>,
}

fn directory(bytes: &[u8]) -> Option<Directory> {
    let big_endian = bytes.starts_with(b"MM\x00*");
    if !big_endian && !bytes.starts_with(b"II*\x00") {
        return None;
    }
    let short = |at| if big_endian { be16(bytes, at) } else { le16(bytes, at) };
    let long = |at| if big_endian { be32(bytes, at) } else { le32(bytes, at) };
    // A value of four bytes or fewer sits in the entry itself; anything
    // longer, a rational among them, is named by offset.
    let scalar = |at: usize, kind: u16| match kind {
        3 => short(at).map(u32::from),
        4 => long(at),
        _ => None,
    };
    let rational = |at: usize| {
        let offset = usize::try_from(long(at)?).ok()?;
        Some((long(offset)?, long(offset.checked_add(4)?)?))
    };

    // Every offset here comes straight out of a `u32` field, so on a
    // 32-bit target — wasm32 is one — the arithmetic has to be checked
    // or a crafted file aborts instead of being refused.
    let directory = usize::try_from(long(4)?).ok()?;
    let (mut width, mut height, mut unit) = (None, None, None);
    let (mut x_res, mut y_res) = (None, None);
    for entry in 0..usize::from(short(directory)?) {
        let entry = entry.checked_mul(12)?.checked_add(directory.checked_add(2)?)?;
        let value = entry.checked_add(8)?;
        let kind = short(entry.checked_add(2)?)?;
        match short(entry)? {
            256 => width = scalar(value, kind),
            257 => height = scalar(value, kind),
            282 => x_res = rational(value),
            283 => y_res = rational(value),
            296 => unit = short(value),
            _ => {}
        }
    }

    // Unit 2 is per inch and 3 per centimetre; unit 1 says the numbers
    // are an aspect ratio, with no absolute size to take from them. A
    // file that states a resolution and no unit at all is read the same
    // way, which is what pandoc does: a 300-dpi TIFF with its
    // ResolutionUnit tag renamed away comes back 72.
    let dpi = |resolution: Option<(u32, u32)>| match (resolution, unit?) {
        (Some((num, den)), 2) if den != 0 => Some(whole_dpi(u64::from(num) / u64::from(den))),
        (Some((num, den)), 3) if den != 0 => {
            Some(whole_dpi(u64::from(num) * 254 / (u64::from(den) * 100)))
        }
        _ => None,
    };
    Some(Directory { width, height, dpi_x: dpi(x_res), dpi_y: dpi(y_res) })
}

/// EMF: the header record's frame is the drawing's physical size, in
/// hundredths of a millimetre — 2540 of them to the inch. The bounds
/// beside it are device pixels of whatever recorded the file, which is
/// not a size anyone can use.
///
/// Pandoc instead quantises the frame onto the pixel grid of the machine
/// that recorded the metafile, taking a resolution from the header's
/// device fields: the same drawing is 4.5 points wide when they say 96
/// dpi and 4.875 when they say 192. That is a property of someone else's
/// monitor, not of the picture, so this places it at the size it states.
fn emf_size(bytes: &[u8]) -> Option<Size> {
    Some(Size {
        width: span(le32i(bytes, 24)?, le32i(bytes, 32)?)?,
        height: span(le32i(bytes, 28)?, le32i(bytes, 36)?)?,
        dpi_x: MM100_PER_INCH,
        dpi_y: MM100_PER_INCH,
    })
}

/// WMF: only the optional placeable header states a size, and it states
/// it in units of its own, however many of them it says go to the inch.
///
/// Pandoc reads none at all, so a WMF gets its 300x200 point default
/// there — a size nobody chose, and one that has nothing to do with the
/// drawing.
fn wmf_size(bytes: &[u8]) -> Option<Size> {
    let units = u32::from(le16(bytes, 14)?);
    Some(Size {
        width: span(i32::from(le16i(bytes, 6)?), i32::from(le16i(bytes, 10)?))?,
        height: span(i32::from(le16i(bytes, 8)?), i32::from(le16i(bytes, 12)?))?,
        dpi_x: units,
        dpi_y: units,
    })
}

/// The distance between two edges, in whatever units they are given in.
fn span(near: i32, far: i32) -> Option<u32> {
    u32::try_from(far.checked_sub(near)?).ok()
}

/// SVG: the root element's own `width` and `height`, falling back to the
/// `viewBox` for either one that is missing or relative.
///
/// Pandoc gives a percentage width a zero extent — an invisible picture —
/// so the fallback is a deliberate divergence, not a guess.
fn svg_size(bytes: &[u8]) -> Option<Size> {
    let tag = svg_start_tag(bytes)?;
    let view_box = view_box(&tag);
    let width = attribute(&tag, "width")
        .and_then(length)
        .or(view_box.map(|(width, _)| width));
    let height = attribute(&tag, "height")
        .and_then(length)
        .or(view_box.map(|(_, height)| height));
    // An SVG that states nothing absolute is still an SVG, and refusing
    // it would lose the picture on the way through a package. Pandoc puts
    // an image it cannot size at 300 by 200 points, so that is the size
    // to give one — it beats the zero extent pandoc computes for a
    // percentage width, which is a picture nobody can see.
    let (Some(width), Some(height)) = (width, height) else {
        return Some(Size {
            width: 300,
            height: 200,
            dpi_x: DEFAULT_DPI,
            dpi_y: DEFAULT_DPI,
        });
    };
    Some(Size { width, height, dpi_x: SCREEN_DPI, dpi_y: SCREEN_DPI })
}

/// The root element's start tag, without its name, or `None` when these
/// bytes are not an SVG document.
///
/// A prologue, a doctype and comments may come first, but the *root* has
/// to be `<svg`: searching the text for one would embed an HTML page that
/// merely contains an icon as an `image/svg+xml` part, and would read the
/// size out of a commented-out element.
fn svg_start_tag(bytes: &[u8]) -> Option<String> {
    let head = String::from_utf8_lossy(bytes.get(..bytes.len().min(SVG_HEAD))?);
    let mut rest = head.trim_start_matches('\u{feff}').trim_start();
    loop {
        rest = if let Some(after) = rest.strip_prefix("<?") {
            after.split_once("?>")?.1
        } else if let Some(after) = rest.strip_prefix("<!--") {
            after.split_once("-->")?.1
        } else if let Some(after) = rest.strip_prefix("<!") {
            // A doctype may carry an internal subset, and every `>` in it
            // is one an editor puts there: Illustrator writes
            // `<!DOCTYPE svg PUBLIC "..." [<!ENTITY ns "...">]>`. Ending
            // the skip at the first `>` loses the root, and the picture.
            skip_doctype(after)?
        } else {
            break;
        }
        .trim_start();
    }
    // The root's *local* name has to be svg — `<svg:svg>` is a root and
    // `<svgfoo>` is a different element.
    let name = rest
        .strip_prefix('<')?
        .split([' ', '\t', '\r', '\n', '>', '/'])
        .next()?;
    if name.rsplit(':').next() != Some("svg") {
        return None;
    }
    let tag = &rest[1 + name.len()..];
    // An attribute value may hold a `>`, so the tag ends at the first one
    // outside quotes.
    let mut quote = None;
    for (at, character) in tag.char_indices() {
        match (quote, character) {
            (None, '"' | '\'') => quote = Some(character),
            (Some(open), character) if character == open => quote = None,
            (None, '>') => return Some(tag[..at].to_owned()),
            _ => {}
        }
    }
    // The root is an svg root whatever else is true, so a tag running off
    // the end of the head is an SVG with no attributes this can read
    // rather than no SVG at all — it gets the same fallback size as one
    // that states nothing. A tag running off the end of the *file* is
    // another matter: that is a truncated file, not a long one.
    (bytes.len() > SVG_HEAD).then(String::new)
}

/// What follows a doctype declaration, given everything after its `<!`.
///
/// The declaration ends at the first `>` that is outside both quotes and
/// the square brackets of an internal subset.
fn skip_doctype(after: &str) -> Option<&str> {
    let (mut quote, mut subset) = (None, false);
    for (at, character) in after.char_indices() {
        match (quote, character) {
            (None, '"' | '\'') => quote = Some(character),
            (Some(open), character) if character == open => quote = None,
            (None, '[') => subset = true,
            (None, ']') => subset = false,
            (None, '>') if !subset => return Some(&after[at + 1..]),
            _ => {}
        }
    }
    None
}

/// The quoted value of `name` in a start tag.
fn attribute<'a>(tag: &'a str, name: &str) -> Option<&'a str> {
    for (at, _) in tag.match_indices(name) {
        // `stroke-width` is not `width`: the name has to stand alone.
        if !tag[..at].ends_with(char::is_whitespace) {
            continue;
        }
        let Some(rest) = tag[at + name.len()..].trim_start().strip_prefix('=') else {
            continue;
        };
        let rest = rest.trim_start();
        let Some(quote) = rest.chars().next().filter(|q| *q == '"' || *q == '\'') else {
            continue;
        };
        return rest[quote.len_utf8()..].split(quote).next();
    }
    None
}

/// The last two numbers of a `viewBox`, which are its width and height.
fn view_box(tag: &str) -> Option<(u32, u32)> {
    let mut numbers = attribute(tag, "viewBox")?
        .split([' ', '\t', '\r', '\n', ','])
        .filter(|part| !part.is_empty())
        .filter_map(|part| part.parse::<f64>().ok());
    let (_, _, width, height) = (
        numbers.next()?,
        numbers.next()?,
        numbers.next()?,
        numbers.next()?,
    );
    Some((pixels(width), pixels(height)))
}

/// A CSS length in whole screen pixels, or `None` when it is relative to
/// something the file alone does not say — a percentage of the viewport,
/// or a font size.
fn length(value: &str) -> Option<u32> {
    let value = value.trim();
    let digits = value.trim_end_matches(|c: char| c.is_ascii_alphabetic() || c == '%');
    let number: f64 = digits.parse().ok()?;
    let per_unit = match &value[digits.len()..] {
        "" | "px" => 1.0,
        "in" => 96.0,
        "cm" => 96.0 / 2.54,
        "mm" => 96.0 / 25.4,
        "pt" => 96.0 / 72.0,
        "pc" => 16.0,
        _ => return None,
    };
    // A length that comes to no pixels at all is no length: it should let
    // the view box answer, the way a missing one does, rather than take
    // the picture down with it. Pandoc writes an invisible drawing here.
    let pixels = pixels(number * per_unit);
    (pixels > 0).then_some(pixels)
}

/// A pixel count, truncated toward zero the way pandoc truncates: an
/// SVG 10 points wide is 13 pixels, not 14.
#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "a negative or absurd length saturates to zero, which inspect refuses"
)]
fn pixels(value: f64) -> u32 {
    value as u32
}

/// A resolution as a whole number, falling back to the default when the
/// file states one no image could have.
fn whole_dpi(dpi: u64) -> u32 {
    u32::try_from(dpi)
        .ok()
        .filter(|dpi| *dpi > 0)
        .unwrap_or(DEFAULT_DPI)
}

/// Every read is bounds-checked *and* overflow-checked: an offset here
/// can come straight out of a `u32` field in the file, and on a 32-bit
/// target `at + 4` would wrap rather than fail.
fn be32(bytes: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_be_bytes(bytes.get(at..at.checked_add(4)?)?.try_into().ok()?))
}

fn be16(bytes: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_be_bytes(bytes.get(at..at.checked_add(2)?)?.try_into().ok()?))
}

fn le32(bytes: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(bytes.get(at..at.checked_add(4)?)?.try_into().ok()?))
}

fn le32i(bytes: &[u8], at: usize) -> Option<i32> {
    Some(i32::from_le_bytes(bytes.get(at..at.checked_add(4)?)?.try_into().ok()?))
}

fn le24(bytes: &[u8], at: usize) -> Option<u32> {
    let bytes = bytes.get(at..at.checked_add(3)?)?;
    Some(u32::from(bytes[0]) | u32::from(bytes[1]) << 8 | u32::from(bytes[2]) << 16)
}

fn le16(bytes: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes(bytes.get(at..at.checked_add(2)?)?.try_into().ok()?))
}

fn le16i(bytes: &[u8], at: usize) -> Option<i16> {
    Some(i16::from_le_bytes(bytes.get(at..at.checked_add(2)?)?.try_into().ok()?))
}

/// One smallest-possible file of every format `inspect` accepts, so the
/// tests that write and read whole packages can cover all of them
/// without a fixture directory.
#[cfg(test)]
pub fn samples() -> Vec<(&'static str, Vec<u8>)> {
    let mut lossy = vec![0, 0, 0, 0x9d, 0x01, 0x2a];
    lossy.extend_from_slice(&7u16.to_le_bytes());
    lossy.extend_from_slice(&11u16.to_le_bytes());
    vec![
        ("png", png(7, 11)),
        ("jpeg", jpeg(1, 300)),
        ("gif", {
            let mut gif = b"GIF89a".to_vec();
            gif.extend_from_slice(&7u16.to_le_bytes());
            gif.extend_from_slice(&11u16.to_le_bytes());
            gif
        }),
        ("webp", webp(*b"VP8 ", &lossy)),
        ("tiff", tiff(false, 300, 2)),
        ("emf", emf([0, 0, 185, 291])),
        ("wmf", wmf([0, 0, 7, 11], 96)),
        ("svg", br#"<svg width="7" height="11"></svg>"#.to_vec()),
    ]
}

#[cfg(test)]
fn png(width: u32, height: u32) -> Vec<u8> {
    let mut png = b"\x89PNG\r\n\x1a\n\x00\x00\x00\x0dIHDR".to_vec();
    png.extend_from_slice(&width.to_be_bytes());
    png.extend_from_slice(&height.to_be_bytes());
    png.extend_from_slice(&[0; 9]); // the rest of IHDR, then its CRC
    png
}

/// APP0 (16 bytes of payload) then SOF0 declaring 5x9.
#[cfg(test)]
fn jpeg(unit: u8, density: u16) -> Vec<u8> {
    let mut jpeg = b"\xff\xd8\xff\xe0\x00\x10JFIF\0\x01\x01".to_vec();
    jpeg.push(unit);
    jpeg.extend_from_slice(&density.to_be_bytes());
    jpeg.extend_from_slice(&density.to_be_bytes());
    jpeg.extend_from_slice(b"\0\0");
    jpeg.extend_from_slice(b"\xff\xc0\x00\x11\x08");
    jpeg.extend_from_slice(&9u16.to_be_bytes());
    jpeg.extend_from_slice(&5u16.to_be_bytes());
    jpeg
}

#[cfg(test)]
fn webp(coding: [u8; 4], payload: &[u8]) -> Vec<u8> {
    let mut webp = b"RIFF\0\0\0\0WEBP".to_vec();
    webp.extend_from_slice(&coding);
    webp.extend_from_slice(&[0; 4]); // the chunk's own length
    webp.extend_from_slice(payload);
    webp
}

/// Entries must be in ascending tag order, which is what a reader is
/// entitled to assume; these are 256, 257, 282, 283, 296.
#[cfg(test)]
fn tiff(big_endian: bool, resolution: u32, unit: u16) -> Vec<u8> {
    let short = |value: u16| -> Vec<u8> {
        if big_endian { value.to_be_bytes().to_vec() } else { value.to_le_bytes().to_vec() }
    };
    let long = |value: u32| -> Vec<u8> {
        if big_endian { value.to_be_bytes().to_vec() } else { value.to_le_bytes().to_vec() }
    };
    let entry = |tag: u16, kind: u16, value: Vec<u8>| {
        let mut out = short(tag);
        out.extend(short(kind));
        out.extend(long(1));
        out.extend(value);
        out
    };
    let mut tiff = if big_endian { b"MM\x00*".to_vec() } else { b"II*\x00".to_vec() };
    tiff.extend(long(8));
    tiff.extend(short(5));
    tiff.extend(entry(256, 3, [short(7), short(0)].concat()));
    tiff.extend(entry(257, 4, long(11)));
    // The rationals do not fit in an entry, so they follow the directory
    // and are named by offset.
    let rationals = 8 + 2 + 5 * 12 + 4;
    tiff.extend(entry(282, 5, long(rationals)));
    tiff.extend(entry(283, 5, long(rationals + 8)));
    tiff.extend(entry(296, 3, [short(unit), short(0)].concat()));
    tiff.extend(long(0)); // no further directory
    for _ in 0..2 {
        tiff.extend(long(resolution));
        tiff.extend(long(1));
    }
    tiff
}

#[cfg(test)]
fn emf(frame: [i32; 4]) -> Vec<u8> {
    let mut emf = 1u32.to_le_bytes().to_vec();
    emf.extend_from_slice(&88u32.to_le_bytes());
    emf.extend_from_slice(&[0; 16]); // rclBounds, which states nothing
    for edge in frame {
        emf.extend_from_slice(&edge.to_le_bytes());
    }
    emf.extend_from_slice(b" EMF");
    emf
}

#[cfg(test)]
fn wmf(bounds: [i16; 4], units: u16) -> Vec<u8> {
    let mut wmf = 0x9AC6_CDD7u32.to_le_bytes().to_vec();
    wmf.extend_from_slice(&0u16.to_le_bytes());
    for edge in bounds {
        wmf.extend_from_slice(&edge.to_le_bytes());
    }
    wmf.extend_from_slice(&units.to_le_bytes());
    wmf
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every number here was read off pandoc 3.8.2.1 converting the same
    /// bytes to `.docx`; the extents it wrote are in the comments.
    fn size(bytes: &[u8]) -> (u32, u32, u32, u32) {
        let image = inspect(bytes).expect("an image this writer can embed");
        let Size { width, height, dpi_x, dpi_y } = image.size;
        (width, height, dpi_x, dpi_y)
    }

    #[test]
    fn png_dimensions_are_read() {
        assert_eq!(size(&png(64, 32)), (64, 32, 72, 72));
        assert_eq!(inspect(&png(64, 32)).expect("a png").extension, "png");
    }

    /// A 300-dpi PNG records 11811 pixels per metre, and pandoc reads
    /// that back as 299 — `<wp:extent cx="21407"/>` for seven pixels,
    /// where 300 would have given 21336.
    #[test]
    fn a_png_states_its_resolution_and_the_truncation_is_pandoc_s() {
        let mut phys = png(7, 11);
        phys.extend_from_slice(&9u32.to_be_bytes());
        phys.extend_from_slice(b"pHYs");
        phys.extend_from_slice(&11811u32.to_be_bytes());
        phys.extend_from_slice(&11811u32.to_be_bytes());
        phys.push(1); // the unit is metres
        assert_eq!(size(&phys), (7, 11, 299, 299));

        // Unit 0 is an aspect ratio, which states no absolute size.
        let mut ratio = phys.clone();
        let last = ratio.len() - 1;
        ratio[last] = 0;
        assert_eq!(size(&ratio), (7, 11, 72, 72));
    }

    #[test]
    fn gif_dimensions_are_read() {
        let mut gif = b"GIF89a".to_vec();
        gif.extend_from_slice(&7u16.to_le_bytes());
        gif.extend_from_slice(&11u16.to_le_bytes());
        assert_eq!(size(&gif), (7, 11, 72, 72));
    }

    #[test]
    fn jpeg_dimensions_are_read_past_earlier_segments() {
        assert_eq!(size(&jpeg(0, 1)), (5, 9, 72, 72));
        assert_eq!(inspect(&jpeg(0, 1)).expect("a jpeg").extension, "jpeg");
    }

    /// Pandoc writes `cx="21336"` for a 300-dpi seven-pixel JPEG, and
    /// `cx="25200"` for 100 dots per centimetre — 254 dpi exactly.
    #[test]
    fn a_jpeg_states_its_resolution_in_inches_or_centimetres() {
        assert_eq!(size(&jpeg(1, 300)).2, 300);
        assert_eq!(size(&jpeg(2, 100)).2, 254);
    }

    /// What an Exif segment says about resolution.
    enum Exif {
        Absent,
        /// A well-formed directory that states no resolution at all,
        /// which is what most Exif segments in the wild are.
        Silent,
        Resolution(u16, u32, u32),
        /// A resolution with no `ResolutionUnit` tag beside it, which
        /// pandoc will not use however plainly it is stated.
        NoUnit(u32),
    }

    /// The same 7x11 JPEG carrying either header, or both.
    fn jpeg_headers(jfif: Option<(u8, u16)>, exif: &Exif) -> Vec<u8> {
        let mut out = b"\xff\xd8".to_vec();
        if let Some((unit, density)) = jfif {
            out.extend_from_slice(b"\xff\xe0\x00\x10JFIF\0\x01\x01");
            out.push(unit);
            out.extend_from_slice(&density.to_be_bytes());
            out.extend_from_slice(&density.to_be_bytes());
            out.extend_from_slice(b"\0\0");
        }
        let entry = |tag: u16, kind: u16, value: &[u8]| {
            let mut out = tag.to_le_bytes().to_vec();
            out.extend_from_slice(&kind.to_le_bytes());
            out.extend_from_slice(&1u32.to_le_bytes());
            out.extend_from_slice(value);
            out
        };
        let mut directory = match exif {
            Exif::Absent => Vec::new(),
            // Orientation, so the directory is well formed and silent.
            Exif::Silent => {
                let mut out = 1u16.to_le_bytes().to_vec();
                out.extend(entry(274, 3, &[1, 0, 0, 0]));
                out
            }
            Exif::Resolution(unit, ..) => {
                // The rationals do not fit in an entry, so they follow
                // the directory and are named by offset.
                let rationals: u32 = 8 + 2 + 3 * 12 + 4;
                let mut out = 3u16.to_le_bytes().to_vec();
                out.extend(entry(282, 5, &rationals.to_le_bytes()));
                out.extend(entry(283, 5, &(rationals + 8).to_le_bytes()));
                out.extend(entry(296, 3, &[unit.to_le_bytes(), [0, 0]].concat()));
                out
            }
            Exif::NoUnit(..) => {
                let rationals: u32 = 8 + 2 + 2 * 12 + 4;
                let mut out = 2u16.to_le_bytes().to_vec();
                out.extend(entry(282, 5, &rationals.to_le_bytes()));
                out.extend(entry(283, 5, &(rationals + 8).to_le_bytes()));
                out
            }
        };
        if !directory.is_empty() {
            directory.extend_from_slice(&0u32.to_le_bytes()); // no further directory
            let rational = match exif {
                Exif::Resolution(_, numerator, denominator) => Some((*numerator, *denominator)),
                Exif::NoUnit(numerator) => Some((*numerator, 1)),
                _ => None,
            };
            if let Some((numerator, denominator)) = rational {
                for _ in 0..2 {
                    directory.extend_from_slice(&numerator.to_le_bytes());
                    directory.extend_from_slice(&denominator.to_le_bytes());
                }
            }
            let mut payload = b"Exif\0\0II*\0".to_vec();
            payload.extend_from_slice(&8u32.to_le_bytes());
            payload.extend_from_slice(&directory);
            out.extend_from_slice(b"\xff\xe1");
            let length = u16::try_from(payload.len() + 2).expect("a short segment");
            out.extend_from_slice(&length.to_be_bytes());
            out.extend_from_slice(&payload);
        }
        out.extend_from_slice(b"\xff\xc0\x00\x11\x08");
        out.extend_from_slice(&11u16.to_be_bytes());
        out.extend_from_slice(&7u16.to_be_bytes());
        out
    }

    /// A camera or a scanner writes Exif, and often no JFIF density at
    /// all, so Exif has to be read — but only where it says something
    /// usable, and JFIF answers otherwise. Both halves measured against
    /// pandoc on real JPEGs (a hand-built one is too minimal for it to
    /// parse at all): `cx` of 21336 for Exif 300, 25200 for 100 dots per
    /// centimetre, and 42672 — JFIF's own 150 — for every case where the
    /// Exif segment states no resolution it can use.
    ///
    /// Getting either half wrong is a four-fold error on exactly the
    /// files that bother to record a resolution.
    #[test]
    fn a_jpeg_prefers_the_exif_resolution_to_the_jfif_one() {
        let dpi = |bytes: &[u8]| size(bytes).2;
        assert_eq!(dpi(&jpeg_headers(Some((1, 150)), &Exif::Absent)), 150);
        assert_eq!(dpi(&jpeg_headers(Some((1, 150)), &Exif::Resolution(2, 300, 1))), 300);
        assert_eq!(dpi(&jpeg_headers(Some((1, 150)), &Exif::Resolution(3, 100, 1))), 254);
        assert_eq!(dpi(&jpeg_headers(None, &Exif::Resolution(2, 300, 1))), 300);
        // The JFIF density loses even when it is the larger of the two.
        assert_eq!(dpi(&jpeg_headers(Some((1, 300)), &Exif::Resolution(2, 72, 1))), 72);
        // An Exif segment that states no resolution, or states one in no
        // unit, leaves JFIF to answer — it does not fall through to the
        // default. Most Exif segments in the wild are exactly this.
        assert_eq!(dpi(&jpeg_headers(Some((1, 150)), &Exif::Silent)), 150);
        assert_eq!(dpi(&jpeg_headers(Some((1, 150)), &Exif::Resolution(1, 300, 1))), 150);
        assert_eq!(dpi(&jpeg_headers(Some((1, 150)), &Exif::NoUnit(300))), 150);
        // A zero denominator makes pandoc exit with "divide by zero" and
        // write no file at all, which is not a result to reproduce.
        assert_eq!(dpi(&jpeg_headers(Some((1, 150)), &Exif::Resolution(2, 300, 0))), 150);
        // With nothing anywhere, the default.
        assert_eq!(dpi(&jpeg_headers(None, &Exif::Silent)), 72);
    }
    #[test]
    fn every_webp_coding_states_its_size_in_its_own_way() {
        let mut lossy = vec![0, 0, 0, 0x9d, 0x01, 0x2a];
        lossy.extend_from_slice(&7u16.to_le_bytes());
        lossy.extend_from_slice(&11u16.to_le_bytes());
        assert_eq!(size(&webp(*b"VP8 ", &lossy)), (7, 11, 96, 96));

        // Lossless packs both, one less than the real size, into 28 bits.
        let mut lossless = vec![0x2f];
        lossless.extend_from_slice(&(6u32 | 10 << 14).to_le_bytes());
        assert_eq!(size(&webp(*b"VP8L", &lossless)), (7, 11, 96, 96));

        // Extended states a 24-bit canvas, also one less, after a flag
        // byte and three reserved.
        let mut extended = vec![0, 0, 0, 0];
        extended.extend_from_slice(&[6, 0, 0, 10, 0, 0]);
        assert_eq!(size(&webp(*b"VP8X", &extended)), (7, 11, 96, 96));
    }

    /// Pandoc reads this 7x11 lossless WebP as 1x6. Following it would
    /// mean writing a picture at a seventh of its width.
    #[test]
    fn the_lossless_webp_pandoc_mis_reads_is_read_correctly() {
        let mut lossless = vec![0x2f];
        lossless.extend_from_slice(&(6u32 | 10 << 14).to_le_bytes());
        let (width, height, ..) = size(&webp(*b"VP8L", &lossless));
        assert_eq!((width, height), (7, 11), "pandoc says (1, 6)");
    }

    #[test]
    fn a_tiff_states_its_size_and_resolution_in_either_byte_order() {
        for big_endian in [false, true] {
            assert_eq!(size(&tiff(big_endian, 72, 2)), (7, 11, 72, 72));
            assert_eq!(size(&tiff(big_endian, 300, 2)), (7, 11, 300, 300));
            assert_eq!(size(&tiff(big_endian, 100, 3)), (7, 11, 254, 254));
            // Unit 1 makes the numbers an aspect ratio, not a size.
            assert_eq!(size(&tiff(big_endian, 300, 1)), (7, 11, 72, 72));
        }
    }
    #[test]
    fn an_emf_is_measured_by_its_frame_not_its_device_bounds() {
        assert_eq!(size(&emf([0, 0, 2540, 5080])), (2540, 5080, 2540, 2540));
        // The bounds say 6x10 device pixels whatever the frame says.
        assert_eq!(size(&emf([0, 0, 185, 291])), (185, 291, 2540, 2540));
        // A frame that starts away from the origin still spans the same.
        assert_eq!(size(&emf([1000, 1000, 3540, 6080])), (2540, 5080, 2540, 2540));
    }

    /// The size a vector image states is exact, and pandoc's is not: it
    /// quantises the frame onto the pixel grid of whatever machine
    /// recorded the file. A 185-by-291 frame is 5.244 by 8.248 points,
    /// and pandoc writes 4.5 by 8.25 — but only while the header claims
    /// a 96-dpi monitor. Told 192 instead, pandoc writes 4.875 points
    /// for the same drawing. That is not a rule this can follow.
    #[test]
    fn a_vector_image_keeps_the_size_it_states() {
        let points = |bytes: &[u8]| {
            let size = inspect(bytes).expect("an image").size;
            (
                f64::from(size.width) * 72.0 / f64::from(size.dpi_x),
                f64::from(size.height) * 72.0 / f64::from(size.dpi_y),
            )
        };
        let (width, height) = points(&emf([0, 0, 185, 291]));
        assert!((width - 5.244).abs() < 0.001, "{width} points, pandoc says 4.5");
        assert!((height - 8.248).abs() < 0.001, "{height} points, pandoc says 8.25");
        // A WMF in twips, which is 20 to the point.
        assert_eq!(points(&wmf([0, 0, 1440, 2880], 1440)), (72.0, 144.0));
    }
    #[test]
    fn a_wmf_is_measured_by_its_placeable_header() {
        assert_eq!(size(&wmf([0, 0, 7, 11], 96)), (7, 11, 96, 96));
        // Units of its own, not pixels: 1440 per inch is twips.
        assert_eq!(size(&wmf([0, 0, 1440, 2880], 1440)), (1440, 2880, 1440, 1440));
        // A header claiming no units to the inch states no size at all.
        assert_eq!(inspect(&wmf([0, 0, 7, 11], 0)), None);
        // Without the header there is no size, so it is not embedded.
        assert_eq!(inspect(b"\x01\x00\x09\x00"), None);
    }

    /// Pandoc writes `cx="123825"` for `10pt`: 13 pixels at 96 dpi, and
    /// `cx="381000"` for a bare `viewBox` of 40 units.
    #[test]
    fn an_svg_is_measured_by_its_attributes_or_its_view_box() {
        let svg = |attributes: &str| {
            format!("<svg xmlns=\"http://www.w3.org/2000/svg\" {attributes}></svg>").into_bytes()
        };
        assert_eq!(size(&svg(r#"width="7" height="11""#)), (7, 11, 96, 96));
        assert_eq!(size(&svg(r#"width="10pt" height="20pt""#)), (13, 26, 96, 96));
        assert_eq!(size(&svg(r#"width="1in" height="2in""#)), (96, 192, 96, 96));
        assert_eq!(size(&svg(r#"viewBox="0 0 40 20""#)), (40, 20, 96, 96));
        // A relative length says nothing absolute, so the view box
        // answers instead. Pandoc writes `cx="0"` here — an invisible
        // picture — which is the one case this deliberately diverges.
        assert_eq!(size(&svg(r#"width="50%" viewBox="0 0 40 20""#)), (40, 20, 96, 96));
        // A prologue may come first, and the size may follow other
        // attributes without them being mistaken for it.
        let prologue = format!(
            "<?xml version=\"1.0\"?>\n{}",
            String::from_utf8(svg(r#"stroke-width="3" width="7" height="11""#)).expect("utf-8")
        );
        assert_eq!(size(prologue.as_bytes()), (7, 11, 96, 96));
        // An attribute value is allowed to hold a `>`.
        assert_eq!(size(&svg(r#"data-x="a>b" width="7" height="11""#)), (7, 11, 96, 96));
        // Nothing absolute anywhere is still an SVG, and losing it would
        // be worse than placing it: 300 by 200 points is where pandoc
        // puts an image it cannot size. Dropping it lost a picture out of
        // a package pandoc itself had written.
        assert_eq!(size(&svg(r#"width="50%" height="30%""#)), (300, 200, 72, 72));
        assert_eq!(size(&svg("")), (300, 200, 72, 72));
        // A length that comes to no pixels is no length either, so the
        // view box answers and the picture survives. Refusing it dropped
        // one pandoc places, invisibly, but places.
        assert_eq!(size(&svg(r#"width="0" viewBox="0 0 40 20""#)), (40, 20, 96, 96));
        assert_eq!(size(&svg(r#"width="0.4" height="0.4""#)), (300, 200, 72, 72));
        // A view box may be separated by commas.
        assert_eq!(size(&svg(r#"viewBox="0,0,40,20""#)), (40, 20, 96, 96));
    }

    /// A root tag longer than the head has still identified the file as an
    /// SVG, so it gets the fallback size rather than being thrown away —
    /// pandoc reads such a file at its stated size. A tag that runs off
    /// the end of the *file* is a different thing: that one is truncated,
    /// and `<svg` on its own is not a picture.
    #[test]
    fn an_svg_whose_root_tag_outruns_the_head_is_still_an_svg() {
        let long = format!(
            r#"<svg data-x="{}" width="7" height="11"></svg>"#,
            "y".repeat(SVG_HEAD)
        );
        assert_eq!(size(long.as_bytes()), (300, 200, 72, 72));
        assert_eq!(inspect(b"<svg"), None);
        assert_eq!(inspect(br#"<svg width="7" height="11""#), None);
    }

    /// The root element decides. Searching the text for `<svg` sized a
    /// picture from a commented-out element, and embedded an HTML page
    /// that merely contains an icon as an `image/svg+xml` part — bytes
    /// that are not the format the package declares them to be.
    #[test]
    fn an_svg_is_recognized_by_its_root_not_by_a_mention_of_one() {
        let commented = br#"<!-- <svg width="999" height="999"> --><svg width="7" height="11"/>"#;
        assert_eq!(size(commented), (7, 11, 96, 96));
        assert_eq!(inspect(br#"<html><body><svg width="9" height="9"/></body></html>"#), None);
        assert_eq!(inspect(br#"<svgfoo width="7" height="11"/>"#), None);
        // A doctype and a processing instruction are allowed to precede
        // the root, in any order and with a comment between them.
        let prologue = br#"<?xml version="1.0"?><!DOCTYPE svg><!-- a --><svg width="7" height="11"/>"#;
        assert_eq!(size(prologue), (7, 11, 96, 96));
        // An unterminated comment has no root after it.
        assert_eq!(inspect(br#"<!-- <svg width="7" height="11"/>"#), None);
        // A doctype's internal subset is full of `>`, and Illustrator
        // writes one. Ending the declaration at the first of them loses
        // the root and the picture with it.
        let illustrator = br#"<?xml version="1.0"?>
<!DOCTYPE svg PUBLIC "-//W3C//DTD SVG 1.1//EN" "svg11.dtd" [<!ENTITY ns_flows "http://ns.adobe.com/Flows/1.0/">]>
<svg width="7" height="11"/>"#;
        assert_eq!(size(illustrator), (7, 11, 96, 96));
        assert_eq!(size(br#"<!DOCTYPE svg SYSTEM "a>b.dtd"><svg width="7" height="11"/>"#), (7, 11, 96, 96));
        // A prefixed root is still an svg root.
        assert_eq!(size(br#"<svg:svg width="7" height="11"/>"#), (7, 11, 96, 96));
    }

    #[test]
    fn truncated_and_unknown_input_is_refused_not_panicked() {
        assert_eq!(inspect(b""), None);
        assert_eq!(inspect(b"\x89PNG\r\n\x1a\n\x00\x00"), None);
        assert_eq!(inspect(b"\xff\xd8\xff\xe0\x00"), None);
        assert_eq!(inspect(b"GIF89a\x00"), None);
        assert_eq!(inspect(b"RIFF\0\0\0\0WEBPVP8 "), None);
        assert_eq!(inspect(b"RIFF\0\0\0\0WEBPNOPE\0\0\0\0"), None);
        assert_eq!(inspect(b"II*\x00\x08\x00\x00\x00"), None);
        assert_eq!(inspect(b"\x01\x00\x00\x00\x58\x00\x00\x00"), None);
        assert_eq!(inspect(b"\xd7\xcd\xc6\x9a\x00\x00"), None);
        assert_eq!(inspect(b"<svg"), None);
        assert_eq!(inspect(b"not an image at all"), None);
        // A frame header that declares a zero dimension.
        let mut zero = b"\xff\xd8\xff\xc0\x00\x11\x08".to_vec();
        zero.extend_from_slice(&0u16.to_be_bytes());
        zero.extend_from_slice(&5u16.to_be_bytes());
        assert_eq!(inspect(&zero), None);
        // A TIFF whose directory is named past the end of the file.
        let mut past = b"II*\x00".to_vec();
        past.extend_from_slice(&u32::MAX.to_le_bytes());
        assert_eq!(inspect(&past), None);
    }

    /// Truncating anywhere inside a file this writer would embed must
    /// leave it refused, never given a picture size from whatever bytes
    /// are left. Resolution is the exception: a TIFF names its own by an
    /// offset past the directory, so a file cut short of it falls back to
    /// the default the way a file that never stated one does.
    #[test]
    fn every_prefix_of_every_format_is_refused_or_sized_the_same() {
        let mut lossy = vec![0, 0, 0, 0x9d, 0x01, 0x2a];
        lossy.extend_from_slice(&7u16.to_le_bytes());
        lossy.extend_from_slice(&11u16.to_le_bytes());
        let whole: Vec<Vec<u8>> = vec![
            png(7, 11),
            jpeg(1, 300),
            webp(*b"VP8 ", &lossy),
            tiff(false, 300, 2),
            emf([0, 0, 2540, 5080]),
            wmf([0, 0, 7, 11], 96),
            b"<svg width=\"7\" height=\"11\"></svg>".to_vec(),
        ];
        let picture = |bytes: &[u8]| inspect(bytes).map(|i| (i.extension, i.size.width, i.size.height));
        for bytes in whole {
            let full = picture(&bytes);
            assert!(full.is_some(), "the whole file should be embeddable");
            for cut in 0..bytes.len() {
                let short = picture(&bytes[..cut]);
                assert!(
                    short.is_none() || short == full,
                    "a file cut at {cut} was sized {short:?}, not {full:?}"
                );
            }
        }
    }
}
