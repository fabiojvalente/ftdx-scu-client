//! The unencrypted 8-byte header.
//!
//! ```text
//! ┌──────┬──────┬──────┬──────┬──────┬──────┬───────┬───────┐
//! │  b0  │  b1  │  b2  │  b3  │  b4  │  b5  │  b6   │  b7   │
//! │ seq  │verify│ chan │ const│ type │ key  │len_lo │len_hi │
//! └──────┴──────┴──────┴──────┴──────┴──────┴───────┴───────┘
//! ```

use crate::constants::{HEADER_B3_CONST, HEADER_B7_XOR};
use crate::error::ProtocolError;

/// Parsed header metadata.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Header {
    pub seq: u8,
    pub channel: u8,
    pub msg_type: u8,
    pub body_len: usize,
    pub key: u8,
}

/// Build the 8-byte header. `key` is a fresh random byte per packet.
pub fn build_header(seq: u8, channel: u8, msg_type: u8, body_len: usize, key: u8) -> [u8; 8] {
    [
        seq,
        key ^ seq ^ 0xFC,
        key ^ channel,
        key ^ HEADER_B3_CONST,
        key ^ msg_type,
        key,
        key ^ (body_len & 0xFF) as u8,
        key ^ (HEADER_B7_XOR ^ ((body_len >> 8) & 0xFF) as u8),
    ]
}

/// Parse and validate the 8-byte header.
pub fn parse_header(raw: &[u8; 8]) -> Result<Header, ProtocolError> {
    let seq = raw[0];
    let key = raw[5];

    if raw[1] != key ^ seq ^ 0xFC {
        return Err(ProtocolError::HeaderMismatch("b1 verify byte"));
    }
    if raw[3] != key ^ HEADER_B3_CONST {
        return Err(ProtocolError::HeaderMismatch("b3 constant byte"));
    }

    let channel = raw[2] ^ key;
    let msg_type = raw[4] ^ key;
    let body_len = (raw[6] ^ key) as usize | (((raw[7] ^ key) ^ HEADER_B7_XOR) as usize) << 8;

    Ok(Header {
        seq,
        channel,
        msg_type,
        body_len,
        key,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::constants::channel;

    #[test]
    fn build_then_parse_round_trip() {
        for &(seq, channel, msg_type, body_len, key) in &[
            (
                0x06u8,
                channel::CTRL,
                crate::constants::msg::AUTH,
                68usize,
                0x3Au8,
            ),
            (0x07, channel::CAT, crate::constants::msg::DATA, 13, 0xAB),
            (
                0x01,
                channel::AUDIO,
                crate::constants::msg::PORT_INIT,
                4,
                0x00,
            ),
            (
                0x00,
                channel::SCOPE,
                crate::constants::msg::KEEPALIVE,
                4,
                0xFF,
            ),
            (
                0x05,
                channel::SCOPE,
                crate::constants::msg::DATA,
                4096,
                0x42,
            ),
        ] {
            let raw = build_header(seq, channel, msg_type, body_len, key);
            let h = parse_header(&raw).unwrap();
            assert_eq!(h.seq, seq);
            assert_eq!(h.channel, channel);
            assert_eq!(h.msg_type, msg_type);
            assert_eq!(h.body_len, body_len);
            assert_eq!(h.key, key);
        }
    }

    #[test]
    fn rejects_corrupted_b1() {
        let mut raw = build_header(1, channel::CTRL, crate::constants::msg::KEEPALIVE, 4, 0x77);
        raw[1] ^= 0xFF;
        assert!(matches!(
            parse_header(&raw),
            Err(ProtocolError::HeaderMismatch(_))
        ));
    }

    #[test]
    fn auth_header_matches_known_capture_shape() {
        let raw = build_header(0x06, channel::CTRL, crate::constants::msg::AUTH, 68, 0x3A);
        assert_eq!(raw[0], 0x06);
        assert_eq!(raw[5], 0x3A);
        assert_eq!(raw[3], 0x3A ^ 0x03);
    }
}
