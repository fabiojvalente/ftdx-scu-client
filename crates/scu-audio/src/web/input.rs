//! Browser microphone capture for TX audio via the Web Audio API.
//!
//! `getUserMedia` is asynchronous, so [`MicInput::new`] returns immediately and
//! finishes wiring the capture graph once the browser grants access. Capture is
//! gated by the shared `enabled` flag (PTT held).

use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU8, Ordering};
use std::sync::{Arc, Mutex};

use thiserror::Error;
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::JsFuture;
use web_sys::{
    AudioContext, AudioNode, AudioProcessingEvent, MediaStreamConstraints, ScriptProcessorNode,
};

use crate::resample::ChannelResampler;
use crate::{encode_tx, AUDIO_FRAMES_PER_PACKET, AUDIO_SAMPLE_RATE};

/// Callback invoked with each encoded TX body (668 bytes) from the capture
/// callback. Implementations must not block.
pub type TxAudioSink = Box<dyn Fn(&[u8]) + Send + 'static>;

#[derive(Debug, Error)]
pub enum InputError {
    #[error("no default input device")]
    NoDevice,
    #[error("input device not found: {0}")]
    DeviceNotFound(String),
    #[error("device does not support a usable input config: {0}")]
    Config(String),
    #[error("failed to build input stream: {0}")]
    Build(String),
}

/// Metadata about the opened input device.
#[derive(Debug, Clone)]
pub struct InputInfo {
    pub device_name: String,
    pub sample_rate: u32,
    pub channels: u16,
}

/// Capture configuration.
#[derive(Debug, Clone)]
pub struct MicConfig {
    /// Input device name; `None` uses the system default.
    pub device: Option<String>,
    /// Linear input gain applied before packetization.
    pub gain: f32,
}

impl Default for MicConfig {
    fn default() -> Self {
        Self {
            device: None,
            gain: 1.0,
        }
    }
}

struct Shared {
    enabled: AtomicBool,
    gain_bits: AtomicU32,
    seq: AtomicU8,
    in_rate: AtomicU32,
    resampler: Mutex<Option<ChannelResampler>>,
    pending: Mutex<Vec<f32>>,
    sink: Mutex<Option<TxAudioSink>>,
}

impl Shared {
    fn gain(&self) -> f32 {
        f32::from_bits(self.gain_bits.load(Ordering::Relaxed))
    }

    fn process(&self, event: &AudioProcessingEvent) {
        if !self.enabled.load(Ordering::Relaxed) {
            self.pending.lock().unwrap().clear();
            return;
        }
        let Ok(input) = event.input_buffer() else {
            return;
        };
        let channels = input.number_of_channels() as usize;
        let length = input.length() as usize;
        if channels == 0 || length == 0 {
            return;
        }
        let left = input.get_channel_data(0).unwrap_or_default();
        let right = if channels > 1 {
            input.get_channel_data(1).ok()
        } else {
            None
        };

        let mut mono = Vec::with_capacity(length);
        for i in 0..length {
            let l = left.get(i).copied().unwrap_or(0.0);
            let r = right.as_ref().and_then(|c| c.get(i).copied()).unwrap_or(l);
            mono.push((l + r) * 0.5);
        }

        let in_rate = self.in_rate.load(Ordering::Relaxed).max(8_000);
        let mut resampled = Vec::with_capacity(mono.len() + 4);
        {
            let mut guard = self.resampler.lock().unwrap();
            let resampler = guard.get_or_insert_with(|| {
                ChannelResampler::new(in_rate as f64 / AUDIO_SAMPLE_RATE as f64)
            });
            resampler.process(&mono, &mut resampled);
        }

        let gain = self.gain();
        let mut pending = self.pending.lock().unwrap();
        pending.extend(resampled.iter().map(|&s| (s * gain).clamp(-1.0, 1.0)));

        let mut seq = self.seq.load(Ordering::Relaxed);
        let sink = self.sink.lock().unwrap();
        while pending.len() >= AUDIO_FRAMES_PER_PACKET {
            let mut stereo = [0i16; AUDIO_FRAMES_PER_PACKET * 2];
            for (i, &s) in pending[..AUDIO_FRAMES_PER_PACKET].iter().enumerate() {
                let value = (s * 32767.0) as i16;
                stereo[i * 2] = value;
                stereo[i * 2 + 1] = value;
            }
            pending.drain(0..AUDIO_FRAMES_PER_PACKET);
            if let Some(sink) = sink.as_ref() {
                let body = encode_tx(seq, &stereo);
                sink(&body);
                seq = seq.wrapping_add(1);
            }
        }
        self.seq.store(seq, Ordering::Relaxed);
    }
}

/// A live microphone capture. Dropping it stops capture.
pub struct MicInput {
    shared: Arc<Shared>,
    _keep: Arc<Mutex<Vec<JsValue>>>,
    _node: Option<ScriptProcessorNode>,
    info: InputInfo,
}

impl MicInput {
    /// Request microphone access and start capturing. The capture graph is
    /// wired asynchronously once the browser grants access.
    pub fn new(config: MicConfig, sink: TxAudioSink) -> Result<Self, InputError> {
        let shared = Arc::new(Shared {
            enabled: AtomicBool::new(false),
            gain_bits: AtomicU32::new(config.gain.clamp(0.0, 4.0).to_bits()),
            seq: AtomicU8::new(0),
            in_rate: AtomicU32::new(48_000),
            resampler: Mutex::new(None),
            pending: Mutex::new(Vec::new()),
            sink: Mutex::new(Some(sink)),
        });
        let keep: Arc<Mutex<Vec<JsValue>>> = Arc::new(Mutex::new(Vec::new()));

        spawn_capture(Arc::clone(&shared), Arc::clone(&keep));

        Ok(Self {
            shared,
            _keep: keep,
            _node: None,
            info: InputInfo {
                device_name: config
                    .device
                    .clone()
                    .unwrap_or_else(|| "Default microphone".into()),
                sample_rate: AUDIO_SAMPLE_RATE,
                channels: 1,
            },
        })
    }

    pub fn info(&self) -> &InputInfo {
        &self.info
    }

    /// Gate capture. While `false`, no TX packets are produced.
    pub fn set_enabled(&self, on: bool) {
        self.shared.enabled.store(on, Ordering::Relaxed);
    }

    pub fn enabled(&self) -> bool {
        self.shared.enabled.load(Ordering::Relaxed)
    }

    pub fn set_gain(&self, gain: f32) {
        self.shared
            .gain_bits
            .store(gain.clamp(0.0, 4.0).to_bits(), Ordering::Relaxed);
    }

    pub fn gain(&self) -> f32 {
        self.shared.gain()
    }

    /// Names of the available input devices (not enumerated in the browser).
    pub fn devices() -> Vec<String> {
        Vec::new()
    }
}

fn spawn_capture(shared: Arc<Shared>, keep: Arc<Mutex<Vec<JsValue>>>) {
    wasm_bindgen_futures::spawn_local(async move {
        if let Err(error) = start_capture(&shared, &keep).await {
            tracing::warn!("microphone capture unavailable: {error:?}");
            // Ensure no stale sink keeps running.
            *shared.sink.lock().unwrap() = None;
        }
    });
}

async fn start_capture(
    shared: &Arc<Shared>,
    keep: &Arc<Mutex<Vec<JsValue>>>,
) -> Result<(), JsValue> {
    let window = web_sys::window().ok_or(JsValue::NULL)?;
    let media_devices = window.navigator().media_devices()?;

    let constraints = MediaStreamConstraints::new();
    constraints.set_audio(&JsValue::TRUE);
    let stream = JsFuture::from(media_devices.get_user_media_with_constraints(&constraints)?)
        .await?
        .dyn_into::<web_sys::MediaStream>()?;

    let context = AudioContext::new()?;
    let _ = context.resume();
    shared
        .in_rate
        .store(context.sample_rate().max(8_000.0) as u32, Ordering::Relaxed);

    let source = context.create_media_stream_source(&stream)?;

    let base: &web_sys::BaseAudioContext = context.unchecked_ref();
    let processor = base
        .create_script_processor_with_buffer_size_and_number_of_input_channels_and_number_of_output_channels(
            2048, 1, 1,
        )?;

    let callback_shared = Arc::clone(shared);
    let on_audio =
        Closure::<dyn FnMut(AudioProcessingEvent)>::new(move |event: AudioProcessingEvent| {
            callback_shared.process(&event);
        });
    processor.set_onaudioprocess(Some(on_audio.as_ref().unchecked_ref()));
    on_audio.forget();

    // source -> processor -> silent gain -> destination (needed to be pulled).
    let source_node: &AudioNode = source.unchecked_ref();
    let processor_node: &AudioNode = processor.unchecked_ref();
    source_node.connect_with_audio_node(processor_node)?;

    let silence = context.create_gain()?;
    silence.gain().set_value(0.0);
    let silence_node: &AudioNode = silence.unchecked_ref();
    processor_node.connect_with_audio_node(silence_node)?;
    let destination_node = context.destination();
    let destination: &AudioNode = destination_node.unchecked_ref();
    silence_node.connect_with_audio_node(destination)?;

    // Keep the JS objects alive for as long as `MicInput` (or its task) lives.
    let mut keep = keep.lock().unwrap();
    keep.push(context.into());
    keep.push(source.into());
    keep.push(processor.clone().into());
    keep.push(silence.into());
    Ok(())
}
