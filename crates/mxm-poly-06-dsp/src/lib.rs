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
//! `flush` below is **the third honest copy** of mxm-mono-01's
//! `crates/mxm-mono-01-dsp`'s, kept byte-identical on purpose (the noise generator, once a copy too,
//! is `mxm-rng`'s `Xorshift32`): the collection extracts shared DSP
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
}
