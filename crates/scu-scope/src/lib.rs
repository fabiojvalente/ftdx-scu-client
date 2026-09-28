//! Spectrum-scope decoding and waterfall color mapping.
//!
//! Scope bodies are 4096 bytes with an inverted amplitude encoding
//! (`0` = strongest signal, `255` = weakest). The active spectrum ends at a
//! boundary marker (see [`find_spectrum_end`]).
//!
//! The active region holds **two bytes per visible bin** (an interleaved second
//! stream / oversampling). The published spec rendered the first contiguous
//! `boundary / 2` bytes, which covers only the left half of the span and makes
//! a centered signal appear at the right edge. We take one byte per bin across
//! the whole region ([`BinInterleave::Split`]) so bins span the full width.

use scu_protocol::SCOPE_BODY_LEN;

/// A single decoded sweep line.
#[derive(Debug, Clone, PartialEq)]
pub struct ScopeLine {
    /// Normalized magnitudes in `0.0..=1.0` (1.0 = strongest).
    pub bins: Vec<f32>,
    /// Position of the boundary marker within the raw body.
    pub boundary: usize,
}

impl ScopeLine {
    pub fn bin_count(&self) -> usize {
        self.bins.len()
    }
}

/// Find the end of the active spectrum (boundary marker) in a scope body.
pub fn find_spectrum_end(packet: &[u8]) -> usize {
    let len = packet.len().min(SCOPE_BODY_LEN);
    if len <= 500 {
        return len.min(2000);
    }

    // Primary: first bin <= 1 after bin 500.
    let upper = len.min(3000);
    for (i, &value) in packet.iter().enumerate().take(upper).skip(500) {
        if value <= 1 {
            return i;
        }
    }

    // Fallback: first 64-bin block with average > 225 (reference/calibration).
    const BLOCK: usize = 64;
    if len > BLOCK {
        let upper = (len - BLOCK).min(3000);
        let mut start = 500;
        while start < upper {
            let avg: u32 = packet[start..start + BLOCK]
                .iter()
                .map(|&b| b as u32)
                .sum::<u32>()
                / BLOCK as u32;
            if avg > 225 {
                return start.saturating_sub(100).max(100);
            }
            start += BLOCK;
        }
    }

    len.min(2000)
}

/// Number of usable bins for a body (empirical `boundary // 2` rule).
pub fn usable_bin_count(packet: &[u8]) -> usize {
    (find_spectrum_end(packet) / 2).max(1)
}

/// Normalize a raw amplitude byte: `1.0 - raw / 255.0`.
#[inline]
pub fn normalize(raw: u8) -> f32 {
    1.0 - (raw as f32 / 255.0)
}

/// How to extract visible bins from the active (doubled) region.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinInterleave {
    /// One byte per bin, stepping over the interleaved second stream. Covers the
    /// full scope width and lines the center bin up with the VFO.
    Split,
    /// The first contiguous `boundary / 2` bytes (the published spec's rule).
    Contiguous,
}

impl BinInterleave {
    pub const ALL: [BinInterleave; 2] = [BinInterleave::Split, BinInterleave::Contiguous];

    pub fn label(&self) -> &'static str {
        match self {
            BinInterleave::Split => "Interleaved (full span)",
            BinInterleave::Contiguous => "Contiguous (first half)",
        }
    }

    fn stride(&self) -> usize {
        match self {
            BinInterleave::Split => 2,
            BinInterleave::Contiguous => 1,
        }
    }
}

/// Decode a raw scope body using the default [`BinInterleave::Split`] rule.
pub fn decode(packet: &[u8]) -> ScopeLine {
    decode_with(packet, BinInterleave::Split)
}

/// Decode a raw scope body into a sweep line using the given bin rule.
pub fn decode_with(packet: &[u8], mode: BinInterleave) -> ScopeLine {
    let boundary = find_spectrum_end(packet);
    if packet.is_empty() {
        return ScopeLine {
            bins: Vec::new(),
            boundary,
        };
    }
    let active = boundary.min(packet.len());
    let count = (active / 2).max(1);
    let stride = mode.stride();
    let bins = (0..count)
        .map(|i| normalize(packet[(i * stride).min(packet.len() - 1)]))
        .collect();
    ScopeLine { bins, boundary }
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

    fn flat_body(floor: u8) -> Vec<u8> {
        let mut v = vec![floor; SCOPE_BODY_LEN];
        // Put a boundary marker at 1800.
        v[1800] = 1;
        v
    }

    #[test]
    fn finds_marker_after_bin_500() {
        let body = flat_body(230);
        assert_eq!(find_spectrum_end(&body), 1800);
        assert_eq!(usable_bin_count(&body), 900);
    }

    #[test]
    fn ignores_strong_signal_before_500() {
        let mut body = flat_body(230);
        body[100] = 0;
        body[200] = 0;
        assert_eq!(find_spectrum_end(&body), 1800);
    }

    #[test]
    fn fallback_uses_reference_block() {
        // No marker <= 1 anywhere; a high-average block after 500 triggers fallback.
        let mut body = vec![230u8; SCOPE_BODY_LEN];
        for b in body.iter_mut().take(1900).skip(1900 - 64) {
            *b = 240;
        }
        let end = find_spectrum_end(&body);
        assert!(end >= 100);
        assert!(end <= 1900);
    }

    #[test]
    fn decode_length_and_range() {
        let line = decode(&flat_body(200));
        assert_eq!(line.bins.len(), 900);
        assert_eq!(line.boundary, 1800);
        assert!(line.bins.iter().all(|m| (0.0..=1.0).contains(m)));
        // floor 200 -> 1 - 200/255
        assert!((line.bins[0] - (1.0 - 200.0 / 255.0)).abs() < 1e-6);
    }

    #[test]
    fn split_covers_full_region_but_contiguous_only_half() {
        // Even bytes carry one sweep; odd bytes carry a second (interleaved) stream.
        let mut body = vec![255u8; SCOPE_BODY_LEN];
        body[1800] = 1;
        for i in (0..1800).step_by(2) {
            body[i] = 40; // a strong bin every other byte: front..back
        }
        for i in (1..1800).step_by(2) {
            body[i] = 200; // interleaved weak stream
        }

        let split = decode_with(&body, BinInterleave::Split);
        assert_eq!(split.bins.len(), 900);
        // Every visible bin came from the strong even stream...
        assert!(split.bins.iter().all(|m| (*m - normalize(40)).abs() < 1e-6));

        let contiguous = decode_with(&body, BinInterleave::Contiguous);
        // ...while the old rule mixes the two streams.
        assert!(contiguous
            .bins
            .iter()
            .any(|m| (*m - normalize(40)).abs() < 1e-6));
        assert!(contiguous
            .bins
            .iter()
            .any(|m| (*m - normalize(200)).abs() < 1e-6));
    }

    #[test]
    fn decode_empty_body_is_safe() {
        let line = decode(&[]);
        assert!(line.bins.is_empty());
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
    }
}
