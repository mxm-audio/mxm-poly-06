//! One-pole TPT filter, the building block the HPF and the chorus's band-limiting share.
//!
//! mxm-kit's `docs/filters/02-topologies.md` §2.1: the topology-preserving transform of an RC.
//! Lowpass and highpass from one state, so a shelf is one filter and a highpass is
//! `x - lowpass(x)`.

use crate::flush;

/// Cutoff ceiling as a fraction of the sample rate. `tan` blows up at Nyquist.
const NYQUIST_FRACTION: f32 = 0.45;

/// `tan(x)` via the [5/4] Pade approximant, for `x` in `[0, PI * 0.45]`.
///
/// The same approximant `filter.rs` uses, for the same reason: `f32::tan` is a libm call and the
/// chorus recomputes nothing per sample, but the HPF and the voice filters do.
#[inline]
pub fn tan_approx(x: f64) -> f64 {
    let x2 = x * x;
    let x4 = x2 * x2;
    x * (945.0 - 105.0 * x2 + x4) / (945.0 - 420.0 * x2 + 15.0 * x4)
}

/// Prewarped integrator gain for a cutoff.
#[inline]
pub fn prewarp(cutoff_hz: f32, sample_rate: f32) -> f32 {
    let fc = cutoff_hz.clamp(1.0, NYQUIST_FRACTION * sample_rate);
    tan_approx(std::f64::consts::PI * f64::from(fc) / f64::from(sample_rate)) as f32
}

#[derive(Debug, Clone, Copy, Default)]
pub struct OnePole {
    big_g: f32,
    s: f32,
}

impl OnePole {
    pub const fn new() -> Self {
        Self { big_g: 0.0, s: 0.0 }
    }

    pub fn set_cutoff(&mut self, cutoff_hz: f32, sample_rate: f32) {
        let g = prewarp(cutoff_hz, sample_rate);
        self.big_g = g / (1.0 + g);
    }

    pub fn reset(&mut self) {
        self.s = 0.0;
    }

    #[inline]
    pub fn lowpass(&mut self, x: f32) -> f32 {
        let v = self.big_g * (x - self.s);
        let y = v + self.s;
        self.s = flush(y + v);
        y
    }

    #[inline]
    pub fn highpass(&mut self, x: f32) -> f32 {
        x - self.lowpass(x)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn magnitude(mut f: OnePole, hz: f32, fs: f32, high: bool) -> f32 {
        let n = (fs * 0.5) as usize;
        let mut peak = 0.0f32;
        for i in 0..n * 2 {
            let x = (std::f32::consts::TAU * hz * i as f32 / fs).sin();
            let y = if high { f.highpass(x) } else { f.lowpass(x) };
            if i >= n {
                peak = peak.max(y.abs());
            }
        }
        peak
    }

    #[test]
    fn lowpass_is_three_db_down_at_its_corner() {
        for fs in [44_100.0f32, 48_000.0, 96_000.0, 192_000.0] {
            let mut f = OnePole::new();
            f.set_cutoff(1_000.0, fs);
            let db = 20.0 * magnitude(f, 1_000.0, fs, false).log10();
            assert!((db + 3.01).abs() < 0.3, "at {fs}: {db:.2} dB at the corner");
        }
    }

    #[test]
    fn highpass_is_the_complement() {
        let fs = 48_000.0;
        let mut f = OnePole::new();
        f.set_cutoff(500.0, fs);
        let low = 20.0 * magnitude(f, 50.0, fs, true).log10();
        let high = 20.0 * magnitude(f, 5_000.0, fs, true).log10();
        assert!(low < -15.0, "50 Hz through a 500 Hz highpass: {low:.1} dB");
        assert!(high > -0.5, "5 kHz through a 500 Hz highpass: {high:.1} dB");
    }

    #[test]
    fn silence_stays_exactly_zero() {
        let mut f = OnePole::new();
        f.set_cutoff(1_000.0, 48_000.0);
        f.lowpass(1.0);
        for _ in 0..100_000 {
            f.lowpass(0.0);
        }
        assert_eq!(f.lowpass(0.0), 0.0);
    }
}
