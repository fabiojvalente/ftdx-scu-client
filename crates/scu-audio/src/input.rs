//! `cpal` microphone capture for TX audio.
//!
//! Captures the default (or a chosen) input device, downmixes to mono,
//! resamples to the network rate (16 kHz), packetizes into 160-frame blocks and
//! hands each encoded 668-byte TX body to a caller-supplied sink. Frames are
//! only produced while the shared `enabled` flag is set (PTT held).
//!
//! The stream callback never allocates or locks: it pushes `f32` samples into a
//! lock-free ring buffer that a worker thread drains.

use std::sync::atomic::{AtomicBool, AtomicU16, AtomicU32, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::Duration;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, Sample, SampleFormat, SizedSample, Stream, StreamConfig};
use ringbuf::traits::{Consumer, Producer, Split};
use ringbuf::HeapRb;
use thiserror::Error;

use crate::resample::ChannelResampler;
use crate::{encode_tx, AUDIO_FRAMES_PER_PACKET, AUDIO_SAMPLE_RATE};

/// Callback invoked with each encoded TX body (668 bytes) from the capture
/// worker thread. Implementations must not block.
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
    #[error("unsupported sample format {0:?}")]
    SampleFormat(SampleFormat),
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

struct Control {
    enabled: AtomicBool,
    gain_bits: AtomicU32,
    stop: AtomicBool,
    /// Most recent input peak, scaled to 0..=32767. Updated even when capture
    /// is disabled so VOX (or a level meter) can observe the live signal.
    level: AtomicU16,
}

impl Control {
    fn gain(&self) -> f32 {
        f32::from_bits(self.gain_bits.load(Ordering::Relaxed))
    }
}

/// A live microphone capture. Dropping it stops capture and joins the worker.
pub struct MicInput {
    control: Arc<Control>,
    info: InputInfo,
    worker: Option<JoinHandle<()>>,
    _stream: Stream,
}

impl MicInput {
    /// Open the configured input device and start capturing.
    pub fn new(config: MicConfig, sink: TxAudioSink) -> Result<Self, InputError> {
        let host = cpal::default_host();
        let device = match &config.device {
            Some(name) => find_input_device(&host, name)?,
            None => host.default_input_device().ok_or(InputError::NoDevice)?,
        };
        let device_name = device.name().unwrap_or_else(|_| "unknown".into());

        let supported = device
            .default_input_config()
            .map_err(|e| InputError::Config(e.to_string()))?;
        let sample_format = supported.sample_format();
        let stream_config: StreamConfig = supported.into();
        let channels = stream_config.channels;
        let rate = stream_config.sample_rate.0;

        // ~0.5 s of headroom; the worker drains continuously.
        let capacity = ((rate as usize * channels as usize) / 2).max(1);
        let (producer, consumer) = HeapRb::<f32>::new(capacity).split();

        let control = Arc::new(Control {
            enabled: AtomicBool::new(false),
            gain_bits: AtomicU32::new(config.gain.clamp(0.0, 4.0).to_bits()),
            stop: AtomicBool::new(false),
            level: AtomicU16::new(0),
        });

        let err_fn = |err| tracing::error!(%err, "microphone stream error");
        let stream = match sample_format {
            SampleFormat::F32 => {
                build_input_stream::<f32, _>(&device, &stream_config, producer, err_fn)
            }
            SampleFormat::I16 => {
                build_input_stream::<i16, _>(&device, &stream_config, producer, err_fn)
            }
            SampleFormat::U16 => {
                build_input_stream::<u16, _>(&device, &stream_config, producer, err_fn)
            }
            other => return Err(InputError::SampleFormat(other)),
        }?;

        stream
            .play()
            .map_err(|e| InputError::Build(e.to_string()))?;

        // Spawn the drain worker only once the stream is live, so an early
        // error can't leave an orphaned thread waiting on a silent ring.
        let worker_control = Arc::clone(&control);
        let worker = std::thread::Builder::new()
            .name("scu-audio-capture".into())
            .spawn(move || worker_loop(consumer, channels as usize, rate, worker_control, sink))
            .map_err(|e| InputError::Build(e.to_string()))?;

        Ok(Self {
            control,
            info: InputInfo {
                device_name,
                sample_rate: rate,
                channels,
            },
            worker: Some(worker),
            _stream: stream,
        })
    }

    pub fn info(&self) -> &InputInfo {
        &self.info
    }

    /// Gate capture. While `false`, no TX packets are produced.
    pub fn set_enabled(&self, on: bool) {
        self.control.enabled.store(on, Ordering::Relaxed);
    }

    pub fn enabled(&self) -> bool {
        self.control.enabled.load(Ordering::Relaxed)
    }

    pub fn set_gain(&self, gain: f32) {
        self.control
            .gain_bits
            .store(gain.clamp(0.0, 4.0).to_bits(), Ordering::Relaxed);
    }

    pub fn gain(&self) -> f32 {
        self.control.gain()
    }

    /// Most recent input peak (0..=32767), available even while capture is
    /// disabled so the caller can implement VOX.
    pub fn level(&self) -> u16 {
        self.control.level.load(Ordering::Relaxed)
    }

    /// Names of the available input devices.
    pub fn devices() -> Vec<String> {
        let host = cpal::default_host();
        match host.input_devices() {
            Ok(devices) => devices.filter_map(|d| d.name().ok()).collect(),
            Err(_) => Vec::new(),
        }
    }
}

impl Drop for MicInput {
    fn drop(&mut self) {
        self.control.stop.store(true, Ordering::Relaxed);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

fn find_input_device(host: &cpal::Host, name: &str) -> Result<cpal::Device, InputError> {
    let devices = host
        .input_devices()
        .map_err(|e| InputError::Config(e.to_string()))?;
    for device in devices {
        if device.name().ok().as_deref() == Some(name) {
            return Ok(device);
        }
    }
    Err(InputError::DeviceNotFound(name.to_string()))
}

fn build_input_stream<T, P>(
    device: &cpal::Device,
    config: &StreamConfig,
    mut producer: P,
    err_fn: impl FnMut(cpal::StreamError) + Send + 'static,
) -> Result<Stream, InputError>
where
    T: SizedSample,
    f32: FromSample<T>,
    P: Producer<Item = f32> + Send + 'static,
{
    device
        .build_input_stream(
            config,
            move |data: &[T], _| {
                for &sample in data {
                    let value = f32::from_sample(sample);
                    let _ = producer.try_push(value.clamp(-1.0, 1.0));
                }
            },
            err_fn,
            None,
        )
        .map_err(|e| InputError::Build(e.to_string()))
}

fn worker_loop<C: Consumer<Item = f32>>(
    mut consumer: C,
    src_channels: usize,
    src_rate: u32,
    control: Arc<Control>,
    sink: TxAudioSink,
) {
    let src_channels = src_channels.max(1);
    let src_rate = if src_rate == 0 {
        AUDIO_SAMPLE_RATE
    } else {
        src_rate
    };
    let mut resampler = ChannelResampler::new(src_rate as f64 / AUDIO_SAMPLE_RATE as f64);

    let mut scratch = vec![0f32; 16_384];
    let mut raw: Vec<f32> = Vec::new();
    let mut pending: Vec<f32> = Vec::with_capacity(AUDIO_FRAMES_PER_PACKET * 2);
    let mut seq: u8 = 0;

    loop {
        if control.stop.load(Ordering::Relaxed) {
            break;
        }

        let n = consumer.pop_slice(&mut scratch);
        if n == 0 {
            std::thread::sleep(Duration::from_millis(2));
            continue;
        }

        // Publish the input peak before the enabled gate so VOX can key the
        // transmitter while capture is otherwise idle.
        let peak = scratch[..n]
            .iter()
            .fold(0.0f32, |acc, sample| acc.max(sample.abs()));
        control
            .level
            .store((peak.clamp(0.0, 1.0) * 32767.0) as u16, Ordering::Relaxed);

        if !control.enabled.load(Ordering::Relaxed) {
            pending.clear();
            raw.clear();
            continue;
        }

        raw.extend_from_slice(&scratch[..n]);

        let frames = raw.len() / src_channels;
        if frames > 0 {
            let mut mono = Vec::with_capacity(frames);
            for i in 0..frames {
                let base = i * src_channels;
                let mut sum = 0.0f32;
                for c in 0..src_channels {
                    sum += raw[base + c];
                }
                mono.push(sum / src_channels as f32);
            }
            raw.drain(0..frames * src_channels);

            let mut resampled = Vec::with_capacity(frames + 4);
            resampler.process(&mono, &mut resampled);

            let gain = control.gain();
            pending.extend(resampled.iter().map(|&s| (s * gain).clamp(-1.0, 1.0)));
        }

        while pending.len() >= AUDIO_FRAMES_PER_PACKET {
            let mut stereo = [0i16; AUDIO_FRAMES_PER_PACKET * 2];
            for (i, &s) in pending[..AUDIO_FRAMES_PER_PACKET].iter().enumerate() {
                let value = (s * 32767.0) as i16;
                stereo[i * 2] = value;
                stereo[i * 2 + 1] = value;
            }
            pending.drain(0..AUDIO_FRAMES_PER_PACKET);

            let body = encode_tx(seq, &stereo);
            seq = seq.wrapping_add(1);
            sink(&body);
        }
    }
}
