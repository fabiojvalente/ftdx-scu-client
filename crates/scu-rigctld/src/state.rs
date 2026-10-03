//! Cached view of the radio, kept up to date from the CAT response stream.
//!
//! rigctld getters are served from this cache rather than by issuing a fresh
//! CAT query. That matters because clients such as WSJT-X poll `get_freq`
//! immediately after `set_freq`; answering from a value updated on the set
//! avoids racing the radio's own apply delay, and lets the GUI keep polling the
//! radio independently.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use scu_cat::{Agc, Mode, RadioModel};

/// How long a value set by a rigctld client is shielded from radio replies.
///
/// The app polls `FA`/`FB`/`VS`/`ST` every second, and a reply that left the
/// radio *before* a client's setter was applied can arrive *after* the cache was
/// updated. Applying it would roll the cache back to the old value, so the next
/// getter (or the mode-settle re-send) would act on stale data and leave RX and
/// TX on different frequencies.
const SET_GUARD: Duration = Duration::from_millis(500);

/// Timestamp of the last client-initiated change to one cached field.
#[derive(Debug, Clone, Copy, Default)]
struct Guard(Option<Instant>);

impl Guard {
    fn arm(&mut self) {
        self.0 = Some(Instant::now());
    }

    fn active(&self) -> bool {
        self.0.is_some_and(|at| at.elapsed() < SET_GUARD)
    }
}

/// Shared, lock-guarded [`RadioState`].
pub type SharedState = Arc<Mutex<RadioState>>;

/// A snapshot of the radio settings rigctld can report.
#[derive(Debug, Clone, Default)]
pub struct RadioState {
    pub radio: Option<RadioModel>,
    pub freq_a: u64,
    pub freq_b: u64,
    pub mode_a: Option<Mode>,
    pub mode_b: Option<Mode>,
    /// `true` when VFO-B (Sub) is selected for receive.
    pub sub_vfo: bool,
    pub split: bool,
    pub rit_on: bool,
    pub xit_on: bool,
    pub rit_hz: i32,
    pub xit_hz: i32,
    pub atu_on: bool,
    pub ptt: bool,
    pub power_on: bool,
    pub smeter: u8,
    pub power_w: u16,
    pub mic_gain: u8,
    pub rf_gain: u8,
    pub squelch: u8,
    pub agc: Option<Agc>,
    pub nb: bool,
    pub nr: bool,
    pub auto_notch: bool,
    pub narrow: bool,
    /// Bumped on every client-initiated frequency change, so deferred
    /// re-sends can tell whether a newer `set_freq` superseded them.
    freq_seq: u64,
    freq_a_guard: Guard,
    freq_b_guard: Guard,
    vfo_guard: Guard,
    split_guard: Guard,
}

impl RadioState {
    /// Sequence number of the latest client-initiated frequency change.
    pub fn freq_seq(&self) -> u64 {
        self.freq_seq
    }

    /// `true` when the transmit VFO is VFO-B. With split on, transmit uses the
    /// VFO that is *not* selected for receive; otherwise it follows receive.
    pub fn tx_sub(&self) -> bool {
        if self.split {
            !self.sub_vfo
        } else {
            self.sub_vfo
        }
    }

    /// Frequency of the VFO that is *not* selected for receive (the split
    /// transmit VFO).
    pub fn other_frequency(&self) -> u64 {
        if self.sub_vfo {
            self.freq_a
        } else {
            self.freq_b
        }
    }

    /// Set the frequency of the VFO that is *not* selected for receive.
    pub fn set_other_frequency(&mut self, hz: u64) {
        self.set_frequency_for(!self.sub_vfo, hz);
    }

    /// Mode of the VFO that is *not* selected for receive.
    pub fn other_mode(&self) -> Option<Mode> {
        if self.sub_vfo {
            self.mode_a
        } else {
            self.mode_b
        }
    }

    pub fn set_other_mode(&mut self, mode: Mode) {
        if self.sub_vfo {
            self.mode_a = Some(mode);
        } else {
            self.mode_b = Some(mode);
        }
    }

    /// Record a client-initiated receive-VFO selection.
    pub fn set_sub_vfo(&mut self, sub: bool) {
        self.sub_vfo = sub;
        self.vfo_guard.arm();
    }

    /// Record a client-initiated split on/off.
    pub fn set_split_on(&mut self, on: bool) {
        self.split = on;
        self.split_guard.arm();
    }

    /// Record a client-initiated frequency change on a specific VFO.
    pub fn set_frequency_for(&mut self, sub: bool, hz: u64) {
        if sub {
            self.freq_b = hz;
            self.freq_b_guard.arm();
        } else {
            self.freq_a = hz;
            self.freq_a_guard.arm();
        }
        self.freq_seq = self.freq_seq.wrapping_add(1);
    }

    /// Frequency of the active receive VFO.
    pub fn frequency(&self) -> u64 {
        if self.sub_vfo {
            self.freq_b
        } else {
            self.freq_a
        }
    }

    pub fn set_frequency(&mut self, hz: u64) {
        self.set_frequency_for(self.sub_vfo, hz);
    }

    /// Mode of the active receive VFO.
    pub fn mode(&self) -> Option<Mode> {
        if self.sub_vfo {
            self.mode_b
        } else {
            self.mode_a
        }
    }

    pub fn set_mode(&mut self, mode: Mode) {
        if self.sub_vfo {
            self.mode_b = Some(mode);
        } else {
            self.mode_a = Some(mode);
        }
    }

    /// Update the cache from one CAT response frame.
    pub fn apply(&mut self, frame: &str) {
        let Some(parsed) = scu_cat::split(frame) else {
            return;
        };
        match parsed.command {
            "FA" => {
                if let Some(hz) = scu_cat::parse_frequency(frame) {
                    if !self.freq_a_guard.active() {
                        self.freq_a = hz;
                    }
                }
            }
            "FB" => {
                if let Some(hz) = scu_cat::parse_frequency(frame) {
                    if !self.freq_b_guard.active() {
                        self.freq_b = hz;
                    }
                }
            }
            "MD" => {
                // The FTDX10's `MD P1` is relative to the operating VFO (`0` =
                // active, `1` = inactive), so map it through the known active
                // VFO. (`FA`/`FB` are absolute; `MD` is not.)
                let p1 = parsed.payload.starts_with('1');
                let sub = self.sub_vfo ^ p1;
                if let Some(mode) = scu_cat::parse_mode(frame) {
                    if sub {
                        self.mode_b = Some(mode);
                    } else {
                        self.mode_a = Some(mode);
                    }
                }
            }
            "VS" => {
                if let Some(sub) = scu_cat::parse_vfo(frame) {
                    if !self.vfo_guard.active() {
                        self.sub_vfo = sub;
                    }
                }
            }
            "ST" => {
                if let Some(on) = scu_cat::parse_split(frame) {
                    if !self.split_guard.active() {
                        self.split = on;
                    }
                }
            }
            "RT" => {
                if let Some(on) = scu_cat::parse_rit(frame) {
                    self.rit_on = on;
                }
            }
            "XT" => {
                if let Some(on) = scu_cat::parse_xit(frame) {
                    self.xit_on = on;
                }
            }
            "CF" => {
                if let Some(hz) = scu_cat::parse_clarifier_offset(frame) {
                    // The FTDX10 shares one clarifier offset between RIT and XIT.
                    self.rit_hz = hz;
                    self.xit_hz = hz;
                }
            }
            "TX" => {
                if let Some(on) = scu_cat::parse_transmit(frame) {
                    self.ptt = on;
                }
            }
            "PS" => {
                if let Some(on) = scu_cat::parse_radio_power(frame) {
                    self.power_on = on;
                }
            }
            "AC" => {
                if let Some(on) = scu_cat::parse_atu(frame) {
                    self.atu_on = on;
                }
            }
            "PC" => {
                if let Some(watts) = scu_cat::parse_power(frame) {
                    self.power_w = watts;
                }
            }
            "MG" => {
                if let Some(percent) = scu_cat::parse_mic_gain(frame) {
                    self.mic_gain = percent;
                }
            }
            "RG" => {
                if let Some(value) = scu_cat::parse_rf_gain(frame) {
                    self.rf_gain = value;
                }
            }
            "SQ" => {
                if let Some(value) = scu_cat::parse_squelch(frame) {
                    self.squelch = value;
                }
            }
            "GT" => {
                if let Some(agc) = scu_cat::parse_agc(frame) {
                    self.agc = Some(agc);
                }
            }
            "NB" => {
                if let Some(on) = scu_cat::parse_noise_blanker(frame) {
                    self.nb = on;
                }
            }
            "NR" => {
                if let Some(on) = scu_cat::parse_noise_reduction(frame) {
                    self.nr = on;
                }
            }
            "BC" => {
                if let Some(on) = scu_cat::parse_auto_notch(frame) {
                    self.auto_notch = on;
                }
            }
            "NA" => {
                if let Some(on) = scu_cat::parse_narrow(frame) {
                    self.narrow = on;
                }
            }
            "SM" => {
                let sub = parsed.payload.starts_with('1');
                if let Some(value) = parsed.payload.get(1..).and_then(|d| d.parse::<u32>().ok()) {
                    // S-meter is shared on these radios; keep the active VFO's.
                    if sub == self.sub_vfo {
                        self.smeter = value.min(255) as u8;
                    }
                }
            }
            "ID" => {
                if let Some(id) = scu_cat::parse_id(frame) {
                    self.radio = Some(RadioModel::from_id(id));
                }
            }
            "IF" => {
                // `IF` reports the *operating* frequency: the receive VFO's, or
                // the transmit VFO's while keyed in split. Only trust it for the
                // receive VFO while not transmitting, and never let it clobber a
                // value a client just set.
                if let Some(status) = scu_cat::parse_if(frame) {
                    if !self.ptt {
                        if self.sub_vfo {
                            if !self.freq_b_guard.active() {
                                self.freq_b = status.frequency_hz;
                            }
                        } else if !self.freq_a_guard.active() {
                            self.freq_a = status.frequency_hz;
                        }
                    }
                }
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn applies_core_frames() {
        let mut state = RadioState::default();
        state.apply("FA014074000;");
        state.apply("FB007074000;");
        state.apply("MD02;");
        state.apply("MD1C;");
        state.apply("VS1;");
        state.apply("ST1;");
        state.apply("TX1;");
        state.apply("PC050;");
        state.apply("ID0761;");

        assert_eq!(state.freq_a, 14_074_000);
        assert_eq!(state.freq_b, 7_074_000);
        assert_eq!(state.mode_a, Some(Mode::Usb));
        assert_eq!(state.mode_b, Some(Mode::DataU));
        assert!(state.sub_vfo);
        assert!(state.split);
        assert!(state.ptt);
        assert_eq!(state.power_w, 50);
        assert_eq!(state.radio, Some(RadioModel::Ftdx10));
        // Active VFO is B.
        assert_eq!(state.frequency(), 7_074_000);
        assert_eq!(state.mode(), Some(Mode::DataU));
    }

    #[test]
    fn md_p1_is_relative_to_active_vfo() {
        // VFO-B active: MD0 = active (B), MD1 = inactive (A).
        let mut state = RadioState {
            sub_vfo: true,
            ..Default::default()
        };
        state.apply("MD0C;");
        state.apply("MD15;");
        assert_eq!(state.mode_b, Some(Mode::DataU));
        assert_eq!(state.mode_a, Some(Mode::Am));

        // VFO-A active: the mapping flips.
        state.sub_vfo = false;
        state.apply("MD05;");
        state.apply("MD1C;");
        assert_eq!(state.mode_a, Some(Mode::Am));
        assert_eq!(state.mode_b, Some(Mode::DataU));
    }

    #[test]
    fn set_frequency_targets_active_vfo() {
        let mut state = RadioState::default();
        state.set_frequency(7_000_000);
        assert_eq!(state.freq_a, 7_000_000);
        state.sub_vfo = true;
        state.set_frequency(14_000_000);
        assert_eq!(state.freq_b, 14_000_000);
        assert_eq!(state.freq_a, 7_000_000);
    }

    #[test]
    fn if_updates_only_the_receive_vfo_and_not_while_keyed() {
        let if_frame = "IF007074000000+00000000000;";
        let mut state = RadioState::default();
        state.apply(if_frame);
        assert_eq!(state.freq_a, 7_074_000);

        let mut state = RadioState {
            sub_vfo: true,
            freq_a: 14_074_000,
            ..Default::default()
        };
        state.apply(if_frame);
        assert_eq!(state.freq_a, 14_074_000);
        assert_eq!(state.freq_b, 7_074_000);

        let mut state = RadioState {
            ptt: true,
            freq_a: 14_074_000,
            ..Default::default()
        };
        state.apply(if_frame);
        assert_eq!(state.freq_a, 14_074_000);
    }

    #[test]
    fn client_set_shields_value_from_stale_replies() {
        let mut state = RadioState::default();
        state.set_frequency_for(false, 14_074_000);
        state.apply("FA007074000;");
        assert_eq!(state.freq_a, 14_074_000);
        // The other VFO is not shielded.
        state.apply("FB007074000;");
        assert_eq!(state.freq_b, 7_074_000);
    }

    #[test]
    fn tx_vfo_follows_split() {
        let mut state = RadioState::default();
        assert!(!state.tx_sub());
        state.split = true;
        assert!(state.tx_sub());
        state.sub_vfo = true;
        assert!(!state.tx_sub());
    }

    #[test]
    fn ignores_garbage() {
        let mut state = RadioState::default();
        state.apply("not a frame");
        state.apply("FA12345;");
        assert_eq!(state.freq_a, 0);
    }
}
