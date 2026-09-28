//! Closed-form position-dependent scramble table used by the body XOR codec.

use std::sync::OnceLock;

/// Offset pattern per residue class (mod 4): `[0, -1, -2, +5]`.
const OFFSETS: [u8; 4] = [0, 0xFF, 0xFE, 5];

/// Largest table we ever need (covers the 4096-byte scope body with margin).
pub const DEFAULT_TABLE_SIZE: usize = 8192;

/// Compute `gTable[i]` directly from the closed-form formula.
#[inline]
pub fn gtable_value(i: usize) -> u8 {
    let base = 4u32 * (i as u32 / 4) + 6 + OFFSETS[i % 4] as u32;
    (base & 0xFF) as u8
}

fn cached_table() -> &'static [u8; DEFAULT_TABLE_SIZE] {
    static TABLE: OnceLock<[u8; DEFAULT_TABLE_SIZE]> = OnceLock::new();
    TABLE.get_or_init(|| {
        let mut t = [0u8; DEFAULT_TABLE_SIZE];
        for (i, v) in t.iter_mut().enumerate() {
            *v = gtable_value(i);
        }
        t
    })
}

/// Look up a scramble byte, using the cached table when in range.
#[inline]
pub fn gtable_at(i: usize) -> u8 {
    if i < DEFAULT_TABLE_SIZE {
        cached_table()[i]
    } else {
        gtable_value(i)
    }
}

/// Build a fresh table of an arbitrary size (reference implementation).
pub fn build_gtable(size: usize) -> Vec<u8> {
    (0..size).map(gtable_value).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_sixteen_values_match_spec() {
        let expected = [
            0x06, 0x05, 0x04, 0x0B, 0x0A, 0x09, 0x08, 0x0F, 0x0E, 0x0D, 0x0C, 0x13, 0x12, 0x11,
            0x10, 0x17,
        ];
        for (i, want) in expected.iter().enumerate() {
            assert_eq!(gtable_value(i), *want, "gtable[{i}]");
            assert_eq!(gtable_at(i), *want, "cached gtable[{i}]");
        }
    }

    #[test]
    fn last_row_matches_spec() {
        // Final 16 values from the published 256-entry table.
        let expected = [
            0xF6, 0xF5, 0xF4, 0xFB, 0xFA, 0xF9, 0xF8, 0xFF, 0xFE, 0xFD, 0xFC, 0x03, 0x02, 0x01,
            0x00, 0x07,
        ];
        for (i, want) in expected.iter().enumerate() {
            assert_eq!(gtable_value(240 + i), *want, "gtable[{}]", 240 + i);
        }
    }

    #[test]
    fn cache_matches_closed_form() {
        for i in [0usize, 1, 3, 4, 255, 256, 1000, 4095, 8191] {
            assert_eq!(gtable_at(i), gtable_value(i));
        }
    }
}
