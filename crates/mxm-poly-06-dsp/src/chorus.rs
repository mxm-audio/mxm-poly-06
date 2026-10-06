//! The JUNO chorus: two bucket brigades, one triangle, a fixed depth and a hard band limit.
//!
//! `research:effects/juno-chorus.md` is the reference and its §6 recipe is the design; the instrument
//! page owns the fact that the 106 has one and where it sits. This module is **inside the
//! instrument** under the collection's effects rule — the machine shipped with it — and it is kept
//! a module with a plain-values API (mono in, mode, stereo out; it knows nothing about voices) so
//! that the effects collection's standalone version is later a move rather than a rewrite.
//!
//! Three things are the effect, and each has a test:
//!
//! - **The wet path is band-limited and the dry path is not.** A one-pole at about 7.2 kHz going
//!   into the delay stands for the anti-alias filter; two Sallen-Key sections at about 10 kHz coming
//!   out are the reconstruction filter. The dry runs straight to the mixer. Leave this out and the
//!   chorus is glassy and modern and nothing like a JUNO.
//! - **The modulator is a triangle**, so the pitch deviation is a square: constant rate of change
//!   flipping sign at each apex. A sine LFO is a different shimmer.
//! - **Off mutes the wet leg; the clocks keep running.** Switching on un-mutes mid-cycle with no
//!   swell and no settling.
//!
//! # The noise is the circuit's
//!
//! An MN3009 is a crude device with an audible floor, and the collection's *warts and all* rule puts
//! that floor in scope. What makes it the circuit's noise rather than a hiss generator with a better
//! name is **where it enters**: two independent sources, one per BBD, injected at each chip's input,
//! so each takes the whole wet path after it — through the delay modulation, through the
//! reconstruction filter, muted with the wet leg. Its spectrum, its stereo and its absence when the
//! chorus is off all follow from that. Its **level is chosen, not measured** ([`NOISE_LEVEL`]); so
//! is the assumption that the two chips' noise is independent. Both are recorded in the crate's
//! AGENTS.md, and the BBD's distortion is deliberately **not** modelled — a wrong guess at a
//! nonlinearity is worse than none.
//!
//! **The noise stops with the instrument** ([`Chorus::set_active`]). A real 106 hisses with no key
//! pressed; this one reaches exact digital silence at idle, which is the plan's working assumption
//! and a labelled deviation from the hardware, recorded in the plugin's AGENTS.md. The noise gain is
//! a smoothed target — nominal while anything sounds, zero once nothing does — so both the fade-out
//! and a note arriving mid-fade are click-free.
//!
//! # What is chosen
//!
//! The delay range and modulation depth are the reference's most useful unmeasured numbers, and
//! the I+II rate has no community figure at all. [`DELAY_CENTRE_MS`], [`DELAY_DEPTH_MS`] and
//! [`RATE_BOTH_HZ`] are inferences, tuned by ear, and say so.
//!
//! # The fixed quantities are inputs, and the synth never touches them
//!
//! The circuit fixes its rate to three switch positions and its depth, wet level and noise floor
//! outright. **Inside `mxm-poly-06` they stay fixed** — `set_mode` is the whole of that
//! instrument's interface to this module, and it sets the rate from the three constants and nothing
//! else, so the synth's arithmetic is the same to the bit as before the inputs existed. The
//! standalone effect `mxm-chorus-06` (the owner's ruling, 2026-09-03: continuous parameters, with
//! the circuit's positions saved as presets) drives [`Chorus::set_rate_hz`],
//! [`Chorus::set_depth_ms`], [`Chorus::set_wet_level`] and [`Chorus::set_noise_level`] instead.
//! Each input starts at the circuit's value, so a chorus nobody has adjusted **is** the circuit;
//! the moving ones glide over [`CONTROL_SLEW_S`] so a knob never steps the delay, and `set_mode`
//! deliberately does not glide, because a switch on the panel never did.
//!
//! [`DEPTH_MAX_MS`] bounds how far a standalone may open the depth, and sizes the delay line.

use crate::onepole::OnePole;
use crate::{Rng, flush};

/// The panel's two buttons, and both together.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Mode {
    #[default]
    Off,
    I,
    II,
    /// Both buttons latched together.
    Both,
}

impl Mode {
    /// The modulation rate each mode gives. I and II are community measurements; both together
    /// is faster than either — both control lines pull the JFET's gate the same way — and its
    /// value is **chosen**.
    #[inline]
    pub const fn rate_hz(self) -> f32 {
        match self {
            // Off still has a rate: the LFO runs whatever the switches say, and what it runs at
            // while muted is inaudible. Mode I's is as good as any.
            Mode::Off | Mode::I => RATE_I_HZ,
            Mode::II => RATE_II_HZ,
            Mode::Both => RATE_BOTH_HZ,
        }
    }
}

/// Mode I. A community figure, not measured here.
pub const RATE_I_HZ: f32 = 0.5;
/// Mode II. Likewise.
pub const RATE_II_HZ: f32 = 0.8;
/// I + II. **Chosen**: faster than II, because a second current path into the gate can only raise it.
pub const RATE_BOTH_HZ: f32 = 1.3;

/// The nominal delay. **Inferred**: the anti-alias corner bounds the clock from below at about
/// 15 kHz, a chorus wants a few milliseconds, and the reference's inferred working range is
/// 1.3–4.3 ms. This sits in the middle of it.
pub const DELAY_CENTRE_MS: f32 = 2.8;
/// How far the triangle swings the delay either side of centre. **Chosen**, tuned by ear.
pub const DELAY_DEPTH_MS: f32 = 1.2;
/// The widest swing a standalone may ask for, and what the delay line is sized to hold. **Chosen**
/// as exactly twice the circuit's, so the circuit's depth is the midpoint of a linear control and
/// lands there to the bit — a half is exact in `f32`, and a preset that says "the circuit" must
/// reach it exactly. The swing stays short of the centre delay by 0.4 ms, more than the
/// interpolator's reach at any rate the collection supports.
pub const DEPTH_MAX_MS: f32 = 2.0 * DELAY_DEPTH_MS;
/// How long a moving rate or depth takes to reach a new value. **Chosen**: a depth step moves the
/// delay in one sample, which is a pitch click; twenty milliseconds is the middle of the range
/// `plugins/AGENTS.md` measured as neither steppy nor smeared. `set_mode` does not use it.
pub const CONTROL_SLEW_S: f32 = 0.02;

/// The anti-alias one-pole before the BBD: `R122 10 kΩ, C52 0.0022 µF`, computed.
pub const PRE_FILTER_HZ: f32 = 7_234.0;
/// Reconstruction stage 1, Sallen-Key: `fc ≈ 9.7 kHz, Q ≈ 0.55`, computed from the schematic.
pub const RECON_1: (f32, f32) = (9_700.0, 0.55);
/// Reconstruction stage 2: `fc ≈ 10.4 kHz, Q ≈ 1.29`, computed.
pub const RECON_2: (f32, f32) = (10_400.0, 1.29);

/// The wet leg's gain relative to the dry: `(100/39) / (100/47)`, read off the output summer. The
/// wet sits about 1.6 dB above the dry.
pub const WET_GAIN: f32 = 47.0 / 39.0;

/// The noise source's amplitude at the BBD input. **Chosen**, roughly −75 dBFS RMS against a
/// full-scale voice sum. Nothing measures the real floor; the fidelity gate is where it is heard.
pub const NOISE_LEVEL: f32 = 3.0e-4;

/// How long the JFET mute takes to open or close. **Chosen**: long enough not to click, short enough
/// that off is off. The hardware's control logic was not traced.
pub const WET_SWITCH_S: f32 = 0.005;

/// How long the noise takes to reach idle silence, or to come back. Chosen.
pub const NOISE_FADE_S: f32 = 0.05;

/// Below this the fading noise gain snaps to exactly zero: about −140 dBFS, two orders under
/// anything a converter can carry. `ln(NOISE_LEVEL / NOISE_SNAP)` is eight time constants.
const NOISE_SNAP: f32 = 1e-7;

/// The longest delay the line has to hold, with a margin for the interpolator's reach. Sized for
/// the widest depth a standalone may ask for, not the circuit's: the ring is a power of two and
/// a read inside the circuit's range is the same read whatever the ring's length.
const MAX_DELAY_MS: f32 = DELAY_CENTRE_MS + DEPTH_MAX_MS + 0.5;

/// RBJ lowpass biquad, transposed direct form II, states flushed.
#[derive(Debug, Clone, Copy, Default)]
struct Biquad {
    b0: f32,
    b1: f32,
    b2: f32,
    a1: f32,
    a2: f32,
    z1: f32,
    z2: f32,
}

impl Biquad {
    fn set_lowpass(&mut self, fc: f32, q: f32, sample_rate: f32) {
        // Audio EQ Cookbook, lowpass. Evaluated in f64: the coefficients are a difference of
        // nearly equal numbers close to Nyquist.
        let fc = f64::from(fc.min(0.45 * sample_rate));
        let w0 = 2.0 * std::f64::consts::PI * fc / f64::from(sample_rate);
        let alpha = w0.sin() / (2.0 * f64::from(q));
        let cos = w0.cos();
        let a0 = 1.0 + alpha;
        self.b0 = ((1.0 - cos) / 2.0 / a0) as f32;
        self.b1 = ((1.0 - cos) / a0) as f32;
        self.b2 = self.b0;
        self.a1 = (-2.0 * cos / a0) as f32;
        self.a2 = ((1.0 - alpha) / a0) as f32;
    }

    fn reset(&mut self) {
        self.z1 = 0.0;
        self.z2 = 0.0;
    }

    #[inline]
    fn process(&mut self, x: f32) -> f32 {
        let y = self.b0 * x + self.z1;
        self.z1 = self.b1 * x - self.a1 * y + self.z2;
        self.z2 = self.b2 * x - self.a2 * y;
        // **Flushed together, never separately.** Flushing each state on its own produced a limit
        // cycle that hovered between 1e-20 and 5e-20 for ever: `z2 = -a2·y` fell under the
        // threshold and was zeroed while `z1 = -a1·y` (with `|a1| > 1`) survived and grew, which
        // removed the damping term from the recursion. Measured, not conjectured. Two states of one
        // recursion go to zero as one.
        if self.z1.abs() < 1e-20 && self.z2.abs() < 1e-20 {
            self.z1 = 0.0;
            self.z2 = 0.0;
        }
        y
    }
}

/// A ring buffer read at a fractional delay with four-point cubic interpolation.
///
/// A BBD's delay changes continuously, so zero-order or linear interpolation would add artefacts
/// of its own on top of the ones the circuit has. Catmull-Rom is the cheapest that does not.
#[derive(Debug, Clone, Default)]
struct DelayLine {
    buf: Vec<f32>,
    mask: usize,
    write: usize,
}

impl DelayLine {
    /// Allocates. Called from `set_sample_rate`, never from the audio path.
    fn allocate(&mut self, max_samples: usize) {
        let len = (max_samples + 8).next_power_of_two();
        self.buf = vec![0.0; len];
        self.mask = len - 1;
        self.write = 0;
    }

    fn clear(&mut self) {
        self.buf.iter_mut().for_each(|s| *s = 0.0);
        self.write = 0;
    }

    #[inline]
    fn push(&mut self, x: f32) {
        self.buf[self.write] = x;
        self.write = (self.write + 1) & self.mask;
    }

    /// The sample `delay` samples ago, where `1.0` is the sample just written.
    #[inline]
    fn read(&self, delay: f32) -> f32 {
        let max = (self.buf.len() - 4) as f32;
        let d = delay.clamp(2.0, max);
        let i = d as usize;
        let frac = d - i as f32;
        // Older is further back: p1 is at `i`, p2 one older, p0 one newer, p3 two older.
        let at = |k: usize| self.buf[(self.write.wrapping_sub(k)) & self.mask];
        let (p0, p1, p2, p3) = (at(i - 1), at(i), at(i + 1), at(i + 2));
        let c1 = 0.5 * (p2 - p0);
        let c2 = p0 - 2.5 * p1 + 2.0 * p2 - 0.5 * p3;
        let c3 = 0.5 * (p3 - p0) + 1.5 * (p1 - p2);
        ((c3 * frac + c2) * frac + c1) * frac + p1
    }
}

/// One BBD channel: the delay line, its own noise, and the reconstruction filter after it.
#[derive(Debug, Clone)]
struct Channel {
    line: DelayLine,
    recon: [Biquad; 2],
    noise: Rng,
    seed: u32,
}

impl Channel {
    fn new(seed: u32) -> Self {
        Self {
            line: DelayLine::default(),
            recon: [Biquad::default(); 2],
            noise: Rng::new(seed),
            seed,
        }
    }

    fn set_sample_rate(&mut self, sample_rate: f32) {
        self.line
            .allocate((MAX_DELAY_MS * sample_rate / 1000.0).ceil() as usize);
        self.recon[0].set_lowpass(RECON_1.0, RECON_1.1, sample_rate);
        self.recon[1].set_lowpass(RECON_2.0, RECON_2.1, sample_rate);
    }

    fn reset(&mut self) {
        self.line.clear();
        self.recon.iter_mut().for_each(Biquad::reset);
        self.noise = Rng::new(self.seed);
    }

    /// One sample through the bucket brigade and its reconstruction filter.
    #[inline]
    fn process(&mut self, pre: f32, noise_gain: f32, delay_samples: f32) -> f32 {
        // The noise enters where the chip is, so it is delayed, modulated and filtered like the
        // signal — which is the difference between the circuit's noise and an added hiss.
        self.line.push(pre + noise_gain * self.noise.next_bipolar());
        let wet = self.line.read(delay_samples);
        let first = self.recon[0].process(wet);
        self.recon[1].process(first)
    }
}

#[derive(Debug, Clone)]
pub struct Chorus {
    mode: Mode,
    pre: OnePole,
    channels: [Channel; 2],
    /// The modulation triangle's phase, `0..1`. Never reset by a mode change: the clocks keep
    /// running.
    phase: f32,
    /// The JFET mute, smoothed: `wet_level` open, `0` muted.
    wet_gain: f32,
    wet_coef: f32,
    /// The noise's gain, smoothed toward `noise_level` or zero.
    noise_gain: f32,
    noise_coef: f32,
    active: bool,
    sample_rate: f32,

    // The four inputs the circuit fixes. Each starts at the circuit's value; `set_mode` writes only
    // the rate, and writes it without a glide.
    //
    // **A glide is kept as the distance still to go, not as the moving value.** Updating the value
    // — `target + (value − target)·coef` — stalls in `f32` once a step falls under half an ulp of
    // the value: measured landing at 1.29994 for a target of 1.3, for ever. The distance decays
    // geometrically against its own scale and reaches the snap; and at zero it adds nothing, to
    // the bit, which is what the synth's render depends on.
    /// The modulator's rate: the target, and how far the glide still has to go.
    rate_target_hz: f32,
    rate_glide_hz: f32,
    /// The delay's swing either side of centre: the target and the remaining glide.
    depth_target_ms: f32,
    depth_glide_ms: f32,
    /// What the mute opens to, as a multiple of [`WET_GAIN`]: `1` is the circuit.
    wet_level: f32,
    /// The noise source's amplitude while active: [`NOISE_LEVEL`] is the circuit.
    noise_level: f32,
    /// The glide for rate and depth, [`CONTROL_SLEW_S`].
    control_coef: f32,
}

impl Default for Chorus {
    fn default() -> Self {
        Self::new()
    }
}

impl Chorus {
    pub fn new() -> Self {
        let mut chorus = Self {
            mode: Mode::Off,
            pre: OnePole::new(),
            channels: [Channel::new(0xBBD0_0001), Channel::new(0xBBD0_0002)],
            phase: 0.0,
            wet_gain: 0.0,
            wet_coef: 0.0,
            noise_gain: 0.0,
            noise_coef: 0.0,
            active: false,
            sample_rate: 48_000.0,
            rate_target_hz: Mode::Off.rate_hz(),
            rate_glide_hz: 0.0,
            depth_target_ms: DELAY_DEPTH_MS,
            depth_glide_ms: 0.0,
            wet_level: 1.0,
            noise_level: NOISE_LEVEL,
            control_coef: 0.0,
        };
        chorus.set_sample_rate(48_000.0);
        chorus
    }

    /// Allocates the delay lines and computes the filters. Never called from the audio path.
    pub fn set_sample_rate(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate;
        self.pre.set_cutoff(PRE_FILTER_HZ, sample_rate);
        for channel in &mut self.channels {
            channel.set_sample_rate(sample_rate);
        }
        self.wet_coef = (-1.0 / (WET_SWITCH_S * sample_rate)).exp();
        self.noise_coef = (-1.0 / (NOISE_FADE_S * sample_rate)).exp();
        self.control_coef = (-1.0 / (CONTROL_SLEW_S * sample_rate)).exp();
    }

    /// Clear every bit of state, reseed the noise, and put the LFO back to its start. Leaves no
    /// tail and makes the next render bit-identical to a fresh instance's — including the mute,
    /// which reopens over its 5 ms exactly as it does on construction.
    pub fn reset(&mut self) {
        self.pre.reset();
        for channel in &mut self.channels {
            channel.reset();
        }
        self.phase = 0.0;
        self.wet_gain = 0.0;
        self.noise_gain = 0.0;
        // `mode`, `active` and the four inputs are control inputs, not state, and are left as the
        // caller set them — but a glide in progress is state, and lands: after a reset nothing is
        // still moving.
        self.rate_glide_hz = 0.0;
        self.depth_glide_ms = 0.0;
    }

    /// Control-rate. Changes the LFO's rate and the mute's target; nothing restarts.
    ///
    /// **The synth's whole interface to this module.** The rate takes the mode's constant at once,
    /// with no glide — a switch on the panel never glided — and the depth, wet level and noise
    /// level are not touched, so an instrument that only ever calls this is the circuit exactly.
    pub fn set_mode(&mut self, mode: Mode) {
        self.mode = mode;
        self.rate_target_hz = mode.rate_hz();
        self.rate_glide_hz = 0.0;
    }

    pub fn mode(&self) -> Mode {
        self.mode
    }

    /// The modulator's rate, for a standalone that has a knob where the panel had two buttons.
    /// Glides over [`CONTROL_SLEW_S`]; the phase runs on, so nothing restarts.
    pub fn set_rate_hz(&mut self, hz: f32) {
        let now = self.rate_hz();
        self.rate_target_hz = hz.max(0.0);
        self.rate_glide_hz = now - self.rate_target_hz;
    }

    /// The rate the modulator is running at this instant, glide included.
    pub fn rate_hz(&self) -> f32 {
        self.rate_target_hz + self.rate_glide_hz
    }

    /// How far the triangle swings the delay, in milliseconds either side of [`DELAY_CENTRE_MS`].
    /// Clamped to `0..=`[`DEPTH_MAX_MS`], and glides over [`CONTROL_SLEW_S`], because a step here
    /// is a step in the delay and the delay's derivative is the pitch.
    pub fn set_depth_ms(&mut self, ms: f32) {
        let now = self.depth_ms();
        self.depth_target_ms = ms.clamp(0.0, DEPTH_MAX_MS);
        self.depth_glide_ms = now - self.depth_target_ms;
    }

    /// The swing at this instant, glide included.
    pub fn depth_ms(&self) -> f32 {
        self.depth_target_ms + self.depth_glide_ms
    }

    /// What the mute opens to, as a multiple of the circuit's [`WET_GAIN`]: `1` is the circuit and
    /// `0` is the mute closed. Reaches the audio through the same smoother the mute uses.
    pub fn set_wet_level(&mut self, level: f32) {
        self.wet_level = level.max(0.0);
    }

    /// The noise source's amplitude while anything sounds. [`NOISE_LEVEL`] is the circuit; zero is
    /// a BBD with no floor, which no chip has and a person may want.
    pub fn set_noise_level(&mut self, level: f32) {
        self.noise_level = level.max(0.0);
    }

    /// The four inputs as they stand, in the order rate, depth, wet level, noise level.
    pub fn inputs(&self) -> (f32, f32, f32, f32) {
        (
            self.rate_target_hz,
            self.depth_target_ms,
            self.wet_level,
            self.noise_level,
        )
    }

    /// True once the mute is fully closed: from here the output is the dry signal to the bit,
    /// whatever the delay lines hold and whatever the noise gain is doing inside them, and a host
    /// that stops calling [`Chorus::process`] loses nothing. What the lines hold is stale the moment
    /// processing stops, so a caller that stops calls [`Chorus::silence`] first and, on resuming,
    /// hands the skipped time to [`Chorus::advance`].
    pub fn is_wet_silent(&self) -> bool {
        self.wet_gain == 0.0
    }

    /// Moves the modulator's phase as if `samples` had been processed, without processing them.
    ///
    /// For a caller that stopped calling [`Chorus::process`] while the wet leg was silent: the
    /// circuit's LFO never stops, so on resuming the phase is where the circuit's would be rather
    /// than where it was left. Only the phase moves; the glides were already landed, or the
    /// caller would not have stopped.
    pub fn advance(&mut self, samples: u32) {
        let cycles = self.rate_hz() / self.sample_rate * samples as f32;
        self.phase = (self.phase + cycles).rem_euclid(1.0);
    }

    /// Whether anything upstream is sounding. The noise floor is present while it is and fades to
    /// exact silence when it is not — see the module doc.
    pub fn set_active(&mut self, active: bool) {
        self.active = active;
    }

    /// All sound off: everything in the audio path goes to zero now — the delay lines, the
    /// filters, the noise — while the modulator's phase and the mute's state stay where they are.
    /// A panic is the one moment a chorus should not ring out its few milliseconds; the hardware
    /// has no panic to compare against, and a host expects silence on the next sample.
    pub fn silence(&mut self) {
        self.pre.reset();
        for channel in &mut self.channels {
            channel.reset();
        }
        self.noise_gain = 0.0;
        self.active = false;
    }

    /// The noise has faded to nothing. Once it has, the wet path drains within
    /// [`Chorus::drain_samples`] of its input going silent, and the output is exactly the dry.
    pub fn is_quiet(&self) -> bool {
        self.noise_gain == 0.0
    }

    /// How long the wet path can still carry something after its input goes silent: the longest
    /// delay plus the reconstruction filter's ring.
    pub fn drain_samples(&self) -> u32 {
        (MAX_DELAY_MS * self.sample_rate / 1000.0) as u32 + 512
    }

    /// The whole tail: the noise fading to its snap point — `ln(noise_level / NOISE_SNAP)` time
    /// constants, rounded up and one more; nine at the circuit's level — and then the drain.
    pub fn tail_samples(&self) -> u32 {
        let level = self.noise_level.max(2.0 * NOISE_SNAP);
        let taus = (level / NOISE_SNAP).ln().ceil() + 1.0;
        let fade = NOISE_FADE_S * self.sample_rate * taus;
        fade as u32 + self.drain_samples()
    }

    /// The modulation triangle's current value, `-1..=1`, for telemetry.
    pub fn lfo(&self) -> f32 {
        triangle(self.phase)
    }

    /// One sample in, two out: the dry plus each clock's wet.
    #[inline]
    pub fn process(&mut self, x: f32) -> (f32, f32) {
        let (a, b) = self.wet(x);
        (flush(x + a), flush(x + b))
    }

    /// One sample in, the two **wet** contributions out — what [`Chorus::process`] adds to the dry.
    ///
    /// For a caller whose dry leg is not `x`: a standalone on a stereo track sums the channels
    /// into the chorus and keeps a dry leg of its own. The arithmetic `process` performs is
    /// exactly this followed by `x + a`, so the two agree to the bit.
    #[inline]
    pub fn wet(&mut self, x: f32) -> (f32, f32) {
        let fs = self.sample_rate;

        // The mute and the noise gain, each a smoothed target.
        let wet_target = if self.mode == Mode::Off {
            0.0
        } else {
            self.wet_level
        };
        self.wet_gain = flush(wet_target + (self.wet_gain - wet_target) * self.wet_coef);
        if (self.wet_gain - wet_target).abs() < 1e-4 {
            self.wet_gain = wet_target;
        }
        let noise_target = if self.active { self.noise_level } else { 0.0 };
        self.noise_gain = flush(noise_target + (self.noise_gain - noise_target) * self.noise_coef);
        if (self.noise_gain - noise_target).abs() < NOISE_SNAP {
            self.noise_gain = noise_target;
        }

        // The two inputs that glide: the remaining distance decays, and once it is nothing it adds
        // nothing, to the bit — which is what keeps the synth's render unchanged. The synth only
        // ever sets the rate through `set_mode`, which lands it at once, and never moves the depth.
        if self.rate_glide_hz != 0.0 {
            self.rate_glide_hz *= self.control_coef;
            if self.rate_glide_hz.abs() < 1e-6 {
                self.rate_glide_hz = 0.0;
            }
        }
        if self.depth_glide_ms != 0.0 {
            self.depth_glide_ms *= self.control_coef;
            if self.depth_glide_ms.abs() < 1e-6 {
                self.depth_glide_ms = 0.0;
            }
        }
        let rate_hz = self.rate_target_hz + self.rate_glide_hz;
        let depth_ms = self.depth_target_ms + self.depth_glide_ms;

        // The one triangle, in antiphase to the two clocks.
        let tri = triangle(self.phase);
        self.phase += rate_hz / fs;
        if self.phase >= 1.0 {
            self.phase -= 1.0;
        }
        let ms_per_sample = 1000.0 / fs;
        let delay_a = (DELAY_CENTRE_MS + depth_ms * tri) / ms_per_sample;
        let delay_b = (DELAY_CENTRE_MS - depth_ms * tri) / ms_per_sample;

        // The wet path's input is band-limited; the dry path's is not.
        let pre = self.pre.lowpass(x);
        let wet_a = self.channels[0].process(pre, self.noise_gain, delay_a);
        let wet_b = self.channels[1].process(pre, self.noise_gain, delay_b);

        let wet = self.wet_gain * WET_GAIN;
        (wet * wet_a, wet * wet_b)
    }
}

#[inline]
fn triangle(p: f32) -> f32 {
    if p < 0.25 {
        4.0 * p
    } else if p < 0.75 {
        2.0 - 4.0 * p
    } else {
        4.0 * p - 4.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FS: f32 = 48_000.0;

    fn render(mode: Mode, active: bool, input: impl Fn(usize) -> f32, n: usize) -> Vec<(f32, f32)> {
        let mut c = Chorus::new();
        c.set_sample_rate(FS);
        c.set_mode(mode);
        c.set_active(active);
        (0..n).map(|i| c.process(input(i))).collect()
    }

    fn saw(i: usize) -> f32 {
        let hz = 110.0;
        2.0 * ((i as f32 * hz / FS) % 1.0) - 1.0
    }

    #[test]
    fn off_is_dual_mono_and_on_is_not() {
        let off = render(Mode::Off, true, saw, 48_000);
        let skip = 20_000; // past the mute's settling and the noise fade-in
        assert!(
            off[skip..].iter().all(|(l, r)| l == r),
            "with the chorus off the two channels must be identical"
        );
        for mode in [Mode::I, Mode::II, Mode::Both] {
            let on = render(mode, true, saw, 48_000);
            let differ = on[skip..]
                .iter()
                .filter(|(l, r)| (l - r).abs() > 1e-3)
                .count();
            assert!(
                differ > 10_000,
                "{mode:?} gave {differ} differing samples of 28000"
            );
        }
    }

    /// Energy above `hz` as a fraction of the total, via a direct DFT on a window.
    fn high_fraction(x: &[f32], hz: f32) -> f64 {
        let n = x.len();
        let (mut total, mut high) = (0.0f64, 0.0f64);
        for bin in 1..n / 2 {
            let (mut re, mut im) = (0.0f64, 0.0f64);
            for (i, &v) in x.iter().enumerate() {
                let ang = -2.0 * std::f64::consts::PI * bin as f64 * i as f64 / n as f64;
                re += v as f64 * ang.cos();
                im += v as f64 * ang.sin();
            }
            let p = re * re + im * im;
            total += p;
            if bin as f32 * FS / n as f32 > hz {
                high += p;
            }
        }
        high / total.max(1e-30)
    }

    /// **The band limit is the sound.** Feed white noise; the wet component (out − dry, since the
    /// dry gain is exactly 1) has far less energy above 12 kHz than the input does.
    #[test]
    fn the_wet_path_is_band_limited_and_the_dry_is_not() {
        let mut rng = Rng::new(42);
        let input: Vec<f32> = (0..8192).map(|_| 0.5 * rng.next_bipolar()).collect();
        let mut c = Chorus::new();
        c.set_sample_rate(FS);
        c.set_mode(Mode::I);
        c.set_active(false);
        let out: Vec<(f32, f32)> = input.iter().map(|&x| c.process(x)).collect();
        let wet: Vec<f32> = out.iter().zip(&input).map(|((l, _), x)| l - x).collect();

        let window = 2048;
        let dry_hf = high_fraction(&input[input.len() - window..], 12_000.0);
        let wet_hf = high_fraction(&wet[wet.len() - window..], 12_000.0);
        assert!(
            dry_hf > 0.3,
            "white noise should carry ~40% of its energy above 12 kHz: {dry_hf}"
        );
        assert!(
            wet_hf < dry_hf * 0.15,
            "the wet path carries {wet_hf:.3} of its energy above 12 kHz against the dry's {dry_hf:.3}: it is not band-limited"
        );
        // And the wet path carries real signal: the delayed noise, not nothing.
        let wet_rms = (wet.iter().map(|v| v * v).sum::<f32>() / wet.len() as f32).sqrt();
        assert!(
            wet_rms > 0.05,
            "the wet path is nearly empty: rms {wet_rms}"
        );
    }

    /// **Off does not stop the clocks.** Not "the dry is unchanged" — a restarted wet would pass
    /// that too. One instance runs with the chorus on throughout; another switches off and back on.
    /// After the mute reopens, their wet paths agree, which they cannot if the modulator restarted.
    #[test]
    fn switching_off_and_on_does_not_restart_the_modulator() {
        let mut always = Chorus::new();
        let mut toggled = Chorus::new();
        for c in [&mut always, &mut toggled] {
            c.set_sample_rate(FS);
            c.set_mode(Mode::I);
            c.set_active(false);
        }
        let n = 48_000;
        let mut diverged_while_off = false;
        let mut agree_after = 0usize;
        for i in 0..n * 3 {
            let x = saw(i);
            if i == n {
                toggled.set_mode(Mode::Off);
            }
            if i == 2 * n {
                toggled.set_mode(Mode::I);
            }
            let (a, _) = always.process(x);
            let (t, _) = toggled.process(x);
            if (n + 2_000..2 * n).contains(&i) && (a - t).abs() > 1e-3 {
                diverged_while_off = true;
            }
            if i > 2 * n + 2_000 && (a - t).abs() < 1e-4 {
                agree_after += 1;
            }
        }
        assert!(diverged_while_off, "the mute did nothing");
        assert!(
            agree_after > n - 2_100,
            "after switching back on only {agree_after} samples agreed with the always-on instance; the modulator restarted"
        );
    }

    #[test]
    fn switching_the_mute_does_not_click() {
        let mut c = Chorus::new();
        c.set_sample_rate(FS);
        c.set_mode(Mode::Off);
        c.set_active(false);
        let mut worst = 0.0f32;
        let mut prev = 0.0f32;
        for i in 0..48_000 {
            if i == 20_000 {
                c.set_mode(Mode::II);
            }
            if i == 35_000 {
                c.set_mode(Mode::Off);
            }
            let (l, _) = c.process(0.5 * (std::f32::consts::TAU * 220.0 * i as f32 / FS).sin());
            if i > 1_000 {
                worst = worst.max((l - prev).abs());
            }
            prev = l;
        }
        // A 220 Hz sine at 0.5 moves at most about 0.015 per sample; the wet adds up to 1.2 times
        // as much again. Anything past that is a step.
        assert!(worst < 0.06, "a step of {worst} at the mute switch");
    }

    #[test]
    fn the_noise_is_present_only_when_on_and_active() {
        let silent = |c: &mut Chorus| {
            let mut peak = 0.0f32;
            for _ in 0..24_000 {
                let (l, r) = c.process(0.0);
                peak = peak.max(l.abs()).max(r.abs());
            }
            peak
        };
        let mut c = Chorus::new();
        c.set_sample_rate(FS);

        c.set_mode(Mode::Off);
        c.set_active(true);
        assert_eq!(silent(&mut c), 0.0, "off mutes the wet leg, noise included");

        c.set_mode(Mode::I);
        c.set_active(true);
        let hiss = silent(&mut c);
        assert!(
            hiss > 1e-5 && hiss < 5e-3,
            "the floor with the chorus on and a note held: {hiss}"
        );

        c.set_active(false);
        silent(&mut c);
        assert_eq!(silent(&mut c), 0.0, "and it fades to exact silence at idle");
        assert!(c.is_quiet());
    }

    #[test]
    fn the_stereo_hiss_is_two_hisses_and_not_one_inverted() {
        let mut c = Chorus::new();
        c.set_sample_rate(FS);
        c.set_mode(Mode::I);
        c.set_active(true);
        for _ in 0..24_000 {
            c.process(0.0);
        }
        let (mut same, mut anti, mut n) = (0.0f64, 0.0f64, 0.0f64);
        for _ in 0..24_000 {
            let (l, r) = c.process(0.0);
            same += (l * r) as f64;
            anti += (l * -r) as f64;
            n += (l * l) as f64;
        }
        assert!(
            same.abs() / n < 0.1 && anti.abs() / n < 0.1,
            "the two channels' noise is correlated: {same} / {anti} against {n}"
        );
    }

    #[test]
    fn two_instances_render_identically() {
        let a = render(Mode::Both, true, saw, 20_000);
        let b = render(Mode::Both, true, saw, 20_000);
        assert_eq!(a, b, "the noise seed and the LFO phase must be fixed");
    }

    #[test]
    fn reset_leaves_no_tail_and_restores_determinism() {
        let mut c = Chorus::new();
        c.set_sample_rate(FS);
        c.set_mode(Mode::I);
        c.set_active(true);
        let first: Vec<(f32, f32)> = (0..10_000).map(|i| c.process(saw(i))).collect();
        c.reset();
        let second: Vec<(f32, f32)> = (0..10_000).map(|i| c.process(saw(i))).collect();
        assert_eq!(first, second);
    }

    #[test]
    fn silence_in_gives_exactly_zero_out_once_drained() {
        for fs in [44_100.0f32, 48_000.0, 96_000.0, 192_000.0] {
            let mut c = Chorus::new();
            c.set_sample_rate(fs);
            c.set_mode(Mode::Both);
            c.set_active(true);
            for i in 0..10_000 {
                c.process(saw(i));
            }
            c.set_active(false);
            for _ in 0..c.tail_samples() * 2 {
                c.process(0.0);
            }
            assert_eq!(c.process(0.0), (0.0, 0.0), "at {fs}");
            assert!(c.is_quiet());
        }
    }

    #[test]
    fn stays_finite_under_a_full_scale_input() {
        let mut c = Chorus::new();
        c.set_sample_rate(FS);
        c.set_mode(Mode::Both);
        c.set_active(true);
        for i in 0..48_000 {
            let (l, r) = c.process(4.0 * saw(i));
            assert!(l.is_finite() && r.is_finite());
            assert!(l.abs() < 12.0 && r.abs() < 12.0);
        }
    }

    // ---- The four inputs, which the synth never touches ----

    /// **The inputs at their defaults are the mode API, to the bit.** One instance is driven the
    /// way the synth drives it; the other the way the standalone will, with every input set to the
    /// circuit's value by hand. If these ever differ, the synth's render has moved.
    #[test]
    fn the_inputs_at_the_circuits_values_render_identically_to_the_mode_api() {
        for mode in [Mode::I, Mode::II, Mode::Both] {
            let by_mode = render(mode, true, saw, 30_000);

            let mut c = Chorus::new();
            c.set_sample_rate(FS);
            c.set_mode(mode);
            c.set_active(true);
            c.set_rate_hz(mode.rate_hz());
            c.set_depth_ms(DELAY_DEPTH_MS);
            c.set_wet_level(1.0);
            c.set_noise_level(NOISE_LEVEL);
            let by_inputs: Vec<(f32, f32)> = (0..30_000).map(|i| c.process(saw(i))).collect();

            assert_eq!(by_mode, by_inputs, "{mode:?}");
        }
    }

    /// **`set_mode` lands at once; `set_rate_hz` glides.** A switch on the panel never glided, and
    /// the synth's render depends on that; a knob must, or a step in the rate is a step in the
    /// pitch deviation.
    #[test]
    fn the_rate_follows_the_knob_after_a_glide_and_the_mode_at_once() {
        let mut c = Chorus::new();
        c.set_sample_rate(FS);
        c.set_mode(Mode::I);
        c.set_rate_hz(RATE_BOTH_HZ);
        assert_eq!(c.rate_hz(), RATE_I_HZ, "a knob does not step");
        for _ in 0..(CONTROL_SLEW_S * FS * 20.0) as usize {
            c.process(0.0);
        }
        // **Lands exactly**, not within a tolerance: the value form of this glide stalled 6e-5
        // short of 1.3 for ever, and the distance form is what this line holds.
        assert_eq!(c.rate_hz(), RATE_BOTH_HZ, "and it lands");
        c.set_mode(Mode::II);
        assert_eq!(c.rate_hz(), RATE_II_HZ, "the switch lands at once");

        // And the period the modulator runs at is the rate asked for: two corners per cycle,
        // counted over ten seconds at 4 Hz.
        c.set_rate_hz(4.0);
        for _ in 0..(CONTROL_SLEW_S * FS * 20.0) as usize {
            c.process(0.0);
        }
        let mut corners = 0;
        let mut prev = c.lfo();
        let mut rising = true;
        for _ in 0..(10.0 * FS) as usize {
            c.process(0.0);
            let v = c.lfo();
            let now_rising = v >= prev;
            if now_rising != rising {
                corners += 1;
                rising = now_rising;
            }
            prev = v;
        }
        assert!(
            (corners - 80i32).abs() <= 2,
            "4 Hz for ten seconds is eighty corners, not {corners}"
        );
    }

    /// **Depth is the stereo.** With no swing both channels read the same delay, so with the noise
    /// off they are one signal; any swing at all parts them. And zero is the *only* depth that does.
    #[test]
    fn depth_zero_collapses_the_two_clocks_onto_one_delay() {
        let settle = (CONTROL_SLEW_S * FS * 20.0) as usize;
        let mut c = Chorus::new();
        c.set_sample_rate(FS);
        c.set_mode(Mode::I);
        c.set_active(true);
        c.set_noise_level(0.0);
        c.set_depth_ms(0.0);
        for i in 0..settle {
            c.process(saw(i));
        }
        for i in settle..settle + 20_000 {
            let (l, r) = c.process(saw(i));
            assert_eq!(l, r, "no depth is no stereo");
        }
        c.set_depth_ms(0.2);
        for i in 0..settle {
            c.process(saw(i));
        }
        let differ = (0..20_000)
            .map(|i| c.process(saw(i)))
            .filter(|(l, r)| l != r)
            .count();
        assert!(differ > 15_000, "a swing parts the channels: {differ}");
    }

    /// **The depth is clamped to the line.** Asking for more than [`DEPTH_MAX_MS`] gets exactly
    /// that, and the read stays inside the ring at every supported rate.
    #[test]
    fn the_widest_depth_stays_finite_and_inside_the_line_at_every_rate() {
        for fs in [44_100.0f32, 48_000.0, 96_000.0, 192_000.0] {
            let mut c = Chorus::new();
            c.set_sample_rate(fs);
            c.set_mode(Mode::Both);
            c.set_active(true);
            c.set_depth_ms(100.0);
            assert_eq!(c.inputs().1, DEPTH_MAX_MS);
            for i in 0..(fs as usize) {
                let (l, r) = c.process(saw(i));
                assert!(l.is_finite() && r.is_finite(), "at {fs}");
                assert!(l.abs() < 12.0 && r.abs() < 12.0, "at {fs}");
            }
        }
    }

    /// **Wet level zero is the mute, and the dry to the bit.** Not a quiet wet: the output *is*
    /// the input, noise included, as it is in Off.
    #[test]
    fn wet_level_zero_is_bit_exact_dry() {
        let mut c = Chorus::new();
        c.set_sample_rate(FS);
        c.set_mode(Mode::I);
        c.set_active(true);
        c.set_wet_level(0.0);
        for i in 0..(WET_SWITCH_S * FS * 20.0) as usize {
            c.process(saw(i));
        }
        assert!(c.is_wet_silent());
        for i in 0..20_000 {
            let x = saw(i);
            assert_eq!(c.process(x), (x, x));
        }
    }

    /// **Noise level zero is a BBD with no floor**: with the input silent and the chorus on and
    /// active, nothing comes out once the lines have drained — where the circuit's level would
    /// hiss for as long as anything is active.
    #[test]
    fn noise_level_zero_leaves_no_floor() {
        let mut c = Chorus::new();
        c.set_sample_rate(FS);
        c.set_mode(Mode::I);
        c.set_active(true);
        c.set_noise_level(0.0);
        for _ in 0..c.drain_samples() * 2 {
            c.process(0.0);
        }
        for _ in 0..10_000 {
            assert_eq!(c.process(0.0), (0.0, 0.0));
        }
    }

    /// **A moving control does not click.** Rate, depth and wet level each swept end to end over
    /// a second under a sine, and the largest sample-to-sample step is bounded by what the signal
    /// itself moves.
    #[test]
    fn sweeping_any_input_does_not_click() {
        let sweep = |set: fn(&mut Chorus, f32)| {
            let mut c = Chorus::new();
            c.set_sample_rate(FS);
            c.set_mode(Mode::I);
            c.set_active(true);
            c.set_noise_level(0.0);
            let n = FS as usize;
            let (mut worst, mut prev) = (0.0f32, 0.0f32);
            for i in 0..3 * n {
                // Up over the second, then down, then held.
                let t = if i < n {
                    i as f32 / n as f32
                } else if i < 2 * n {
                    (2 * n - i) as f32 / n as f32
                } else {
                    0.0
                };
                set(&mut c, t);
                let (l, _) = c.process(0.5 * (std::f32::consts::TAU * 220.0 * i as f32 / FS).sin());
                if i > 1_000 {
                    worst = worst.max((l - prev).abs());
                }
                prev = l;
            }
            worst
        };
        // A 220 Hz sine at 0.5 moves at most about 0.015 per sample, the wet up to 1.2 times as
        // much again; the depth sweep also bends the pitch, which the bound has room for.
        let rate = sweep(|c, t| c.set_rate_hz(0.1 + 9.9 * t));
        assert!(rate < 0.06, "a step of {rate} under a rate sweep");
        let depth = sweep(|c, t| c.set_depth_ms(DEPTH_MAX_MS * t));
        assert!(depth < 0.06, "a step of {depth} under a depth sweep");
        let wet = sweep(|c, t| c.set_wet_level(1.5 * t));
        assert!(wet < 0.06, "a step of {wet} under a wet sweep");
    }

    /// **Skipped time is not lost time.** A caller that stops processing while the wet leg is
    /// silent hands the skipped samples to `advance`, and the modulator is where the circuit's
    /// would be — within a rounding of the phase, not a fraction of a cycle.
    #[test]
    fn advance_moves_the_modulator_as_processing_would_have() {
        let mut processed = Chorus::new();
        let mut skipped = Chorus::new();
        for c in [&mut processed, &mut skipped] {
            c.set_sample_rate(FS);
            c.set_mode(Mode::II);
        }
        let n = 100_000u32;
        for _ in 0..n {
            processed.process(0.0);
        }
        skipped.advance(n);
        assert!(
            (processed.lfo() - skipped.lfo()).abs() < 1e-2,
            "{} against {}",
            processed.lfo(),
            skipped.lfo()
        );
    }

    /// **The modulator is a triangle, not a sine.** A triangle sweeping the delay means a square
    /// pitch deviation — constant rate of change flipping sign at each apex — which is part of this
    /// chorus's particular shimmer. Asserted on the modulator itself: piecewise linear, with exactly
    /// two corners per cycle. A sine would have a non-zero second difference everywhere.
    #[test]
    fn the_modulator_is_a_triangle_with_two_corners_per_cycle() {
        let mut c = Chorus::new();
        c.set_sample_rate(FS);
        c.set_mode(Mode::I);
        let n = (FS / RATE_I_HZ) as usize; // one cycle
        let lfo: Vec<f32> = (0..n)
            .map(|_| {
                let v = c.lfo();
                c.process(0.0);
                v
            })
            .collect();
        let (lo, hi) = lfo
            .iter()
            .fold((f32::INFINITY, f32::NEG_INFINITY), |(lo, hi), &v| {
                (lo.min(v), hi.max(v))
            });
        assert!(lo < -0.99 && hi > 0.99, "range {lo}..{hi}");

        let slope = RATE_I_HZ * 4.0 / FS; // a triangle's |d/dt| per sample
        // An apex falls between two samples, so its bend shows in two adjacent windows: count
        // clusters of bent windows, not windows.
        let bent: Vec<usize> = lfo
            .windows(3)
            .enumerate()
            .filter(|(_, w)| ((w[2] - w[1]) - (w[1] - w[0])).abs() > slope * 0.5)
            .map(|(i, _)| i)
            .collect();
        let corners = bent
            .iter()
            .enumerate()
            .filter(|(k, i)| *k == 0 || bent[k - 1] + 1 != **i)
            .count();
        assert_eq!(
            corners, 2,
            "a triangle has two corners per cycle; a sine bends everywhere: {bent:?}"
        );
        let straight = lfo
            .windows(2)
            .filter(|w| ((w[1] - w[0]).abs() - slope).abs() < slope * 0.05)
            .count();
        assert!(
            straight > n - 8,
            "only {straight} of {n} steps had the triangle's slope"
        );
    }
}
