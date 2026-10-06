//! The one LFO: a global, free-running triangle with a delayed fade-in.
//!
//! `research:instruments/juno-106.md` §3.1: one LFO for the whole instrument, so every voice's vibrato
//! is in phase. **Not per voice** — per-voice LFOs would decorrelate the voices and do part of the
//! chorus's job, which is a different sound. Triangle only, no shape selector, no sync, no key
//! trigger.
//!
//! **DELAY is a fade-in, not a wait.** After the delay time the depth ramps up rather than switching
//! on. The fade's own time constant is not documented anywhere the research found; it is taken as
//! **half the delay time** here, so a long delay also fades in slowly and a zero delay is
//! transparent. Chosen, not measured, and recorded in the crate's AGENTS.md.
//!
//! **What retriggers the delay is a research gap** (`juno-106.md` §3.1). The usual polysynth answer
//! — the first key pressed after every key was released — is what [`poly::Synth`] does, and it is
//! recorded there as chosen. This module only offers [`Lfo::retrigger_delay`].

/// The fade-in's time constant, as a fraction of the delay time.
pub const FADE_FRACTION: f32 = 0.5;

/// Highest rate the control reaches. The hardware's slider ends around 30 Hz.
pub const RATE_MAX_HZ: f32 = 30.0;

#[derive(Debug, Clone)]
pub struct Lfo {
    phase: f32,
    /// Seconds since the delay was last retriggered.
    since_trigger_s: f32,
    /// The depth multiplier, `0..=1`, climbing after the delay elapses.
    fade: f32,
}

impl Default for Lfo {
    fn default() -> Self {
        Self::new()
    }
}

impl Lfo {
    pub const fn new() -> Self {
        Self {
            phase: 0.0,
            since_trigger_s: f32::MAX,
            fade: 1.0,
        }
    }

    /// Zero the phase and the delay. `reset()` only — the LFO free-runs across notes.
    pub fn reset(&mut self) {
        *self = Self::new();
    }

    /// A new phrase began: hold the depth at zero for the delay, then fade it in.
    pub fn retrigger_delay(&mut self) {
        self.since_trigger_s = 0.0;
        self.fade = 0.0;
    }

    /// The current depth multiplier, for tests and telemetry.
    pub fn fade(&self) -> f32 {
        self.fade
    }

    /// Advance one sample and return the triangle in `[-1, 1]`, already scaled by the delay fade.
    #[inline]
    pub fn process(&mut self, rate_hz: f32, delay_s: f32, sample_rate: f32) -> f32 {
        let p = self.phase;
        let triangle = if p < 0.25 {
            4.0 * p
        } else if p < 0.75 {
            2.0 - 4.0 * p
        } else {
            4.0 * p - 4.0
        };

        self.phase += rate_hz.clamp(0.0, RATE_MAX_HZ) / sample_rate;
        if self.phase >= 1.0 {
            self.phase -= 1.0;
        }

        // The delay, then the fade. A zero delay means no fade either: the control at its bottom
        // must be transparent, or "no delay" would still soften every phrase's first vibrato.
        if delay_s <= 0.0 {
            self.fade = 1.0;
        } else if self.since_trigger_s < delay_s {
            self.since_trigger_s += 1.0 / sample_rate;
            self.fade = 0.0;
        } else {
            let tau = (delay_s * FADE_FRACTION).max(0.001);
            let coef = (-1.0 / (tau * sample_rate)).exp();
            self.fade = 1.0 + (self.fade - 1.0) * coef;
            // Snap the last hundredth of a decibel: the exponential would otherwise never arrive.
            if self.fade > 0.999 {
                self.fade = 1.0;
            }
        }

        triangle * self.fade
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FS: f32 = 48_000.0;

    #[test]
    fn the_triangle_stays_bipolar_and_hits_both_ends() {
        let mut lfo = Lfo::new();
        let (mut lo, mut hi) = (f32::INFINITY, f32::NEG_INFINITY);
        for _ in 0..(FS as usize) {
            let v = lfo.process(2.0, 0.0, FS);
            assert!((-1.0..=1.0).contains(&v));
            lo = lo.min(v);
            hi = hi.max(v);
        }
        assert!(lo < -0.99 && hi > 0.99, "range {lo}..{hi}");
    }

    #[test]
    fn the_rate_is_what_was_asked_for() {
        let mut lfo = Lfo::new();
        let secs = 10.0;
        // Strictly from below: the triangle starts at exactly zero and rises, and that is not a
        // crossing.
        let mut prev = 0.0f32;
        let mut crossings = 0usize;
        for _ in 0..(FS * secs) as usize {
            let v = lfo.process(3.0, 0.0, FS);
            if prev < 0.0 && v >= 0.0 {
                crossings += 1;
            }
            prev = v;
        }
        assert_eq!(crossings, 30, "3 Hz over 10 s");
    }

    #[test]
    fn no_delay_means_full_depth_from_the_first_sample() {
        let mut lfo = Lfo::new();
        lfo.retrigger_delay();
        lfo.process(5.0, 0.0, FS);
        assert_eq!(lfo.fade(), 1.0);
    }

    #[test]
    fn the_delay_holds_the_depth_at_zero_then_fades_it_in() {
        let mut lfo = Lfo::new();
        lfo.retrigger_delay();
        let delay = 0.5;
        // During the delay: exactly nothing.
        for _ in 0..(FS * delay * 0.99) as usize {
            assert_eq!(lfo.process(5.0, delay, FS), 0.0);
        }
        // After it: a ramp, not a step.
        for _ in 0..(FS * 0.02) as usize {
            lfo.process(5.0, delay, FS);
        }
        let early = lfo.fade();
        assert!(early > 0.0 && early < 0.5, "fade after 20 ms: {early}");
        for _ in 0..(FS * 2.0) as usize {
            lfo.process(5.0, delay, FS);
        }
        assert_eq!(lfo.fade(), 1.0, "and it arrives at full depth");
    }

    #[test]
    fn retriggering_does_not_reset_the_phase() {
        // Free-running: the delay gate restarts, the triangle does not.
        let mut lfo = Lfo::new();
        for _ in 0..1_000 {
            lfo.process(2.0, 0.0, FS);
        }
        let before = lfo.phase;
        lfo.retrigger_delay();
        assert_eq!(lfo.phase, before);
    }
}
