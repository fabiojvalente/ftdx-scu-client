//! `cpal` audio playback with a streaming linear resampler.
//!
//! Network audio arrives at 16 kHz; the output device usually runs at
//! 44.1/48 kHz. A worker thread deinterleaves, resamples, applies gain, and
//! pushes interleaved `f32` into a lock-free ring buffer consumed by the audio
//! callback (which never allocates or locks).

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, SampleFormat, SizedSample, Stream, StreamConfig};
use ringbuf::traits::{Consumer, Producer, Split};
use ringbuf::HeapRb;
use thiserror::Error;

use crate::resample::ChannelResampler;
use crate::AudioFrame;

#[derive(Debug, Error)]
pub enum OutputError {
    #[error("no default output device")]
    NoDevice,
    #[error("device does not support a usable output config: {0}")]
    Config(String),
    #[error("failed to build output stream: {0}")]
    Build(String),
    #[error("unsupported sample format {0:?}")]
    SampleFormat(SampleFormat),
}

/// Metadata about the opened output device.
#[derive(Debug, Clone)]
pub struct OutputInfo {
    pub device_name: String,
    pub sample_rate: u32,
    pub channels: u16,
}

struct Control {
    enabled: AtomicBool,
    volume_bits: AtomicU32,
    stereo: AtomicBool,
}

impl Control {
    fn volume(&self) -> f32 {
        f32::from_bits(self.volume_bits.load(Ordering::Relaxed))
    }
}

/// A `Send` handle for feeding decoded audio to a live [`AudioOutput`] from
/// another thread. The output's `cpal` stream stays on the thread that created it.
#[derive(Clone)]
pub struct AudioSink {
    tx: crossbeam_channel::Sender<AudioFrame>,
}

impl AudioSink {
    /// Queue a decoded RX frame. Drops the frame if the queue is saturated.
    pub fn push(&self, frame: AudioFrame) {
        let _ = self.tx.try_send(frame);
    }
}

/// A live audio output. Dropping it stops playback.
pub struct AudioOutput {
    tx: crossbeam_channel::Sender<AudioFrame>,
    control: Arc<Control>,
    _worker: JoinHandle<()>,
    _stream: Stream,
    info: OutputInfo,
}

impl AudioOutput {
    /// Open the default output device and start playback.
    pub fn new() -> Result<Self, OutputError> {
        let host = cpal::default_host();
        let device = host.default_output_device().ok_or(OutputError::NoDevice)?;
        let device_name = device.name().unwrap_or_else(|_| "unknown".into());

        let supported = choose_config(&device)?;
        let sample_format = supported.sample_format();
        let config: StreamConfig = supported.into();
        let out_channels = config.channels;
        let out_rate = config.sample_rate.0;

        let capacity = (out_rate as usize) * (out_channels as usize) * 2;
        let (producer, consumer) = HeapRb::<f32>::new(capacity).split();

        let control = Arc::new(Control {
            enabled: AtomicBool::new(true),
            volume_bits: AtomicU32::new(1.0f32.to_bits()),
            stereo: AtomicBool::new(false),
        });

        let worker_control = Arc::clone(&control);
        let (tx, rx) = crossbeam_channel::bounded::<AudioFrame>(64);
        let worker = std::thread::Builder::new()
            .name("scu-audio-resample".into())
            .spawn(move || worker_loop(rx, producer, out_rate, out_channels, worker_control))
            .map_err(|e| OutputError::Build(e.to_string()))?;

        let err_fn = |err| tracing::error!(%err, "audio stream error");
        let stream = match sample_format {
            SampleFormat::F32 => build_stream::<f32, _>(&device, &config, consumer, err_fn),
            SampleFormat::I16 => build_stream::<i16, _>(&device, &config, consumer, err_fn),
            SampleFormat::U16 => build_stream::<u16, _>(&device, &config, consumer, err_fn),
            other => return Err(OutputError::SampleFormat(other)),
        }?;

        stream
            .play()
            .map_err(|e| OutputError::Build(e.to_string()))?;

        Ok(Self {
            tx,
            control,
            _worker: worker,
            _stream: stream,
            info: OutputInfo {
                device_name,
                sample_rate: out_rate,
                channels: out_channels,
            },
        })
    }

    pub fn info(&self) -> &OutputInfo {
        &self.info
    }

    /// Queue a decoded RX frame. Drops the frame if the queue is saturated.
    pub fn push(&self, frame: AudioFrame) {
        let _ = self.tx.try_send(frame);
    }

    /// A `Send` handle for feeding this output from another thread.
    pub fn sink(&self) -> AudioSink {
        AudioSink {
            tx: self.tx.clone(),
        }
    }

    /// When `true`, source channel 1 is routed to the right output; when `false`
    /// (default), channel 0 is duplicated across all output channels.
    pub fn set_stereo(&self, on: bool) {
        self.control.stereo.store(on, Ordering::Relaxed);
    }

    pub fn stereo(&self) -> bool {
        self.control.stereo.load(Ordering::Relaxed)
    }

    pub fn set_enabled(&self, on: bool) {
        self.control.enabled.store(on, Ordering::Relaxed);
    }

    pub fn enabled(&self) -> bool {
        self.control.enabled.load(Ordering::Relaxed)
    }

    pub fn set_volume(&self, volume: f32) {
        self.control
            .volume_bits
            .store(volume.clamp(0.0, 2.0).to_bits(), Ordering::Relaxed);
    }

    pub fn volume(&self) -> f32 {
        self.control.volume()
    }
}

fn choose_config(device: &cpal::Device) -> Result<cpal::SupportedStreamConfig, OutputError> {
    // Prefer a config that runs at the source rate (16 kHz) to avoid resampling.
    if let Ok(ranges) = device.supported_output_configs() {
        for range in ranges {
            if range.min_sample_rate().0 <= 16_000 && 16_000 <= range.max_sample_rate().0 {
                return Ok(range.with_sample_rate(cpal::SampleRate(16_000)));
            }
        }
    }
    device
        .default_output_config()
        .map_err(|e| OutputError::Config(e.to_string()))
}

fn build_stream<T, C>(
    device: &cpal::Device,
    config: &StreamConfig,
    mut consumer: C,
    err_fn: impl FnMut(cpal::StreamError) + Send + 'static,
) -> Result<Stream, OutputError>
where
    T: SizedSample + FromSample<f32>,
    C: Consumer<Item = f32> + Send + 'static,
{
    device
        .build_output_stream(
            config,
            move |data: &mut [T], _| {
                for slot in data.iter_mut() {
                    let value = consumer.try_pop().unwrap_or(0.0);
                    *slot = T::from_sample(value);
                }
            },
            err_fn,
            None,
        )
        .map_err(|e| OutputError::Build(e.to_string()))
}

fn worker_loop<P: Producer<Item = f32>>(
    rx: crossbeam_channel::Receiver<AudioFrame>,
    mut producer: P,
    out_rate: u32,
    out_channels: u16,
    control: Arc<Control>,
) {
    let mut src_channels = 0usize;
    let mut resamplers: Vec<ChannelResampler> = Vec::new();
    let out_ch = out_channels.max(1) as usize;

    while let Ok(frame) = rx.recv() {
        if frame.samples.is_empty() {
            continue;
        }
        let sc = frame.channels.max(1) as usize;
        let in_rate = if frame.sample_rate == 0 {
            16_000
        } else {
            frame.sample_rate
        };

        if sc != src_channels {
            src_channels = sc;
            let step = in_rate as f64 / out_rate as f64;
            resamplers = (0..sc).map(|_| ChannelResampler::new(step)).collect();
        }

        let frames = frame.samples.len() / sc;
        let mut channels: Vec<Vec<f32>> = vec![Vec::with_capacity(frames); sc];
        for (i, &s) in frame.samples.iter().enumerate() {
            channels[i % sc].push(s);
        }

        let mut resampled: Vec<Vec<f32>> = Vec::with_capacity(sc);
        for (ch, resampler) in channels.iter().zip(resamplers.iter_mut()) {
            let mut out = Vec::with_capacity(frames + 4);
            resampler.process(ch, &mut out);
            resampled.push(out);
        }

        let Some(first) = resampled.first() else {
            continue;
        };
        let n_out = first.len();
        let enabled = control.enabled.load(Ordering::Relaxed);
        let volume = control.volume();
        let stereo = control.stereo.load(Ordering::Relaxed);

        for (f, _) in first.iter().enumerate().take(n_out) {
            let mono = resampled[0][f];
            #[allow(clippy::needless_range_loop)]
            for j in 0..out_ch {
                // ch0 is the receiver speaker; ch1 is near-silent RX / TX monitor.
                let value = if !stereo {
                    mono
                } else if j < sc {
                    resampled[j][f]
                } else if sc == 1 {
                    mono
                } else {
                    0.0
                };
                let value = if enabled {
                    (value * volume).clamp(-1.0, 1.0)
                } else {
                    0.0
                };
                let _ = producer.try_push(value);
            }
        }
    }
}
