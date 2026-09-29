//! RX audio decoding for the SCU-LAN10 audio channel.
//!
//! Each RX packet carries a 24-byte inner header followed by 640 bytes of
//! interleaved stereo `Int16 LE` PCM at 16 kHz (160 stereo frames / 10 ms).
//!
//! This module is pure decoding + a small PCM ring buffer. The platform
//! playback/capture backends live in [`output`] and [`input`]: `cpal` on the
//! desktop, the Web Audio API in the browser.

#[cfg(not(target_arch = "wasm32"))]
pub mod input;
#[cfg(not(target_arch = "wasm32"))]
pub mod output;

#[cfg(target_arch = "wasm32")]
#[path = "web/input.rs"]
pub mod input;
#[cfg(target_arch = "wasm32")]
#[path = "web/output.rs"]
pub mod output;

pub mod vox;

mod resample;

use scu_protocol::{AUDIO_BODY_LEN, AUDIO_INNER_HEADER_LEN, AUDIO_PAYLOAD_LEN};
use thiserror::Error;

/// Sample rate of the network audio channel (RX and TX).
pub const AUDIO_SAMPLE_RATE: u32 = 16_000;

/// Stereo PCM frames per audio packet (10 ms at 16 kHz).
pub const AUDIO_FRAMES_PER_PACKET: usize = 160;

/// Decoded RX audio frame (interleaved samples normalized to -1.0..=1.0).
#[derive(Debug, Clone, PartialEq)]
pub struct AudioFrame {
    pub seq: u8,
    /// Format id byte (byte 2 of the inner header); 1 = PCM Int16 LE.
    pub format: u8,
    pub channels: u8,
    pub sample_rate: u32,
    /// Number of PCM payload bytes (from the inner header).
    pub payload_bytes: u16,
    /// Interleaved samples, `channels` per frame.
    pub samples: Vec<f32>,
}

impl AudioFrame {
    /// Number of interleaved sample values.
    pub fn len(&self) -> usize {
        self.samples.len()
    }

    pub fn is_empty(&self) -> bool {
        self.samples.is_empty()
    }
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum AudioError {
    #[error("audio body too short: {0} bytes")]
    TooShort(usize),
    #[error("payload count {declared} exceeds available bytes {available}")]
    PayloadOverflow { declared: usize, available: usize },
}

fn u16le(b: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([b[at], b[at + 1]])
}

/// Decode a decrypted audio body into an [`AudioFrame`].
pub fn decode(body: &[u8]) -> Result<AudioFrame, AudioError> {
    if body.len() < AUDIO_INNER_HEADER_LEN {
        return Err(AudioError::TooShort(body.len()));
    }

    let seq = body[0];
    let format = body[2];
    let channels = body[3];
    let sample_rate = u32::from_le_bytes([body[4], body[5], body[6], body[7]]);
    let payload_bytes = u16le(body, 22);

    let available = body.len() - AUDIO_INNER_HEADER_LEN;
    let declared = payload_bytes as usize;
    let take = declared.min(available);
    if declared > available {
        return Err(AudioError::PayloadOverflow {
            declared,
            available,
        });
    }

    let pcm = &body[AUDIO_INNER_HEADER_LEN..AUDIO_INNER_HEADER_LEN + take];
    let samples = pcm
        .as_chunks::<2>()
        .0
        .iter()
        .map(|c| i16::from_le_bytes([c[0], c[1]]) as f32 / 32768.0)
        .collect();

    Ok(AudioFrame {
        seq,
        format,
        channels: channels.max(1),
        sample_rate,
        payload_bytes,
        samples,
    })
}

/// Encode the audio portion of a TX packet (client -> server).
///
/// Returns an RX-shaped body: a 24-byte inner header, followed by 640 bytes of
/// interleaved stereo `Int16 LE` PCM (160 frames). The session-id prefix that
/// makes the on-wire TX body 668 bytes is added by the session layer
/// (`ScuHandle::send_tx_audio`), since only it knows the session id.
///
/// `stereo` is the interleaved L/R sample stream; it is padded with silence or
/// truncated to 160 frames as needed. The exact TX inner-header layout has not
/// been fully characterized upstream, so this mirrors the RX layout.
pub fn encode_tx(seq: u8, stereo: &[i16]) -> Vec<u8> {
    let mut body = vec![0u8; AUDIO_BODY_LEN];

    body[0] = seq;
    body[1] = 0x00;
    body[2] = 0x01; // PCM Int16 LE
    body[3] = 2; // channels
    body[4..8].copy_from_slice(&AUDIO_SAMPLE_RATE.to_le_bytes());
    body[16..18].copy_from_slice(&2000u16.to_le_bytes());
    body[18..20].copy_from_slice(&1000u16.to_le_bytes());
    body[20] = 100;
    body[21] = 100;
    body[22..24].copy_from_slice(&(AUDIO_PAYLOAD_LEN as u16).to_le_bytes());

    let payload = &mut body[AUDIO_INNER_HEADER_LEN..AUDIO_INNER_HEADER_LEN + AUDIO_PAYLOAD_LEN];
    for (chunk, &sample) in payload.as_chunks_mut::<2>().0.iter_mut().zip(stereo) {
        *chunk = sample.to_le_bytes();
    }

    body
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_body(sample_rate: u32, channels: u8, frames: usize) -> Vec<u8> {
        let mut body = vec![0u8; AUDIO_INNER_HEADER_LEN];
        body[0] = 0x2A;
        body[1] = 0x00;
        body[2] = 0x01;
        body[3] = channels;
        body[4..8].copy_from_slice(&sample_rate.to_le_bytes());
        let payload = frames * channels as usize * 2;
        body[16..18].copy_from_slice(&2000u16.to_le_bytes());
        body[18..20].copy_from_slice(&1000u16.to_le_bytes());
        body[20] = 100;
        body[21] = 100;
        body[22..24].copy_from_slice(&(payload as u16).to_le_bytes());
        for i in 0..frames * channels as usize {
            let v = ((i as i16) * 100).to_le_bytes();
            body.extend_from_slice(&v);
        }
        body
    }

    #[test]
    fn decodes_160_stereo_frames() {
        let body = make_body(16000, 2, 160);
        assert_eq!(body.len(), AUDIO_INNER_HEADER_LEN + 640);
        let frame = decode(&body).unwrap();
        assert_eq!(frame.seq, 0x2A);
        assert_eq!(frame.channels, 2);
        assert_eq!(frame.sample_rate, 16000);
        assert_eq!(frame.payload_bytes, 640);
        assert_eq!(frame.samples.len(), 320);
    }

    #[test]
    fn samples_are_normalized() {
        let body = make_body(16000, 2, 2);
        let frame = decode(&body).unwrap();
        assert!(frame.samples.iter().all(|s| (-1.0..=1.0).contains(s)));
    }

    #[test]
    fn rejects_short_body() {
        assert_eq!(decode(&[0u8; 10]), Err(AudioError::TooShort(10)));
    }

    #[test]
    fn rejects_payload_overflow() {
        let mut body = make_body(16000, 2, 2);
        body[22..24].copy_from_slice(&4000u16.to_le_bytes());
        assert!(matches!(
            decode(&body),
            Err(AudioError::PayloadOverflow { .. })
        ));
    }

    #[test]
    fn encode_tx_matches_the_wire_shape() {
        let body = encode_tx(0x42, &[]);
        assert_eq!(body.len(), AUDIO_BODY_LEN);
        assert_eq!(body[0], 0x42);
        assert_eq!(body[1], 0x00);
        assert_eq!(body[2], 0x01);
        assert_eq!(body[3], 2);
        assert_eq!(
            u32::from_le_bytes([body[4], body[5], body[6], body[7]]),
            16000
        );
        assert_eq!(u16::from_le_bytes([body[22], body[23]]), 640);
    }

    #[test]
    fn encode_tx_round_trips_pcm() {
        let stereo: Vec<i16> = (0..320).map(|i| (i as i16) * 100).collect();
        let body = encode_tx(7, &stereo);
        let decoded: Vec<i16> = body[AUDIO_INNER_HEADER_LEN..AUDIO_INNER_HEADER_LEN + 640]
            .as_chunks::<2>()
            .0
            .iter()
            .map(|c| i16::from_le_bytes([c[0], c[1]]))
            .collect();
        assert_eq!(decoded, stereo);
    }

    #[test]
    fn encode_tx_pads_and_truncates() {
        let short = encode_tx(0, &[1, 2, 3, 4]);
        assert_eq!(i16::from_le_bytes([short[24], short[25]]), 1);
        assert!(
            short[AUDIO_INNER_HEADER_LEN + 8..AUDIO_INNER_HEADER_LEN + AUDIO_PAYLOAD_LEN]
                .iter()
                .all(|&b| b == 0)
        );

        let long = vec![9i16; 400];
        let body = encode_tx(0, &long);
        assert_eq!(body.len(), AUDIO_BODY_LEN);
    }
}
