//! Symmetric body codec: `data[i] ^ key ^ gTable[i]`.

use crate::gtable::gtable_at;

/// Apply the XOR codec in place. Encryption and decryption are identical.
pub fn xor_in_place(data: &mut [u8], key: u8) {
    for (i, b) in data.iter_mut().enumerate() {
        *b ^= key ^ gtable_at(i);
    }
}

/// Apply the XOR codec to `data`, returning a new buffer.
pub fn xor_bytes(data: &[u8], key: u8) -> Vec<u8> {
    data.iter()
        .enumerate()
        .map(|(i, b)| b ^ key ^ gtable_at(i))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip() {
        let key = 0x3A;
        let plaintext: Vec<u8> = (0..=255u8).collect();
        let enc = xor_bytes(&plaintext, key);
        assert_ne!(enc, plaintext);
        assert_eq!(xor_bytes(&enc, key), plaintext);
    }

    #[test]
    fn different_keys_decrypt_differently() {
        let plaintext = b"FA007007000;";
        let a = xor_bytes(plaintext, 0x11);
        let b = xor_bytes(plaintext, 0x22);
        assert_ne!(a, b);
    }
}
