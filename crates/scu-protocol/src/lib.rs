//! # scu-protocol
//!
//! Pure, synchronous implementation of the SCU-LAN10 UDP wire format:
//! framing, the 8-byte header, and the per-packet XOR body codec. No I/O lives
//! here so the whole layer can be unit-tested and reused by any client.
//!
//! See the [protocol specification](https://github.com/CarrierWaveApp/sculan10-protocol).

pub mod auth;
pub mod constants;
pub mod crypto;
pub mod error;
pub mod frame;
pub mod gtable;
pub mod header;
pub mod init;

pub use constants::{
    channel, msg, AUDIO_BODY_LEN, AUDIO_INNER_HEADER_LEN, AUDIO_PAYLOAD_LEN, AUDIO_TX_BODY_LEN,
    AUDIO_TX_PREFIX_LEN, CAT_HEADER_LEN, DEFAULT_BASE_PORT, SCOPE_BODY_LEN,
};
pub use error::ProtocolError;
pub use frame::{frame_new, frame_packet, parse_packet, Packet};
pub use header::{build_header, parse_header, Header};

/// Total wire overhead: magic (2) + length (2) + type (2).
pub const WIRE_OVERHEAD: usize = 6;

/// Given a body length, the on-wire datagram size (including the 8-byte header).
pub const fn datagram_len(body_len: usize) -> usize {
    WIRE_OVERHEAD + CAT_HEADER_LEN + body_len
}
