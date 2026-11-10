//! Maps game-clock seconds (what events carry) to seconds into the recording.

/// One observation: at `rec` seconds into the recording the game clock read `game`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Sample {
    pub rec: f64,
    pub game: f64,
}

#[derive(Debug, Clone, Default)]
pub struct TimeMap {
    samples: Vec<Sample>,
}

impl TimeMap {
    pub fn new() -> Self {
        Self::default()
    }

    /// Record an observation. Samples whose offset matches the previous one
    /// (within 50 ms) are skipped, so the list only grows when the game clock
    /// and wall clock drift apart, e.g. on a pause.
    pub fn observe(&mut self, rec: f64, game: f64) {
        if let Some(last) = self.samples.last() {
            if game < last.game {
                return; // clock went backwards; ignore
            }
            if ((rec - game) - (last.rec - last.game)).abs() < 0.05 {
                return;
            }
        }
        self.samples.push(Sample { rec, game });
    }

    pub fn is_empty(&self) -> bool {
        self.samples.is_empty()
    }

    /// Recording time for an event at `game` seconds, using the offset that was
    /// in force at that point of the game. `None` until something was observed.
    /// The result is clamped at zero for events before the recording began.
    pub fn to_recording(&self, game: f64) -> Option<f64> {
        let first = self.samples.first()?;
        let s = self
            .samples
            .iter()
            .rev()
            .find(|s| s.game <= game)
            .unwrap_or(first);
        Some((game + (s.rec - s.game)).max(0.0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_map_gives_none() {
        assert_eq!(TimeMap::new().to_recording(10.0), None);
    }

    #[test]
    fn constant_offset() {
        let mut m = TimeMap::new();
        m.observe(5.0, 20.0); // recording started 15 s into the game
        m.observe(6.0, 21.0);
        m.observe(7.0, 22.01);
        assert_eq!(m.samples.len(), 1);
        assert_eq!(m.to_recording(100.0), Some(85.0));
        assert_eq!(m.to_recording(3.0), Some(0.0)); // before recording: clamped
    }

    #[test]
    fn pause_shifts_later_events_only() {
        let mut m = TimeMap::new();
        m.observe(0.0, 10.0);
        m.observe(100.0, 110.0);
        // 30 s pause: wall clock advanced, game clock did not.
        m.observe(131.0, 111.0);
        assert_eq!(m.to_recording(60.0), Some(50.0));
        assert_eq!(m.to_recording(120.0), Some(140.0));
    }

    #[test]
    fn backwards_clock_ignored() {
        let mut m = TimeMap::new();
        m.observe(10.0, 50.0);
        m.observe(11.0, 2.0);
        assert_eq!(m.to_recording(60.0), Some(20.0));
    }
}
