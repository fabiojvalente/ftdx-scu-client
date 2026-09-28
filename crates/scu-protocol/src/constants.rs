//! Wire-level constants for the SCU-LAN10 protocol.

/// Magic sentinel that starts every UDP datagram (ASCII "ZZ").
pub const MAGIC: [u8; 2] = [0x5A, 0x5A];

/// Type field; always `00 01` in observed traffic.
pub const TYPE_FIELD: [u8; 2] = [0x00, 0x01];

/// Channel routing identifiers (the value stored in `b2 ^ key`).
pub mod channel {
    pub const CTRL: u8 = 0xFC;
    pub const CAT: u8 = 0xFD;
    pub const AUDIO: u8 = 0xFE;
    pub const SCOPE: u8 = 0xFA;
}

/// Message types (`b4 ^ key`). Bit 7 signals server -> client.
pub mod msg {
    pub const SETUP: u8 = 0x01;
    pub const AUTH: u8 = 0x03;
    pub const KEEPALIVE: u8 = 0x05;
    pub const PORT_INIT: u8 = 0x06;
    pub const DATA: u8 = 0x07;

    pub const SETUP_ACK: u8 = 0x81;
    pub const AUTH_RESP: u8 = 0x83;
    pub const KEEPALIVE_RESP: u8 = 0x85;
    pub const PORT_INIT_ACK: u8 = 0x86;
    pub const DATA_RESP: u8 = 0x87;
}

/// Default base port. The four channels live on `base + 0..=3`.
pub const DEFAULT_BASE_PORT: u16 = 50000;

/// Sizes from the specification.
pub const CAT_HEADER_LEN: usize = 8;
pub const AUDIO_INNER_HEADER_LEN: usize = 24;
pub const AUDIO_PAYLOAD_LEN: usize = 640;
pub const AUDIO_BODY_LEN: usize = AUDIO_INNER_HEADER_LEN + AUDIO_PAYLOAD_LEN;
/// Client -> server payloads are prefixed with `[session_id, 0, 0, 0]`; TX
/// audio is no exception, which is why its body is 4 bytes larger than RX.
pub const AUDIO_TX_PREFIX_LEN: usize = 4;
/// TX audio body length (client -> server): the session prefix plus an
/// RX-shaped inner header and payload. The exact TX inner-header layout has not
/// been fully characterized upstream, so this mirrors the RX layout.
pub const AUDIO_TX_BODY_LEN: usize = AUDIO_TX_PREFIX_LEN + AUDIO_BODY_LEN;
pub const SCOPE_BODY_LEN: usize = 4096;

/// Fixed byte `b3` is XOR'd with this value.
pub const HEADER_B3_CONST: u8 = 0x03;
/// High body-length byte is additionally XOR'd with this value.
pub const HEADER_B7_XOR: u8 = 0x07;
