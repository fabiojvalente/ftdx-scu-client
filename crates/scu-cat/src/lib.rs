//! Yaesu CAT command construction and response parsing.
//!
//! The CAT command set is not part of the SCU-LAN10 protocol; it is the
//! standard Yaesu CAT protocol documented in each radio's CAT Operation
//! Reference manual. This crate covers the subset needed by the client.

use std::fmt;

/// Build the "read VFO-A frequency" command.
pub fn read_frequency() -> String {
    "FA;".to_string()
}

/// Build a "set VFO-A frequency" command (9 digits, Hz).
pub fn set_frequency(hz: u64) -> String {
    format!("FA{:09};", hz.min(999_999_999))
}

/// Build a "read VFO-B frequency" command.
pub fn read_frequency_b() -> String {
    "FB;".to_string()
}

/// Build a "set VFO-B frequency" command (9 digits, Hz).
pub fn set_frequency_b(hz: u64) -> String {
    format!("FB{:09};", hz.min(999_999_999))
}

/// Build a "select receive VFO" command (`FR`). `sub = true` selects VFO-B.
pub fn select_rx_vfo(sub: bool) -> &'static str {
    if sub {
        "FR1;"
    } else {
        "FR0;"
    }
}

/// Build a "read receive VFO" command.
pub fn read_rx_vfo() -> &'static str {
    "FR;"
}

/// Parse the receive VFO from an `FR` response (`true` = VFO-B / Sub).
pub fn parse_rx_vfo(frame: &str) -> Option<bool> {
    parse_on_off(frame, "FR")
}

/// Build a "select transmit VFO" command (`FT`). `sub = true` selects VFO-B.
pub fn select_tx_vfo(sub: bool) -> &'static str {
    if sub {
        "FT1;"
    } else {
        "FT0;"
    }
}

/// Build a "read transmit VFO" command.
pub fn read_tx_vfo() -> &'static str {
    "FT;"
}

/// Parse the transmit VFO from an `FT` response (`true` = VFO-B / Sub).
pub fn parse_tx_vfo(frame: &str) -> Option<bool> {
    parse_on_off(frame, "FT")
}

/// Build a "split on/off" command (`ST1;` / `ST0;`).
pub fn set_split(on: bool) -> &'static str {
    if on {
        "ST1;"
    } else {
        "ST0;"
    }
}

/// Build a "read split state" command.
pub fn read_split() -> &'static str {
    "ST;"
}

/// Parse the split state from an `ST` response.
pub fn parse_split(frame: &str) -> Option<bool> {
    parse_on_off(frame, "ST")
}

/// Build a "swap VFO-A and VFO-B" command (`SV;`, write-only).
pub fn swap_vfo() -> &'static str {
    "SV;"
}

/// Build a "copy VFO-A to VFO-B" command (`AB;`, write-only).
pub fn copy_a_to_b() -> &'static str {
    "AB;"
}

/// Build a "copy VFO-B to VFO-A" command (`BA;`, write-only).
pub fn copy_b_to_a() -> &'static str {
    "BA;"
}

/// Build a "RIT (receive clarifier) on/off" command (`RT`).
pub fn set_rit(on: bool) -> &'static str {
    if on {
        "RT1;"
    } else {
        "RT0;"
    }
}

/// Build a "read RIT state" command.
pub fn read_rit() -> &'static str {
    "RT;"
}

/// Parse the RIT state from an `RT` response.
pub fn parse_rit(frame: &str) -> Option<bool> {
    parse_on_off(frame, "RT")
}

/// Build a "XIT (transmit clarifier) on/off" command (`XT`).
pub fn set_xit(on: bool) -> &'static str {
    if on {
        "XT1;"
    } else {
        "XT0;"
    }
}

/// Build a "read XIT state" command.
pub fn read_xit() -> &'static str {
    "XT;"
}

/// Parse the XIT state from an `XT` response.
pub fn parse_xit(frame: &str) -> Option<bool> {
    parse_on_off(frame, "XT")
}

/// Clarifier offset unit: the `RC` command expresses the offset in 10 Hz steps.
pub const CLARIFIER_STEP_HZ: i32 = 10;
/// Maximum clarifier offset magnitude (`RC` 4-digit field, ±9999 * 10 Hz).
pub const CLARIFIER_MAX_HZ: i32 = 9999 * CLARIFIER_STEP_HZ;

/// Clamp an arbitrary offset to the 10 Hz grid the radio accepts.
pub fn clamp_clarifier_hz(hz: i32) -> i32 {
    let steps = (hz as f64 / CLARIFIER_STEP_HZ as f64).round() as i32;
    steps.clamp(-9999, 9999) * CLARIFIER_STEP_HZ
}

/// Build a "set clarifier offset" command (`RC`).
///
/// `tx = false` sets the receive clarifier (RIT), `tx = true` the transmit
/// clarifier (XIT). The offset is quantized to 10 Hz.
pub fn set_clarifier(tx: bool, hz: i32) -> String {
    let hz = clamp_clarifier_hz(hz);
    let sign = if hz < 0 { '-' } else { '+' };
    let digits = (hz.abs() / CLARIFIER_STEP_HZ).min(9999);
    format!("RC{}{}{:04};", tx as u8, sign, digits)
}

/// Build a "read clarifier offset" command (`tx` selects RIT vs XIT).
pub fn read_clarifier(tx: bool) -> String {
    format!("RC{};", tx as u8)
}

/// Parse an `RC` clarifier response into `(tx, offset_hz)`.
pub fn parse_clarifier(frame: &str) -> Option<(bool, i32)> {
    let parsed = split(frame)?;
    if parsed.command != "RC" || parsed.payload.len() < 6 {
        return None;
    }
    let mut chars = parsed.payload.chars();
    let tx = match chars.next()? {
        '0' => false,
        '1' => true,
        _ => return None,
    };
    let sign: i32 = match chars.next()? {
        '+' => 1,
        '-' => -1,
        _ => return None,
    };
    let digits: i32 = chars.as_str()[..4].parse().ok()?;
    Some((tx, sign * digits * CLARIFIER_STEP_HZ))
}

/// Build a "noise blanker on/off" command (`NB`).
pub fn set_noise_blanker(on: bool) -> &'static str {
    if on {
        "NB1;"
    } else {
        "NB0;"
    }
}

/// Build a "read noise blanker state" command.
pub fn read_noise_blanker() -> &'static str {
    "NB;"
}

/// Parse the noise blanker state from an `NB` response.
pub fn parse_noise_blanker(frame: &str) -> Option<bool> {
    parse_on_off(frame, "NB")
}

/// Build a "noise reduction on/off" command (`NR`).
pub fn set_noise_reduction(on: bool) -> &'static str {
    if on {
        "NR1;"
    } else {
        "NR0;"
    }
}

/// Build a "read noise reduction state" command.
pub fn read_noise_reduction() -> &'static str {
    "NR;"
}

/// Parse the noise reduction state from an `NR` response.
pub fn parse_noise_reduction(frame: &str) -> Option<bool> {
    parse_on_off(frame, "NR")
}

/// Build an "auto notch on/off" command (`BC`).
pub fn set_auto_notch(on: bool) -> &'static str {
    if on {
        "BC1;"
    } else {
        "BC0;"
    }
}

/// Build a "read auto notch state" command.
pub fn read_auto_notch() -> &'static str {
    "BC;"
}

/// Parse the auto notch state from a `BC` response.
pub fn parse_auto_notch(frame: &str) -> Option<bool> {
    parse_on_off(frame, "BC")
}

/// Build a "narrow filter on/off" command (`NA`).
pub fn set_narrow(on: bool) -> &'static str {
    if on {
        "NA1;"
    } else {
        "NA0;"
    }
}

/// Build a "read narrow filter state" command (`NA0;`, MAIN).
pub fn read_narrow() -> &'static str {
    "NA0;"
}

/// Parse the narrow filter state from an `NA` response.
pub fn parse_narrow(frame: &str) -> Option<bool> {
    parse_on_off(frame, "NA")
}

/// AGC time constant (`GT` P2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Agc {
    Off,
    Fast,
    Mid,
    Slow,
    Auto,
    AutoFast,
    AutoMid,
    AutoSlow,
}

impl Agc {
    pub fn code(&self) -> char {
        match self {
            Agc::Off => '0',
            Agc::Fast => '1',
            Agc::Mid => '2',
            Agc::Slow => '3',
            Agc::Auto => '4',
            Agc::AutoFast => '5',
            Agc::AutoMid => '6',
            Agc::AutoSlow => '7',
        }
    }

    pub fn from_code(code: char) -> Option<Self> {
        Some(match code {
            '0' => Agc::Off,
            '1' => Agc::Fast,
            '2' => Agc::Mid,
            '3' => Agc::Slow,
            '4' => Agc::Auto,
            '5' => Agc::AutoFast,
            '6' => Agc::AutoMid,
            '7' => Agc::AutoSlow,
            _ => return None,
        })
    }

    pub fn label(&self) -> &'static str {
        match self {
            Agc::Off => "OFF",
            Agc::Fast => "FAST",
            Agc::Mid => "MID",
            Agc::Slow => "SLOW",
            Agc::Auto => "AUTO",
            Agc::AutoFast => "A-FAST",
            Agc::AutoMid => "A-MID",
            Agc::AutoSlow => "A-SLOW",
        }
    }

    pub const ALL: [Agc; 8] = [
        Agc::Off,
        Agc::Fast,
        Agc::Mid,
        Agc::Slow,
        Agc::Auto,
        Agc::AutoFast,
        Agc::AutoMid,
        Agc::AutoSlow,
    ];
}

/// Build a "set AGC" command (`GT0` + code).
pub fn set_agc(agc: Agc) -> String {
    format!("GT0{};", agc.code())
}

/// Build a "read AGC" command.
pub fn read_agc() -> &'static str {
    "GT0;"
}

/// Parse the AGC from a `GT` response (e.g. `GT02;`).
pub fn parse_agc(frame: &str) -> Option<Agc> {
    let parsed = split(frame)?;
    if parsed.command != "GT" {
        return None;
    }
    Agc::from_code(parsed.payload.chars().last()?)
}

/// Maximum RF gain value (`RG` 0-255).
pub const RF_GAIN_MAX: u8 = 255;
/// Maximum squelch value (`SQ` 0-100).
pub const SQUELCH_MAX: u8 = 100;

/// Build a "set RF gain" command (`RG0` + three digits, 0-255).
pub fn set_rf_gain(value: u8) -> String {
    format!("RG0{:03};", value)
}

/// Build a "read RF gain" command.
pub fn read_rf_gain() -> &'static str {
    "RG0;"
}

/// Parse the RF gain from an `RG` response (e.g. `RG0255;`).
pub fn parse_rf_gain(frame: &str) -> Option<u8> {
    parse_indexed_u8(frame, "RG", RF_GAIN_MAX)
}

/// Build a "set squelch" command (`SQ0` + three digits, 0-100).
pub fn set_squelch(value: u8) -> String {
    format!("SQ0{:03};", value.min(SQUELCH_MAX))
}

/// Build a "read squelch" command.
pub fn read_squelch() -> &'static str {
    "SQ0;"
}

/// Parse the squelch from an `SQ` response (e.g. `SQ0100;`).
pub fn parse_squelch(frame: &str) -> Option<u8> {
    parse_indexed_u8(frame, "SQ", SQUELCH_MAX)
}

/// Parse a `<CMD><index><3 digits>;` response, returning the trailing value.
fn parse_indexed_u8(frame: &str, command: &str, max: u8) -> Option<u8> {
    let parsed = split(frame)?;
    if parsed.command != command {
        return None;
    }
    let digits = parsed.payload.get(1..)?;
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let value: u32 = digits.parse().ok()?;
    Some(value.min(max as u32) as u8)
}

/// Parse a `<CMD><digit>;` on/off response (e.g. `NB1;`, `RT0;`).
fn parse_on_off(frame: &str, command: &str) -> Option<bool> {
    let parsed = split(frame)?;
    if parsed.command != command {
        return None;
    }
    match parsed.payload.chars().next()? {
        '0' => Some(false),
        '1' | '2' => Some(true),
        _ => None,
    }
}

/// Build a "read transceiver identification" command.
pub fn read_id() -> String {
    "ID;".to_string()
}

/// Build a "read composite status" command.
pub fn read_status() -> String {
    "IF;".to_string()
}

/// Build a "read S-meter" command.
pub fn read_smeter() -> String {
    "SM0;".to_string()
}

/// Build a "read spectrum scope settings" command.
pub fn read_scope() -> String {
    "SS00;".to_string()
}

/// Yaesu scope span codes (`SS` P2=5). Index = the P3 digit.
pub const SCOPE_SPANS_HZ: [f64; 10] = [
    1_000.0,
    2_000.0,
    5_000.0,
    10_000.0,
    20_000.0,
    50_000.0,
    100_000.0,
    200_000.0,
    500_000.0,
    1_000_000.0,
];

/// Read the MAIN scope span (`SS05;`).
pub fn read_scope_span() -> &'static str {
    "SS05;"
}

/// Read the MAIN scope mode (`SS06;`).
pub fn read_scope_mode() -> &'static str {
    "SS06;"
}

/// Set the MAIN scope span by code (0-9).
pub fn set_scope_span(index: u8) -> String {
    let digit = (index.min(9) + b'0') as char;
    format!("SS05{digit}0000;")
}

/// Set the MAIN scope mode by code character (`0`-`B`).
pub fn set_scope_mode(code: char) -> String {
    format!("SS06{}0000;", code)
}

/// Parse an `SS` answer into `(P1, P2, value)` where `value` is the 5-byte field.
///
/// Answer shape: `SS P1 P2 P3P4P5P6P7;` (e.g. `SS0590000;`).
pub fn parse_scope_field(frame: &str) -> Option<(char, char, &str)> {
    let parsed = split(frame)?;
    if parsed.command != "SS" || parsed.payload.len() < 7 {
        return None;
    }
    let p1 = parsed.payload.chars().next()?;
    let p2 = parsed.payload.chars().nth(1)?;
    Some((p1, p2, &parsed.payload[2..7]))
}

/// Parse the span index from an `SS` answer.
pub fn parse_scope_span_index(frame: &str) -> Option<usize> {
    let (_, p2, value) = parse_scope_field(frame)?;
    if p2 != '5' {
        return None;
    }
    value.chars().next()?.to_digit(10).map(|d| d as usize)
}

/// Parse the scope mode from an `SS` answer.
pub fn parse_scope_mode(frame: &str) -> Option<ScopeMode> {
    let (_, p2, value) = parse_scope_field(frame)?;
    if p2 != '6' {
        return None;
    }
    ScopeMode::from_code(value.chars().next()?)
}

/// What the scope is drawn over.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScopeDisplay {
    ThreeD,
    Waterfall,
}

/// How the scope tracks the receiver.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScopePlacement {
    /// Scope span is centered on the VFO and follows tuning.
    Center,
    /// A movable cursor sets the center.
    Cursor,
    /// Fixed span, independent of the VFO.
    Fix,
}

/// Scope span preselection size.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScopeSize {
    Large,
    Normal,
    Small,
}

/// Decoded `SS06` scope mode (FTDX10 / FTDX101 value table).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScopeMode {
    pub display: ScopeDisplay,
    pub placement: ScopePlacement,
    pub size: ScopeSize,
}

use ScopeDisplay::{ThreeD, Waterfall};
use ScopePlacement::{Center, Cursor, Fix};
use ScopeSize::{Large, Normal, Small};

impl ScopeMode {
    fn new(display: ScopeDisplay, placement: ScopePlacement, size: ScopeSize) -> Self {
        Self {
            display,
            placement,
            size,
        }
    }

    pub fn from_code(code: char) -> Option<Self> {
        Some(match code.to_ascii_uppercase() {
            '0' => Self::new(ThreeD, Center, Normal),
            '1' => Self::new(ThreeD, Cursor, Normal),
            '2' => Self::new(ThreeD, Fix, Normal),
            '3' => Self::new(Waterfall, Center, Large),
            '4' => Self::new(Waterfall, Center, Normal),
            '5' => Self::new(Waterfall, Center, Small),
            '6' => Self::new(Waterfall, Cursor, Large),
            '7' => Self::new(Waterfall, Cursor, Normal),
            '8' => Self::new(Waterfall, Cursor, Small),
            '9' => Self::new(Waterfall, Fix, Large),
            'A' => Self::new(Waterfall, Fix, Normal),
            'B' => Self::new(Waterfall, Fix, Small),
            _ => return None,
        })
    }

    pub fn code(&self) -> char {
        match (self.display, self.placement, self.size) {
            (ThreeD, Center, _) => '0',
            (ThreeD, Cursor, _) => '1',
            (ThreeD, Fix, _) => '2',
            (Waterfall, Center, Large) => '3',
            (Waterfall, Center, Normal) => '4',
            (Waterfall, Center, Small) => '5',
            (Waterfall, Cursor, Large) => '6',
            (Waterfall, Cursor, Normal) => '7',
            (Waterfall, Cursor, Small) => '8',
            (Waterfall, Fix, Large) => '9',
            (Waterfall, Fix, Normal) => 'A',
            (Waterfall, Fix, Small) => 'B',
        }
    }

    /// Same display type/size, but centered on the VFO.
    pub fn with_center(self) -> Self {
        Self {
            placement: Center,
            ..self
        }
    }

    pub fn is_center(&self) -> bool {
        self.placement == Center
    }
}

/// Build an "auto information" command. `on` enables unsolicited updates.
pub fn auto_info(on: bool) -> String {
    format!("AI{};", if on { 1 } else { 0 })
}

/// A parsed CAT frame: a two-letter command plus its payload (sans `;`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CatFrame<'a> {
    pub command: &'a str,
    pub payload: &'a str,
}

impl<'a> CatFrame<'a> {
    pub fn as_u64(&self) -> Option<u64> {
        self.payload.parse().ok()
    }
}

/// Split a semicolon-terminated CAT frame into command + payload.
pub fn split(frame: &str) -> Option<CatFrame<'_>> {
    let frame = frame.trim();
    let body = frame.strip_suffix(';')?;
    if body.len() < 2 {
        return None;
    }
    Some(CatFrame {
        command: &body[..2],
        payload: &body[2..],
    })
}

/// Parse the frequency from an `FA`/`FB` response.
pub fn parse_frequency(frame: &str) -> Option<u64> {
    let parsed = split(frame)?;
    if parsed.command != "FA" && parsed.command != "FB" {
        return None;
    }
    if parsed.payload.len() != 9 || !parsed.payload.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    parsed.payload.parse().ok()
}

/// Parse the 4-digit radio ID from an `ID` response.
pub fn parse_id(frame: &str) -> Option<u32> {
    let parsed = split(frame)?;
    if parsed.command != "ID" {
        return None;
    }
    u32::from_str_radix(parsed.payload, 16).ok()
}

/// Parse an S-meter reading from an `SM0` response (0-255 decimal).
pub fn parse_smeter(frame: &str) -> Option<u8> {
    let parsed = split(frame)?;
    if parsed.command != "SM" {
        return None;
    }
    let value: u32 = parsed.payload.get(1..)?.parse().ok()?;
    Some(value.min(255) as u8)
}

/// Parse a meter reading from an `RM` unsolicited response.
/// Returns `(meter_index, value)`.
pub fn parse_meter(frame: &str) -> Option<(u8, u32)> {
    let parsed = split(frame)?;
    if parsed.command != "RM" {
        return None;
    }
    let mut chars = parsed.payload.chars();
    let index = chars.next()?.to_digit(10)? as u8;
    let value: u32 = chars.as_str().parse().ok()?;
    Some((index, value))
}

/// A transceiver meter, identified by the `RM` P1 index (FTDX10 / Yaesu newcat).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MeterKind {
    /// S-meter (`SM0`), not an `RM` meter.
    S,
    /// Speech compressor meter (`RM3`).
    Comp,
    /// Automatic level control (`RM4`).
    Alc,
    /// RF power output (`RM5`).
    Power,
    /// Standing-wave ratio (`RM6`).
    Swr,
    /// Final amplifier drain current (`RM7`).
    Id,
    /// Final amplifier supply voltage (`RM8`).
    Vdd,
    /// Final amplifier temperature (`RM9`).
    Temp,
    Unknown(u8),
}

/// Piecewise-linear interpolation over a calibration table.
fn interp(points: &[(u8, f64)], raw: u8) -> f64 {
    let raw = raw as f64;
    let Some((first_x, first_y)) = points.first().copied() else {
        return raw;
    };
    if raw <= first_x as f64 {
        return first_y;
    }
    for pair in points.windows(2) {
        let (x0, y0) = pair[0];
        let (x1, y1) = pair[1];
        if raw <= x1 as f64 {
            return y0 + (y1 - y0) * (raw - x0 as f64) / (x1 as f64 - x0 as f64);
        }
    }
    points.last().map(|p| p.1).unwrap_or(raw)
}

/// S-meter raw → dB relative to S9 (Yaesu default, W6HN data).
const S_CAL: [(u8, f64); 11] = [
    (0, -54.0),
    (26, -42.0),
    (51, -30.0),
    (81, -18.0),
    (105, -9.0),
    (130, 0.0),
    (157, 12.0),
    (186, 25.0),
    (203, 35.0),
    (237, 50.0),
    (255, 60.0),
];
const SWR_CAL: [(u8, f64); 5] = [(12, 1.0), (39, 1.35), (65, 1.5), (89, 2.0), (242, 5.0)];
const COMP_CAL: [(u8, f64); 9] = [
    (0, 0.0),
    (40, 2.5),
    (60, 5.0),
    (85, 7.5),
    (135, 10.0),
    (150, 12.5),
    (175, 15.0),
    (195, 17.5),
    (220, 20.0),
];
const ID_CAL: [(u8, f64); 3] = [(0, 0.0), (100, 10.0), (255, 25.5)];
const VDD_CAL: [(u8, f64); 3] = [(0, 0.0), (196, 13.8), (255, 17.95)];
/// FTDX10 RF power output (flrig). Max 100 W.
const POWER_CAL: [(u8, f64); 6] = [
    (0, 0.0),
    (35, 5.0),
    (94, 25.0),
    (147, 50.0),
    (176, 75.0),
    (205, 100.0),
];

impl MeterKind {
    /// Map an `RM` P1 index to a meter.
    pub fn from_rm_index(index: u8) -> Self {
        match index {
            3 => MeterKind::Comp,
            4 => MeterKind::Alc,
            5 => MeterKind::Power,
            6 => MeterKind::Swr,
            7 => MeterKind::Id,
            8 => MeterKind::Vdd,
            9 => MeterKind::Temp,
            other => MeterKind::Unknown(other),
        }
    }

    pub fn label(&self) -> String {
        match self {
            MeterKind::S => "S".into(),
            MeterKind::Comp => "COMP".into(),
            MeterKind::Alc => "ALC".into(),
            MeterKind::Power => "PO".into(),
            MeterKind::Swr => "SWR".into(),
            MeterKind::Id => "ID".into(),
            MeterKind::Vdd => "VDD".into(),
            MeterKind::Temp => "TEMP".into(),
            MeterKind::Unknown(i) => format!("RM{i}"),
        }
    }

    /// Convert a raw 0-255 reading to a display string in real units.
    pub fn format(&self, raw: u8) -> String {
        match self {
            MeterKind::S => {
                let db = interp(&S_CAL, raw);
                if db <= 0.0 {
                    let s_units = (9.0 + db / 6.0).round().clamp(0.0, 9.0);
                    format!("S{s_units:.0}")
                } else {
                    format!("S9+{db:.0}dB")
                }
            }
            MeterKind::Comp => format!("{:.1} dB", interp(&COMP_CAL, raw)),
            MeterKind::Alc => format!("{:.0} %", raw as f32 / 2.55),
            MeterKind::Power => format!("{:.0} W", interp(&POWER_CAL, raw)),
            MeterKind::Swr => format!("{:.1}", interp(&SWR_CAL, raw)),
            MeterKind::Id => format!("{:.1} A", interp(&ID_CAL, raw)),
            MeterKind::Vdd => format!("{:.1} V", interp(&VDD_CAL, raw)),
            MeterKind::Temp | MeterKind::Unknown(_) => format!("{raw}"),
        }
    }

    /// Normalized 0.0..=1.0 bar fraction for the reading.
    pub fn fraction(&self, raw: u8) -> f32 {
        let value = match self {
            MeterKind::S => raw as f64 / 255.0,
            MeterKind::Comp => interp(&COMP_CAL, raw) / 20.0,
            MeterKind::Alc => raw as f64 / 255.0,
            MeterKind::Power => interp(&POWER_CAL, raw) / 100.0,
            MeterKind::Swr => (interp(&SWR_CAL, raw) - 1.0) / 4.0,
            MeterKind::Id => interp(&ID_CAL, raw) / 25.5,
            MeterKind::Vdd => interp(&VDD_CAL, raw) / 18.0,
            MeterKind::Temp | MeterKind::Unknown(_) => raw as f64 / 255.0,
        };
        value.clamp(0.0, 1.0) as f32
    }
}

/// Parse an `RM` frame into a meter kind and raw reading.
pub fn parse_meter_kind(frame: &str) -> Option<(MeterKind, u8)> {
    let (index, value) = parse_meter(frame)?;
    Some((MeterKind::from_rm_index(index), value.min(255) as u8))
}

/// Radio model identification from the `ID;` command.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RadioModel {
    Ftdx10,
    Ftdx101D,
    Ftdx101Mp,
    Ft710,
    Other(u32),
}

impl RadioModel {
    pub fn from_id(id: u32) -> Self {
        match id {
            0x0670 => RadioModel::Ftdx10,
            0x0681 => RadioModel::Ftdx101D,
            0x0682 => RadioModel::Ftdx101Mp,
            0x0800 => RadioModel::Ft710,
            other => RadioModel::Other(other),
        }
    }

    pub fn name(&self) -> String {
        match self {
            RadioModel::Ftdx10 => "FTDX10".into(),
            RadioModel::Ftdx101D => "FTDX101D".into(),
            RadioModel::Ftdx101Mp => "FTDX101MP".into(),
            RadioModel::Ft710 => "FT-710".into(),
            RadioModel::Other(id) => format!("Unknown (0x{id:04X})"),
        }
    }
}

/// Common Yaesu operating modes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Lsb,
    Usb,
    CwU,
    Fm,
    Am,
    RttyL,
    CwL,
    DataL,
    RttyU,
    DataU,
}

impl Mode {
    /// Yaesu `MD` mode code digit.
    pub fn code(&self) -> char {
        match self {
            Mode::Lsb => '1',
            Mode::Usb => '2',
            Mode::CwU => '3',
            Mode::Fm => '4',
            Mode::Am => '5',
            Mode::RttyL => '6',
            Mode::CwL => '7',
            Mode::DataL => '8',
            Mode::RttyU => '9',
            Mode::DataU => 'B',
        }
    }

    pub fn from_code(c: char) -> Option<Self> {
        Some(match c.to_ascii_uppercase() {
            '1' => Mode::Lsb,
            '2' => Mode::Usb,
            '3' => Mode::CwU,
            '4' => Mode::Fm,
            '5' => Mode::Am,
            '6' => Mode::RttyL,
            '7' => Mode::CwL,
            '8' => Mode::DataL,
            '9' => Mode::RttyU,
            'B' => Mode::DataU,
            _ => return None,
        })
    }

    pub fn label(&self) -> &'static str {
        match self {
            Mode::Lsb => "LSB",
            Mode::Usb => "USB",
            Mode::CwU => "CW-U",
            Mode::Fm => "FM",
            Mode::Am => "AM",
            Mode::RttyL => "RTTY-L",
            Mode::CwL => "CW-L",
            Mode::DataL => "DATA-L",
            Mode::RttyU => "RTTY-U",
            Mode::DataU => "DATA-U",
        }
    }

    pub const ALL: [Mode; 10] = [
        Mode::Lsb,
        Mode::Usb,
        Mode::CwU,
        Mode::Fm,
        Mode::Am,
        Mode::RttyL,
        Mode::CwL,
        Mode::DataL,
        Mode::RttyU,
        Mode::DataU,
    ];
}

/// Build a "set mode (MAIN VFO)" command.
pub fn set_mode(mode: Mode) -> String {
    format!("MD0{};", mode.code())
}

/// Build a "set mode" command for a specific VFO (`sub = true` -> VFO-B).
pub fn set_mode_vfo(sub: bool, mode: Mode) -> String {
    format!("MD{}{};", sub as u8, mode.code())
}

/// Build a "read mode" command for a specific VFO (`sub = true` -> VFO-B).
pub fn read_mode_vfo(sub: bool) -> String {
    format!("MD{};", sub as u8)
}

/// Build a "transmit / receive" (PTT) command.
///
/// `TX1;` keys the transmitter (mic audio path); `TX0;` returns to receive.
pub fn transmit(on: bool) -> String {
    format!("TX{};", if on { 1 } else { 0 })
}

/// Minimum RF power setting (watts) on the FTDX10 / FTDX101 / FT-710.
pub const POWER_MIN_W: u16 = 5;
/// Maximum RF power setting (watts) on the FTDX10 (100 W).
pub const POWER_MAX_W: u16 = 100;

/// Build a "set RF power output" command: `PC` + three digits of watts.
pub fn set_power(watts: u16) -> String {
    format!("PC{:03};", watts.clamp(POWER_MIN_W, POWER_MAX_W))
}

/// Build a "read RF power output" command.
pub fn read_power() -> &'static str {
    "PC;"
}

/// Parse the watts from a `PC` response (e.g. `PC050;` -> 50).
pub fn parse_power(frame: &str) -> Option<u16> {
    let parsed = split(frame)?;
    if parsed.command != "PC" {
        return None;
    }
    if parsed.payload.is_empty() || !parsed.payload.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    parsed.payload.parse().ok()
}

/// Microphone gain (`MG`) is expressed as a percentage, 0-100.
pub const MIC_GAIN_MAX: u8 = 100;

/// Build a "set microphone gain" command: `MG` + three digits of percent.
pub fn set_mic_gain(percent: u8) -> String {
    format!("MG{:03};", percent.min(MIC_GAIN_MAX))
}

/// Build a "read microphone gain" command.
pub fn read_mic_gain() -> &'static str {
    "MG;"
}

/// Parse the percent from an `MG` response (e.g. `MG050;` -> 50).
pub fn parse_mic_gain(frame: &str) -> Option<u8> {
    let parsed = split(frame)?;
    if parsed.command != "MG" {
        return None;
    }
    if parsed.payload.is_empty() || !parsed.payload.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    parsed.payload.parse().ok()
}

/// Build an "antenna tuner on/off" command: `AC001;` / `AC000;`.
pub fn set_atu(on: bool) -> String {
    format!("AC{};", if on { "001" } else { "000" })
}

/// Build a "start ATU tuning cycle" command (`AC002;`).
pub fn start_tune() -> &'static str {
    "AC002;"
}

/// Build a "read antenna tuner state" command.
pub fn read_atu() -> &'static str {
    "AC;"
}

/// Parse the ATU state from an `AC` response (e.g. `AC001;` -> on).
///
/// The third digit is the tuner state: `0` = off, `1` = on, `2` = tuning.
pub fn parse_atu(frame: &str) -> Option<bool> {
    let parsed = split(frame)?;
    if parsed.command != "AC" {
        return None;
    }
    let digit = parsed
        .payload
        .chars()
        .nth(2)
        .or_else(|| parsed.payload.chars().last())?;
    match digit {
        '0' => Some(false),
        '1' | '2' => Some(true),
        _ => None,
    }
}

/// Parse the transmit state from a `TX` response (`TX0;` / `TX1;`).
pub fn parse_transmit(frame: &str) -> Option<bool> {
    let parsed = split(frame)?;
    if parsed.command != "TX" {
        return None;
    }
    match parsed.payload.chars().next()? {
        '0' => Some(false),
        '1' | '2' => Some(true),
        _ => None,
    }
}

/// Build a "radio power switch" command (`PS`): `PS1;` on, `PS0;` off.
///
/// The SCU-LAN10 stays reachable while the transceiver is powered down, so
/// this can be used to bring the radio up (or put it into standby) remotely.
pub fn set_radio_power(on: bool) -> &'static str {
    if on {
        "PS1;"
    } else {
        "PS0;"
    }
}

/// Build a "read radio power state" command (`PS;`).
pub fn read_radio_power() -> &'static str {
    "PS;"
}

/// Parse the radio power state from a `PS` response (`PS0;` = off, `PS1;` = on).
pub fn parse_radio_power(frame: &str) -> Option<bool> {
    let parsed = split(frame)?;
    if parsed.command != "PS" {
        return None;
    }
    match parsed.payload.chars().next()? {
        '0' => Some(false),
        '1' => Some(true),
        _ => None,
    }
}

/// Parse the mode from an `MD0` response.
pub fn parse_mode(frame: &str) -> Option<Mode> {
    let parsed = split(frame)?;
    if parsed.command != "MD" {
        return None;
    }
    // Response is `MD P1 P2;`: P1 is the VFO, P2 is the mode digit.
    let mode = parsed
        .payload
        .chars()
        .nth(1)
        .or_else(|| parsed.payload.chars().next())?;
    Mode::from_code(mode)
}

/// Format a frequency in Hz for display, e.g. `7.007.000`.
pub fn format_hz(hz: u64) -> String {
    let s = format!("{:09}", hz.min(999_999_999));
    let high = s[0..3].trim_start_matches('0');
    let high = if high.is_empty() { "0" } else { high };
    format!("{}.{}.{}", high, &s[3..6], &s[6..9])
}

impl fmt::Display for Mode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_commands() {
        assert_eq!(read_frequency(), "FA;");
        assert_eq!(set_frequency(7_007_000), "FA007007000;");
        assert_eq!(set_mode(Mode::Usb), "MD02;");
        assert_eq!(read_id(), "ID;");
    }

    #[test]
    fn build_and_parse_transmit() {
        assert_eq!(transmit(true), "TX1;");
        assert_eq!(transmit(false), "TX0;");
        assert_eq!(parse_transmit("TX1;"), Some(true));
        assert_eq!(parse_transmit("TX2;"), Some(true));
        assert_eq!(parse_transmit("TX0;"), Some(false));
        assert_eq!(parse_transmit("FA;"), None);
    }

    #[test]
    fn build_and_parse_power() {
        assert_eq!(read_power(), "PC;");
        assert_eq!(set_power(50), "PC050;");
        assert_eq!(set_power(5), "PC005;");
        assert_eq!(set_power(100), "PC100;");
        assert_eq!(set_power(0), "PC005;");
        assert_eq!(set_power(250), "PC100;");
        assert_eq!(parse_power("PC050;"), Some(50));
        assert_eq!(parse_power("PC100;"), Some(100));
        assert_eq!(parse_power("PC;"), None);
        assert_eq!(parse_power("FA;"), None);
    }

    #[test]
    fn build_and_parse_radio_power() {
        assert_eq!(read_radio_power(), "PS;");
        assert_eq!(set_radio_power(true), "PS1;");
        assert_eq!(set_radio_power(false), "PS0;");
        assert_eq!(parse_radio_power("PS1;"), Some(true));
        assert_eq!(parse_radio_power("PS0;"), Some(false));
        assert_eq!(parse_radio_power("PS;"), None);
        assert_eq!(parse_radio_power("PC050;"), None);
    }

    #[test]
    fn build_and_parse_mic_gain() {
        assert_eq!(read_mic_gain(), "MG;");
        assert_eq!(set_mic_gain(50), "MG050;");
        assert_eq!(set_mic_gain(0), "MG000;");
        assert_eq!(set_mic_gain(100), "MG100;");
        assert_eq!(set_mic_gain(250), "MG100;");
        assert_eq!(parse_mic_gain("MG050;"), Some(50));
        assert_eq!(parse_mic_gain("MG100;"), Some(100));
        assert_eq!(parse_mic_gain("MG;"), None);
        assert_eq!(parse_mic_gain("FA;"), None);
    }

    #[test]
    fn build_and_parse_atu() {
        assert_eq!(read_atu(), "AC;");
        assert_eq!(set_atu(true), "AC001;");
        assert_eq!(set_atu(false), "AC000;");
        assert_eq!(start_tune(), "AC002;");
        assert_eq!(parse_atu("AC001;"), Some(true));
        assert_eq!(parse_atu("AC000;"), Some(false));
        assert_eq!(parse_atu("AC002;"), Some(true));
        assert_eq!(parse_atu("FA;"), None);
    }

    #[test]
    fn parse_frequency_response() {
        assert_eq!(parse_frequency("FA007007000;"), Some(7_007_000));
        assert_eq!(parse_frequency("FB014250000;"), Some(14_250_000));
        assert_eq!(parse_frequency("MD02;"), None);
        assert_eq!(parse_frequency("FA12345;"), None);
    }

    #[test]
    fn parse_radio_id() {
        assert_eq!(parse_id("ID0670;"), Some(0x0670));
        assert_eq!(RadioModel::from_id(0x0670), RadioModel::Ftdx10);
        assert_eq!(RadioModel::from_id(0x0670).name(), "FTDX10");
    }

    #[test]
    fn parse_smeter_and_meter() {
        assert_eq!(parse_smeter("SM0123;"), Some(123));
        assert_eq!(parse_meter("RM1123;"), Some((1, 123)));
        assert_eq!(parse_meter("SM0123;"), None);
    }

    #[test]
    fn split_frame() {
        let f = split("IF000007007000+000000...;").unwrap();
        assert_eq!(f.command, "IF");
        assert!(!f.payload.is_empty());
    }

    #[test]
    fn mode_round_trip() {
        for m in Mode::ALL {
            assert_eq!(Mode::from_code(m.code()), Some(m));
        }
    }

    #[test]
    fn parse_mode_response() {
        // `MD0;` read on the MAIN VFO returning USB.
        assert_eq!(parse_mode("MD02;"), Some(Mode::Usb));
        assert_eq!(parse_mode("MD01;"), Some(Mode::Lsb));
        assert_eq!(parse_mode("MD0B;"), Some(Mode::DataU));
        assert_eq!(set_mode_vfo(false, Mode::Usb), "MD02;");
        assert_eq!(set_mode_vfo(true, Mode::Usb), "MD12;");
        assert_eq!(read_mode_vfo(true), "MD1;");
        assert_eq!(parse_mode("MD12;"), Some(Mode::Usb));
    }

    #[test]
    fn hz_formatting() {
        assert_eq!(format_hz(7_007_000), "7.007.000");
        assert_eq!(format_hz(14_250_000), "14.250.000");
    }

    #[test]
    fn parse_scope_span_answer() {
        assert_eq!(parse_scope_span_index("SS0590000;"), Some(9));
        assert_eq!(parse_scope_span_index("SS0570000;"), Some(7));
        assert_eq!(SCOPE_SPANS_HZ[9], 1_000_000.0);
        assert_eq!(SCOPE_SPANS_HZ[7], 200_000.0);
        assert_eq!(parse_scope_span_index("SS0620000;"), None);
    }

    #[test]
    fn parse_scope_mode_answer() {
        // Code '6' = W/F CURSOR (L); centering preserves W/F + Large -> '3'.
        let mode = parse_scope_mode("SS0660000;").unwrap();
        assert_eq!(mode.placement, ScopePlacement::Cursor);
        assert_eq!(mode.display, ScopeDisplay::Waterfall);
        assert_eq!(mode.with_center().code(), '3');
        assert_eq!(parse_scope_mode("SS0020000;"), None);
    }

    #[test]
    fn scope_mode_round_trips_and_centers() {
        for code in '0'..='9' {
            let mode = ScopeMode::from_code(code).unwrap();
            assert_eq!(mode.code(), code);
        }
        assert_eq!(ScopeMode::from_code('A').unwrap().code(), 'A');
        assert_eq!(ScopeMode::from_code('B').unwrap().code(), 'B');

        let fixed = ScopeMode::from_code('A').unwrap();
        assert!(!fixed.is_center());
        assert_eq!(fixed.with_center().code(), '4');

        let threed_fix = ScopeMode::from_code('2').unwrap();
        assert_eq!(threed_fix.with_center().code(), '0');
    }

    #[test]
    fn meter_kind_mapping() {
        assert_eq!(MeterKind::from_rm_index(3), MeterKind::Comp);
        assert_eq!(MeterKind::from_rm_index(4), MeterKind::Alc);
        assert_eq!(MeterKind::from_rm_index(5), MeterKind::Power);
        assert_eq!(MeterKind::from_rm_index(6), MeterKind::Swr);
        assert_eq!(MeterKind::from_rm_index(7), MeterKind::Id);
        assert_eq!(MeterKind::from_rm_index(8), MeterKind::Vdd);
        assert_eq!(MeterKind::from_rm_index(9), MeterKind::Temp);
        assert_eq!(MeterKind::from_rm_index(1), MeterKind::Unknown(1));
    }

    #[test]
    fn smeter_true_scale() {
        assert_eq!(MeterKind::S.format(0), "S0");
        assert_eq!(MeterKind::S.format(51), "S4");
        assert_eq!(MeterKind::S.format(81), "S6");
        assert_eq!(MeterKind::S.format(130), "S9");
        assert_eq!(MeterKind::S.format(157), "S9+12dB");
        assert_eq!(MeterKind::S.format(255), "S9+60dB");
    }

    #[test]
    fn other_meter_true_scales() {
        assert_eq!(MeterKind::Power.format(205), "100 W");
        assert!(MeterKind::Power.format(147).starts_with("50"));
        assert_eq!(MeterKind::Swr.format(12), "1.0");
        assert_eq!(MeterKind::Swr.format(242), "5.0");
        assert_eq!(MeterKind::Vdd.format(196), "13.8 V");
        assert_eq!(MeterKind::Id.format(100), "10.0 A");
        assert_eq!(MeterKind::Alc.format(255), "100 %");
    }

    #[test]
    fn meter_fractions_are_bounded() {
        for kind in [
            MeterKind::S,
            MeterKind::Comp,
            MeterKind::Alc,
            MeterKind::Power,
            MeterKind::Swr,
            MeterKind::Id,
            MeterKind::Vdd,
        ] {
            for raw in [0u8, 12, 100, 205, 255] {
                let f = kind.fraction(raw);
                assert!((0.0..=1.0).contains(&f), "{kind:?} raw {raw} -> {f}");
            }
        }
    }

    #[test]
    fn parse_meter_kind_frame() {
        assert_eq!(parse_meter_kind("RM4128;"), Some((MeterKind::Alc, 128)));
        assert_eq!(parse_meter_kind("RM6007;"), Some((MeterKind::Swr, 7)));
        assert_eq!(parse_meter_kind("SM0130;"), None);
    }

    #[test]
    fn build_scope_commands() {
        assert_eq!(read_scope_span(), "SS05;");
        assert_eq!(read_scope_mode(), "SS06;");
        assert_eq!(set_scope_span(7), "SS0570000;");
        assert_eq!(set_scope_mode('4'), "SS0640000;");
    }

    #[test]
    fn build_and_parse_vfo_b() {
        assert_eq!(set_frequency_b(14_250_000), "FB014250000;");
        assert_eq!(parse_frequency("FB014250000;"), Some(14_250_000));
    }

    #[test]
    fn build_and_parse_vfo_select() {
        assert_eq!(select_rx_vfo(false), "FR0;");
        assert_eq!(select_rx_vfo(true), "FR1;");
        assert_eq!(parse_rx_vfo("FR1;"), Some(true));
        assert_eq!(parse_rx_vfo("FR0;"), Some(false));
        assert_eq!(parse_rx_vfo("FT1;"), None);

        assert_eq!(select_tx_vfo(false), "FT0;");
        assert_eq!(select_tx_vfo(true), "FT1;");
        assert_eq!(parse_tx_vfo("FT1;"), Some(true));
    }

    #[test]
    fn build_and_parse_split_and_swap() {
        assert_eq!(set_split(true), "ST1;");
        assert_eq!(set_split(false), "ST0;");
        assert_eq!(read_split(), "ST;");
        assert_eq!(parse_split("ST1;"), Some(true));
        assert_eq!(parse_split("ST0;"), Some(false));
        assert_eq!(swap_vfo(), "SV;");
        assert_eq!(copy_a_to_b(), "AB;");
        assert_eq!(copy_b_to_a(), "BA;");
    }

    #[test]
    fn build_and_parse_rit_xit() {
        assert_eq!(set_rit(true), "RT1;");
        assert_eq!(set_rit(false), "RT0;");
        assert_eq!(parse_rit("RT1;"), Some(true));
        assert_eq!(parse_rit("RT0;"), Some(false));
        assert_eq!(set_xit(true), "XT1;");
        assert_eq!(set_xit(false), "XT0;");
        assert_eq!(parse_xit("XT1;"), Some(true));
        assert_eq!(parse_xit("XT0;"), Some(false));
    }

    #[test]
    fn build_and_parse_clarifier() {
        assert_eq!(set_clarifier(false, 100), "RC0+0010;");
        assert_eq!(set_clarifier(true, -250), "RC1-0025;");
        assert_eq!(set_clarifier(false, 0), "RC0+0000;");
        // Quantize to the 10 Hz grid and clamp to the field width.
        assert_eq!(set_clarifier(false, 104), "RC0+0010;");
        assert_eq!(set_clarifier(false, 20_000_000), "RC0+9999;");

        assert_eq!(parse_clarifier("RC0+0010;"), Some((false, 100)));
        assert_eq!(parse_clarifier("RC1-0025;"), Some((true, -250)));
        assert_eq!(parse_clarifier("RC0+0000;"), Some((false, 0)));
        assert_eq!(parse_clarifier("FA;"), None);
    }

    #[test]
    fn build_and_parse_dsp_toggles() {
        assert_eq!(set_noise_blanker(true), "NB1;");
        assert_eq!(parse_noise_blanker("NB0;"), Some(false));
        assert_eq!(parse_noise_blanker("NB1;"), Some(true));
        assert_eq!(set_noise_reduction(true), "NR1;");
        assert_eq!(parse_noise_reduction("NR1;"), Some(true));
        assert_eq!(set_auto_notch(false), "BC0;");
        assert_eq!(parse_auto_notch("BC1;"), Some(true));
        assert_eq!(set_narrow(true), "NA1;");
        assert_eq!(parse_narrow("NA0;"), Some(false));
        assert_eq!(parse_narrow("NB1;"), None);
    }

    #[test]
    fn build_and_parse_agc() {
        assert_eq!(set_agc(Agc::Mid), "GT02;");
        assert_eq!(read_agc(), "GT0;");
        assert_eq!(parse_agc("GT02;"), Some(Agc::Mid));
        assert_eq!(parse_agc("GT03;"), Some(Agc::Slow));
        assert_eq!(parse_agc("GT04;"), Some(Agc::Auto));
        assert_eq!(parse_agc("GT09;"), None);
        for agc in Agc::ALL {
            assert_eq!(Agc::from_code(agc.code()), Some(agc));
        }
    }

    #[test]
    fn build_and_parse_rf_gain_and_squelch() {
        assert_eq!(set_rf_gain(128), "RG0128;");
        assert_eq!(set_rf_gain(255), "RG0255;");
        assert_eq!(set_rf_gain(250), "RG0250;");
        assert_eq!(parse_rf_gain("RG0128;"), Some(128));
        assert_eq!(parse_rf_gain("RG0255;"), Some(255));

        assert_eq!(set_squelch(50), "SQ0050;");
        assert_eq!(set_squelch(200), "SQ0100;");
        assert_eq!(parse_squelch("SQ0050;"), Some(50));
        assert_eq!(parse_squelch("RG0128;"), None);
    }
}
