//! What mxm-poly-06 can modulate, and with what.
//!
//! `plans/plan-mxm-poly-06-modulation.md` §2, under `plans/plan-modulation-routing.md` §9 M5. The
//! shared machinery is [`mxm_modulation`]; this module is the instrument's own declaration — its
//! **source list**, its **target list**, and each route's **full scale**. Those are per instrument by
//! design and are the one place a copy still says something about the machine it copies.
//!
//! # Six voices, one frame each
//!
//! `plan-modulation-routing.md` §4.1: a frame is per scope, and *"a global source is simply the same
//! value in every voice."* So each voice owns a [`Graph`] — its frame and its compacted routes — and
//! the one global source, the LFO, is published into every voice's frame. That is
//! `mxm-creative-sampler`'s choice, the collection's first polyphonic conversion, and it keeps the
//! shared crate untouched. The three channel sources — wheel, pressure and bend — keep the reduction
//! to one channel the plugin already made, so every existing patch still hears what it heard.
//!
//! # The machine's own modulation is the init patch
//!
//! The JUNO's five sliders that were modulation — the DCO's LFO, the PWM slider in LFO mode, and the
//! VCF's ENV, LFO and KYBD — and the bender's cheek into the VCF are routes present in the init patch
//! at zero depth, each at the reach its slider had ([`INIT_PRESENT`], [`FULL_SCALE`]). The envelope's
//! polarity is the sign of its route.
//!
//! # Everything else is the collection's standard
//!
//! Key, Velocity, Wheel, Pressure and Bend mean what they mean on every instrument, and a route the
//! machine never had reaches what it reaches on every instrument ([`mxm_modulation::standard`];
//! `plans/plan-modulation-standard.md`, piloted here). So Velocity is `v − 1`, and a route from it
//! does nothing at the hardest note; an added pitch route throws an octave, an added cutoff route
//! four; Amplitude is the standard factor, silence to double.

use mxm_modulation::standard::{self, Law, Offer, Performance, reach};
use mxm_modulation::{Compacted, SourceFrame};

use crate::voice::{DCO_LFO_SEMITONES, FILTER_ENV_OCTAVES, FILTER_LFO_OCTAVES, PWM_SWING};

/// Every source this instrument can route, in **declared evaluation order**.
///
/// The order is load-bearing twice over. **A route whose source comes later reads last sample's
/// value** — the unit delay that makes a player's cycle finite — and the voice's own audio is last
/// because it exists only after the DCO has run, so a route from it into pitch or pulse width is one
/// sample late (a comb at audio rate, declared rather than discovered). And **a sum runs over its
/// live routes in this order**: key, envelope and LFO come first because the old cutoff expression
/// added them in that order, with bend after them, so the machine's own routes associate exactly as
/// that expression did and only rounding the plan names can move.
pub mod source {
    /// The played note **after the voice's portamento**, as a signed distance from middle C over
    /// five octaves (`standard::key`). Voice-scoped.
    pub const KEY: usize = 0;
    /// The voice's one envelope, before the VCA's ENV/GATE choice. Unipolar. Voice-scoped.
    pub const ENVELOPE: usize = 1;
    /// The LFO, with its delay fade applied — what reaches the DCO. Bipolar. **Global**, published
    /// into every voice's frame.
    pub const LFO: usize = 2;
    /// The velocity of the press that last triggered the envelope, `v − 1` (`standard::velocity`):
    /// zero at the hardest note. Voice-scoped.
    pub const VELOCITY: usize = 3;
    /// The mod wheel, CC 1, on the channel the plugin reduces to. Unipolar.
    pub const WHEEL: usize = 4;
    /// Channel pressure, reduced the same way. Unipolar.
    pub const PRESSURE: usize = 5;
    /// The bender's position, reduced the same way. Bipolar.
    pub const BEND: usize = 6;
    /// The DCO's sawtooth, before the mixer. Audio rate.
    pub const SAW: usize = 7;
    /// The DCO's pulse at the current width, before the mixer. Audio rate.
    pub const PULSE: usize = 8;
    /// The sub-oscillator, before the mixer. Audio rate.
    pub const SUB: usize = 9;
    /// The noise the mixer uses — the same sample, never a second draw. Audio rate.
    pub const NOISE: usize = 10;
}

/// How many sources the instrument declares.
pub const SOURCES: usize = 11;

/// Their names, in source order, for the interface and for accessibility.
pub const SOURCE_NAMES: [&str; SOURCES] = [
    "Key", "Envelope", "LFO", "Velocity", "Wheel", "Pressure", "Bend", "Saw", "Pulse", "Sub",
    "Noise",
];

/// Every target this instrument declares — **the pilot's four** (plan D1), each inside a voice.
///
/// Resonance, the LFO's rate, the HPF, the patch level, the chorus and the volume are not targets:
/// the first two could be appended later without moving an id, and the rest sit after the voices are
/// summed, where a voice-scoped source has nothing to say.
pub mod target {
    /// DCO pitch, summed in semitones.
    pub const PITCH: usize = 0;
    /// Pulse width, summed as an offset onto the base width; the DCO clamps what it reaches.
    pub const PULSE_WIDTH: usize = 1;
    /// Filter cutoff, summed in octaves.
    pub const CUTOFF: usize = 2;
    /// The VCA. **A factor on the envelope-or-gate result**, never added to it.
    pub const AMPLITUDE: usize = 3;
}

/// How many targets the instrument declares.
pub const TARGETS: usize = 4;

/// Their names, in target order.
pub const TARGET_NAMES: [&str; TARGETS] = ["Pitch", "Pulse width", "Cutoff", "Amplitude"];

/// Which of the standard's performance sources each source is — `None` for the machine's own
/// generators and the voice's audio, which keep their own meaning.
pub const PERFORMANCE: [Option<Performance>; SOURCES] = [
    Some(Performance::Key),
    None,
    None,
    Some(Performance::Velocity),
    Some(Performance::Wheel),
    Some(Performance::Pressure),
    Some(Performance::Bend),
    None,
    None,
    None,
    None,
];

/// Each target's law, for the standard's offer: three sums and the amplitude factor.
pub const LAW: [Law; TARGETS] = [Law::Sum, Law::Sum, Law::Sum, Law::Factor];

/// Whether **the machine itself** has this path — one of [`INIT_PRESENT`] — so its reach is the
/// machine's rather than the standard's.
#[must_use]
pub const fn machine(target: usize, source: usize) -> bool {
    let mut i = 0;
    while i < INIT_PRESENT.len() {
        if INIT_PRESENT[i].0 == target && INIT_PRESENT[i].1 == source {
            return true;
        }
        i += 1;
    }
    false
}

/// Whether and how a pair is offered — `standard::offer`. Every target here sums or scales, so on
/// this instrument every pair is offered on both halves.
#[must_use]
pub const fn offer(target: usize, source: usize) -> Offer {
    standard::offer(LAW[target], PERFORMANCE[source], machine(target, source))
}

/// The key source's unit, in semitones: a voice publishes `(note − 60) / 60`, so one unit of Key is
/// five octaves of keyboard either side of middle C.
pub const KEY_UNIT_SEMITONES: f32 = 60.0;

/// How far a Key route reaches on the cutoff: one octave of cutoff per octave of keyboard.
///
/// The key source is published normalised over five octaves either side of middle C, so five octaves
/// of reach is unity tracking — the KYBD slider's top, and the pilot's arithmetic.
pub const KEY_OCTAVES: f32 = 5.0;

/// How far a Bend route reaches on the cutoff, in octaves: the range the retired `bendfilter`
/// sensitivity had.
pub const BEND_OCTAVES: f32 = 4.0;

/// Each route's full scale, in the target's own domain, at an amount of one — **per route, not per
/// target**.
///
/// A route is `(amount × source) × full_scale`, summed in that domain and converted once, which is
/// the instruction sequence these voices executed with the constant written into each expression.
///
/// # Why the table has a column per source
///
/// Because the machine's own wiring is not uniform, and `plan-modulation-routing.md` §5 takes the
/// conservative form: **a route the instrument itself wires keeps the reach it always had.** The
/// JUNO's envelope reaches seven octaves of cutoff and its LFO three, from the same cutoff; the
/// keyboard follows at one octave per octave; the bender's cheek reached four. An old slider
/// position is then the same number on the route that replaced it.
///
/// **Every route the machine did not have takes the collection's standard reach**
/// (`standard::reach`): an octave of pitch, twelve semitones per octave of keyboard from Key, four
/// octaves of cutoff, 45 % of width, the whole amplitude factor, and a fifth of a linear reach per
/// octave from Key. Before the standard, every added pitch route took the LFO slider's seven
/// semitones and every added cutoff route the envelope's seven octaves.
pub const FULL_SCALE: [[f32; SOURCES]; TARGETS] = {
    let key_linear = reach::KEY_LINEAR_FRACTION_PER_OCTAVE;
    // Semitones. The DCO's LFO slider is the machine's; Key tracks an octave per octave, so −100 %
    // holds one pitch across the keyboard.
    let mut pitch = [reach::PITCH_SEMITONES; SOURCES];
    pitch[source::LFO] = DCO_LFO_SEMITONES;
    pitch[source::KEY] =
        standard::key_scale(reach::KEY_PITCH_SEMITONES_PER_OCTAVE, KEY_UNIT_SEMITONES);
    // A width offset: the ± 0.45 the PWM slider swung in LFO mode, which the standard also takes.
    let mut width = [reach::WIDTH; SOURCES];
    width[source::LFO] = PWM_SWING;
    width[source::KEY] = standard::key_scale(reach::WIDTH * key_linear, KEY_UNIT_SEMITONES);
    // Octaves: the VCF's ENV, LFO and KYBD sliders and the bender's cheek are the machine's.
    let mut cutoff = [reach::OCTAVES; SOURCES];
    cutoff[source::ENVELOPE] = FILTER_ENV_OCTAVES;
    cutoff[source::LFO] = FILTER_LFO_OCTAVES;
    cutoff[source::KEY] = KEY_OCTAVES;
    cutoff[source::BEND] = BEND_OCTAVES;
    // The amplitude factor's swing: one route can silence the VCA or double it.
    let mut amplitude = [reach::AMPLITUDE; SOURCES];
    amplitude[source::KEY] = standard::key_scale(reach::AMPLITUDE * key_linear, KEY_UNIT_SEMITONES);
    [pitch, width, cutoff, amplitude]
};

/// The routes **the machine itself wires**, present in the init patch at zero depth.
///
/// Six paths that used to be six sliders — `dcolfo`, `pwmdepth` in LFO mode, `envamount` with its
/// polarity, `vcflfo`, `keytrack` and `bendfilter`. **Every one starts at zero**, as those sliders
/// did, so a fresh instance is still the copy and the routing is the freedom on top.
pub const INIT_PRESENT: [(usize, usize); 6] = [
    (target::PITCH, source::LFO),
    (target::PULSE_WIDTH, source::LFO),
    (target::CUTOFF, source::ENVELOPE),
    (target::CUTOFF, source::LFO),
    (target::CUTOFF, source::KEY),
    (target::CUTOFF, source::BEND),
];

/// Which sources are live into which targets, and how much of each.
///
/// **Presence is what the DSP reads.** An absent route contributes nothing whatever its amount holds,
/// which is what makes removing a source one parameter write and re-adding it restore the depth the
/// player last set.
#[derive(Debug, Clone, Copy)]
pub struct Routing {
    /// Per target, per source: whether that route exists.
    pub present: [[bool; SOURCES]; TARGETS],
    /// Per target, per source: how much, signed, as a fraction of that route's full scale.
    ///
    /// Only a **live** route's amount is refreshed by the caller each sample; an absent one is never
    /// read, which leaves its stored depth where the player put it.
    pub amounts: [[f32; SOURCES]; TARGETS],
    /// The live pairs, `(target, source)`, compacted by [`Routing::compact`] — built once per
    /// interval rather than walked per sample, because on the pilot walking the grid every sample
    /// cost more than everything else the routing added put together.
    live: [(u8, u8); TARGETS * SOURCES],
    live_len: usize,
    /// Which sources some live route reads. **A source nothing reads is not published.**
    needed: [bool; SOURCES],
}

impl Default for Routing {
    fn default() -> Self {
        Self::new()
    }
}

impl Routing {
    /// Nothing routed anywhere.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            present: [[false; SOURCES]; TARGETS],
            amounts: [[0.0; SOURCES]; TARGETS],
            live: [(0, 0); TARGETS * SOURCES],
            live_len: 0,
            needed: [false; SOURCES],
        }
    }

    /// The machine's own wiring at zero depth — [`INIT_PRESENT`]. What the init patch holds.
    #[must_use]
    pub fn init() -> Self {
        let mut routing = Self::new();
        for (t, s) in INIT_PRESENT {
            routing.present[t][s] = true;
        }
        routing.compact();
        routing
    }

    /// Exactly these routes, at these amounts, compacted: `(target, source, amount)`.
    #[must_use]
    pub fn from_pairs(pairs: &[(usize, usize, f32)]) -> Self {
        let mut routing = Self::new();
        for &(t, s, amount) in pairs {
            routing.present[t][s] = true;
            routing.amounts[t][s] = amount;
        }
        routing.compact();
        routing
    }

    /// Rebuilds the live list and the needed sources from [`Routing::present`]. **Once per
    /// interval, never per sample**, and every caller that changes `present` owes this before the
    /// next render.
    pub fn compact(&mut self) {
        self.live_len = 0;
        self.needed = [false; SOURCES];
        for (t, target) in self.present.iter().enumerate() {
            for (s, &on) in target.iter().enumerate() {
                if on {
                    self.live[self.live_len] = (t as u8, s as u8);
                    self.live_len += 1;
                    self.needed[s] = true;
                }
            }
        }
    }

    /// The live pairs, `(target, source)`, as compacted.
    #[inline]
    #[must_use]
    pub fn live(&self) -> &[(u8, u8)] {
        &self.live[..self.live_len]
    }

    /// Whether any route at all is live.
    #[inline]
    #[must_use]
    pub fn any(&self) -> bool {
        self.live_len > 0
    }

    /// Whether some live route reads this source.
    #[inline]
    #[must_use]
    pub fn needs(&self, source: usize) -> bool {
        self.needed[source]
    }
}

/// One voice's routing state: its own source frame, and one compacted list per target.
#[derive(Debug, Clone)]
pub struct Graph {
    frame: SourceFrame<SOURCES>,
    live: [Compacted<SOURCES>; TARGETS],
    /// Which sources the last topology had some live route read, so the next one can tell which have
    /// **just** become read. See [`Graph::set_topology`].
    needed: [bool; SOURCES],
    /// Whether any route is live, cached at [`Graph::set_topology`]. With nothing live the voice
    /// opens no frame, publishes nothing and takes no sum — the arithmetic it ran before routing.
    any: bool,
}

impl Default for Graph {
    fn default() -> Self {
        Self::new()
    }
}

impl Graph {
    /// An empty graph.
    #[must_use]
    pub fn new() -> Self {
        Self {
            frame: SourceFrame::new(),
            live: [const { Compacted::new() }; TARGETS],
            needed: [false; SOURCES],
            any: false,
        }
    }

    /// Rebuilds this voice's per-target lists. Call once per interval, never per sample.
    ///
    /// **A source that becomes needed starts from silence.** Publication is gated on need, so while
    /// nothing read a source nothing published it, and its slot still holds whatever it held the last
    /// time something did — possibly from a different phrase. A backward route added to a sounding
    /// voice would read that ancient value for exactly one sample, and how ancient would depend on how
    /// the host split its buffers. Clearing the slot makes the first read a deterministic zero
    /// (`crates/mxm-modulation/AGENTS.md`, *A gated publication owes a `clear`*).
    pub fn set_topology(&mut self, routing: &Routing) {
        for (target, live) in self.live.iter_mut().enumerate() {
            live.build(&routing.present[target]);
        }
        self.any = self.live.iter().any(|l| !l.is_empty());
        for (source, before) in self.needed.iter_mut().enumerate() {
            let now = routing.needs(source);
            if now && !*before {
                self.frame.clear(source);
            }
            *before = now;
        }
    }

    /// Whether anything is routed at all.
    #[inline]
    #[must_use]
    pub fn any_live(&self) -> bool {
        self.any
    }

    /// Whether no route is live into `target`.
    #[inline]
    #[must_use]
    pub fn is_empty(&self, target: usize) -> bool {
        self.live[target].is_empty()
    }

    /// Opens a sample. Every publication for it happens after this and before any read.
    #[inline]
    pub fn begin_sample(&mut self) {
        self.frame.begin_sample();
    }

    /// Publishes a source's value for this sample, **if anything reads it**.
    #[inline]
    pub fn write(&mut self, source: usize, value: f32) {
        if self.needed[source] {
            self.frame.write(source, value);
        }
    }

    /// This target's summed modulation, in its own domain, each route's full scale applied last.
    #[inline]
    #[must_use]
    pub fn sum(&self, target: usize, routing: &Routing) -> f32 {
        // Generous rather than tight: each target applies its own limit where it matters — the
        // filter clamps its cutoff, the DCO its width, `standard::amplitude_factor` the amplitude
        // sum at one — and a bound here as well would silently narrow what a player can reach.
        mxm_modulation::sum_scaled(
            &self.frame,
            &self.live[target],
            &routing.amounts[target],
            &FULL_SCALE[target],
            64.0,
        )
    }

    /// What the frame holds for one source, for tests.
    #[cfg(test)]
    pub(crate) fn read_for_test(&self, source: usize) -> f32 {
        self.frame.read(source)
    }

    /// Forgets every published value, so a reused voice cannot read a finished note's.
    pub fn reset(&mut self) {
        self.frame.reset();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A graph with exactly these routes, one sample opened and these sources published.
    fn graph_with(routing: &Routing, published: &[(usize, f32)]) -> Graph {
        let mut graph = Graph::new();
        graph.set_topology(routing);
        graph.begin_sample();
        for &(source, value) in published {
            graph.write(source, value);
        }
        graph
    }

    /// **Each route the machine wires reads the reach its slider had**, in the slider's own numbers:
    /// seven semitones, ± 0.45 of width, seven octaves, three octaves, one octave per octave, four
    /// octaves. The one defect a player meets on the first knob turned, and no audio assertion sees.
    #[test]
    fn each_route_the_machine_wires_reads_the_reach_its_slider_had() {
        assert_eq!(
            FULL_SCALE[target::PITCH][source::LFO],
            7.0,
            "DCO LFO, semitones"
        );
        assert_eq!(
            FULL_SCALE[target::PULSE_WIDTH][source::LFO],
            0.45,
            "PWM, width"
        );
        assert_eq!(
            FULL_SCALE[target::CUTOFF][source::ENVELOPE],
            7.0,
            "VCF ENV, octaves"
        );
        assert_eq!(
            FULL_SCALE[target::CUTOFF][source::LFO],
            3.0,
            "VCF LFO, octaves"
        );
        assert_eq!(
            FULL_SCALE[target::CUTOFF][source::KEY] / 60.0 * 12.0,
            1.0,
            "KYBD: one octave of cutoff per octave of keyboard, through the normalised key source"
        );
        assert_eq!(
            FULL_SCALE[target::CUTOFF][source::BEND],
            4.0,
            "bend into VCF, octaves"
        );
        assert!(FULL_SCALE[target::AMPLITUDE].iter().all(|&s| s == 1.0));

        let init = Routing::init();
        assert_eq!(init.live().len(), INIT_PRESENT.len());
        assert!(
            init.amounts.iter().flatten().all(|&a| a == 0.0),
            "every Init route at zero"
        );
    }

    /// **The machine's own routes are bit-identical to the expressions they replace.** Each old term
    /// was `(depth × source) × reach`, which is the route's instruction sequence, and the cutoff's
    /// terms are declared in the order the old expression added them — with the key and bend routes
    /// present at zero depth, as Init holds them.
    #[test]
    fn the_machines_own_routes_are_bit_identical_to_the_expressions_they_replace() {
        const DEPTHS: [f32; 6] = [0.0, 0.1, 0.33, 0.5, 0.9, 1.0];
        const VALUES: [f32; 7] = [-1.0, -0.62, -0.001, 0.0, 0.25, 0.7777, 1.0];
        for a in DEPTHS {
            for x in VALUES {
                // The DCO's LFO slider: `lfo × dcolfo × 7`.
                let r = Routing::from_pairs(&[(target::PITCH, source::LFO, a)]);
                let g = graph_with(&r, &[(source::LFO, x)]);
                let base = 57.25f32;
                assert_eq!(
                    (base + g.sum(target::PITCH, &r)).to_bits(),
                    (base + x * a * DCO_LFO_SEMITONES).to_bits(),
                    "pitch at depth {a}, LFO {x}"
                );

                // PWM in LFO mode: `0.5 + pwmdepth × lfo × 0.45`, over a base width of a half.
                let r = Routing::from_pairs(&[(target::PULSE_WIDTH, source::LFO, a)]);
                let g = graph_with(&r, &[(source::LFO, x)]);
                assert_eq!(
                    (0.5 + g.sum(target::PULSE_WIDTH, &r)).to_bits(),
                    (0.5 + a * x * PWM_SWING).to_bits(),
                    "width at depth {a}, LFO {x}"
                );

                // The VCF: envelope with its polarity, LFO, and key and bend at zero.
                for sign in [1.0f32, -1.0] {
                    let env = x.abs();
                    let mut r = Routing::init();
                    r.amounts[target::CUTOFF][source::ENVELOPE] = sign * a;
                    r.amounts[target::CUTOFF][source::LFO] = a;
                    let g = graph_with(
                        &r,
                        &[
                            (source::KEY, 12.0 / 60.0),
                            (source::ENVELOPE, env),
                            (source::LFO, x),
                            (source::BEND, x),
                        ],
                    );
                    let old = 0.0f32 * (72.0 - 60.0) / 12.0
                        + sign * a * env * FILTER_ENV_OCTAVES
                        + a * x * FILTER_LFO_OCTAVES
                        + x * 0.0;
                    assert_eq!(
                        g.sum(target::CUTOFF, &r).exp2().to_bits(),
                        old.exp2().to_bits(),
                        "cutoff at depth {a}, polarity {sign}, envelope {env}, LFO {x}"
                    );
                }
            }
        }
    }

    /// **Key tracking and bend into the cutoff are the two that are not bit-identical**, as the plan
    /// says: a different division for the key, a unit change for the bend. They agree to a hundredth
    /// of a cent of cutoff. Notes above C9 track no further, because the key source is bounded to
    /// five octaves either side of middle C.
    #[test]
    fn key_tracking_and_bend_reach_the_old_numbers_to_rounding() {
        for note in [0.0f32, 36.0, 59.0, 60.0, 61.0, 84.0, 120.0] {
            for depth in [0.0f32, 0.25, 0.5, 1.0] {
                let r = Routing::from_pairs(&[(target::CUTOFF, source::KEY, depth)]);
                let g = graph_with(&r, &[(source::KEY, (note - 60.0) / 60.0)]);
                let old = depth * (note - 60.0) / 12.0;
                let error = (g.sum(target::CUTOFF, &r) - old).abs();
                assert!(error < 1e-5, "key {note} at {depth}: {error} octaves off");
            }
        }
        for bend in [-1.0f32, -0.3, 0.0, 0.8, 1.0] {
            for sensitivity in [0.0f32, 0.5, 2.0, 4.0] {
                let r = Routing::from_pairs(&[(
                    target::CUTOFF,
                    source::BEND,
                    sensitivity / BEND_OCTAVES,
                )]);
                let g = graph_with(&r, &[(source::BEND, bend)]);
                let error = (g.sum(target::CUTOFF, &r) - bend * sensitivity).abs();
                assert!(
                    error < 1e-5,
                    "bend {bend} at {sensitivity}: {error} octaves off"
                );
            }
        }
    }

    /// **A source that becomes needed starts from silence, not from an old phrase.** The saw is
    /// published while a route reads it, unread for a long gap in which other routes keep the frame
    /// running, then read again by a backward route before this sample publishes it.
    #[test]
    fn a_source_that_becomes_needed_starts_from_silence_not_from_an_old_phrase() {
        let with = Routing::from_pairs(&[
            (target::PITCH, source::SAW, 1.0),
            (target::CUTOFF, source::ENVELOPE, 0.5),
        ]);
        let without = Routing::from_pairs(&[(target::CUTOFF, source::ENVELOPE, 0.5)]);
        let mut graph = Graph::new();
        graph.set_topology(&with);
        graph.begin_sample();
        graph.write(source::SAW, 0.7);

        graph.set_topology(&without);
        for _ in 0..10_000 {
            graph.begin_sample();
            graph.write(source::ENVELOPE, 0.3);
            graph.write(source::SAW, -0.9);
        }

        graph.set_topology(&with);
        graph.begin_sample();
        graph.write(source::ENVELOPE, 0.3);
        assert_eq!(
            graph.sum(target::PITCH, &with),
            0.0,
            "the first read of a newly read source must be silence, not the last phrase's 0.7"
        );
    }
}
