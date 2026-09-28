//! The `5A 5A` datagram envelope and whole-packet (de)serialization.

use crate::constants::{MAGIC, TYPE_FIELD};
use crate::crypto::xor_bytes;
use crate::error::ProtocolError;
use crate::header::{build_header, parse_header, Header};

/// A parsed packet with a decrypted body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Packet {
    pub header: Header,
    pub body: Vec<u8>,
}

impl Packet {
    pub fn channel(&self) -> u8 {
        self.header.channel
    }

    pub fn msg_type(&self) -> u8 {
        self.header.msg_type
    }

    /// True when the server -> client direction bit (`0x80`) is set.
    pub fn from_server(&self) -> bool {
        self.header.msg_type & 0x80 != 0
    }
}

/// Wrap an 8-byte header plus plaintext body into a full datagram.
pub fn frame_packet(header: &[u8; 8], plaintext: &[u8]) -> Vec<u8> {
    let key = header[5];
    let encrypted = xor_bytes(plaintext, key);

    let payload_len = header.len() + encrypted.len();
    let length = (payload_len + TYPE_FIELD.len()) as u16;

    let mut out = Vec::with_capacity(4 + length as usize);
    out.extend_from_slice(&MAGIC);
    out.extend_from_slice(&length.to_le_bytes());
    out.extend_from_slice(&TYPE_FIELD);
    out.extend_from_slice(header);
    out.extend_from_slice(&encrypted);
    out
}

/// Convenience: build a header (with a random key) and frame it in one call.
pub fn frame_new(seq: u8, channel: u8, msg_type: u8, body: &[u8], key: Option<u8>) -> Vec<u8> {
    let key = key.unwrap_or_else(rand::random);
    let header = build_header(seq, channel, msg_type, body.len(), key);
    frame_packet(&header, body)
}

/// Parse a full datagram, validating framing and decrypting the body.
pub fn parse_packet(datagram: &[u8]) -> Result<Packet, ProtocolError> {
    if datagram.len() < 4 + 2 + 8 {
        return Err(ProtocolError::TooShort(datagram.len()));
    }

    let magic = [datagram[0], datagram[1]];
    if magic != MAGIC {
        return Err(ProtocolError::BadMagic(magic));
    }

    let declared = u16::from_le_bytes([datagram[2], datagram[3]]) as usize;
    let actual = datagram.len() - 4;
    if declared != actual {
        return Err(ProtocolError::LengthMismatch { declared, actual });
    }

    let type_field = [datagram[4], datagram[5]];
    if type_field != TYPE_FIELD {
        return Err(ProtocolError::BadType(type_field));
    }

    let mut raw_header = [0u8; 8];
    raw_header.copy_from_slice(&datagram[6..14]);
    let header = parse_header(&raw_header)?;

    let encrypted = &datagram[14..];
    if header.body_len != encrypted.len() {
        return Err(ProtocolError::BodyLengthMismatch {
            declared: header.body_len,
            actual: encrypted.len(),
        });
    }

    let body = xor_bytes(encrypted, header.key);
    Ok(Packet { header, body })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::constants::{channel, msg};

    #[test]
    fn frame_then_parse_round_trip() {
        let body = b"FA007007000;";
        let datagram = frame_new(0x01, channel::CAT, msg::DATA, body, Some(0x5C));
        let packet = parse_packet(&datagram).unwrap();
        assert_eq!(packet.body, body);
        assert_eq!(packet.channel(), channel::CAT);
        assert_eq!(packet.msg_type(), msg::DATA);
        assert!(!packet.from_server());
    }

    #[test]
    fn hardcoded_init_packets_parse() {
        let hex = [
            "5a5a0e00000101a0a15e585d595a5b585956",
            "5a5a0e000001024042bdbbbebab9b8bbbab5",
            "5a5a0e00000103e0e31c1a1f1b18191a1b14",
            "5a5a0e0000010480847b7d787c7f7e7d7c73",
            "5a5a0e000001052025dadcd9dddedfdcddd2",
        ];
        for (idx, h) in hex.iter().enumerate() {
            let bytes = hex_to_bytes(h);
            let packet = parse_packet(&bytes).unwrap();
            assert_eq!(packet.header.seq, (idx + 1) as u8);
            assert_eq!(packet.channel(), channel::CTRL);
            assert_eq!(packet.msg_type(), msg::KEEPALIVE);
            // NB: the published spec claims the decrypted body is
            // `[0x04; 4]`, but applying the documented gTable yields zeroes
            // for all five hardcoded packets. The wire bytes are what matter
            // (we resend them verbatim); this asserts the true decode.
            assert_eq!(packet.body, [0x00, 0x00, 0x00, 0x00]);
        }
    }

    #[test]
    fn rejects_bad_magic() {
        let mut d = frame_new(1, channel::CTRL, msg::KEEPALIVE, &[0; 4], Some(1));
        d[0] = 0x00;
        assert!(matches!(parse_packet(&d), Err(ProtocolError::BadMagic(_))));
    }

    #[test]
    fn rejects_truncated() {
        let d = frame_new(1, channel::CTRL, msg::KEEPALIVE, &[0; 4], Some(1));
        assert!(matches!(
            parse_packet(&d[..8]),
            Err(ProtocolError::TooShort(_))
        ));
    }

    fn hex_to_bytes(s: &str) -> Vec<u8> {
        (0..s.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
            .collect()
    }
}
