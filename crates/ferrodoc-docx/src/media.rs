//! Just enough image inspection to embed a picture in an OOXML package.
//!
//! A media part needs three things the bytes themselves carry: the file
//! extension, the content type to declare for it, and the intrinsic pixel
//! size that becomes the drawing's extent when the document does not say
//! how big the image should be. Formats not listed here are not embedded —
//! the writer falls back to the alt text rather than produce a package
//! Word would reject.

/// What the writer needs to know about an image it is about to embed.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct Image {
    pub extension: &'static str,
    pub content_type: &'static str,
    pub width: u32,
    pub height: u32,
}

/// Identify an image and read its pixel dimensions, or `None` if the bytes
/// are not a format this writer can embed.
pub(crate) fn inspect(bytes: &[u8]) -> Option<Image> {
    let (width, height, extension, content_type) = if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        let (w, h) = png_size(bytes)?;
        (w, h, "png", "image/png")
    } else if bytes.starts_with(b"\xff\xd8") {
        let (w, h) = jpeg_size(bytes)?;
        (w, h, "jpeg", "image/jpeg")
    } else if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        let (w, h) = gif_size(bytes)?;
        (w, h, "gif", "image/gif")
    } else {
        return None;
    };
    // A zero dimension would give the drawing a zero extent, which Word
    // renders as an invisible picture.
    if width == 0 || height == 0 {
        return None;
    }
    Some(Image { extension, content_type, width, height })
}

/// PNG: the IHDR chunk is required to come first, so the size sits at a
/// fixed offset.
fn png_size(bytes: &[u8]) -> Option<(u32, u32)> {
    if bytes.get(12..16)? != b"IHDR" {
        return None;
    }
    Some((be32(bytes, 16)?, be32(bytes, 20)?))
}

/// JPEG: walk the marker segments to the frame header, which is the only
/// place the size is recorded.
fn jpeg_size(bytes: &[u8]) -> Option<(u32, u32)> {
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
        // SOF0..SOF15, excluding the four markers in that range that are
        // not frame headers, carry the dimensions at a fixed offset.
        if (0xc0..=0xcf).contains(&marker) && !matches!(marker, 0xc4 | 0xc8 | 0xcc) {
            return Some((
                u32::from(be16(bytes, at + 7)?),
                u32::from(be16(bytes, at + 5)?),
            ));
        }
        // Entropy-coded data follows the scan header and is not a segment.
        if marker == 0xda {
            return None;
        }
        at = at.checked_add(2)?.checked_add(length)?;
    }
}

/// GIF: the logical screen descriptor follows the six-byte signature.
fn gif_size(bytes: &[u8]) -> Option<(u32, u32)> {
    Some((
        u32::from(le16(bytes, 6)?),
        u32::from(le16(bytes, 8)?),
    ))
}

fn be32(bytes: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_be_bytes(bytes.get(at..at + 4)?.try_into().ok()?))
}

fn be16(bytes: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_be_bytes(bytes.get(at..at + 2)?.try_into().ok()?))
}

fn le16(bytes: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes(bytes.get(at..at + 2)?.try_into().ok()?))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn png_dimensions_are_read() {
        let mut png = b"\x89PNG\r\n\x1a\n\x00\x00\x00\x0dIHDR".to_vec();
        png.extend_from_slice(&64u32.to_be_bytes());
        png.extend_from_slice(&32u32.to_be_bytes());
        let image = inspect(&png).expect("a png");
        assert_eq!((image.width, image.height, image.extension), (64, 32, "png"));
    }

    #[test]
    fn gif_dimensions_are_read() {
        let mut gif = b"GIF89a".to_vec();
        gif.extend_from_slice(&7u16.to_le_bytes());
        gif.extend_from_slice(&11u16.to_le_bytes());
        let image = inspect(&gif).expect("a gif");
        assert_eq!((image.width, image.height, image.extension), (7, 11, "gif"));
    }

    #[test]
    fn jpeg_dimensions_are_read_past_earlier_segments() {
        // APP0 (16 bytes of payload) then SOF0 declaring 5x9.
        let mut jpeg = b"\xff\xd8\xff\xe0\x00\x10JFIF\0\x01\x01\0\0\x01\0\x01\0\0".to_vec();
        jpeg.extend_from_slice(b"\xff\xc0\x00\x11\x08");
        jpeg.extend_from_slice(&9u16.to_be_bytes());
        jpeg.extend_from_slice(&5u16.to_be_bytes());
        let image = inspect(&jpeg).expect("a jpeg");
        assert_eq!((image.width, image.height, image.extension), (5, 9, "jpeg"));
    }

    #[test]
    fn truncated_and_unknown_input_is_refused_not_panicked() {
        assert_eq!(inspect(b""), None);
        assert_eq!(inspect(b"\x89PNG\r\n\x1a\n\x00\x00"), None);
        assert_eq!(inspect(b"\xff\xd8\xff\xe0\x00"), None);
        assert_eq!(inspect(b"GIF89a\x00"), None);
        assert_eq!(inspect(b"not an image at all"), None);
        // A frame header that declares a zero dimension.
        let mut zero = b"\xff\xd8\xff\xc0\x00\x11\x08".to_vec();
        zero.extend_from_slice(&0u16.to_be_bytes());
        zero.extend_from_slice(&5u16.to_be_bytes());
        assert_eq!(inspect(&zero), None);
    }
}
