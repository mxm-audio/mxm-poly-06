//! Analog-style ADSR envelope.
//!
//! **A copy of mxm-mono-01's `crates/mxm-mono-01-dsp/src/envelope.rs`, deliberately whole**,
//! because the JUNO's envelope is the same kind of thing: one-pole exponential segments, an attack
//! that aims past its target and switches when it crosses 1.0 — the snap a real ADSR has and a
//! linear ramp lacks. It is the third honest copy the collection's extraction rule asks for, and
//! this crate's NOTES.md records it as one of the candidates that proved identical.
//!
//! **One envelope per voice, shared by the filter and the amplifier.** That is the machine's
//! defining constraint (`research:instruments/juno-106.md` §3.6): you cannot have a slow filter sweep
//! under a percussive amplitude, and an implementation that quietly adds a second envelope is not a
//! JUNO-106.
//!
//! The A/D/R times are **not** smoothed. They set segment coefficients rather than being signals in
//! their own right, and smoothing a time constant makes the state machine's timing impossible to
//! reason about. Only sustain is smoothed, and that happens at the parameter layer.
//!
//! **The segment curves are not the hardware's**, or rather nobody knows whether they are: the
//! research records the 106's times and not its shapes, and names the curves the first thing worth
//! measuring if a unit appears. Exponential is the analogue default and the honest guess.

use crate::flush;

/// Level below which the envelope is considered finished and snaps to exactly zero.
///
/// An exponential never truly reaches zero, so without a threshold there is no
/// point at which the note is over — which would make the plugin's tail time
/// uncomputable and leave the voice running forever.
pub const ZERO_THRESHOLD: f32 = 1e-4; // -80 dB

/// How far past 1.0 the attack segment aims. The overshoot is never reached; it is
/// what bends the attack into an analog-looking curve instead of a saturating one.
const ATTACK_OVERSHOOT: f32 = 0.2;

/// `ln((1 + overshoot) / overshoot)`, the number of time constants needed for the
/// attack to cross 1.0. Precomputed so the attack time parameter means "time to
/// reach full level", not "time constant".
const ATTACK_TAUS: f32 = 1.791_759_5; // ln(1.2 / 0.2)

/// Time constants for a decay or release to get within 1% of its target.
const DECAY_TAUS: f32 = 4.605_170_2; // ln(100)

const MIN_TIME_S: f32 = 0.001;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stage {
    Idle,
    Attack,
    Decay,
    Sustain,
    Release,
}

#[derive(Debug, Clone)]
pub struct Adsr {
    stage: Stage,
    level: f32,
    sample_rate: f32,

    // Cached coefficients, recomputed only when the corresponding time changes.
    // `exp` is far too expensive to call per sample, and these change rarely.
    attack_time: f32,
    decay_time: f32,
    release_time: f32,
    attack_coef: f32,
    decay_coef: f32,
    release_coef: f32,
}

impl Default for Adsr {
    fn default() -> Self {
        Self::new()
    }
}

impl Adsr {
    pub fn new() -> Self {
        Self {
            stage: Stage::Idle,
            level: 0.0,
            sample_rate: 48_000.0,
            attack_time: -1.0,
            decay_time: -1.0,
            release_time: -1.0,
            attack_coef: 0.0,
            decay_coef: 0.0,
            release_coef: 0.0,
        }
    }

    pub fn set_sample_rate(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate;
        // Force the coefficients to be recomputed against the new rate.
        self.attack_time = -1.0;
        self.decay_time = -1.0;
        self.release_time = -1.0;
    }

    pub fn reset(&mut self) {
        self.stage = Stage::Idle;
        self.level = 0.0;
    }

    pub fn stage(&self) -> Stage {
        self.stage
    }

    pub fn level(&self) -> f32 {
        self.level
    }

    pub fn is_active(&self) -> bool {
        self.stage != Stage::Idle
    }

    /// Start a note.
    ///
    /// The level is *not* zeroed: restarting from wherever the envelope currently
    /// sits avoids a click when a voice is stolen or retriggered during a release.
    pub fn trigger(&mut self) {
        self.stage = Stage::Attack;
    }

    /// Begin the release segment.
    pub fn release(&mut self) {
        if self.stage != Stage::Idle {
            self.stage = Stage::Release;
        }
    }

    /// Cut the envelope immediately, as an all-sound-off or choke requires.
    pub fn silence(&mut self) {
        self.stage = Stage::Idle;
        self.level = 0.0;
    }

    fn coef(time_s: f32, taus: f32, sample_rate: f32) -> f32 {
        let t = time_s.max(MIN_TIME_S);
        (-taus / (t * sample_rate)).exp()
    }

    fn update_coefficients(&mut self, attack: f32, decay: f32, release: f32) {
        if attack != self.attack_time {
            self.attack_time = attack;
            self.attack_coef = Self::coef(attack, ATTACK_TAUS, self.sample_rate);
        }
        if decay != self.decay_time {
            self.decay_time = decay;
            self.decay_coef = Self::coef(decay, DECAY_TAUS, self.sample_rate);
        }
        if release != self.release_time {
            self.release_time = release;
            self.release_coef = Self::coef(release, DECAY_TAUS, self.sample_rate);
        }
    }

    /// Advance one sample. Times are in seconds, `sustain` in `0..=1`.
    #[inline]
    pub fn process(&mut self, attack: f32, decay: f32, sustain: f32, release: f32) -> f32 {
        self.update_coefficients(attack, decay, release);
        let sustain = sustain.clamp(0.0, 1.0);

        match self.stage {
            Stage::Idle => return 0.0,
            Stage::Attack => {
                let target = 1.0 + ATTACK_OVERSHOOT;
                self.level = target + (self.level - target) * self.attack_coef;
                if self.level >= 1.0 {
                    self.level = 1.0;
                    self.stage = Stage::Decay;
                }
            }
            Stage::Decay => {
                let before = self.level;
                self.level = sustain + (self.level - sustain) * self.decay_coef;
                // **Or the step has stopped moving it.** With a long decay and a high sustain the
                // per-sample step falls under half an ulp of the level — at 0.8 and 0.4 s that is
                // 3e-8 against 6e-8 — and in `f32` the level then sits 1.2e-4 above the threshold
                // for ever, never reaching `Sustain`. Inaudible, and it left the stage wrong.
                // Measured here; the `mxm-mono-01` copy this was made from has the same stall.
                if (self.level - sustain).abs() <= ZERO_THRESHOLD || self.level == before {
                    self.level = sustain;
                    self.stage = Stage::Sustain;
                }
            }
            Stage::Sustain => {
                // Track sustain changes smoothly rather than stepping to them.
                self.level = sustain + (self.level - sustain) * self.decay_coef;
            }
            Stage::Release => {
                self.level *= self.release_coef;
                if self.level <= ZERO_THRESHOLD {
                    self.level = 0.0;
                    self.stage = Stage::Idle;
                }
            }
        }

        self.level = flush(self.level);
        self.level
    }

    /// Samples remaining before the envelope reaches [`ZERO_THRESHOLD`].
    ///
    /// This is what makes an honest `ProcessStatus::Tail` possible: without a
    /// defined zero point an exponential release has no end, and the plugin would
    /// have to either lie about its tail or never report being finished.
    ///
    /// Returns 0 when idle. While held (not yet releasing) the answer depends on
    /// when the note is let go, so this reports the full release from the current
    /// level, which is the correct conservative estimate.
    pub fn tail_samples(&self, release: f32) -> u32 {
        if self.stage == Stage::Idle || self.level <= ZERO_THRESHOLD {
            return 0;
        }
        let coef = Self::coef(release, DECAY_TAUS, self.sample_rate);
        if coef <= 0.0 || coef >= 1.0 {
            return 0;
        }
        let n = (ZERO_THRESHOLD / self.level).ln() / coef.ln();
        n.max(0.0).ceil() as u32
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FS: f32 = 48_000.0;

    fn run(env: &mut Adsr, secs: f32, a: f32, d: f32, s: f32, r: f32) -> f32 {
        let n = (FS * secs) as usize;
        let mut last = 0.0;
        for _ in 0..n {
            last = env.process(a, d, s, r);
        }
        last
    }

    #[test]
    fn idle_envelope_is_exactly_zero() {
        let mut env = Adsr::new();
        env.set_sample_rate(FS);
        for _ in 0..1_000 {
            assert_eq!(env.process(0.01, 0.1, 0.7, 0.1), 0.0);
        }
    }

    #[test]
    fn attack_reaches_full_level_in_about_the_attack_time() {
        for attack in [0.005f32, 0.05, 0.5] {
            let mut env = Adsr::new();
            env.set_sample_rate(FS);
            env.trigger();
            let mut samples = 0usize;
            while env.stage() == Stage::Attack && samples < (FS * 5.0) as usize {
                env.process(attack, 0.3, 0.7, 0.2);
                samples += 1;
            }
            let measured = samples as f32 / FS;
            let err = (measured - attack).abs() / attack;
            assert!(err < 0.1, "attack {attack}s measured {measured}s");
        }
    }

    #[test]
    fn release_returns_to_exactly_zero_and_goes_idle() {
        let mut env = Adsr::new();
        env.set_sample_rate(FS);
        env.trigger();
        run(&mut env, 1.0, 0.005, 0.1, 0.7, 0.1);
        env.release();
        let level = run(&mut env, 2.0, 0.005, 0.1, 0.7, 0.1);
        assert_eq!(level, 0.0, "release did not reach exactly zero");
        assert_eq!(env.stage(), Stage::Idle);
    }

    #[test]
    fn tail_estimate_matches_the_actual_release_length() {
        for release in [0.05f32, 0.2, 1.0] {
            let mut env = Adsr::new();
            env.set_sample_rate(FS);
            env.trigger();
            run(&mut env, 1.0, 0.005, 0.1, 0.8, release);
            env.release();

            let predicted = env.tail_samples(release);
            let mut actual = 0u32;
            while env.is_active() && actual < (FS * 10.0) as u32 {
                env.process(0.005, 0.1, 0.8, release);
                actual += 1;
            }
            let err = (predicted as f32 - actual as f32).abs() / actual as f32;
            assert!(
                err < 0.05,
                "release {release}s: predicted {predicted}, actual {actual}"
            );
        }
    }

    /// The property a voice steal depends on: restarting from the current level, not from zero.
    #[test]
    fn retriggering_during_release_does_not_jump_to_zero() {
        let mut env = Adsr::new();
        env.set_sample_rate(FS);
        env.trigger();
        run(&mut env, 0.5, 0.005, 0.2, 0.8, 0.5);
        env.release();
        run(&mut env, 0.05, 0.005, 0.2, 0.8, 0.5);
        let before = env.level();
        assert!(before > 0.1, "test setup: expected a partly released level");

        env.trigger();
        let after = env.process(0.005, 0.2, 0.8, 0.5);
        assert!(
            (after - before).abs() < 0.05,
            "retrigger stepped from {before} to {after}"
        );
    }

    #[test]
    fn stays_finite_at_every_sample_rate_and_extreme_time() {
        for fs in [44_100.0f32, 48_000.0, 96_000.0, 192_000.0] {
            let mut env = Adsr::new();
            env.set_sample_rate(fs);
            for &t in &[0.0f32, 0.0001, 1.0, 12.0, 1e6] {
                env.trigger();
                for _ in 0..10_000 {
                    let v = env.process(t, t, 0.5, t);
                    assert!(v.is_finite() && (0.0..=1.0).contains(&v), "level {v}");
                }
                env.release();
                for _ in 0..10_000 {
                    let v = env.process(t, t, 0.5, t);
                    assert!(v.is_finite() && (0.0..=1.0).contains(&v), "level {v}");
                }
            }
        }
    }
}
