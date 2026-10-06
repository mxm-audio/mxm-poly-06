//! The DCO: an analogue ramp reset by a digital clock, and everything derived from it.
//!
//! `research:instruments/juno-106.md` §4 is the reference. What makes this oscillator *this* one:
//!
//! - **It does not drift, at all.** Pitch comes from a crystal, so there is no random detune here
//!   and there must never be — six voices playing a chord are mathematically in tune with each
//!   other, and that is why the chorus exists. A model that adds "analogue drift" for warmth is
//!   modelling a property the reference never had (`research:oscillators/05-machines.md` §5.3).
//! - **Saw, pulse and sub are one waveform with harmonics added.** The pulse is the ramp through a
//!   comparator; the sub is the reset clock through a divide-by-two flip-flop. Selecting all three
//!   produces no beating whatsoever, by construction, and [`tests::saw_pulse_and_sub_do_not_beat`]
//!   holds that.
//! - **The ramp rises.** The 106 resets with an NPN on the rising clock edge, where the Juno-6/60
//!   used a PNP on the falling one and produced a falling saw. Audibly identical in isolation;
//!   stated so nobody flips it while tidying.
//!
//! # What is not modelled, and why
//!
//! **The finite reset.** The discharge takes about 5.3 µs (§4.1). At 44.1 kHz that is 0.23 of a
//! sample, and its effect on the spectrum is a `sinc(f · 5.3 µs)` rolloff: −0.16 dB at 20 kHz, and
//! less than −0.02 dB below 8 kHz. Derived, not measured — but derived to be two orders of magnitude
//! below the aliasing floor the band limiting leaves, so modelling it would model nothing audible.
//!
//! **The residual amplitude variation.** The CPU compensates the ramp's amplitude against pitch and
//! about 1 V of 12 remains across the keyboard (§4.2). Its *shape* is not documented anywhere the
//! research found, and an unmarked guess at a curve is worse than a gap. Left out and recorded.
//!
//! # Band limiting
//!
//! PolyBLEP residuals on every discontinuity, exactly as `crates/mxm-mono-01-dsp` does it, and for
//! the same reason (`docs/oscillators/07-rust-recipes.md`): cheap, no tables, and quiet enough for an
//! instrument whose character is in the filter and the chorus. The sub gets its own residual at the
//! wraps where the flip-flop toggles — a divider without band limiting aliases exactly like a
//! square wave without one.

use crate::{Rng, flush};

/// Lowest frequency the oscillator will produce.
const FREQ_MIN_HZ: f32 = 8.0;

/// Highest frequency, as a fraction of the sample rate. MIDI 127 at 4' with bend and vibrato on
/// top can otherwise exceed Nyquist, and PolyBLEP does not make an invalid increment safe.
const NYQUIST_FRACTION: f32 = 0.45;

/// The DCO's range switch: three octaves, in organ footage.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Range {
    /// One octave down.
    Sixteen,
    /// Concert pitch.
    #[default]
    Eight,
    /// One octave up.
    Four,
}

impl Range {
    /// The semitone offset the switch applies.
    #[inline]
    pub const fn semitones(self) -> f32 {
        match self {
            Range::Sixteen => -12.0,
            Range::Eight => 0.0,
            Range::Four => 12.0,
        }
    }
}

/// PolyBLEP residual for a step discontinuity of height +2 at the phase wrap.
///
/// `t` is the phase in `0..1` and `dt` the per-sample phase increment. The residual corrects the two
/// samples either side of a discontinuity, which removes most of the aliasing energy for the cost of
/// a few operations. Subtract it for a falling step; add it for a rising one.
#[inline]
fn poly_blep(t: f32, dt: f32) -> f32 {
    if t < dt {
        let x = t / dt;
        x + x - x * x - 1.0
    } else if t > 1.0 - dt {
        let x = (t - 1.0) / dt;
        x * x + x + x + 1.0
    } else {
        0.0
    }
}

/// Keep the pulse's two PolyBLEP corrections from overlapping.
///
/// Each correction spans `dt` either side of an edge, so if the pulse is narrower than `2*dt` the
/// two would overlap and produce nonsense. At extreme frequencies where no valid width exists, fall
/// back to a square.
#[inline]
pub fn clamp_pulse_width(width: f32, dt: f32) -> f32 {
    let lo = 0.05f32.max(2.0 * dt);
    let hi = 0.95f32.min(1.0 - 2.0 * dt);
    if lo > hi { 0.5 } else { width.clamp(lo, hi) }
}

/// What the four sources are mixed at, each `0..=1`.
///
/// Saw and pulse are **switches** on the hardware, so a voice hands them as 0 or 1; sub and noise are
/// the two level sliders. One struct rather than four arguments, because a voice reads them as one
/// thing: the mixer.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Mix {
    pub saw: f32,
    pub pulse: f32,
    pub sub: f32,
    pub noise: f32,
}

/// One sample of each thing the DCO makes, before the mixer.
///
/// The instrument's own audio, made routable so a target can be modulated by the oscillator or by
/// noise — `plans/plan-modulation-routing.md` §2.3's *expose what is already generating*, which is
/// what makes FM reachable. They were already computed separately and mixed at the end.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Parts {
    /// The sawtooth.
    pub saw: f32,
    /// The pulse, at the current width.
    pub pulse: f32,
    /// The sub-oscillator.
    pub sub: f32,
    /// The noise sample the mixer used.
    pub noise: f32,
}

impl Parts {
    /// Nothing produced yet — what a fresh or reset DCO has published.
    #[must_use]
    pub const fn silent() -> Self {
        Self {
            saw: 0.0,
            pulse: 0.0,
            sub: 0.0,
            noise: 0.0,
        }
    }
}

/// One voice's DCO.
#[derive(Debug, Clone)]
pub struct Dco {
    /// The ramp. Free-running: never reset on note-on, because an analogue integrator is not, and
    /// resetting mid-legato is a click.
    phase: f32,
    inc: f32,
    /// The flip-flop. Toggles at every reset, so it is a square exactly one octave down and in fixed
    /// phase with the ramp.
    sub_state: f32,
    noise: Rng,
    /// What each source produced on the last call to [`Dco::process`]. State a backward route
    /// reads, so `reset` clears it with everything else.
    last: Parts,
}

impl Default for Dco {
    fn default() -> Self {
        Self::new(0)
    }
}

impl Dco {
    /// `voice` seeds the noise source, so six voices do not hiss in unison.
    pub const fn new(voice: u32) -> Self {
        Self {
            phase: 0.0,
            inc: 0.0,
            sub_state: -1.0,
            noise: Rng::new(0x0517_0600 ^ voice.wrapping_mul(0x9E37_79B9)),
            last: Parts::silent(),
        }
    }

    /// Zero the phase and reseed. Called from `reset()`, never from note-on.
    pub fn reset(&mut self, voice: u32) {
        *self = Self::new(voice);
    }

    /// Render one sample of the mixed oscillator section.
    ///
    /// The `0.5` headroom keeps four sources at full level from slamming the filter's input
    /// saturator — the same figure `mxm-mono-01` settled on.
    #[inline]
    pub fn process(&mut self, freq_hz: f32, pulse_width: f32, mix: &Mix, sample_rate: f32) -> f32 {
        let hz = freq_hz.clamp(FREQ_MIN_HZ, NYQUIST_FRACTION * sample_rate);
        self.inc = hz / sample_rate;
        let (t, dt) = (self.phase, self.inc);

        // The ramp: rising, and it falls by 2 at the wrap, so the residual is subtracted.
        let saw = 2.0 * t - 1.0 - poly_blep(t, dt);

        // The comparator: high while the ramp is below the reference.
        let w = clamp_pulse_width(pulse_width, dt);
        let mut pulse = if t < w { 1.0 } else { -1.0 };
        pulse += poly_blep(t, dt);
        let second = {
            let x = t - w;
            if x < 0.0 { x + 1.0 } else { x }
        };
        pulse -= poly_blep(second, dt);

        // The flip-flop: its edge is at the wrap, and its direction is whatever the state is about
        // to do. Before the wrap the correction leans against the current state; after it, the
        // state has already toggled, so the correction leans with it.
        let s = self.sub_state;
        let sub = s + if t < dt { s } else { -s } * poly_blep(t, dt);

        let noise = self.noise.next_bipolar();

        self.phase += self.inc;
        if self.phase >= 1.0 {
            self.phase -= 1.0;
            self.sub_state = -self.sub_state;
        }

        self.last = Parts {
            saw,
            pulse,
            sub,
            noise,
        };
        flush((saw * mix.saw + pulse * mix.pulse + sub * mix.sub + noise * mix.noise) * 0.5)
    }

    /// What each source produced on the **last** call to [`Dco::process`].
    ///
    /// **Last, not this**: they exist only after the DCO has run, so a route from one of them into
    /// pitch or width is a backward route and one sample late. `routing`'s declared source order is
    /// where that is written down.
    #[must_use]
    pub fn parts(&self) -> Parts {
        self.last
    }

    /// The ramp's phase, for tests that want to know the sub is locked to it.
    pub fn phase(&self) -> f32 {
        self.phase
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mxm_measure::{convert, pitch};

    const RATES: [f32; 4] = [44_100.0, 48_000.0, 96_000.0, 192_000.0];

    fn only(saw: f32, pulse: f32, sub: f32, noise: f32) -> Mix {
        Mix {
            saw,
            pulse,
            sub,
            noise,
        }
    }

    /// Renders the DCO and measures its frequency with `mxm-measure`'s interpolated ruler.
    ///
    /// **This used to count whole zero crossings over a fixed window**, which quantises to ±1 cycle
    /// — about ±9 cents at 55 Hz over two seconds, far coarser than the one cent the assertions
    /// below claimed. `mxm-mono-00-dsp` had already found and fixed that; the fix could not travel
    /// until the ruler was shared.
    fn measure_freq(mix: Mix, freq: f32, fs: f32, secs: f32) -> f64 {
        let mut dco = Dco::new(0);
        let n = (fs * secs) as usize;
        let rendered: Vec<f32> = (0..n).map(|_| dco.process(freq, 0.5, &mix, fs)).collect();
        pitch::frequency_by_crossings(&rendered, f64::from(fs)).expect("the DCO sounds")
    }

    #[test]
    fn saw_frequency_is_accurate_to_a_tenth_of_a_cent() {
        // **Re-derived when the ruler was corrected, not rescaled.** This asserted one cent while
        // being measured by a crossing *count*, which quantises to ±1 cycle — ±9 cents at 55 Hz over
        // two seconds — so it could not fail for any tuning error smaller than its own ruler's.
        // Worst case with the interpolated ruler over these twenty combinations is **0.009 cents**;
        // a tenth of a cent leaves an order of magnitude of headroom and is ninety times tighter.
        for fs in RATES {
            for freq in [55.0f32, 110.0, 440.0, 1_000.0, 4_000.0] {
                let measured = measure_freq(only(1.0, 0.0, 0.0, 0.0), freq, fs, 2.0);
                let cents = convert::cents_error(measured, f64::from(freq));
                assert!(
                    cents.abs() < 0.1,
                    "{freq} Hz at {fs}: measured {measured}, off by {cents:.4} cents"
                );
            }
        }
    }

    #[test]
    fn the_sub_is_exactly_one_octave_below() {
        for fs in RATES {
            let measured = measure_freq(only(0.0, 0.0, 1.0, 0.0), 440.0, fs, 2.0);
            let cents = convert::cents_error(measured, 220.0);
            // Worst measured 0.007 cents, so the same tenth-of-a-cent bound applies here.
            assert!(
                cents.abs() < 0.1,
                "at {fs}: sub measured {measured} Hz ({cents:.4} cents from 220)"
            );
        }
    }

    /// **The structural fact.** Saw + pulse + sub is one periodic waveform, so its amplitude over
    /// successive fundamental periods is constant — there is nothing to beat against. Three
    /// independent oscillators at the same nominal pitch would show slow amplitude modulation as
    /// their phases wandered.
    #[test]
    fn saw_pulse_and_sub_do_not_beat() {
        let fs = 48_000.0;
        // Exactly periodic: 2048 samples per sub period so the window is a whole number of them.
        let freq = fs / 1024.0;
        let mut dco = Dco::new(0);
        let mix = only(1.0, 1.0, 1.0, 0.0);
        for _ in 0..8192 {
            dco.process(freq, 0.5, &mix, fs);
        }
        // Peak per sub period, across many periods.
        let mut peaks = Vec::new();
        for _ in 0..64 {
            let mut peak = 0.0f32;
            for _ in 0..2048 {
                peak = peak.max(dco.process(freq, 0.5, &mix, fs).abs());
            }
            peaks.push(peak);
        }
        let (lo, hi) = peaks.iter().fold((f32::INFINITY, 0.0f32), |(lo, hi), &p| {
            (lo.min(p), hi.max(p))
        });
        assert!(
            (hi - lo) / hi < 1e-3,
            "the summed waveform's peak wandered from {lo} to {hi}: something is beating"
        );
    }

    #[test]
    fn the_sub_toggles_exactly_at_the_ramps_reset() {
        let fs = 48_000.0;
        let mut dco = Dco::new(0);
        let mix = only(0.0, 0.0, 1.0, 0.0);
        let mut prev_phase = dco.phase();
        let mut prev_state = dco.sub_state;
        for _ in 0..20_000 {
            dco.process(440.0, 0.5, &mix, fs);
            let wrapped = dco.phase() < prev_phase;
            let toggled = dco.sub_state != prev_state;
            assert_eq!(wrapped, toggled, "the flip-flop and the reset disagreed");
            prev_phase = dco.phase();
            prev_state = dco.sub_state;
        }
    }

    #[test]
    fn output_is_bounded_and_finite_at_extreme_pitch() {
        for fs in RATES {
            let mut dco = Dco::new(3);
            for freq in [0.0f32, 1.0, 20_000.0, 40_000.0, 1e9, -100.0] {
                for width in [0.0f32, 0.05, 0.5, 0.95, 1.0] {
                    for _ in 0..2_000 {
                        let y = dco.process(freq, width, &only(1.0, 1.0, 1.0, 1.0), fs);
                        assert!(
                            y.is_finite(),
                            "non-finite at {freq} Hz, width {width}, {fs}"
                        );
                        assert!(y.abs() <= 2.0, "unbounded: {y}");
                    }
                }
            }
        }
    }

    #[test]
    fn silence_when_every_source_is_off() {
        for fs in RATES {
            let mut dco = Dco::new(0);
            for _ in 0..10_000 {
                assert_eq!(dco.process(440.0, 0.5, &only(0.0, 0.0, 0.0, 0.0), fs), 0.0);
            }
        }
    }

    #[test]
    fn changing_frequency_never_resets_the_phase() {
        // What keeps a legato pitch change click-free: the ramp retunes without discontinuity.
        let fs = 48_000.0;
        let mut dco = Dco::new(0);
        for _ in 0..100 {
            dco.process(220.0, 0.5, &only(1.0, 0.0, 0.0, 0.0), fs);
        }
        let before = dco.phase();
        dco.process(880.0, 0.5, &only(1.0, 0.0, 0.0, 0.0), fs);
        let expected = (before + 880.0 / fs) % 1.0;
        assert!(
            (dco.phase() - expected).abs() < 1e-6,
            "phase {} did not advance by exactly the new increment (expected {expected})",
            dco.phase()
        );
    }

    #[test]
    fn two_voices_do_not_share_a_noise_source() {
        let (mut a, mut b) = (Dco::new(0), Dco::new(1));
        let mix = only(0.0, 0.0, 0.0, 1.0);
        let same = (0..1_000)
            .filter(|_| {
                a.process(440.0, 0.5, &mix, 48_000.0) == b.process(440.0, 0.5, &mix, 48_000.0)
            })
            .count();
        assert!(
            same < 10,
            "two voices' noise agreed on {same} of 1000 samples"
        );
    }

    /// Alias-to-signal ratio of an exactly periodic buffer, in dB. See `mxm-mono-01`'s oscillator
    /// tests for the argument: `periods` odd and `N` a power of two separate wanted harmonics from
    /// aliases exactly, with no window and no peak picking.
    fn alias_to_signal_db(x: &[f64], periods: usize) -> f64 {
        let n = x.len();
        let half = n / 2;
        let (mut wanted, mut alias) = (0.0f64, 0.0f64);
        for bin in 1..half {
            let (mut re, mut im) = (0.0f64, 0.0f64);
            for (i, &v) in x.iter().enumerate() {
                let ang = -2.0 * std::f64::consts::PI * bin as f64 * i as f64 / n as f64;
                re += v * ang.cos();
                im += v * ang.sin();
            }
            let power = re * re + im * im;
            if bin % periods == 0 {
                wanted += power;
            } else {
                alias += power;
            }
        }
        10.0 * (alias / wanted.max(1e-30)).max(1e-30).log10()
    }

    /// Each source's aliasing, measured, with the measurement validated against an ideal additive
    /// sawtooth in the same test. A regression guard, not a quality target.
    #[test]
    fn every_source_is_band_limited() {
        const N: usize = 2048;
        const PERIODS: usize = 21;
        let fs = 44_100.0f32;
        let freq = PERIODS as f32 / N as f32 * fs;

        let harmonics = (N / 2 - 1) / PERIODS;
        let ideal: Vec<f64> = (0..N)
            .map(|i| {
                (1..=harmonics)
                    .map(|k| {
                        let ang =
                            2.0 * std::f64::consts::PI * (k * PERIODS) as f64 * i as f64 / N as f64;
                        -2.0 / (std::f64::consts::PI * k as f64) * ang.sin()
                    })
                    .sum()
            })
            .collect();
        let floor = alias_to_signal_db(&ideal, PERIODS);
        assert!(
            floor < -100.0,
            "the analysis is not trustworthy: {floor:.1} dB"
        );

        for (name, mix, bound) in [
            ("saw", only(1.0, 0.0, 0.0, 0.0), -30.0),
            ("pulse", only(0.0, 1.0, 0.0, 0.0), -25.0),
        ] {
            let mut dco = Dco::new(0);
            let x: Vec<f64> = (0..N)
                .map(|_| dco.process(freq, 0.25, &mix, fs) as f64)
                .collect();
            let db = alias_to_signal_db(&x, PERIODS);
            assert!(
                db < bound,
                "{name}: aliasing {db:.1} dB, expected below {bound}"
            );
        }

        // The sub's period is twice the ramp's, so its analysis uses half the periods — and the
        // count has to stay odd for the separation to hold, which 21 / 2 is not. Use a ramp at
        // 2 × 21 periods so the sub completes 21.
        let mut dco = Dco::new(0);
        let x: Vec<f64> = (0..N)
            .map(|_| dco.process(2.0 * freq, 0.5, &only(0.0, 0.0, 1.0, 0.0), fs) as f64)
            .collect();
        let db = alias_to_signal_db(&x, PERIODS);
        assert!(
            db < -25.0,
            "sub: aliasing {db:.1} dB, expected below -25 dB"
        );
    }
}
