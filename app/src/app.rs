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
#[cfg(not(target_arch = "wasm32"))]
use scu_audio::vox::{Vox, VoxConfig};
use scu_cat::{self, Agc, MeterKind, Mode, RadioModel, ScopeMode};
use scu_client::{ConnectConfig, Event, ScuClient, ScuHandle};
use scu_scope::{BinInterleave, Colormap, FrequencyAxis};
use serde::{Deserialize, Serialize};

use crate::layout::{self, LayoutsFile, Pane};
use crate::shortcuts;
use crate::theme;
use crate::waterfall::Waterfall;

#[cfg(not(target_arch = "wasm32"))]
use crate::cat_server::CatServerState;

/// Messages from the background session engine to the UI.
enum EngineMsg {
    Handle(ScuHandle),
    Event(Event),
    Failed(String),
    Stopped,
}

/// A modal prompt for naming a layout: either saving the current tree as a new
/// preset, or renaming an existing one (by id).
#[derive(Clone)]
enum LayoutPrompt {
    SaveAs,
    Rename(String),
}

/// UI preferences, persisted separately from the connection config.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppSettings {
    /// RM indices (`MeterKind::from_rm_index`) shown in the side rail.
    #[serde(default = "default_visible_meters")]
    pub visible_meters: Vec<u8>,
    #[serde(default = "default_true")]
    pub show_cat_console: bool,
    /// Start the external CAT server (native rigctld endpoint) alongside the session.
    #[serde(default)]
    pub cat_server_enabled: bool,
    #[serde(default = "default_rigctld_port")]
    pub cat_server_port: u16,
    /// Route RX to a loopback device for external software.
    #[serde(default)]
    pub rx_stream_enabled: bool,
    #[serde(default)]
    pub rx_stream_device: Option<String>,
    /// Take TX audio from a loopback device instead of the local microphone.
    #[serde(default)]
    pub tx_stream_enabled: bool,
    #[serde(default)]
    pub tx_stream_device: Option<String>,
    /// Voice-operated transmit.
    #[serde(default)]
    pub vox_enabled: bool,
    #[serde(default = "default_vox_threshold")]
    pub vox_threshold: u16,
    #[serde(default = "default_vox_attack")]
    pub vox_attack_ms: u32,
    #[serde(default = "default_vox_hang")]
    pub vox_hang_ms: u32,
    /// Active colour palette.
    #[serde(default)]
    pub theme_kind: theme::ThemeKind,
    /// UI zoom step (fonts, spacing, hit targets).
    #[serde(default)]
    pub ui_scale: theme::UiScale,
    /// Boosts contrast for low-vision use.
    #[serde(default)]
    pub high_contrast: bool,
    /// Enlarges control hit targets for easier pointing.
    #[serde(default)]
    pub large_targets: bool,
    /// Console log verbosity.
    #[serde(default)]
    pub log_level: LogLevel,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            visible_meters: default_visible_meters(),
            show_cat_console: true,
            cat_server_enabled: false,
            cat_server_port: default_rigctld_port(),
            rx_stream_enabled: false,
            rx_stream_device: None,
            tx_stream_enabled: false,
            tx_stream_device: None,
            vox_enabled: false,
            vox_threshold: default_vox_threshold(),
            vox_attack_ms: default_vox_attack(),
            vox_hang_ms: default_vox_hang(),
            theme_kind: theme::ThemeKind::default(),
            ui_scale: theme::UiScale::default(),
            high_contrast: false,
            large_targets: false,
            log_level: LogLevel::default(),
        }
    }
}

fn default_vox_threshold() -> u16 {
    scu_audio::vox::VoxConfig::default().threshold
}

fn default_vox_attack() -> u32 {
    scu_audio::vox::VoxConfig::default().attack_ms
}

fn default_vox_hang() -> u32 {
    scu_audio::vox::VoxConfig::default().hang_ms
}

fn default_rigctld_port() -> u16 {
    4532
}

fn default_visible_meters() -> Vec<u8> {
    METER_INDICES.to_vec()
}

fn default_true() -> bool {
    true
}

/// RM meter indices offered by the FTDX10 (`RM3` ..= `RM9`).
const METER_INDICES: [u8; 7] = [3, 4, 5, 6, 7, 8, 9];

/// Settle time allowed after a VFO switch before the per-VFO state is re-read.
/// The radio broadcasts crossed pre/post-switch values during this window.
const VFO_SWITCH_SETTLE: Duration = Duration::from_millis(300);

/// Console log verbosity, applied live to the tracing subscriber.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum LogLevel {
    Error,
    Warn,
    #[default]
    Info,
    Debug,
    Trace,
}

impl LogLevel {
    pub const ALL: [LogLevel; 5] = [
        LogLevel::Error,
        LogLevel::Warn,
        LogLevel::Info,
        LogLevel::Debug,
        LogLevel::Trace,
    ];

    pub fn label(self) -> &'static str {
        match self {
            LogLevel::Error => "Error",
            LogLevel::Warn => "Warning",
            LogLevel::Info => "Info",
            LogLevel::Debug => "Debug",
            LogLevel::Trace => "Trace",
        }
    }

    /// The string handed to `tracing_subscriber::EnvFilter`.
    pub fn filter(self) -> &'static str {
        match self {
            LogLevel::Error => "error",
            LogLevel::Warn => "warn",
            LogLevel::Info => "info",
            LogLevel::Debug => "debug",
            LogLevel::Trace => "trace",
        }
    }
}

/// Sections of the Settings window.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum SettingsTab {
    #[default]
    Appearance,
    Meters,
    Panes,
    Logging,
}

impl SettingsTab {
    const ALL: [SettingsTab; 4] = [
        SettingsTab::Appearance,
        SettingsTab::Meters,
        SettingsTab::Panes,
        SettingsTab::Logging,
    ];

    fn label(self) -> &'static str {
        match self {
            SettingsTab::Appearance => "Appearance",
            SettingsTab::Meters => "Meters",
            SettingsTab::Panes => "Panes",
            SettingsTab::Logging => "Logging",
        }
    }
}

pub struct ScuApp {
    config: ConnectConfig,

    handle: Option<ScuHandle>,
    audio: Option<AudioOutput>,
    engine_rx: Option<std::sync::mpsc::Receiver<EngineMsg>>,
    shutdown: Option<Arc<AtomicBool>>,
    connecting: bool,

    status: String,
    radio: Option<RadioModel>,
    radio_power: Option<bool>,
    frequency: u64,
    frequency_b: u64,
    rx_sub: bool,
    split: bool,
    rit_on: bool,
    xit_on: bool,
    /// One clarifier offset is shared by RIT and XIT on the FTDX10.
    clarifier_offset_hz: i32,
    noise_blanker: bool,
    noise_reduction: bool,
    auto_notch: bool,
    narrow: bool,
    agc: Option<Agc>,
    rf_gain: u8,
    squelch: u8,
    /// Operating mode per VFO (0 = A/Main, 1 = B/Sub).
    mode: [Option<Mode>; 2],
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
    /// Case-insensitive substring filter for the CAT console; empty shows all.
    cat_filter: String,
    /// When set, incoming CAT frames are not appended to the console log, so a
    /// busy stream can be frozen while reading or copying it.
    cat_paused: bool,

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
    /// Active section of the Settings window.
    settings_tab: SettingsTab,

    /// Keyboard-shortcuts help overlay.
    show_shortcuts: bool,
    /// Transient message from a shortcut (text, shown-since).
    notice: Option<(String, Instant)>,
    /// VFO frequency editors asked to take focus on the next frame.
    focus_freq: [bool; 2],
    /// IF width (`SH`) code per VFO; 0 is the radio's mode default.
    if_width: [u8; 2],
    /// IF shift (`IS`) in Hz per VFO.
    if_shift_hz: [i32; 2],
    /// While set and in the future, crossed per-VFO width/shift frames from the
    /// radio are ignored. Around a `VS` switch the radio emits the outgoing
    /// VFO's values for the P1=0-fixed `SH`/`IS` commands; the per-VFO state is
    /// re-read once the window closes. `MD` is per-VFO and not affected.
    vfo_switch_ignore_until: Option<Instant>,
    /// A post-switch per-VFO re-read is pending once the settle window closes.
    vfo_switch_refresh: bool,

    /// Active palette (source of truth; mirrored into the theme module).
    theme: theme::Theme,
    /// Last appearance settings pushed to egui, so changes apply once.
    applied_appearance: Option<(theme::ThemeKind, theme::UiScale, bool, bool)>,

    /// Persisted dock layout (working draft, active id and named presets).
    layouts: LayoutsFile,
    /// Set when the tree changes; drives debounced persistence.
    layout_dirty: bool,
    last_layout_save: Instant,
    /// Pending "save/rename layout" prompt, if any.
    layout_prompt: Option<LayoutPrompt>,
    /// Editable name buffer for the pending layout prompt, kept across frames.
    layout_prompt_input: String,
    /// Whether the layout prompt's name field still needs initial focus.
    layout_prompt_focus: bool,

    /// PTT button held this frame (set while rendering the Operate pane).
    ptt_held: bool,
    /// Panes asked to pop out this frame (processed after the tree render).
    popout_requests: Vec<Pane>,

    /// Audio fan-out the engine writes RX frames to (local + streaming outputs).
    audio_sinks: Arc<std::sync::Mutex<Vec<AudioSink>>>,

    #[cfg(not(target_arch = "wasm32"))]
    rx_stream: Option<AudioOutput>,
    #[cfg(not(target_arch = "wasm32"))]
    tx_stream: Option<MicInput>,
    #[cfg(not(target_arch = "wasm32"))]
    rx_stream_devices: Vec<String>,
    #[cfg(not(target_arch = "wasm32"))]
    tx_stream_devices: Vec<String>,
    #[cfg(not(target_arch = "wasm32"))]
    vox: Vox,
    #[cfg(not(target_arch = "wasm32"))]
    vox_last: Instant,

    #[cfg(not(target_arch = "wasm32"))]
    cat_server: CatServerState,
    /// Last observed external-PTT state, to detect rigctld key-ups.
    #[cfg(not(target_arch = "wasm32"))]
    external_ptt_seen: bool,

    last_poll: Instant,
}

impl ScuApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
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
        let theme = theme::Theme::for_kind(settings.theme_kind);
        theme::apply(
            &cc.egui_ctx,
            theme,
            settings.ui_scale,
            settings.high_contrast,
            settings.large_targets,
        );
        let appearance = (
            settings.theme_kind,
            settings.ui_scale,
            settings.high_contrast,
            settings.large_targets,
        );
        let layouts = LayoutsFile::load();
        // RUST_LOG wins; otherwise apply the saved level to the subscriber.
        #[cfg(not(target_arch = "wasm32"))]
        if std::env::var_os("RUST_LOG").is_none() {
            crate::logging::set_level(settings.log_level.filter());
        }
        Self {
            config,
            handle: None,
            audio: None,
            engine_rx: None,
            shutdown: None,
            connecting: false,
            status: "disconnected".into(),
            radio: None,
            radio_power: None,
            frequency: 0,
            frequency_b: 0,
            rx_sub: false,
            split: false,
            rit_on: false,
            xit_on: false,
            clarifier_offset_hz: 0,
            noise_blanker: false,
            noise_reduction: false,
            auto_notch: false,
            narrow: false,
            agc: None,
            rf_gain: 128,
            squelch: 0,
            mode: [None, None],
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
            cat_filter: String::new(),
            cat_paused: false,
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
            settings_tab: SettingsTab::default(),
            show_shortcuts: false,
            notice: None,
            focus_freq: [false, false],
            if_width: [0, 0],
            if_shift_hz: [0, 0],
            vfo_switch_ignore_until: None,
            vfo_switch_refresh: false,
            theme,
            applied_appearance: Some(appearance),
            layouts,
            layout_dirty: false,
            last_layout_save: Instant::now(),
            layout_prompt: None,
            layout_prompt_input: String::new(),
            layout_prompt_focus: false,
            ptt_held: false,
            popout_requests: Vec::new(),
            audio_sinks: Arc::new(std::sync::Mutex::new(Vec::new())),
            #[cfg(not(target_arch = "wasm32"))]
            rx_stream: None,
            #[cfg(not(target_arch = "wasm32"))]
            tx_stream: None,
            #[cfg(not(target_arch = "wasm32"))]
            rx_stream_devices: AudioOutput::devices(),
            #[cfg(not(target_arch = "wasm32"))]
            tx_stream_devices: MicInput::devices(),
            #[cfg(not(target_arch = "wasm32"))]
            vox: Vox::new(),
            #[cfg(not(target_arch = "wasm32"))]
            vox_last: Instant::now(),
            #[cfg(not(target_arch = "wasm32"))]
            cat_server: CatServerState::new(),
            #[cfg(not(target_arch = "wasm32"))]
            external_ptt_seen: false,
            last_poll: Instant::now(),
        }
    }

    fn connect(&mut self, ctx: egui::Context) {
        if self.connecting || self.engine_rx.is_some() {
            return;
        }
        save_config(&self.config);
        self.ensure_audio();
        self.sync_audio_sinks();

        let config = self.config.clone();
        let sinks = Arc::clone(&self.audio_sinks);
        let (tx, rx) = std::sync::mpsc::channel();
        let shutdown = Arc::new(AtomicBool::new(false));

        self.engine_rx = Some(rx);
        self.shutdown = Some(Arc::clone(&shutdown));
        self.connecting = true;
        self.status = "connecting...".into();

        if start_engine(config, sinks, tx, shutdown, ctx).is_err() {
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
        self.sync_cat_server();
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
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.tx_stream = None;
        }
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
                output.set_enabled(!self.muted && !self.tx_keyed());
                output.set_stereo(self.stereo);
                self.audio = Some(output);
            }
            Err(e) => {
                self.status = format!("audio unavailable: {e}");
            }
        }
    }

    /// Rebuild the fan-out list the engine writes RX frames to: the local
    /// playback device plus, when enabled, the streaming loopback device.
    fn sync_audio_sinks(&self) {
        let mut sinks = self.audio_sinks.lock().unwrap();
        sinks.clear();
        if let Some(audio) = &self.audio {
            sinks.push(audio.sink());
        }
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(stream) = &self.rx_stream {
            sinks.push(stream.sink());
        }
    }

    /// RX playback is silenced while muted or transmitting (avoids the mic
    /// picking up the speakers).
    fn sync_audio_enabled(&self) {
        if let Some(audio) = &self.audio {
            audio.set_enabled(!self.muted && !self.tx_keyed());
        }
    }

    /// Start or stop the external CAT server to match the current settings and
    /// connection state.
    #[cfg(not(target_arch = "wasm32"))]
    fn sync_cat_server(&mut self) {
        let enabled = self.settings.cat_server_enabled && self.handle.is_some();
        if !enabled {
            self.cat_server.stop();
            return;
        }
        if self.cat_server.running() {
            return;
        }
        let Some(handle) = self.handle.clone() else {
            return;
        };
        self.cat_server.start(handle, self.settings.cat_server_port);
    }

    #[cfg(target_arch = "wasm32")]
    fn sync_cat_server(&mut self) {}

    /// Reconcile audio gating when an external rigctld client keys the radio:
    /// the app must enable whichever TX audio source is active.
    #[cfg(not(target_arch = "wasm32"))]
    fn reconcile_external_ptt(&mut self) {
        let external = self.cat_server.external_ptt_active();
        if external == self.external_ptt_seen {
            return;
        }
        self.external_ptt_seen = external;
        self.sync_tx_sources();
        self.sync_audio_enabled();
    }

    /// `true` when the radio should be transmitting, whether keyed locally
    /// (button/space/VOX) or by an external rigctld client.
    fn tx_keyed(&self) -> bool {
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.ptt || self.cat_server.external_ptt_active()
        }
        #[cfg(target_arch = "wasm32")]
        {
            self.ptt
        }
    }

    /// Side-rail section for the built-in rigctld server.
    #[cfg(not(target_arch = "wasm32"))]
    fn ui_cat_server(&mut self, ui: &mut egui::Ui) {
        theme::section(ui, "Radio Server (CAT)");
        ui.label(
            egui::RichText::new(
                "Expose the radio to WSJT-X, fldigi, N1MM and other Hamlib clients over the network.",
            )
            .small()
            .color(theme::text_faint()),
        );

        let mut enabled = self.settings.cat_server_enabled;
        if ui
            .checkbox(&mut enabled, "Enable rigctld server")
            .on_hover_text("Serve the rigctld protocol so external software can control the radio")
            .changed()
        {
            self.settings.cat_server_enabled = enabled;
            save_settings(&self.settings);
            self.sync_cat_server();
        }

        ui.horizontal(|ui| {
            ui.label("Port");
            let mut port = self.settings.cat_server_port;
            if ui
                .add(egui::DragValue::new(&mut port).range(1..=65535).speed(1.0))
                .changed()
            {
                self.settings.cat_server_port = port;
                save_settings(&self.settings);
            }
        });

        let status = self.cat_server.status();
        if status.running {
            ui.label(
                egui::RichText::new(format!(
                    "Listening on 127.0.0.1:{} ({} client{})",
                    status.port,
                    status.clients,
                    if status.clients == 1 { "" } else { "s" }
                ))
                .strong()
                .color(theme::rx_green()),
            );
            ui.label(
                egui::RichText::new("Point clients at Hamlib NET rigctl / rigctld")
                    .small()
                    .color(theme::text_faint()),
            );
        } else {
            ui.label(egui::RichText::new("Stopped").color(theme::text_dim()));
        }
        if let Some(error) = &status.error {
            ui.label(egui::RichText::new(error).small().color(theme::warn_amber()));
        }
    }

    /// Side-rail section for routing RX/TX audio to a loopback device so
    /// external software (WSJT-X, fldigi, …) can share the radio's audio.
    #[cfg(not(target_arch = "wasm32"))]
    fn ui_audio_streaming(&mut self, ui: &mut egui::Ui) {
        theme::section(ui, "Audio Streaming");
        ui.label(
            egui::RichText::new(
                "Bridge RX/TX to a virtual device (BlackHole, Loopback, Common-Radio, VB-Cable).",
            )
            .small()
            .color(theme::text_faint()),
        );

        // RX: radio -> loopback -> external app.
        let mut rx_enabled = self.settings.rx_stream_enabled;
        if ui
            .checkbox(&mut rx_enabled, "Stream RX to a device")
            .on_hover_text(
                "Play decoded RX audio to a virtual device the external app records from",
            )
            .changed()
        {
            self.settings.rx_stream_enabled = rx_enabled;
            save_settings(&self.settings);
            self.sync_rx_stream();
        }
        if let Some(choice) = device_combo(
            ui,
            "rx-stream-device",
            "Output",
            &self.rx_stream_devices,
            &self.settings.rx_stream_device,
        ) {
            self.settings.rx_stream_device = choice;
            save_settings(&self.settings);
            if self.settings.rx_stream_enabled {
                self.sync_rx_stream();
            }
        }

        // TX: external app -> loopback -> radio.
        let mut tx_enabled = self.settings.tx_stream_enabled;
        if ui
            .checkbox(&mut tx_enabled, "Take TX from a device")
            .on_hover_text(
                "Send audio captured from a virtual device to the radio (replaces the mic)",
            )
            .changed()
        {
            self.settings.tx_stream_enabled = tx_enabled;
            save_settings(&self.settings);
            if tx_enabled {
                if let Some(handle) = self.handle.clone() {
                    self.ensure_tx_stream(handle);
                }
            } else {
                self.tx_stream = None;
            }
            self.sync_tx_sources();
        }
        if let Some(choice) = device_combo(
            ui,
            "tx-stream-device",
            "Input",
            &self.tx_stream_devices,
            &self.settings.tx_stream_device,
        ) {
            self.settings.tx_stream_device = choice;
            save_settings(&self.settings);
            if self.settings.tx_stream_enabled {
                self.tx_stream = None;
                if let Some(handle) = self.handle.clone() {
                    self.ensure_tx_stream(handle);
                }
                self.sync_tx_sources();
            }
        }
        ui.label(
            egui::RichText::new("TX streaming replaces the local microphone while enabled.")
                .small()
                .color(theme::text_faint()),
        );

        if ui.button("Rescan audio devices").clicked() {
            self.rx_stream_devices = AudioOutput::devices();
            self.tx_stream_devices = MicInput::devices();
            self.mic_devices = MicInput::devices();
        }
    }

    /// Side-rail section for voice-operated transmit.
    #[cfg(not(target_arch = "wasm32"))]
    fn ui_vox(&mut self, ui: &mut egui::Ui) {
        theme::section(ui, "VOX (audio keying)");
        let mut enabled = self.settings.vox_enabled;
        if ui
            .checkbox(&mut enabled, "Enable VOX")
            .on_hover_text("Key the transmitter from the audio level instead of PTT")
            .changed()
        {
            self.settings.vox_enabled = enabled;
            if !enabled {
                self.vox.reset();
                if self.ptt {
                    self.set_ptt(false);
                }
            }
            save_settings(&self.settings);
        }
        if !self.settings.vox_enabled {
            return;
        }

        let level = self.tx_level();
        ui.add(
            egui::ProgressBar::new((level as f32 / 32767.0).clamp(0.0, 1.0))
                .desired_height(10.0)
                .fill(if self.tx_keyed() { theme::tx_red() } else { theme::rx_green() })
                .text(if self.tx_keyed() { "TX" } else { "RX" }),
        );
        ui.label(
            egui::RichText::new(format!(
                "Input {level} / threshold {}",
                self.settings.vox_threshold
            ))
            .small()
            .color(theme::text_faint()),
        );

        let mut threshold = self.settings.vox_threshold;
        if ui
            .add(egui::Slider::new(&mut threshold, 0..=32767).text("Threshold"))
            .changed()
        {
            self.settings.vox_threshold = threshold;
            save_settings(&self.settings);
        }
        let mut attack = self.settings.vox_attack_ms as i32;
        if ui
            .add(egui::Slider::new(&mut attack, 0..=1000).text("Attack (ms)"))
            .changed()
        {
            self.settings.vox_attack_ms = attack as u32;
            save_settings(&self.settings);
        }
        let mut hang = self.settings.vox_hang_ms as i32;
        if ui
            .add(egui::Slider::new(&mut hang, 0..=3000).text("Hang (ms)"))
            .changed()
        {
            self.settings.vox_hang_ms = hang as u32;
            save_settings(&self.settings);
        }
        ui.label(
            egui::RichText::new("Uses whichever TX audio source is active (mic or streaming).")
                .small()
                .color(theme::text_faint()),
        );
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
                self.mic_status = format!(
                    "mic: {} ({} Hz, {} ch)",
                    mic.info().device_name,
                    mic.info().sample_rate,
                    mic.info().channels
                );
                self.mic = Some(mic);
                self.sync_tx_sources();
            }
            Err(e) => self.mic_status = format!("mic unavailable: {e}"),
        }
    }

    /// Open the streaming (external-software) TX input device, if enabled.
    #[cfg(not(target_arch = "wasm32"))]
    fn ensure_tx_stream(&mut self, handle: ScuHandle) {
        if !self.settings.tx_stream_enabled || self.tx_stream.is_some() {
            return;
        }
        let sink: TxAudioSink = Box::new(move |body: &[u8]| handle.send_tx_audio(body));
        let config = MicConfig {
            device: self.settings.tx_stream_device.clone(),
            gain: 1.0,
        };
        match MicInput::new(config, sink) {
            Ok(mic) => {
                self.tx_stream = Some(mic);
                self.sync_tx_sources();
            }
            Err(e) => {
                self.settings.tx_stream_enabled = false;
                self.status = format!("TX stream unavailable: {e}");
            }
        }
    }

    /// Enable exactly one TX audio source (microphone or streaming device)
    /// while transmitting.
    fn sync_tx_sources(&mut self) {
        let keyed = self.tx_keyed();
        #[cfg(not(target_arch = "wasm32"))]
        let use_stream = self.settings.tx_stream_enabled && self.tx_stream.is_some();
        #[cfg(target_arch = "wasm32")]
        let use_stream = false;

        if let Some(mic) = &self.mic {
            mic.set_enabled(keyed && !use_stream);
        }
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(stream) = &self.tx_stream {
            stream.set_enabled(keyed && use_stream);
        }
    }

    /// (Re)build the RX streaming output to match settings: open a loopback
    /// device and add it to the engine's sink fan-out, or drop it.
    #[cfg(not(target_arch = "wasm32"))]
    fn sync_rx_stream(&mut self) {
        self.rx_stream = None;
        if self.settings.rx_stream_enabled {
            match AudioOutput::with_device(self.settings.rx_stream_device.clone()) {
                Ok(output) => {
                    output.set_enabled(true);
                    output.set_volume(1.0);
                    self.rx_stream = Some(output);
                }
                Err(e) => self.status = format!("RX stream unavailable: {e}"),
            }
        }
        self.sync_audio_sinks();
    }

    /// Current peak level of the active TX audio source.
    #[cfg(not(target_arch = "wasm32"))]
    fn tx_level(&self) -> u16 {
        if self.settings.tx_stream_enabled {
            self.tx_stream.as_ref().map(MicInput::level).unwrap_or(0)
        } else {
            self.mic.as_ref().map(MicInput::level).unwrap_or(0)
        }
    }

    /// Advance the VOX state machine and key/unkey the transmitter.
    #[cfg(not(target_arch = "wasm32"))]
    fn update_vox(&mut self) {
        let now = Instant::now();
        let dt = now.duration_since(self.vox_last).as_millis().min(500) as u32;
        self.vox_last = now;

        if !self.settings.vox_enabled || self.handle.is_none() {
            if self.vox.keyed() {
                self.vox.reset();
                self.set_ptt(false);
            }
            return;
        }

        let config = VoxConfig {
            threshold: self.settings.vox_threshold,
            attack_ms: self.settings.vox_attack_ms,
            hang_ms: self.settings.vox_hang_ms,
        };
        let level = self.tx_level();
        let keyed = self.vox.update(level, &config, dt);
        if keyed != self.ptt {
            self.set_ptt(keyed);
        }
    }

    /// Key (`on = true`) or unkey the transmitter and gate TX capture.
    fn set_ptt(&mut self, on: bool) {
        if self.ptt == on {
            return;
        }
        self.ptt = on;
        self.sync_tx_sources();
        self.sync_audio_enabled();
        if let Some(handle) = &self.handle {
            handle.set_transmit(on);
            self.push_log(format!("> {}", scu_cat::transmit(on)));
        }
    }

    fn initial_queries(&self) {
        if let Some(handle) = &self.handle {
            for cmd in [
                // `VS;` first, then `MD0;`/`MD1;`: on the FTDX10 the `MD` P1
                // digit is relative to the operating VFO, so the active VFO
                // needs to be known to route the two answers.
                "ID;", "FA;", "FB;", "VS;", "MD0;", "MD1;", "FT;", "ST;", "SH0;", "SH1;", "IS0;",
                "IS1;", "SM0;", "PS;", "PC;", "MG;", "AC;", "RT;", "XT;", "NB0;", "NR0;", "BC0;",
                "NA0;", "GT0;", "RG0;", "SQ0;", "TX;", "AI1;", "SS05;", "SS06;",
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
                #[cfg(not(target_arch = "wasm32"))]
                {
                    self.rx_stream_devices = AudioOutput::devices();
                    self.tx_stream_devices = MicInput::devices();
                    self.sync_rx_stream();
                    self.ensure_tx_stream(handle.clone());
                }
                self.handle = Some(handle);
                self.connecting = false;
                self.scope_centered_sent = false;
                self.meters = [None; 10];
                self.status = "connected".into();
                self.initial_queries();
                self.sync_cat_server();
            }
            EngineMsg::Event(event) => self.handle_event(event),
            EngineMsg::Failed(error) => {
                self.connecting = false;
                self.engine_rx = None;
                self.shutdown = None;
                self.stop_tx();
                self.status = format!("connect failed: {error}");
                self.sync_cat_server();
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
                self.sync_cat_server();
            }
        }
    }

    fn handle_event(&mut self, event: Event) {
        match event {
            Event::Connected { session_id } => {
                self.status = format!("connected (session 0x{session_id:02X})");
                self.initial_queries();
            }
            Event::Radio(model) => {
                // `ID;` answers are surfaced as a model, not a raw CAT frame;
                // reconstruct the frame so the bridge can satisfy rigctld.
                #[cfg(not(target_arch = "wasm32"))]
                self.cat_server.apply(&format!("ID{:04};", model.id()));
                self.radio = Some(model);
            }
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
                self.sync_cat_server();
            }
        }
    }

    fn on_cat(&mut self, text: &str) {
        // The radio streams meters continuously; let the user freeze the
        // console so the frames they care about aren't pushed out.
        if !self.cat_paused {
            self.push_log(text.to_string());
        }
        let settling = self
            .vfo_switch_ignore_until
            .is_some_and(|until| Instant::now() < until);
        let command = scu_cat::split(text).map(|frame| frame.command);
        // Drop the crossed pre/post-switch `SH`/`IS` frames (those commands are
        // P1=0-fixed on the FTDX10 and get broadcast for whichever VFO is
        // operating). `MD` is per-VFO and tagged with the VFO it describes, so
        // its answers are never ambiguous and are applied normally.
        if settling && matches!(command, Some("SH" | "IS")) {
            return;
        }
        #[cfg(not(target_arch = "wasm32"))]
        self.cat_server.apply(text);
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
            "VS" => {
                if let Some(sub) = scu_cat::parse_vfo(text) {
                    if sub != self.rx_sub {
                        self.begin_vfo_switch(sub);
                    }
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
            "CF" => {
                if let Some(hz) = scu_cat::parse_clarifier_offset(text) {
                    self.clarifier_offset_hz = hz;
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
            "SH" => {
                if let Some(code) = scu_cat::parse_if_width(text) {
                    let sub = frame_vfo_sub(text, self.rx_sub);
                    self.if_width[sub as usize] = code;
                }
            }
            "IS" => {
                if let Some(hz) = scu_cat::parse_if_shift(text) {
                    let sub = frame_vfo_sub(text, self.rx_sub);
                    self.if_shift_hz[sub as usize] = hz;
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
                // FTDX10 quirk: `MD P1` is relative to the *operating* VFO
                // (`MD0` = the active VFO, `MD1` = the other one), not fixed to
                // VFO-A/VFO-B like `FA`/`FB`. Translate the digit through the
                // known active VFO so each VFO gets its own mode.
                if let Some(mode) = scu_cat::parse_mode(text) {
                    let p1 = frame_vfo_sub(text, false);
                    let sub = md_vfo_sub(p1, self.rx_sub);
                    self.mode[sub as usize] = Some(mode);
                }
            }
            "PC" => {
                if let Some(watts) = scu_cat::parse_power(text) {
                    self.tx_power = watts.clamp(scu_cat::POWER_MIN_W, scu_cat::POWER_MAX_W);
                }
            }
            "PS" => {
                if let Some(on) = scu_cat::parse_radio_power(text) {
                    self.radio_power = Some(on);
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
        if self.cat_log.len() > 2000 {
            let excess = self.cat_log.len() - 2000;
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
        let current = if sub {
            self.frequency_b
        } else {
            self.frequency
        };
        let next = (current as i64 + steps * step).clamp(0, 999_999_990) as u64;
        if next != current {
            self.send_frequency(sub, next);
        }
    }

    /// Restore `sub`'s entry field to the radio's current frequency.
    fn reset_frequency_input_for(&mut self, sub: bool) {
        let hz = if sub {
            self.frequency_b
        } else {
            self.frequency
        };
        self.freq_input[sub as usize] = freq_input_text(hz);
        self.freq_editing[sub as usize] = false;
    }

    /// Set the active receive VFO's mode (keyboard shortcuts).
    fn set_mode(&mut self, mode: Mode) {
        self.set_mode_for(self.rx_sub, mode);
    }

    /// Set `sub`'s mode, apply it locally and send it to the radio.
    ///
    /// The FTDX10's `MD` P1 selects the active (`0`) or inactive (`1`) VFO
    /// rather than a fixed VFO-A/VFO-B, so the target is translated against the
    /// currently-active VFO. This still lets the inactive VFO's mode be changed.
    fn set_mode_for(&mut self, sub: bool, mode: Mode) {
        // Apply locally so the dropdown updates immediately; the radio's `MD`
        // echo (or the next poll) confirms it.
        self.mode[sub as usize] = Some(mode);
        if let Some(handle) = &self.handle {
            let p1 = md_vfo_sub(sub, self.rx_sub);
            handle.send_cat(&scu_cat::set_mode_vfo(p1, mode));
        }
    }

    /// Operating mode of the active receive VFO.
    fn active_mode(&self) -> Option<Mode> {
        self.mode[self.rx_sub as usize]
    }

    /// IF width code of the active receive VFO.
    fn active_if_width(&self) -> u8 {
        self.if_width[self.rx_sub as usize]
    }

    /// IF shift (Hz) of the active receive VFO.
    fn active_if_shift_hz(&self) -> i32 {
        self.if_shift_hz[self.rx_sub as usize]
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
        if let Some(handle) = &self.handle {
            handle.send_cat(scu_cat::select_vfo(sub));
        }
        self.begin_vfo_switch(sub);
    }

    /// Record that the operating VFO changed. `SH`/`IS` are P1=0-fixed on the
    /// FTDX10 and the radio broadcasts crossed values around the switch, so
    /// those frames are ignored for a short settle window and the per-VFO state
    /// is re-read afterwards (see [`Self::maybe_refresh_vfo`]).
    fn begin_vfo_switch(&mut self, sub: bool) {
        self.rx_sub = sub;
        // The waterfall history belongs to the previous VFO's frequency; drop
        // it so the display doesn't look stuck on the old centre.
        self.waterfall.clear();
        self.vfo_switch_ignore_until = Some(Instant::now() + VFO_SWITCH_SETTLE);
        self.vfo_switch_refresh = true;
    }

    /// Once the settle window after a VFO switch closes, re-read the per-VFO
    /// state and re-assert the active frequency so the radio's native scope
    /// recentres. Modes are read for both VFOs (`MD0`/`MD1` each address one
    /// VFO), so the radio is the source of truth for whichever mode each VFO
    /// holds and no app-side mode memory or re-apply is needed.
    fn maybe_refresh_vfo(&mut self) {
        if !self.vfo_switch_refresh {
            return;
        }
        if self
            .vfo_switch_ignore_until
            .is_some_and(|until| Instant::now() < until)
        {
            return;
        }
        self.vfo_switch_refresh = false;
        self.vfo_switch_ignore_until = None;
        let cf = if self.rx_sub { "CF101;" } else { "CF001;" };
        if let Some(handle) = &self.handle {
            for command in ["MD0;", "MD1;", "SH0;", "SH1;", "IS0;", "IS1;", cf] {
                handle.send_cat(command);
            }
            // The native scope follows a *tune*, not the CAT VFO select, so
            // re-asserting the active VFO's frequency forces it to recentre.
            let hz = self.active_frequency();
            if hz != 0 {
                let set = if self.rx_sub {
                    scu_cat::set_frequency_b(hz)
                } else {
                    scu_cat::set_frequency(hz)
                };
                handle.send_cat(&set);
            }
            // Belt and braces: make sure the scope is back in CENTER/follow.
            if self.follow_vfo {
                if let Some(mode) = self.scope_mode {
                    handle.send_cat(&scu_cat::set_scope_mode(mode.with_center().code()));
                }
            }
        }
    }

    /// Power the transceiver up (`on = true`) or put it into standby.
    ///
    /// The SCU-LAN10 stays reachable while the radio is off, so this is the
    /// only way to bring it up remotely.
    fn set_radio_power(&mut self, on: bool) {
        self.radio_power = Some(on);
        let command = scu_cat::set_radio_power(on);
        if let Some(handle) = &self.handle {
            handle.send_cat(command);
            self.push_log(format!("> {command}"));
        }
    }

    fn set_split(&mut self, on: bool) {
        self.split = on;
        if let Some(handle) = &self.handle {
            handle.send_cat(scu_cat::set_split(on));
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

    /// Set the shared clarifier offset for the active VFO.
    fn set_clarifier_offset(&mut self, hz: i32) {
        self.clarifier_offset_hz = scu_cat::clamp_clarifier_hz(hz);
        if let Some(handle) = &self.handle {
            handle.send_cat(&scu_cat::set_clarifier_offset(
                self.rx_sub,
                self.clarifier_offset_hz,
            ));
        }
    }

    fn clear_clarifier(&mut self) {
        self.clarifier_offset_hz = 0;
        if let Some(handle) = &self.handle {
            handle.send_cat(scu_cat::clear_clarifier());
        }
    }

    fn set_noise_blanker(&mut self, on: bool) {
        self.noise_blanker = on;
        if let Some(handle) = &self.handle {
            handle.send_cat(&scu_cat::set_noise_blanker(on));
        }
    }

    fn set_noise_reduction(&mut self, on: bool) {
        self.noise_reduction = on;
        if let Some(handle) = &self.handle {
            handle.send_cat(&scu_cat::set_noise_reduction(on));
        }
    }

    fn set_auto_notch(&mut self, on: bool) {
        self.auto_notch = on;
        if let Some(handle) = &self.handle {
            handle.send_cat(&scu_cat::set_auto_notch(on));
        }
    }

    fn set_narrow(&mut self, on: bool) {
        self.narrow = on;
        if let Some(handle) = &self.handle {
            handle.send_cat(&scu_cat::set_narrow(on));
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
            // `MD0;`/`MD1;` are relative to the operating VFO, so `VS;` is
            // polled first to keep that association current.
            let cf = if self.rx_sub { "CF101;" } else { "CF001;" };
            if let Some(handle) = &self.handle {
                for cmd in [
                    "FA;", "FB;", "VS;", "MD0;", "MD1;", "ST;", "SH0;", "SH1;", "IS0;", "IS1;", cf,
                    "SM0;", "PC;", "MG;", "AC;", "RT;", "XT;", "NB0;", "NR0;", "BC0;", "NA0;",
                    "GT0;", "RG0;", "SQ0;", "TX;", "SS05;", "RM3;", "RM4;", "RM5;", "RM6;", "RM7;",
                    "RM8;", "RM9;",
                ] {
                    handle.send_cat(cmd);
                }
            }
        }
    }

    fn axis(&self) -> FrequencyAxis {
        FrequencyAxis::new(self.active_frequency() as f64, self.span_hz)
    }

    fn paint_spectrum(&self, painter: &egui::Painter, rect: egui::Rect) {
        painter.rect_filled(rect, 4.0, theme::inset_bg());
        let midline = egui::Stroke::new(1.0, theme::outline());
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
                theme::text_dim(),
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
            egui::Stroke::new(1.2, theme::spectrum_green()),
        ));
    }

    fn paint_frequency_axis(&self, painter: &egui::Painter, rect: egui::Rect) {
        if self.active_frequency() == 0 {
            return;
        }
        let axis = self.axis();
        let color = theme::text();
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
        let center = egui::Stroke::new(1.0, theme::warn_amber());
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
                            .color(theme::accent()),
                    );
                    ui.separator();

                    let mut config_changed = false;
                    ui.label(
                        egui::RichText::new("HOST")
                            .small()
                            .color(theme::text_faint()),
                    );
                    config_changed |= ui
                        .add(egui::TextEdit::singleline(&mut self.config.host).desired_width(120.0))
                        .changed();
                    ui.label(
                        egui::RichText::new("PORT")
                            .small()
                            .color(theme::text_faint()),
                    );
                    config_changed |= ui
                        .add(
                            egui::DragValue::new(&mut self.config.base_port)
                                .range(1..=65535)
                                .speed(1.0),
                        )
                        .changed();
                    #[cfg(target_arch = "wasm32")]
                    {
                        ui.label(
                            egui::RichText::new("BRIDGE")
                                .small()
                                .color(theme::text_faint()),
                        );
                        let mut bridge = self.config.bridge_url.clone().unwrap_or_default();
                        if ui
                            .add(egui::TextEdit::singleline(&mut bridge).desired_width(160.0))
                            .changed()
                        {
                            self.config.bridge_url = Some(bridge);
                            config_changed = true;
                        }
                    }
                    ui.label(
                        egui::RichText::new("USER")
                            .small()
                            .color(theme::text_faint()),
                    );
                    config_changed |= ui
                        .add(
                            egui::TextEdit::singleline(&mut self.config.username)
                                .desired_width(90.0),
                        )
                        .changed();
                    ui.label(
                        egui::RichText::new("PASS")
                            .small()
                            .color(theme::text_faint()),
                    );
                    config_changed |= ui
                        .add(
                            egui::TextEdit::singleline(&mut self.config.password)
                                .password(true)
                                .desired_width(90.0),
                        )
                        .changed();
                    if config_changed {
                        save_config(&self.config);
                    }

                    ui.separator();
                    if self.connected() {
                        if ui
                            .add(egui::Button::new(
                                egui::RichText::new("Disconnect").strong().color(theme::tx_red()),
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
                        .on_hover_text("Show appearance and meter options")
                        .clicked()
                    {
                        self.show_settings = !self.show_settings;
                    }

                    ui.separator();
                    ui.menu_button("Layouts", |ui| self.layouts_menu(ui));
                    ui.menu_button("Panels", |ui| self.panels_menu(ui));
                    ui.menu_button(
                        format!("Theme: {}", self.settings.theme_kind.label()),
                        |ui| self.theme_menu(ui),
                    );
                    ui.menu_button(
                        format!("Scale: {}", self.settings.ui_scale.label()),
                        |ui| self.scale_menu(ui),
                    );
                    ui.separator();

                    ui.label(egui::RichText::new(&self.status).color(self.status_color()));
                    if let Some(note) = self.active_notice() {
                        ui.separator();
                        ui.label(egui::RichText::new(note).small().color(theme::warn_amber()));
                    }

                    if let Some(radio) = self.radio {
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.label(
                                egui::RichText::new(radio.name())
                                    .monospace()
                                    .color(theme::text_dim()),
                            );
                        });
                    }
                });
            });
    }

    /// Colour-coded connection status text.
    fn status_color(&self) -> egui::Color32 {
        if self.status.starts_with("connected") {
            theme::rx_green()
        } else if self.status.contains("failed")
            || self.status.contains("unavailable")
            || self.status.starts_with("disconnected:")
        {
            theme::tx_red()
        } else {
            theme::text_dim()
        }
    }

    /// Operate pane: transmit key, ATU tune and the quick operate toggles.
    fn pane_operate(&mut self, ui: &mut egui::Ui) {
        let mut ptt_held = false;
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ptt_held = self.ptt_button(ui);
                if self.tune_button(ui) {
                    self.start_atu_tune();
                }
            });
            ui.add_space(10.0);
            ui.separator();
            ui.add_space(6.0);
            ui.vertical(|ui| self.quick_toggles(ui));
        });
        self.ptt_held = ptt_held;
    }

    /// One VFO read-out pane, sized to the available width. Carries the VFO's
    /// S-meter and the VFO switch/copy shortcuts.
    fn pane_vfo(&mut self, ui: &mut egui::Ui, sub: bool) {
        let width = ui.available_width().max(180.0);
        self.vfo_card(ui, sub, width);
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            if ui
                .add_enabled(self.handle.is_some(), egui::Button::new("A/B").small())
                .on_hover_text("Switch the operating VFO (VFO-A / VFO-B)")
                .clicked()
            {
                self.select_rx_vfo(!self.rx_sub);
            }
            if ui
                .add_enabled(self.handle.is_some(), egui::Button::new("A->B").small())
                .on_hover_text("Copy VFO-A to VFO-B")
                .clicked()
            {
                self.copy_a_to_b();
            }
            if ui
                .add_enabled(self.handle.is_some(), egui::Button::new("B->A").small())
                .on_hover_text("Copy VFO-B to VFO-A")
                .clicked()
            {
                self.copy_b_to_a();
            }
        });
    }

    /// Spectrum pane: panadapter trace plus click-to-tune.
    fn pane_spectrum(&mut self, ui: &mut egui::Ui) {
        let size = ui.available_size();
        let (response, painter) =
            ui.allocate_painter(egui::Vec2::new(size.x, size.y.max(80.0)), egui::Sense::click());
        self.paint_spectrum(&painter, response.rect);
        self.tune_interaction(&response, &painter, response.rect);
    }

    /// Waterfall pane: scrolling texture plus the frequency axis.
    fn pane_waterfall(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        self.waterfall.update_texture(&ctx);
        let size = egui::Vec2::new(ui.available_width(), ui.available_height().max(80.0));
        let (response, painter) = ui.allocate_painter(size, egui::Sense::click());
        self.waterfall.paint(&painter, response.rect);
        self.paint_frequency_axis(&painter, response.rect);
        self.tune_interaction(&response, &painter, response.rect);
    }

    /// Tab title for a pane, with live VFO state for the VFO panes.
    pub(crate) fn pane_tab_title(&self, pane: Pane) -> String {
        match pane {
            Pane::VfoA => self.vfo_pane_title(false),
            Pane::VfoB => self.vfo_pane_title(true),
            _ => pane.title().to_string(),
        }
    }

    fn vfo_pane_title(&self, sub: bool) -> String {
        let letter = if sub { "B" } else { "A" };
        let mut title = format!("VFO {letter}");
        if self.rx_sub == sub {
            title.push_str(" \u{25cf}");
        }
        if self.tx_on(sub) {
            title.push_str(" TX");
        }
        title
    }

    /// Render one dock pane.
    pub(crate) fn render_pane(&mut self, pane: Pane, ui: &mut egui::Ui, _theme: theme::Theme) {
        let scrolls = !matches!(
            pane,
            Pane::Spectrum | Pane::Waterfall | Pane::VfoA | Pane::VfoB
        );
        if scrolls {
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| self.render_pane_inner(pane, ui));
        } else {
            self.render_pane_inner(pane, ui);
        }
    }

    fn render_pane_inner(&mut self, pane: Pane, ui: &mut egui::Ui) {
        match pane {
            Pane::VfoA => self.pane_vfo(ui, false),
            Pane::VfoB => self.pane_vfo(ui, true),
            Pane::Operate => self.pane_operate(ui),
            Pane::Spectrum => self.pane_spectrum(ui),
            Pane::Waterfall => self.pane_waterfall(ui),
            Pane::Radio => self.pane_radio(ui),
            Pane::Tuning => self.pane_tuning(ui),
            Pane::Clarifier => self.pane_clarifier(ui),
            Pane::Dsp => self.pane_dsp(ui),
            Pane::Receiver => self.pane_receiver(ui),
            Pane::Meters => self.pane_meters(ui),
            Pane::Scope => self.pane_scope(ui),
            Pane::Audio => self.pane_audio(ui),
            Pane::Transmit => self.pane_transmit(ui),
            Pane::CatConsole => self.pane_cat_console(ui),
            Pane::CatServer => {
                #[cfg(not(target_arch = "wasm32"))]
                self.ui_cat_server(ui);
                #[cfg(target_arch = "wasm32")]
                ui.label("Not available in the browser build.");
            }
            Pane::AudioStreaming => {
                #[cfg(not(target_arch = "wasm32"))]
                self.ui_audio_streaming(ui);
                #[cfg(target_arch = "wasm32")]
                ui.label("Not available in the browser build.");
            }
            Pane::Vox => {
                #[cfg(not(target_arch = "wasm32"))]
                self.ui_vox(ui);
                #[cfg(target_arch = "wasm32")]
                ui.label("Not available in the browser build.");
            }
        }
    }

    /// Big red/green push-to-talk key. Returns `true` while held down.
    fn ptt_button(&self, ui: &mut egui::Ui) -> bool {
        let on = self.tx_keyed();
        let fill = if on {
            theme::tx_red()
        } else {
            egui::Color32::from_rgb(48, 54, 64)
        };
        let stroke = if on { theme::tx_red() } else { theme::outline() };
        let text_color = if on { theme::on_accent() } else { theme::text() };
        let label = if on { "TX" } else { "PTT" };
        let button = egui::Button::new(
            egui::RichText::new(label)
                .strong()
                .size(19.0)
                .color(text_color),
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
            .fill(if self.atu_on {
                theme::accent_deep()
            } else {
                theme::button_bg()
            });
        ui.add_enabled(self.handle.is_some(), button)
            .on_hover_text("Start an ATU tuning cycle (keys a carrier)")
            .clicked()
    }

    /// One VFO read-out card (Main = VFO-A, Sub = VFO-B).
    fn vfo_card(&mut self, ui: &mut egui::Ui, sub: bool, width: f32) {
        let active = self.rx_sub == sub;
        let border = if active { theme::accent() } else { theme::outline() };
        let name = if sub { "SUB" } else { "MAIN" };
        let vfo = if sub { "B" } else { "A" };
        // Keep the frequency inside the card at any window width.
        let freq_size = (width / 7.5).clamp(18.0, 30.0);

        egui::Frame::new()
            .fill(theme::card_bg())
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
                            .color(if active { theme::accent() } else { theme::text_dim() }),
                    );
                    ui.label(
                        egui::RichText::new(format!("VFO {vfo}"))
                            .small()
                            .color(theme::text_faint()),
                    );
                    // A bounded right-aligned cluster; an unbounded
                    // right-to-left layout would swallow all remaining width.
                    ui.allocate_ui_with_layout(
                        egui::Vec2::new(78.0, 20.0),
                        egui::Layout::right_to_left(egui::Align::Center),
                        |ui| {
                            let (fill, text_color) = if active {
                                (theme::rx_green(), theme::on_accent())
                            } else {
                                (theme::button_bg(), theme::text_dim())
                            };
                            if ui
                                .add(
                                    egui::Button::new(
                                        egui::RichText::new("RX")
                                            .small()
                                            .strong()
                                            .color(text_color),
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
                                ui.label(egui::RichText::new("TX").small().strong().color(theme::tx_red()));
                            }
                        },
                    );
                });

                // The big read-out doubles as an inline editor: it shows the
                // plain cyan digits until clicked, then becomes a text field.
                let idx = sub as usize;
                let freq = if sub {
                    self.frequency_b
                } else {
                    self.frequency
                };
                let edit_id = ui.make_persistent_id(("vfo-freq", idx));
                if self.freq_editing[idx] {
                    let response = ui
                        .add(
                            egui::TextEdit::singleline(&mut self.freq_input[idx])
                                .id(edit_id)
                                .font(egui::FontId::monospace(freq_size))
                                .text_color(if active { theme::freq_cyan() } else { theme::text_dim() })
                                .desired_width(width)
                                .frame(egui::Frame::NONE)
                                .margin(egui::Margin::ZERO),
                        )
                        .on_hover_text("Enter to apply, Esc to cancel, scroll to tune");

                    if std::mem::take(&mut self.focus_freq[idx]) {
                        ui.memory_mut(|m| m.request_focus(response.id));
                        select_all_text(ui.ctx(), response.id, self.freq_input[idx].chars().count());
                    }
                    if response.gained_focus() {
                        select_all_text(
                            ui.ctx(),
                            response.id,
                            self.freq_input[idx].chars().count(),
                        );
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
                                    .color(if active { theme::freq_cyan() } else { theme::text_dim() }),
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

                // The S-meter lives with the VFO it belongs to: bright green on
                // the active receive VFO, greyed out otherwise.
                ui.add_space(3.0);
                let (s_fill, s_text) = if active {
                    (theme::rx_green(), theme::text())
                } else {
                    (theme::button_bg(), theme::text_faint())
                };
                ui.add(
                    egui::ProgressBar::new(MeterKind::S.fraction(self.smeter))
                        .desired_width(width)
                        .desired_height(16.0)
                        .fill(s_fill)
                        .corner_radius(theme::current().radius)
                        .text(
                            egui::RichText::new(MeterKind::S.format(self.smeter))
                                .strong()
                                .color(s_text),
                        ),
                )
                .on_hover_text(if active {
                    "S-meter (active receive VFO)"
                } else {
                    "S-meter (this VFO is not receiving)"
                });

                ui.horizontal(|ui| {
                    // Mode selector for *this* VFO. `MD` addresses the VFO
                    // directly, so the inactive VFO can be changed too.
                    let current = self.mode[sub as usize];
                    let mut selected = current;
                    ui.add_enabled_ui(self.handle.is_some(), |ui| {
                        egui::ComboBox::from_id_salt(("vfo-mode", idx))
                            .selected_text(current.map(|m| m.label()).unwrap_or("--"))
                            .width(92.0)
                            .show_ui(ui, |ui| {
                                for mode in Mode::ALL {
                                    ui.selectable_value(&mut selected, Some(mode), mode.label());
                                }
                            });
                    });
                    if selected != current {
                        if let Some(mode) = selected {
                            self.set_mode_for(sub, mode);
                        }
                    }
                    let band = if sub {
                        self.frequency_b
                    } else {
                        self.frequency
                    };
                    let band = scu_cat::band_for_hz(band)
                        .map(|band| band.name)
                        .unwrap_or("--");
                    let sub_label = if self.split {
                        format!("SPLIT \u{00b7} {band}")
                    } else {
                        format!("{band} \u{00b7} {}", span_label(self.span_hz))
                    };
                    ui.label(
                        egui::RichText::new(sub_label)
                            .small()
                            .color(theme::text_faint()),
                    );
                });
            });
    }

    /// Whether VFO `sub` is currently the transmit VFO.
    fn tx_on(&self, sub: bool) -> bool {
        if !self.tx_keyed() {
            return false;
        }
        let tx_sub = if self.split {
            !self.rx_sub
        } else {
            self.rx_sub
        };
        tx_sub == sub
    }

    /// Compact operate toggles next to the VFO cards.
    fn quick_toggles(&mut self, ui: &mut egui::Ui) {
        ui.set_min_width(148.0);
        theme::section(ui, "Operate");

        let (text, fill) = if self.tx_keyed() {
            ("TX  ON AIR", theme::tx_red())
        } else {
            ("RX  STANDBY", theme::rx_green())
        };
        ui.add(
            egui::Button::new(egui::RichText::new(text).strong().color(theme::on_accent()))
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

    /// Radio pane: power state and receive-VFO selection.
    fn pane_radio(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("RIG").small().strong().color(theme::accent()));
            let radio = self
                .radio
                .map(|r| r.name())
                .unwrap_or_else(|| "unknown".to_string());
            ui.label(egui::RichText::new(radio).color(theme::text_dim()));
        });

        theme::section(ui, "Power");
        ui.horizontal(|ui| {
            let (state, color) = match self.radio_power {
                Some(true) => ("ON", theme::rx_green()),
                Some(false) => ("STANDBY", theme::tx_red()),
                None => ("UNKNOWN", theme::text_dim()),
            };
            ui.label(egui::RichText::new(state).strong().color(color));
            let action = if self.radio_power == Some(true) {
                "Power off"
            } else {
                "Power on"
            };
            if ui
                .add_enabled(self.connected(), egui::Button::new(action))
                .on_hover_text(
                    "The SCU-LAN10 stays reachable while the radio is in \
                     standby; use this to power it on or off remotely.",
                )
                .clicked()
            {
                self.set_radio_power(self.radio_power != Some(true));
            }
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
                egui::RichText::new(format!("A {}", scu_cat::format_hz(self.frequency)))
                    .monospace()
                    .color(theme::text_dim()),
            );
            ui.label(
                egui::RichText::new(format!("B {}", scu_cat::format_hz(self.frequency_b)))
                    .monospace()
                    .color(theme::text_dim()),
            );
        });
    }

    /// Tuning pane: step size and up/down.
    fn pane_tuning(&mut self, ui: &mut egui::Ui) {
        theme::section(ui, "Tuning");
        ui.label(
            egui::RichText::new("Type directly on either VFO card above")
                .small()
                .color(theme::text_faint()),
        );
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("Step").small().color(theme::text_faint()));
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
    }

    /// Clarifier pane. The FTDX10 has one offset shared by RIT and XIT.
    fn pane_clarifier(&mut self, ui: &mut egui::Ui) {
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
        let mut offset = self.clarifier_offset_hz;
        if ui
            .add(
                egui::Slider::new(&mut offset, -9990..=9990)
                    .step_by(10.0)
                    .text("Clarifier offset")
                    .suffix(" Hz"),
            )
            .on_hover_text("One offset, shared by RIT and XIT")
            .changed()
        {
            self.set_clarifier_offset(offset);
        }
    }

    /// DSP pane.
    fn pane_dsp(&mut self, ui: &mut egui::Ui) {
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
    }

    /// Receiver pane: AGC, RF gain and squelch.
    fn pane_receiver(&mut self, ui: &mut egui::Ui) {
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
                egui::Slider::new(&mut rf_gain, 0..=scu_cat::RF_GAIN_MAX as i32).text("RF gain"),
            )
            .changed()
        {
            self.set_rf_gain(rf_gain as u8);
        }
        let mut squelch = self.squelch as i32;
        if ui
            .add(
                egui::Slider::new(&mut squelch, 0..=scu_cat::SQUELCH_MAX as i32).text("Squelch"),
            )
            .changed()
        {
            self.set_squelch(squelch as u8);
        }

        theme::section(ui, "Filter");
        let options = self.current_if_width_options();
        let current_width = self.active_if_width();
        let width_label = self.if_width_label(current_width);
        ui.horizontal(|ui| {
            ui.label("IF width");
            match &options {
                Some(options) => {
                    let mut choice = current_width;
                    egui::ComboBox::from_id_salt("if-width")
                        .selected_text(width_label.clone())
                        .show_ui(ui, |ui| {
                            for (code, hz) in options {
                                let label = if *code == 0 {
                                    "Default".to_string()
                                } else if *hz >= 1000 {
                                    format!("{:.1} kHz", *hz as f64 / 1000.0)
                                } else {
                                    format!("{hz} Hz")
                                };
                                ui.selectable_value(&mut choice, *code, label);
                            }
                        });
                    if choice != current_width {
                        self.set_if_width(choice);
                    }
                }
                None => {
                    ui.label(
                        egui::RichText::new("not available in this mode")
                            .small()
                            .color(theme::text_faint()),
                    );
                }
            }
        });

        let current_shift = self.active_if_shift_hz();
        let mut shift = current_shift;
        if ui
            .add(
                egui::Slider::new(
                    &mut shift,
                    scu_cat::IF_SHIFT_MIN_HZ..=scu_cat::IF_SHIFT_MAX_HZ,
                )
                .step_by(scu_cat::IF_SHIFT_STEP_HZ as f64)
                .text("IF shift")
                .suffix(" Hz"),
            )
            .changed()
        {
            let next = scu_cat::clamp_if_shift_hz(shift);
            if next != current_shift {
                self.set_if_shift_hz(next);
            }
        }
    }

    /// Meters pane.
    fn pane_meters(&mut self, ui: &mut egui::Ui) {
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
                ui.label(egui::RichText::new(kind.label()).monospace().strong());
                ui.add(
                    egui::ProgressBar::new(kind.fraction(raw))
                        .fill(theme::accent())
                        .text(egui::RichText::new(kind.format(raw)).color(theme::text())),
                );
            });
        }
    }

    /// Scope & waterfall settings pane.
    fn pane_scope(&mut self, ui: &mut egui::Ui) {
        theme::section(ui, "Scope & Waterfall");
        let mut selected_span: Option<usize> = None;
        ui.horizontal(|ui| {
            ui.label("Span");
            egui::ComboBox::from_id_salt("span")
                .selected_text(span_label(self.span_hz))
                .show_ui(ui, |ui| {
                    for (index, span) in scu_cat::SCOPE_SPANS_HZ.iter().enumerate() {
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
            .on_hover_text("Set the radio's scope to CENTER mode so the waterfall tracks tuning")
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
                        ui.selectable_value(&mut self.waterfall.colormap, map, map.label());
                    }
                });
        });
        ui.add(egui::Slider::new(&mut self.waterfall.black_level, 0.0..=0.9).text("Black"));
        ui.add(egui::Slider::new(&mut self.waterfall.gain, 0.2..=4.0).text("Gain"));
        ui.horizontal(|ui| {
            ui.label("Bins");
            egui::ComboBox::from_id_salt("bin-mode")
                .selected_text(self.scope_bins.label())
                .show_ui(ui, |ui| {
                    for mode in BinInterleave::ALL {
                        ui.selectable_value(&mut self.scope_bins, mode, mode.label());
                    }
                });
            if ui.button("Clear").clicked() {
                self.waterfall.clear();
            }
        });
    }

    /// Audio pane: mute, volume and stereo.
    fn pane_audio(&mut self, ui: &mut egui::Ui) {
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
    }

    /// Transmit pane: power, microphone gain and capture device.
    fn pane_transmit(&mut self, ui: &mut egui::Ui) {
        theme::section(ui, "Transmit");
        let mut power = self.tx_power as i32;
        if ui
            .add(
                egui::Slider::new(
                    &mut power,
                    scu_cat::POWER_MIN_W as i32..=scu_cat::POWER_MAX_W as i32,
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
                        let selected = self.mic_device.as_deref() == Some(name.as_str());
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
    }

    /// CAT console pane.
    fn pane_cat_console(&mut self, ui: &mut egui::Ui) {
        theme::section(ui, "CAT Console");
        ui.horizontal(|ui| {
            let response = ui.add(
                egui::TextEdit::singleline(&mut self.cat_input)
                    .desired_width(180.0)
                    .hint_text("e.g. IF;"),
            );
            if ui.button("Send").clicked()
                || (response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)))
            {
                self.send_cat_input();
            }
        });
        ui.horizontal(|ui| {
            ui.label("Filter");
            ui.add(
                egui::TextEdit::singleline(&mut self.cat_filter)
                    .desired_width(100.0)
                    .hint_text("e.g. MD"),
            )
            .on_hover_text("Show only lines containing this text (case-insensitive)");
            if ui.button("Clear").clicked() {
                self.cat_log.clear();
            }
            ui.checkbox(&mut self.cat_paused, "Pause")
                .on_hover_text("Freeze the console so incoming frames don't push lines out");
            if ui.button("Copy").clicked() {
                let filter = self.cat_filter.trim().to_ascii_lowercase();
                let lines: Vec<&str> = self
                    .cat_log
                    .iter()
                    .filter(|line| {
                        filter.is_empty() || line.to_ascii_lowercase().contains(&filter)
                    })
                    .map(String::as_str)
                    .collect();
                let count = lines.len();
                ui.ctx().copy_text(lines.join("\n"));
                self.notice =
                    Some((format!("Copied {count} CAT line(s) to clipboard"), Instant::now()));
            }
            #[cfg(not(target_arch = "wasm32"))]
            if ui.button("Save").clicked() {
                let filter = self.cat_filter.trim().to_ascii_lowercase();
                let text: String = self
                    .cat_log
                    .iter()
                    .filter(|line| {
                        filter.is_empty() || line.to_ascii_lowercase().contains(&filter)
                    })
                    .cloned()
                    .collect::<Vec<_>>()
                    .join("\n");
                let path = std::env::current_dir()
                    .unwrap_or_else(|_| std::path::PathBuf::from("."))
                    .join("cat-log.txt");
                self.notice = Some(match std::fs::write(&path, text) {
                    Ok(()) => (format!("Saved CAT log to {}", path.display()), Instant::now()),
                    Err(e) => (format!("Could not save CAT log: {e}"), Instant::now()),
                });
            }
        });
        let filter = self.cat_filter.trim().to_ascii_lowercase();
        egui::ScrollArea::vertical()
            .max_height(240.0)
            .stick_to_bottom(!self.cat_paused)
            .show(ui, |ui| {
                for line in &self.cat_log {
                    if filter.is_empty() || line.to_ascii_lowercase().contains(&filter) {
                        ui.label(egui::RichText::new(line).monospace().small());
                    }
                }
            });
    }

    /// Floating settings window, split into tabs: appearance, meters, panes and
    /// logging.
    fn ui_settings(&mut self, ctx: &egui::Context) {
        if !self.show_settings {
            return;
        }
        let mut open = self.show_settings;
        egui::Window::new("Settings")
            .open(&mut open)
            .collapsible(false)
            .resizable(true)
            .default_width(340.0)
            .default_height(400.0)
            .min_width(300.0)
            .show(ctx, |ui| {
                ui.horizontal_wrapped(|ui| {
                    for tab in SettingsTab::ALL {
                        if ui
                            .selectable_label(self.settings_tab == tab, tab.label())
                            .clicked()
                        {
                            self.settings_tab = tab;
                        }
                    }
                });
                ui.separator();
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| match self.settings_tab {
                        SettingsTab::Appearance => self.settings_appearance(ui),
                        SettingsTab::Meters => self.settings_meters(ui),
                        SettingsTab::Panes => self.settings_panes(ui),
                        SettingsTab::Logging => self.settings_logging(ui),
                    });
            });
        self.show_settings = open;
    }

    /// Appearance tab: palette, UI size and accessibility options.
    fn settings_appearance(&mut self, ui: &mut egui::Ui) {
        theme::section(ui, "Theme");
        ui.horizontal_wrapped(|ui| {
            for kind in theme::ThemeKind::ALL {
                if ui
                    .selectable_label(self.settings.theme_kind == kind, kind.label())
                    .clicked()
                {
                    self.settings.theme_kind = kind;
                    save_settings(&self.settings);
                }
            }
        });

        theme::section(ui, "UI size");
        ui.horizontal_wrapped(|ui| {
            for scale in theme::UiScale::ALL {
                if ui
                    .selectable_label(self.settings.ui_scale == scale, scale.label())
                    .clicked()
                {
                    self.settings.ui_scale = scale;
                    save_settings(&self.settings);
                }
            }
        });

        theme::section(ui, "Accessibility");
        let mut high_contrast = self.settings.high_contrast;
        if ui
            .checkbox(&mut high_contrast, "High contrast")
            .on_hover_text("Boost text and border contrast")
            .changed()
        {
            self.settings.high_contrast = high_contrast;
            save_settings(&self.settings);
        }
        let mut large_targets = self.settings.large_targets;
        if ui
            .checkbox(&mut large_targets, "Large targets")
            .on_hover_text("Enlarge control hit areas")
            .changed()
        {
            self.settings.large_targets = large_targets;
            save_settings(&self.settings);
        }
    }

    /// Meters tab: which `RM` meters are shown.
    fn settings_meters(&mut self, ui: &mut egui::Ui) {
        theme::section(ui, "Visible meters");
        ui.label(
            egui::RichText::new("Choose which meters appear in the Meters pane.")
                .small()
                .color(theme::text_faint()),
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
    }

    /// Panes tab: add/remove optional panels and reset the arrangement.
    fn settings_panes(&mut self, ui: &mut egui::Ui) {
        theme::section(ui, "Optional panes");
        let mut show = layout::pane_tile(&self.layouts.draft, Pane::CatConsole).is_some();
        if ui
            .checkbox(&mut show, "CAT console")
            .on_hover_text("Add or remove the CAT command log pane")
            .changed()
        {
            if show {
                layout::add_pane(&mut self.layouts.draft, Pane::CatConsole);
            } else {
                layout::remove_pane(&mut self.layouts.draft, Pane::CatConsole);
            }
            self.settings.show_cat_console = show;
            save_settings(&self.settings);
            self.mark_layout_dirty();
        }
        ui.label(
            egui::RichText::new("Use the Panels menu in the toolbar for every other pane.")
                .small()
                .color(theme::text_faint()),
        );

        theme::section(ui, "Layout");
        if ui
            .button("Reset arrangement to default")
            .on_hover_text("Discard the current arrangement and relayout the panes")
            .clicked()
        {
            self.reset_layout();
        }
    }

    /// Logging tab: console verbosity, applied to the tracing subscriber.
    fn settings_logging(&mut self, ui: &mut egui::Ui) {
        theme::section(ui, "Console log level");
        let before = self.settings.log_level;
        egui::ComboBox::from_id_salt("log-level")
            .selected_text(self.settings.log_level.label())
            .width(160.0)
            .show_ui(ui, |ui| {
                for level in LogLevel::ALL {
                    ui.selectable_value(&mut self.settings.log_level, level, level.label());
                }
            });
        if self.settings.log_level != before {
            crate::logging::set_level(self.settings.log_level.filter());
            save_settings(&self.settings);
        }

        ui.label(
            egui::RichText::new(
                "Controls how much the app writes to the console. Debug and Trace \
                 are useful when diagnosing a connection or protocol issue.",
            )
            .small()
            .color(theme::text_faint()),
        );
        #[cfg(target_arch = "wasm32")]
        ui.label(
            egui::RichText::new("The browser build keeps its console logger at its default level.")
                .small()
                .color(theme::text_faint()),
        );
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

        let shift = response.ctx.input(|i| i.modifiers.shift);

        if let Some(pos) = response.hover_pos() {
            let stroke = egui::Stroke::new(1.0, theme::text_dim());
            painter.line_segment(
                [
                    egui::pos2(pos.x, rect.top()),
                    egui::pos2(pos.x, rect.bottom()),
                ],
                stroke,
            );
            // With shift held the readout previews the nearest kHz, matching
            // what a shift-click will tune to.
            let label = if shift {
                format!(
                    "{} (1 kHz)",
                    format_hz_label(snap_hz(hz_at(pos.x), 1_000) as f64)
                )
            } else {
                format_hz_label(hz_at(pos.x))
            };
            painter.text(
                egui::pos2((pos.x + 4.0).min(rect.right() - 140.0), rect.top() + 3.0),
                egui::Align2::LEFT_TOP,
                label,
                egui::FontId::monospace(12.0),
                theme::warn_amber(),
            );
        }

        response.clone().on_hover_text(
            "Click to tune • Shift-click to snap to the nearest kHz • Scroll the VFO to step",
        );

        if response.clicked() {
            if let Some(pos) = response.interact_pointer_pos() {
                let hz = hz_at(pos.x);
                self.tune_to_hz(hz, shift);
            }
        }
    }

    fn tune_to_hz(&mut self, hz: f64, snap_khz: bool) {
        if hz <= 0.0 {
            return;
        }
        // A plain click keeps 10 Hz resolution (the scope bin resolution is
        // coarser than that); shift-click snaps to a whole kHz.
        let step = if snap_khz { 1_000 } else { 10 };
        let hz = snap_hz(hz, step);
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
        self.maybe_refresh_vfo();
        self.poll();
        #[cfg(not(target_arch = "wasm32"))]
        self.reconcile_external_ptt();
        #[cfg(not(target_arch = "wasm32"))]
        self.update_vox();
        self.maybe_save_layout();
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.apply_appearance(&ctx);
        self.handle_shortcuts(&ctx);
        self.ui_top(ui);

        // The Operate pane sets this while rendering; reset before the tree.
        self.ptt_held = false;
        egui::CentralPanel::default().show(ui, |ui| {
            if self.layouts.draft.is_empty() {
                ui.vertical_centered(|ui| {
                    ui.add_space(40.0);
                    ui.label("All panels are closed.");
                    if ui.button("Reset to default layout").clicked() {
                        self.reset_layout();
                    }
                });
                return;
            }
            let mut tree = std::mem::replace(
                &mut self.layouts.draft,
                egui_tiles::Tree::empty(layout::TREE_ID),
            );
            {
                let theme = self.theme;
                let mut behavior = layout::FlexBehavior { app: self, theme };
                tree.ui(&mut behavior, ui);
            }
            self.layouts.draft = tree;
        });

        // Pop-out requests were queued while the tree was borrowed; apply them
        // now, then draw the floating windows.
        self.process_popouts();
        self.render_popped(&ctx);

        let space = self.handle.is_some()
            && ctx.memory(|m| m.focused().is_none())
            && ctx.input(|i| i.key_down(egui::Key::Space));
        self.set_ptt(self.ptt_held || space);

        self.ui_settings(&ctx);
        self.ui_layout_prompt(&ctx);
        self.ui_shortcuts_help(&ctx);
        ctx.request_repaint_after(Duration::from_millis(16));
    }
}

impl ScuApp {
    fn connected(&self) -> bool {
        self.handle.is_some()
    }

    /// Push theme/scale/accessibility changes to egui when any of them change.
    fn apply_appearance(&mut self, ctx: &egui::Context) {
        let current = (
            self.settings.theme_kind,
            self.settings.ui_scale,
            self.settings.high_contrast,
            self.settings.large_targets,
        );
        if self.applied_appearance == Some(current) {
            return;
        }
        self.theme = theme::Theme::for_kind(self.settings.theme_kind);
        theme::apply(
            ctx,
            self.theme,
            self.settings.ui_scale,
            self.settings.high_contrast,
            self.settings.large_targets,
        );
        self.applied_appearance = Some(current);
    }

    pub(crate) fn mark_layout_dirty(&mut self) {
        self.layout_dirty = true;
    }

    /// When a named preset is active, keep its stored tree in step with edits.
    fn sync_active_preset(&mut self) {
        let id = self.layouts.active_id.clone();
        if let Some(preset) = self.layouts.presets.iter_mut().find(|p| p.id == id) {
            preset.tree = self.layouts.draft.clone();
        }
    }

    /// Persist the layout once the user stops rearranging it.
    fn maybe_save_layout(&mut self) {
        if !self.layout_dirty {
            return;
        }
        if self.last_layout_save.elapsed() < Duration::from_millis(500) {
            return;
        }
        self.persist_layout_now();
    }

    /// Persist the layout right away (used when presets are added, renamed or
    /// deleted, where waiting for the debounce risks losing the change).
    fn persist_layout_now(&mut self) {
        self.sync_active_preset();
        self.layouts.save();
        self.layout_dirty = false;
        self.last_layout_save = Instant::now();
    }

    fn reset_layout(&mut self) {
        self.layouts.active_id = layout::DEFAULT_ID.to_string();
        self.layouts.draft = layout::default_tree();
        self.mark_layout_dirty();
    }

    fn switch_layout(&mut self, id: &str) {
        if id == layout::DEFAULT_ID {
            self.layouts.active_id = layout::DEFAULT_ID.to_string();
            self.layouts.draft = layout::default_tree();
        } else if id == layout::DASHBOARD_ID {
            self.layouts.active_id = layout::DASHBOARD_ID.to_string();
            self.layouts.draft = layout::dashboard_tree();
        } else if let Some(preset) = self.layouts.presets.iter().find(|p| p.id == id) {
            self.layouts.active_id = preset.id.clone();
            self.layouts.draft = preset.tree.clone();
        }
        self.mark_layout_dirty();
    }

    fn save_current_as(&mut self, name: &str) {
        let mut n = self.layouts.presets.len() + 1;
        let mut id = format!("layout-{n}");
        while self.layouts.presets.iter().any(|p| p.id == id) {
            n += 1;
            id = format!("layout-{n}");
        }
        self.layouts.presets.push(layout::Preset {
            id: id.clone(),
            name: name.to_string(),
            tree: self.layouts.draft.clone(),
        });
        self.layouts.active_id = id;
        self.persist_layout_now();
    }

    fn rename_preset(&mut self, id: &str, name: &str) {
        if let Some(preset) = self.layouts.presets.iter_mut().find(|p| p.id == id) {
            preset.name = name.to_string();
        }
        self.persist_layout_now();
    }

    fn delete_preset(&mut self, id: &str) {
        self.layouts.presets.retain(|p| p.id != id);
        if self.layouts.active_id == id {
            self.switch_layout(layout::DEFAULT_ID);
        }
        self.persist_layout_now();
    }

    /// Whether `pane` is currently floating in its own window.
    pub(crate) fn is_popped(&self, pane: Pane) -> bool {
        self.layouts.popped.contains(&pane)
    }

    /// Queue a pane to be popped out after the current tree render.
    pub(crate) fn request_popout(&mut self, pane: Pane) {
        if !self.popout_requests.contains(&pane) {
            self.popout_requests.push(pane);
        }
    }

    /// Move a pane out of the dock tree and into its own window.
    fn process_popouts(&mut self) {
        if self.popout_requests.is_empty() {
            return;
        }
        for pane in std::mem::take(&mut self.popout_requests) {
            layout::remove_pane(&mut self.layouts.draft, pane);
            if !self.layouts.popped.contains(&pane) {
                self.layouts.popped.push(pane);
            }
        }
        self.mark_layout_dirty();
    }

    /// Return a floating pane to the dock tree.
    fn dock_pane(&mut self, pane: Pane) {
        self.layouts.popped.retain(|p| *p != pane);
        layout::add_pane(&mut self.layouts.draft, pane);
        self.mark_layout_dirty();
    }

    /// Draw each floating pane in its own OS window (an embedded window on the
    /// web, where multi-viewport is unavailable).
    fn render_popped(&mut self, ctx: &egui::Context) {
        let popped = self.layouts.popped.clone();
        for pane in popped {
            let viewport_id = egui::ViewportId::from_hash_of(("scu-popout", pane));
            let builder = egui::ViewportBuilder::default()
                .with_title(format!("SCU-LAN10 \u{2014} {}", pane.title()))
                .with_inner_size([420.0, 360.0])
                .with_min_inner_size([260.0, 180.0]);
            let theme = self.theme;
            let mut dock = false;
            let mut close_requested = false;
            ctx.show_viewport_immediate(viewport_id, builder, |ui, _class| {
                if ui.ctx().input(|i| i.viewport().close_requested()) {
                    close_requested = true;
                }
                egui::Panel::top(egui::Id::new(("scu-popout-bar", pane)))
                    .frame(theme::top_bar_frame())
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.label(egui::RichText::new(pane.title()).strong());
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    if ui
                                        .button("Dock")
                                        .on_hover_text("Return this panel to the main window")
                                        .clicked()
                                    {
                                        dock = true;
                                    }
                                },
                            );
                        });
                    });
                egui::CentralPanel::default().show(ui, |ui| {
                    self.render_pane(pane, ui, theme);
                });
            });
            if dock || close_requested {
                self.dock_pane(pane);
            }
        }
    }

    /// The Layouts toolbar menu.
    fn layouts_menu(&mut self, ui: &mut egui::Ui) {
        let active = self.layouts.active_id.clone();
        if ui
            .selectable_label(active == layout::DEFAULT_ID, "Default")
            .clicked()
        {
            self.switch_layout(layout::DEFAULT_ID);
            ui.close();
        }
        if ui
            .selectable_label(active == layout::DASHBOARD_ID, "Dashboard")
            .clicked()
        {
            self.switch_layout(layout::DASHBOARD_ID);
            ui.close();
        }
        if !self.layouts.presets.is_empty() {
            ui.separator();
        }

        let presets = self.layouts.presets.clone();
        for preset in presets {
            let mark = if active == preset.id { "\u{25cf} " } else { "" };
            let label = format!("{mark}{}", preset.name);
            ui.menu_button(label, |ui| {
                if ui.button("Switch to").clicked() {
                    self.switch_layout(&preset.id);
                    ui.close();
                }
                if ui.button("Rename…").clicked() {
                    self.layout_prompt_input = preset.name.clone();
                    self.layout_prompt_focus = true;
                    self.layout_prompt = Some(LayoutPrompt::Rename(preset.id.clone()));
                    ui.close();
                }
                if ui.button("Delete").clicked() {
                    self.delete_preset(&preset.id);
                    ui.close();
                }
            });
        }

        ui.separator();
        if ui.button("Save current as…").clicked() {
            let suggested = format!("Layout {}", self.layouts.presets.len() + 1);
            self.layout_prompt_input = suggested.clone();
            self.layout_prompt_focus = true;
            self.layout_prompt = Some(LayoutPrompt::SaveAs);
            ui.close();
        }
        if ui.button("Reset to default").clicked() {
            self.reset_layout();
            ui.close();
        }
    }

    /// The Panels toolbar menu: show/hide each available pane.
    fn panels_menu(&mut self, ui: &mut egui::Ui) {
        for pane in Pane::ALL {
            if !pane.available(true) {
                continue;
            }
            let popped = self.is_popped(pane);
            let open = popped || layout::pane_tile(&self.layouts.draft, pane).is_some();
            let label = if popped {
                format!("{} (floating)", pane.title())
            } else {
                pane.title().to_string()
            };
            if ui.selectable_label(open, label).clicked() {
                if popped {
                    self.dock_pane(pane);
                } else if open {
                    layout::remove_pane(&mut self.layouts.draft, pane);
                    self.mark_layout_dirty();
                } else {
                    layout::add_pane(&mut self.layouts.draft, pane);
                    self.mark_layout_dirty();
                }
            }
        }
    }

    /// The Theme toolbar menu.
    fn theme_menu(&mut self, ui: &mut egui::Ui) {
        for kind in theme::ThemeKind::ALL {
            if ui
                .selectable_label(self.settings.theme_kind == kind, kind.label())
                .clicked()
            {
                self.settings.theme_kind = kind;
                save_settings(&self.settings);
                ui.close();
            }
        }
    }

    /// The Scale toolbar menu.
    fn scale_menu(&mut self, ui: &mut egui::Ui) {
        for scale in theme::UiScale::ALL {
            if ui
                .selectable_label(self.settings.ui_scale == scale, scale.label())
                .clicked()
            {
                self.settings.ui_scale = scale;
                save_settings(&self.settings);
                ui.close();
            }
        }
    }

    /// Modal prompt for naming a layout (save-as or rename).
    fn ui_layout_prompt(&mut self, ctx: &egui::Context) {
        let Some(prompt) = self.layout_prompt.clone() else {
            return;
        };
        let mut open = true;
        let mut confirm = false;
        let mut cancel = false;
        let title = match &prompt {
            LayoutPrompt::SaveAs => "Save layout",
            LayoutPrompt::Rename(_) => "Rename layout",
        };
        egui::Window::new(title)
            .collapsible(false)
            .resizable(false)
            .open(&mut open)
            .show(ctx, |ui| {
                ui.label("Name");
                let response = ui.add(
                    egui::TextEdit::singleline(&mut self.layout_prompt_input).desired_width(220.0),
                );
                // Focus the field once when the prompt opens, so the user can
                // start typing immediately.
                if self.layout_prompt_focus {
                    response.request_focus();
                }
                if response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    confirm = true;
                }
                if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                    cancel = true;
                }
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    if ui.button("Save").clicked() {
                        confirm = true;
                    }
                    if ui.button("Cancel").clicked() {
                        cancel = true;
                    }
                });
            });
        self.layout_prompt_focus = false;

        if confirm {
            let name = self.layout_prompt_input.trim().to_string();
            if !name.is_empty() {
                match prompt {
                    LayoutPrompt::SaveAs => self.save_current_as(&name),
                    LayoutPrompt::Rename(id) => self.rename_preset(&id, &name),
                }
            }
            self.layout_prompt = None;
        } else if cancel || !open {
            self.layout_prompt = None;
        }
    }

    // ---- Keyboard shortcuts -------------------------------------------------

    /// A fresh transient shortcut message, if one is still within its window.
    fn active_notice(&self) -> Option<String> {
        self.notice.as_ref().and_then(|(message, at)| {
            (at.elapsed() < Duration::from_secs(4)).then(|| message.clone())
        })
    }

    fn show_notice(&mut self, message: impl Into<String>) {
        self.notice = Some((message.into(), Instant::now()));
    }

    /// This frame's key presses that resolve to a shortcut.
    fn collect_shortcuts(ctx: &egui::Context) -> Vec<(shortcuts::Chord, shortcuts::Action)> {
        ctx.input(|i| {
            i.events
                .iter()
                .filter_map(|event| {
                    let egui::Event::Key {
                        key,
                        physical_key,
                        pressed,
                        repeat,
                        modifiers,
                    } = event
                    else {
                        return None;
                    };
                    if !pressed {
                        return None;
                    }
                    let chord = shortcuts::Chord {
                        key: *key,
                        physical: *physical_key,
                        shift: modifiers.shift,
                        alt: modifiers.alt,
                        ctrl: modifiers.ctrl,
                        command: modifiers.command,
                        repeat: *repeat,
                    };
                    shortcuts::resolve(&chord).map(|action| (chord, action))
                })
                .collect()
        })
    }

    /// Dispatch keyboard shortcuts. Runs before the panels so that a frequency
    /// editor can take focus in the same frame.
    fn handle_shortcuts(&mut self, ctx: &egui::Context) {
        let escape = ctx.input(|i| i.key_pressed(egui::Key::Escape));
        let presses = Self::collect_shortcuts(ctx);

        // While the help overlay is open it owns the keyboard: Esc or the help
        // chord closes it, and everything else is swallowed.
        if self.show_shortcuts {
            if escape
                || presses
                    .iter()
                    .any(|(_, a)| *a == shortcuts::Action::ToggleHelp)
            {
                self.show_shortcuts = false;
            }
            return;
        }

        // The layout name prompt owns the keyboard until it closes.
        if self.layout_prompt.is_some() {
            return;
        }

        // Never steal keys while a text widget has focus (host, credentials,
        // the frequency editor, the CAT console, ...).
        if ctx.memory(|m| m.focused().is_some()) {
            return;
        }

        if escape && ctx.input(|i| i.viewport().fullscreen.unwrap_or(false)) {
            ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(false));
        }

        for (chord, action) in presses {
            if chord.repeat && !action.is_repeatable() {
                continue;
            }
            match action {
                shortcuts::Action::ToggleHelp => self.show_shortcuts = true,
                other => self.apply_shortcut(ctx, other),
            }
        }
    }

    /// Apply a resolved shortcut action.
    fn apply_shortcut(&mut self, ctx: &egui::Context, action: shortcuts::Action) {
        use shortcuts::Action;
        match action {
            Action::TuneUp => self.step_frequency(1),
            Action::TuneDown => self.step_frequency(-1),
            Action::TuneUp10 => self.step_frequency(10),
            Action::TuneDown10 => self.step_frequency(-10),
            Action::TuneUp100 => self.step_frequency(100),
            Action::TuneDown100 => self.step_frequency(-100),
            Action::FineTuneUp => self.nudge_active_hz(1),
            Action::FineTuneDown => self.nudge_active_hz(-1),
            Action::FineTuneUp50 => self.nudge_active_hz(50),
            Action::FineTuneDown50 => self.nudge_active_hz(-50),
            Action::SwitchVfo => self.select_rx_vfo(!self.rx_sub),
            Action::CopyAToB => self.copy_a_to_b(),
            Action::BandDown => self.cycle_band(false),
            Action::BandUp => self.cycle_band(true),
            Action::FocusFrequency => self.focus_frequency_entry(),
            Action::Mode(mode) => self.set_mode(mode),
            Action::IfWidthNarrower => self.cycle_if_width(-1),
            Action::IfWidthWider => self.cycle_if_width(1),
            Action::IfWidthDefault => self.restore_default_if_width(),
            Action::IfShiftUp => self.nudge_if_shift(20),
            Action::IfShiftDown => self.nudge_if_shift(-20),
            Action::IfShiftUpFast => self.nudge_if_shift(100),
            Action::IfShiftDownFast => self.nudge_if_shift(-100),
            Action::ScopeZoomIn | Action::ScopeNarrower => self.cycle_span(-1, false),
            Action::ScopeZoomOut | Action::ScopeWider => self.cycle_span(1, false),
            Action::ScopeZoomInExtreme => self.cycle_span(-1, true),
            Action::ScopeZoomOutExtreme => self.cycle_span(1, true),
            Action::ToggleVfoB => self.toggle_pane(Pane::VfoB),
            Action::ToggleFullscreen => self.toggle_fullscreen(ctx),
            Action::VolumeDown => self.nudge_volume(-0.05),
            Action::VolumeUp => self.nudge_volume(0.05),
            Action::ToggleMute => self.toggle_mute(),
            Action::ToggleHelp => self.show_shortcuts = true,
            Action::Unavailable(message) => self.show_notice(message),
        }
    }

    /// Nudge a VFO by an absolute number of Hz (fine tune).
    fn nudge_active_hz(&mut self, delta_hz: i64) {
        let current = self.active_frequency();
        let next = (current as i64 + delta_hz).clamp(0, 999_999_990) as u64;
        if next != current {
            self.send_frequency(self.rx_sub, next);
        }
    }

    /// Move the active VFO to the previous (`up = false`) or next band.
    fn cycle_band(&mut self, up: bool) {
        let count = scu_cat::HF_BANDS.len();
        let next = match scu_cat::nearest_band_index(self.active_frequency()) {
            Some(index) if up => (index + 1) % count,
            Some(index) => (index + count - 1) % count,
            None if up => 0,
            None => count - 1,
        };
        let band = scu_cat::HF_BANDS[next];
        self.send_frequency(self.rx_sub, band.calling_hz);
        self.show_notice(format!("Band {}", band.name));
    }

    /// Put the active VFO's frequency editor into text-edit mode and focus it.
    fn focus_frequency_entry(&mut self) {
        let index = self.rx_sub as usize;
        let hz = if self.rx_sub {
            self.frequency_b
        } else {
            self.frequency
        };
        self.freq_input[index] = freq_input_text(hz);
        self.freq_editing[index] = true;
        self.focus_freq[index] = true;
    }

    /// Mode-aware `(code, Hz)` table for the active VFO, if it has one.
    fn current_if_width_options(&self) -> Option<Vec<(u8, u16)>> {
        let model = self.radio.unwrap_or(RadioModel::Ftdx10);
        scu_cat::if_width_options(model, self.active_mode()?)
    }

    /// Human label for an `SH` code on the active mode.
    fn if_width_label(&self, code: u8) -> String {
        if code == 0 {
            return "Default".to_string();
        }
        match self
            .current_if_width_options()
            .and_then(|options| options.into_iter().find(|(c, _)| *c == code))
            .map(|(_, hz)| hz)
        {
            Some(hz) if hz >= 1000 => format!("{:.1} kHz", hz as f64 / 1000.0),
            Some(hz) if hz > 0 => format!("{hz} Hz"),
            _ => format!("Code {code}"),
        }
    }

    fn set_if_width(&mut self, code: u8) {
        self.if_width[self.rx_sub as usize] = code;
        if let Some(handle) = &self.handle {
            handle.send_cat(&scu_cat::set_if_width(self.rx_sub, code));
        }
        self.show_notice(format!("IF width {}", self.if_width_label(code)));
    }

    /// Step through the non-default IF widths for the current mode.
    fn cycle_if_width(&mut self, delta: i32) {
        let Some(codes) = self.current_if_width_options().map(|options| {
            options
                .into_iter()
                .map(|(code, _)| code)
                .filter(|code| *code != 0)
                .collect::<Vec<_>>()
        }) else {
            self.show_notice("IF width not available in this mode");
            return;
        };
        if codes.is_empty() {
            return;
        }
        let current = self.active_if_width();
        let next = match codes.iter().position(|code| *code == current) {
            Some(index) => (index as i32 + delta).clamp(0, codes.len() as i32 - 1) as usize,
            None if delta < 0 => 0,
            None => codes.len() - 1,
        };
        self.set_if_width(codes[next]);
    }

    fn restore_default_if_width(&mut self) {
        if self.current_if_width_options().is_none() {
            self.show_notice("IF width not available in this mode");
            return;
        }
        self.set_if_width(0);
    }

    /// Store and send an IF shift for the active VFO.
    fn set_if_shift_hz(&mut self, hz: i32) {
        self.if_shift_hz[self.rx_sub as usize] = hz;
        if let Some(handle) = &self.handle {
            handle.send_cat(&scu_cat::set_if_shift(self.rx_sub, hz));
        }
        self.show_notice(format!("IF shift {hz:+} Hz"));
    }

    fn nudge_if_shift(&mut self, delta_hz: i32) {
        let current = self.active_if_shift_hz();
        let next = scu_cat::clamp_if_shift_hz(current + delta_hz);
        if next != current {
            self.set_if_shift_hz(next);
        }
    }

    fn span_index(&self) -> Option<usize> {
        scu_cat::SCOPE_SPANS_HZ
            .iter()
            .position(|span| (self.span_hz - span).abs() < 0.5)
    }

    /// Step the scope span. `extreme` jumps to the narrowest / widest span.
    fn cycle_span(&mut self, delta: i32, extreme: bool) {
        let count = scu_cat::SCOPE_SPANS_HZ.len() as i32;
        let current = self.span_index().unwrap_or(7) as i32;
        let next = if extreme {
            if delta < 0 {
                0
            } else {
                count - 1
            }
        } else {
            (current + delta).clamp(0, count - 1)
        };
        self.set_span(next as usize);
        self.show_notice(format!("Span {}", span_label(self.span_hz)));
    }

    /// Show or hide one dock pane (also docks it if it is floating).
    fn toggle_pane(&mut self, pane: Pane) {
        if self.is_popped(pane) {
            self.dock_pane(pane);
            return;
        }
        if layout::pane_tile(&self.layouts.draft, pane).is_some() {
            layout::remove_pane(&mut self.layouts.draft, pane);
        } else {
            layout::add_pane(&mut self.layouts.draft, pane);
        }
        self.mark_layout_dirty();
    }

    fn toggle_fullscreen(&mut self, ctx: &egui::Context) {
        let full = ctx.input(|i| i.viewport().fullscreen.unwrap_or(false));
        ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(!full));
    }

    fn nudge_volume(&mut self, delta: f32) {
        let next = (self.volume + delta).clamp(0.0, 1.5);
        if (next - self.volume).abs() < f32::EPSILON {
            return;
        }
        self.volume = next;
        if let Some(audio) = &self.audio {
            audio.set_volume(next);
        }
        self.show_notice(format!("Volume {:.0}%", next * 100.0));
    }

    fn toggle_mute(&mut self) {
        self.muted = !self.muted;
        self.sync_audio_enabled();
        self.show_notice(if self.muted { "Muted" } else { "Unmuted" });
    }

    /// The `?` / `h` keyboard-shortcuts overlay.
    fn ui_shortcuts_help(&mut self, ctx: &egui::Context) {
        if !self.show_shortcuts {
            return;
        }
        let response = egui::Modal::new(egui::Id::new("shortcuts-help")).show(ctx, |ui| {
            ui.set_max_width(560.0);
            ui.heading("Keyboard shortcuts");
            ui.label(
                egui::RichText::new(
                    "Shortcuts are ignored while typing in a text field. Esc closes this dialog.",
                )
                .small()
                .color(theme::text_faint()),
            );
            ui.separator();
            egui::ScrollArea::vertical()
                .max_height(460.0)
                .show(ui, |ui| {
                    let mut group = "";
                    for row in shortcuts::HELP_ROWS {
                        if row.group != group {
                            group = row.group;
                            theme::section(ui, group);
                        }
                        ui.horizontal(|ui| {
                            ui.add_sized(
                                [150.0, 18.0],
                                egui::Label::new(
                                    egui::RichText::new(row.keys)
                                        .monospace()
                                        .strong()
                                        .color(theme::accent()),
                                ),
                            );
                            ui.label(row.action);
                            if let Some(note) = row.note {
                                ui.label(
                                    egui::RichText::new(format!("({note})"))
                                        .small()
                                        .color(theme::text_dim()),
                                );
                            }
                        });
                    }
                });
        });
        if response.should_close() {
            self.show_shortcuts = false;
        }
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
    sinks: Arc<std::sync::Mutex<Vec<AudioSink>>>,
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
                runtime.block_on(engine_future(config, sinks, tx, shutdown, ctx));
            })
            .map(|_| ())
    }

    #[cfg(target_arch = "wasm32")]
    {
        wasm_bindgen_futures::spawn_local(engine_future(config, sinks, tx, shutdown, ctx));
        Ok(())
    }
}

async fn engine_future(
    config: ConnectConfig,
    sinks: Arc<std::sync::Mutex<Vec<AudioSink>>>,
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
                for sink in sinks.lock().unwrap().iter() {
                    sink.push((*frame).clone());
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

/// The VFO (0 = A/Main, 1 = B/Sub) a CAT response is about, from its P1 digit.
/// Falls back to `fallback` when the frame has no payload.
fn frame_vfo_sub(frame: &str, fallback: bool) -> bool {
    scu_cat::split(frame)
        .and_then(|parsed| parsed.payload.chars().next())
        .map(|digit| digit == '1')
        .unwrap_or(fallback)
}

/// Translate an `MD` P1 digit into the physical VFO it describes (0 = A, 1 = B).
///
/// On the FTDX10 `MD P1` is relative to the operating VFO: `0` addresses the
/// active VFO and `1` the inactive one (unlike `FA`/`FB`, which are fixed to
/// VFO-A/VFO-B). `active_sub` is the current `rx_sub`; XOR-ing the relative
/// digit with it recovers the physical VFO, and symmetrically maps a target VFO
/// back to the digit to send.
fn md_vfo_sub(p1_is_one: bool, active_sub: bool) -> bool {
    active_sub ^ p1_is_one
}

fn span_label(span: f64) -> String {
    if span >= 1_000_000.0 {
        format!("{:.1} MHz", span / 1_000_000.0)
    } else {
        format!("{:.0} kHz", span / 1_000.0)
    }
}

/// A labelled combo box for choosing an audio device (or the system default).
/// Returns the new selection when the user changes it.
#[cfg(not(target_arch = "wasm32"))]
fn device_combo(
    ui: &mut egui::Ui,
    id: &str,
    label: &str,
    devices: &[String],
    current: &Option<String>,
) -> Option<Option<String>> {
    let mut choice = None;
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(label).small().color(theme::text_faint()));
        let text = current
            .clone()
            .unwrap_or_else(|| "System default".to_string());
        egui::ComboBox::from_id_salt(id)
            .selected_text(text)
            .show_ui(ui, |ui| {
                if ui
                    .selectable_label(current.is_none(), "System default")
                    .clicked()
                {
                    choice = Some(None);
                }
                for name in devices {
                    let selected = current.as_deref() == Some(name.as_str());
                    if ui.selectable_label(selected, name).clicked() {
                        choice = Some(Some(name.clone()));
                    }
                }
            });
    });
    choice
}

/// A compact toggle button that lights up with the accent colour when `on`.
fn toggle_chip(ui: &mut egui::Ui, on: bool, label: &str) -> bool {
    let fill = if on { theme::accent() } else { theme::button_bg() };
    let stroke = if on { theme::accent() } else { theme::outline() };
    let text_color = if on { theme::on_accent() } else { theme::text_dim() };
    ui.add(
        egui::Button::new(
            egui::RichText::new(label)
                .small()
                .strong()
                .color(text_color),
        )
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
        state
            .cursor
            .set_char_range(Some(egui::text::CCursorRange::two(
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

/// Round a frequency to the nearest multiple of `step` Hz.
fn snap_hz(hz: f64, step: u64) -> u64 {
    (hz / step as f64).round().max(0.0) as u64 * step
}

/// Full-frequency readout (`MHz.kHz.Hz`, e.g. `14.270.005`), so the scope shows
/// the exact Hz the radio will tune to rather than a rounded MHz value.
fn format_hz_label(hz: f64) -> String {
    scu_cat::format_hz(snap_hz(hz, 1))
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
    use super::{format_hz_label, frame_vfo_sub, md_vfo_sub, snap_hz};

    #[test]
    fn cat_frames_route_to_their_vfo() {
        assert!(!frame_vfo_sub("MD01;", true));
        assert!(frame_vfo_sub("MD12;", false));
        assert!(!frame_vfo_sub("SH0008;", true));
        assert!(frame_vfo_sub("SH1021;", false));
        assert!(frame_vfo_sub("IS10+0600;", false));
        assert!(!frame_vfo_sub("IS00+0600;", true));
        // No payload falls back to the caller's active VFO.
        assert!(frame_vfo_sub("IS;", true));
    }

    #[test]
    fn md_p1_is_relative_to_the_operating_vfo() {
        // FTDX10: `MD0` addresses the active VFO, `MD1` the inactive one. So
        // with VFO-B active, `MD0` is VFO-B and `MD1` is VFO-A...
        assert!(md_vfo_sub(false, true)); // MD0, B active -> VFO-B
        assert!(!md_vfo_sub(true, true)); // MD1, B active -> VFO-A
        // ...and with VFO-A active the mapping flips.
        assert!(!md_vfo_sub(false, false)); // MD0, A active -> VFO-A
        assert!(md_vfo_sub(true, false)); // MD1, A active -> VFO-B

        // The transform is its own inverse: mapping a target VFO to a digit
        // round-trips back to that VFO.
        for active in [false, true] {
            for target in [false, true] {
                let p1 = md_vfo_sub(target, active);
                assert_eq!(md_vfo_sub(p1, active), target);
            }
        }
    }

    #[test]
    fn snap_hz_rounds_to_step() {
        assert_eq!(snap_hz(14_270_005.4, 1_000), 14_270_000);
        assert_eq!(snap_hz(14_270_540.0, 1_000), 14_271_000);
        assert_eq!(snap_hz(14_270_004.9, 10), 14_270_000);
        assert_eq!(snap_hz(-5.0, 10), 0);
    }

    #[test]
    fn hz_label_shows_full_frequency() {
        assert_eq!(format_hz_label(14_270_004.6), "14.270.005");
        assert_eq!(format_hz_label(7_074_000.0), "7.074.000");
        assert_eq!(format_hz_label(531_000.0), "0.531.000");
    }

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
