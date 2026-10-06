//! The 80017A: an IR3109 four-pole OTA ladder, with the JUNO's external circuit around it.
//!
//! **The core is `crates/mxm-mono-01-dsp/src/filter.rs`, copied whole**: TPT one-poles in the style
//! of Zavalishin's *The Art of VA Filter Design*, with a saturating resonance feedback path solved
//! per sample by Newton iteration. The reasons that design won — the nonlinearity inside the loop,
//! the boundedness argument, the two cheaper designs measured and rejected — are in that file and
//! in `research:filters/machines/ir3109-roland.md`, and are not restated. This crate does **not** depend
//! on `mxm-mono-01-dsp`: the copy is the third honest one the collection's extraction rule asks for.
//!
//! # What is the JUNO's, and not the SH-101's
//!
//! `research:filters/machines/ir3109-roland.md`'s thesis is that the linear filter is identical across
//! every IR3109 machine and they still do not sound alike, because the external circuit differs.
//! Three differences are modelled here, and each is **chosen from that research rather than
//! measured on a 106**:
//!
//! 1. **Input-side Q compensation.** `mxm-mono-01`'s doc says of its own ladder's droop: *do not fix
//!    it — adding compensation would make it a Juno.* This is the instrument that wants that. The
//!    Juno-6/60 re-injects input signal as resonance rises (research §5), so the gain sits **before**
//!    the saturator: the filter keeps its body as the peak grows *and* gets dirtier as it does,
//!    which is the interaction the research describes. Which side the 106 specifically compensates on
//!    is unverified; the Juno-6/60 configuration is the starting point and [`COMP_AMOUNT`] its value.
//! 2. **No diode clamp.** The SH-101's feedback path clips through diodes, and that flat-then-abrupt
//!    curve is its identity. The 80017A's resonance is a BA662 OTA with no clipping diodes, and an
//!    OTA's transconductance curve is a `tanh` — so the saturator here is the OTA's own.
//! 3. **Per-voice capacitor mismatch.** Roland used ceramic disc capacitors; §10 of the research
//!    measures ±2% spread moving the resonant peak by ±2 dB and the threshold by essentially
//!    nothing. Six voice cards, six spreads, **fixed per voice** so every instance, export and build
//!    sounds the same — and so POLY 1 and POLY 2 are audibly two things.
//!
//! **The resonance is mapped past the measured threshold**, as `mxm-mono-01`'s is: the slider's top
//! self-oscillates, which the hardware's does.

use crate::onepole::tan_approx;
use crate::{Rng, flush};
use std::f32::consts::PI;

/// Resonance maps to `k` in `0..=K_MAX`. The measured oscillation threshold sits
/// below this, so the top of the control is meaningfully past self-oscillation.
pub const K_MAX: f32 = 4.5;

/// Above this resonance the filter is excited so self-oscillation can start from
/// silence. See [`Ladder::process`].
pub const EXCITATION_THRESHOLD: f32 = 0.9;

/// Amplitude of that excitation. About -120 dB: inaudible against any real signal,
/// but enough to seed oscillation.
pub const EXCITATION_LEVEL: f32 = 1e-6;

/// How much of the ladder's `1/(1+k)` DC droop the external circuit puts back, as extra input gain.
///
/// **Chosen, not measured**: `ir3109-roland.md`'s `juno()` starting point. `1.0` would cancel the
/// droop exactly; less leaves some of the thinning the research says a Juno's resonance does to the
/// bass. A bench measurement of output level against resonance at DC would fit this.
pub const COMP_AMOUNT: f32 = 0.8;

/// Ceramic disc capacitor tolerance, as a fraction. Research §10: ±2% is plausible.
pub const CAPACITOR_SPREAD: f32 = 0.02;

/// Input saturation amount. Fixed rather than exposed as a parameter: this is the
/// filter's character, not a control.
const DRIVE: f32 = 1.0;

const CUTOFF_MIN_HZ: f32 = 20.0;

/// Newton steps used to solve the resonance feedback each sample. Three is enough
/// for the residual to fall below the noise floor across the whole parameter range;
/// the count is fixed so the cost is constant and the audio thread has no branch on
/// convergence.
const NEWTON_ITERATIONS: usize = 3;

/// Cutoff ceiling as a fraction of the sample rate. `tan` blows up at Nyquist, and
/// the Pade approximation is only valid short of it.
const NYQUIST_FRACTION: f32 = 0.45;

/// Output bound following from the boundedness argument: the saturator's magnitude never exceeds 1,
/// so the injected feedback is bounded by `k` and the ladder input by `1 + k`, plus headroom for
/// the resonant peak's transient overshoot. The input-side compensation sits *inside* the saturator
/// and so cannot move this.
pub const OUTPUT_BOUND: f32 = 1.0 + K_MAX + 2.5;

/// `tanh(x)` via the [7/6] Pade approximant, with the input clamped.
///
/// Accurate to better than 1e-4 absolute over the clamped range. The input clamp is load-bearing:
/// without it the rational form diverges for large `x`, which would break the boundedness argument.
/// The output clamp guards the last ulp so `|tanh_approx(x)| <= 1` is exactly true in `f32`.
#[inline]
pub fn tanh_approx(x: f32) -> f32 {
    let x = x.clamp(-4.0, 4.0);
    let x2 = x * x;
    let num = x * (135135.0 + x2 * (17325.0 + x2 * (378.0 + x2)));
    let den = 135135.0 + x2 * (62370.0 + x2 * (3150.0 + x2 * 28.0));
    (num / den).clamp(-1.0, 1.0)
}

/// A four-pole resonant lowpass ladder in the JUNO's arrangement.
#[derive(Debug, Clone)]
pub struct Ladder {
    /// One integrator state per pole.
    s: [f32; 4],
    /// Previous output, used to start the Newton solve.
    y_prev: f32,
    /// Per-stage multiplicative trim on the integrator gain — the capacitor tolerance.
    trim: [f32; 4],
    /// Deterministic excitation source for self-oscillation.
    rng: Rng,
    /// Its seed, so `reset` restores exactly the sequence a fresh instance has.
    seed: u32,
}

impl Default for Ladder {
    fn default() -> Self {
        Self::new(0)
    }
}

impl Ladder {
    /// A filter for voice `voice`, with that voice card's capacitor spread.
    ///
    /// The spread is derived from the voice number by the crate's own PRNG, so it is a constant of
    /// the build: the same on every instance, every platform and every render. Voice 0 has no
    /// spread at all, which is what the threshold tests measure against.
    pub fn new(voice: u32) -> Self {
        let mut trim = [1.0; 4];
        if voice != 0 {
            let mut rng = Rng::new(0x80017A ^ voice.wrapping_mul(0x9E37_79B9));
            for t in trim.iter_mut() {
                *t = 1.0 + CAPACITOR_SPREAD * rng.next_bipolar();
            }
        }
        let seed = 0x5EED_0600 ^ voice;
        Self {
            s: [0.0; 4],
            y_prev: 0.0,
            trim,
            rng: Rng::new(seed),
            seed,
        }
    }

    /// Clear all state. Leaves no tail from previous playback. The trims are a property of the
    /// voice card and survive.
    pub fn reset(&mut self) {
        self.s = [0.0; 4];
        self.y_prev = 0.0;
        // The voice's own seed, not a shared one: a reset that reseeded every card alike made
        // voices 1–5 differ from fresh instances and correlated their excitation. Found in review.
        self.rng = Rng::new(self.seed);
    }

    /// Process one sample.
    ///
    /// `resonance` is `0..=1`. Coefficients are recomputed every sample so that per-sample envelope
    /// and LFO modulation of the cutoff actually takes effect.
    #[inline]
    pub fn process(&mut self, input: f32, cutoff_hz: f32, resonance: f32, sample_rate: f32) -> f32 {
        let fc = cutoff_hz.clamp(CUTOFF_MIN_HZ, NYQUIST_FRACTION * sample_rate);
        let g = tan_approx((PI * fc / sample_rate) as f64) as f32;

        let resonance = resonance.clamp(0.0, 1.0);
        let k = K_MAX * resonance;

        // The JUNO's external circuit: more input as the resonance rises, **before** the OTA's
        // input stage saturates. That ordering is the whole of the difference from an output-side
        // makeup — the compensation drives the stages harder, so the filter gets grittier as it
        // gets peakier, and the saturator still bounds everything that follows.
        let comp_gain = 1.0 + COMP_AMOUNT * k;
        let mut u = tanh_approx(DRIVE * comp_gain * input);

        // A filter fed digital silence stays silent forever, so self-oscillation needs a seed.
        // Deliberate excitation, not denormal protection, and gated so "no input, low resonance"
        // stays exactly zero.
        if resonance > EXCITATION_THRESHOLD {
            u += EXCITATION_LEVEL * self.rng.next_bipolar();
        }

        // Each TPT one-pole gives y = G*x + (1-G)*s. With per-stage trims the four G differ, so the
        // cascade's constant term and its input gain are products over the stages:
        //   y4 = p*x + a,   p = G1 G2 G3 G4,   a = G4 G3 G2 s1' + G4 G3 s2' + G4 s3' + s4'
        // and with the feedback x = u - k*tanh(y4) that is a scalar nonlinear equation in y4.
        let mut big_g = [0.0f32; 4];
        for (gi, t) in big_g.iter_mut().zip(&self.trim) {
            let gt = g * t;
            *gi = gt / (1.0 + gt);
        }
        let s1 = (1.0 - big_g[0]) * self.s[0];
        let s2 = (1.0 - big_g[1]) * self.s[1];
        let s3 = (1.0 - big_g[2]) * self.s[2];
        let s4 = (1.0 - big_g[3]) * self.s[3];
        let a = big_g[3] * big_g[2] * big_g[1] * s1 + big_g[3] * big_g[2] * s2 + big_g[3] * s3 + s4;
        let p = big_g[0] * big_g[1] * big_g[2] * big_g[3];

        // F(y) = p*u + a - p*k*tanh(y) - y is strictly decreasing (F' <= -1 everywhere), so it has
        // exactly one root and the derivative can never be zero: a fixed number of Newton steps, no
        // convergence check, no fallback.
        let pu_a = p * u + a;
        let pk = p * k;
        let mut y_solved = self.y_prev;
        for _ in 0..NEWTON_ITERATIONS {
            let t = tanh_approx(y_solved);
            let f = pu_a - pk * t - y_solved;
            let df = -pk * (1.0 - t * t) - 1.0;
            y_solved -= f / df;
        }

        let x = u - k * tanh_approx(y_solved);

        let mut y = x;
        for (s, gi) in self.s.iter_mut().zip(&big_g) {
            let v = (y - *s) * gi;
            y = v + *s;
            *s = flush(y + v);
        }

        self.y_prev = flush(y);
        y
    }
}

/// Measure the resonance at which the filter starts to self-oscillate, by exciting it with an
/// impulse and comparing energy early and late in the decay.
pub fn measure_oscillation_threshold(cutoff_hz: f32, sample_rate: f32) -> f32 {
    let sustains = |resonance: f32| {
        let mut f = Ladder::new(0);
        f.process(1.0, cutoff_hz, resonance, sample_rate);
        let settle = (sample_rate * 0.20) as usize;
        for _ in 0..settle {
            f.process(0.0, cutoff_hz, resonance, sample_rate);
        }
        let window = (sample_rate * 0.05) as usize;
        let mut early = 0.0f32;
        for _ in 0..window {
            early = early.max(f.process(0.0, cutoff_hz, resonance, sample_rate).abs());
        }
        for _ in 0..(sample_rate as usize / 2) {
            f.process(0.0, cutoff_hz, resonance, sample_rate);
        }
        let mut late = 0.0f32;
        for _ in 0..window {
            late = late.max(f.process(0.0, cutoff_hz, resonance, sample_rate).abs());
        }
        late > early * 0.5 && late > 1e-9
    };

    let (mut lo, mut hi) = (0.0f32, 1.0f32);
    if sustains(lo) {
        return 0.0;
    }
    if !sustains(hi) {
        return f32::NAN;
    }
    for _ in 0..24 {
        let mid = 0.5 * (lo + hi);
        if sustains(mid) { hi = mid } else { lo = mid }
    }
    0.5 * (lo + hi)
}

#[cfg(test)]
mod tests {
    use super::*;
    use mxm_measure::spectrum::{Probe, transfer_gain};

    const RATES: [f32; 4] = [44_100.0, 48_000.0, 96_000.0, 192_000.0];

    /// Peak magnitude of the response to a sine at `freq`, after the transient has decayed,
    /// via `mxm-measure`'s shared transfer-gain probe.
    ///
    /// Amplitude is small so the saturators stay in their linear region and we measure the
    /// *filter*, not the drive.
    fn magnitude_at(f: &mut Ladder, cutoff: f32, resonance: f32, freq: f32, fs: f32) -> f32 {
        const AMP: f64 = 1e-3;
        let probe = Probe::new(f64::from(freq), AMP, 0.5, 8.0, f64::from(fs));
        transfer_gain(probe, f64::from(fs), |x| {
            f.process(x, cutoff, resonance, fs)
        })
        .expect("a probe at a real frequency has a gain") as f32
    }

    #[test]
    fn tanh_approx_is_bounded_and_monotonic() {
        let mut prev = f32::NEG_INFINITY;
        for i in -20_000..=20_000 {
            let x = i as f32 / 1000.0;
            let y = tanh_approx(x);
            assert!(y.abs() <= 1.0, "tanh_approx({x}) = {y} exceeds 1");
            assert!(y >= prev - 1e-6, "not monotonic at x={x}");
            prev = y;
        }
        let mut worst = 0.0f32;
        for i in 0..=400 {
            let x = i as f32 / 100.0;
            worst = worst.max((tanh_approx(x) - x.tanh()).abs());
        }
        assert!(worst < 1e-4, "worst deviation from tanh: {worst}");
    }

    #[test]
    fn four_pole_rolloff_is_minus_12_db_at_cutoff() {
        for fs in RATES {
            let mut f = Ladder::new(0);
            let m = magnitude_at(&mut f, 1_000.0, 0.0, 1_000.0, fs);
            let db = 20.0 * m.log10();
            assert!((db + 12.04).abs() < 1.0, "at {fs}: {db:.2} dB at cutoff");
        }
    }

    #[test]
    fn stopband_rolls_off_at_24_db_per_octave() {
        let fs = 48_000.0;
        let mut f = Ladder::new(0);
        let two_octaves = magnitude_at(&mut f, 1_000.0, 0.0, 4_000.0, fs);
        let db = 20.0 * two_octaves.log10();
        assert!(
            db < -44.0 && db > -52.0,
            "two octaves above cutoff: {db:.1} dB"
        );
    }

    /// **The JUNO's whole difference from the SH-101, as a number.** `mxm-mono-01`'s ladder loses
    /// `1/(1+k)` of its body as resonance rises — 13 dB at `k = 3.6`. The compensation keeps most
    /// of it: at small signal the passband should sit within a few dB of where it was.
    #[test]
    fn the_compensation_keeps_the_body_as_resonance_rises() {
        let fs = 48_000.0;
        let mut f = Ladder::new(0);
        let flat = magnitude_at(&mut f, 2_000.0, 0.0, 100.0, fs);
        let resonant = magnitude_at(&mut f, 2_000.0, 0.8, 100.0, fs);
        let lost_db = 20.0 * (resonant / flat).log10();
        // Uncompensated the loss would be 20*log10(1/(1+3.6)) = -13.3 dB.
        assert!(
            lost_db > -4.0 && lost_db < 1.0,
            "passband moved {lost_db:.1} dB at resonance 0.8; the compensation is not doing its job"
        );
    }

    #[test]
    fn oscillation_threshold_is_measured_and_stable_across_sample_rates() {
        let mut thresholds = Vec::new();
        for fs in RATES {
            let r = measure_oscillation_threshold(1_000.0, fs);
            assert!(r.is_finite(), "at {fs}: never oscillated");
            thresholds.push(r);
        }
        let k: Vec<f32> = thresholds.iter().map(|r| r * K_MAX).collect();
        for (fs, k) in RATES.iter().zip(&k) {
            assert!(
                (k - 4.0).abs() < 0.1,
                "at {fs}: threshold k = {k:.3}, expected about 4.0"
            );
        }
    }

    #[test]
    fn a_voice_cards_spread_moves_the_peak_and_not_the_threshold() {
        // Research §10: ±2% capacitor spread changes *how much* resonance, not *where* it sings.
        let fs = 48_000.0;
        let reference = measure_oscillation_threshold(1_000.0, fs);
        for voice in 1..6u32 {
            let mut f = Ladder::new(voice);
            // The threshold of a trimmed card, by the same bisection.
            let sustains = |f: &mut Ladder, resonance: f32| {
                f.reset();
                f.process(1.0, 1_000.0, resonance, fs);
                for _ in 0..(fs * 0.2) as usize {
                    f.process(0.0, 1_000.0, resonance, fs);
                }
                let mut early = 0.0f32;
                for _ in 0..(fs * 0.05) as usize {
                    early = early.max(f.process(0.0, 1_000.0, resonance, fs).abs());
                }
                for _ in 0..(fs as usize / 2) {
                    f.process(0.0, 1_000.0, resonance, fs);
                }
                let mut late = 0.0f32;
                for _ in 0..(fs * 0.05) as usize {
                    late = late.max(f.process(0.0, 1_000.0, resonance, fs).abs());
                }
                late > early * 0.5 && late > 1e-9
            };
            let (mut lo, mut hi) = (0.0f32, 1.0f32);
            for _ in 0..16 {
                let mid = 0.5 * (lo + hi);
                if sustains(&mut f, mid) {
                    hi = mid
                } else {
                    lo = mid
                }
            }
            let threshold = 0.5 * (lo + hi);
            assert!(
                (threshold - reference).abs() < 0.02,
                "voice {voice}: threshold {threshold:.3} against the untrimmed {reference:.3}"
            );
            assert_ne!(f.trim, [1.0; 4], "voice {voice} has no spread at all");
        }
    }

    #[test]
    fn output_stays_bounded_under_overdrive_past_the_threshold() {
        for fs in RATES {
            let mut f = Ladder::new(2);
            let mut peak = 0.0f32;
            for n in 0..(fs as usize) {
                let t = n as f32 / fs;
                let x = 10.0 * (2.0 * PI * 220.0 * t).sin();
                let y = f.process(x, 3_000.0, 1.0, fs);
                assert!(y.is_finite(), "non-finite at {fs}");
                peak = peak.max(y.abs());
            }
            assert!(
                peak <= OUTPUT_BOUND,
                "peak {peak} exceeds the stated bound {OUTPUT_BOUND}"
            );
        }
    }

    #[test]
    fn no_nan_or_inf_across_the_sweep() {
        for fs in RATES {
            for cutoff in [10.0f32, 100.0, 1_000.0, 10_000.0, 30_000.0] {
                for resonance in [0.0f32, 0.5, 0.9, 1.0, 1.5] {
                    let mut f = Ladder::new(1);
                    for n in 0..2_000 {
                        let x = if n % 100 == 0 { 1.0 } else { 0.0 };
                        let y = f.process(x, cutoff, resonance, fs);
                        assert!(
                            y.is_finite(),
                            "{fs} Hz, cutoff {cutoff}, resonance {resonance}"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn silence_in_gives_exactly_zero_out_after_decay() {
        for fs in RATES {
            let mut f = Ladder::new(4);
            f.process(1.0, 1_000.0, 0.5, fs);
            for _ in 0..(fs as usize) {
                f.process(0.0, 1_000.0, 0.5, fs);
            }
            assert_eq!(f.process(0.0, 1_000.0, 0.5, fs), 0.0, "at {fs}");
        }
    }

    /// A reset card renders exactly what a fresh one does — per card, excitation included.
    #[test]
    fn reset_restores_a_fresh_instances_sequence_for_every_card() {
        for voice in 0..6u32 {
            let mut used = Ladder::new(voice);
            for _ in 0..2_000 {
                used.process(0.3, 900.0, 1.0, 48_000.0);
            }
            used.reset();
            let mut fresh = Ladder::new(voice);
            for _ in 0..4_000 {
                assert_eq!(
                    used.process(0.0, 900.0, 1.0, 48_000.0),
                    fresh.process(0.0, 900.0, 1.0, 48_000.0),
                    "card {voice} after reset differs from a fresh one"
                );
            }
        }
    }

    #[test]
    fn reset_leaves_no_tail() {
        let mut f = Ladder::new(0);
        for _ in 0..1_000 {
            f.process(1.0, 500.0, 0.9, 48_000.0);
        }
        f.reset();
        assert_eq!(f.process(0.0, 500.0, 0.5, 48_000.0), 0.0);
    }
}
