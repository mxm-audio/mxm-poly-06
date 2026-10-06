//! One of the six voices: a DCO, the 80017A, and the one envelope that drives both.
//!
//! ```text
//! key ─► portamento ─► DCO (saw | pulse | sub | noise) ─► VCF ─► VCA ─► out
//!         (per voice)        ▲          ▲                 ▲       ▲
//!                            │  LFO ────┼─────────────────┤       │
//!                            │          │  one ADSR ──────┴───────┘  (or the gate)
//!                            └──────────┘  key tracking
//! ```
//!
//! # What is per voice, and what is not
//!
//! The DCO, the filter, the envelope, the portamento lag and the per-note pitch expression are
//! this voice's. The LFO is **not** — there is one for the whole instrument and it arrives as a
//! value — and neither are the HPF, the patch level, the chorus or the master volume, which sit
//! after the six are summed ([`crate::poly`]).
//!
//! # Voices differ, deliberately
//!
//! Each 80017A has its own component tolerances, so cutoff and level vary slightly from card to
//! card, and *which voice plays a note changes the sound*. That is what makes POLY 1 and POLY 2
//! audibly two modes. [`CUTOFF_OFFSET_CENTS`] and [`LEVEL_OFFSET_DB`] are six fixed constants — the
//! same on every instance, platform and build, because an export renders through a second instance
//! and must sound like the first — and the filter adds its own capacitor spread. **Chosen, not
//! measured**: nobody has characterised six real cards here.
//!
//! # Portamento glides from where this voice last was
//!
//! The hardware's portamento is a time and nothing else — no legato switch — and each voice's
//! pitch CV slews from wherever it sat, which under POLY 2's rotation is whatever note that voice
//! played six presses ago. A characteristic, not a defect, and kept. A voice that has never sounded
//! snaps, because there is nothing to glide from.
//!
//! # Per-note pitch expression rides on the lag
//!
//! Added **after** portamento, so a curve the host drew against one note arrives at the oscillator
//! as drawn rather than smeared by the RC. The same rule the mono instruments carry.
//!
//! # Modulation is routing
//!
//! Every modulation path is a route in [`crate::routing`], summed per voice from its own frame. The
//! machine's own paths are routes present in the init patch at their sliders' reach; with nothing
//! routed the voice opens no frame and takes no sum. The VCA's ENV/GATE switch is not a route: its
//! crossfade is a law no factor can express, which is the pilot's `vcasource` ruling.

use crate::dco::{Dco, Mix, Range};
use crate::envelope::{Adsr, Stage, ZERO_THRESHOLD};
use crate::filter::Ladder;
use crate::flush;
use crate::routing::{Graph, Routing, source, target};
use mxm_modulation::standard;

/// How many voices the instrument has. Not a parameter: seven notes steal.
pub const VOICES: usize = 6;

/// Full-scale filter envelope amount, in octaves. Chosen so the slider's top sweeps the whole
/// audible range from a closed filter.
pub const FILTER_ENV_OCTAVES: f32 = 7.0;
/// Full-scale filter LFO amount, in octaves.
pub const FILTER_LFO_OCTAVES: f32 = 3.0;
/// Full-scale LFO-to-pitch amount, in semitones.
pub const DCO_LFO_SEMITONES: f32 = 7.0;
/// How far the LFO can swing the pulse width either side of half, at full depth.
pub const PWM_SWING: f32 = 0.45;
/// Gate rise/fall time, and the time over which a VCA source change crossfades.
pub const GATE_TIME_S: f32 = 0.002;

/// Each card's cutoff offset, in cents. Chosen: small enough to be a chord's shimmer, not a tuning.
pub const CUTOFF_OFFSET_CENTS: [f32; VOICES] = [0.0, 18.0, -14.0, 9.0, -21.0, 12.0];
/// Each card's level offset, in dB. Chosen: under half a decibel either way.
pub const LEVEL_OFFSET_DB: [f32; VOICES] = [0.0, -0.3, 0.2, -0.15, 0.35, -0.4];

/// Everything a voice needs for one sample, as plain values.
///
/// Rebuilt per sample by the plugin shell from the framework's parameter smoothers, which is what
/// keeps modulation sample-accurate. `Copy` and free of any framework type.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct VoicePatch {
    pub range: Range,
    /// The tuning control plus the channel bend, already scaled by its sensitivity, in semitones.
    pub tune_semitones: f32,
    pub portamento_s: f32,

    /// The mod wheel, `0..=1`, on the channel the plugin reduces to. A routing source.
    pub wheel: f32,
    /// Channel pressure, `0..=1`, reduced the same way. A routing source.
    pub pressure: f32,
    /// The bender's position, `-1..=1`, reduced the same way. A routing source; its reach into
    /// pitch is already in [`VoicePatch::tune_semitones`].
    pub bend: f32,

    pub mix: Mix,
    /// The pulse width, `0..=1` — the base the pulse-width routes sum onto.
    pub pulse_width: f32,

    pub cutoff_hz: f32,
    pub resonance: f32,

    pub attack_s: f32,
    pub decay_s: f32,
    pub sustain: f32,
    pub release_s: f32,

    /// The VCA's ENV / GATE switch: `true` is GATE.
    pub vca_gate: bool,
}

impl Default for VoicePatch {
    fn default() -> Self {
        Self {
            range: Range::Eight,
            tune_semitones: 0.0,
            portamento_s: 0.0,
            wheel: 0.0,
            pressure: 0.0,
            bend: 0.0,
            mix: Mix {
                saw: 1.0,
                pulse: 0.0,
                sub: 0.0,
                noise: 0.0,
            },
            pulse_width: 0.5,
            cutoff_hz: 16_000.0,
            resonance: 0.0,
            attack_s: 0.002,
            decay_s: 0.4,
            sustain: 0.8,
            release_s: 0.3,
            vca_gate: false,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Voice {
    index: usize,
    dco: Dco,
    filter: Ladder,
    env: Adsr,

    /// The key this voice is sounding or releasing.
    note: f32,
    /// The portamento's **remaining distance** from `note`, in semitones: the lagged pitch is
    /// `note + glide_offset`. Kept as the distance rather than the pitch because the pitch form,
    /// `note + (glide − note) × coef`, stops moving in `f32` once a step is under half an ulp of the
    /// note — a glide at the two-second maximum came to rest 37 cents short at 48 kHz. The distance
    /// keeps full precision down to zero, so every glide lands.
    glide_offset: f32,
    /// Whether this voice has ever been given a note, so the first one snaps.
    ever_sounded: bool,
    /// Whether a key is holding this voice down. What the GATE mode follows.
    held: bool,
    /// Per-note pitch expression, in semitones. A new note clears it; a note-off does not.
    expression_semitones: f32,
    /// Smoothed gate, and smoothed VCA-source position (0 = envelope, 1 = gate).
    gate: f32,
    vca_mix: f32,
    /// The saw and pulse switches, smoothed. They are on/off on the machine and arrive as 0 or 1,
    /// but they multiply audio, and the collection's rule is *smooth signals, not coefficients*:
    /// a switch thrown mid-note must not step the waveform.
    saw_gain: f32,
    pulse_gain: f32,
    /// Set when a note begins on an idle voice, and cleared by the first sample it renders: that
    /// sample takes the smoothed switches' targets outright. See `process`.
    fresh: bool,
    /// The note-on velocity of the press this voice sounds, held for its life. A routing source.
    velocity: f32,
    /// This voice's routing: its own source frame and compacted routes.
    graph: Graph,

    cutoff_ratio: f32,
    level_gain: f32,
    sample_rate: f32,
    gate_coef: f32,
}

impl Voice {
    pub fn new(index: usize) -> Self {
        let index = index.min(VOICES - 1);
        let mut v = Self {
            index,
            dco: Dco::new(index as u32),
            filter: Ladder::new(index as u32),
            env: Adsr::new(),
            note: 60.0,
            glide_offset: 0.0,
            ever_sounded: false,
            held: false,
            expression_semitones: 0.0,
            gate: 0.0,
            vca_mix: 0.0,
            saw_gain: 0.0,
            pulse_gain: 0.0,
            fresh: true,
            // Full before any note, so the standard Velocity rests at zero.
            velocity: 1.0,
            graph: Graph::new(),
            cutoff_ratio: 2f32.powf(CUTOFF_OFFSET_CENTS[index] / 1200.0),
            level_gain: 10f32.powf(LEVEL_OFFSET_DB[index] / 20.0),
            sample_rate: 48_000.0,
            gate_coef: 0.0,
        };
        v.set_sample_rate(48_000.0);
        v
    }

    pub fn set_sample_rate(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate;
        self.env.set_sample_rate(sample_rate);
        self.gate_coef = (-1.0 / (GATE_TIME_S * sample_rate)).exp();
    }

    /// Clear every bit of state. Must leave no tail from the previous playback.
    pub fn reset(&mut self) {
        self.dco.reset(self.index as u32);
        self.filter.reset();
        self.env.reset();
        self.note = 60.0;
        self.glide_offset = 0.0;
        self.ever_sounded = false;
        self.held = false;
        self.expression_semitones = 0.0;
        self.gate = 0.0;
        self.vca_mix = 0.0;
        self.saw_gain = 0.0;
        self.pulse_gain = 0.0;
        self.fresh = true;
        self.velocity = 1.0;
        self.graph.reset();
    }

    pub fn index(&self) -> usize {
        self.index
    }

    /// Start `note` on this voice.
    ///
    /// `retrigger` is false for a repeated press of the key this voice already holds — the
    /// collection's legato joint, which retriggers nothing — and true for everything else,
    /// including a steal. A steal restarts the envelope from its current level, as an analogue
    /// envelope does, so it does not click.
    ///
    /// `velocity` is held for the voice's life as a routing source. A joint changes it no more than
    /// it retriggers.
    pub fn note_on(&mut self, note: u8, retrigger: bool, velocity: f32) {
        // A note on an idle voice starts fresh: its first sample takes the switches' current
        // targets rather than ramping from whatever they were when the voice last sounded. A
        // note on a voice that is sounding — a steal — is mid-sound, and ramps.
        if !self.is_active() {
            self.fresh = true;
        }
        self.held = true;
        if !retrigger {
            return;
        }
        // The glide starts from wherever this voice's pitch is, as a distance from the new note.
        let from = self.glided();
        self.note = f32::from(note);
        self.velocity = velocity;
        self.glide_offset = if self.ever_sounded {
            from - self.note
        } else {
            0.0
        };
        self.ever_sounded = true;
        self.expression_semitones = 0.0;
        self.env.trigger();
    }

    /// The pitch the portamento has reached, in semitones of MIDI note.
    fn glided(&self) -> f32 {
        self.note + self.glide_offset
    }

    /// The key came up. The envelope releases; in GATE mode the amplifier closes.
    pub fn release(&mut self) {
        self.held = false;
        self.env.release();
    }

    /// Ends an envelope that is still running behind a closed amplifier.
    ///
    /// Called by the instrument when it goes idle as a whole. A hidden release keeps time for as
    /// long as `process` is called, but a host stops calling it once the instrument reports idle,
    /// and a retrigger that depended on whether the host slept would be worse than one that always
    /// starts from zero. So idle is *defined*: an instrument reporting idle has every envelope at
    /// zero. Inaudible by construction, because the amplifier the envelope would reach is closed.
    pub fn retire_hidden_envelope(&mut self) {
        if !self.is_active() {
            self.env.silence();
        }
    }

    /// Choke: immediate, no release.
    pub fn silence(&mut self) {
        self.held = false;
        self.env.silence();
        self.gate = 0.0;
    }

    /// All sound off: [`Voice::silence`], and the ladder cleared too.
    ///
    /// A silenced voice is idle, and an idle voice skips its filter until its next note, so the
    /// ladder would otherwise keep what it was ringing with when the panic landed and hand it to
    /// that note, however long afterwards. A panic clears the state it owns. The DCO's phase, the
    /// portamento lag and the voice card's trims are kept.
    pub fn all_sound_off(&mut self) {
        self.silence();
        self.filter.reset();
    }

    pub fn set_expression(&mut self, semitones: f32) {
        self.expression_semitones = semitones;
    }

    pub fn is_held(&self) -> bool {
        self.held
    }

    /// Sounding or releasing: anything **audible** is still coming out of it.
    ///
    /// Audible through the amplifier as it is currently sourced: `amp = env + (gate - env) * mix`,
    /// so the envelope counts while the crossfade is on its side and the gate counts while it is on
    /// the gate's. In GATE mode a long release is a hidden filter sweep behind a closed amplifier,
    /// and counting it kept the voice rendering and the plugin reporting a tail long after the
    /// note had audibly ended — `plugins/AGENTS.md` says a tail reflects audible output. Found in
    /// review.
    ///
    /// **And a held key always counts**, whatever the amplifier is doing this sample: a voice whose
    /// gate has not yet risen — the first sample of a note, or a GATE-mode voice re-triggered after
    /// its gate had fallen — is about to be audible, and an `is_active` that said no would stop
    /// `process` from ever advancing the gate that makes it so. The first Gate-mode fix did exactly
    /// that and left a reused voice silent for good. Found in review.
    pub fn is_active(&self) -> bool {
        let env_audible = self.vca_mix < 0.999 && self.env.is_active();
        let gate_audible = self.vca_mix > 0.001 && self.gate > ZERO_THRESHOLD;
        self.held || env_audible || gate_audible
    }

    /// The key this voice last sounded, whether or not it is still held.
    pub fn note(&self) -> u8 {
        self.note as u8
    }

    /// The envelope's level, for the voice display. Read-only telemetry.
    pub fn env_level(&self) -> f32 {
        self.env.level()
    }

    pub fn env_stage(&self) -> Stage {
        self.env.stage()
    }

    /// Samples of audible tail remaining, post-VCA — through the amplifier as it is sourced, like
    /// [`Voice::is_active`].
    pub fn tail_samples(&self, release_s: f32) -> u32 {
        let env_tail = if self.vca_mix < 0.999 {
            self.env.tail_samples(release_s)
        } else {
            0
        };
        let gate_tail = if self.vca_mix > 0.001 && self.gate > ZERO_THRESHOLD {
            (GATE_TIME_S * self.sample_rate * 4.0) as u32
        } else {
            0
        };
        env_tail.max(gate_tail)
    }

    /// Rebuilds which routes are live on this voice. **Once per interval, never per sample** — and
    /// on every voice, idle ones included, or a voice's first note after allocation renders
    /// unrouted.
    pub fn set_topology(&mut self, routing: &Routing) {
        self.graph.set_topology(routing);
    }

    /// Whether any route is live into `target` on this voice, for tests.
    #[cfg(test)]
    pub(crate) fn routed_for_test(&self, target: usize) -> bool {
        !self.graph.is_empty(target)
    }

    /// What this voice's frame holds for `source`, for tests.
    #[cfg(test)]
    pub(crate) fn published_for_test(&self, source: usize) -> f32 {
        self.graph.read_for_test(source)
    }

    /// Render one sample. `lfo` is the global LFO's value in `-1..=1`, delay fade applied;
    /// `routing` carries the live routes' amounts, into the topology [`Voice::set_topology`] set.
    #[inline]
    pub fn process(&mut self, p: &VoicePatch, lfo: f32, routing: &Routing) -> f32 {
        let fs = self.sample_rate;
        if !self.is_active() {
            // An idle voice outputs exactly zero and skips the DCO and the filter, but **its
            // envelope and its portamento lag keep time**. In GATE mode a release runs on behind
            // the closed amplifier, and the next note's filter sweep starts from where that release
            // has actually got to, as it does on the machine, whose envelope never stops. Freezing
            // it here made a reused voice retrigger from a stale level. Found in review.
            if self.env.is_active() {
                self.env
                    .process(p.attack_s, p.decay_s, p.sustain, p.release_s);
            }
            if p.portamento_s > 0.0 && self.glide_offset != 0.0 {
                let coef = (-1.0 / (p.portamento_s * fs)).exp();
                self.glide_offset = flush(self.glide_offset * coef);
            }
            return 0.0;
        }

        if self.fresh {
            // **A note from idle takes its switch state outright.** The smoothed switches — the
            // VCA's source, the two waveforms — exist to avoid a step in the *sound*, and a voice
            // that was silent has no sound to step. Done here, on the note's first rendered
            // sample, rather than while idle: a host stops calling `process` once the instrument
            // reports idle, and a control changed during that sleep would otherwise reach the
            // next note as a two-millisecond crossfade over its first samples. Found in review.
            self.fresh = false;
            self.vca_mix = if p.vca_gate { 1.0 } else { 0.0 };
            self.saw_gain = p.mix.saw;
            self.pulse_gain = p.mix.pulse;
            self.gate = 0.0;
            // And its routing starts from nothing: a backward route would otherwise read the last
            // note's audio, from however long ago this voice fell silent. An idle voice publishes
            // nothing, so its frame is exactly that stale.
            self.graph.reset();
        }

        let env = self
            .env
            .process(p.attack_s, p.decay_s, p.sustain, p.release_s);

        // Portamento: this voice's own lag toward its own note. Before the sources are published,
        // because Key is the note the voice is sounding, glide and all.
        if p.portamento_s <= 0.0 {
            self.glide_offset = 0.0;
        } else {
            let coef = (-1.0 / (p.portamento_s * fs)).exp();
            self.glide_offset = flush(self.glide_offset * coef);
        }
        let glided = self.glided();

        // **Nothing routed costs nothing**: with no live route the frame is not opened, nothing is
        // published and no sum is taken. Otherwise the sources that exist by now are published in
        // `routing::source`'s order, the performance sources through the collection's standard so
        // each is zero at its rest; the DCO's own audio follows once it has run.
        let routed = self.graph.any_live();
        if routed {
            self.graph.begin_sample();
            self.graph.write(
                source::KEY,
                standard::key(glided, crate::routing::KEY_UNIT_SEMITONES),
            );
            self.graph.write(source::ENVELOPE, env);
            self.graph.write(source::LFO, lfo);
            self.graph
                .write(source::VELOCITY, standard::velocity(self.velocity));
            self.graph.write(source::WHEEL, standard::wheel(p.wheel));
            self.graph
                .write(source::PRESSURE, standard::pressure(p.pressure));
            self.graph.write(source::BEND, standard::bend(p.bend));
        }

        let pitch_st = glided
            + p.range.semitones()
            + p.tune_semitones
            + self.expression_semitones
            + if routed {
                self.graph.sum(target::PITCH, routing)
            } else {
                0.0
            };
        let freq_hz = 440.0 * ((pitch_st - 69.0) / 12.0).exp2();

        // The width routes sum onto the base width, and the DCO clamps what they reach.
        let width = p.pulse_width
            + if routed {
                self.graph.sum(target::PULSE_WIDTH, routing)
            } else {
                0.0
            };

        // The two switches ramp over the gate time rather than stepping — see the field doc.
        self.saw_gain = flush(p.mix.saw + (self.saw_gain - p.mix.saw) * self.gate_coef);
        self.pulse_gain = flush(p.mix.pulse + (self.pulse_gain - p.mix.pulse) * self.gate_coef);
        let mix = Mix {
            saw: self.saw_gain,
            pulse: self.pulse_gain,
            ..p.mix
        };
        let mixed = self.dco.process(freq_hz, width, &mix, fs);

        // The voice's own audio, published now that it exists: a backward route for pitch and width,
        // on time for cutoff and amplitude.
        if routed {
            let parts = self.dco.parts();
            self.graph.write(source::SAW, parts.saw);
            self.graph.write(source::PULSE, parts.pulse);
            self.graph.write(source::SUB, parts.sub);
            self.graph.write(source::NOISE, parts.noise);
        }

        // Cutoff modulation in octaves, which is the only way it stays consistent across the range.
        // The routes sum in source order — key, envelope, LFO, bend — the order the old expression
        // added its terms in, so the machine's own routes associate as it did.
        let octaves = if routed {
            self.graph.sum(target::CUTOFF, routing)
        } else {
            0.0
        };
        let cutoff = p.cutoff_hz * self.cutoff_ratio * octaves.exp2();

        let filtered = self.filter.process(mixed, cutoff, p.resonance, fs);

        // The gate follows the key; the VCA source crossfades rather than switching, so changing it
        // mid-note does not click. The envelope keeps running in GATE mode and still drives the
        // filter — that is how a filter sweep sits under a hard-edged note.
        let gate_target = if self.held { 1.0 } else { 0.0 };
        self.gate = flush(gate_target + (self.gate - gate_target) * self.gate_coef);
        let mix_target = if p.vca_gate { 1.0 } else { 0.0 };
        // Flushed like every other recursive state, and snapped to its target once within 1e-4
        // of it. Toward zero the exponential would otherwise end in subnormals; toward one it
        // **stalls** first — at about 48 ulps short, where the per-sample decrement rounds back
        // to the value it started from — so the snap sits well above that, and 1e-4 of a
        // crossfade position is inaudible. Measured, not conjectured.
        self.vca_mix = flush(mix_target + (self.vca_mix - mix_target) * self.gate_coef);
        if (self.vca_mix - mix_target).abs() < 1e-4 {
            self.vca_mix = mix_target;
        }
        let amp = env + (self.gate - env) * self.vca_mix;
        // **Amplitude routes scale the envelope-or-gate result and never add to it**, so a voice's
        // life stays the envelope's and the gate's to decide: no route can make a finished voice
        // sound. The collection's one amplitude law, silence to double, however many routes.
        let amp = if routed {
            amp * standard::amplitude_factor(self.graph.sum(target::AMPLITUDE, routing))
        } else {
            amp
        };

        flush(filtered * amp * self.level_gain)
    }
}

#[cfg(test)]
// A test patch reads better as "the default, then the three things this test changes" than as a
// struct literal that names them out of order.
#[allow(clippy::field_reassign_with_default)]
mod tests {
    use super::*;

    const FS: f32 = 48_000.0;

    fn voice(index: usize) -> Voice {
        let mut v = Voice::new(index);
        v.set_sample_rate(FS);
        v
    }

    /// Fundamental from rising zero crossings, **interpolated**: the time between the first and the
    /// last crossing over the number of cycles between them. Counting crossings alone is only as
    /// fine as one cycle per window, which at 110 Hz over half a second is six cents.
    fn fundamental_hz(samples: &[f32]) -> f32 {
        let crossings: Vec<f32> = samples
            .windows(2)
            .enumerate()
            .filter(|(_, w)| w[0] <= 0.0 && w[1] > 0.0)
            .map(|(i, w)| i as f32 + w[0] / (w[0] - w[1]))
            .collect();
        assert!(
            crossings.len() >= 3,
            "too few cycles to measure: {}",
            crossings.len()
        );
        let cycles = (crossings.len() - 1) as f32;
        cycles * FS / (crossings[crossings.len() - 1] - crossings[0])
    }

    /// Nothing routed.
    const NONE: Routing = Routing::new();

    fn render(v: &mut Voice, p: &VoicePatch, n: usize) -> Vec<f32> {
        (0..n).map(|_| v.process(p, 0.0, &NONE)).collect()
    }

    /// Renders with `routes`, which the caller has already set as this voice's topology.
    fn render_routed(v: &mut Voice, p: &VoicePatch, routes: &Routing, n: usize) -> Vec<f32> {
        (0..n).map(|_| v.process(p, 0.0, routes)).collect()
    }

    #[test]
    fn an_idle_voice_outputs_exactly_zero() {
        let mut v = voice(0);
        for _ in 0..10_000 {
            assert_eq!(v.process(&VoicePatch::default(), 0.5, &NONE), 0.0);
        }
    }

    #[test]
    fn a_note_sounds_at_its_pitch_and_the_range_switch_moves_octaves() {
        let mut p = VoicePatch::default();
        p.attack_s = 0.001;
        for (range, expected) in [
            (Range::Sixteen, 110.0f32),
            (Range::Eight, 220.0),
            (Range::Four, 440.0),
        ] {
            p.range = range;
            let mut v = voice(0);
            v.note_on(57, true, 0.8);
            let out = render(&mut v, &p, 24_000);
            let hz = fundamental_hz(&out[4_000..]);
            let cents = 1200.0 * (hz / expected).log2();
            assert!(
                cents.abs() < 3.0,
                "{range:?}: {hz} Hz, expected {expected} ({cents:.1} cents)"
            );
        }
    }

    #[test]
    fn a_release_returns_to_exactly_zero_and_the_voice_goes_idle() {
        let mut v = voice(1);
        let p = VoicePatch::default();
        v.note_on(60, true, 0.8);
        render(&mut v, &p, 12_000);
        v.release();
        let tail = render(&mut v, &p, (FS * 3.0) as usize);
        assert_eq!(*tail.last().unwrap(), 0.0);
        assert!(!v.is_active());
    }

    #[test]
    fn the_tail_estimate_is_conservative_and_finite() {
        let mut v = voice(2);
        let p = VoicePatch::default();
        v.note_on(60, true, 0.8);
        render(&mut v, &p, 12_000);
        v.release();
        let predicted = v.tail_samples(p.release_s);
        let mut actual = 0u32;
        while v.is_active() && actual < FS as u32 * 10 {
            v.process(&p, 0.0, &NONE);
            actual += 1;
        }
        assert!(
            predicted >= actual * 9 / 10,
            "predicted {predicted}, actual {actual}"
        );
        assert!(
            predicted <= actual * 3 / 2 + 512,
            "predicted {predicted}, actual {actual}"
        );
    }

    /// **A steal does not click.** The envelope restarts from where it is.
    #[test]
    fn a_steal_mid_release_has_no_discontinuity() {
        let mut v = voice(3);
        let p = VoicePatch::default();
        v.note_on(60, true, 0.8);
        render(&mut v, &p, 12_000);
        v.release();
        render(&mut v, &p, 2_000);
        let before = render(&mut v, &p, 1);
        v.note_on(64, true, 0.8);
        let after = render(&mut v, &p, 64);
        let worst = std::iter::once(before[0])
            .chain(after.iter().copied())
            .collect::<Vec<_>>()
            .windows(2)
            .map(|w| (w[1] - w[0]).abs())
            .fold(0.0f32, f32::max);
        assert!(worst < 0.2, "a jump of {worst} at the steal");
    }

    #[test]
    fn a_repeated_press_of_the_held_key_retriggers_nothing() {
        let mut v = voice(0);
        let p = VoicePatch::default();
        v.note_on(60, true, 0.8);
        render(&mut v, &p, 24_000);
        let stage = v.env_stage();
        let level = v.env_level();
        v.note_on(60, false, 0.8);
        assert_eq!(v.env_stage(), stage);
        assert_eq!(v.env_level(), level);
    }

    /// The characteristic: a voice glides from its **own** last pitch.
    #[test]
    fn portamento_glides_from_this_voices_last_note() {
        let mut p = VoicePatch::default();
        p.portamento_s = 0.3;
        p.attack_s = 0.001;
        let mut v = voice(0);
        v.note_on(48, true, 0.8);
        render(&mut v, &p, 24_000);
        v.release();
        render(&mut v, &p, (FS * 2.0) as usize);
        assert!(!v.is_active());

        // A new note a whole octave up, after this voice has been silent: it still slides.
        v.note_on(60, true, 0.8);
        let early = render(&mut v, &p, 2_400);
        // 0.3 s is a time constant, not a duration (`docs/modulation/04`): three seconds is ten
        // of them, and only then is the pitch where it is going.
        let late = render(&mut v, &p, (FS * 3.0) as usize);
        let hz_early = fundamental_hz(&early[400..]);
        let hz_late = fundamental_hz(&late[late.len() - 24_000..]);
        assert!(
            hz_early < 200.0,
            "the glide should start near C3's 131 Hz: {hz_early}"
        );
        assert!((hz_late - 261.6).abs() < 1.0, "and land on C4: {hz_late}");
    }

    /// **A glide lands exactly on its note**, at the longest portamento and at a high rate. The
    /// pitch form, `note + (glide − note) × coef`, stopped moving once a step fell under half an
    /// ulp of the note: at two seconds it came to rest 37 cents short at 48 kHz and 73 at 96 kHz,
    /// until the next note. Run mostly while idle, where the lag keeps time just the same, so
    /// twenty time constants cost little.
    ///
    /// Falsified before trusted: with the pitch form back, it rests at 71.63 at 48 kHz.
    #[test]
    fn a_glide_lands_exactly_on_its_note() {
        for rate in [48_000.0f32, 96_000.0] {
            let p = VoicePatch {
                portamento_s: 2.0,
                release_s: 0.01,
                ..VoicePatch::default()
            };
            let mut v = Voice::new(0);
            v.set_sample_rate(rate);
            v.note_on(48, true, 1.0);
            render(&mut v, &p, 16);
            v.note_on(72, true, 1.0);
            render(&mut v, &p, 4_800);
            v.release();
            render(&mut v, &p, (40.0 * rate) as usize);
            assert_eq!(v.glided(), 72.0, "at {rate} Hz the glide rests short");
        }
    }

    #[test]
    fn a_voice_that_has_never_sounded_snaps_to_its_first_note() {
        let mut p = VoicePatch::default();
        p.portamento_s = 0.5;
        p.attack_s = 0.001;
        let mut v = voice(4);
        v.note_on(69, true, 0.8);
        let out = render(&mut v, &p, 4_800);
        let hz = fundamental_hz(&out[800..]);
        assert!((hz - 440.0).abs() < 5.0, "{hz}");
    }

    /// The machine's constraint: one envelope, and in GATE mode it still drives the filter.
    #[test]
    fn gate_mode_is_hard_edged_and_the_filter_still_sweeps() {
        let mut p = VoicePatch::default();
        p.vca_gate = true;
        p.attack_s = 0.5;
        p.decay_s = 0.5;
        p.sustain = 0.2;
        p.cutoff_hz = 200.0;
        let routes = Routing::from_pairs(&[(target::CUTOFF, source::ENVELOPE, 1.0)]);
        let mut v = voice(0);
        v.set_topology(&routes);
        v.note_on(48, true, 0.8);
        let out = render_routed(&mut v, &p, &routes, 48_000);
        // Amplitude: full within a few milliseconds, not a half-second swell.
        let first = out[..480].iter().fold(0.0f32, |m, s| m.max(s.abs()));
        let later = out[20_000..24_000]
            .iter()
            .fold(0.0f32, |m, s| m.max(s.abs()));
        assert!(
            first > 0.3 * later,
            "the gate should open at once: {first} against {later}"
        );
        // Brightness: moves with the envelope even though the level does not.
        let bright =
            |s: &[f32]| s.windows(2).map(|w| (w[1] - w[0]).abs()).sum::<f32>() / s.len() as f32;
        let early = bright(&out[2_000..6_000]);
        let peak = bright(&out[22_000..26_000]);
        assert!(
            peak > early * 1.3,
            "the filter did not sweep under the gate: {early} then {peak}"
        );
    }

    /// The envelope's polarity is its route's sign: a negative amount closes the filter as the
    /// envelope rises, which is what the retired ENV polarity switch did.
    #[test]
    fn a_negative_envelope_route_inverts_the_sweep() {
        let mut p = VoicePatch::default();
        p.cutoff_hz = 800.0;
        p.attack_s = 0.3;
        p.sustain = 1.0;
        let bright =
            |s: &[f32]| s.windows(2).map(|w| (w[1] - w[0]).abs()).sum::<f32>() / s.len() as f32;
        let run = |amount: f32| {
            let routes = Routing::from_pairs(&[(target::CUTOFF, source::ENVELOPE, amount)]);
            let mut v = voice(0);
            v.set_topology(&routes);
            v.note_on(48, true, 0.8);
            let out = render_routed(&mut v, &p, &routes, 24_000);
            (bright(&out[1_000..3_000]), bright(&out[20_000..24_000]))
        };
        let (up_early, up_late) = run(0.6);
        let (down_early, down_late) = run(-0.6);
        assert!(
            up_late > up_early,
            "a positive route opens the filter as the envelope rises"
        );
        assert!(
            down_late < down_early,
            "a negative one closes it: {down_early} then {down_late}"
        );
    }

    #[test]
    fn six_voices_are_six_slightly_different_cards() {
        let p = VoicePatch::default();
        let renders: Vec<Vec<f32>> = (0..VOICES)
            .map(|i| {
                let mut v = voice(i);
                v.note_on(60, true, 0.8);
                render(&mut v, &p, 8_000)
            })
            .collect();
        for i in 0..VOICES {
            for j in i + 1..VOICES {
                assert_ne!(renders[i], renders[j], "voices {i} and {j} are identical");
            }
        }
        // But the same card twice is bit-identical — the offsets are constants.
        let mut a = voice(3);
        let mut b = voice(3);
        a.note_on(60, true, 0.8);
        b.note_on(60, true, 0.8);
        assert_eq!(render(&mut a, &p, 8_000), render(&mut b, &p, 8_000));
    }

    #[test]
    fn expression_moves_the_pitch_exactly_as_the_tuning_does() {
        let mut tuned = VoicePatch::default();
        tuned.tune_semitones = 7.0;
        let expressed = VoicePatch::default();
        let mut a = voice(0);
        a.note_on(48, true, 0.8);
        let mut b = voice(0);
        b.note_on(48, true, 0.8);
        b.set_expression(7.0);
        assert_eq!(
            render(&mut a, &tuned, 12_000),
            render(&mut b, &expressed, 12_000)
        );
    }

    #[test]
    fn no_nan_across_a_parameter_sweep_at_every_rate() {
        for fs in [44_100.0f32, 48_000.0, 96_000.0, 192_000.0] {
            let mut v = Voice::new(5);
            v.set_sample_rate(fs);
            v.note_on(120, true, 1.0);
            let mut p = VoicePatch::default();
            p.mix = Mix {
                saw: 1.0,
                pulse: 1.0,
                sub: 1.0,
                noise: 1.0,
            };
            for (cutoff, res, env, lfo) in [
                (20.0f32, 1.0f32, 1.0f32, 1.0f32),
                (20_000.0, 1.0, -1.0, -1.0),
                (1_000.0, 0.5, 0.0, 0.0),
            ] {
                p.cutoff_hz = cutoff;
                p.resonance = res;
                // The machine's own routes at their limits, and the voice's audio into every target.
                let routes = Routing::from_pairs(&[
                    (target::CUTOFF, source::ENVELOPE, env),
                    (target::CUTOFF, source::LFO, 1.0),
                    (target::PITCH, source::LFO, 1.0),
                    (target::PULSE_WIDTH, source::LFO, 1.0),
                    (target::PITCH, source::SAW, 1.0),
                    (target::CUTOFF, source::NOISE, 1.0),
                    (target::AMPLITUDE, source::PULSE, 1.0),
                ]);
                v.set_topology(&routes);
                for _ in 0..4_000 {
                    let y = v.process(&p, lfo, &routes);
                    assert!(y.is_finite(), "{fs}: cutoff {cutoff} res {res}");
                }
            }
        }
    }

    /// The VCA crossfade is a recursive state like any other: it reaches its target exactly and
    /// never sits in subnormals.
    #[test]
    fn the_vca_crossfade_reaches_its_target_exactly() {
        let mut p = VoicePatch::default();
        p.vca_gate = true;
        let mut v = voice(0);
        v.note_on(60, true, 0.8);
        render(&mut v, &p, 4_800);
        assert_eq!(v.vca_mix, 1.0, "toward the gate");
        p.vca_gate = false;
        render(&mut v, &p, 48_000);
        assert_eq!(
            v.vca_mix, 0.0,
            "and toward the envelope, exactly, not a subnormal"
        );
    }

    /// A control moved while the voice is idle is in place when the next note starts, not
    /// crossfaded over its first two milliseconds — **in the host's order**: the mode changes while
    /// nothing is being processed, and the note-on arrives before the next `process` call.
    #[test]
    fn a_source_switched_while_idle_is_in_place_for_the_next_note() {
        let rms = |x: &[f32]| (x.iter().map(|v| v * v).sum::<f32>() / x.len() as f32).sqrt();
        let mut p = VoicePatch::default();
        p.vca_gate = true;
        p.attack_s = 0.5;
        let mut reused = voice(1);
        reused.note_on(60, true, 0.8);
        render(&mut reused, &p, 4_800);
        reused.release();
        // Long enough for the hidden release to finish too, so the two voices below start their
        // envelopes from the same place and the only thing that could differ is the switch.
        render(&mut reused, &p, 96_000);
        assert!(!reused.is_active(), "the premise: idle");
        assert_eq!(
            reused.env_level(),
            0.0,
            "and the hidden release has finished"
        );

        // The mode changes with no `process` in between, then the note.
        p.vca_gate = false;
        reused.note_on(64, true, 0.8);
        let onset = rms(&render(&mut reused, &p, 480));

        // What a voice that never had the gate does with the same note. Not bit-identical — the
        // reused DCO's phase is wherever it got to — so a level comparison with room for that.
        let mut fresh = voice(1);
        fresh.note_on(64, true, 0.8);
        let expected = rms(&render(&mut fresh, &p, 480));

        assert!(
            onset < expected * 1.5 && onset > expected / 1.5,
            "ten milliseconds in, the reused voice is at {onset} where a fresh one is at {expected}: \
             the old gate leaked through the switch"
        );
    }

    /// The hidden envelope keeps time while a GATE-mode voice is inaudible, so a reused voice
    /// retriggers from the level a continuously running envelope would have: the same level an
    /// ENVELOPE-mode voice, which stays audible and active, has at the same moment.
    #[test]
    fn a_gate_mode_voices_hidden_envelope_keeps_time_while_it_is_idle() {
        let mut heard = VoicePatch::default();
        heard.vca_gate = false;
        heard.release_s = 6.0;
        let mut hidden = heard;
        hidden.vca_gate = true;
        let mut a = voice(0);
        let mut b = voice(0);
        for (v, p) in [(&mut a, &heard), (&mut b, &hidden)] {
            v.note_on(60, true, 0.8);
            render(v, p, 12_000);
            v.release();
            render(v, p, 48_000);
        }
        assert!(
            a.is_active() && !b.is_active(),
            "the premise: one audible, one idle"
        );
        assert!(
            (a.env_level() - b.env_level()).abs() < 1e-4,
            "the hidden envelope has stalled: audible {} against hidden {}",
            a.env_level(),
            b.env_level()
        );
    }

    /// A GATE-mode voice released until idle sounds again on its next note — the regression the
    /// first Gate-mode fix introduced: inactive by the gate, active by the hidden envelope, and
    /// stuck between the two.
    #[test]
    fn a_gate_mode_voice_released_until_idle_sounds_its_next_note() {
        let mut p = VoicePatch::default();
        p.vca_gate = true;
        p.release_s = 10.0;
        let mut v = voice(2);
        v.note_on(60, true, 0.8);
        render(&mut v, &p, 12_000);
        v.release();
        render(&mut v, &p, 4_800);
        assert!(
            !v.is_active(),
            "the premise: the gate closed and the voice went idle"
        );

        v.note_on(64, true, 0.8);
        let again = render(&mut v, &p, 4_800);
        let peak = again.iter().fold(0.0f32, |m, s| m.max(s.abs()));
        assert!(peak > 0.05, "the reused voice must sound: peak {peak}");

        // And after a choke, in either mode.
        for gate in [true, false] {
            p.vca_gate = gate;
            let mut v = voice(3);
            v.note_on(60, true, 0.8);
            render(&mut v, &p, 4_800);
            v.silence();
            assert!(!v.is_active());
            v.note_on(62, true, 0.8);
            let peak = render(&mut v, &p, 4_800)
                .iter()
                .fold(0.0f32, |m, s| m.max(s.abs()));
            assert!(
                peak > 0.05,
                "after a choke the voice must sound again (gate {gate}): {peak}"
            );
        }
    }

    /// In GATE mode a long release is inaudible, so it is not a tail.
    #[test]
    fn gate_mode_ends_with_the_key_and_not_with_the_hidden_release() {
        let mut p = VoicePatch::default();
        p.vca_gate = true;
        p.release_s = 10.0;
        let mut v = voice(0);
        v.note_on(60, true, 0.8);
        render(&mut v, &p, 12_000);
        v.release();
        assert!(
            v.tail_samples(p.release_s) < (FS * 0.05) as u32,
            "the tail must be the gate's few milliseconds, not the envelope's ten seconds: {}",
            v.tail_samples(p.release_s)
        );
        render(&mut v, &p, 2_400);
        assert!(
            !v.is_active(),
            "fifty milliseconds after the key lifted the voice is done"
        );

        // And in ENVELOPE mode the same release is audible and counts.
        p.vca_gate = false;
        let mut v = voice(0);
        v.note_on(60, true, 0.8);
        render(&mut v, &p, 12_000);
        v.release();
        render(&mut v, &p, 2_400);
        assert!(v.is_active(), "the envelope's release is audible here");
    }

    /// Throwing a waveform switch mid-note ramps rather than steps.
    #[test]
    fn switching_a_waveform_mid_note_does_not_click() {
        let mut on = VoicePatch::default();
        on.mix.saw = 1.0;
        on.mix.pulse = 0.0;
        on.attack_s = 0.001;
        let mut both = on;
        both.mix.pulse = 1.0;
        let mut v = voice(0);
        v.note_on(45, true, 0.8);
        render(&mut v, &on, 12_000);
        let before = render(&mut v, &on, 1)[0];
        let after = render(&mut v, &both, 64);
        let worst = std::iter::once(before)
            .chain(after.iter().copied())
            .collect::<Vec<_>>()
            .windows(2)
            .map(|w| (w[1] - w[0]).abs())
            .fold(0.0f32, f32::max);
        // A 110 Hz saw at 48 kHz moves about 0.005 per sample between its edges; the pulse arriving
        // as a step would add 0.5 at once. The ramp keeps the join under a tenth of that.
        assert!(worst < 0.06, "a jump of {worst} when the pulse switched on");
    }

    #[test]
    fn reset_leaves_no_tail() {
        let mut v = voice(0);
        let p = VoicePatch::default();
        v.note_on(60, true, 0.8);
        render(&mut v, &p, 4_000);
        v.reset();
        assert!(!v.is_active());
        assert_eq!(v.process(&p, 0.0, &NONE), 0.0);
    }
    /// **A voice starting from idle reads no audio from its last note.** An idle voice publishes
    /// nothing, so its frame still holds the finished note's saw; a backward route would read it on
    /// the next note's first sample. The reference is the same voice with its frame cleared by hand.
    #[test]
    fn a_voice_starting_from_idle_reads_no_audio_from_its_last_note() {
        let routes = Routing::from_pairs(&[(target::PITCH, source::SAW, 1.0)]);
        let p = VoicePatch::default();
        let mut v = voice(0);
        v.set_topology(&routes);
        v.note_on(60, true, 0.8);
        render_routed(&mut v, &p, &routes, 4_800);
        v.release();
        render_routed(&mut v, &p, &routes, 96_000);
        assert!(!v.is_active(), "the premise: idle");
        assert_ne!(
            v.graph.read_for_test(source::SAW),
            0.0,
            "the premise: the frame still holds the finished note's saw"
        );

        let mut clean = v.clone();
        clean.graph.reset();
        v.note_on(64, true, 0.8);
        clean.note_on(64, true, 0.8);
        assert_eq!(
            render_routed(&mut v, &p, &routes, 256),
            render_routed(&mut clean, &p, &routes, 256),
            "a reused voice must render as one whose frame starts empty"
        );
    }

    /// **Velocity follows the press, and a legato joint keeps it.** A routing source held for the
    /// voice's life: a retriggering press brings its own velocity, and a repeated press of the held
    /// key — the collection's joint, which retriggers nothing — changes it no more than it retriggers
    /// (`plans/plan-mxm-poly-06-modulation.md` §10).
    #[test]
    fn velocity_follows_the_press_and_a_legato_joint_keeps_it() {
        let routes = Routing::from_pairs(&[(target::CUTOFF, source::VELOCITY, 1.0)]);
        let p = VoicePatch::default();
        let mut v = voice(0);
        v.set_topology(&routes);
        // Published as the standard's `v − 1`.
        v.note_on(60, true, 0.25);
        render_routed(&mut v, &p, &routes, 16);
        assert_eq!(v.graph.read_for_test(source::VELOCITY), -0.75);
        v.note_on(60, false, 0.875);
        render_routed(&mut v, &p, &routes, 16);
        assert_eq!(
            v.graph.read_for_test(source::VELOCITY),
            -0.75,
            "a joint keeps the press's velocity"
        );
        v.note_on(64, true, 0.5);
        render_routed(&mut v, &p, &routes, 16);
        assert_eq!(
            v.graph.read_for_test(source::VELOCITY),
            -0.5,
            "a new press brings its own"
        );
    }

    /// **The declared evaluation order, rendered.** A route from the voice's own audio is one
    /// sample late into pitch — the DCO has not run when pitch is summed — and on time into cutoff.
    #[test]
    fn a_route_from_the_voices_audio_is_a_sample_late_into_pitch_and_on_time_into_cutoff() {
        let mut p = VoicePatch::default();
        p.attack_s = 0.001;
        p.cutoff_hz = 2_000.0;
        p.mix.noise = 1.0;
        let run = |routes: &Routing| {
            let mut v = voice(0);
            v.set_topology(routes);
            v.note_on(60, true, 0.8);
            render_routed(&mut v, &p, routes, 64)
        };
        let plain = run(&NONE);
        let into_pitch = run(&Routing::from_pairs(&[(target::PITCH, source::NOISE, 1.0)]));
        let into_cutoff = run(&Routing::from_pairs(&[(
            target::CUTOFF,
            source::NOISE,
            1.0,
        )]));
        assert_eq!(
            into_pitch[0], plain[0],
            "pitch reads last sample's noise, and a fresh voice has none"
        );
        assert_ne!(
            into_pitch, plain,
            "and from the next sample it is modulated"
        );
        assert_ne!(
            into_cutoff[0], plain[0],
            "cutoff reads this sample's noise, on the first sample"
        );
    }

    /// **An amplitude route scales the envelope, and a released voice still ends exactly.** The LFO
    /// held at full doubles the level; after release the voice reaches exact zero, with no step at
    /// its end, and goes idle.
    #[test]
    fn an_amplitude_route_scales_the_envelope_and_a_released_voice_still_ends_exactly() {
        let routes = Routing::from_pairs(&[(target::AMPLITUDE, source::LFO, 1.0)]);
        let mut p = VoicePatch::default();
        p.release_s = 0.2;
        let mut plain = voice(0);
        let mut routed = voice(0);
        routed.set_topology(&routes);
        plain.note_on(60, true, 0.8);
        routed.note_on(60, true, 0.8);
        for _ in 0..12_000 {
            let a = plain.process(&p, 1.0, &NONE);
            let b = routed.process(&p, 1.0, &routes);
            if a.abs() > 1e-3 {
                assert!((b / a - 2.0).abs() < 1e-3, "{b} is not twice {a}");
            }
        }
        routed.release();
        let mut last = 1.0f32;
        let mut n = 0;
        while routed.is_active() {
            let y = routed.process(&p, 1.0, &routes);
            if y != 0.0 {
                last = y;
            }
            n += 1;
            assert!(n < 48_000 * 3, "a routed voice never ended");
        }
        assert!(last.abs() < 1e-3, "a step of {last} at the voice's end");
        assert_eq!(routed.process(&p, 1.0, &routes), 0.0);
    }
}
