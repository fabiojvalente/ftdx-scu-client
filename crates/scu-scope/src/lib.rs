//! Spectrum-scope decoding and waterfall color mapping.
//!
//! A decrypted scope body is 4096 bytes and uses the same layout the
//! FTDX10 / FTDX101 / FT-710 expose over their SPI scope interface. The main
//! receiver's spectrum is the first **850 bytes**, one amplitude byte per bin,
//! with an inverted encoding (`0` = strongest, `255` = weakest):
//!
//! | Offset      | Length | Meaning                                             |
//! |-------------|--------|-----------------------------------------------------|
//! | `0..850`    | 850    | main receiver waterfall (`wf1`)                     |
//! | `850..1700` | 850    | second receiver waterfall (`wf2`, unused on FTDX10) |
//! | `1700..`    | —      | audio FFT / oscilloscope / scope metadata           |
//!
//! The bin count and offset are fixed and independent of the scope span, so no
//! boundary search is needed. Earlier versions searched for a "boundary marker"
//! and decoded two bytes per bin, which interleaved `wf1` with the unused `wf2`
//! buffer and halved the frequency resolution.

/// Bins in the main receiver waterfall (`wf1`), at the start of a scope body.
pub const WF1_BINS: usize = 850;

/// A single decoded sweep line.
#[derive(Debug, Clone, PartialEq)]
pub struct ScopeLine {
    /// Normalized magnitudes in `0.0..=1.0` (1.0 = strongest).
    pub bins: Vec<f32>,
}

impl ScopeLine {
    pub fn bin_count(&self) -> usize {
        self.bins.len()
    }
}

/// Normalize a raw amplitude byte: `1.0 - raw / 255.0`.
#[inline]
pub fn normalize(raw: u8) -> f32 {
    1.0 - (raw as f32 / 255.0)
}

/// Decode a raw scope body into a sweep line.
///
/// The main receiver's spectrum is the first [`WF1_BINS`] bytes of the body,
/// one bin per byte. Shorter bodies yield only the bytes present.
pub fn decode(packet: &[u8]) -> ScopeLine {
    let count = packet.len().min(WF1_BINS);
    let bins = packet[..count].iter().map(|&raw| normalize(raw)).collect();
    ScopeLine { bins }
}

/// Linear mapping from bin index to absolute frequency (Hz).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FrequencyAxis {
    pub center_hz: f64,
    pub span_hz: f64,
}

impl FrequencyAxis {
    pub fn new(center_hz: f64, span_hz: f64) -> Self {
        Self { center_hz, span_hz }
    }

    /// Frequency at normalized position `t` in `0.0..=1.0` (left → right).
    pub fn hz_at(&self, t: f64) -> f64 {
        self.center_hz + (t - 0.5) * self.span_hz
    }

    /// Frequency of a given bin for a line with `bins` total.
    pub fn hz_for_bin(&self, bin: usize, bins: usize) -> f64 {
        if bins <= 1 {
            return self.center_hz;
        }
        self.hz_at(bin as f64 / (bins - 1) as f64)
    }
}

/// Waterfall color maps.
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Colormap {
    Grayscale,
    Viridis,
    Turbo,
}

impl Colormap {
    pub const ALL: [Colormap; 3] = [Colormap::Turbo, Colormap::Viridis, Colormap::Grayscale];

    pub fn label(&self) -> &'static str {
        match self {
            Colormap::Grayscale => "Grayscale",
            Colormap::Viridis => "Viridis",
            Colormap::Turbo => "Turbo",
        }
    }

    /// Map a normalized magnitude to RGBA.
    pub fn rgba(&self, magnitude: f32) -> [u8; 4] {
        let t = magnitude.clamp(0.0, 1.0);
        match self {
            Colormap::Grayscale => {
                let v = (t * 255.0) as u8;
                [v, v, v, 255]
            }
            Colormap::Viridis => viridis(t),
            Colormap::Turbo => turbo(t),
        }
    }
}

fn lerp(a: u8, b: u8, t: f32) -> u8 {
    (a as f32 + (b as f32 - a as f32) * t)
        .round()
        .clamp(0.0, 255.0) as u8
}

fn sample(anchors: &[[u8; 3]], t: f32) -> [u8; 4] {
    let n = anchors.len();
    if n == 0 {
        return [0, 0, 0, 255];
    }
    let scaled = t * (n - 1) as f32;
    let i = (scaled.floor() as usize).min(n - 2);
    let f = scaled - i as f32;
    let a = anchors[i];
    let b = anchors[i + 1];
    [
        lerp(a[0], b[0], f),
        lerp(a[1], b[1], f),
        lerp(a[2], b[2], f),
        255,
    ]
}

const VIRIDIS: [[u8; 3]; 9] = [
    [68, 1, 84],
    [72, 40, 120],
    [62, 74, 137],
    [49, 104, 142],
    [38, 130, 142],
    [31, 158, 137],
    [53, 183, 121],
    [109, 205, 89],
    [253, 231, 37],
];

const TURBO: [[u8; 3]; 9] = [
    [48, 18, 59],
    [70, 107, 227],
    [40, 187, 235],
    [32, 220, 160],
    [162, 236, 66],
    [249, 199, 51],
    [247, 127, 31],
    [204, 51, 6],
    [122, 4, 3],
];

fn viridis(t: f32) -> [u8; 4] {
    sample(&VIRIDIS, t)
}

fn turbo(t: f32) -> [u8; 4] {
    sample(&TURBO, t)
}

#[cfg(test)]
mod tests {
    use super::*;
    use scu_protocol::SCOPE_BODY_LEN;

    #[test]
    fn decode_reads_wf1_contiguously() {
        // `wf1` is the first 850 bytes, one bin per byte. `wf2` (850..1700)
        // must not bleed into the decoded line.
        let mut body = vec![255u8; SCOPE_BODY_LEN];
        body[..WF1_BINS].fill(200);
        body[0] = 0; // strongest bin in wf1
        body[WF1_BINS] = 0; // first byte of wf2, must be ignored

        let line = decode(&body);
        assert_eq!(line.bins.len(), WF1_BINS);
        assert_eq!(line.bins[0], 1.0);
        assert!((line.bins[1] - (1.0 - 200.0 / 255.0)).abs() < 1e-6);
        assert!(line.bins.iter().all(|m| (0.0..=1.0).contains(m)));
    }

    #[test]
    fn decode_short_body_is_safe() {
        assert!(decode(&[]).bins.is_empty());
        assert_eq!(decode(&[0, 128, 255]).bins.len(), 3);
    }

    #[test]
    fn normalization_bounds() {
        assert_eq!(normalize(0), 1.0);
        assert_eq!(normalize(255), 0.0);
    }

    #[test]
    fn colormaps_are_opaque() {
        for map in Colormap::ALL {
            for t in [0.0f32, 0.5, 1.0] {
                assert_eq!(map.rgba(t)[3], 255);
            }
        }
    }

    #[test]
    fn frequency_axis_maps_center() {
        let axis = FrequencyAxis::new(7_007_000.0, 200_000.0);
        assert_eq!(axis.hz_at(0.5), 7_007_000.0);
        assert_eq!(axis.hz_at(0.0), 7_007_000.0 - 100_000.0);
        assert_eq!(axis.hz_at(1.0), 7_007_000.0 + 100_000.0);
    }

    #[test]
    fn frequency_axis_maps_ft8_offset_from_vfo() {
        let axis = FrequencyAxis::new(7_030_000.0, 100_000.0);

        // 7.074 MHz is 44 kHz above a 7.030 MHz carrier-centred VFO, so it
        // belongs at 94% of a 100 kHz scope span.
        assert_eq!(axis.hz_at(0.94), 7_074_000.0);
    }
}
