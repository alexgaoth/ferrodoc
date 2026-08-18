//! SHA-1, for naming extracted images.
//!
//! Not a security choice and not a dependency worth taking: pandoc's media
//! bag names an extracted image `<sha1 of its bytes>.<ext>`, so matching
//! that filename is the only way `diff-ipynb` can agree with pandoc on a
//! notebook that plots anything.

use std::fmt::Write as _;

/// The hex SHA-1 digest of `bytes`.
pub fn hex(bytes: &[u8]) -> String {
    let mut state: [u32; 5] = [0x6745_2301, 0xefcd_ab89, 0x98ba_dcfe, 0x1032_5476, 0xc3d2_e1f0];
    let mut message = bytes.to_vec();
    let bit_len = (bytes.len() as u64).wrapping_mul(8);
    message.push(0x80);
    while message.len() % 64 != 56 {
        message.push(0);
    }
    message.extend_from_slice(&bit_len.to_be_bytes());

    for block in message.chunks_exact(64) {
        let mut schedule = [0u32; 80];
        for (i, word) in block.chunks_exact(4).enumerate() {
            schedule[i] = u32::from_be_bytes([word[0], word[1], word[2], word[3]]);
        }
        for i in 16..80 {
            schedule[i] = (schedule[i - 3] ^ schedule[i - 8] ^ schedule[i - 14]
                ^ schedule[i - 16])
                .rotate_left(1);
        }
        let mut round = state;
        for (i, word) in schedule.iter().enumerate() {
            let (mixed, constant) = match i {
                0..=19 => ((round[1] & round[2]) | ((!round[1]) & round[3]), 0x5a82_7999),
                20..=39 => (round[1] ^ round[2] ^ round[3], 0x6ed9_eba1),
                40..=59 => (
                    (round[1] & round[2]) | (round[1] & round[3]) | (round[2] & round[3]),
                    0x8f1b_bcdc,
                ),
                _ => (round[1] ^ round[2] ^ round[3], 0xca62_c1d6),
            };
            let temp = round[0]
                .rotate_left(5)
                .wrapping_add(mixed)
                .wrapping_add(round[4])
                .wrapping_add(constant)
                .wrapping_add(*word);
            round = [temp, round[0], round[1].rotate_left(30), round[2], round[3]];
        }
        for (slot, value) in state.iter_mut().zip(round) {
            *slot = slot.wrapping_add(value);
        }
    }

    state.iter().fold(String::with_capacity(40), |mut out, word| {
        let _ = write!(out, "{word:08x}");
        out
    })
}

#[cfg(test)]
mod tests {
    use super::hex;

    #[test]
    fn matches_the_published_vectors() {
        assert_eq!(hex(b""), "da39a3ee5e6b4b0d3255bfef95601890afd80709");
        assert_eq!(hex(b"abc"), "a9993e364706816aba3e25717850c26c9cd0d89d");
        assert_eq!(
            hex(b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq"),
            "84983e441c3bd26ebaae4aa1f95129e5e54670f1"
        );
        // Long enough to need a second block, which is where a wrong
        // padding length shows up.
        assert_eq!(hex(&[b'a'; 1000]), "291e9a6c66994949b57ba5e650361e98fc36b1ba");
    }
}
