//! The egui application: connection, rig controls, audio, spectrum, waterfall.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use web_time::Instant;

#[cfg(not(target_arch = "wasm32"))]
use std::path::PathBuf;

use eframe::egui;
use scu_audio::input::{MicConfig, MicInput, TxAudioSink};
use scu_audio::output::{AudioOutput, AudioSink};
use scu_cat::{self, Agc, MeterKind, Mode, RadioModel, ScopeMode};
use scu_client::{ConnectConfig, Event, ScuClient, ScuHandle};
use scu_scope::{BinInterleave, Colormap, FrequencyAxis};
use serde::{Deserialize, Serialize};

use crate::theme::{self, ACCENT, FREQ_CYAN, ON_ACCENT, OUTLINE, RX_GREEN, SPECTRUM_GREEN, TEXT, TEXT_DIM, TEXT_FAINT, TX_RED};
use crate::waterfall::Waterfall;

/// Messages from the background session engine to the UI.
enum EngineMsg {
    Handle(ScuHandle),
    Event(Event),
    Failed(String),
    Stopped,
}

/// UI preferences, persisted separately from the connection config.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppSettings {
    /// RM indices (`MeterKind::from_rm_index`) shown in the side rail.
    #[serde(default = "default_visible_meters")]
    pub visible_meters: Vec<u8>,
    #[serde(default = "default_true")]
    pub show_cat_console: bool,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            visible_meters: default_visible_meters(),
            show_cat_console: true,
        }
    }
}

fn default_visible_meters() -> Vec<u8> {
    METER_INDICES.to_vec()
}

fn default_true() -> bool {
    true
}

/// RM meter indices offered by the FTDX10 (`RM3` ..= `RM9`).
const METER_INDICES: [u8; 7] = [3, 4, 5, 6, 7, 8, 9];

pub struct ScuApp {
    config: ConnectConfig,

    handle: Option<ScuHandle>,
    audio: Option<AudioOutput>,
    engine_rx: Option<std::sync::mpsc::Receiver<EngineMsg>>,
    shutdown: Option<Arc<AtomicBool>>,
    connecting: bool,

    status: String,
    radio: Option<RadioModel>,
    frequency: u64,
    frequency_b: u64,
    rx_sub: bool,
    split: bool,
    rit_on: bool,
    xit_on: bool,
    rit_offset_hz: i32,
    xit_offset_hz: i32,
    noise_blanker: bool,
    noise_reduction: bool,
    auto_notch: bool,
    narrow: bool,
    agc: Option<Agc>,
    rf_gain: u8,
    squelch: u8,
    mode: Option<Mode>,
    smeter: u8,
    meters: [Option<u8>; 10],
    span_hz: f64,
    scope_mode: Option<ScopeMode>,
    follow_vfo: bool,
    scope_centered_sent: bool,

    latest_bins: Vec<f32>,
    waterfall: Waterfall,
    scope_bins: BinInterleave,

    /// Editable frequency text, indexed by VFO (0 = A/Main, 1 = B/Sub).
    freq_input: [String; 2],
    freq_editing: [bool; 2],
    freq_step_hz: u64,
    cat_input: String,
    cat_log: Vec<String>,

    volume: f32,
    muted: bool,
    stereo: bool,

    ptt: bool,
    tx_power: u16,
    radio_mic_gain: u8,
    atu_on: bool,
    mic: Option<MicInput>,
    mic_devices: Vec<String>,
    mic_device: Option<String>,
    mic_gain: f32,
    mic_status: String,

    settings: AppSettings,
    show_settings: bool,

    last_poll: Instant,
}

impl ScuApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        theme::apply(&cc.egui_ctx);
        #[allow(unused_mut)]
        let mut config = load_config().unwrap_or_default();
        #[cfg(target_arch = "wasm32")]
        if config.bridge_url.is_none() {
            config.bridge_url = Some("ws://127.0.0.1:9000".into());
        }
        let freq_input = [
            freq_input_text(config_default_freq()),
            freq_input_text(config_default_freq()),
        ];
        let settings = load_settings().unwrap_or_default();
        Self {
            config,
            handle: None,
            audio: None,
            engine_rx: None,
            shutdown: None,
            connecting: false,
            status: "disconnected".into(),
            radio: None,
            frequency: 0,
            frequency_b: 0,
            rx_sub: false,
            split: false,
            rit_on: false,
            xit_on: false,
            rit_offset_hz: 0,
            xit_offset_hz: 0,
            noise_blanker: false,
            noise_reduction: false,
            auto_notch: false,
            narrow: false,
            agc: None,
            rf_gain: 128,
            squelch: 0,
            mode: None,
            smeter: 0,
            meters: [None; 10],
            span_hz: 200_000.0,
            scope_mode: None,
            follow_vfo: true,
            scope_centered_sent: false,
            latest_bins: Vec::new(),
            waterfall: Waterfall::new(1024, 480),
            scope_bins: BinInterleave::Split,
            freq_input,
            freq_editing: [false, false],
            freq_step_hz: 1_000,
            cat_input: String::new(),
            cat_log: Vec::new(),
            volume: 1.0,
            muted: false,
            stereo: false,
            ptt: false,
            tx_power: scu_cat::POWER_MAX_W,
            radio_mic_gain: 50,
            atu_on: false,
            mic: None,
            mic_devices: MicInput::devices(),
            mic_device: None,
            mic_gain: 1.0,
            mic_status: String::new(),
            settings,
            show_settings: false,
            last_poll: Instant::now(),
        }
    }

    fn connect(&mut self, ctx: egui::Context) {
        if self.connecting || self.engine_rx.is_some() {
            return;
        }
        save_config(&self.config);
        self.ensure_audio();

        let config = self.config.clone();
        let sink = self.audio.as_ref().map(|a| a.sink());
        let (tx, rx) = std::sync::mpsc::channel();
        let shutdown = Arc::new(AtomicBool::new(false));

        self.engine_rx = Some(rx);
        self.shutdown = Some(Arc::clone(&shutdown));
        self.connecting = true;
        self.status = "connecting...".into();

        if start_engine(config, sink, tx, shutdown, ctx).is_err() {
            self.connecting = false;
            self.engine_rx = None;
            self.shutdown = None;
            self.status = "failed to start engine".into();
        }
    }

    fn disconnect(&mut self) {
        if let Some(flag) = &self.shutdown {
            flag.store(true, Ordering::Relaxed);
        }
        self.stop_tx();
        self.shutdown = None;
        self.engine_rx = None;
        self.handle = None;
        self.connecting = false;
        self.status = "disconnected".into();
    }

    /// Unkey the radio (if needed) and release the microphone.
    fn stop_tx(&mut self) {
        if self.ptt {
            if let Some(handle) = &self.handle {
                handle.set_transmit(false);
            }
        }
        self.ptt = false;
        self.mic = None;
        self.mic_status.clear();
        self.sync_audio_enabled();
    }

    fn ensure_audio(&mut self) {
        if self.audio.is_some() {
            return;
        }
        match AudioOutput::new() {
            Ok(output) => {
                output.set_volume(self.volume);
                output.set_enabled(!self.muted && !self.ptt);
                output.set_stereo(self.stereo);
                self.audio = Some(output);
            }
            Err(e) => {
                self.status = format!("audio unavailable: {e}");
            }
        }
    }

    /// RX playback is silenced while muted or transmitting (avoids the mic
    /// picking up the speakers).
    fn sync_audio_enabled(&self) {
        if let Some(audio) = &self.audio {
            audio.set_enabled(!self.muted && !self.ptt);
        }
    }

    /// Open the configured microphone and route its encoded TX audio to the
    /// session. Called once the session handle is available.
    fn ensure_mic(&mut self, handle: ScuHandle) {
        if self.mic.is_some() {
            return;
        }
        let sink: TxAudioSink = Box::new(move |body: &[u8]| handle.send_tx_audio(body));
        let config = MicConfig {
            device: self.mic_device.clone(),
            gain: self.mic_gain,
        };
        match MicInput::new(config, sink) {
            Ok(mic) => {
                mic.set_enabled(self.ptt);
                self.mic_status = format!(
                    "mic: {} ({} Hz, {} ch)",
                    mic.info().device_name,
                    mic.info().sample_rate,
                    mic.info().channels
                );
                self.mic = Some(mic);
            }
            Err(e) => self.mic_status = format!("mic unavailable: {e}"),
        }
    }

    /// Key (`on = true`) or unkey the transmitter and gate mic capture.
    fn set_ptt(&mut self, on: bool) {
        if self.ptt == on {
            return;
        }
        self.ptt = on;
        if let Some(mic) = &self.mic {
            mic.set_enabled(on);
        }
        self.sync_audio_enabled();
        if let Some(handle) = &self.handle {
            handle.set_transmit(on);
            self.push_log(format!("> {}", scu_cat::transmit(on)));
        }
    }

    fn initial_queries(&self) {
        if let Some(handle) = &self.handle {
            for cmd in [
                "ID;", "FA;", "FB;", "FR;", "FT;", "ST;", "MD0;", "SM0;", "PC;", "MG;", "AC;",
                "RT;", "XT;", "RC0;", "RC1;", "NB;", "NR;", "BC;", "NA0;", "GT0;", "RG0;", "SQ0;",
                "AI1;", "SS05;", "SS06;",
            ] {
                handle.send_cat(cmd);
            }
        }
    }

    /// Drain messages from the session engine. Audio never comes through here
    /// (the engine feeds it straight to the audio sink).
    fn drain_engine(&mut self) {
        let mut messages = Vec::new();
        if let Some(rx) = &self.engine_rx {
            while let Ok(message) = rx.try_recv() {
                messages.push(message);
            }
        }
        for message in messages {
            self.on_engine_msg(message);
        }
    }

    fn on_engine_msg(&mut self, message: EngineMsg) {
        match message {
            EngineMsg::Handle(handle) => {
                self.mic_devices = MicInput::devices();
                self.ensure_mic(handle.clone());
                self.handle = Some(handle);
                self.connecting = false;
                self.scope_centered_sent = false;
                self.meters = [None; 10];
                self.status = "connected".into();
                self.initial_queries();
            }
            EngineMsg::Event(event) => self.handle_event(event),
            EngineMsg::Failed(error) => {
                self.connecting = false;
                self.engine_rx = None;
                self.shutdown = None;
                self.stop_tx();
                self.status = format!("connect failed: {error}");
            }
            EngineMsg::Stopped => {
                self.connecting = false;
                self.engine_rx = None;
                self.shutdown = None;
                if self.handle.is_some() {
                    self.status = "disconnected".into();
                }
                self.handle = None;
                self.stop_tx();
            }
        }
    }

    fn handle_event(&mut self, event: Event) {
        match event {
            Event::Connected { session_id } => {
                self.status = format!("connected (session 0x{session_id:02X})");
                self.initial_queries();
            }
            Event::Radio(model) => self.radio = Some(model),
            Event::Cat(text) => self.on_cat(&text),
            Event::Audio(_) => {}
            Event::Scope(body) => {
                let line = scu_scope::decode_with(&body, self.scope_bins);
                self.waterfall.push(&line.bins);
                self.latest_bins = line.bins;
            }
            Event::Error(message) => self.push_log(format!("! {message}")),
            Event::Disconnected(reason) => {
                self.status = format!("disconnected: {reason}");
                self.handle = None;
                self.stop_tx();
            }
        }
    }

    fn on_cat(&mut self, text: &str) {
        self.push_log(text.to_string());
        let Some(frame) = scu_cat::split(text) else {
            return;
        };
        match frame.command {
            "FA" => {
                if let Some(hz) = scu_cat::parse_frequency(text) {
                    self.frequency = hz;
                    // Don't clobber the field while the user is typing in it.
                    if !self.freq_editing[0] {
                        self.freq_input[0] = freq_input_text(hz);
                    }
                }
            }
            "FB" => {
                if let Some(hz) = scu_cat::parse_frequency(text) {
                    self.frequency_b = hz;
                    if !self.freq_editing[1] {
                        self.freq_input[1] = freq_input_text(hz);
                    }
                }
            }
            "FR" => {
                if let Some(sub) = scu_cat::parse_rx_vfo(text) {
                    self.rx_sub = sub;
                }
            }
            "ST" => {
                if let Some(on) = scu_cat::parse_split(text) {
                    self.split = on;
                }
            }
            "RT" => {
                if let Some(on) = scu_cat::parse_rit(text) {
                    self.rit_on = on;
                }
            }
            "XT" => {
                if let Some(on) = scu_cat::parse_xit(text) {
                    self.xit_on = on;
                }
            }
            "RC" => {
                if let Some((tx, hz)) = scu_cat::parse_clarifier(text) {
                    if tx {
                        self.xit_offset_hz = hz;
                    } else {
                        self.rit_offset_hz = hz;
                    }
                }
            }
            "NB" => {
                if let Some(on) = scu_cat::parse_noise_blanker(text) {
                    self.noise_blanker = on;
                }
            }
            "NR" => {
                if let Some(on) = scu_cat::parse_noise_reduction(text) {
                    self.noise_reduction = on;
                }
            }
            "BC" => {
                if let Some(on) = scu_cat::parse_auto_notch(text) {
                    self.auto_notch = on;
                }
            }
            "NA" => {
                if let Some(on) = scu_cat::parse_narrow(text) {
                    self.narrow = on;
                }
            }
            "GT" => {
                if let Some(agc) = scu_cat::parse_agc(text) {
                    self.agc = Some(agc);
                }
            }
            "RG" => {
                if let Some(value) = scu_cat::parse_rf_gain(text) {
                    self.rf_gain = value;
                }
            }
            "SQ" => {
                if let Some(value) = scu_cat::parse_squelch(text) {
                    self.squelch = value;
                }
            }
            "MD" => {
                if let Some(mode) = scu_cat::parse_mode(text) {
                    self.mode = Some(mode);
                }
            }
            "PC" => {
                if let Some(watts) = scu_cat::parse_power(text) {
                    self.tx_power = watts.clamp(scu_cat::POWER_MIN_W, scu_cat::POWER_MAX_W);
                }
            }
            "MG" => {
                if let Some(percent) = scu_cat::parse_mic_gain(text) {
                    self.radio_mic_gain = percent.min(scu_cat::MIC_GAIN_MAX);
                }
            }
            "AC" => {
                if let Some(on) = scu_cat::parse_atu(text) {
                    self.atu_on = on;
                }
            }
            "SM" => {
                if let Some(value) = scu_cat::parse_smeter(text) {
                    self.smeter = value;
                }
            }
            "RM" => {
                if let Some((index, value)) = scu_cat::parse_meter(text) {
                    if let Some(slot) = self.meters.get_mut(index as usize) {
                        *slot = Some(value.min(255) as u8);
                    }
                }
            }
            "SS" => self.on_scope_settings(text),
            _ => {}
        }
    }

    fn on_scope_settings(&mut self, text: &str) {
        if let Some(index) = scu_cat::parse_scope_span_index(text) {
            if let Some(span) = scu_cat::SCOPE_SPANS_HZ.get(index) {
                self.span_hz = *span;
            }
            return;
        }
        if let Some(mode) = scu_cat::parse_scope_mode(text) {
            self.scope_mode = Some(mode);
            // A FIX/CURSOR scope won't track the VFO; switch to CENTER once.
            if self.follow_vfo && !mode.is_center() && !self.scope_centered_sent {
                self.scope_centered_sent = true;
                if let Some(handle) = &self.handle {
                    handle.send_cat(&scu_cat::set_scope_mode(mode.with_center().code()));
                }
            }
        }
    }

    fn set_span(&mut self, index: usize) {
        let Some(hz) = scu_cat::SCOPE_SPANS_HZ.get(index).copied() else {
            return;
        };
        self.span_hz = hz;
        if let Some(handle) = &self.handle {
            handle.send_cat(&scu_cat::set_scope_span(index as u8));
        }
    }

    fn set_scope_follow_vfo(&mut self, follow: bool) {
        self.follow_vfo = follow;
        if follow {
            if let Some(mode) = self.scope_mode {
                if let Some(handle) = &self.handle {
                    handle.send_cat(&scu_cat::set_scope_mode(mode.with_center().code()));
                }
            }
        }
    }

    fn push_log(&mut self, line: String) {
        self.cat_log.push(line);
        if self.cat_log.len() > 200 {
            let excess = self.cat_log.len() - 200;
            self.cat_log.drain(0..excess);
        }
    }

    /// Parse and apply whatever is typed in `sub`'s entry field.
    fn apply_frequency_for(&mut self, sub: bool) {
        if let Some(hz) = parse_frequency_text(&self.freq_input[sub as usize]) {
            self.send_frequency(sub, hz);
        }
    }

    /// Store, display and send a new frequency on the active VFO.
    fn commit_frequency(&mut self, hz: u64) {
        self.send_frequency(self.rx_sub, hz);
    }

    /// Store and send a frequency for a specific VFO.
    fn send_frequency(&mut self, sub: bool, hz: u64) {
        if hz == 0 || hz > 999_999_990 {
            return;
        }
        if sub {
            self.frequency_b = hz;
        } else {
            self.frequency = hz;
        }
        self.freq_input[sub as usize] = scu_cat::format_hz(hz);
        self.freq_editing[sub as usize] = false;
        let command = if sub {
            scu_cat::set_frequency_b(hz)
        } else {
            scu_cat::set_frequency(hz)
        };
        if let Some(handle) = &self.handle {
            handle.send_cat(&command);
        }
    }

    /// Nudge the active VFO by whole tuning steps (used by the +/− keys).
    fn step_frequency(&mut self, steps: i64) {
        self.step_frequency_on(self.rx_sub, steps);
    }

    /// Nudge a specific VFO by whole tuning steps (used by the scroll wheel on
    /// each VFO card).
    fn step_frequency_on(&mut self, sub: bool, steps: i64) {
        let step = self.freq_step_hz.max(1) as i64;
        let current = if sub { self.frequency_b } else { self.frequency };
        let next = (current as i64 + steps * step).clamp(0, 999_999_990) as u64;
        if next != current {
            self.send_frequency(sub, next);
        }
    }

    /// Restore `sub`'s entry field to the radio's current frequency.
    fn reset_frequency_input_for(&mut self, sub: bool) {
        let hz = if sub { self.frequency_b } else { self.frequency };
        self.freq_input[sub as usize] = freq_input_text(hz);
        self.freq_editing[sub as usize] = false;
    }

    fn set_mode(&mut self, mode: Mode) {
        if let Some(handle) = &self.handle {
            handle.send_cat(&scu_cat::set_mode_vfo(self.rx_sub, mode));
        }
    }

    /// Frequency of the VFO currently selected for receive.
    fn active_frequency(&self) -> u64 {
        if self.rx_sub {
            self.frequency_b
        } else {
            self.frequency
        }
    }

    fn select_rx_vfo(&mut self, sub: bool) {
        if self.rx_sub == sub {
            return;
        }
        self.rx_sub = sub;
        if let Some(handle) = &self.handle {
            handle.send_cat(scu_cat::select_rx_vfo(sub));
        }
    }

    fn set_split(&mut self, on: bool) {
        self.split = on;
        if let Some(handle) = &self.handle {
            handle.send_cat(scu_cat::set_split(on));
        }
    }

    fn swap_vfo(&mut self) {
        std::mem::swap(&mut self.frequency, &mut self.frequency_b);
        self.freq_input.swap(0, 1);
        self.freq_editing.swap(0, 1);
        if let Some(handle) = &self.handle {
            handle.send_cat(scu_cat::swap_vfo());
        }
    }

    fn copy_a_to_b(&mut self) {
        if let Some(handle) = &self.handle {
            handle.send_cat(scu_cat::copy_a_to_b());
        }
    }

    fn copy_b_to_a(&mut self) {
        if let Some(handle) = &self.handle {
            handle.send_cat(scu_cat::copy_b_to_a());
        }
    }

    fn set_rit(&mut self, on: bool) {
        self.rit_on = on;
        if let Some(handle) = &self.handle {
            handle.send_cat(scu_cat::set_rit(on));
        }
    }

    fn set_xit(&mut self, on: bool) {
        self.xit_on = on;
        if let Some(handle) = &self.handle {
            handle.send_cat(scu_cat::set_xit(on));
        }
    }

    fn set_rit_offset(&mut self, hz: i32) {
        self.rit_offset_hz = scu_cat::clamp_clarifier_hz(hz);
        if let Some(handle) = &self.handle {
            handle.send_cat(&scu_cat::set_clarifier(false, self.rit_offset_hz));
        }
    }

    fn set_xit_offset(&mut self, hz: i32) {
        self.xit_offset_hz = scu_cat::clamp_clarifier_hz(hz);
        if let Some(handle) = &self.handle {
            handle.send_cat(&scu_cat::set_clarifier(true, self.xit_offset_hz));
        }
    }

    fn clear_clarifier(&mut self) {
        self.set_rit_offset(0);
        self.set_xit_offset(0);
    }

    fn set_noise_blanker(&mut self, on: bool) {
        self.noise_blanker = on;
        if let Some(handle) = &self.handle {
            handle.send_cat(scu_cat::set_noise_blanker(on));
        }
    }

    fn set_noise_reduction(&mut self, on: bool) {
        self.noise_reduction = on;
        if let Some(handle) = &self.handle {
            handle.send_cat(scu_cat::set_noise_reduction(on));
        }
    }

    fn set_auto_notch(&mut self, on: bool) {
        self.auto_notch = on;
        if let Some(handle) = &self.handle {
            handle.send_cat(scu_cat::set_auto_notch(on));
        }
    }

    fn set_narrow(&mut self, on: bool) {
        self.narrow = on;
        if let Some(handle) = &self.handle {
            handle.send_cat(scu_cat::set_narrow(on));
        }
    }

    fn set_agc(&mut self, agc: Agc) {
        self.agc = Some(agc);
        if let Some(handle) = &self.handle {
            handle.send_cat(&scu_cat::set_agc(agc));
        }
    }

    fn set_rf_gain(&mut self, value: u8) {
        self.rf_gain = value;
        if let Some(handle) = &self.handle {
            handle.send_cat(&scu_cat::set_rf_gain(value));
        }
    }

    fn set_squelch(&mut self, value: u8) {
        self.squelch = value;
        if let Some(handle) = &self.handle {
            handle.send_cat(&scu_cat::set_squelch(value));
        }
    }

    fn send_cat_input(&mut self) {
        let mut command = self.cat_input.trim().to_string();
        if command.is_empty() {
            return;
        }
        if !command.ends_with(';') {
            command.push(';');
        }
        if let Some(handle) = &self.handle {
            handle.send_cat(&command);
            self.push_log(format!("> {command}"));
        }
        self.cat_input.clear();
    }

    fn poll(&mut self) {
        if self.handle.is_none() {
            return;
        }
        if self.last_poll.elapsed() >= Duration::from_secs(1) {
            self.last_poll = Instant::now();
            let md = if self.rx_sub { "MD1;" } else { "MD0;" };
            if let Some(handle) = &self.handle {
                for cmd in [
                    "FA;", "FB;", "FR;", "ST;", md, "SM0;", "PC;", "MG;", "AC;", "RT;", "XT;",
                    "NB;", "NR;", "BC;", "NA0;", "GT0;", "RG0;", "SQ0;", "SS05;", "RM3;", "RM4;",
                    "RM5;", "RM6;", "RM7;", "RM8;", "RM9;",
                ] {
                    handle.send_cat(cmd);
                }
                handle.send_cat(&scu_cat::read_clarifier(false));
                handle.send_cat(&scu_cat::read_clarifier(true));
            }
        }
    }

    fn axis(&self) -> FrequencyAxis {
        FrequencyAxis::new(self.active_frequency() as f64, self.span_hz)
    }

    fn paint_spectrum(&self, painter: &egui::Painter, rect: egui::Rect) {
        painter.rect_filled(rect, 4.0, theme::INSET_BG);
        let midline = egui::Stroke::new(1.0, theme::OUTLINE);
        painter.line_segment(
            [
                egui::pos2(rect.left(), rect.center().y),
                egui::pos2(rect.right(), rect.center().y),
            ],
            midline,
        );

        if self.latest_bins.len() < 2 {
            painter.text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                "waiting for scope data...",
                egui::FontId::proportional(16.0),
                TEXT_DIM,
            );
            return;
        }

        let n = self.latest_bins.len();
        let mut points = Vec::with_capacity(n);
        for (i, &magnitude) in self.latest_bins.iter().enumerate() {
            let x = rect.left() + rect.width() * i as f32 / (n - 1) as f32;
            let y = rect.bottom() - magnitude * rect.height();
            points.push(egui::pos2(x, y));
        }
        painter.add(egui::Shape::line(
            points,
            egui::Stroke::new(1.2, SPECTRUM_GREEN),
        ));
    }

    fn paint_frequency_axis(&self, painter: &egui::Painter, rect: egui::Rect) {
        if self.active_frequency() == 0 {
            return;
        }
        let axis = self.axis();
        let color = TEXT;
        let font = egui::FontId::monospace(11.0);
        for (t, align) in [
            (0.0, egui::Align2::LEFT_BOTTOM),
            (0.5, egui::Align2::CENTER_BOTTOM),
            (1.0, egui::Align2::RIGHT_BOTTOM),
        ] {
            let hz = axis.hz_at(t);
            let label = format_hz_label(hz);
            let x = rect.left() + rect.width() * t as f32;
            let pos = egui::pos2(x, rect.bottom() - 2.0);
            painter.text(pos, align, label, font.clone(), color);
        }

        // Center-tuned marker.
        let center = egui::Stroke::new(1.0, theme::WARN_AMBER);
        painter.line_segment(
            [
                egui::pos2(rect.center().x, rect.top()),
                egui::pos2(rect.center().x, rect.bottom()),
            ],
            center,
        );
    }

    fn ui_top(&mut self, root: &mut egui::Ui) {
        egui::Panel::top(egui::Id::new("top"))
            .frame(theme::top_bar_frame())
            .show(root, |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new("SCU-LAN10")
                            .strong()
                            .color(ACCENT),
                    );
                    ui.separator();

                    ui.label(egui::RichText::new("HOST").small().color(TEXT_FAINT));
                    ui.add(egui::TextEdit::singleline(&mut self.config.host).desired_width(120.0));
                    ui.label(egui::RichText::new("PORT").small().color(TEXT_FAINT));
                    ui.add(
                        egui::DragValue::new(&mut self.config.base_port)
                            .range(1..=65535)
                            .speed(1.0),
                    );
                    #[cfg(target_arch = "wasm32")]
                    {
                        ui.label(egui::RichText::new("BRIDGE").small().color(TEXT_FAINT));
                        let mut bridge = self.config.bridge_url.clone().unwrap_or_default();
                        if ui
                            .add(egui::TextEdit::singleline(&mut bridge).desired_width(160.0))
                            .changed()
                        {
                            self.config.bridge_url = Some(bridge);
                        }
                    }
                    ui.label(egui::RichText::new("USER").small().color(TEXT_FAINT));
                    ui.add(
                        egui::TextEdit::singleline(&mut self.config.username).desired_width(90.0),
                    );
                    ui.label(egui::RichText::new("PASS").small().color(TEXT_FAINT));
                    ui.add(
                        egui::TextEdit::singleline(&mut self.config.password)
                            .password(true)
                            .desired_width(90.0),
                    );

                    ui.separator();
                    if self.connected() {
                        if ui
                            .add(egui::Button::new(
                                egui::RichText::new("Disconnect").strong().color(TX_RED),
                            ))
                            .clicked()
                        {
                            self.disconnect();
                        }
                    } else if ui
                        .add_enabled(
                            !self.connecting,
                            egui::Button::new(egui::RichText::new("Connect").strong()),
                        )
                        .clicked()
                    {
                        let ctx = ui.ctx().clone();
                        self.connect(ctx);
                    }

                    if ui
                        .add(egui::Button::new(egui::RichText::new("Settings")))
                        .on_hover_text("Show meters and CAT console options")
                        .clicked()
                    {
                        self.show_settings = !self.show_settings;
                    }

                    ui.label(egui::RichText::new(&self.status).color(self.status_color()));

                    if let Some(radio) = self.radio {
                        ui.with_layout(
                            egui::Layout::right_to_left(egui::Align::Center),
                            |ui| {
                                ui.label(
                                    egui::RichText::new(radio.name())
                                        .monospace()
                                        .color(TEXT_DIM),
                                );
                            },
                        );
                    }
                });
            });
    }

    /// Colour-coded connection status text.
    fn status_color(&self) -> egui::Color32 {
        if self.status.starts_with("connected") {
            RX_GREEN
        } else if self.status.contains("failed")
            || self.status.contains("unavailable")
            || self.status.starts_with("disconnected:")
        {
            TX_RED
        } else {
            TEXT_DIM
        }
    }

    /// The prominent dual-VFO header: transmit key, both VFO read-outs, quick
    /// operate toggles and the S-meter, laid out like a table-top radio.
    fn ui_vfo(&mut self, root: &mut egui::Ui) {
        let held = egui::Panel::top(egui::Id::new("vfo"))
            .frame(theme::header_frame())
            .show(root, |ui| {
                ui.add_space(8.0);
                let mut ptt_held = false;
                ui.horizontal(|ui| {
                    // Transmit key cluster.
                    ui.vertical(|ui| {
                        ptt_held = self.ptt_button(ui);
                        if self.tune_button(ui) {
                            self.start_atu_tune();
                        }
                    });
                    ui.separator();

                    // Share the remaining width between the two VFO cards so
                    // neither ever spills past the window edge.
                    const TOGGLES_WIDTH: f32 = 172.0;
                    const SEPARATORS: f32 = 30.0;
                    const FRAME_PAD: f32 = 28.0;
                    let available = ui.available_width();
                    let card_width = ((available - TOGGLES_WIDTH - SEPARATORS) / 2.0
                        - FRAME_PAD)
                        .clamp(196.0, 420.0);

                    self.vfo_card(ui, false, card_width);
                    ui.add_space(10.0);
                    self.vfo_card(ui, true, card_width);
                    ui.separator();

                    ui.vertical(|ui| self.quick_toggles(ui));
                });
                ui.add_space(8.0);
                self.smeter_strip(ui);
                ui.add_space(6.0);
                ptt_held
            })
            .inner;

        let space = self.handle.is_some()
            && root.ctx().memory(|m| m.focused().is_none())
            && root.ctx().input(|i| i.key_down(egui::Key::Space));
        self.set_ptt(held || space);
    }

    /// Big red/green push-to-talk key. Returns `true` while held down.
    fn ptt_button(&self, ui: &mut egui::Ui) -> bool {
        let on = self.ptt;
        let fill = if on { TX_RED } else { egui::Color32::from_rgb(48, 54, 64) };
        let stroke = if on { TX_RED } else { OUTLINE };
        let text_color = if on { ON_ACCENT } else { TEXT };
        let label = if on { "TX" } else { "PTT" };
        let button = egui::Button::new(
            egui::RichText::new(label).strong().size(19.0).color(text_color),
        )
        .min_size(egui::Vec2::new(84.0, 52.0))
        .fill(fill)
        .stroke(egui::Stroke::new(1.5, stroke));
        ui.add_enabled(self.handle.is_some(), button)
            .on_hover_text("Push to talk (or hold space)")
            .is_pointer_button_down_on()
    }

    /// Runs an ATU tuning cycle. The radio tuner is enabled first if needed.
    fn start_atu_tune(&mut self) {
        if let Some(handle) = &self.handle {
            if !self.atu_on {
                self.atu_on = true;
                handle.send_cat(&scu_cat::set_atu(true));
            }
            handle.send_cat(scu_cat::start_tune());
            self.push_log(format!("> {}", scu_cat::start_tune()));
        }
    }

    fn tune_button(&self, ui: &mut egui::Ui) -> bool {
        let button = egui::Button::new(egui::RichText::new("TUNE").strong())
            .min_size(egui::Vec2::new(84.0, 26.0))
            .fill(if self.atu_on { theme::ACCENT_DEEP } else { theme::BUTTON_BG });
        ui.add_enabled(self.handle.is_some(), button)
            .on_hover_text("Start an ATU tuning cycle (keys a carrier)")
            .clicked()
    }

    /// One VFO read-out card (Main = VFO-A, Sub = VFO-B).
    fn vfo_card(&mut self, ui: &mut egui::Ui, sub: bool, width: f32) {
        let active = self.rx_sub == sub;
        let border = if active { ACCENT } else { OUTLINE };
        let name = if sub { "SUB" } else { "MAIN" };
        let vfo = if sub { "B" } else { "A" };
        // Keep the frequency inside the card at any window width.
        let freq_size = (width / 7.5).clamp(18.0, 30.0);

        egui::Frame::new()
            .fill(theme::CARD_BG)
            .stroke(egui::Stroke::new(if active { 1.5 } else { 1.0 }, border))
            .corner_radius(6)
            .inner_margin(egui::Margin::symmetric(12, 8))
            .show(ui, |ui| {
                ui.set_width(width);
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new(name)
                            .strong()
                            .size(14.0)
                            .color(if active { ACCENT } else { TEXT_DIM }),
                    );
                    ui.label(
                        egui::RichText::new(format!("VFO {vfo}"))
                            .small()
                            .color(TEXT_FAINT),
                    );
                    // A bounded right-aligned cluster; an unbounded
                    // right-to-left layout would swallow all remaining width.
                    ui.allocate_ui_with_layout(
                        egui::Vec2::new(78.0, 20.0),
                        egui::Layout::right_to_left(egui::Align::Center),
                        |ui| {
                            let (fill, text_color) = if active {
                                (RX_GREEN, ON_ACCENT)
                            } else {
                                (theme::BUTTON_BG, TEXT_DIM)
                            };
                            if ui
                                .add(
                                    egui::Button::new(
                                        egui::RichText::new("RX").small().strong().color(text_color),
                                    )
                                    .fill(fill)
                                    .min_size(egui::Vec2::new(42.0, 20.0)),
                                )
                                .on_hover_text("Receive on this VFO")
                                .clicked()
                            {
                                self.select_rx_vfo(sub);
                            }
                            if self.tx_on(sub) {
                                ui.label(
                                    egui::RichText::new("TX").small().strong().color(TX_RED),
                                );
                            }
                        },
                    );
                });

                // The big read-out doubles as an inline editor: it shows the
                // plain cyan digits until clicked, then becomes a text field.
                let idx = sub as usize;
                let freq = if sub { self.frequency_b } else { self.frequency };
                let edit_id = ui.make_persistent_id(("vfo-freq", idx));
                if self.freq_editing[idx] {
                    let response = ui
                        .add(
                            egui::TextEdit::singleline(&mut self.freq_input[idx])
                                .id(edit_id)
                                .font(egui::FontId::monospace(freq_size))
                                .text_color(if active { FREQ_CYAN } else { TEXT_DIM })
                                .desired_width(width)
                                .frame(egui::Frame::NONE)
                                .margin(egui::Margin::ZERO),
                        )
                        .on_hover_text("Enter to apply, Esc to cancel, scroll to tune");

                    if response.gained_focus() {
                        select_all_text(ui.ctx(), response.id, self.freq_input[idx].chars().count());
                    }
                    if response.lost_focus() {
                        // Enter applies; clicking away does too.
                        self.apply_frequency_for(sub);
                        self.freq_editing[idx] = false;
                    }
                    if response.has_focus() && ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                        self.reset_frequency_input_for(sub);
                        self.freq_editing[idx] = false;
                        response.surrender_focus();
                    }
                    if response.hovered() {
                        let steps = scroll_steps(ui);
                        if steps != 0 {
                            self.step_frequency_on(sub, steps);
                        }
                    }
                } else {
                    let response = ui
                        .add(
                            egui::Label::new(
                                egui::RichText::new(scu_cat::format_hz(freq))
                                    .monospace()
                                    .strong()
                                    .size(freq_size)
                                    .color(if active { FREQ_CYAN } else { TEXT_DIM }),
                            )
                            .sense(egui::Sense::click()),
                        )
                        .on_hover_text("Click to edit, or scroll to tune this VFO");
                    if response.clicked() {
                        self.freq_input[idx] = freq_input_text(freq);
                        self.freq_editing[idx] = true;
                        ui.memory_mut(|m| m.request_focus(edit_id));
                    }
                    if response.hovered() {
                        let steps = scroll_steps(ui);
                        if steps != 0 {
                            self.step_frequency_on(sub, steps);
                        }
                    }
                }

                ui.horizontal(|ui| {
                    let mode = self.mode.map(|m| m.label()).unwrap_or("--");
                    ui.label(egui::RichText::new(mode).strong().color(TEXT));
                    let sub_label = if self.split {
                        "SPLIT".to_string()
                    } else {
                        span_label(self.span_hz)
                    };
                    ui.label(egui::RichText::new(sub_label).small().color(TEXT_FAINT));
                });
            });
    }

    /// Whether VFO `sub` is currently the transmit VFO.
    fn tx_on(&self, sub: bool) -> bool {
        if !self.ptt {
            return false;
        }
        let tx_sub = if self.split { !self.rx_sub } else { self.rx_sub };
        tx_sub == sub
    }

    /// Compact operate toggles next to the VFO cards.
    fn quick_toggles(&mut self, ui: &mut egui::Ui) {
        ui.set_min_width(148.0);
        theme::section(ui, "Operate");

        let (text, fill) = if self.ptt {
            ("TX  ON AIR", TX_RED)
        } else {
            ("RX  STANDBY", RX_GREEN)
        };
        ui.add(
            egui::Button::new(egui::RichText::new(text).strong().color(ON_ACCENT))
                .fill(fill)
                .min_size(egui::Vec2::new(148.0, 26.0)),
        );

        ui.horizontal_wrapped(|ui| {
            if toggle_chip(ui, self.split, "SPLIT") {
                self.set_split(!self.split);
            }
            if toggle_chip(ui, self.rit_on, "RIT") {
                self.set_rit(!self.rit_on);
            }
            if toggle_chip(ui, self.xit_on, "XIT") {
                self.set_xit(!self.xit_on);
            }
            if toggle_chip(ui, self.atu_on, "ATU") {
                self.atu_on = !self.atu_on;
                if let Some(handle) = &self.handle {
                    handle.send_cat(&scu_cat::set_atu(self.atu_on));
                }
            }
        });
    }

    /// Full-width S-meter with VFO swap/copy shortcuts.
    fn smeter_strip(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new("S-METER")
                    .small()
                    .strong()
                    .color(ACCENT),
            );
            let width = (ui.available_width() - 230.0).max(140.0);
            ui.add(
                egui::ProgressBar::new(MeterKind::S.fraction(self.smeter))
                    .desired_width(width)
                    .desired_height(18.0)
                    .fill(RX_GREEN)
                    .corner_radius(4)
                    .text(MeterKind::S.format(self.smeter)),
            );
            if ui
                .add_enabled(self.handle.is_some(), egui::Button::new("A<->B").small())
                .on_hover_text("Swap VFO-A and VFO-B")
                .clicked()
            {
                self.swap_vfo();
            }
            if ui
                .add_enabled(self.handle.is_some(), egui::Button::new("A->B").small())
                .clicked()
            {
                self.copy_a_to_b();
            }
            if ui
                .add_enabled(self.handle.is_some(), egui::Button::new("B->A").small())
                .clicked()
            {
                self.copy_b_to_a();
            }
        });
    }

    fn ui_side(&mut self, root: &mut egui::Ui) {
        egui::Panel::left(egui::Id::new("rig"))
            .default_size(320.0)
            .frame(theme::rail_frame())
            .show(root, |ui| {
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        ui.add_space(2.0);
                        ui.horizontal(|ui| {
                            ui.label(egui::RichText::new("RIG").small().strong().color(ACCENT));
                            let radio = self
                                .radio
                                .map(|r| r.name())
                                .unwrap_or_else(|| "unknown".to_string());
                            ui.label(egui::RichText::new(radio).color(TEXT_DIM));
                        });

                        theme::section(ui, "VFO");
                        ui.horizontal(|ui| {
                            ui.label("Receive");
                            if ui.selectable_label(!self.rx_sub, "A / Main").clicked() {
                                self.select_rx_vfo(false);
                            }
                            if ui.selectable_label(self.rx_sub, "B / Sub").clicked() {
                                self.select_rx_vfo(true);
                            }
                        });
                        ui.horizontal(|ui| {
                            ui.label(
                                egui::RichText::new(format!(
                                    "A {}",
                                    scu_cat::format_hz(self.frequency)
                                ))
                                .monospace()
                                .color(TEXT_DIM),
                            );
                            ui.label(
                                egui::RichText::new(format!(
                                    "B {}",
                                    scu_cat::format_hz(self.frequency_b)
                                ))
                                .monospace()
                                .color(TEXT_DIM),
                            );
                        });
                        theme::section(ui, "Tuning");
                        ui.label(
                            egui::RichText::new("Type directly on either VFO card above")
                                .small()
                                .color(TEXT_FAINT),
                        );
                        ui.horizontal(|ui| {
                            ui.label(egui::RichText::new("Step").small().color(TEXT_FAINT));
                            let mut step = self.freq_step_hz;
                            egui::ComboBox::from_id_salt("freq-step")
                                .selected_text(step_label(self.freq_step_hz))
                                .width(88.0)
                                .show_ui(ui, |ui| {
                                    for (value, label) in FREQ_STEPS {
                                        ui.selectable_value(&mut step, value, label);
                                    }
                                });
                            self.freq_step_hz = step;
                            if ui
                                .button("-")
                                .on_hover_text("Tune down one step")
                                .clicked()
                            {
                                self.step_frequency(-1);
                            }
                            if ui
                                .button("+")
                                .on_hover_text("Tune up one step")
                                .clicked()
                            {
                                self.step_frequency(1);
                            }
                        });

                        theme::section(ui, "Mode");
                        ui.horizontal_wrapped(|ui| {
                            for mode in Mode::ALL {
                                if ui
                                    .selectable_label(self.mode == Some(mode), mode.label())
                                    .clicked()
                                {
                                    self.set_mode(mode);
                                }
                            }
                        });

                        theme::section(ui, "Clarifier (RIT/XIT)");
                        ui.horizontal(|ui| {
                            let mut rit = self.rit_on;
                            if ui.checkbox(&mut rit, "RIT").changed() {
                                self.set_rit(rit);
                            }
                            let mut xit = self.xit_on;
                            if ui.checkbox(&mut xit, "XIT").changed() {
                                self.set_xit(xit);
                            }
                            if ui.button("Clear").clicked() {
                                self.clear_clarifier();
                            }
                        });
                        let mut rit_offset = self.rit_offset_hz;
                        if ui
                            .add(
                                egui::Slider::new(&mut rit_offset, -9990..=9990)
                                    .step_by(10.0)
                                    .text("RIT offset")
                                    .suffix(" Hz"),
                            )
                            .changed()
                        {
                            self.set_rit_offset(rit_offset);
                        }
                        let mut xit_offset = self.xit_offset_hz;
                        if ui
                            .add(
                                egui::Slider::new(&mut xit_offset, -9990..=9990)
                                    .step_by(10.0)
                                    .text("XIT offset")
                                    .suffix(" Hz"),
                            )
                            .changed()
                        {
                            self.set_xit_offset(xit_offset);
                        }

                        theme::section(ui, "DSP");
                        let mut nb = self.noise_blanker;
                        if ui.checkbox(&mut nb, "Noise blanker").changed() {
                            self.set_noise_blanker(nb);
                        }
                        let mut nr = self.noise_reduction;
                        if ui.checkbox(&mut nr, "Noise reduction").changed() {
                            self.set_noise_reduction(nr);
                        }
                        let mut notch = self.auto_notch;
                        if ui.checkbox(&mut notch, "Auto notch").changed() {
                            self.set_auto_notch(notch);
                        }
                        let mut narrow = self.narrow;
                        if ui.checkbox(&mut narrow, "Narrow filter").changed() {
                            self.set_narrow(narrow);
                        }

                        theme::section(ui, "Receiver");
                        let current_agc = self.agc;
                        let mut selected_agc = current_agc;
                        egui::ComboBox::from_id_salt("agc")
                            .selected_text(format!(
                                "AGC {}",
                                current_agc.map(|a| a.label()).unwrap_or("--")
                            ))
                            .show_ui(ui, |ui| {
                                for agc in Agc::ALL {
                                    ui.selectable_value(&mut selected_agc, Some(agc), agc.label());
                                }
                            });
                        if selected_agc != current_agc {
                            if let Some(agc) = selected_agc {
                                self.set_agc(agc);
                            }
                        }
                        let mut rf_gain = self.rf_gain as i32;
                        if ui
                            .add(
                                egui::Slider::new(&mut rf_gain, 0..=scu_cat::RF_GAIN_MAX as i32)
                                    .text("RF gain"),
                            )
                            .changed()
                        {
                            self.set_rf_gain(rf_gain as u8);
                        }
                        let mut squelch = self.squelch as i32;
                        if ui
                            .add(
                                egui::Slider::new(&mut squelch, 0..=scu_cat::SQUELCH_MAX as i32)
                                    .text("Squelch"),
                            )
                            .changed()
                        {
                            self.set_squelch(squelch as u8);
                        }

                        theme::section(ui, "Meters");
                        for index in 0..self.meters.len() {
                            if !self.settings.visible_meters.contains(&(index as u8)) {
                                continue;
                            }
                            let Some(raw) = self.meters[index] else {
                                continue;
                            };
                            let kind = MeterKind::from_rm_index(index as u8);
                            ui.horizontal(|ui| {
                                ui.label(
                                    egui::RichText::new(kind.label()).monospace().strong(),
                                );
                                ui.add(
                                    egui::ProgressBar::new(kind.fraction(raw))
                                        .fill(ACCENT)
                                        .text(kind.format(raw)),
                                );
                            });
                        }

                        theme::section(ui, "Scope & Waterfall");
                        let mut selected_span: Option<usize> = None;
                        ui.horizontal(|ui| {
                            ui.label("Span");
                            egui::ComboBox::from_id_salt("span")
                                .selected_text(span_label(self.span_hz))
                                .show_ui(ui, |ui| {
                                    for (index, span) in
                                        scu_cat::SCOPE_SPANS_HZ.iter().enumerate()
                                    {
                                        let selected = (self.span_hz - span).abs() < 0.5;
                                        if ui
                                            .selectable_label(selected, span_label(*span))
                                            .clicked()
                                        {
                                            selected_span = Some(index);
                                        }
                                    }
                                });
                        });
                        if let Some(index) = selected_span {
                            self.set_span(index);
                        }

                        let mut follow = self.follow_vfo;
                        if ui
                            .checkbox(&mut follow, "Scope follows VFO")
                            .on_hover_text(
                                "Set the radio's scope to CENTER mode so the waterfall tracks tuning",
                            )
                            .changed()
                        {
                            self.set_scope_follow_vfo(follow);
                        }

                        ui.horizontal(|ui| {
                            ui.label("Colormap");
                            egui::ComboBox::from_id_salt("colormap")
                                .selected_text(self.waterfall.colormap.label())
                                .show_ui(ui, |ui| {
                                    for map in Colormap::ALL {
                                        ui.selectable_value(
                                            &mut self.waterfall.colormap,
                                            map,
                                            map.label(),
                                        );
                                    }
                                });
                        });
                        ui.add(
                            egui::Slider::new(&mut self.waterfall.black_level, 0.0..=0.9)
                                .text("Black"),
                        );
                        ui.add(
                            egui::Slider::new(&mut self.waterfall.gain, 0.2..=4.0).text("Gain"),
                        );
                        ui.horizontal(|ui| {
                            ui.label("Bins");
                            egui::ComboBox::from_id_salt("bin-mode")
                                .selected_text(self.scope_bins.label())
                                .show_ui(ui, |ui| {
                                    for mode in BinInterleave::ALL {
                                        ui.selectable_value(
                                            &mut self.scope_bins,
                                            mode,
                                            mode.label(),
                                        );
                                    }
                                });
                            if ui.button("Clear").clicked() {
                                self.waterfall.clear();
                            }
                        });

                        theme::section(ui, "Audio");
                        let mut muted = self.muted;
                        if ui.checkbox(&mut muted, "Mute").changed() {
                            self.muted = muted;
                            self.sync_audio_enabled();
                        }
                        if ui
                            .add(egui::Slider::new(&mut self.volume, 0.0..=1.5).text("Volume"))
                            .changed()
                        {
                            if let Some(audio) = &self.audio {
                                audio.set_volume(self.volume);
                            }
                        }
                        let mut stereo = self.stereo;
                        if ui
                            .checkbox(&mut stereo, "Stereo (ch1 -> right)")
                            .on_hover_text("Off: duplicate the receiver channel to both outputs")
                            .changed()
                        {
                            self.stereo = stereo;
                            if let Some(audio) = &self.audio {
                                audio.set_stereo(stereo);
                            }
                        }

                        theme::section(ui, "Transmit");
                        let mut power = self.tx_power as i32;
                        if ui
                            .add(
                                egui::Slider::new(
                                    &mut power,
                                    scu_cat::POWER_MIN_W as i32
                                        ..=scu_cat::POWER_MAX_W as i32,
                                )
                                .text("TX power")
                                .suffix(" W"),
                            )
                            .changed()
                        {
                            self.tx_power = power as u16;
                            if let Some(handle) = &self.handle {
                                handle.send_cat(&scu_cat::set_power(self.tx_power));
                            }
                        }

                        let mut mic_gain = self.radio_mic_gain as i32;
                        if ui
                            .add(
                                egui::Slider::new(&mut mic_gain, 0..=scu_cat::MIC_GAIN_MAX as i32)
                                    .text("Mic gain")
                                    .suffix(" %"),
                            )
                            .changed()
                        {
                            self.radio_mic_gain = mic_gain as u8;
                            if let Some(handle) = &self.handle {
                                handle.send_cat(&scu_cat::set_mic_gain(self.radio_mic_gain));
                            }
                        }

                        ui.horizontal(|ui| {
                            ui.label("Mic");
                            let current = self
                                .mic_device
                                .clone()
                                .unwrap_or_else(|| "Default".to_string());
                            let mut changed = false;
                            egui::ComboBox::from_id_salt("mic-device")
                                .selected_text(current)
                                .show_ui(ui, |ui| {
                                    if ui
                                        .selectable_label(self.mic_device.is_none(), "Default")
                                        .clicked()
                                    {
                                        self.mic_device = None;
                                        changed = true;
                                    }
                                    let devices = self.mic_devices.clone();
                                    for name in devices {
                                        let selected =
                                            self.mic_device.as_deref() == Some(name.as_str());
                                        if ui.selectable_label(selected, &name).clicked() {
                                            self.mic_device = Some(name);
                                            changed = true;
                                        }
                                    }
                                });
                            if changed {
                                self.mic = None;
                                if let Some(handle) = self.handle.clone() {
                                    self.ensure_mic(handle);
                                }
                            }
                        });
                        if ui
                            .add(egui::Slider::new(&mut self.mic_gain, 0.0..=3.0).text("Capture gain"))
                            .changed()
                        {
                            if let Some(mic) = &self.mic {
                                mic.set_gain(self.mic_gain);
                            }
                        }
                        if !self.mic_status.is_empty() {
                            ui.label(egui::RichText::new(&self.mic_status).small().weak());
                        }

                        if self.settings.show_cat_console {
                            theme::section(ui, "CAT Console");
                            ui.horizontal(|ui| {
                                let response = ui.add(
                                    egui::TextEdit::singleline(&mut self.cat_input)
                                        .desired_width(190.0)
                                        .hint_text("e.g. IF;"),
                                );
                                if ui.button("Send").clicked()
                                    || (response.lost_focus()
                                        && ui.input(|i| i.key_pressed(egui::Key::Enter)))
                                {
                                    self.send_cat_input();
                                }
                            });
                            egui::ScrollArea::vertical()
                                .max_height(140.0)
                                .stick_to_bottom(true)
                                .show(ui, |ui| {
                                    for line in &self.cat_log {
                                        ui.label(egui::RichText::new(line).monospace().small());
                                    }
                                });
                        }
                    });
            });
    }

    /// Floating settings page: meter visibility and CAT console toggle.
    fn ui_settings(&mut self, ctx: &egui::Context) {
        if !self.show_settings {
            return;
        }
        let mut open = self.show_settings;
        egui::Window::new("Settings")
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .default_width(260.0)
            .show(ctx, |ui| {
                theme::section(ui, "Meters");
                ui.label(
                    egui::RichText::new("Choose which meters appear in the side rail.")
                        .small()
                        .color(TEXT_FAINT),
                );
                for index in METER_INDICES {
                    let kind = MeterKind::from_rm_index(index);
                    let mut on = self.settings.visible_meters.contains(&index);
                    if ui.checkbox(&mut on, kind.label()).changed() {
                        if on {
                            if !self.settings.visible_meters.contains(&index) {
                                self.settings.visible_meters.push(index);
                            }
                        } else {
                            self.settings.visible_meters.retain(|&i| i != index);
                        }
                        self.settings.visible_meters.sort_unstable();
                        save_settings(&self.settings);
                    }
                }

                theme::section(ui, "CAT Console");
                let mut show = self.settings.show_cat_console;
                if ui
                    .checkbox(&mut show, "Show CAT console")
                    .on_hover_text("Display the CAT command log and entry field in the side rail")
                    .changed()
                {
                    self.settings.show_cat_console = show;
                    save_settings(&self.settings);
                }
            });
        self.show_settings = open;
    }

    fn ui_center(&mut self, root: &mut egui::Ui) {
        let ctx = root.ctx().clone();
        egui::CentralPanel::default().show(root, |ui| {
            let size = ui.available_size();
            let spectrum_height = (size.y * 0.30).clamp(120.0, 240.0);

            let (response, painter) = ui.allocate_painter(
                egui::Vec2::new(size.x, spectrum_height),
                egui::Sense::click(),
            );
            self.paint_spectrum(&painter, response.rect);
            self.tune_interaction(&response, &painter, response.rect);

            ui.separator();

            self.waterfall.update_texture(&ctx);
            let wf_size = egui::Vec2::new(ui.available_width(), ui.available_height());
            let (wf_response, wf_painter) = ui.allocate_painter(wf_size, egui::Sense::click());
            self.waterfall.paint(&wf_painter, wf_response.rect);
            self.paint_frequency_axis(&wf_painter, wf_response.rect);
            self.tune_interaction(&wf_response, &wf_painter, wf_response.rect);
        });
    }

    /// Click-to-tune plus a hover frequency readout over a spectrum/waterfall area.
    fn tune_interaction(
        &mut self,
        response: &egui::Response,
        painter: &egui::Painter,
        rect: egui::Rect,
    ) {
        if self.active_frequency() == 0 || rect.width() <= 0.0 {
            return;
        }
        let axis = self.axis();
        let hz_at = |x: f32| {
            let t = ((x - rect.left()) / rect.width()).clamp(0.0, 1.0) as f64;
            axis.hz_at(t)
        };

        if let Some(pos) = response.hover_pos() {
            let stroke = egui::Stroke::new(1.0, TEXT_DIM);
            painter.line_segment(
                [
                    egui::pos2(pos.x, rect.top()),
                    egui::pos2(pos.x, rect.bottom()),
                ],
                stroke,
            );
            painter.text(
                egui::pos2((pos.x + 4.0).min(rect.right() - 70.0), rect.top() + 3.0),
                egui::Align2::LEFT_TOP,
                format_hz_label(hz_at(pos.x)),
                egui::FontId::monospace(12.0),
                theme::WARN_AMBER,
            );
        }

        if response.clicked() {
            if let Some(pos) = response.interact_pointer_pos() {
                let hz = hz_at(pos.x);
                self.tune_to_hz(hz);
            }
        }
    }

    fn tune_to_hz(&mut self, hz: f64) {
        if hz <= 0.0 {
            return;
        }
        // Round to the nearest 10 Hz; the scope bin resolution is coarser than that.
        let hz = (hz / 10.0).round() as u64 * 10;
        if hz == self.active_frequency() {
            return;
        }
        self.commit_frequency(hz);
    }
}

impl eframe::App for ScuApp {
    /// Runs every frame, even when the window is hidden or unfocused, so the
    /// session keeps polling and the UI keeps receiving engine messages.
    fn logic(&mut self, _ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.drain_engine();
        self.poll();
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.ui_top(ui);
        self.ui_vfo(ui);
        self.ui_side(ui);
        self.ui_center(ui);
        self.ui_settings(&ctx);
        ctx.request_repaint_after(Duration::from_millis(16));
    }
}

impl ScuApp {
    fn connected(&self) -> bool {
        self.handle.is_some()
    }
}

/// Background session engine.
///
/// Owns the [`ScuClient`], feeds audio frames directly to the [`AudioSink`]
/// (never blocked by UI repaints), and forwards everything else to the UI over
/// a channel. The same future runs on a Tokio thread natively and under
/// `spawn_local` in the browser.
fn start_engine(
    config: ConnectConfig,
    sink: Option<AudioSink>,
    tx: std::sync::mpsc::Sender<EngineMsg>,
    shutdown: Arc<AtomicBool>,
    ctx: egui::Context,
) -> Result<(), std::io::Error> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        std::thread::Builder::new()
            .name("scu-engine".into())
            .spawn(move || {
                let runtime = match tokio::runtime::Builder::new_multi_thread()
                    .enable_all()
                    .build()
                {
                    Ok(runtime) => runtime,
                    Err(error) => {
                        let _ = tx.send(EngineMsg::Failed(error.to_string()));
                        return;
                    }
                };
                runtime.block_on(engine_future(config, sink, tx, shutdown, ctx));
            })
            .map(|_| ())
    }

    #[cfg(target_arch = "wasm32")]
    {
        wasm_bindgen_futures::spawn_local(engine_future(config, sink, tx, shutdown, ctx));
        Ok(())
    }
}

async fn engine_future(
    config: ConnectConfig,
    sink: Option<AudioSink>,
    tx: std::sync::mpsc::Sender<EngineMsg>,
    shutdown: Arc<AtomicBool>,
    ctx: egui::Context,
) {
    let mut client = match ScuClient::connect(config).await {
        Ok(client) => client,
        Err(error) => {
            let _ = tx.send(EngineMsg::Failed(error.to_string()));
            return;
        }
    };

    if tx.send(EngineMsg::Handle(client.handle())).is_err() {
        return;
    }

    loop {
        if shutdown.load(Ordering::Relaxed) {
            break;
        }
        match scu_client::timeout(Duration::from_millis(250), client.recv()).await {
            Some(Some(Event::Audio(frame))) => {
                if let Some(sink) = &sink {
                    sink.push(*frame);
                }
            }
            Some(Some(event)) => {
                if tx.send(EngineMsg::Event(event)).is_err() {
                    break;
                }
                // Wake the UI even when it is unfocused or occluded.
                ctx.request_repaint();
            }
            Some(None) => break,
            None => {}
        }
    }

    let _ = tx.send(EngineMsg::Stopped);
}

fn span_label(span: f64) -> String {
    if span >= 1_000_000.0 {
        format!("{:.1} MHz", span / 1_000_000.0)
    } else {
        format!("{:.0} kHz", span / 1_000.0)
    }
}

/// A compact toggle button that lights up with the accent colour when `on`.
fn toggle_chip(ui: &mut egui::Ui, on: bool, label: &str) -> bool {
    let fill = if on { ACCENT } else { theme::BUTTON_BG };
    let stroke = if on { ACCENT } else { OUTLINE };
    let text_color = if on { ON_ACCENT } else { TEXT_DIM };
    ui.add(
        egui::Button::new(egui::RichText::new(label).small().strong().color(text_color))
            .fill(fill)
            .stroke(egui::Stroke::new(1.0, stroke))
            .min_size(egui::Vec2::new(44.0, 24.0)),
    )
    .clicked()
}

/// Tuning steps offered by the frequency entry control.
const FREQ_STEPS: [(u64, &str); 8] = [
    (1, "1 Hz"),
    (10, "10 Hz"),
    (100, "100 Hz"),
    (1_000, "1 kHz"),
    (2_500, "2.5 kHz"),
    (5_000, "5 kHz"),
    (10_000, "10 kHz"),
    (100_000, "100 kHz"),
];

fn step_label(step: u64) -> &'static str {
    FREQ_STEPS
        .iter()
        .find(|(value, _)| *value == step)
        .map(|(_, label)| *label)
        .unwrap_or("custom")
}

/// Text to show in the frequency entry field (`""` while unknown).
fn freq_input_text(hz: u64) -> String {
    if hz == 0 {
        String::new()
    } else {
        scu_cat::format_hz(hz)
    }
}

/// Parse operator-style frequency text into Hz.
///
/// Separators (`.`, `,`, space, `_`) are ignored. Accepted forms:
/// * `14.265.160` → 14265160 (MHz.kHz.Hz)
/// * `14.265`     → 14265000 (MHz with a decimal fraction)
/// * `14.`        → 14000000 (whole MHz)
/// * `14265160`   → 14265160 (whole Hz, 7+ digits)
/// * `14265`      → 14265000 (kHz, up to 6 digits)
fn parse_frequency_text(text: &str) -> Option<u64> {
    let cleaned: String = text
        .chars()
        .filter(|c| c.is_ascii_digit() || *c == '.')
        .collect();
    let digits: String = cleaned.chars().filter(|c| c.is_ascii_digit()).collect();
    if digits.is_empty() {
        return None;
    }

    let groups: Vec<&str> = cleaned.split('.').filter(|g| !g.is_empty()).collect();
    let hz = match groups.len() {
        0 | 1 => {
            let value: u64 = digits.parse().ok()?;
            if cleaned.contains('.') {
                value * 1_000_000
            } else if digits.len() <= 6 {
                value * 1_000
            } else {
                value
            }
        }
        // MHz.kHz — "14.265" means 14.265 MHz.
        2 => {
            let mhz: u64 = groups[0].parse().ok()?;
            let fraction = format!("{:0<6}", groups[1]);
            let sub: u64 = fraction.get(..6)?.parse().ok()?;
            mhz * 1_000_000 + sub
        }
        // Three (or more) dotted groups: the digits read as whole Hz.
        _ => digits.parse().ok()?,
    };
    (hz > 0 && hz <= 999_999_990).then_some(hz)
}

/// Select the entire contents of the text field with the given id.
fn select_all_text(ctx: &egui::Context, id: egui::Id, len: usize) {
    if let Some(mut state) = egui::widgets::text_edit::TextEditState::load(ctx, id) {
        state.cursor.set_char_range(Some(egui::text::CCursorRange::two(
            egui::text::CCursor::new(0),
            egui::text::CCursor::new(len),
        )));
        state.store(ctx, id);
    }
}

/// Number of tuning steps implied by this frame's mouse-wheel input, read from
/// the raw events so it works without smoothing/decay.
fn scroll_steps(ui: &egui::Ui) -> i64 {
    ui.input(|i| {
        i.events
            .iter()
            .filter_map(|event| match event {
                egui::Event::MouseWheel { unit, delta, .. } => Some(match unit {
                    egui::MouseWheelUnit::Line => delta.y,
                    egui::MouseWheelUnit::Point => delta.y / 50.0,
                    egui::MouseWheelUnit::Page => delta.y * 10.0,
                }),
                _ => None,
            })
            .sum::<f32>()
            .round() as i64
    })
}

fn format_hz_label(hz: f64) -> String {
    if hz >= 1_000_000.0 {
        format!("{:.4}", hz / 1_000_000.0)
    } else {
        format!("{:.1}", hz / 1_000.0)
    }
}

fn config_default_freq() -> u64 {
    0
}

#[cfg(not(target_arch = "wasm32"))]
fn config_path() -> Option<PathBuf> {
    let home = std::env::var_os("HOME")?;
    Some(PathBuf::from(home).join(".config/scu-client/config.toml"))
}

#[cfg(not(target_arch = "wasm32"))]
fn load_config() -> Option<ConnectConfig> {
    let path = config_path()?;
    let text = std::fs::read_to_string(path).ok()?;
    toml::from_str(&text).ok()
}

#[cfg(not(target_arch = "wasm32"))]
fn save_config(config: &ConnectConfig) {
    let Some(path) = config_path() else {
        return;
    };
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(text) = toml::to_string_pretty(config) {
        let _ = std::fs::write(path, text);
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn settings_path() -> Option<PathBuf> {
    let home = std::env::var_os("HOME")?;
    Some(PathBuf::from(home).join(".config/scu-client/settings.toml"))
}

#[cfg(not(target_arch = "wasm32"))]
fn load_settings() -> Option<AppSettings> {
    let path = settings_path()?;
    let text = std::fs::read_to_string(path).ok()?;
    toml::from_str(&text).ok()
}

#[cfg(not(target_arch = "wasm32"))]
fn save_settings(settings: &AppSettings) {
    let Some(path) = settings_path() else {
        return;
    };
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(text) = toml::to_string_pretty(settings) {
        let _ = std::fs::write(path, text);
    }
}

#[cfg(target_arch = "wasm32")]
const CONFIG_KEY: &str = "scu-client-config";

#[cfg(target_arch = "wasm32")]
fn load_config() -> Option<ConnectConfig> {
    let window = web_sys::window()?;
    let storage = window.local_storage().ok()??;
    let text = storage.get_item(CONFIG_KEY).ok()??;
    toml::from_str(&text).ok()
}

#[cfg(target_arch = "wasm32")]
fn save_config(config: &ConnectConfig) {
    let Some(window) = web_sys::window() else {
        return;
    };
    let Ok(Some(storage)) = window.local_storage() else {
        return;
    };
    if let Ok(text) = toml::to_string_pretty(config) {
        let _ = storage.set_item(CONFIG_KEY, &text);
    }
}

#[cfg(target_arch = "wasm32")]
const SETTINGS_KEY: &str = "scu-client-settings";

#[cfg(target_arch = "wasm32")]
fn load_settings() -> Option<AppSettings> {
    let window = web_sys::window()?;
    let storage = window.local_storage().ok()??;
    let text = storage.get_item(SETTINGS_KEY).ok()??;
    toml::from_str(&text).ok()
}

#[cfg(target_arch = "wasm32")]
fn save_settings(settings: &AppSettings) {
    let Some(window) = web_sys::window() else {
        return;
    };
    let Ok(Some(storage)) = window.local_storage() else {
        return;
    };
    if let Ok(text) = toml::to_string_pretty(settings) {
        let _ = storage.set_item(SETTINGS_KEY, &text);
    }
}

#[cfg(test)]
mod tests {
    use super::parse_frequency_text as parse;

    #[test]
    fn dotted_triplet_reads_as_hz() {
        assert_eq!(parse("14.265.160"), Some(14_265_160));
        assert_eq!(parse("145.500.000"), Some(145_500_000));
        assert_eq!(parse("1.900.000"), Some(1_900_000));
    }

    #[test]
    fn mhz_shorthand() {
        assert_eq!(parse("14.265"), Some(14_265_000));
        assert_eq!(parse("7.074"), Some(7_074_000));
        assert_eq!(parse("50.125"), Some(50_125_000));
        assert_eq!(parse("14."), Some(14_000_000));
    }

    #[test]
    fn integers_infer_a_unit() {
        // 7+ digits are whole Hz.
        assert_eq!(parse("14265160"), Some(14_265_160));
        assert_eq!(parse("7100000"), Some(7_100_000));
        // Up to 6 digits are treated as kHz.
        assert_eq!(parse("14265"), Some(14_265_000));
        assert_eq!(parse("7007"), Some(7_007_000));
    }

    #[test]
    fn separators_are_ignored() {
        assert_eq!(parse("14 265 160"), Some(14_265_160));
        assert_eq!(parse("14,265"), Some(14_265_000));
        assert_eq!(parse(" 14.265 "), Some(14_265_000));
    }

    #[test]
    fn rejects_unusable_input() {
        assert_eq!(parse(""), None);
        assert_eq!(parse("   "), None);
        assert_eq!(parse("abc"), None);
        assert_eq!(parse("0"), None);
        assert_eq!(parse("0.000.000"), None);
    }
}
