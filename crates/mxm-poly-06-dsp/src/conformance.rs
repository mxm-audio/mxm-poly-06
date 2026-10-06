//! mxm-poly-06's routing as the collection's modulation standard checks it
//! (`mxm_modulation::conformance`; `plans/plan-modulation-standard.md`).
//!
//! Behind the `conformance` feature, which only `[dev-dependencies]` enable — this crate's own
//! tests, and the plugin's, whose route readings are held to [`Declared::deliver`] — so no shipped
//! graph carries it. [`Declared`] answers every question through [`crate::routing`]'s own tables
//! and a real [`Graph`], never a copy of them.

use mxm_modulation::conformance::{Declaration, Kind};
use mxm_modulation::standard::{self, Offer, Performance};

use crate::routing::{
    self, Graph, KEY_UNIT_SEMITONES, Routing, SOURCE_NAMES, SOURCES, TARGET_NAMES, TARGETS, target,
};

/// What each target is, for the standard: a pitch, a symmetric width, a cutoff and the amplitude
/// factor.
const KINDS: [Kind; TARGETS] = [Kind::Pitch, Kind::Width, Kind::Cutoff, Kind::Amplitude];

/// mxm-poly-06's routing declaration.
#[derive(Debug, Clone, Copy, Default)]
pub struct Declared;

impl Declaration for Declared {
    fn sources(&self) -> usize {
        SOURCES
    }

    fn targets(&self) -> usize {
        TARGETS
    }

    fn performance(&self, source: usize) -> Option<Performance> {
        routing::PERFORMANCE[source]
    }

    fn kind(&self, target: usize) -> Kind {
        KINDS[target]
    }

    fn machine(&self, target: usize, source: usize) -> bool {
        routing::machine(target, source)
    }

    fn offered(&self, target: usize, source: usize) -> Offer {
        routing::offer(target, source)
    }

    fn key_unit(&self) -> f32 {
        KEY_UNIT_SEMITONES
    }

    /// One route alone through a voice's own [`Graph`], its source publishing `raw`; Amplitude
    /// through the factor the voice applies.
    fn deliver(&self, target: usize, source: usize, amount: f32, raw: f32) -> f32 {
        let routing = Routing::from_pairs(&[(target, source, amount)]);
        let mut graph = Graph::new();
        graph.set_topology(&routing);
        graph.begin_sample();
        graph.write(source, raw);
        let sum = graph.sum(target, &routing);
        if target == target::AMPLITUDE {
            standard::amplitude_factor(sum) - 1.0
        } else {
            sum
        }
    }

    fn name(&self, target: usize, source: usize) -> String {
        format!("{} from {}", TARGET_NAMES[target], SOURCE_NAMES[source])
    }
}

#[cfg(test)]
mod tests {
    use mxm_modulation::conformance::{self, Case, Input};

    use super::*;
    use crate::routing::source;
    use crate::voice::{Voice, VoicePatch};

    const RATE: f32 = 48_000.0;

    fn report(result: Result<(), Vec<String>>) {
        if let Err(failures) = result {
            panic!("{} failure(s):\n{}", failures.len(), failures.join("\n"));
        }
    }

    /// **Every pair means what the standard says**: offered as `standard::offer` says, nothing at
    /// its source's rest, a meaningful move at full, and the standard reach for every pair the
    /// JUNO did not have.
    ///
    /// Falsified before trusted: with the added pitch reach left at the LFO slider's seven
    /// semitones, it names every added pitch pair from a performance source and the reach it
    /// should have.
    #[test]
    fn every_pair_means_what_the_standard_says() {
        report(conformance::check_declaration(&Declared));
    }

    /// **A voice publishes what the standard says**: Key from middle C over its unit, Velocity as
    /// `v − 1`, the gestures as they arrive, each exactly zero at rest.
    ///
    /// Falsified before trusted: publishing the raw velocity fails at every input.
    #[test]
    fn a_voice_publishes_what_the_standard_says() {
        report(conformance::check_publishers(&Declared, |from, input| {
            // A route reads the source, so the voice publishes it; at zero depth nothing moves.
            let routing = Routing::from_pairs(&[(target::CUTOFF, from, 0.0)]);
            let mut voice = Voice::new(0);
            voice.set_sample_rate(RATE);
            voice.set_topology(&routing);
            let mut patch = VoicePatch::default();
            let (note, velocity) = match input {
                Input::Note(note) => (note, 1.0),
                Input::Normalised(value) if from == source::VELOCITY => (60, value),
                _ => (60, 1.0),
            };
            match input {
                Input::Normalised(value) if from == source::WHEEL => patch.wheel = value,
                Input::Normalised(value) if from == source::PRESSURE => patch.pressure = value,
                Input::Lever(value) => patch.bend = value,
                _ => {}
            }
            voice.note_on(note, true, velocity);
            voice.process(&patch, 0.0, &routing);
            voice.published_for_test(from)
        }));
    }

    /// **Key is the note the voice is sounding, glide and all**: a glide's first sample reads where
    /// the voice was, and one time constant in it reads the lag's own value. Where it lands is the
    /// portamento's own property, held by `voice::tests::a_glide_lands_exactly_on_its_note`.
    ///
    /// Falsified before trusted: publishing the key before the lag reads 72 on the first sample.
    #[test]
    fn key_follows_the_portamento() {
        let routing = Routing::from_pairs(&[(target::CUTOFF, source::KEY, 0.0)]);
        let mut voice = Voice::new(0);
        voice.set_sample_rate(RATE);
        voice.set_topology(&routing);
        let patch = VoicePatch {
            portamento_s: 0.1,
            ..VoicePatch::default()
        };
        voice.note_on(48, true, 1.0);
        for _ in 0..(0.5 * RATE) as usize {
            voice.process(&patch, 0.0, &routing);
        }
        voice.note_on(72, true, 1.0);
        voice.process(&patch, 0.0, &routing);
        let started = voice.published_for_test(source::KEY) * KEY_UNIT_SEMITONES + 60.0;
        assert!(
            (48.0..48.1).contains(&started),
            "a glide starts where the voice was: {started}"
        );
        // One time constant: 0.1 s at this rate, the first sample already taken.
        for _ in 1..(0.1 * RATE) as usize {
            voice.process(&patch, 0.0, &routing);
        }
        let one_tau = voice.published_for_test(source::KEY) * KEY_UNIT_SEMITONES + 60.0;
        let expected = 72.0 - 24.0 * (-1.0f32).exp();
        assert!(
            (one_tau - expected).abs() < 0.01,
            "one time constant in, the key reads the lag: {one_tau}, not {expected}"
        );
    }

    /// **After a release, no performance route holds a note open** — every pair, both halves,
    /// the softest and hardest notes and the keyboard's ends, gestures held at full through the
    /// note and let go at the release, in both of the VCA's modes.
    #[test]
    fn after_a_release_no_performance_route_holds_a_note_open() {
        report(conformance::check_release_silence(
            &Declared,
            &[],
            |case: Case| {
                [false, true].into_iter().all(|vca_gate| {
                    let routing = Routing::from_pairs(&[(case.target, case.source, case.amount)]);
                    let mut voice = Voice::new(0);
                    voice.set_sample_rate(RATE);
                    voice.set_topology(&routing);
                    let mut patch = VoicePatch {
                        wheel: 1.0,
                        pressure: 1.0,
                        bend: 1.0,
                        release_s: 0.05,
                        vca_gate,
                        ..VoicePatch::default()
                    };
                    voice.note_on(case.key, true, case.velocity);
                    for _ in 0..(0.1 * RATE) as usize {
                        voice.process(&patch, 0.0, &routing);
                    }
                    voice.release();
                    patch.wheel = 0.0;
                    patch.pressure = 0.0;
                    patch.bend = 0.0;
                    for _ in 0..(2.0 * RATE) as usize {
                        voice.process(&patch, 0.0, &routing);
                        if !voice.is_active() {
                            break;
                        }
                    }
                    !voice.is_active() && voice.process(&patch, 0.0, &routing) == 0.0
                })
            },
        ));
    }
}
