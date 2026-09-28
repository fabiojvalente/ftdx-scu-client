//! Events emitted by a live session.

use scu_audio::AudioFrame;
use scu_cat::RadioModel;

/// Something the client observed or a state transition.
#[derive(Debug, Clone)]
pub enum Event {
    /// The handshake completed and the session is live.
    Connected { session_id: u8 },
    /// The radio announced its model via a CAT `ID;` response.
    Radio(RadioModel),
    /// A CAT frame from the radio (response or unsolicited meter reading).
    Cat(String),
    /// A decoded RX audio frame.
    Audio(Box<AudioFrame>),
    /// A raw decrypted spectrum sweep body (4096 bytes). Decode with
    /// [`scu_scope`](https://docs.rs/scu-scope) so the client can choose the
    /// bin-extraction rule without reconnecting.
    Scope(Vec<u8>),
    /// A non-fatal transport error.
    Error(String),
    /// The receive loop stopped.
    Disconnected(String),
}
