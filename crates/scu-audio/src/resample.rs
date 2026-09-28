//! A tiny streaming linear resampler shared by the playback (RX) and capture
//! (TX) paths.
//!
//! It carries the last input sample and the fractional read position across
//! calls, so it can resample an arbitrarily-chunked stream without seams.

/// Mono linear resampler. `step` is `input_rate / output_rate`.
pub(crate) struct ChannelResampler {
    prev: f32,
    pos: f64,
    step: f64,
}

impl ChannelResampler {
    pub(crate) fn new(step: f64) -> Self {
        Self {
            prev: 0.0,
            pos: 1.0,
            step,
        }
    }

    /// Push resampled output for a new block of mono input samples.
    pub(crate) fn process(&mut self, input: &[f32], out: &mut Vec<f32>) {
        let n = input.len();
        if n == 0 {
            return;
        }
        while self.pos < n as f64 {
            let i = self.pos.floor() as usize;
            let frac = (self.pos - i as f64) as f32;
            let a = if i == 0 { self.prev } else { input[i - 1] };
            let b = input[i];
            out.push(a + (b - a) * frac);
            self.pos += self.step;
        }
        self.pos -= n as f64;
        self.prev = input[n - 1];
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run_resampler(step: f64, blocks: &[Vec<f32>]) -> Vec<f32> {
        let mut r = ChannelResampler::new(step);
        let mut all = Vec::new();
        for b in blocks {
            r.process(b, &mut all);
        }
        all
    }

    #[test]
    fn identity_rate_preserves_values() {
        let blocks = vec![vec![1.0, 2.0, 3.0, 4.0], vec![5.0, 6.0, 7.0, 8.0]];
        let out = run_resampler(1.0, &blocks);
        // One sample of pipeline latency: N inputs -> N-1 outputs overall.
        assert_eq!(out.len(), 7);
        assert!((out[0] - 1.0).abs() < 1e-6);
        assert!((out[6] - 7.0).abs() < 1e-6);
    }

    #[test]
    fn upsampling_triples_throughput() {
        let blocks = vec![vec![0.0f32, 1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0]];
        let out = run_resampler(1.0 / 3.0, &blocks);
        assert!(out.len() >= 21 && out.len() <= 24, "got {}", out.len());
    }

    #[test]
    fn interpolation_is_monotonic_for_ramp() {
        let blocks = vec![(0..100).map(|i| i as f32).collect::<Vec<_>>()];
        let out = run_resampler(0.5, &blocks);
        for w in out.windows(2) {
            assert!(w[1] >= w[0] - 1e-3);
        }
    }
}
