//! Voice-operated transmit (VOX) state machine.
//!
//! Pure and time-injected so it can be unit-tested: the caller passes the
//! elapsed milliseconds since the previous update together with the current
//! input level.

/// VOX tuning. Levels use the same 0..=32767 scale as [`crate::input`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VoxConfig {
    /// Input peak at or above which the transmitter keys.
    pub threshold: u16,
    /// Level must stay above the threshold this long before keying.
    pub attack_ms: u32,
    /// After the level drops, wait this long before unkeying.
    pub hang_ms: u32,
}

impl Default for VoxConfig {
    fn default() -> Self {
        Self {
            threshold: 800,
            attack_ms: 60,
            hang_ms: 500,
        }
    }
}

/// VOX keyer state.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Vox {
    above_ms: u32,
    below_ms: u32,
    keyed: bool,
}

impl Vox {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn keyed(&self) -> bool {
        self.keyed
    }

    /// Feed a new level and return whether the transmitter should be keyed.
    pub fn update(&mut self, level: u16, config: &VoxConfig, elapsed_ms: u32) -> bool {
        if level >= config.threshold {
            self.above_ms = self.above_ms.saturating_add(elapsed_ms);
            self.below_ms = 0;
            if !self.keyed && self.above_ms >= config.attack_ms {
                self.keyed = true;
            }
        } else {
            self.below_ms = self.below_ms.saturating_add(elapsed_ms);
            self.above_ms = 0;
            if self.keyed && self.below_ms >= config.hang_ms {
                self.keyed = false;
            }
        }
        self.keyed
    }

    /// Force the keyer back to idle (e.g. VOX disabled or disconnected).
    pub fn reset(&mut self) {
        self.above_ms = 0;
        self.below_ms = 0;
        self.keyed = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config() -> VoxConfig {
        VoxConfig {
            threshold: 1000,
            attack_ms: 50,
            hang_ms: 200,
        }
    }

    #[test]
    fn keys_after_attack_and_releases_after_hang() {
        let mut vox = Vox::new();
        let config = config();

        // Below threshold: stays unkeyed.
        assert!(!vox.update(500, &config, 100));

        // Loud, but not yet long enough.
        assert!(!vox.update(2000, &config, 20));
        assert!(!vox.update(2000, &config, 20));
        // Attack satisfied.
        assert!(vox.update(2000, &config, 20));

        // Quiet, but still within the hang time.
        assert!(vox.update(0, &config, 100));
        // Hang expired.
        assert!(!vox.update(0, &config, 100));
    }

    #[test]
    fn brief_spike_does_not_key() {
        let mut vox = Vox::new();
        let config = config();
        assert!(!vox.update(2000, &config, 10));
        assert!(!vox.update(0, &config, 10));
        assert!(!vox.keyed());
    }

    #[test]
    fn reset_clears_state() {
        let mut vox = Vox::new();
        let config = config();
        vox.update(2000, &config, 100);
        assert!(vox.keyed());
        vox.reset();
        assert!(!vox.keyed());
    }
}
