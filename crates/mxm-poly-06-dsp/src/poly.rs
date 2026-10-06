//! The instrument: six voices, the ledger that hands them out, and the chain after the sum.
//!
//! Nothing else in this collection allocates voices — both shipped instruments have one — so the
//! ledger is this crate's genuinely new module, and its rules are the plan's §5.1 table made code.
//!
//! # The ledger
//!
//! The unit of bookkeeping is a **press**: one note-on, remembered until its note-off arrives,
//! carrying its key (channel and note), its note id if the host gave one, and the **set of voices**
//! it went to — one in the POLY modes, all six in UNISON. A press whose voices have all since been
//! stolen stays in the ledger with an empty set: a **tombstone**, kept so that its late note-off
//! has something to land on other than the voice now sounding a re-pressed key.
//!
//! | Event | Rule |
//! |---|---|
//! | Note-on for a key no live press holds | Take a voice per the assign mode; none free → steal. A steal takes one voice out of every press's set that held it; only an emptied set becomes a tombstone |
//! | Note-on for a key a live press already holds | The voice continues, un-retriggered; the new press joins it. **The collection's legato joint**, not the hardware's — the player's export merges a same-pitch joint into a hold because this collection's voices define the joint as no-retrigger |
//! | Note-off with a note id | Retires the **oldest** press carrying that id, live or tombstone |
//! | Note-off without one | Retires the oldest press of that key, **tombstones first** — so a late note-off for a stolen press is absorbed and does not release the re-pressed key |
//! | Retiring a press | Each voice in its set releases when its last live press is gone; a tombstone changes nothing audible |
//! | A note-off matching nothing | Dropped |
//! | Per-note pitch expression | Reaches every press carrying its id (or key); a voice sounds its **newest** live press's, a new press clears it, a note-off never changes it |
//! | The assign mode changes while keys are held | Future allocation only; nothing is re-voiced or retired |
//! | The ledger is full | Evict the oldest tombstone, else the oldest press, retiring it as a note-off would. A release one press early, never a stuck note, never an allocation |
//!
//! # What is chosen, and unverified about the machine
//!
//! `research:instruments/juno-106.md` §7 gives the three assign modes and not what happens when a
//! seventh key arrives, nor what UNISON does with a second key, nor what retriggers the LFO delay.
//! This module decides: **POLY 1 steals from the oldest press**, **POLY 2 takes the next voice in
//! rotation whether or not it is free**, **UNISON is last-trigger with no fallback** (a second key takes all
//! six and the first key's press becomes a tombstone, so releasing the second does not return to the
//! first — unlike a conventional last-note-priority monosynth), and **the LFO delay restarts on the first key
//! after every key was released**. Each is the usual polysynth answer and none is measured on a 106.
//! The plugin's brief and this crate's NOTES.md record them as chosen.

use crate::chorus::{Chorus, Mode};
use crate::flush;
use crate::hpf::{Hpf, Position};
use crate::lfo::Lfo;
use crate::routing::Routing;
use crate::voice::{VOICES, Voice, VoicePatch};

/// How many presses the ledger holds. A host cannot hold more keys than it has, but it can send
/// note-ons with fresh ids for one key forever and never a note-off, so this is a bound with an
/// overflow rule and not a claim that it cannot fill.
pub const LEDGER_CAPACITY: usize = 64;

/// The KEY ASSIGN switch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Assign {
    /// Voices assigned starting from the first free one.
    #[default]
    Poly1,
    /// Voices rotate 1 to 6 continuously.
    Poly2,
    /// All six voices stack on one note.
    Unison,
}

/// A key: channel and note number. Presses on the same key are the same key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Key {
    pub channel: u8,
    pub note: u8,
}

#[derive(Debug, Clone, Copy)]
struct Press {
    key: Key,
    note_id: Option<i32>,
    /// A bitmask over the six voices. Empty is a tombstone.
    voices: u8,
    /// Monotonic: lower is older.
    order: u64,
}

impl Press {
    #[inline]
    fn is_tombstone(&self) -> bool {
        self.voices == 0
    }

    /// Whether a note-off or an expression naming `key` and `note_id` is about this press.
    ///
    /// The mono instruments' rule: **an id is authoritative when both sides have one**; otherwise
    /// the key decides. So an id-less note-off reaches an id-carrying press by key, and a note-off
    /// carrying an id this press does not carry is not about it, whatever the key says.
    #[inline]
    fn matches(&self, key: Key, note_id: Option<i32>) -> bool {
        match (self.note_id, note_id) {
            (Some(a), Some(b)) => a == b,
            _ => self.key == key,
        }
    }
}

/// How long after the last voice falls silent the post-mix chain can still carry anything: the
/// chorus's noise fading to its snap point (about 0.45 s) and the DC blocker's state flushing to
/// exact zero (about 0.5 s at 15 Hz). Both are inaudible long before this, and both are why the
/// instrument reports a tail past its envelopes: a `Tail` that ended while a state was still
/// non-zero would leave a host free to stop calling `process` mid-decay.
pub const POST_TAIL_S: f32 = 0.6;

/// What the six voices are summed at. Six cards at full level would otherwise reach six times a
/// single voice's peak, so this is the mixing constant that puts a full chord a little under full
/// scale at the patch level's default — the bus gain the hardware's summing resistors set, and
/// not a control.
pub const VOICE_SUM_GAIN: f32 = 1.0 / 3.0;

const ALL_VOICES: u8 = (1 << VOICES) - 1;

/// One-pole DC blocker on the voice sum, before the HPF.
///
/// A pulse wave is not symmetric unless its width is exactly 50%, so PWM swings the offset around,
/// and the HPF's flat position passes DC. This is the output stage's AC coupling. 15 Hz, below the
/// fundamental of the lowest note at 16', so it removes offset without thinning bass.
#[derive(Debug, Clone, Copy, Default)]
struct DcBlocker {
    x1: f32,
    y1: f32,
    r: f32,
}

impl DcBlocker {
    const CUTOFF_HZ: f32 = 15.0;

    fn set_sample_rate(&mut self, sample_rate: f32) {
        self.r = 1.0 - (std::f32::consts::TAU * Self::CUTOFF_HZ / sample_rate);
    }

    fn reset(&mut self) {
        self.x1 = 0.0;
        self.y1 = 0.0;
    }

    #[inline]
    fn process(&mut self, x: f32) -> f32 {
        let y = x - self.x1 + self.r * self.y1;
        self.x1 = x;
        self.y1 = flush(y);
        self.y1
    }
}

/// Everything the instrument needs for one sample, as plain values.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Patch {
    pub voice: VoicePatch,
    pub lfo_rate_hz: f32,
    pub lfo_delay_s: f32,
    pub hpf: Position,
    /// The patch's VCA, linear. Before the chorus, so it sets how hard the BBD is driven.
    pub level: f32,
    pub chorus: Mode,
    /// The master volume, linear. After the chorus.
    pub volume: f32,
    pub assign: Assign,
}

impl Default for Patch {
    fn default() -> Self {
        Self {
            voice: VoicePatch::default(),
            lfo_rate_hz: 4.0,
            lfo_delay_s: 0.0,
            hpf: Position::Flat,
            level: 0.8,
            chorus: Mode::Off,
            volume: 0.5,
            assign: Assign::Poly1,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Synth {
    voices: [Voice; VOICES],
    ledger: [Option<Press>; LEDGER_CAPACITY],
    next_order: u64,
    /// POLY 2's pointer: the next voice to hand out.
    rotation: usize,
    /// Per voice, the order of the press whose expression it sounds.
    expression_order: [u64; VOICES],
    lfo: Lfo,
    dc: DcBlocker,
    hpf: Hpf,
    chorus: Chorus,
    /// The routes, as [`Synth::set_topology`] last set them, with the amounts the caller refreshes
    /// per sample. Held here rather than in [`Patch`], which is rebuilt per sample: a grid copied
    /// 48 000 times a second for nothing is `mxm-mono-00`'s measured lesson.
    routing: Routing,
    /// Samples of [`POST_TAIL_S`] still to run after the last voice went idle.
    settle: u32,
    sample_rate: f32,
}

impl Default for Synth {
    fn default() -> Self {
        Self::new()
    }
}

impl Synth {
    pub fn new() -> Self {
        let mut synth = Self {
            voices: std::array::from_fn(Voice::new),
            ledger: [None; LEDGER_CAPACITY],
            next_order: 1,
            rotation: 0,
            expression_order: [0; VOICES],
            lfo: Lfo::new(),
            dc: DcBlocker::default(),
            hpf: Hpf::new(),
            chorus: Chorus::new(),
            routing: Routing::new(),
            settle: 0,
            sample_rate: 48_000.0,
        };
        synth.set_sample_rate(48_000.0);
        synth
    }

    /// Allocates the chorus's delay lines. Never called from the audio path.
    pub fn set_sample_rate(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate;
        for voice in &mut self.voices {
            voice.set_sample_rate(sample_rate);
        }
        self.dc.set_sample_rate(sample_rate);
        self.hpf.set_sample_rate(sample_rate);
        self.chorus.set_sample_rate(sample_rate);
    }

    /// Clear every bit of state. Leaves no tail, and the next render is bit-identical to the last.
    pub fn reset(&mut self) {
        for voice in &mut self.voices {
            voice.reset();
        }
        self.ledger = [None; LEDGER_CAPACITY];
        self.next_order = 1;
        self.rotation = 0;
        self.expression_order = [0; VOICES];
        self.lfo.reset();
        self.dc.reset();
        self.hpf.reset();
        self.chorus.reset();
        self.settle = 0;
    }

    // ---- the ledger ----

    fn order(&mut self) -> u64 {
        let order = self.next_order;
        self.next_order += 1;
        order
    }

    /// Every voice some live press holds.
    fn held_mask(&self) -> u8 {
        self.ledger
            .iter()
            .flatten()
            .fold(0, |mask, press| mask | press.voices)
    }

    fn any_live(&self) -> bool {
        self.held_mask() != 0
    }

    /// The slot of the oldest press satisfying `pick`.
    fn oldest_where(&self, pick: impl Fn(&Press) -> bool) -> Option<usize> {
        self.ledger
            .iter()
            .enumerate()
            .filter_map(|(slot, press)| press.filter(|p| pick(p)).map(|p| (slot, p.order)))
            .min_by_key(|(_, order)| *order)
            .map(|(slot, _)| slot)
    }

    /// Removes a press, releasing each of its voices that no other live press still holds.
    fn retire(&mut self, slot: usize) {
        self.retire_with(slot, false);
    }

    /// The same, silencing rather than releasing when `choke` is set.
    fn retire_with(&mut self, slot: usize, choke: bool) {
        let Some(press) = self.ledger[slot].take() else {
            return;
        };
        let still_held = self.held_mask();
        for voice in 0..VOICES {
            let bit = 1 << voice;
            if press.voices & bit != 0 && still_held & bit == 0 {
                if choke {
                    self.voices[voice].silence();
                } else {
                    self.voices[voice].release();
                }
            }
        }
    }

    /// Puts a press in the ledger, making room by the overflow rule if it has to.
    fn insert(&mut self, press: Press) {
        if let Some(slot) = self.ledger.iter().position(Option::is_none) {
            self.ledger[slot] = Some(press);
            return;
        }
        // Full. The oldest tombstone costs nothing to lose; failing that, the oldest press goes,
        // retired as its note-off would have — a release one press early, never a stuck note.
        let slot = self
            .oldest_where(Press::is_tombstone)
            .or_else(|| self.oldest_where(|_| true))
            .expect("a full ledger has an oldest press");
        self.retire(slot);
        self.ledger[slot] = Some(press);
    }

    /// Takes `mask`'s voices away from every live press. A press left with nothing is a tombstone.
    fn steal(&mut self, mask: u8) {
        for press in self.ledger.iter_mut().flatten() {
            press.voices &= !mask;
        }
    }

    /// POLY 1: the lowest free voice, else one from the oldest press.
    fn pick_poly1(&self) -> u8 {
        let held = self.held_mask();
        if held != ALL_VOICES {
            return lowest_bit(!held & ALL_VOICES);
        }
        self.oldest_where(|p| !p.is_tombstone())
            .and_then(|slot| self.ledger[slot])
            .map_or(1, |p| lowest_bit(p.voices))
    }

    /// POLY 2: the next voice round the rotation, free or not.
    fn pick_poly2(&mut self) -> u8 {
        let voice = self.rotation % VOICES;
        self.rotation = (voice + 1) % VOICES;
        1 << voice
    }

    /// A key went down, at `velocity` — held by the voices it reaches as a routing source.
    pub fn note_on(&mut self, key: Key, note_id: Option<i32>, assign: Assign, velocity: f32) {
        let was_live = self.any_live();
        let order = self.order();

        // A repeated press of a key a live press holds joins it: the voices continue, untouched.
        if let Some(existing) = self
            .ledger
            .iter()
            .flatten()
            .filter(|p| !p.is_tombstone() && p.key == key)
            .max_by_key(|p| p.order)
            .copied()
        {
            let mask = existing.voices;
            self.insert(Press {
                key,
                note_id,
                voices: mask,
                order,
            });
            for voice in voices_in(mask) {
                self.voices[voice].note_on(key.note, false, velocity);
                // A new press clears the expression, joint or not.
                self.voices[voice].set_expression(0.0);
                self.expression_order[voice] = order;
            }
            return;
        }

        let mask = match assign {
            Assign::Poly1 => self.pick_poly1(),
            Assign::Poly2 => self.pick_poly2(),
            Assign::Unison => ALL_VOICES,
        };
        self.steal(mask);
        self.insert(Press {
            key,
            note_id,
            voices: mask,
            order,
        });
        for voice in voices_in(mask) {
            self.voices[voice].note_on(key.note, true, velocity);
            self.expression_order[voice] = order;
        }

        // The first key of a phrase restarts the LFO delay. Chosen; see the module doc.
        if !was_live {
            self.lfo.retrigger_delay();
        }
    }

    /// A key came up. Retires the oldest press it is about, **tombstones first**, so a late note-off
    /// for a stolen press is absorbed by the tombstone and never reaches the re-pressed key's voice.
    pub fn note_off(&mut self, key: Key, note_id: Option<i32>) {
        let slot = self
            .oldest_where(|p| p.is_tombstone() && p.matches(key, note_id))
            .or_else(|| self.oldest_where(|p| p.matches(key, note_id)));
        if let Some(slot) = slot {
            self.retire(slot);
        }
        // Matching nothing: dropped.
    }

    /// A choke for one note: like its note-off, but immediate, with no release.
    pub fn choke(&mut self, key: Key, note_id: Option<i32>) {
        let slot = self
            .oldest_where(|p| p.is_tombstone() && p.matches(key, note_id))
            .or_else(|| self.oldest_where(|p| p.matches(key, note_id)));
        if let Some(slot) = slot {
            self.retire_with(slot, true);
        }
    }

    /// Per-note pitch expression, in semitones.
    ///
    /// **A non-finite value is dropped**, and every press keeps the offset it had: a NaN in a
    /// voice's pitch sum would reach its DCO's phase and never leave.
    pub fn expression(&mut self, key: Key, note_id: Option<i32>, semitones: f32) {
        if !semitones.is_finite() {
            return;
        }
        for slot in 0..LEDGER_CAPACITY {
            let Some(press) = self.ledger[slot] else {
                continue;
            };
            if !press.matches(key, note_id) || press.is_tombstone() {
                continue;
            }
            for voice in voices_in(press.voices) {
                if press.order >= self.expression_order[voice] {
                    self.voices[voice].set_expression(semitones);
                    self.expression_order[voice] = press.order;
                }
            }
        }
    }

    /// CC 120 and choke: immediate, no release. The ledger empties, so a later stray note-off
    /// matches nothing and is dropped.
    pub fn all_sound_off(&mut self) {
        self.ledger = [None; LEDGER_CAPACITY];
        // Each voice's ladder too: a silenced voice skips its filter until its next note, so a
        // ladder left as the panic found it would be where that note starts.
        for voice in &mut self.voices {
            voice.all_sound_off();
        }
        // **Immediate means the next sample is zero.** The voices alone going silent left the
        // post-mix chain to settle — the DC blocker answering the cut with a decaying step, the
        // chorus draining its delay lines, `settle` holding `is_active` for the whole post tail —
        // which is not what CC 120 is for. Everything downstream of the voices is cleared too, so
        // a host that panics gets silence on the next sample and `Normal` on the next block.
        self.dc.reset();
        self.hpf.reset();
        self.chorus.silence();
        self.settle = 0;
    }

    /// CC 123: every note releases normally, and the chain settles as after any release.
    pub fn all_notes_off(&mut self) {
        self.ledger = [None; LEDGER_CAPACITY];
        for voice in &mut self.voices {
            voice.release();
        }
    }

    // ---- the audio path ----

    /// Control-rate settings, applied at a block boundary: the HPF's position and the chorus mode
    /// are switches, and recomputing a filter per sample for a switch is waste.
    pub fn prepare(&mut self, patch: &Patch) {
        self.set_switches(patch.hpf, patch.chorus);
    }

    /// The same, from the two switch values alone — so a caller with parameter smoothers does not
    /// have to build a whole patch, and advance every smoother, to set two switches.
    pub fn set_switches(&mut self, hpf: Position, chorus: Mode) {
        if self.hpf.position() != hpf {
            self.hpf.set_position(hpf);
        }
        self.chorus.set_mode(chorus);
    }

    /// Which routes exist. **Once per processing interval, never per sample**: the routing is
    /// copied, compacted, and every voice armed — idle ones too, or a voice's first note after it
    /// was allocated would render unrouted.
    pub fn set_topology(&mut self, routing: &Routing) {
        self.routing = *routing;
        self.routing.compact();
        for voice in &mut self.voices {
            voice.set_topology(&self.routing);
        }
    }

    /// The routing as [`Synth::set_topology`] last set it.
    #[must_use]
    pub fn routing(&self) -> &Routing {
        &self.routing
    }

    /// The routing, for refreshing the live routes' **amounts** before each [`Synth::process`].
    /// Changing which routes are present here does nothing until [`Synth::set_topology`].
    pub fn routing_mut(&mut self) -> &mut Routing {
        &mut self.routing
    }

    /// Render one stereo sample.
    #[inline]
    pub fn process(&mut self, patch: &Patch) -> (f32, f32) {
        let lfo = self
            .lfo
            .process(patch.lfo_rate_hz, patch.lfo_delay_s, self.sample_rate);

        let mut sum = 0.0f32;
        let mut active = false;
        for voice in &mut self.voices {
            sum += voice.process(&patch.voice, lfo, &self.routing);
            active |= voice.is_active();
        }

        // The post-mix chain, in the schematic's order: sum → HPF → the patch's VCA → chorus →
        // master. The HPF shapes a chord's low end once; the level sets how hard the BBD is driven.
        let blocked = self.dc.process(sum * VOICE_SUM_GAIN);
        let shaped = self.hpf.process(blocked) * patch.level;
        self.chorus.set_active(active);
        let (l, r) = self.chorus.process(shaped);

        // The post-mix chain settles for a fixed time after the last voice goes idle, rather than
        // anything trying to detect exact zeros along it. See `POST_TAIL_S`.
        if active {
            self.settle = (POST_TAIL_S * self.sample_rate) as u32;
        } else if self.settle > 0 {
            self.settle -= 1;
            if self.settle == 0 {
                // **Idle is defined.** From here the plugin reports `Normal` and a host may stop
                // calling `process`, which freezes time; a GATE-mode release still running behind
                // its closed amplifier would then resume from wherever it was, however long the
                // host slept. So every envelope is at zero when the instrument is idle, and a
                // note after idle always starts its filter sweep from the bottom. See
                // `Voice::retire_hidden_envelope`.
                for voice in &mut self.voices {
                    voice.retire_hidden_envelope();
                }
            }
        }

        (flush(l * patch.volume), flush(r * patch.volume))
    }

    /// Anything still coming out: a voice sounding or releasing, or the post-mix chain settling.
    pub fn is_active(&self) -> bool {
        self.settle > 0 || self.voices.iter().any(Voice::is_active)
    }

    /// Samples of tail remaining: the longest voice's release, then [`POST_TAIL_S`].
    pub fn tail_samples(&self, release_s: f32) -> u32 {
        let voices = self
            .voices
            .iter()
            .map(|v| v.tail_samples(release_s))
            .max()
            .unwrap_or(0);
        voices.saturating_add((POST_TAIL_S * self.sample_rate) as u32)
    }

    // ---- read-only accessors, for telemetry and tests ----

    /// Each voice's envelope level, `0..=1`. Read once per block by the audio thread and published
    /// to atomics; a UI thread must never call it.
    pub fn voice_levels(&self) -> [f32; VOICES] {
        std::array::from_fn(|i| self.voices[i].env_level())
    }

    /// Which key each voice last sounded, and whether it is still held.
    pub fn voice_notes(&self) -> [(u8, bool); VOICES] {
        std::array::from_fn(|i| (self.voices[i].note(), self.voices[i].is_held()))
    }

    /// The mask of voices some live press holds.
    pub fn held_voices(&self) -> u8 {
        self.held_mask()
    }

    /// How many presses the ledger holds, tombstones included.
    pub fn ledger_len(&self) -> usize {
        self.ledger.iter().flatten().count()
    }

    /// The chorus modulator's current value, for a display.
    pub fn chorus_lfo(&self) -> f32 {
        self.chorus.lfo()
    }

    /// The LFO's delay fade, `0..=1`, for tests.
    pub fn lfo_fade(&self) -> f32 {
        self.lfo.fade()
    }
}

#[inline]
fn lowest_bit(mask: u8) -> u8 {
    mask & mask.wrapping_neg()
}

fn voices_in(mask: u8) -> impl Iterator<Item = usize> {
    (0..VOICES).filter(move |v| mask & (1 << v) != 0)
}

#[cfg(test)]
// A test patch reads better as "the default, then the three things this test changes" than as a
// struct literal that names them out of order.
#[allow(clippy::field_reassign_with_default)]
mod tests {
    use super::*;
    use crate::dco::Mix;
    use crate::envelope::Stage;
    use crate::routing::{SOURCES, TARGETS, source, target};

    const FS: f32 = 48_000.0;

    fn synth() -> Synth {
        let mut s = Synth::new();
        s.set_sample_rate(FS);
        s
    }

    fn key(note: u8) -> Key {
        Key { channel: 0, note }
    }

    /// Every source into every target, at one amount.
    fn every_route(amount: f32) -> Routing {
        let mut routing = Routing::new();
        for t in 0..TARGETS {
            for s in 0..SOURCES {
                routing.present[t][s] = true;
                routing.amounts[t][s] = amount;
            }
        }
        routing.compact();
        routing
    }

    fn run(s: &mut Synth, patch: &Patch, n: usize) -> Vec<(f32, f32)> {
        s.prepare(patch);
        (0..n).map(|_| s.process(patch)).collect()
    }

    #[test]
    fn six_keys_take_six_voices_and_the_seventh_steals_the_oldest() {
        let mut s = synth();
        for n in 0..6u8 {
            s.note_on(key(60 + n), None, Assign::Poly1, 0.8);
        }
        assert_eq!(s.held_voices(), ALL_VOICES);
        assert_eq!(s.ledger_len(), 6);

        s.note_on(key(72), None, Assign::Poly1, 0.8);
        assert_eq!(s.held_voices(), ALL_VOICES, "still six voices held");
        assert_eq!(
            s.ledger_len(),
            7,
            "the stolen press is a tombstone, not gone"
        );
        let notes = s.voice_notes();
        assert_eq!(
            notes[0].0, 72,
            "POLY 1 stole the oldest press's voice, which was voice 0"
        );
        assert!(notes.iter().all(|(_, held)| *held));

        // The stolen key's note-off retires its tombstone and releases nothing.
        s.note_off(key(60), None);
        assert_eq!(s.held_voices(), ALL_VOICES);
        assert_eq!(s.ledger_len(), 6);
    }

    /// The case revision 2 of the plan got wrong.
    #[test]
    fn a_late_id_less_note_off_for_a_stolen_key_does_not_release_its_re_press() {
        let mut s = synth();
        for n in 0..6u8 {
            s.note_on(key(60 + n), None, Assign::Poly1, 0.8);
        }
        s.note_on(key(72), None, Assign::Poly1, 0.8); // steals key 60's voice
        s.note_on(key(60), None, Assign::Poly1, 0.8); // key 60 again, on some other stolen voice
        let held_before = s.held_voices();
        assert_eq!(held_before, ALL_VOICES);

        s.note_off(key(60), None); // the FIRST press's late note-off
        assert_eq!(
            s.held_voices(),
            ALL_VOICES,
            "the tombstone absorbed it; the re-pressed key is still sounding"
        );
        s.note_off(key(60), None); // the second press's
        assert_ne!(s.held_voices(), ALL_VOICES, "and that one releases it");
    }

    #[test]
    fn a_repeated_press_joins_the_voice_and_the_last_release_frees_it() {
        let mut s = synth();
        let patch = Patch::default();
        s.note_on(key(60), None, Assign::Poly1, 0.8);
        // Long enough for the decay to hand over to sustain, which it does when within 1e-4.
        run(&mut s, &patch, 48_000);
        let stage = s.voices[0].env_stage();
        assert_eq!(stage, Stage::Sustain);

        s.note_on(key(60), None, Assign::Poly1, 0.8);
        assert_eq!(s.held_voices(), 1, "the same voice, no second one");
        assert_eq!(
            s.voices[0].env_stage(),
            Stage::Sustain,
            "and nothing retriggered"
        );

        s.note_off(key(60), None);
        assert_eq!(s.held_voices(), 1, "one press still holds it");
        s.note_off(key(60), None);
        assert_eq!(s.held_voices(), 0);
        assert_eq!(s.voices[0].env_stage(), Stage::Release);
    }

    #[test]
    fn a_note_off_with_an_id_retires_the_press_carrying_it() {
        let mut s = synth();
        s.note_on(key(60), Some(7), Assign::Poly1, 0.8);
        s.note_on(key(64), Some(8), Assign::Poly1, 0.8);
        s.note_off(key(64), Some(7)); // the id wins over the key
        assert_eq!(
            s.held_voices(),
            0b10,
            "key 60's voice released, key 64's still held"
        );
    }

    #[test]
    fn a_note_off_matching_nothing_is_dropped() {
        let mut s = synth();
        s.note_on(key(60), None, Assign::Poly1, 0.8);
        s.note_off(key(61), None);
        assert_eq!(s.held_voices(), 1, "a different key is not this press");
        s.note_off(key(60), Some(99));
        assert_eq!(
            s.held_voices(),
            0,
            "the press has no id, so the key decides — and the key matches"
        );

        let mut s = synth();
        s.note_on(key(60), Some(1), Assign::Poly1, 0.8);
        s.note_off(key(60), Some(99));
        assert_eq!(
            s.held_voices(),
            1,
            "both sides carry an id and they differ, so this note-off is about some other press"
        );
        s.note_off(key(60), None);
        assert_eq!(s.held_voices(), 0, "an id-less note-off reaches it by key");
    }

    #[test]
    fn the_ledger_overflows_by_retiring_the_oldest_and_never_sticks() {
        let mut s = synth();
        // A host sending fresh-id note-ons for one key forever.
        for i in 0..(LEDGER_CAPACITY as i32 * 3) {
            s.note_on(key(60), Some(i), Assign::Poly1, 0.8);
            assert!(s.ledger_len() <= LEDGER_CAPACITY);
        }
        assert_eq!(s.held_voices(), 1, "one voice, however many presses");
        for i in 0..(LEDGER_CAPACITY as i32 * 3) {
            s.note_off(key(60), Some(i));
        }
        assert_eq!(s.held_voices(), 0, "no stuck note");
        assert_eq!(s.ledger_len(), 0);
    }

    /// The case revision 4 of the plan got wrong.
    #[test]
    fn leaving_unison_and_stealing_one_voice_leaves_five_to_release_with_the_held_key() {
        let mut s = synth();
        s.note_on(key(48), None, Assign::Unison, 0.8);
        assert_eq!(s.held_voices(), ALL_VOICES);

        // Mode switched to POLY 1 with the key still down: future allocation only.
        s.note_on(key(72), None, Assign::Poly1, 0.8);
        assert_eq!(s.held_voices(), ALL_VOICES, "six voices are still held");
        let notes = s.voice_notes();
        assert_eq!(
            notes.iter().filter(|(n, _)| *n == 72).count(),
            1,
            "exactly one voice was taken"
        );
        assert_eq!(
            notes.iter().filter(|(n, _)| *n == 48).count(),
            5,
            "five stay on the unison key"
        );

        s.note_off(key(48), None);
        assert_eq!(
            s.held_voices().count_ones(),
            1,
            "exactly five released; the new key's voice stays"
        );
        assert_eq!(s.ledger_len(), 1);
    }

    #[test]
    fn poly_1_reuses_the_lowest_voice_and_poly_2_rotates() {
        let mut s = synth();
        let mut poly1 = Vec::new();
        for n in 0..7u8 {
            s.note_on(key(60 + n), None, Assign::Poly1, 0.8);
            poly1.push(lowest_bit(s.held_voices()).trailing_zeros());
            s.note_off(key(60 + n), None);
        }
        assert_eq!(poly1, vec![0; 7], "POLY 1 goes back to voice 0 every time");

        let mut s = synth();
        let mut poly2 = Vec::new();
        for n in 0..7u8 {
            s.note_on(key(60 + n), None, Assign::Poly2, 0.8);
            poly2.push(lowest_bit(s.held_voices()).trailing_zeros());
            s.note_off(key(60 + n), None);
        }
        assert_eq!(
            poly2,
            vec![0, 1, 2, 3, 4, 5, 0],
            "POLY 2 rotates through all six"
        );
    }

    /// Voices differ, so the two modes render a phrase differently — **and removing the difference
    /// must make this test fail**, which is what the second half checks by construction: two POLY 1
    /// renders of the same phrase are identical, so the assertion is not passing on noise.
    #[test]
    fn poly_1_and_poly_2_are_audibly_different() {
        let mut patch = Patch::default();
        patch.voice.cutoff_hz = 1_200.0;
        patch.voice.resonance = 0.6;
        let phrase = |assign: Assign| {
            let mut s = synth();
            let mut out = Vec::new();
            for n in [60u8, 64, 67, 71] {
                s.note_on(key(n), None, assign, 0.8);
                out.extend(run(&mut s, &patch, 6_000));
                s.note_off(key(n), None);
                out.extend(run(&mut s, &patch, 6_000));
            }
            out
        };
        let a = phrase(Assign::Poly1);
        let b = phrase(Assign::Poly2);
        let again = phrase(Assign::Poly1);
        assert_eq!(
            a, again,
            "the same mode renders the same phrase bit-identically"
        );
        let differ = a
            .iter()
            .zip(&b)
            .filter(|(x, y)| (x.0 - y.0).abs() > 1e-4)
            .count();
        assert!(
            differ > a.len() / 2,
            "the modes differed on only {differ} of {} samples",
            a.len()
        );
    }

    #[test]
    fn unison_is_loud_not_wide() {
        let mut patch = Patch::default();
        patch.voice.mix.saw = 1.0;
        let level = |assign: Assign| {
            let mut s = synth();
            s.note_on(key(57), None, assign, 0.8);
            let out = run(&mut s, &patch, 24_000);
            let tail = &out[12_000..];
            (tail.iter().map(|(l, _)| l * l).sum::<f32>() / tail.len() as f32).sqrt()
        };
        let one = level(Assign::Poly1);
        let six = level(Assign::Unison);
        // Six coherent voices sum toward 6x, not sqrt(6)x — the offsets keep it a little short.
        assert!(six > one * 4.0, "unison {six} against a single voice {one}");
    }

    #[test]
    fn the_lfo_delay_restarts_on_the_first_key_of_a_phrase_only() {
        let mut s = synth();
        let mut patch = Patch::default();
        patch.lfo_delay_s = 1.0;
        s.note_on(key(60), None, Assign::Poly1, 0.8);
        run(&mut s, &patch, 4_800);
        assert_eq!(s.lfo_fade(), 0.0, "held at zero during the delay");
        run(&mut s, &patch, (FS * 1.5) as usize);
        let fade = s.lfo_fade();
        assert!(fade > 0.5, "fading in: {fade}");

        s.note_on(key(64), None, Assign::Poly1, 0.8); // a second key while the first is held
        run(&mut s, &patch, 100);
        assert!(
            s.lfo_fade() >= fade,
            "a second key does not restart the delay"
        );

        s.note_off(key(60), None);
        s.note_off(key(64), None);
        run(&mut s, &patch, (FS * 2.0) as usize);
        s.note_on(key(60), None, Assign::Poly1, 0.8);
        run(&mut s, &patch, 100);
        assert_eq!(
            s.lfo_fade(),
            0.0,
            "the first key after every key was released does"
        );
    }

    /// **A panic clears every voice's filter**, as it clears the DC blocker, the HPF and the chorus
    /// after them. A silenced voice is idle and skips its ladder until its next note, and the
    /// instrument reports idle at once, so whatever each ladder was ringing with when the panic
    /// landed would be where that voice's next note starts — whether the host sleeps or keeps
    /// calling. Two instruments play the same keys for the same time, so the DCOs, which an idle
    /// voice freezes rather than resets, and the global LFO are in step; their cutoffs, resonances,
    /// mixes and sustains differ. Both panic, both are called through a silent gap, and the same
    /// chord must render bit-identically. The ledger empties, so a late release carrying a
    /// pre-panic press's id is not about the new press of that key.
    #[test]
    fn panic_clears_every_voices_filter_and_a_late_release_of_an_old_press_moves_nothing() {
        let bright = Patch {
            voice: VoicePatch {
                mix: Mix {
                    saw: 1.0,
                    pulse: 1.0,
                    sub: 1.0,
                    noise: 0.3,
                },
                cutoff_hz: 16_000.0,
                resonance: 0.95,
                sustain: 1.0,
                ..VoicePatch::default()
            },
            ..Patch::default()
        };
        let dark = Patch {
            voice: VoicePatch {
                cutoff_hz: 200.0,
                resonance: 0.1,
                sustain: 0.3,
                ..VoicePatch::default()
            },
            ..Patch::default()
        };
        let default = Patch::default();
        let mut wakes = [Vec::new(), Vec::new()];
        for (patch, wake) in [&bright, &dark].into_iter().zip(wakes.iter_mut()) {
            let mut s = synth();
            for (id, note) in [(1, 48), (2, 55), (3, 60)] {
                s.note_on(key(note), Some(id), Assign::Poly1, 0.8);
            }
            run(&mut s, patch, 4_800);
            s.all_sound_off();
            assert!(!s.is_active(), "the plugin reports Normal at once");
            assert_eq!(s.ledger_len(), 0);
            assert!(
                run(&mut s, &default, 480)
                    .iter()
                    .all(|&(l, r)| l == 0.0 && r == 0.0)
            );

            s.note_on(key(60), Some(4), Assign::Poly1, 0.8);
            s.note_on(key(55), Some(5), Assign::Poly1, 0.8);
            s.note_off(key(60), Some(3));
            assert_eq!(
                s.held_voices(),
                0b11,
                "a pre-panic press's release is not about its key's new press"
            );
            *wake = run(&mut s, &default, 2_048);
        }
        assert!(
            wakes[0].iter().any(|(l, _)| l.abs() > 0.05),
            "the chord sounds"
        );
        let first_difference = wakes[0]
            .iter()
            .zip(&wakes[1])
            .position(|(a, b)| a.0.to_bits() != b.0.to_bits() || a.1.to_bits() != b.1.to_bits())
            .map(|i| (i, wakes[0][i], wakes[1][i]));
        assert_eq!(
            first_difference, None,
            "a voice's ladder leaked through All Sound Off"
        );
    }

    #[test]
    fn all_sound_off_empties_everything_and_a_stray_note_off_is_harmless() {
        // With audio rendered first: an all-sound-off that was only ever tested on a silent synth
        // proved nothing about the sound stopping. Found in review.
        let mut patch = Patch::default();
        patch.chorus = Mode::Both;
        let mut s = synth();
        for n in 0..4u8 {
            s.note_on(key(60 + n), None, Assign::Poly1, 0.8);
        }
        let before = run(&mut s, &patch, 12_000);
        assert!(
            before.iter().any(|(l, _)| l.abs() > 0.05),
            "the premise: it was sounding"
        );

        s.all_sound_off();
        assert_eq!(s.held_voices(), 0);
        assert_eq!(s.ledger_len(), 0);
        assert!(!s.voices.iter().any(|v| v.env_stage() != Stage::Idle));

        // **Immediate.** The next sample is exactly zero and the instrument reports idle: the
        // voices are silenced, and so is everything downstream of them — the DC blocker that would
        // otherwise answer the cut with a decaying step, the HPF, the chorus's delay lines and its
        // noise. Nothing rings out, because a panic is what CC 120 is.
        assert!(!s.is_active(), "all sound off must leave nothing active");
        let after = run(&mut s, &patch, 4_800);
        assert!(
            after.iter().all(|&(l, r)| l == 0.0 && r == 0.0),
            "all sound off must be silence from the next sample: {:?}",
            after.iter().find(|&&(l, r)| l != 0.0 || r != 0.0)
        );

        s.note_off(key(61), None);
        assert_eq!(s.ledger_len(), 0);

        // And the instrument plays again afterwards: a panic clears, it does not disable.
        s.note_on(key(64), None, Assign::Poly1, 0.8);
        let again = run(&mut s, &patch, 4_800);
        assert!(
            again.iter().any(|(l, _)| l.abs() > 0.05),
            "a note after all sound off must sound"
        );
    }

    #[test]
    fn expression_reaches_the_newest_press_and_a_note_off_keeps_it() {
        let mut s = synth();
        s.note_on(key(60), Some(1), Assign::Poly1, 0.8);
        s.expression(key(60), Some(1), 3.0);
        assert_eq!(s.voices[0].note(), 60);
        // A joining press clears it and becomes the owner.
        s.note_on(key(60), Some(2), Assign::Poly1, 0.8);
        let mut patch = Patch::default();
        patch.voice.tune_semitones = 0.0;
        s.expression(key(60), Some(1), 5.0); // the older press: ignored
        s.expression(key(60), Some(2), -2.0); // the newer: applied
        // Compare against a plain voice bent by the same amount.
        let mut reference = synth();
        reference.note_on(key(60), None, Assign::Poly1, 0.8);
        reference.note_on(key(60), None, Assign::Poly1, 0.8);
        let mut bent = patch;
        bent.voice.tune_semitones = -2.0;
        assert_eq!(
            run(&mut s, &patch, 2_000),
            run(&mut reference, &bent, 2_000)
        );

        // A note-off does not change it.
        s.note_off(key(60), Some(2));
        s.note_off(key(60), Some(1));
        reference.note_off(key(60), None);
        reference.note_off(key(60), None);
        assert_eq!(
            run(&mut s, &patch, 2_000),
            run(&mut reference, &bent, 2_000)
        );
    }

    #[test]
    fn an_expression_for_a_tombstone_is_dropped() {
        let mut s = synth();
        for n in 0..6u8 {
            s.note_on(key(60 + n), Some(n as i32), Assign::Poly1, 0.8);
        }
        s.note_on(key(72), Some(20), Assign::Poly1, 0.8); // steals press 0
        s.expression(key(60), Some(0), 12.0);
        let mut reference = synth();
        for n in 0..6u8 {
            reference.note_on(key(60 + n), Some(n as i32), Assign::Poly1, 0.8);
        }
        reference.note_on(key(72), Some(20), Assign::Poly1, 0.8);
        let patch = Patch::default();
        assert_eq!(
            run(&mut s, &patch, 2_000),
            run(&mut reference, &patch, 2_000)
        );
    }

    /// **A non-finite expression is dropped**, and the press keeps the offset it had. A NaN in a
    /// voice's pitch sum reaches its DCO's frequency and phase, which never recover.
    #[test]
    fn a_non_finite_expression_is_dropped_and_the_pitch_stays_finite() {
        let patch = Patch::default();
        let bent = || {
            let mut s = synth();
            s.note_on(key(60), Some(1), Assign::Poly1, 0.8);
            s.note_on(key(64), Some(2), Assign::Poly1, 0.8);
            s.expression(key(60), Some(1), 3.0);
            s
        };
        for bad in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            let (mut actual, mut reference) = (bent(), bent());
            actual.expression(key(60), Some(1), bad);
            actual.expression(key(64), None, bad);
            let heard = run(&mut actual, &patch, 2_000);
            assert!(
                heard.iter().all(|(l, r)| l.is_finite() && r.is_finite()),
                "{bad}"
            );
            assert_eq!(heard, run(&mut reference, &patch, 2_000), "{bad}");
            assert!(heard.iter().any(|(l, _)| *l != 0.0), "the chord sounds");
        }
    }

    #[test]
    fn the_chorus_makes_the_stereo_and_off_is_dual_mono() {
        let mut patch = Patch::default();
        let mut s = synth();
        s.note_on(key(57), None, Assign::Poly1, 0.8);
        let off = run(&mut s, &patch, 24_000);
        assert!(off[4_000..].iter().all(|(l, r)| l == r));
        patch.chorus = Mode::I;
        let on = run(&mut s, &patch, 24_000);
        let differ = on[4_000..]
            .iter()
            .filter(|(l, r)| (l - r).abs() > 1e-4)
            .count();
        assert!(differ > 10_000, "{differ}");
    }

    #[test]
    fn silence_in_gives_exactly_zero_out_after_the_tail_and_two_instances_agree() {
        let mut patch = Patch::default();
        patch.chorus = Mode::Both;
        patch.hpf = Position::Boost;
        let render = || {
            let mut s = synth();
            for n in [60u8, 64, 67] {
                s.note_on(key(n), None, Assign::Poly1, 0.8);
            }
            let mut out = run(&mut s, &patch, 12_000);
            for n in [60u8, 64, 67] {
                s.note_off(key(n), None);
            }
            let tail = s.tail_samples(patch.voice.release_s) as usize;
            out.extend(run(&mut s, &patch, tail * 2));
            (out, s)
        };
        let (a, mut s) = render();
        let (b, _) = render();
        assert_eq!(a, b, "two instances must render identically");
        assert!(
            !s.is_active(),
            "after twice the reported tail nothing should be active"
        );
        assert_eq!(s.process(&patch), (0.0, 0.0));
    }

    #[test]
    fn the_tail_covers_the_actual_decay() {
        let mut patch = Patch::default();
        patch.chorus = Mode::I;
        patch.voice.release_s = 0.5;
        let mut s = synth();
        s.note_on(key(60), None, Assign::Poly1, 0.8);
        run(&mut s, &patch, 12_000);
        s.note_off(key(60), None);
        let predicted = s.tail_samples(patch.voice.release_s) as usize;
        let mut actual = 0usize;
        while s.is_active() && actual < FS as usize * 10 {
            s.process(&patch);
            actual += 1;
        }
        assert!(
            predicted >= actual,
            "predicted {predicted} but it took {actual}"
        );
    }

    /// **Idle is defined.** A GATE-mode release longer than the settle time is running behind a
    /// closed amplifier when the instrument reports idle; a host may then stop calling `process`.
    /// So when the instrument goes idle every envelope is at zero, and the next note starts its
    /// filter sweep from the bottom, whether the host slept or not.
    #[test]
    fn when_the_instrument_goes_idle_every_hidden_envelope_is_at_zero() {
        let mut patch = Patch::default();
        patch.voice.vca_gate = true;
        patch.voice.release_s = 12.0;
        let mut s = synth();
        s.note_on(key(60), None, Assign::Poly1, 0.8);
        run(&mut s, &patch, 12_000);
        s.note_off(key(60), None);

        // Run to idle exactly as a host would, and no further.
        let mut n = 0usize;
        while s.is_active() {
            s.process(&patch);
            n += 1;
            assert!(n < FS as usize * 5, "the instrument never went idle");
        }
        assert_eq!(
            s.voice_levels(),
            [0.0; VOICES],
            "an idle instrument must have every envelope at zero, hidden ones included"
        );

        // A host slept for an unknown time; the next note is the same whatever that time was.
        s.note_on(key(64), None, Assign::Poly1, 0.8);
        let first = s.voice_levels();
        assert!(
            first.iter().all(|&l| l < 0.01),
            "the next note starts its envelope from the bottom: {first:?}"
        );
        let again = run(&mut s, &patch, 4_800);
        assert!(again.iter().any(|(l, _)| l.abs() > 0.05), "and it sounds");
    }

    #[test]
    fn no_nan_with_everything_at_its_limit() {
        for fs in [44_100.0f32, 48_000.0, 96_000.0, 192_000.0] {
            let mut s = Synth::new();
            s.set_sample_rate(fs);
            let mut patch = Patch::default();
            patch.voice.cutoff_hz = 20_000.0;
            patch.voice.resonance = 1.0;
            s.set_topology(&every_route(1.0));
            patch.voice.mix.pulse = 1.0;
            patch.voice.mix.sub = 1.0;
            patch.voice.mix.noise = 1.0;
            patch.level = 4.0;
            patch.chorus = Mode::Both;
            patch.hpf = Position::Boost;
            for n in 0..8u8 {
                s.note_on(key(120 + n / 2), None, Assign::Unison, 0.8);
            }
            for (l, r) in run(&mut s, &patch, 8_000) {
                assert!(l.is_finite() && r.is_finite(), "at {fs}");
            }
        }
    }
    /// **Every voice is armed with the topology, idle ones included** — the sampler's lesson: a voice
    /// armed only when it sounds renders its first note unrouted.
    #[test]
    fn every_voice_is_armed_with_the_topology_idle_ones_included() {
        let mut s = synth();
        s.set_topology(&Routing::from_pairs(&[(
            target::CUTOFF,
            source::VELOCITY,
            1.0,
        )]));
        for (i, voice) in s.voices.iter().enumerate() {
            assert!(
                voice.routed_for_test(target::CUTOFF),
                "voice {i} is not armed"
            );
        }
    }

    /// **The init routes at zero depth render bit-identically to nothing routed** — the default and
    /// chorus golden scores' guarantee at this layer, with noise and the pulse in the mix so every
    /// source the DCO publishes is exercised.
    #[test]
    fn the_init_routes_at_zero_depth_render_bit_identically_to_nothing_routed() {
        let mut patch = Patch::default();
        patch.chorus = Mode::I;
        patch.lfo_delay_s = 0.3;
        patch.voice.mix.noise = 0.3;
        patch.voice.mix.pulse = 1.0;
        let render = |routing: &Routing| {
            let mut s = synth();
            s.set_topology(routing);
            for n in [48u8, 52, 55, 60] {
                s.note_on(key(n), None, Assign::Poly1, 0.8);
            }
            let mut out = run(&mut s, &patch, 24_000);
            for n in [48u8, 52, 55, 60] {
                s.note_off(key(n), None);
            }
            out.extend(run(&mut s, &patch, 48_000));
            out
        };
        assert_eq!(render(&Routing::init()), render(&Routing::new()));
    }

    /// **Every source into every target, with no key down, is exact silence**: no route can make a
    /// silent voice sound, and the global LFO cannot hold the instrument open.
    #[test]
    fn every_source_into_every_target_with_no_key_down_is_exact_silence() {
        let mut s = synth();
        s.set_topology(&every_route(1.0));
        let mut patch = Patch::default();
        patch.voice.wheel = 1.0;
        patch.voice.pressure = 1.0;
        patch.voice.bend = 1.0;
        let out = run(&mut s, &patch, 48_000);
        assert!(out.iter().all(|&(l, r)| l == 0.0 && r == 0.0));
        assert!(!s.is_active());
    }

    /// **Several audio sources summed into cutoff at the top of resonance stay bounded.** A summing
    /// input can drive a modulated coefficient where one source never could — `mxm-mono-00`'s
    /// phaser self-oscillated to infinity that way. The ladder's feedback is the loop that could
    /// pump, so the late window must not grow on the earlier one.
    #[test]
    fn summed_audio_into_cutoff_stays_bounded_at_the_top_of_resonance() {
        let rms = |x: &[(f32, f32)]| {
            (x.iter().map(|(l, r)| l * l + r * r).sum::<f32>() / (2 * x.len()) as f32).sqrt()
        };
        for fs in [8_000.0f32, 48_000.0, 192_000.0] {
            let mut s = Synth::new();
            s.set_sample_rate(fs);
            s.set_topology(&Routing::from_pairs(&[
                (target::CUTOFF, source::SAW, 1.0),
                (target::CUTOFF, source::PULSE, 1.0),
                (target::CUTOFF, source::SUB, 1.0),
                (target::CUTOFF, source::NOISE, 1.0),
            ]));
            let mut patch = Patch::default();
            patch.voice.resonance = 1.0;
            patch.voice.cutoff_hz = 2_000.0;
            patch.voice.mix = Mix {
                saw: 1.0,
                pulse: 1.0,
                sub: 1.0,
                noise: 1.0,
            };
            for n in [36u8, 60, 84] {
                s.note_on(key(n), None, Assign::Poly1, 1.0);
            }
            let out = run(&mut s, &patch, (fs * 3.0) as usize);
            assert!(
                out.iter().all(|(l, r)| l.is_finite() && r.is_finite()),
                "at {fs}"
            );
            let third = out.len() / 3;
            let (early, late) = (rms(&out[third..2 * third]), rms(&out[2 * third..]));
            assert!(
                late <= early * 1.5 + 1e-6,
                "at {fs} the late window grew: {early} then {late}"
            );
        }
    }
}
