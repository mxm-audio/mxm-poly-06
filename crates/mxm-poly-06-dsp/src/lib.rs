//! DSP for mxm-poly-06 — a six-voice polysynth with a built-in chorus.
//!
//! Deliberately free of any plugin-framework types: everything here takes plain values and a
//! sample rate, so the whole instrument is testable with `cargo test` and no host involved.
//!
//! ```text
//!                 ┌──────────── per voice, x6 ─────────────┐
//!   notes ─► ledger ─► DCO (saw | pulse | sub | noise) ─► VCF ─► VCA ─┐
//!                 │        ▲                              ▲       ▲   │ sum
//!                 └────────┼── one ADSR per voice ────────┴───────┘   │
//!                          │                                          ▼
//!   LFO (one, global, triangle + delay) ─► pitch · width · cutoff   HPF (4 positions)
//!                                                                     ▼
//!                                                          LEVEL (the patch's VCA)
//!                                                                     ▼
//!                                                    CHORUS off / I / II / I+II
//!                                                                     ▼
//!                                                          VOLUME (master) ─► L / R
//! ```
//!
//! [`poly::Synth`] is the whole instrument; [`voice::Voice`] is one of its six; [`chorus::Chorus`]
//! is the effect the machine shipped with, kept a module with a plain-values API so a standalone
//! version is later a move rather than a rewrite.
//!
//! `flush` and `Rng` below are **the third honest copy** of mxm-mono-01's
//! `crates/mxm-mono-01-dsp`'s, kept byte-identical on purpose: the collection extracts shared DSP
//! from the evidence of copies that proved identical, and this crate's NOTES.md records which of
//! its modules did.

pub mod chorus;
#[cfg(any(test, feature = "conformance"))]
pub mod conformance;
pub mod dco;
pub mod envelope;
pub mod filter;
pub mod hpf;
pub mod lfo;
pub mod onepole;
pub mod poly;
pub mod routing;
pub mod voice;

/// The lowest host rate the plugin activates at; a non-finite rate is refused with it.
///
/// `f32::clamp` panics when its lower bound is above its upper one or either is NaN, and each
/// voice's cutoff is clamped to `20 Hz ..= 0.45 × rate`, which crosses below 44.4 Hz. 1 kHz is far
/// clear of that, and no higher than the lowest rate clap-validator (1234.57 Hz) or the player's
/// robustness sweeps (1 kHz) ask for.
pub const MIN_SAMPLE_RATE: f32 = 1_000.0;

/// Flush a recursive state toward zero before it can become denormal.
///
/// Denormal arithmetic can cost orders of magnitude more than normal arithmetic,
/// which in a feedback filter shows up as a CPU spike exactly when a note decays
/// into silence. We do this in the DSP rather than relying on a framework FTZ
/// guard: the guard may be a no-op unless an opt-in feature is enabled, and
/// flushing here is also what keeps digital silence *exactly* zero.
///
/// `1e-20` is far above the f32 denormal threshold (~1.18e-38) and about -400 dB,
/// so nothing audible is lost.
#[inline(always)]
pub fn flush(x: f32) -> f32 {
    if x.abs() < 1e-20 { 0.0 } else { x }
}

/// Small xorshift PRNG. Allocation-free, deterministic, and seeded explicitly so
/// every noise source in the synth is bit-repeatable for a given seed — which is
/// what makes the filter's self-oscillation excitation testable, and what makes an
/// export render bit-identically to live playback.
#[derive(Debug, Clone)]
pub struct Rng {
    state: u32,
}

impl Rng {
    pub const fn new(seed: u32) -> Self {
        // A zero state is a fixed point for xorshift, so forbid it.
        Self {
            state: if seed == 0 { 0x9E37_79B9 } else { seed },
        }
    }

    /// Next uniform sample in `[-1, 1)`.
    #[inline]
    pub fn next_bipolar(&mut self) -> f32 {
        self.state ^= self.state << 13;
        self.state ^= self.state >> 17;
        self.state ^= self.state << 5;
        // Map the top 24 bits into [-1, 1) so the result is exactly representable.
        ((self.state >> 8) as f32 / 8_388_608.0) - 1.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flush_preserves_audible_values_and_kills_tiny_ones() {
        assert_eq!(flush(0.0), 0.0);
        assert_eq!(flush(1e-30), 0.0);
        assert_eq!(flush(-1e-30), 0.0);
        assert_eq!(flush(0.5), 0.5);
        assert_eq!(flush(-1e-6), -1e-6);
    }

    #[test]
    fn rng_is_deterministic_and_bounded() {
        let mut a = Rng::new(0x1234_5678);
        let mut b = Rng::new(0x1234_5678);
        for _ in 0..10_000 {
            let x = a.next_bipolar();
            assert_eq!(x, b.next_bipolar(), "same seed must give same sequence");
            assert!((-1.0..1.0).contains(&x), "out of range: {x}");
        }
    }

    #[test]
    fn rng_never_gets_stuck_at_zero_state() {
        let mut r = Rng::new(0);
        let first = r.next_bipolar();
        let second = r.next_bipolar();
        assert_ne!(first, second);
    }
}
