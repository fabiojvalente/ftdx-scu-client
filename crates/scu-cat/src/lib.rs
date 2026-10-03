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

/// Build a "select VFO" command (`VS`). `sub = true` selects VFO-B.
///
/// The FTDX10 uses `VS` ("VFO SELECT") for the operating VFO; the `FR`
/// command found on the FTDX101 is not documented for this radio.
pub fn select_vfo(sub: bool) -> &'static str {
    if sub {
        "VS1;"
    } else {
        "VS0;"
    }
}

/// Build a "read selected VFO" command.
pub fn read_vfo() -> &'static str {
    "VS;"
}

/// Parse the selected VFO from a `VS` response (`true` = VFO-B).
pub fn parse_vfo(frame: &str) -> Option<bool> {
    parse_on_off(frame, "VS")
}

/// Build a "select transmit VFO" command (`FT`). `sub = true` selects VFO-B.
///
/// `FT0;`/`FT1;` select MAIN/SUB directly, the form the radio answers with and
/// the form the reference client sends on the FTDX10. Selecting the Sub VFO also
/// lights split on this radio, so callers must pair it with `ST0;` when simplex
/// is wanted.
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
///
/// The radio answers `FT0;` (MAIN/A) / `FT1;` (SUB/B), but the set form is
/// `FT2;`/`FT3;`, so accept those too: `2` is MAIN/A (`false`).
pub fn parse_tx_vfo(frame: &str) -> Option<bool> {
    let parsed = split(frame)?;
    if parsed.command != "FT" {
        return None;
    }
    match parsed.payload.chars().next()? {
        '0' | '2' => Some(false),
        '1' | '3' => Some(true),
        _ => None,
    }
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

/// Build a "band select" command (`BS` P1 P2), using the Yaesu band codes.
pub fn set_band(code: u8) -> String {
    format!("BS{:02};", code.min(99))
}

/// An amateur band for quick band select.
///
/// `calling_hz` is where `b` / `B` land; `lo_hz` / `hi_hz` bound the band so an
/// arbitrary frequency can be classified. The ranges span the common IARU
/// allocations rather than one region's exact plan. The FTDX10 covers
/// 160 m - 6 m; 4 m is included for FTDX101 parity and may be rejected by the
/// radio.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Band {
    pub name: &'static str,
    pub calling_hz: u64,
    pub lo_hz: u64,
    pub hi_hz: u64,
}

/// Bands offered for quick band select, in ascending frequency order.
pub const HF_BANDS: [Band; 12] = [
    Band {
        name: "160m",
        calling_hz: 1_840_000,
        lo_hz: 1_800_000,
        hi_hz: 2_000_000,
    },
    Band {
        name: "80m",
        calling_hz: 3_700_000,
        lo_hz: 3_500_000,
        hi_hz: 4_000_000,
    },
    Band {
        name: "60m",
        calling_hz: 5_357_000,
        lo_hz: 5_250_000,
        hi_hz: 5_450_000,
    },
    Band {
        name: "40m",
        calling_hz: 7_100_000,
        lo_hz: 7_000_000,
        hi_hz: 7_300_000,
    },
    Band {
        name: "30m",
        calling_hz: 10_136_000,
        lo_hz: 10_100_000,
        hi_hz: 10_150_000,
    },
    Band {
        name: "20m",
        calling_hz: 14_074_000,
        lo_hz: 14_000_000,
        hi_hz: 14_350_000,
    },
    Band {
        name: "17m",
        calling_hz: 18_110_000,
        lo_hz: 18_068_000,
        hi_hz: 18_168_000,
    },
    Band {
        name: "15m",
        calling_hz: 21_074_000,
        lo_hz: 21_000_000,
        hi_hz: 21_450_000,
    },
    Band {
        name: "12m",
        calling_hz: 24_915_000,
        lo_hz: 24_890_000,
        hi_hz: 24_990_000,
    },
    Band {
        name: "10m",
        calling_hz: 28_074_000,
        lo_hz: 28_000_000,
        hi_hz: 29_700_000,
    },
    Band {
        name: "6m",
        calling_hz: 50_313_000,
        lo_hz: 50_000_000,
        hi_hz: 54_000_000,
    },
    Band {
        name: "4m",
        calling_hz: 70_100_000,
        lo_hz: 70_000_000,
        hi_hz: 70_500_000,
    },
];

/// The band containing `hz`, if any.
pub fn band_for_hz(hz: u64) -> Option<&'static Band> {
    HF_BANDS
        .iter()
        .find(|band| hz >= band.lo_hz && hz <= band.hi_hz)
}

/// Index into [`HF_BANDS`] of the band whose calling frequency is nearest to
/// `hz`, or `None` when `hz` is zero.
pub fn nearest_band_index(hz: u64) -> Option<usize> {
    if hz == 0 {
        return None;
    }
    HF_BANDS
        .iter()
        .enumerate()
        .min_by_key(|(_, band)| hz.abs_diff(band.calling_hz))
        .map(|(index, _)| index)
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

/// Clarifier offset granularity on the FTDX10 (10 Hz).
pub const CLARIFIER_STEP_HZ: i32 = 10;
/// Maximum clarifier offset magnitude (`CF` P5-P8 field, Hz).
pub const CLARIFIER_MAX_HZ: i32 = 9990;

/// Clamp an arbitrary offset to the 10 Hz grid the radio accepts.
pub fn clamp_clarifier_hz(hz: i32) -> i32 {
    let steps = (hz as f64 / CLARIFIER_STEP_HZ as f64).round() as i32;
    steps.clamp(-999, 999) * CLARIFIER_STEP_HZ
}

/// Build a "set clarifier offset" command (`CF` P3=1) for `sub`'s VFO.
///
/// The FTDX10 has a single clarifier offset shared by RIT and XIT; use
/// [`set_rit`] / [`set_xit`] to enable it for receive and/or transmit.
pub fn set_clarifier_offset(sub: bool, hz: i32) -> String {
    let hz = clamp_clarifier_hz(hz);
    let sign = if hz < 0 { '-' } else { '+' };
    let digits = hz.abs().min(CLARIFIER_MAX_HZ);
    format!("CF{}01{}{:04};", sub as u8, sign, digits)
}

/// Build a "read clarifier offset" command (`CF` P3=1) for `sub`'s VFO.
pub fn read_clarifier_offset(sub: bool) -> String {
    format!("CF{}01;", sub as u8)
}

/// Parse a `CF?01{sign}dddd;` clarifier-offset response into Hz.
pub fn parse_clarifier_offset(frame: &str) -> Option<i32> {
    let parsed = split(frame)?;
    if parsed.command != "CF" || parsed.payload.len() < 8 {
        return None;
    }
    if parsed.payload.as_bytes().get(2) != Some(&b'1') {
        return None;
    }
    let mut chars = parsed.payload[3..].chars();
    let sign: i32 = match chars.next()? {
        '+' => 1,
        '-' => -1,
        _ => return None,
    };
    let digits: i32 = chars.as_str().get(..4)?.parse().ok()?;
    Some(sign * digits)
}

/// Build a "clear the clarifier" command (`RC;`).
pub fn clear_clarifier() -> &'static str {
    "RC;"
}

/// Build a "noise blanker on/off" command (`NB` P1=0, P2).
pub fn set_noise_blanker(on: bool) -> String {
    format!("NB0{};", on as u8)
}

/// Build a "read noise blanker state" command.
pub fn read_noise_blanker() -> &'static str {
    "NB0;"
}

/// Parse the noise blanker state from an `NB0P2;` response.
pub fn parse_noise_blanker(frame: &str) -> Option<bool> {
    parse_indexed_on_off(frame, "NB")
}

/// Build a "noise reduction on/off" command (`NR` P1=0, P2).
pub fn set_noise_reduction(on: bool) -> String {
    format!("NR0{};", on as u8)
}

/// Build a "read noise reduction state" command.
pub fn read_noise_reduction() -> &'static str {
    "NR0;"
}

/// Parse the noise reduction state from an `NR0P2;` response.
pub fn parse_noise_reduction(frame: &str) -> Option<bool> {
    parse_indexed_on_off(frame, "NR")
}

/// Maximum noise blanker level (`NL`, 1-20).
pub const NOISE_BLANKER_LEVEL_MAX: u8 = 20;

/// Maximum noise reduction (DNR) level (`RL`, 1-15).
pub const NOISE_REDUCTION_LEVEL_MAX: u8 = 15;

/// Build a "set noise blanker level" command (`NL` + VFO + three digits).
pub fn set_noise_blanker_level(sub: bool, level: u8) -> String {
    let level = level.clamp(1, NOISE_BLANKER_LEVEL_MAX);
    format!("NL{}{level:03};", sub as u8)
}

/// Build a "read noise blanker level" command (`NL0;` / `NL1;`).
pub fn read_noise_blanker_level(sub: bool) -> String {
    format!("NL{};", sub as u8)
}

/// Parse the noise blanker level from an `NL0NNN;` response.
pub fn parse_noise_blanker_level(frame: &str) -> Option<u8> {
    parse_indexed_u8(frame, "NL", NOISE_BLANKER_LEVEL_MAX)
}

/// Build a "set noise reduction (DNR) level" command (`RL` + VFO + two digits).
pub fn set_noise_reduction_level(sub: bool, level: u8) -> String {
    let level = level.clamp(1, NOISE_REDUCTION_LEVEL_MAX);
    format!("RL{}{level:02};", sub as u8)
}

/// Build a "read noise reduction level" command (`RL0;` / `RL1;`).
pub fn read_noise_reduction_level(sub: bool) -> String {
    format!("RL{};", sub as u8)
}

/// Parse the noise reduction level from an `RL0NN;` response.
///
/// The `RL` answer carries the DNR algorithm selector (1-15) on the FTDX10 /
/// FTDX101 / FT-710, not an on/off flag.
pub fn parse_noise_reduction_level(frame: &str) -> Option<u8> {
    parse_indexed_u8(frame, "RL", NOISE_REDUCTION_LEVEL_MAX)
}

/// Build an "auto notch on/off" command (`BC` P1=0, P2).
pub fn set_auto_notch(on: bool) -> String {
    format!("BC0{};", on as u8)
}

/// Build a "read auto notch state" command.
pub fn read_auto_notch() -> &'static str {
    "BC0;"
}

/// Parse the auto notch state from a `BC0P2;` response.
pub fn parse_auto_notch(frame: &str) -> Option<bool> {
    parse_indexed_on_off(frame, "BC")
}

/// Build a "narrow filter on/off" command (`NA` P1=0, P2).
pub fn set_narrow(on: bool) -> String {
    format!("NA0{};", on as u8)
}

/// Build a "read narrow filter state" command (`NA0;`, MAIN).
pub fn read_narrow() -> &'static str {
    "NA0;"
}

/// Parse the narrow filter state from an `NA0P2;` response.
pub fn parse_narrow(frame: &str) -> Option<bool> {
    parse_indexed_on_off(frame, "NA")
}

/// Roofing filter bandwidth selection (`RF`).
///
/// The FTDX10 offers 12 kHz, 3 kHz, 500 Hz and the optional 300 Hz roofing
/// filters. The `RF` *set* command reports them with P2 codes `1`/`2`/`4`/`5`,
/// while the radio answers with P3 codes `6`/`7`/`9`/`A` (the slots `3` and `8`
/// are reserved).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RoofingFilter {
    Khz12,
    Khz3,
    Hz500,
    Hz300,
}

impl RoofingFilter {
    /// The `RF` P2 digit carrying the filter in the set command.
    pub fn set_code(self) -> char {
        match self {
            RoofingFilter::Khz12 => '1',
            RoofingFilter::Khz3 => '2',
            RoofingFilter::Hz500 => '4',
            RoofingFilter::Hz300 => '5',
        }
    }

    /// The `RF` P3 digit the radio reports back in its answer.
    pub fn answer_code(self) -> char {
        match self {
            RoofingFilter::Khz12 => '6',
            RoofingFilter::Khz3 => '7',
            RoofingFilter::Hz500 => '9',
            RoofingFilter::Hz300 => 'A',
        }
    }

    /// Decode either the set (`1`-`5`) or answer (`6`-`A`) digit.
    pub fn from_code(code: char) -> Option<Self> {
        Some(match code.to_ascii_uppercase() {
            '1' | '6' => RoofingFilter::Khz12,
            '2' | '7' => RoofingFilter::Khz3,
            '4' | '9' => RoofingFilter::Hz500,
            '5' | 'A' => RoofingFilter::Hz300,
            _ => return None,
        })
    }

    pub fn label(self) -> &'static str {
        match self {
            RoofingFilter::Khz12 => "12 kHz",
            RoofingFilter::Khz3 => "3 kHz",
            RoofingFilter::Hz500 => "500 Hz",
            RoofingFilter::Hz300 => "300 Hz",
        }
    }

    pub const ALL: [RoofingFilter; 4] = [
        RoofingFilter::Khz12,
        RoofingFilter::Khz3,
        RoofingFilter::Hz500,
        RoofingFilter::Hz300,
    ];
}

/// Build a "set roofing filter" command (`RF` P1=0 fixed + P2).
pub fn set_roofing_filter(filter: RoofingFilter) -> String {
    format!("RF0{};", filter.set_code())
}

/// Build a "read roofing filter" command (`RF0;`, MAIN).
pub fn read_roofing_filter() -> &'static str {
    "RF0;"
}

/// Parse the roofing filter from an `RF0P3;` response.
pub fn parse_roofing_filter(frame: &str) -> Option<RoofingFilter> {
    let parsed = split(frame)?;
    if parsed.command != "RF" {
        return None;
    }
    RoofingFilter::from_code(parsed.payload.chars().nth(1)?)
}

// ---- IF width (`SH`) and IF shift (`IS`) -----------------------------------

/// Which `SH` bandwidth table a mode uses.
///
/// `SH` codes are shared across modes but map to different bandwidths per
/// group. AM, FM and the DATA-FM variants have no IF width.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IfWidthGroup {
    /// LSB / USB: the wide SSB table.
    Ssb,
    /// CW / RTTY / PSK / DATA: the narrow table.
    Cw,
}

/// Classify a mode into the `SH` bandwidth table it uses.
pub fn if_width_group(mode: Mode) -> Option<IfWidthGroup> {
    Some(match mode {
        Mode::Lsb | Mode::Usb => IfWidthGroup::Ssb,
        Mode::CwU
        | Mode::CwL
        | Mode::RttyL
        | Mode::RttyU
        | Mode::Psk
        | Mode::DataL
        | Mode::DataU => IfWidthGroup::Cw,
        Mode::Am | Mode::AmN | Mode::Fm | Mode::FmN | Mode::DataFm | Mode::DataFmN => return None,
    })
}

/// FTDX10 SSB IF-width table: index = `SH` code, value = bandwidth in Hz.
/// Index 0 is the radio's mode-dependent default.
pub const IF_WIDTH_SSB_HZ: [u16; 24] = [
    0, 300, 400, 600, 850, 1100, 1200, 1500, 1650, 1800, 1950, 2100, 2250, 2400, 2450, 2500, 2600,
    2700, 2800, 2900, 3000, 3200, 3500, 4000,
];

/// FTDX10 CW / RTTY / PSK / DATA IF-width table: index = `SH` code.
pub const IF_WIDTH_CW_HZ: [u16; 22] = [
    0, 50, 100, 150, 200, 250, 300, 350, 400, 450, 500, 600, 800, 1200, 1400, 1700, 2000, 2400,
    3000, 3200, 3500, 4000,
];

/// FT-710 `SH` codes, which are non-contiguous (only these codes are exposed).
const FT710_IF_WIDTH_SSB: &[(u8, u16)] = &[
    (0, 0),
    (1, 300),
    (3, 850),
    (5, 1100),
    (7, 1500),
    (9, 1800),
    (12, 2250),
    (16, 2600),
    (19, 2900),
    (20, 3200),
    (21, 3500),
    (22, 4000),
];
const FT710_IF_WIDTH_CW: &[(u8, u16)] = &[
    (0, 0),
    (1, 50),
    (3, 150),
    (5, 250),
    (7, 350),
    (9, 450),
    (12, 800),
    (16, 2000),
    (19, 3200),
    (20, 3500),
    (21, 4000),
];

/// `(code, bandwidth_hz)` pairs valid for `mode` on `model`, ascending by code.
/// Returns `None` when the mode has no IF width (AM / FM).
///
/// FTDX101D / FTDX101MP share the FTDX10 tables; their firmware differs by at
/// most one SSB step, which is close enough for key-cycle purposes.
pub fn if_width_options(model: RadioModel, mode: Mode) -> Option<Vec<(u8, u16)>> {
    let group = if_width_group(mode)?;
    let options = match model {
        RadioModel::Ft710 => match group {
            IfWidthGroup::Ssb => FT710_IF_WIDTH_SSB,
            IfWidthGroup::Cw => FT710_IF_WIDTH_CW,
        },
        _ => {
            let table: &[u16] = match group {
                IfWidthGroup::Ssb => &IF_WIDTH_SSB_HZ,
                IfWidthGroup::Cw => &IF_WIDTH_CW_HZ,
            };
            return Some(
                table
                    .iter()
                    .enumerate()
                    .map(|(code, &hz)| (code as u8, hz))
                    .collect(),
            );
        }
    };
    Some(options.to_vec())
}

/// Bandwidth in Hz used when the `SH` code is `0` (the mode default). The SSB
/// default is 3 kHz with the 12 kHz roofing filter; the CW/RTTY/PSK default is
/// 500 Hz.
pub fn default_if_width_hz(mode: Mode) -> Option<u16> {
    Some(match if_width_group(mode)? {
        IfWidthGroup::Ssb => 3000,
        IfWidthGroup::Cw => 500,
    })
}

/// Resolve an `SH` code to a bandwidth in Hz, mapping code `0` to the mode
/// default. `None` for modes without an IF width (AM / FM).
pub fn if_width_hz(model: RadioModel, mode: Mode, code: u8) -> Option<u16> {
    let options = if_width_options(model, mode)?;
    let hz = options
        .iter()
        .find(|(c, _)| *c == code)
        .map(|(_, hz)| *hz)
        .unwrap_or(0);
    (hz > 0).then_some(hz).or_else(|| default_if_width_hz(mode))
}

/// Bandwidth in Hz used for full-carrier modes, which have no `SH` filter.
fn full_carrier_width_hz(mode: Mode) -> Option<u16> {
    Some(match mode {
        Mode::Am | Mode::AmN => 6000,
        Mode::Fm | Mode::FmN | Mode::DataFm | Mode::DataFmN => 5000,
        _ => return None,
    })
}

/// Nominal passband / IF centre by mode, in Hz relative to the suppressed
/// carrier. This is the radio's "carrier point" (`SCOPE CTR = CARRIER`): about
/// +1.5 kHz in USB, -1.5 kHz in LSB, the sidetone pitch in CW. Full-carrier
/// modes (AM / FM) are centred on the carrier itself.
pub fn if_passband_center_hz(mode: Mode) -> Option<i32> {
    Some(match mode {
        Mode::Usb | Mode::DataU | Mode::Psk | Mode::RttyU => 1_500,
        Mode::Lsb | Mode::DataL | Mode::RttyL => -1_500,
        Mode::CwU => 700,
        Mode::CwL => -700,
        Mode::Am | Mode::AmN | Mode::Fm | Mode::FmN | Mode::DataFm | Mode::DataFmN => 0,
    })
}

/// Upper-sideband audio edges `(low_hz, high_hz)` for an SSB `SH` filter of
/// `width` Hz. Narrow filters (< 1 kHz) sit symmetrically about the 1500 Hz IF
/// centre; wider ones keep a ~100 Hz low edge and take the width off the high
/// side, so a 4 kHz filter spans ~100..4100 Hz instead of straddling the
/// carrier. Measured on an FTdx101MP, which shares the FTDX10's filter design.
fn ssb_passband_edges_hz(width: i32) -> (i32, i32) {
    if width < 1000 {
        let half = width / 2;
        (1500 - half, 1500 + half)
    } else {
        let lo = 100 + ((3000 - width).max(0) as f32 * 0.3) as i32;
        (lo, lo + width)
    }
}

/// Audio passband edges `(low_hz, high_hz)` relative to the carrier, derived
/// from the mode, the `SH` width code and the `IS` shift.
///
/// USB / LSB are **anchored to the carrier** (see [`ssb_passband_edges_hz`]).
/// Other modes keep the nominal centre of [`if_passband_center_hz`], displaced
/// by the IF shift. Full-carrier modes (AM / FM), which expose no IF width, use
/// a fixed nominal bandwidth.
pub fn if_passband_hz(
    model: RadioModel,
    mode: Mode,
    width_code: u8,
    shift_hz: i32,
) -> Option<(i32, i32)> {
    let width =
        if_width_hz(model, mode, width_code).or_else(|| full_carrier_width_hz(mode))? as i32;
    let shift = clamp_if_shift_hz(shift_hz);

    if matches!(mode, Mode::Usb | Mode::Lsb) {
        let (lo, hi) = ssb_passband_edges_hz(width);
        let (lo, hi) = (lo + shift, hi + shift);
        return Some(if matches!(mode, Mode::Lsb) {
            (-hi, -lo)
        } else {
            (lo, hi)
        });
    }

    let center = if_passband_center_hz(mode)? + shift;
    let half = width / 2;
    Some((center - half, center + half))
}

/// Build a "set IF width" command (`SH` P1=0/1 + three-digit code).
pub fn set_if_width(sub: bool, code: u8) -> String {
    format!("SH{}{:03};", sub as u8, code)
}

/// Build a "read IF width" command for `sub`'s VFO.
pub fn read_if_width(sub: bool) -> String {
    format!("SH{};", sub as u8)
}

/// Parse the `SH` code from an `SH0xxx;` response.
pub fn parse_if_width(frame: &str) -> Option<u8> {
    let parsed = split(frame)?;
    if parsed.command != "SH" || parsed.payload.len() < 4 {
        return None;
    }
    parsed.payload.get(1..)?.parse().ok()
}

/// IF shift limits and grid on the FTDX10.
pub const IF_SHIFT_MIN_HZ: i32 = -1000;
pub const IF_SHIFT_MAX_HZ: i32 = 1000;
pub const IF_SHIFT_STEP_HZ: i32 = 20;

/// Clamp an arbitrary IF shift to the radio's 20 Hz grid and +-1000 Hz range.
pub fn clamp_if_shift_hz(hz: i32) -> i32 {
    let steps = (hz as f64 / IF_SHIFT_STEP_HZ as f64).round() as i32;
    (steps * IF_SHIFT_STEP_HZ).clamp(IF_SHIFT_MIN_HZ, IF_SHIFT_MAX_HZ)
}

/// Build a "set IF shift" command (`IS` P1=0/1 + `0` + sign + four digits).
pub fn set_if_shift(sub: bool, hz: i32) -> String {
    let hz = clamp_if_shift_hz(hz);
    let sign = if hz < 0 { '-' } else { '+' };
    format!("IS{}0{}{:04};", sub as u8, sign, hz.abs())
}

/// Build a "read IF shift" command for `sub`'s VFO.
pub fn read_if_shift(sub: bool) -> String {
    format!("IS{};", sub as u8)
}

/// Parse the IF shift in Hz from an `IS` response (`IS00[+-]dddd;`).
pub fn parse_if_shift(frame: &str) -> Option<i32> {
    let parsed = split(frame)?;
    if parsed.command != "IS" {
        return None;
    }
    let sign_at = parsed.payload.find(['+', '-'])?;
    let sign = if parsed.payload.as_bytes()[sign_at] == b'-' {
        -1
    } else {
        1
    };
    let digits: i32 = parsed.payload.get(sign_at + 1..sign_at + 5)?.parse().ok()?;
    Some(clamp_if_shift_hz(sign * digits))
}

/// AGC time constant (`GT` P2).
///
/// The FTDX10 `GT` *set* command accepts only `0`-`4`; the radio resolves an
/// `AUTO` request to one of its auto sub-modes, which it reports back as
/// `4`/`5`/`6` (AUTO-FAST / AUTO-MID / AUTO-SLOW). Those are collapsed to
/// [`Agc::Auto`] here, matching the settable front-panel choices.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Agc {
    Off,
    Fast,
    Mid,
    Slow,
    Auto,
}

impl Agc {
    pub fn code(&self) -> char {
        match self {
            Agc::Off => '0',
            Agc::Fast => '1',
            Agc::Mid => '2',
            Agc::Slow => '3',
            Agc::Auto => '4',
        }
    }

    pub fn from_code(code: char) -> Option<Self> {
        Some(match code {
            '0' => Agc::Off,
            '1' => Agc::Fast,
            '2' => Agc::Mid,
            '3' => Agc::Slow,
            '4' | '5' | '6' => Agc::Auto,
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
        }
    }

    pub const ALL: [Agc; 5] = [Agc::Off, Agc::Fast, Agc::Mid, Agc::Slow, Agc::Auto];
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

/// Receiver front-end gain stage selected by the `PA` command.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Preamp {
    /// IPO: front-end preamplifier bypassed for a better strong-signal
    /// intercept point.
    Ipo,
    /// AMP1: 10 dB preamplifier.
    Amp1,
    /// AMP2: 20 dB preamplifier.
    Amp2,
}

impl Preamp {
    pub fn code(self) -> char {
        match self {
            Preamp::Ipo => '0',
            Preamp::Amp1 => '1',
            Preamp::Amp2 => '2',
        }
    }

    pub fn from_code(code: char) -> Option<Self> {
        Some(match code {
            '0' => Preamp::Ipo,
            '1' => Preamp::Amp1,
            '2' => Preamp::Amp2,
            _ => return None,
        })
    }

    pub fn label(self) -> &'static str {
        match self {
            Preamp::Ipo => "IPO",
            Preamp::Amp1 => "AMP1",
            Preamp::Amp2 => "AMP2",
        }
    }

    pub const ALL: [Preamp; 3] = [Preamp::Ipo, Preamp::Amp1, Preamp::Amp2];
}

/// Build a "set preamp / IPO" command (`PA` + VFO + code).
pub fn set_preamp(sub: bool, preamp: Preamp) -> String {
    format!("PA{}{};", sub as u8, preamp.code())
}

/// Build a "read preamp / IPO" command (`PA0;` / `PA1;`).
pub fn read_preamp(sub: bool) -> String {
    format!("PA{};", sub as u8)
}

/// Parse the preamp / IPO from a `PA0P2;` response.
pub fn parse_preamp(frame: &str) -> Option<Preamp> {
    let parsed = split(frame)?;
    if parsed.command != "PA" {
        return None;
    }
    Preamp::from_code(parsed.payload.chars().nth(1)?)
}

/// Attenuator steps in dB, indexed by the `RA` code (0 = off).
pub const ATTENUATOR_STEPS_DB: [u8; 4] = [0, 6, 12, 18];

/// Build a "set attenuator" command (`RA` + VFO + step code, 0-3).
pub fn set_attenuator(sub: bool, code: u8) -> String {
    let code = code.min((ATTENUATOR_STEPS_DB.len() - 1) as u8);
    format!("RA{}{};", sub as u8, code)
}

/// Build a "read attenuator" command (`RA0;` / `RA1;`).
pub fn read_attenuator(sub: bool) -> String {
    format!("RA{};", sub as u8)
}

/// Parse the attenuator step code from an `RA0P2;` response.
pub fn parse_attenuator(frame: &str) -> Option<u8> {
    let parsed = split(frame)?;
    if parsed.command != "RA" {
        return None;
    }
    let code: u32 = parsed.payload.get(1..)?.parse().ok()?;
    Some(code.min((ATTENUATOR_STEPS_DB.len() - 1) as u32) as u8)
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

/// Parse a `<CMD><index><digit>;` on/off response (e.g. `NB01;`, `BC00;`).
fn parse_indexed_on_off(frame: &str, command: &str) -> Option<bool> {
    let parsed = split(frame)?;
    if parsed.command != command {
        return None;
    }
    match parsed.payload.chars().nth(1)? {
        '0' => Some(false),
        '1' => Some(true),
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

/// Number of characters in the FTDX10 / FT-991 `IF;` response, including the
/// leading `IF` and the trailing `;` (FT-450 and friends answer 27).
pub const IF_RESPONSE_LEN: usize = 28;

/// The subset of the Yaesu `IF;` composite-status response this client decodes.
///
/// The response is a fixed-width record whose notable field offsets (relative
/// to the start of the `IF`) are, for the 28-character FTDX10 form:
///
/// | Offset | Field |
/// |---|---|
/// | 2  | operating frequency (9 ASCII digits, Hz) |
/// | 14 | clarifier offset (5 chars: sign + 4 digits) |
/// | 22 | VFO/memory indicator (`0` = VFO, non-zero = memory) |
///
/// These offsets match the reference Hamlib `newcat` decoder and flrig's
/// FTDX10/FT-710 front ends. The clarifier field is reported in the radio's
/// native unit (Hz on the FTDX10 per its `CF` command); a couple of older
/// models express it in 10 Hz steps, so treat it as advisory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IfStatus {
    pub frequency_hz: u64,
    pub clarifier_hz: i32,
    /// `true` when the radio is showing a memory channel rather than a VFO.
    pub memory: bool,
    /// The complete response text, including the trailing `;`.
    pub raw: String,
}

/// Parse a Yaesu `IF;` composite-status response.
///
/// Returns `None` when the frame is not an `IF` answer or its frequency field
/// is missing/malformed. Fields whose offsets fall outside a shorter (older
/// radio) response degrade to defaults rather than failing the whole parse.
pub fn parse_if(frame: &str) -> Option<IfStatus> {
    let frame = frame.trim();
    let body = frame.strip_prefix("IF")?.strip_suffix(';')?;
    if body.len() < 11 {
        return None;
    }
    let frequency = body.get(0..9)?;
    if !frequency.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let frequency_hz = frequency.parse().ok()?;
    let clarifier_hz = body.get(12..17).and_then(parse_signed_offset).unwrap_or(0);
    let memory = body.as_bytes().get(20).is_some_and(|&b| b != b'0');
    Some(IfStatus {
        frequency_hz,
        clarifier_hz,
        memory,
        raw: frame.to_string(),
    })
}

/// Parse a `±dddd` clarifier field (sign followed by up to four digits).
fn parse_signed_offset(field: &str) -> Option<i32> {
    let mut chars = field.chars();
    let sign = match chars.next()? {
        '+' => 1,
        '-' => -1,
        _ => return None,
    };
    let digits = chars.as_str();
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    Some(sign * digits.parse::<i32>().ok()?)
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

/// What the scope screen is centred on (`SCOPE CTR`, menu `EX040202`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScopeCenter {
    /// Centred on the centre of the IF filter (offset from the carrier by the
    /// passband centre).
    Filter,
    /// Centred on the signal carrier point, i.e. the VFO (the default).
    Carrier,
}

impl ScopeCenter {
    pub fn label(self) -> &'static str {
        match self {
            ScopeCenter::Filter => "FILTER",
            ScopeCenter::Carrier => "CARRIER",
        }
    }
}

/// Build a "read scope centre" command (menu `DISPLAY SETTING -> SCOPE ->
/// SCOPE CTR`, `EX040202`).
pub fn read_scope_center() -> &'static str {
    "EX040202;"
}

/// Parse an `EX040202v;` answer: `0` = FILTER, `1` = CARRIER.
pub fn parse_scope_center(frame: &str) -> Option<ScopeCenter> {
    let parsed = split(frame)?;
    if parsed.command != "EX" || parsed.payload.len() < 7 {
        return None;
    }
    if &parsed.payload[0..6] != "040202" {
        return None;
    }
    match parsed.payload.as_bytes()[6] {
        b'0' => Some(ScopeCenter::Filter),
        b'1' => Some(ScopeCenter::Carrier),
        _ => None,
    }
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
///
/// The `ID` answer is a four-digit decimal number (e.g. `ID0761;` for the
/// FTDX10), not hexadecimal.
pub fn parse_id(frame: &str) -> Option<u32> {
    let parsed = split(frame)?;
    if parsed.command != "ID" {
        return None;
    }
    parsed.payload.parse().ok()
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

/// Parse a meter reading from an `RM` response.
/// Returns `(meter_index, value)`.
///
/// The FTdx10/FTdx101 answer `RMP1LLLRRR;` — the meter index followed by *two*
/// 3-digit slots (left/right). Only the left slot is meaningful for a direct
/// read, so the first three digits are taken; older rigs answer a single
/// `RMP1LLL;`, which parses identically. Concatenating both slots (the bug this
/// fixes) overflowed the 0-255 range and pinned every meter to full scale.
pub fn parse_meter(frame: &str) -> Option<(u8, u32)> {
    let parsed = split(frame)?;
    if parsed.command != "RM" {
        return None;
    }
    let mut chars = parsed.payload.chars();
    let index = chars.next()?.to_digit(10)? as u8;
    let digits: String = chars.as_str().chars().take(3).collect();
    let value: u32 = digits.parse().ok()?;
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

/// S-meter raw → dB relative to S9. Measured on an FTdx10 and shipped as the
/// per-model default in Yaesu Web Control (`calibration.default.FTdx10.json`):
/// S0=0, S1=4, S3=30, S5=65, S7=95, S9=131, +20=171, +40=213, +60=255.
const S_CAL: [(u8, f64); 9] = [
    (0, -54.0),
    (4, -48.0),
    (30, -36.0),
    (65, -24.0),
    (95, -12.0),
    (131, 0.0),
    (171, 20.0),
    (213, 40.0),
    (255, 60.0),
];
/// SWR raw → ratio (Yaesu Web Control FTdx10 default).
const SWR_CAL: [(u8, f64); 6] = [
    (0, 1.0),
    (51, 1.5),
    (77, 2.0),
    (128, 3.0),
    (173, 5.0),
    (242, 9.9),
];
/// Speech-compression raw → dB (Yaesu Web Control FTdx10 default).
const COMP_CAL: [(u8, f64); 5] = [(0, 0.0), (56, 5.0), (102, 10.0), (140, 15.0), (204, 20.0)];
/// Final-amp drain current raw → amps (Yaesu Web Control FTdx10 default).
const ID_CAL: [(u8, f64); 6] = [
    (0, 0.0),
    (51, 5.0),
    (102, 10.0),
    (153, 15.0),
    (204, 20.0),
    (242, 25.0),
];
/// Final-amp supply voltage raw → volts. The 13.8 V radios (FTdx10/FTdx101D)
/// read 10–16 V; the FTdx101MP's 50 V final uses a different scale.
const VDD_CAL: [(u8, f64); 8] = [
    (0, 0.0),
    (170, 11.0),
    (182, 12.1),
    (194, 13.2),
    (206, 14.1),
    (218, 14.9),
    (235, 15.2),
    (255, 16.0),
];
/// PA temperature raw → °C (Yaesu Web Control `TPA`).
const TEMP_CAL: [(u8, f64); 7] = [
    (0, -6.0),
    (14, 0.0),
    (60, 20.0),
    (106, 40.0),
    (152, 60.0),
    (198, 80.0),
    (244, 100.0),
];
/// RF power output raw → watts (Yaesu Web Control FTdx10 default). The FTdx10
/// is a 100 W radio, so the table is capped there even though the upstream
/// table continues to the FTdx101MP's 200 W.
const POWER_CAL: [(u8, f64); 5] = [(0, 0.0), (30, 5.0), (76, 25.0), (112, 50.0), (157, 100.0)];

/// Full-scale values used to normalise each bar to 0.0..=1.0.
const POWER_FULL_W: f64 = 100.0;
const COMP_FULL_DB: f64 = 20.0;
const SWR_MIN: f64 = 1.0;
const SWR_FULL_SPAN: f64 = 4.0;
const ID_FULL_A: f64 = 25.0;
const VDD_FULL_V: f64 = 16.0;
const TEMP_FULL_C: f64 = 100.0;

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
            MeterKind::Comp => {
                format!("{:.1} dB", interp(&COMP_CAL, raw).clamp(0.0, COMP_FULL_DB))
            }
            MeterKind::Alc => format!("{:.0} %", raw as f32 / 2.55),
            MeterKind::Power => {
                format!("{:.0} W", interp(&POWER_CAL, raw).clamp(0.0, POWER_FULL_W))
            }
            MeterKind::Swr => format!("{:.1}", interp(&SWR_CAL, raw).clamp(SWR_MIN, 10.0)),
            MeterKind::Id => format!("{:.1} A", interp(&ID_CAL, raw).clamp(0.0, ID_FULL_A)),
            MeterKind::Vdd => format!("{:.1} V", interp(&VDD_CAL, raw).clamp(0.0, VDD_FULL_V)),
            MeterKind::Temp => {
                format!("{:.0} °C", interp(&TEMP_CAL, raw).clamp(0.0, TEMP_FULL_C))
            }
            MeterKind::Unknown(_) => format!("{raw}"),
        }
    }

    /// Normalized 0.0..=1.0 bar fraction for the reading.
    pub fn fraction(&self, raw: u8) -> f32 {
        let value = match self {
            MeterKind::S => raw as f64 / 255.0,
            MeterKind::Comp => interp(&COMP_CAL, raw) / COMP_FULL_DB,
            MeterKind::Alc => raw as f64 / 255.0,
            MeterKind::Power => interp(&POWER_CAL, raw) / POWER_FULL_W,
            MeterKind::Swr => (interp(&SWR_CAL, raw) - SWR_MIN) / SWR_FULL_SPAN,
            MeterKind::Id => interp(&ID_CAL, raw) / ID_FULL_A,
            MeterKind::Vdd => interp(&VDD_CAL, raw) / VDD_FULL_V,
            MeterKind::Temp => interp(&TEMP_CAL, raw) / TEMP_FULL_C,
            MeterKind::Unknown(_) => raw as f64 / 255.0,
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
            761 => RadioModel::Ftdx10,
            681 => RadioModel::Ftdx101D,
            682 => RadioModel::Ftdx101Mp,
            800 => RadioModel::Ft710,
            other => RadioModel::Other(other),
        }
    }

    /// The 4-digit decimal `ID;` value for this model.
    pub fn id(&self) -> u32 {
        match self {
            RadioModel::Ftdx10 => 761,
            RadioModel::Ftdx101D => 681,
            RadioModel::Ftdx101Mp => 682,
            RadioModel::Ft710 => 800,
            RadioModel::Other(id) => *id,
        }
    }

    pub fn name(&self) -> String {
        match self {
            RadioModel::Ftdx10 => "FTDX10".into(),
            RadioModel::Ftdx101D => "FTDX101D".into(),
            RadioModel::Ftdx101Mp => "FTDX101MP".into(),
            RadioModel::Ft710 => "FT-710".into(),
            RadioModel::Other(id) => format!("Unknown (ID{id:04})"),
        }
    }
}

/// FTDX10 `MD` P2 operating modes.
///
/// Codes are taken verbatim from the *FTDX10 CAT Operation Reference* `MD`
/// table: `1` LSB, `2` USB, `3` CW-U, `4` FM, `5` AM, `6` RTTY-L, `7` CW-L,
/// `8` DATA-L, `9` RTTY-U, `A` DATA-FM, `B` FM-N, `C` DATA-U, `D` AM-N,
/// `E` PSK, `F` DATA-FM-N.
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
    DataFm,
    FmN,
    DataU,
    AmN,
    Psk,
    DataFmN,
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
            Mode::DataFm => 'A',
            Mode::FmN => 'B',
            Mode::DataU => 'C',
            Mode::AmN => 'D',
            Mode::Psk => 'E',
            Mode::DataFmN => 'F',
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
            'A' => Mode::DataFm,
            'B' => Mode::FmN,
            'C' => Mode::DataU,
            'D' => Mode::AmN,
            'E' => Mode::Psk,
            'F' => Mode::DataFmN,
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
            Mode::DataFm => "DATA-FM",
            Mode::FmN => "FM-N",
            Mode::DataU => "DATA-U",
            Mode::AmN => "AM-N",
            Mode::Psk => "PSK",
            Mode::DataFmN => "DATA-FM-N",
        }
    }

    pub const ALL: [Mode; 15] = [
        Mode::Lsb,
        Mode::Usb,
        Mode::CwU,
        Mode::Fm,
        Mode::Am,
        Mode::RttyL,
        Mode::CwL,
        Mode::DataL,
        Mode::RttyU,
        Mode::DataFm,
        Mode::FmN,
        Mode::DataU,
        Mode::AmN,
        Mode::Psk,
        Mode::DataFmN,
    ];
}

/// Build a "set mode (MAIN VFO)" command.
pub fn set_mode(mode: Mode) -> String {
    format!("MD0{};", mode.code())
}

/// Build a `MD P1 P2;` mode-set command.
///
/// `sub` is the raw P1 digit (`0`/`1`). On dual-receiver radios it selects the
/// receiver (0 = MAIN/VFO-A, 1 = SUB/VFO-B). On the single-receiver FTDX10 it is
/// instead **relative to the operating VFO**: `0` addresses the active VFO and
/// `1` the inactive one (unlike `FA`/`FB`, which are fixed to VFO-A/VFO-B).
/// Callers target a physical VFO by translating first (see the app's
/// `md_vfo_sub`).
pub fn set_mode_vfo(sub: bool, mode: Mode) -> String {
    format!("MD{}{};", sub as u8, mode.code())
}

/// Build a `MD P1;` mode-read command. See [`set_mode_vfo`] for the meaning of
/// the P1 digit.
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

/// Parse the mode digit from an `MD P1 P2;` response.
///
/// The P1 digit is returned separately by the caller via `split`; on the
/// single-receiver FTDX10 it is relative to the operating VFO (see
/// [`set_mode_vfo`]).
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
        assert_eq!(parse_id("ID0761;"), Some(761));
        assert_eq!(parse_id("ID0681;"), Some(681));
        assert_eq!(RadioModel::from_id(761), RadioModel::Ftdx10);
        assert_eq!(RadioModel::from_id(761).name(), "FTDX10");
        assert_eq!(RadioModel::from_id(761).id(), 761);
        assert_eq!(RadioModel::from_id(9999).id(), 9999);
    }

    #[test]
    fn parse_smeter_and_meter() {
        assert_eq!(parse_smeter("SM0123;"), Some(123));
        assert_eq!(parse_meter("RM1123;"), Some((1, 123)));
        assert_eq!(parse_meter("SM0123;"), None);
        // FTdx10 answers two 3-digit slots; only the left one is the reading.
        assert_eq!(parse_meter("RM5072000;"), Some((5, 72)));
        assert_eq!(parse_meter("RM8206000;"), Some((8, 206)));
        assert_eq!(parse_meter_kind("RM8206000;"), Some((MeterKind::Vdd, 206)));
    }

    #[test]
    fn split_frame() {
        let f = split("IF000007007000+000000...;").unwrap();
        assert_eq!(f.command, "IF");
        assert!(!f.payload.is_empty());
    }

    #[test]
    fn parse_if_status() {
        // 28-char FTDX10 form: IF + freq(9) + 3 + clarifier(5) + 3 + mem + 4 + ;
        let status = parse_if("IF007007000000+00000000000;").unwrap();
        assert_eq!(status.frequency_hz, 7_007_000);
        assert_eq!(status.clarifier_hz, 0);
        assert!(!status.memory);
        assert_eq!(status.raw, "IF007007000000+00000000000;");

        // Offset 14 clarifier (negative) and offset 22 memory indicator set.
        let status = parse_if("IF007007000000-025000010000;").unwrap();
        assert_eq!(status.frequency_hz, 7_007_000);
        assert_eq!(status.clarifier_hz, -250);
        assert!(status.memory);

        // 14.250.000 with a positive clarifier.
        let status = parse_if("IF014250000000+01000000000;").unwrap();
        assert_eq!(status.frequency_hz, 14_250_000);
        assert_eq!(status.clarifier_hz, 100);
    }

    #[test]
    fn parse_if_rejects_other_frames() {
        assert_eq!(parse_if("FA007007000;"), None);
        assert_eq!(parse_if("IF;"), None);
        assert_eq!(parse_if("IF00ABC7000;"), None);
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
        assert_eq!(parse_mode("MD0B;"), Some(Mode::FmN));
        assert_eq!(parse_mode("MD0C;"), Some(Mode::DataU));
        assert_eq!(parse_mode("MD0A;"), Some(Mode::DataFm));
        assert_eq!(parse_mode("MD0E;"), Some(Mode::Psk));
        assert_eq!(set_mode_vfo(false, Mode::Usb), "MD02;");
        assert_eq!(set_mode_vfo(true, Mode::Usb), "MD12;");
        assert_eq!(set_mode_vfo(false, Mode::DataU), "MD0C;");
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
    fn parse_scope_center_answer() {
        assert_eq!(parse_scope_center("EX0402020;"), Some(ScopeCenter::Filter));
        assert_eq!(parse_scope_center("EX0402021;"), Some(ScopeCenter::Carrier));
        assert_eq!(parse_scope_center("EX040203;"), None);
        assert_eq!(parse_scope_center("EX0104051500;"), None);
        assert_eq!(read_scope_center(), "EX040202;");
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
        assert_eq!(MeterKind::S.format(4), "S1");
        assert_eq!(MeterKind::S.format(30), "S3");
        assert_eq!(MeterKind::S.format(65), "S5");
        assert_eq!(MeterKind::S.format(95), "S7");
        assert_eq!(MeterKind::S.format(131), "S9");
        assert_eq!(MeterKind::S.format(171), "S9+20dB");
        assert_eq!(MeterKind::S.format(213), "S9+40dB");
        assert_eq!(MeterKind::S.format(255), "S9+60dB");
    }

    #[test]
    fn other_meter_true_scales() {
        assert_eq!(MeterKind::Power.format(112), "50 W");
        assert_eq!(MeterKind::Power.format(157), "100 W");
        // Capped at the FTdx10's 100 W even though the raw table runs higher.
        assert_eq!(MeterKind::Power.format(205), "100 W");
        assert_eq!(MeterKind::Swr.format(0), "1.0");
        assert_eq!(MeterKind::Swr.format(242), "9.9");
        assert_eq!(MeterKind::Vdd.format(206), "14.1 V");
        assert_eq!(MeterKind::Id.format(51), "5.0 A");
        assert_eq!(MeterKind::Comp.format(204), "20.0 dB");
        assert_eq!(MeterKind::Temp.format(106), "40 °C");
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
        assert_eq!(select_vfo(false), "VS0;");
        assert_eq!(select_vfo(true), "VS1;");
        assert_eq!(read_vfo(), "VS;");
        assert_eq!(parse_vfo("VS1;"), Some(true));
        assert_eq!(parse_vfo("VS0;"), Some(false));
        assert_eq!(parse_vfo("FT1;"), None);

        assert_eq!(select_tx_vfo(false), "FT0;");
        assert_eq!(select_tx_vfo(true), "FT1;");
        assert_eq!(read_tx_vfo(), "FT;");
        assert_eq!(parse_tx_vfo("FT0;"), Some(false));
        assert_eq!(parse_tx_vfo("FT1;"), Some(true));
        assert_eq!(parse_tx_vfo("FT2;"), Some(false));
        assert_eq!(parse_tx_vfo("FT3;"), Some(true));
        assert_eq!(parse_tx_vfo("VS1;"), None);
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
        assert_eq!(set_clarifier_offset(false, 100), "CF001+0100;");
        assert_eq!(set_clarifier_offset(true, -250), "CF101-0250;");
        assert_eq!(set_clarifier_offset(false, 0), "CF001+0000;");
        // Quantize to the 10 Hz grid and clamp to the field width.
        assert_eq!(set_clarifier_offset(false, 104), "CF001+0100;");
        assert_eq!(set_clarifier_offset(false, 20_000_000), "CF001+9990;");

        assert_eq!(read_clarifier_offset(false), "CF001;");
        assert_eq!(read_clarifier_offset(true), "CF101;");
        assert_eq!(parse_clarifier_offset("CF001+0100;"), Some(100));
        assert_eq!(parse_clarifier_offset("CF101-0250;"), Some(-250));
        assert_eq!(parse_clarifier_offset("CF001+0000;"), Some(0));
        // CLAR on/off (P3=0) is not an offset response.
        assert_eq!(parse_clarifier_offset("CF0001000;"), None);
        assert_eq!(parse_clarifier_offset("FA;"), None);
        assert_eq!(clear_clarifier(), "RC;");
    }

    #[test]
    fn build_and_parse_dsp_toggles() {
        assert_eq!(set_noise_blanker(true), "NB01;");
        assert_eq!(read_noise_blanker(), "NB0;");
        assert_eq!(parse_noise_blanker("NB00;"), Some(false));
        assert_eq!(parse_noise_blanker("NB01;"), Some(true));
        assert_eq!(set_noise_reduction(true), "NR01;");
        assert_eq!(read_noise_reduction(), "NR0;");
        assert_eq!(parse_noise_reduction("NR01;"), Some(true));
        assert_eq!(set_auto_notch(false), "BC00;");
        assert_eq!(read_auto_notch(), "BC0;");
        assert_eq!(parse_auto_notch("BC01;"), Some(true));
        assert_eq!(set_narrow(true), "NA01;");
        assert_eq!(read_narrow(), "NA0;");
        assert_eq!(parse_narrow("NA00;"), Some(false));
        assert_eq!(parse_narrow("NB01;"), None);
    }

    #[test]
    fn build_and_parse_roofing_filter() {
        assert_eq!(read_roofing_filter(), "RF0;");
        assert_eq!(set_roofing_filter(RoofingFilter::Khz12), "RF01;");
        assert_eq!(set_roofing_filter(RoofingFilter::Khz3), "RF02;");
        assert_eq!(set_roofing_filter(RoofingFilter::Hz500), "RF04;");
        assert_eq!(set_roofing_filter(RoofingFilter::Hz300), "RF05;");
        // The radio answers with the 6-A code range.
        assert_eq!(parse_roofing_filter("RF06;"), Some(RoofingFilter::Khz12));
        assert_eq!(parse_roofing_filter("RF07;"), Some(RoofingFilter::Khz3));
        assert_eq!(parse_roofing_filter("RF09;"), Some(RoofingFilter::Hz500));
        assert_eq!(parse_roofing_filter("RF0A;"), Some(RoofingFilter::Hz300));
        // Echoed set codes decode too, reserved slots do not.
        assert_eq!(parse_roofing_filter("RF02;"), Some(RoofingFilter::Khz3));
        assert_eq!(parse_roofing_filter("RF03;"), None);
        assert_eq!(parse_roofing_filter("RF08;"), None);
        assert_eq!(parse_roofing_filter("RG0;"), None);
        for filter in RoofingFilter::ALL {
            assert_eq!(RoofingFilter::from_code(filter.answer_code()), Some(filter));
            assert_eq!(RoofingFilter::from_code(filter.set_code()), Some(filter));
        }
    }

    #[test]
    fn build_and_parse_dsp_levels() {
        assert_eq!(set_noise_blanker_level(false, 10), "NL0010;");
        assert_eq!(set_noise_blanker_level(true, 5), "NL1005;");
        assert_eq!(set_noise_blanker_level(false, 99), "NL0020;");
        assert_eq!(read_noise_blanker_level(false), "NL0;");
        assert_eq!(read_noise_blanker_level(true), "NL1;");
        assert_eq!(parse_noise_blanker_level("NL0010;"), Some(10));
        assert_eq!(parse_noise_blanker_level("NL1005;"), Some(5));
        assert_eq!(parse_noise_blanker_level("NR01;"), None);

        assert_eq!(set_noise_reduction_level(false, 1), "RL001;");
        assert_eq!(set_noise_reduction_level(true, 15), "RL115;");
        assert_eq!(set_noise_reduction_level(false, 99), "RL015;");
        assert_eq!(read_noise_reduction_level(false), "RL0;");
        assert_eq!(read_noise_reduction_level(true), "RL1;");
        assert_eq!(parse_noise_reduction_level("RL001;"), Some(1));
        assert_eq!(parse_noise_reduction_level("RL115;"), Some(15));
        assert_eq!(parse_noise_reduction_level("RM001;"), None);
    }

    #[test]
    fn build_and_parse_agc() {
        assert_eq!(set_agc(Agc::Mid), "GT02;");
        assert_eq!(read_agc(), "GT0;");
        assert_eq!(parse_agc("GT02;"), Some(Agc::Mid));
        assert_eq!(parse_agc("GT03;"), Some(Agc::Slow));
        assert_eq!(parse_agc("GT04;"), Some(Agc::Auto));
        // The radio reports its resolved auto sub-mode; collapse to AUTO.
        assert_eq!(parse_agc("GT05;"), Some(Agc::Auto));
        assert_eq!(parse_agc("GT06;"), Some(Agc::Auto));
        assert_eq!(parse_agc("GT09;"), None);
        for agc in Agc::ALL {
            assert_eq!(Agc::from_code(agc.code()), Some(agc));
        }
    }

    #[test]
    fn build_and_parse_preamp_and_attenuator() {
        assert_eq!(set_preamp(false, Preamp::Amp2), "PA02;");
        assert_eq!(set_preamp(true, Preamp::Ipo), "PA10;");
        assert_eq!(read_preamp(false), "PA0;");
        assert_eq!(read_preamp(true), "PA1;");
        assert_eq!(parse_preamp("PA02;"), Some(Preamp::Amp2));
        assert_eq!(parse_preamp("PA10;"), Some(Preamp::Ipo));
        assert_eq!(parse_preamp("GT02;"), None);
        for preamp in Preamp::ALL {
            assert_eq!(Preamp::from_code(preamp.code()), Some(preamp));
        }

        assert_eq!(set_attenuator(false, 2), "RA02;");
        assert_eq!(set_attenuator(true, 3), "RA13;");
        assert_eq!(set_attenuator(false, 99), "RA03;");
        assert_eq!(read_attenuator(false), "RA0;");
        assert_eq!(read_attenuator(true), "RA1;");
        assert_eq!(parse_attenuator("RA00;"), Some(0));
        assert_eq!(parse_attenuator("RA13;"), Some(3));
        assert_eq!(parse_attenuator("RG0128;"), None);
        assert_eq!(ATTENUATOR_STEPS_DB, [0, 6, 12, 18]);
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

    #[test]
    fn build_and_parse_if_width() {
        assert_eq!(set_if_width(false, 8), "SH0008;");
        assert_eq!(set_if_width(true, 12), "SH1012;");
        assert_eq!(read_if_width(false), "SH0;");
        assert_eq!(read_if_width(true), "SH1;");
        assert_eq!(parse_if_width("SH0008;"), Some(8));
        assert_eq!(parse_if_width("SH1021;"), Some(21));
        assert_eq!(parse_if_width("IS0;"), None);
    }

    #[test]
    fn if_width_tables_follow_mode() {
        assert_eq!(if_width_group(Mode::Usb), Some(IfWidthGroup::Ssb));
        assert_eq!(if_width_group(Mode::Lsb), Some(IfWidthGroup::Ssb));
        assert_eq!(if_width_group(Mode::CwU), Some(IfWidthGroup::Cw));
        assert_eq!(if_width_group(Mode::DataU), Some(IfWidthGroup::Cw));
        assert_eq!(if_width_group(Mode::Fm), None);
        assert_eq!(if_width_group(Mode::AmN), None);

        let ssb = if_width_options(RadioModel::Ftdx10, Mode::Usb).unwrap();
        assert_eq!(ssb[0], (0, 0));
        assert_eq!(ssb[8], (8, 1650));
        let cw = if_width_options(RadioModel::Ftdx10, Mode::CwU).unwrap();
        assert_eq!(cw[5], (5, 250));
        assert_eq!(if_width_options(RadioModel::Ftdx10, Mode::Fm), None);

        // FT-710 codes are non-contiguous.
        let ft710 = if_width_options(RadioModel::Ft710, Mode::Usb).unwrap();
        assert!(ft710.iter().all(|(code, _)| *code != 2));
        assert!(ft710.contains(&(20, 3200)));
    }

    #[test]
    fn build_and_parse_if_shift() {
        assert_eq!(set_if_shift(false, 600), "IS00+0600;");
        assert_eq!(set_if_shift(true, -240), "IS10-0240;");
        assert_eq!(set_if_shift(false, 0), "IS00+0000;");
        // 20 Hz grid and +-1000 Hz clamp.
        assert_eq!(set_if_shift(false, 104), "IS00+0100;");
        assert_eq!(set_if_shift(false, 20_000_000), "IS00+1000;");
        assert_eq!(read_if_shift(false), "IS0;");
        assert_eq!(read_if_shift(true), "IS1;");
        assert_eq!(parse_if_shift("IS00+0600;"), Some(600));
        assert_eq!(parse_if_shift("IS10-0240;"), Some(-240));
        assert_eq!(parse_if_shift("SH0008;"), None);
    }

    #[test]
    fn if_passband_tracks_mode_width_and_shift() {
        // USB, default width (code 0 -> 3000 Hz) anchors the low edge near
        // 100 Hz and grows upward, not a fixed centre.
        assert_eq!(
            if_passband_hz(RadioModel::Ftdx10, Mode::Usb, 0, 0),
            Some((100, 3100))
        );
        // Narrower width (code 3 = 600 Hz) shrinks symmetrically about 1500.
        assert_eq!(
            if_passband_hz(RadioModel::Ftdx10, Mode::Usb, 3, 0),
            Some((1200, 1800))
        );
        // A wide filter (code 23 = 4000 Hz) stays entirely above the carrier.
        assert_eq!(
            if_passband_hz(RadioModel::Ftdx10, Mode::Usb, 23, 0),
            Some((100, 4100))
        );
        // LSB mirrors to negative frequencies.
        assert_eq!(
            if_passband_hz(RadioModel::Ftdx10, Mode::Lsb, 0, 0),
            Some((-3100, -100))
        );
        assert_eq!(
            if_passband_hz(RadioModel::Ftdx10, Mode::Lsb, 23, 0),
            Some((-4100, -100))
        );
        // A positive shift moves the passband up, also in LSB.
        assert_eq!(
            if_passband_hz(RadioModel::Ftdx10, Mode::Lsb, 0, 500),
            Some((-3600, -600))
        );
        // CW sits around the 700 Hz sidetone pitch.
        assert_eq!(
            if_passband_hz(RadioModel::Ftdx10, Mode::CwU, 5, 0),
            Some((575, 825))
        );
        // Full-carrier modes straddle the carrier.
        assert_eq!(
            if_passband_hz(RadioModel::Ftdx10, Mode::Am, 0, 0),
            Some((-3000, 3000))
        );
        assert_eq!(
            if_passband_hz(RadioModel::Ftdx10, Mode::Fm, 0, 0),
            Some((-2500, 2500))
        );
    }

    #[test]
    fn nearest_band_picks_calling_frequency() {
        assert_eq!(nearest_band_index(0), None);
        assert_eq!(nearest_band_index(14_250_000), Some(5)); // 20m
        assert_eq!(nearest_band_index(7_005_000), Some(3)); // 40m
        assert_eq!(nearest_band_index(50_200_000), Some(10)); // 6m
        assert_eq!(HF_BANDS[5].calling_hz, 14_074_000);
    }

    #[test]
    fn band_classification_respects_edges() {
        assert_eq!(band_for_hz(14_074_000).map(|b| b.name), Some("20m"));
        assert_eq!(band_for_hz(7_000_000).map(|b| b.name), Some("40m"));
        assert_eq!(band_for_hz(1_800_000).map(|b| b.name), Some("160m"));
        // Between bands: no label.
        assert_eq!(band_for_hz(5_000_000), None);
        assert_eq!(band_for_hz(0), None);
    }
}
