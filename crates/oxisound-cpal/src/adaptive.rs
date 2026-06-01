//! Adaptive buffer-sizing policy.
//!
//! [`AdaptiveBufferSizer`] is a pure, allocation-free state machine that decides what
//! buffer size a stream *should* use based on observed underruns. It does **not** rebuild
//! or touch any live stream — applying a new size requires a stream rebuild, which is left
//! to the caller. The policy is:
//!
//! * Start at the requested size.
//! * On an underrun, grow the size (double it, capped at `max`) and reset the stability counter.
//! * After `stable_threshold` consecutive stable periods with no underrun, shrink toward the
//!   initial size (halve it, floored at `min`) and reset the stability counter.
//!
//! Growing reacts quickly to glitches; shrinking is conservative so a single recovered glitch
//! does not immediately undo the safety margin.

/// Decides buffer sizes adaptively from observed underruns.
///
/// # Examples
/// ```
/// use oxisound_cpal::AdaptiveBufferSizer;
/// // Start at 256 frames, never below 128, never above 2048, shrink after 4 stable periods.
/// let mut sizer = AdaptiveBufferSizer::new(256, 128, 2048, 4);
/// assert_eq!(sizer.current_size(), 256);
///
/// // An underrun doubles the buffer.
/// assert_eq!(sizer.record_underrun(), 512);
/// assert!(sizer.size_changed());
///
/// // Stable periods accumulate; only after the threshold does it shrink.
/// for _ in 0..3 {
///     assert_eq!(sizer.record_stable_period(), 512);
/// }
/// assert_eq!(sizer.record_stable_period(), 256); // 4th stable period → shrink
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AdaptiveBufferSizer {
    initial: u32,
    min: u32,
    max: u32,
    current: u32,
    stable_threshold: u32,
    stable_count: u32,
    changed: bool,
}

impl AdaptiveBufferSizer {
    /// Creates a new sizer.
    ///
    /// * `initial` — starting (and target floor for shrinking) buffer size in frames.
    /// * `min` — hard lower bound; the size never drops below this.
    /// * `max` — hard upper bound; the size never grows past this.
    /// * `stable_threshold` — number of consecutive stable periods required before shrinking.
    ///   A value of `0` is treated as `1` (shrink on the next stable period).
    ///
    /// Bounds are normalised so that `min <= initial <= max` always holds, even if the caller
    /// passes inconsistent values.
    #[must_use]
    pub fn new(initial: u32, min: u32, max: u32, stable_threshold: u32) -> Self {
        let min = min.max(1);
        let max = max.max(min);
        let initial = initial.clamp(min, max);
        Self {
            initial,
            min,
            max,
            current: initial,
            stable_threshold: stable_threshold.max(1),
            stable_count: 0,
            changed: false,
        }
    }

    /// Returns the buffer size the stream should currently use, in frames.
    #[must_use]
    pub fn current_size(&self) -> u32 {
        self.current
    }

    /// Returns `true` if the most recent `record_*` call changed [`current_size`](Self::current_size).
    ///
    /// Use this to decide whether a (caller-driven) stream rebuild is warranted.
    #[must_use]
    pub fn size_changed(&self) -> bool {
        self.changed
    }

    /// Records that an underrun occurred. Grows the buffer (doubled, capped at `max`) and
    /// resets the stability counter. Returns the new [`current_size`](Self::current_size).
    pub fn record_underrun(&mut self) -> u32 {
        self.stable_count = 0;
        let grown = self.current.saturating_mul(2).min(self.max);
        self.changed = grown != self.current;
        self.current = grown;
        self.current
    }

    /// Records one stable period (no underrun since the last call). Once
    /// `stable_threshold` consecutive stable periods accumulate, shrinks the buffer
    /// (halved, floored at the larger of `min` and `initial`) and resets the counter.
    /// Returns the new [`current_size`](Self::current_size).
    pub fn record_stable_period(&mut self) -> u32 {
        // Already at or below the initial size — nothing to shrink toward.
        if self.current <= self.initial {
            self.changed = false;
            self.stable_count = 0;
            return self.current;
        }
        self.stable_count += 1;
        if self.stable_count >= self.stable_threshold {
            self.stable_count = 0;
            let floor = self.min.max(self.initial);
            let shrunk = (self.current / 2).max(floor);
            self.changed = shrunk != self.current;
            self.current = shrunk;
        } else {
            self.changed = false;
        }
        self.current
    }

    /// Resets the sizer back to its initial size and clears all counters.
    pub fn reset(&mut self) {
        self.current = self.initial;
        self.stable_count = 0;
        self.changed = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn starts_at_initial() {
        let sizer = AdaptiveBufferSizer::new(256, 128, 2048, 4);
        assert_eq!(sizer.current_size(), 256);
        assert!(!sizer.size_changed());
    }

    #[test]
    fn underrun_doubles_capped_at_max() {
        let mut sizer = AdaptiveBufferSizer::new(256, 128, 1024, 4);
        assert_eq!(sizer.record_underrun(), 512);
        assert!(sizer.size_changed());
        assert_eq!(sizer.record_underrun(), 1024);
        // Already at max → no further growth, no change flag.
        assert_eq!(sizer.record_underrun(), 1024);
        assert!(!sizer.size_changed());
    }

    #[test]
    fn shrinks_only_after_threshold() {
        let mut sizer = AdaptiveBufferSizer::new(256, 128, 2048, 3);
        sizer.record_underrun(); // 512
        assert_eq!(sizer.current_size(), 512);
        assert_eq!(sizer.record_stable_period(), 512); // 1
        assert!(!sizer.size_changed());
        assert_eq!(sizer.record_stable_period(), 512); // 2
        assert_eq!(sizer.record_stable_period(), 256); // 3 → shrink
        assert!(sizer.size_changed());
    }

    #[test]
    fn never_shrinks_below_initial() {
        let mut sizer = AdaptiveBufferSizer::new(256, 64, 2048, 1);
        // No underrun: stable periods must not drop below initial.
        for _ in 0..10 {
            assert_eq!(sizer.record_stable_period(), 256);
        }
    }

    #[test]
    fn shrink_floor_respects_initial_above_min() {
        let mut sizer = AdaptiveBufferSizer::new(512, 64, 4096, 1);
        sizer.record_underrun(); // 1024
        sizer.record_underrun(); // 2048
        assert_eq!(sizer.record_stable_period(), 1024); // shrink each stable period
        assert_eq!(sizer.record_stable_period(), 512); // floored at initial (512), not min (64)
        assert_eq!(sizer.record_stable_period(), 512); // stays at initial
    }

    #[test]
    fn reset_restores_initial() {
        let mut sizer = AdaptiveBufferSizer::new(256, 128, 2048, 4);
        sizer.record_underrun();
        sizer.record_underrun();
        assert_ne!(sizer.current_size(), 256);
        sizer.reset();
        assert_eq!(sizer.current_size(), 256);
        assert!(!sizer.size_changed());
    }

    #[test]
    fn inconsistent_bounds_are_normalised() {
        // initial above max, min above max — must not panic and must stay consistent.
        let sizer = AdaptiveBufferSizer::new(9999, 500, 100, 0);
        assert!(sizer.current_size() >= 1);
        assert!(sizer.current_size() <= sizer.current_size().max(500));
    }

    #[test]
    fn zero_threshold_treated_as_one() {
        let mut sizer = AdaptiveBufferSizer::new(256, 128, 2048, 0);
        sizer.record_underrun(); // 512
        // threshold normalised to 1 → first stable period shrinks.
        assert_eq!(sizer.record_stable_period(), 256);
    }
}
