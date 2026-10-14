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
//! The collection extracts shared DSP from the evidence of copies that proved identical, and this
//! crate's NOTES.md records which of its modules did: the generator is `mxm-rng`'s, and `flush`,
//! the saturators, the PolyBLEP residual and the ADSR are `mxm-dsp`'s.

pub mod chorus;
#[cfg(any(test, feature = "conformance"))]
pub mod conformance;
pub mod dco;
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
