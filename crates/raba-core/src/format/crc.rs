//! CRC-32 (IEEE 802.3), the same polynomial gzip and PNG use.
//!
//! Hand-written because `raba-core` has a dependency budget of zero and this is
//! forty lines. See `DEPENDENCIES.md`.

/// Lookup table, built at compile time.
///
/// Indexing is unavoidable when filling an array in a `const` block, and the index
/// is a loop counter bounded by the array's own length. This is the one place in
/// the crate where `indexing_slicing` is waived, and it is waived at compile time.
#[allow(
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation
)]
const TABLE: [u32; 256] = {
    let mut table = [0u32; 256];
    let mut i = 0;
    while i < 256 {
        let mut crc = i as u32;
        let mut bit = 0;
        while bit < 8 {
            crc = if crc & 1 != 0 { 0xEDB8_8320 ^ (crc >> 1) } else { crc >> 1 };
            bit += 1;
        }
        table[i] = crc;
        i += 1;
    }
    table
};

/// CRC-32 of `data`.
#[must_use]
pub fn crc32(data: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for &byte in data {
        // Masked to 8 bits, so always within the table; `get` keeps that provable
        // rather than merely true.
        let index = usize::try_from((crc ^ u32::from(byte)) & 0xFF).unwrap_or(0);
        let entry = TABLE.get(index).copied().unwrap_or(0);
        crc = entry ^ (crc >> 8);
    }
    crc ^ 0xFFFF_FFFF
}

#[cfg(test)]
mod tests {
    use super::crc32;

    #[test]
    fn known_vectors() {
        // Standard CRC-32 check values.
        assert_eq!(crc32(b""), 0x0000_0000);
        assert_eq!(crc32(b"a"), 0xE8B7_BE43);
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
        assert_eq!(crc32(b"The quick brown fox jumps over the lazy dog"), 0x414F_A339);
    }

    #[test]
    fn detects_single_bit_flips() {
        let data = b"RABA animation payload";
        let base = crc32(data);
        for byte_index in 0..data.len() {
            for bit in 0..8u32 {
                let mut flipped = data.to_vec();
                if let Some(b) = flipped.get_mut(byte_index) {
                    *b ^= 1 << bit;
                }
                assert_ne!(crc32(&flipped), base, "flip at byte {byte_index} bit {bit}");
            }
        }
    }
}
