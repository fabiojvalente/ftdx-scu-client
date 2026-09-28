//! The five hardcoded Phase 1 keepalives, reproduced byte-for-byte.

/// Raw hex of the five init packets, in transmission order.
pub const INIT_PACKETS_HEX: [&str; 5] = [
    "5a5a0e00000101a0a15e585d595a5b585956",
    "5a5a0e000001024042bdbbbebab9b8bbbab5",
    "5a5a0e00000103e0e31c1a1f1b18191a1b14",
    "5a5a0e0000010480847b7d787c7f7e7d7c73",
    "5a5a0e000001052025dadcd9dddedfdcddd2",
];

fn decode_hex(s: &str) -> Vec<u8> {
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).expect("valid hex"))
        .collect()
}

/// The five init packets as raw bytes.
pub fn init_packets() -> [Vec<u8>; 5] {
    INIT_PACKETS_HEX.map(decode_hex)
}
