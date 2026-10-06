//! Presets: this instrument's factory set, and what the collection's preset crate needs of it.
//!
//! The format, the library on disk, favourites, the loaded identity and the app-bar controls are
//! `mxm-preset`'s — one crate for every instrument and effect, extracted from the five verbatim
//! copies this file used to be one of (`plugins/AGENTS.md`, *A preset is parameter values*). What
//! is left here is what only this instrument knows: its id, its parameters, and its sounds.

use std::sync::RwLock;

pub use mxm_preset::{
    Category, Entry, INIT_NAME, Library, Loaded, Origin, Preset, PresetIdentity, Refused, Value,
    factory, loaded, mark_loaded, mark_none, read_favourites, snapshot, write_favourites,
};

use crate::params::MxmPoly06Params;

/// **The tempo syncs this plugin gained on 2026-09-25** (`plans/plan-tempo-sync-controls.md`). A
/// preset file written before them was written unsynced, so each loads off rather than keeping the
/// instance's sync, and without reporting a missing control.
pub(crate) const TEMPO_SYNC_IDS: &[&str] = &["lfosync"];

impl mxm_preset::Instrument for MxmPoly06Params {
    fn clap_id(&self) -> &'static str {
        crate::CLAP_ID
    }

    /// In declaration order, from the one list the editor draws from.
    fn parameters(&self) -> Vec<(&'static str, &dyn mxm_preset::ErasedParam)> {
        // **The routes are parameters like any other**, and a preset that did not name them would
        // leave the previous patch's modulation in place — which bites hardest here, because the
        // machine's own envelope, LFO and key paths *are* routes now.
        crate::editor::sections::all_parameters(self)
            .into_iter()
            .map(|bound| (bound.id, bound.param))
            .chain(self.routes.parameters())
            .collect()
    }

    fn identity(&self) -> &RwLock<PresetIdentity> {
        &self.preset
    }

    fn factory_files(&self) -> &'static [(&'static str, &'static str)] {
        FACTORY_FILES
    }

    fn default_missing_legacy_parameter(&self, id: &str) -> bool {
        TEMPO_SYNC_IDS.contains(&id)
    }
}

/// The factory set, compiled in.
///
/// **Fifty files, and Init is not one of them** — see [`Preset::init`]. These fifty are
/// *content*: a sound nobody can read is a sound nobody can learn from, so they are files rather
/// than code.
pub const FACTORY_FILES: &[(&str, &str)] = &[
    ("Strings", include_str!("../presets/strings.json")),
    ("Brass", include_str!("../presets/brass.json")),
    ("Organ", include_str!("../presets/organ.json")),
    ("Chorus pad", include_str!("../presets/chorus-pad.json")),
    ("Pulse bass", include_str!("../presets/pulse-bass.json")),
    ("Sub bass", include_str!("../presets/sub-bass.json")),
    ("Hollow pad", include_str!("../presets/hollow-pad.json")),
    ("Soft keys", include_str!("../presets/soft-keys.json")),
    ("Bright lead", include_str!("../presets/bright-lead.json")),
    ("Stacked lead", include_str!("../presets/stacked-lead.json")),
    ("Slow sweep", include_str!("../presets/slow-sweep.json")),
    ("Gate pad", include_str!("../presets/gate-pad.json")),
    ("Noise wash", include_str!("../presets/noise-wash.json")),
    ("Bell keys", include_str!("../presets/bell-keys.json")),
    ("Unison bass", include_str!("../presets/unison-bass.json")),
    ("High pad", include_str!("../presets/high-pad.json")),
    ("Thin pulse", include_str!("../presets/thin-pulse.json")),
    ("Dark pad", include_str!("../presets/dark-pad.json")),
    ("Vibrato lead", include_str!("../presets/vibrato-lead.json")),
    ("Wide chords", include_str!("../presets/wide-chords.json")),
    ("Juno piano", include_str!("../presets/juno-piano.json")),
    (
        "Electric piano",
        include_str!("../presets/electric-piano.json"),
    ),
    ("Harpsichord", include_str!("../presets/harpsichord.json")),
    ("Clav", include_str!("../presets/clav.json")),
    ("Music box", include_str!("../presets/music-box.json")),
    ("Pad strings", include_str!("../presets/pad-strings.json")),
    ("Cello", include_str!("../presets/cello.json")),
    ("Solo violin", include_str!("../presets/solo-violin.json")),
    ("Fanfare", include_str!("../presets/fanfare.json")),
    ("Soft horn", include_str!("../presets/soft-horn.json")),
    ("Synth brass", include_str!("../presets/synth-brass.json")),
    ("Warm pad", include_str!("../presets/warm-pad.json")),
    ("Glass pad", include_str!("../presets/glass-pad.json")),
    ("Evolving pad", include_str!("../presets/evolving-pad.json")),
    ("Choir", include_str!("../presets/choir.json")),
    (
        "Cinematic swell",
        include_str!("../presets/cinematic-swell.json"),
    ),
    ("Sweep bass", include_str!("../presets/sweep-bass.json")),
    ("Square bass", include_str!("../presets/square-bass.json")),
    ("Rubber bass", include_str!("../presets/rubber-bass.json")),
    ("Sub kick", include_str!("../presets/sub-kick.json")),
    ("Noise hat", include_str!("../presets/noise-hat.json")),
    ("Snare", include_str!("../presets/snare.json")),
    ("Tom", include_str!("../presets/tom.json")),
    ("Poly lead", include_str!("../presets/poly-lead.json")),
    ("Soft lead", include_str!("../presets/soft-lead.json")),
    ("Pluck", include_str!("../presets/pluck.json")),
    ("Harp", include_str!("../presets/harp.json")),
    ("Rain drops", include_str!("../presets/rain-drops.json")),
    ("Wobble", include_str!("../presets/wobble.json")),
    ("Drone chord", include_str!("../presets/drone-chord.json")),
];

#[cfg(test)]
mod tests {
    use super::*;

    /// **A project saved before the tempo syncs restores them Off** (`mxm_preset::add_switches_off`),
    /// whatever this instance had.
    #[test]
    fn an_older_state_restores_the_tempo_syncs_off() {
        use nice_plug::prelude::Plugin as _;
        let mut state = nice_plug::prelude::PluginState {
            version: String::new(),
            params: Default::default(),
            fields: Default::default(),
        };
        crate::MxmPoly06::filter_state(&mut state);
        for id in TEMPO_SYNC_IDS {
            assert!(
                matches!(
                    state.params.get(*id),
                    Some(nice_plug::plugin::ParamValue::Bool(false))
                ),
                "{{id}} was not restored off"
            );
        }
    }

    /// **A preset saved before the tempo syncs loads them off, and cleanly** ([`TEMPO_SYNC_IDS`]).
    #[test]
    fn a_preset_from_before_the_tempo_syncs_loads_them_off() {
        let params = crate::params::MxmPoly06Params::default();
        let mut old = mxm_preset::Preset::init(&params);
        for id in TEMPO_SYNC_IDS {
            old.params.remove(*id);
        }
        let (writes, problems) = old.resolve(&params);
        assert!(problems.is_empty(), "{{problems:?}}");
        for id in TEMPO_SYNC_IDS {
            assert!(
                writes.iter().any(|(w, _, v)| w == id && *v == 0.0),
                "{{id}} was not written off"
            );
        }
    }

    use mxm_preset::user_root;
    use nice_plug::params::Param;

    fn params() -> MxmPoly06Params {
        MxmPoly06Params::default()
    }

    /// Prints every parameter as eleven `normalised=formatted` steps.
    ///
    /// A facility, not a test: designing a factory preset means choosing normalised values, and
    /// choosing them blind is how a preset ends up with a filter at 0.5 that nobody meant.
    ///
    /// ```text
    /// cargo test -p mxm-poly-06 --lib the_mapping_table -- --ignored --nocapture
    /// ```
    #[test]
    #[ignore = "prints what each normalised value means, for preset design"]
    fn the_mapping_table() {
        let params = params();
        for bound in crate::editor::sections::all_parameters(&params) {
            let steps: Vec<String> = (0..=10)
                .map(|i| {
                    let v = i as f32 / 10.0;
                    format!("{v:.1}={}", bound.param.format(v))
                })
                .collect();
            eprintln!("{:<12} {}", bound.id, steps.join("  "));
        }
    }

    /// The factory sounds, as **overrides on the defaults**.
    ///
    /// Written as the handful of values that make each sound rather than as every number
    /// apiece: a file full of defaults hides the three that matter. `write_the_factory_presets`
    /// turns each into a complete file, because the format takes no sparse overlays — an overlay's
    /// meaning would change the day the defaults were retuned.
    ///
    /// **Fifty, and Init is not one of them.** Init is generated from the parameter defaults and
    /// has no file at all; see [`Preset::init`].
    ///
    /// Most designs are an envelope shape, a filter position and a chorus button — which is
    /// faithful, because that is how the machine's own patches were made.
    ///
    /// **The machine's modulation is routing**, so a design sets a route's amount by its permanent
    /// id, normalised — a depth `d` is `(d + 1) / 2` — and the envelope's polarity is its sign. A
    /// design that used the PWM slider's LFO mode sets the pulse-width route and leaves the width
    /// at one half, which is what LFO mode swept around (`plan-mxm-poly-06-modulation.md` §4.3).
    const FACTORY_DESIGN: &[Design] = &[
        (
            "Strings",
            Category::Strings,
            // the one everybody means: saw, slow attack, chorus I, a little HPF cut
            &[
                ("attack", 0.62),
                ("decay", 0.55),
                ("sustain", 0.85),
                ("release", 0.62),
                ("cutoff", 0.72),
                ("chorus", 0.333_33),
                ("hpf", 0.666_67),
            ],
        ),
        (
            "Brass",
            Category::Brass,
            // saw and sub, the envelope sweeping the filter, chorus II
            &[
                ("sub", 0.5),
                ("cutoff", 0.4),
                ("mod_cutoff_env", 0.725),
                ("attack", 0.38),
                ("decay", 0.5),
                ("sustain", 0.6),
                ("release", 0.4),
                ("chorus", 0.666_67),
            ],
        ),
        (
            "Organ",
            Category::Keys,
            // pulse plus sub, the VCA on the gate: hard on, hard off
            &[
                ("saw", 0.0),
                ("pulse", 1.0),
                ("sub", 0.8),
                ("cutoff", 0.65),
                ("vcamode", 1.0),
                ("release", 0.2),
                ("chorus", 0.333_33),
            ],
        ),
        (
            "Chorus pad",
            Category::Pad,
            // saw and pulse, both buttons, the boost in
            &[
                ("pulse", 1.0),
                ("pulsewidth", 0.35),
                ("attack", 0.55),
                ("decay", 0.6),
                ("sustain", 0.8),
                ("release", 0.65),
                ("cutoff", 0.55),
                ("hpf", 0.0),
                ("chorus", 1.0),
            ],
        ),
        (
            "Pulse bass",
            Category::Bass,
            // a narrow pulse, the filter low and plucked, 16'
            &[
                ("saw", 0.0),
                ("pulse", 1.0),
                ("pulsewidth", 0.2),
                ("range", 0.0),
                ("cutoff", 0.28),
                ("resonance", 0.3),
                ("mod_cutoff_env", 0.775),
                ("decay", 0.42),
                ("sustain", 0.2),
                ("release", 0.3),
            ],
        ),
        (
            "Sub bass",
            Category::Bass,
            // the sub carrying it, the filter nearly shut, the boost in
            &[
                ("sub", 1.0),
                ("range", 0.0),
                ("cutoff", 0.2),
                ("mod_cutoff_env", 0.65),
                ("decay", 0.45),
                ("sustain", 0.5),
                ("release", 0.3),
                ("hpf", 0.0),
            ],
        ),
        (
            "Hollow pad",
            Category::Pad,
            // a pulse with its width on the LFO, chorus I
            &[
                ("saw", 0.0),
                ("pulse", 1.0),
                ("mod_width_lfo", 0.8),
                ("lforate", 0.35),
                ("attack", 0.6),
                ("sustain", 0.9),
                ("release", 0.6),
                ("cutoff", 0.5),
                ("chorus", 0.333_33),
            ],
        ),
        (
            "Soft keys",
            Category::Keys,
            // saw with a gentle envelope on the filter, keyboard tracking on
            &[
                ("cutoff", 0.35),
                ("mod_cutoff_env", 0.7),
                ("mod_cutoff_key", 0.85),
                ("decay", 0.52),
                ("sustain", 0.35),
                ("release", 0.45),
                ("chorus", 0.333_33),
            ],
        ),
        (
            "Bright lead",
            Category::Lead,
            // open, a little resonance, the filter tracking the keys
            &[
                ("cutoff", 0.85),
                ("resonance", 0.35),
                ("mod_cutoff_key", 0.75),
                ("sustain", 0.9),
                ("release", 0.3),
                ("mod_pitch_lfo", 0.54),
                ("lforate", 0.5),
            ],
        ),
        (
            "Stacked lead",
            Category::Lead,
            // saw and pulse together in unison: loud, not wide, because that is the machine
            &[
                ("pulse", 1.0),
                ("pulsewidth", 0.3),
                ("keyassign", 1.0),
                ("cutoff", 0.6),
                ("resonance", 0.25),
                ("mod_cutoff_env", 0.65),
                ("decay", 0.45),
                ("sustain", 0.7),
                ("portamento", 0.3),
            ],
        ),
        (
            "Slow sweep",
            Category::Pad,
            // the envelope taking a long time to open the filter, then holding
            &[
                ("cutoff", 0.15),
                ("mod_cutoff_env", 0.875),
                ("attack", 0.78),
                ("decay", 0.7),
                ("sustain", 1.0),
                ("release", 0.7),
                ("resonance", 0.4),
                ("chorus", 0.666_67),
            ],
        ),
        (
            "Gate pad",
            Category::Pad,
            // the VCA on the gate while the envelope sweeps the filter underneath
            &[
                ("vcamode", 1.0),
                ("cutoff", 0.3),
                ("mod_cutoff_env", 0.8),
                ("attack", 0.5),
                ("decay", 0.55),
                ("sustain", 0.4),
                ("chorus", 0.333_33),
            ],
        ),
        (
            "Noise wash",
            Category::Fx,
            // noise through a resonant filter swept by the LFO
            &[
                ("saw", 0.0),
                ("noise", 1.0),
                ("cutoff", 0.45),
                ("resonance", 0.7),
                ("mod_cutoff_lfo", 0.75),
                ("lforate", 0.2),
                ("attack", 0.6),
                ("sustain", 1.0),
                ("release", 0.7),
                ("chorus", 1.0),
            ],
        ),
        (
            "Bell keys",
            Category::Keys,
            // a fast filter pluck on a narrow pulse, inverted polarity for the dip
            &[
                ("saw", 0.0),
                ("pulse", 1.0),
                ("pulsewidth", 0.12),
                ("cutoff", 0.5),
                ("resonance", 0.55),
                ("mod_cutoff_env", 0.25),
                ("decay", 0.35),
                ("sustain", 0.0),
                ("release", 0.5),
            ],
        ),
        (
            "Unison bass",
            Category::Bass,
            // six voices on one key, 16', portamento
            &[
                ("keyassign", 1.0),
                ("range", 0.0),
                ("sub", 0.4),
                ("cutoff", 0.32),
                ("mod_cutoff_env", 0.7),
                ("decay", 0.45),
                ("sustain", 0.4),
                ("portamento", 0.25),
            ],
        ),
        (
            "High pad",
            Category::Pad,
            // 4' and a wide pulse: the top of the keyboard, both chorus buttons
            &[
                ("range", 1.0),
                ("pulse", 1.0),
                ("pulsewidth", 0.6),
                ("attack", 0.55),
                ("sustain", 0.9),
                ("release", 0.6),
                ("cutoff", 0.6),
                ("chorus", 1.0),
            ],
        ),
        (
            "Thin pulse",
            Category::Lead,
            // the narrowest pulse, HPF cut 2, no chorus: the dry, thin end of the machine
            &[
                ("saw", 0.0),
                ("pulse", 1.0),
                ("pulsewidth", 0.06),
                ("hpf", 1.0),
                ("cutoff", 0.7),
                ("decay", 0.4),
                ("sustain", 0.6),
            ],
        ),
        (
            "Dark pad",
            Category::Pad,
            // the filter low and slow, the boost in, chorus I
            &[
                ("cutoff", 0.25),
                ("mod_cutoff_env", 0.6),
                ("attack", 0.7),
                ("decay", 0.6),
                ("sustain", 0.9),
                ("release", 0.75),
                ("hpf", 0.0),
                ("chorus", 0.333_33),
            ],
        ),
        (
            "Vibrato lead",
            Category::Lead,
            // the LFO on the pitch after a delay: the delay is the machine's own vibrato control
            &[
                ("mod_pitch_lfo", 0.6),
                ("lforate", 0.55),
                ("lfodelay", 0.25),
                ("cutoff", 0.7),
                ("sustain", 0.85),
                ("release", 0.3),
                ("portamento", 0.15),
            ],
        ),
        (
            "Wide chords",
            Category::Pad,
            // saw, sub and both chorus buttons: the sound the machine is known for
            &[
                ("sub", 0.35),
                ("attack", 0.45),
                ("decay", 0.55),
                ("sustain", 0.8),
                ("release", 0.6),
                ("cutoff", 0.62),
                ("mod_cutoff_env", 0.575),
                ("chorus", 1.0),
            ],
        ),
        (
            "Juno piano",
            Category::Keys,
            &[
                ("pulse", 1.0),
                ("pulsewidth", 0.3),
                ("cutoff", 0.55),
                ("mod_cutoff_env", 0.675),
                ("attack", 0.0),
                ("decay", 0.6),
                ("sustain", 0.3),
                ("release", 0.45),
                ("chorus", 0.33333),
            ],
        ),
        (
            "Electric piano",
            Category::Keys,
            &[
                ("saw", 0.0),
                ("pulse", 1.0),
                ("pulsewidth", 0.15),
                ("sub", 0.5),
                ("cutoff", 0.5),
                ("mod_cutoff_env", 0.7),
                ("decay", 0.65),
                ("sustain", 0.2),
                ("release", 0.5),
                ("chorus", 0.33333),
                ("mod_cutoff_key", 0.75),
            ],
        ),
        (
            "Harpsichord",
            Category::Keys,
            &[
                ("saw", 0.0),
                ("pulse", 1.0),
                ("pulsewidth", 0.1),
                ("cutoff", 0.75),
                ("mod_cutoff_env", 0.65),
                ("decay", 0.5),
                ("sustain", 0.15),
                ("release", 0.3),
                ("hpf", 0.66667),
            ],
        ),
        (
            "Clav",
            Category::Keys,
            &[
                ("saw", 0.0),
                ("pulse", 1.0),
                ("pulsewidth", 0.2),
                ("cutoff", 0.6),
                ("resonance", 0.4),
                ("mod_cutoff_env", 0.75),
                ("decay", 0.4),
                ("sustain", 0.1),
                ("release", 0.25),
                ("mod_cutoff_key", 0.8),
            ],
        ),
        (
            "Music box",
            Category::Keys,
            &[
                ("saw", 0.0),
                ("pulse", 1.0),
                ("range", 1.0),
                ("cutoff", 0.8),
                ("attack", 0.0),
                ("decay", 0.55),
                ("sustain", 0.0),
                ("release", 0.6),
                ("chorus", 0.33333),
            ],
        ),
        (
            "Pad strings",
            Category::Strings,
            &[
                ("attack", 0.7),
                ("decay", 0.6),
                ("sustain", 0.9),
                ("release", 0.7),
                ("cutoff", 0.6),
                ("hpf", 0.66667),
                ("chorus", 0.66667),
                ("mod_pitch_lfo", 0.52),
                ("lforate", 0.4),
                ("lfodelay", 0.3),
            ],
        ),
        (
            "Cello",
            Category::Strings,
            &[
                ("sub", 0.4),
                ("range", 0.0),
                ("attack", 0.5),
                ("sustain", 0.9),
                ("release", 0.5),
                ("cutoff", 0.5),
                ("mod_cutoff_env", 0.6),
                ("chorus", 0.33333),
                ("mod_pitch_lfo", 0.525),
                ("lfodelay", 0.4),
            ],
        ),
        (
            "Solo violin",
            Category::Strings,
            &[
                ("range", 1.0),
                ("attack", 0.45),
                ("sustain", 0.9),
                ("release", 0.45),
                ("cutoff", 0.7),
                ("mod_cutoff_key", 0.7),
                ("mod_pitch_lfo", 0.54),
                ("lforate", 0.55),
                ("lfodelay", 0.35),
                ("keyassign", 1.0),
                ("chorus", 0.33333),
            ],
        ),
        (
            "Fanfare",
            Category::Brass,
            &[
                ("pulse", 1.0),
                ("pulsewidth", 0.4),
                ("cutoff", 0.45),
                ("mod_cutoff_env", 0.75),
                ("attack", 0.25),
                ("decay", 0.5),
                ("sustain", 0.7),
                ("release", 0.4),
                ("hpf", 0.33333),
                ("chorus", 0.66667),
            ],
        ),
        (
            "Soft horn",
            Category::Brass,
            &[
                ("sub", 0.3),
                ("cutoff", 0.35),
                ("mod_cutoff_env", 0.7),
                ("attack", 0.4),
                ("decay", 0.55),
                ("sustain", 0.6),
                ("release", 0.45),
                ("chorus", 0.33333),
                ("mod_cutoff_key", 0.65),
            ],
        ),
        (
            "Synth brass",
            Category::Brass,
            &[
                ("pulse", 1.0),
                ("pulsewidth", 0.45),
                ("cutoff", 0.4),
                ("mod_cutoff_env", 0.775),
                ("resonance", 0.15),
                ("attack", 0.3),
                ("decay", 0.5),
                ("sustain", 0.65),
                ("release", 0.4),
                ("keyassign", 0.5),
                ("portamento", 0.15),
            ],
        ),
        (
            "Warm pad",
            Category::Pad,
            &[
                ("pulse", 1.0),
                ("cutoff", 0.45),
                ("attack", 0.65),
                ("decay", 0.7),
                ("sustain", 0.9),
                ("release", 0.7),
                ("hpf", 0.33333),
                ("chorus", 0.33333),
                ("mod_width_lfo", 0.65),
                ("lforate", 0.3),
            ],
        ),
        (
            "Glass pad",
            Category::Pad,
            &[
                ("saw", 0.0),
                ("pulse", 1.0),
                ("range", 1.0),
                ("cutoff", 0.7),
                ("attack", 0.6),
                ("sustain", 0.85),
                ("release", 0.7),
                ("hpf", 0.66667),
                ("chorus", 1.0),
                ("mod_pitch_lfo", 0.515),
                ("lforate", 0.35),
            ],
        ),
        (
            "Evolving pad",
            Category::Pad,
            &[
                ("pulse", 1.0),
                ("mod_width_lfo", 0.85),
                ("lforate", 0.15),
                ("cutoff", 0.4),
                ("mod_cutoff_env", 0.65),
                ("attack", 0.75),
                ("decay", 0.8),
                ("sustain", 0.8),
                ("release", 0.75),
                ("chorus", 0.66667),
                ("resonance", 0.2),
            ],
        ),
        (
            "Choir",
            Category::Pad,
            &[
                ("saw", 0.0),
                ("pulse", 1.0),
                ("mod_width_lfo", 0.675),
                ("lforate", 0.3),
                ("cutoff", 0.4),
                ("resonance", 0.3),
                ("attack", 0.6),
                ("sustain", 0.9),
                ("release", 0.65),
                ("chorus", 1.0),
                ("hpf", 0.66667),
            ],
        ),
        (
            "Cinematic swell",
            Category::Pad,
            &[
                ("sub", 0.4),
                ("range", 0.0),
                ("cutoff", 0.3),
                ("mod_cutoff_env", 0.75),
                ("attack", 0.85),
                ("decay", 0.8),
                ("sustain", 0.9),
                ("release", 0.8),
                ("resonance", 0.2),
                ("chorus", 0.66667),
            ],
        ),
        (
            "Sweep bass",
            Category::Bass,
            &[
                ("sub", 0.6),
                ("range", 0.0),
                ("cutoff", 0.25),
                ("resonance", 0.5),
                ("mod_cutoff_env", 0.8),
                ("decay", 0.5),
                ("sustain", 0.3),
                ("release", 0.3),
                ("keyassign", 1.0),
            ],
        ),
        (
            "Square bass",
            Category::Bass,
            &[
                ("saw", 0.0),
                ("pulse", 1.0),
                ("sub", 0.5),
                ("range", 0.0),
                ("cutoff", 0.35),
                ("mod_cutoff_env", 0.675),
                ("decay", 0.45),
                ("sustain", 0.4),
                ("release", 0.3),
                ("hpf", 0.0),
            ],
        ),
        (
            "Rubber bass",
            Category::Bass,
            &[
                ("pulse", 1.0),
                ("pulsewidth", 0.3),
                ("range", 0.0),
                ("cutoff", 0.3),
                ("resonance", 0.35),
                ("mod_cutoff_env", 0.75),
                ("decay", 0.35),
                ("sustain", 0.1),
                ("release", 0.25),
                ("keyassign", 1.0),
                ("portamento", 0.1),
            ],
        ),
        (
            "Sub kick",
            Category::Percussion,
            &[
                ("saw", 0.0),
                ("sub", 1.0),
                ("range", 0.0),
                ("cutoff", 0.3),
                ("mod_cutoff_env", 0.75),
                ("attack", 0.0),
                ("decay", 0.3),
                ("sustain", 0.0),
                ("release", 0.25),
                ("hpf", 0.0),
            ],
        ),
        (
            "Noise hat",
            Category::Percussion,
            &[
                ("saw", 0.0),
                ("noise", 1.0),
                ("cutoff", 0.9),
                ("hpf", 1.0),
                ("attack", 0.0),
                ("decay", 0.15),
                ("sustain", 0.0),
                ("release", 0.1),
            ],
        ),
        (
            "Snare",
            Category::Percussion,
            &[
                ("saw", 0.0),
                ("pulse", 1.0),
                ("noise", 0.8),
                ("cutoff", 0.65),
                ("resonance", 0.3),
                ("mod_cutoff_env", 0.7),
                ("attack", 0.0),
                ("decay", 0.25),
                ("sustain", 0.0),
                ("release", 0.2),
            ],
        ),
        (
            "Tom",
            Category::Percussion,
            &[
                ("saw", 0.0),
                ("sub", 0.7),
                ("pulse", 1.0),
                ("range", 0.0),
                ("cutoff", 0.4),
                ("mod_cutoff_env", 0.8),
                ("decay", 0.35),
                ("sustain", 0.0),
                ("release", 0.3),
                ("resonance", 0.2),
            ],
        ),
        (
            "Poly lead",
            Category::Lead,
            &[
                ("pulse", 1.0),
                ("pulsewidth", 0.35),
                ("cutoff", 0.75),
                ("resonance", 0.2),
                ("mod_cutoff_env", 0.625),
                ("decay", 0.5),
                ("sustain", 0.8),
                ("release", 0.3),
                ("chorus", 0.33333),
                ("mod_pitch_lfo", 0.525),
                ("lfodelay", 0.4),
            ],
        ),
        (
            "Soft lead",
            Category::Lead,
            &[
                ("saw", 0.0),
                ("pulse", 1.0),
                ("cutoff", 0.55),
                ("mod_cutoff_env", 0.65),
                ("attack", 0.2),
                ("decay", 0.5),
                ("sustain", 0.8),
                ("release", 0.4),
                ("mod_pitch_lfo", 0.535),
                ("lforate", 0.5),
                ("lfodelay", 0.5),
                ("portamento", 0.2),
                ("keyassign", 1.0),
            ],
        ),
        (
            "Pluck",
            Category::Pluck,
            &[
                ("pulse", 1.0),
                ("pulsewidth", 0.3),
                ("cutoff", 0.35),
                ("resonance", 0.35),
                ("mod_cutoff_env", 0.8),
                ("attack", 0.0),
                ("decay", 0.3),
                ("sustain", 0.0),
                ("release", 0.35),
                ("chorus", 0.33333),
            ],
        ),
        (
            "Harp",
            Category::Pluck,
            &[
                ("saw", 0.0),
                ("pulse", 1.0),
                ("pulsewidth", 0.15),
                ("cutoff", 0.55),
                ("mod_cutoff_env", 0.675),
                ("attack", 0.0),
                ("decay", 0.5),
                ("sustain", 0.0),
                ("release", 0.55),
                ("chorus", 0.33333),
                ("mod_cutoff_key", 0.75),
                ("range", 1.0),
            ],
        ),
        (
            "Rain drops",
            Category::Fx,
            &[
                ("saw", 0.0),
                ("pulse", 1.0),
                ("range", 1.0),
                ("cutoff", 0.7),
                ("resonance", 0.6),
                ("mod_cutoff_env", 0.75),
                ("attack", 0.0),
                ("decay", 0.2),
                ("sustain", 0.0),
                ("release", 0.4),
                ("mod_cutoff_lfo", 0.7),
                ("lforate", 0.6),
                ("chorus", 1.0),
            ],
        ),
        (
            "Wobble",
            Category::Sequence,
            &[
                ("sub", 0.5),
                ("range", 0.0),
                ("cutoff", 0.35),
                ("resonance", 0.5),
                ("mod_cutoff_lfo", 0.8),
                ("lforate", 0.45),
                ("sustain", 1.0),
                ("release", 0.3),
            ],
        ),
        (
            "Drone chord",
            Category::Drone,
            &[
                ("pulse", 1.0),
                ("mod_width_lfo", 0.75),
                ("lforate", 0.1),
                ("cutoff", 0.45),
                ("attack", 0.5),
                ("sustain", 1.0),
                ("release", 1.0),
                ("chorus", 1.0),
            ],
        ),
    ];

    /// One designed sound: its name, its category, and the values that make it.
    type Design = (&'static str, Category, &'static [(&'static str, f32)]);

    /// Writes the fifty factory presets to `plugins/mxm-poly-06/presets/`.
    ///
    /// A facility, not a test — and the *only* thing that writes those files, so the numbers in
    /// `FACTORY_DESIGN` stay the readable statement of each sound and the JSON stays generated
    /// output. `every_factory_preset_covers_every_parameter` is what catches a file that has fallen
    /// behind a new parameter.
    ///
    /// ```text
    /// cargo test -p mxm-poly-06 --lib write_the_factory_presets -- --ignored --nocapture
    /// ```
    #[test]
    #[ignore = "writes the factory preset files"]
    fn write_the_factory_presets() {
        let params = params();

        for (name, category, overrides) in FACTORY_DESIGN {
            let preset = generated(&params, name, *category, overrides);

            // From the manifest directory, not the working one: a test's cwd is the crate root
            // and not the workspace root, which is the sort of thing that only says so once.
            let file = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("presets")
                .join(format!("{}.json", name.to_lowercase().replace(' ', "-")));
            std::fs::write(&file, preset.to_json()).expect("write the preset");
            eprintln!("wrote {}", file.display());
        }
    }

    #[test]
    fn every_designed_preset_names_real_parameters() {
        // Runs by default, unlike the generator: a typo in `FACTORY_DESIGN` would otherwise only
        // surface the next time somebody regenerated the files, and silently leave that value at
        // its default in the meantime.
        let params = params();
        let known = mxm_preset::Instrument::parameters(&params);
        for (name, _category, overrides) in FACTORY_DESIGN {
            for (id, v) in *overrides {
                assert!(
                    known.iter().any(|(known_id, _)| *known_id == *id),
                    "{name:?} names `{id}`, which is not a parameter of this instrument"
                );
                assert!(
                    (0.0..=1.0).contains(v),
                    "{name:?} sets `{id}` to {v}, which is not a normalised value"
                );
            }
        }
    }

    /// What the generator would write for one design, in memory.
    fn generated(
        params: &MxmPoly06Params,
        name: &str,
        category: Category,
        overrides: &[(&str, f32)],
    ) -> Preset {
        let known = mxm_preset::Instrument::parameters(params);
        let mut preset = Preset::init(params);
        preset.name = name.to_owned();
        preset.category = category;
        for (id, v) in overrides {
            let (_, param) = known
                .iter()
                .find(|(known_id, _)| *known_id == *id)
                .unwrap_or_else(|| panic!("{name:?} names `{id}`, which is not a parameter"));
            preset.params.insert(
                (*id).to_owned(),
                Value {
                    v: *v,
                    text: param.format(*v),
                },
            );
        }
        preset
    }

    #[test]
    fn the_factory_files_match_the_design_they_were_generated_from() {
        // The generator is `#[ignore]`d, so nothing forces it to have been run. This is what says
        // the shipped files are the current design rather than a stale one — the same class of
        // mistake as a stale `.clap` bundle, and just as quiet.
        //
        // **The whole preset, `text` included**, not only the overridden values. The first version
        // compared the overrides alone and let files generated before a formatter changed ship
        // with `"5.4 kHz Hz"` in them: never read, but exactly the drift this test exists to see.
        let params = params();
        for (name, category, overrides) in FACTORY_DESIGN {
            let (_, text) = FACTORY_FILES
                .iter()
                .find(|(file_name, _)| file_name == name)
                .unwrap_or_else(|| panic!("no factory file for {name:?}"));
            let shipped = Preset::parse(text, crate::CLAP_ID).expect("parses");
            let expected = generated(&params, name, *category, overrides);
            assert_eq!(
                shipped, expected,
                "{name:?} on disk is not what the design generates — regenerate the files"
            );
        }
    }

    #[test]
    fn the_user_root_is_under_this_instruments_own_id() {
        // Namespaced by CLAP id so another instrument's presets cannot appear in this one's list.
        let Some(root) = user_root(crate::CLAP_ID) else {
            return;
        };
        assert!(root.ends_with("presets"));
        assert!(root.to_string_lossy().contains(crate::CLAP_ID));
    }

    #[test]
    fn every_factory_preset_can_be_heard() {
        // Audibility here is a source, the two levels and the filter. A preset with every source
        // off, or the output at nothing, or the filter shut with no envelope to open it, loads
        // without complaint and reads as the instrument being broken.
        for (name, text) in FACTORY_FILES {
            let preset = Preset::parse(text, crate::CLAP_ID).expect("parses");
            let v = |id: &str| preset.params.get(id).map_or(0.0, |value| value.v);
            assert!(
                v("saw") > 0.5 || v("pulse") > 0.5 || v("sub") > 0.05 || v("noise") > 0.05,
                "{name:?} has every source off"
            );
            assert!(v("level") > 0.05, "{name:?} has the patch level at nothing");
            assert!(v("volume") > 0.05, "{name:?} is turned down to nothing");
            assert!(
                v("cutoff") > 0.05 || v("mod_cutoff_env") > 0.6,
                "{name:?} has the filter shut and nothing to open it"
            );
        }
    }

    #[test]
    fn no_two_factory_presets_are_the_same_sound() {
        // Fifty is enough that a copied-and-edited design could lose its edit unnoticed.
        for (index, (name, text)) in FACTORY_FILES.iter().enumerate() {
            let a = Preset::parse(text, crate::CLAP_ID).expect("parses");
            for (other, text) in &FACTORY_FILES[index + 1..] {
                let b = Preset::parse(text, crate::CLAP_ID).expect("parses");
                assert_ne!(a.params, b.params, "{name:?} and {other:?} are identical");
            }
        }
    }

    #[test]
    fn every_factory_preset_has_a_category() {
        // A sound is saved with its category (the owner's rule, 2026-09-04), and the factory set
        // is where a person first sees what the categories mean. *Uncategorised* is for files
        // written before the field existed, not for sounds this instrument ships.
        for (name, text) in FACTORY_FILES {
            let preset = Preset::parse(text, crate::CLAP_ID).expect("parses");
            assert_ne!(
                preset.category,
                Category::Uncategorised,
                "factory preset {name:?} has no category"
            );
        }
    }

    #[test]
    fn every_factory_preset_parses_and_is_for_this_instrument() {
        // A malformed factory preset is a build mistake, not a user's, so it is caught here rather
        // than skipped quietly in the browser.
        for (name, text) in FACTORY_FILES {
            let preset = Preset::parse(text, crate::CLAP_ID)
                .unwrap_or_else(|e| panic!("factory preset {name:?} does not parse: {e}"));
            assert_eq!(preset.name, *name, "the file's name must match its listing");
        }
    }

    #[test]
    fn every_factory_preset_covers_every_parameter() {
        // The one that catches a factory preset written before a parameter existed: it would load
        // and quietly leave that parameter wherever the last patch left it.
        let params = params();
        for (name, text) in FACTORY_FILES {
            let preset = Preset::parse(text, crate::CLAP_ID).expect("parses");
            let (_, problems) = preset.resolve(&params);
            assert!(
                problems.is_empty(),
                "factory preset {name:?} is incomplete: {problems:?}"
            );
        }
    }

    #[test]
    fn the_factory_list_begins_with_init() {
        let params = params();
        let all = factory(&params);
        assert_eq!(all[0].name, INIT_NAME);
        assert_eq!(all.len(), FACTORY_FILES.len() + 1);
    }

    #[test]
    fn no_factory_preset_sets_the_master_volume() {
        // Volume is an ordinary parameter under the shared preset contract — saved, restored,
        // compared — and what keeps it out of the *sound* of a preset is content, not mechanism:
        // no factory sound moves it. The patch's Level is part of the sound; the master is the
        // person's.
        let params = params();
        let default = params.volume.default_normalized_value();
        for (name, text) in FACTORY_FILES {
            let preset = Preset::parse(text, crate::CLAP_ID).expect("parses");
            let v = preset.params.get("volume").map_or(default, |value| value.v);
            assert!(
                (v - default).abs() < 1e-6,
                "{name:?} sets the master volume, which no factory preset does"
            );
        }
    }
}
