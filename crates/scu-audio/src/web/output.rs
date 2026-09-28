//! Browser RX playback via the Web Audio API.
//!
//! Network audio (16 kHz) is decoded, resampled to the `AudioContext` rate and
//! queued as interleaved stereo `f32`. A `ScriptProcessorNode` drains the queue
//! into the output. Volume/mute are handled by a `GainNode`; channel routing by
//! the queue builder.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

use thiserror::Error;
use wasm_bindgen::prelude::*;
use web_sys::{
    AudioBuffer, AudioContext, AudioNode, AudioParam, AudioProcessingEvent, ScriptProcessorNode,
};

use crate::resample::ChannelResampler;
use crate::AudioFrame;

#[derive(Debug, Error)]
pub enum OutputError {
    #[error("Web Audio unavailable: {0}")]
    Unavailable(String),
    #[error("no default output device")]
    NoDevice,
    #[error("device does not support a usable output config: {0}")]
    Config(String),
    #[error("failed to build output stream: {0}")]
    Build(String),
}

/// Metadata about the opened output device.
#[derive(Debug, Clone)]
pub struct OutputInfo {
    pub device_name: String,
    pub sample_rate: u32,
    pub channels: u16,
}

/// Interleaved stereo frames held before playback.
const RING_FRAMES: usize = 1 << 15;

struct ResamplerState {
    channels: usize,
    rate: u32,
    resamplers: Vec<ChannelResampler>,
}

struct Shared {
    ring: Mutex<VecDeque<f32>>,
    enabled: AtomicBool,
    volume_bits: AtomicU32,
    stereo: AtomicBool,
    out_rate: u32,
    resamplers: Mutex<ResamplerState>,
}

impl Shared {
    fn volume(&self) -> f32 {
        f32::from_bits(self.volume_bits.load(Ordering::Relaxed))
    }

    fn gain(&self) -> f32 {
        if self.enabled.load(Ordering::Relaxed) {
            self.volume()
        } else {
            0.0
        }
    }

    fn push(&self, frame: AudioFrame) {
        if frame.samples.is_empty() {
            return;
        }
        let sc = frame.channels.max(1) as usize;
        let in_rate = if frame.sample_rate == 0 {
            16_000
        } else {
            frame.sample_rate
        };

        {
            let mut state = self.resamplers.lock().unwrap();
            if state.channels != sc || state.rate != in_rate {
                let step = in_rate as f64 / self.out_rate as f64;
                state.channels = sc;
                state.rate = in_rate;
                state.resamplers = (0..sc).map(|_| ChannelResampler::new(step)).collect();
            }
        }

        let frames = frame.samples.len() / sc;
        let mut channels: Vec<Vec<f32>> = vec![Vec::with_capacity(frames); sc];
        for (i, &sample) in frame.samples.iter().enumerate() {
            channels[i % sc].push(sample);
        }

        let mut resampled: Vec<Vec<f32>> = Vec::with_capacity(sc);
        {
            let mut state = self.resamplers.lock().unwrap();
            for (channel, resampler) in channels.iter().zip(state.resamplers.iter_mut()) {
                let mut out = Vec::with_capacity(channel.len() + 4);
                resampler.process(channel, &mut out);
                resampled.push(out);
            }
        }

        let Some(first) = resampled.first() else {
            return;
        };
        let stereo = self.stereo.load(Ordering::Relaxed);
        let mut ring = self.ring.lock().unwrap();
        for f in 0..first.len() {
            let left = resampled[0][f];
            let right = if !stereo || sc == 1 {
                left
            } else {
                resampled[1][f]
            };
            ring.push_back(left);
            ring.push_back(right);
        }
        let capacity = RING_FRAMES * 2;
        while ring.len() > capacity {
            ring.pop_front();
        }
    }

    fn drain_into(&self, output: &AudioBuffer) {
        let channels = output.number_of_channels();
        let length = output.length() as usize;
        let Some(left) = output_channel(output, 0) else {
            return;
        };
        let right = output_channel(output, 1);
        let mut ring = self.ring.lock().unwrap();
        for i in 0..length {
            let l = ring.pop_front().unwrap_or(0.0);
            let r = ring.pop_front().unwrap_or(0.0);
            left.set_index(i as u32, l);
            if let Some(right) = &right {
                right.set_index(i as u32, r);
            }
        }
        let _ = channels;
    }
}

/// The `getChannelData` web-sys binding returns a copy; call it through JS to
/// get the live `Float32Array` backing the output buffer.
fn output_channel(buffer: &AudioBuffer, index: u32) -> Option<js_sys::Float32Array> {
    let object: &JsValue = buffer.as_ref();
    let function = js_sys::Reflect::get(object, &JsValue::from_str("getChannelData")).ok()?;
    let function = function.dyn_into::<js_sys::Function>().ok()?;
    let data = function
        .call1(object, &JsValue::from_f64(index as f64))
        .ok()?;
    data.dyn_into::<js_sys::Float32Array>().ok()
}

/// A `Send` handle for feeding decoded audio to a live [`AudioOutput`] from the
/// engine task.
#[derive(Clone)]
pub struct AudioSink {
    shared: Arc<Shared>,
}

impl AudioSink {
    /// Queue a decoded RX frame. Drops the frame if the queue is saturated.
    pub fn push(&self, frame: AudioFrame) {
        self.shared.push(frame);
    }
}

/// A live audio output. Dropping it stops playback.
pub struct AudioOutput {
    shared: Arc<Shared>,
    gain: AudioParam,
    _context: AudioContext,
    _node: ScriptProcessorNode,
    _on_audio: Closure<dyn FnMut(AudioProcessingEvent)>,
    info: OutputInfo,
}

impl AudioOutput {
    /// Open an `AudioContext` and start playback.
    pub fn new() -> Result<Self, OutputError> {
        let context =
            AudioContext::new().map_err(|e| OutputError::Unavailable(format!("{e:?}")))?;
        let _ = context.resume();

        let out_rate = context.sample_rate().max(8_000.0) as u32;
        let shared = Arc::new(Shared {
            ring: Mutex::new(VecDeque::new()),
            enabled: AtomicBool::new(true),
            volume_bits: AtomicU32::new(1.0f32.to_bits()),
            stereo: AtomicBool::new(false),
            out_rate,
            resamplers: Mutex::new(ResamplerState {
                channels: 0,
                rate: 0,
                resamplers: Vec::new(),
            }),
        });

        let base: &web_sys::BaseAudioContext = context.unchecked_ref();
        let node = base
            .create_script_processor_with_buffer_size_and_number_of_input_channels_and_number_of_output_channels(
                2048, 0, 2,
            )
            .map_err(|e| OutputError::Build(format!("{e:?}")))?;

        let callback_shared = Arc::clone(&shared);
        let on_audio =
            Closure::<dyn FnMut(AudioProcessingEvent)>::new(move |event: AudioProcessingEvent| {
                if let Ok(output) = event.output_buffer() {
                    callback_shared.drain_into(&output);
                }
            });
        node.set_onaudioprocess(Some(on_audio.as_ref().unchecked_ref()));

        let gain_node = context
            .create_gain()
            .map_err(|e| OutputError::Build(format!("{e:?}")))?;
        let gain = gain_node.gain();
        gain.set_value(shared.gain());

        let node_audio: &AudioNode = node.unchecked_ref();
        let gain_audio: &AudioNode = gain_node.unchecked_ref();
        node_audio
            .connect_with_audio_node(gain_audio)
            .map_err(|e| OutputError::Build(format!("{e:?}")))?;
        let destination_node = context.destination();
        let destination: &AudioNode = destination_node.unchecked_ref();
        gain_audio
            .connect_with_audio_node(destination)
            .map_err(|e| OutputError::Build(format!("{e:?}")))?;

        Ok(Self {
            shared,
            gain,
            _context: context,
            _node: node,
            _on_audio: on_audio,
            info: OutputInfo {
                device_name: "Web Audio".into(),
                sample_rate: out_rate,
                channels: 2,
            },
        })
    }

    pub fn info(&self) -> &OutputInfo {
        &self.info
    }

    /// Queue a decoded RX frame. Drops the frame if the queue is saturated.
    pub fn push(&self, frame: AudioFrame) {
        self.shared.push(frame);
    }

    /// A `Send` handle for feeding this output from the engine task.
    pub fn sink(&self) -> AudioSink {
        AudioSink {
            shared: Arc::clone(&self.shared),
        }
    }

    /// When `true`, source channel 1 is routed to the right output; when `false`
    /// (default), channel 0 is duplicated across all output channels.
    pub fn set_stereo(&self, on: bool) {
        self.shared.stereo.store(on, Ordering::Relaxed);
    }

    pub fn stereo(&self) -> bool {
        self.shared.stereo.load(Ordering::Relaxed)
    }

    pub fn set_enabled(&self, on: bool) {
        self.shared.enabled.store(on, Ordering::Relaxed);
        self.gain.set_value(self.shared.gain());
    }

    pub fn enabled(&self) -> bool {
        self.shared.enabled.load(Ordering::Relaxed)
    }

    pub fn set_volume(&self, volume: f32) {
        self.shared
            .volume_bits
            .store(volume.clamp(0.0, 2.0).to_bits(), Ordering::Relaxed);
        self.gain.set_value(self.shared.gain());
    }

    pub fn volume(&self) -> f32 {
        self.shared.volume()
    }
}
