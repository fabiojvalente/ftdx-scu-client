//! Authentication packet construction (Phase 2).

use crate::constants::{channel, msg};
use crate::frame::frame_packet;
use crate::header::build_header;

/// The auth packet always uses sequence `0x06` and a fixed key of `0x3A`.
pub const AUTH_SEQ: u8 = 0x06;
pub const AUTH_KEY: u8 = 0x3A;

/// Body layout: `01 00` + username (33, null-padded) + password (33, null-padded).
pub const AUTH_BODY_LEN: usize = 2 + 33 + 33;

fn pad33(s: &str) -> [u8; 33] {
    let mut out = [0u8; 33];
    let bytes = s.as_bytes();
    let n = bytes.len().min(32);
    out[..n].copy_from_slice(&bytes[..n]);
    out
}

/// Build the plaintext auth body.
pub fn auth_body(username: &str, password: &str) -> Vec<u8> {
    let mut body = Vec::with_capacity(AUTH_BODY_LEN);
    body.extend_from_slice(&[0x01, 0x00]);
    body.extend_from_slice(&pad33(username));
    body.extend_from_slice(&pad33(password));
    body
}

/// Build the full auth datagram.
pub fn build_auth(username: &str, password: &str) -> Vec<u8> {
    let body = auth_body(username, password);
    let header = build_header(AUTH_SEQ, channel::CTRL, msg::AUTH, body.len(), AUTH_KEY);
    frame_packet(&header, &body)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frame::parse_packet;

    #[test]
    fn auth_packet_shape() {
        let pkt = build_auth("defaultuser", "defaultuser");
        let parsed = parse_packet(&pkt).unwrap();
        assert_eq!(parsed.header.seq, AUTH_SEQ);
        assert_eq!(parsed.header.key, AUTH_KEY);
        assert_eq!(parsed.header.channel, channel::CTRL);
        assert_eq!(parsed.msg_type(), msg::AUTH);
        assert_eq!(parsed.body.len(), AUTH_BODY_LEN);
        assert_eq!(&parsed.body[..2], &[0x01, 0x00]);
        assert_eq!(&parsed.body[2..13], b"defaultuser");
        assert_eq!(&parsed.body[35..46], b"defaultuser");
        assert!(parsed.body[13..35].iter().all(|&b| b == 0));
    }

    #[test]
    fn long_credentials_are_truncated_to_32() {
        let long = "x".repeat(64);
        let pkt = build_auth(&long, &long);
        let parsed = parse_packet(&pkt).unwrap();
        assert_eq!(parsed.body.len(), AUTH_BODY_LEN);
        assert!(parsed.body[2..34].iter().all(|&b| b == b'x'));
        assert_eq!(parsed.body[34], 0);
    }
}
