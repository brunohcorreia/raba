//! Unit tests for [`super`].
//!
//! Its own file rather than a `mod tests` block inside the module it tests, so
//! that `scripts/coverage.sh` can leave it out: test code counted as covered
//! product code makes the only number anybody reads say the wrong thing. See
//! `docs/maintenance.md`.

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
