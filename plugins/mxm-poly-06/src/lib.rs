//! mxm-poly-06 — six-voice polysynth with a built-in chorus.
//!
//! Architecture inspired by the Roland JUNO-106: six DCOs that do not drift, one IR3109 filter per
//! voice with the JUNO's compensation around it, one envelope per voice shared by filter and
//! amplifier, a global four-position HPF whose bottom position boosts, and the BBD chorus that does
//! most of the work. Not affiliated with or endorsed by Roland.
//!
//! This file is the plugin shell: identity, parameter plumbing, and MIDI. All the signal processing
//! lives in `mxm-poly-06-dsp`, which knows nothing about nice-plug.
//!
//! # The chorus is inside, and that is the collection's rule
//!
//! *No effect that was not on the original instrument* — and the machine shipped with this one, so
//! it is part of the machine. The output is stereo because the chorus makes it so; with the chorus
//! off the two channels are identical.
//!
//! # Notes reach a ledger, not a voice
//!
//! Six voices and a host that may or may not send note ids: every note event goes to
//! `poly::Synth`, whose ledger decides which voice, whether a repeated press is a continuation, and
//! what a late note-off for a stolen note means. The rules are that module's; this file only
//! translates events.

/// The plugin's name, and the **only** place it is written in this crate.
///
/// Everything else that names the instrument derives from here: [`NAME`], which the host shows, and
/// [`CLAP_ID`], which it remembers. A rename is this line.
///
/// A macro rather than a `const` because [`CLAP_ID`] is built with `concat!`, which takes literals.
macro_rules! plugin_name {
    () => {
        "mxm-poly-06"
    };
}

/// What the host displays.
pub const NAME: &str = plugin_name!();

/// The permanent CLAP identifier.
///
/// **Deliberately assembled from [`plugin_name!`] and not from `CARGO_PKG_NAME`.** Deriving it from
/// the package name would mean a future `git mv` of this directory silently changed the plugin's
/// permanent identity — no compile error, no failing test, and every preset and saved project
/// written under the old id orphaned. Renaming the plugin is a deliberate act that edits
/// `plugin_name!` above: one line, one decision.
pub const CLAP_ID: &str = concat!("dk.mxm.", plugin_name!());

// Public for `apps/mxm-layout-lab` (in the private archive) on the `dynamic-layout` branch: the
// lab draws these real cards outside a host. Nothing else about them changes, and the shipped
// cdylib is unaffected.
pub mod editor;
pub mod params;
pub mod preset;
pub mod routes;
pub mod telemetry;

use mxm_poly_06_dsp::dco::Mix;
use mxm_poly_06_dsp::poly::{Key, Patch, Synth};
use mxm_poly_06_dsp::voice::VoicePatch;
use nice_plug::prelude::*;
use params::{MxmPoly06Params, VcaMode};
use std::sync::Arc;

/// Upper bound on how many samples are rendered between event checks.
///
/// Splitting only on events is not enough: a buffer containing no MIDI at all would otherwise
/// become one arbitrarily long block, and per-sample modulation would be the only thing keeping it
/// honest.
const MAX_BLOCK_SIZE: usize = 64;

/// MIDI channels, for the per-channel bend and wheel state.
const NUM_CHANNELS: usize = 16;

/// **The collection's developer channel, off unless asked for** (`plugins/AGENTS.md`). With
/// `MXM_DEV_CC` set in the plugin's process environment when an instance is made, CC 119 selects the
/// category (0–5) or Parameters (127); CC 118 opens (≥ 64) or closes its expander, through telemetry
/// atomics; the DSP reads nothing. It exists so a script, a screenshot run or an AI can put the
/// editor in a state CLAP gives a host no way to ask for — through the player, `mxm-cli cc 119 1`.
const DEV_VIEW_CC: u8 = 119;
const DEV_DISCLOSURE_CC: u8 = 118;
const DEV_BROWSER_CC: u8 = 117;
const DEV_THEME_CC: u8 = 116;
const DEV_CC_ENV: &str = "MXM_DEV_CC";

pub struct MxmPoly06 {
    params: Arc<MxmPoly06Params>,
    synth: Synth,

    /// Pitch bend per channel, in `-1..=1`. CLAP delivers `0..=1` with 0.5 centred.
    bend: [f32; NUM_CHANNELS],
    /// The mod wheel per channel, `0..=1` — the bender's forward push.
    wheel: [f32; NUM_CHANNELS],
    /// Channel pressure per channel, `0..=1`. A routing source, reduced like the wheel.
    pressure: [f32; NUM_CHANNELS],
    /// The channel of the most recent note-on, whose bend and wheel apply.
    ///
    /// The machine is one instrument on one channel; MPE is not a goal. Per-channel state is kept
    /// so a controller on any channel works, and the most recent channel is the one heard.
    active_channel: u8,

    sample_rate: f32,

    /// DSP -> editor, atomics only. Held even with no editor open: `activate` publishes the sample
    /// rate, and a few atomic stores per block are not worth branching on.
    telemetry: Arc<telemetry::Telemetry>,
    /// Whether the developer channel is on: `DEV_CC_ENV` was set when this instance was made.
    dev_cc: bool,
    /// The LFO rate as its sync resolved it for this buffer, or `None` for the free rate.
    synced_lfo_hz: Option<f32>,
}

impl Default for MxmPoly06 {
    fn default() -> Self {
        Self {
            params: Arc::new(MxmPoly06Params::default()),
            synth: Synth::new(),
            bend: [0.0; NUM_CHANNELS],
            wheel: [0.0; NUM_CHANNELS],
            pressure: [0.0; NUM_CHANNELS],
            active_channel: 0,
            sample_rate: 48_000.0,
            telemetry: telemetry::Telemetry::shared(),
            dev_cc: std::env::var_os(DEV_CC_ENV).is_some(),
            synced_lfo_hz: None,
        }
    }
}

impl MxmPoly06 {
    /// Renders one stereo block through the plugin's own per-sample path, for measurement.
    ///
    /// **A measurement seam, not a second `process()`.** It runs exactly the loop `process()` runs —
    /// the switches once, then `next_patch()` per sample, which advances every smoother and rebuilds
    /// the `Patch`, then `Synth::process` — and it deliberately omits the wrapper's per-block event
    /// handling, telemetry and buffer plumbing, which are not what the modulation work lands on.
    /// `mxm-mono-01` carries the same seam for the same reason.
    ///
    /// It exists because `plans/plan-mxm-poly-06-modulation.md`'s cost gate must measure the
    /// **plugin** path across six voices: the smoothers and the per-sample `Patch` rebuild are where
    /// the routing work lands, and a framework-free DSP bench cannot reach them.
    pub fn render_block_for_test(&mut self, left: &mut [f32], right: &mut [f32]) {
        self.synth.set_switches(
            self.params.hpf.value().into(),
            self.params.chorus.value().into(),
        );
        let routed = self.resolve_topology();
        for (l, r) in left.iter_mut().zip(right.iter_mut()) {
            (*l, *r) = self.render_sample(routed);
        }
    }

    /// Which routes are live this interval, as `process()` resolves it: **once per block**, with
    /// every newly present route's smoother snapped to its stored depth. Returns whether any is.
    fn resolve_topology(&mut self) -> bool {
        let topology = self.params.routes.topology_from(self.synth.routing());
        self.synth.set_topology(&topology);
        topology.any()
    }

    /// One stereo sample through the plugin's own path: the patch, the live routes' amounts, the
    /// synth.
    ///
    /// `lfomod` is read here, beside the patch, because it no longer reaches a voice as a field: it
    /// is the wheel's push into the vibrato route, which [`routes::Routes::advance`] adds.
    #[inline]
    fn render_sample(&mut self, routed: bool) -> (f32, f32) {
        let patch = self.next_patch();
        let push = self.wheel[self.active_channel as usize] * self.params.lfo_mod.smoothed.next();
        if routed {
            self.params.routes.advance(self.synth.routing_mut(), push);
        }
        self.synth.process(&patch)
    }

    /// Build one sample's worth of plain values from the parameter smoothers.
    ///
    /// Called per sample, which is what makes envelope and LFO modulation of the cutoff
    /// sample-accurate. Every smoother must be advanced exactly once per sample: this reads the
    /// patch's, and [`MxmPoly06::render_sample`] reads `lfomod` and the live route amounts beside it.
    #[inline]
    fn next_patch(&self) -> Patch {
        let p = &self.params;
        let channel = self.active_channel as usize;
        let bend = self.bend[channel];

        let voice = VoicePatch {
            range: p.range.value().into(),
            tune_semitones: bend * p.bend_range.smoothed.next(),
            portamento_s: p.portamento.value(),
            // The routing sources a channel owns, reduced to the channel of the latest note-on as
            // the bend always was. They write no parameter.
            wheel: self.wheel[channel],
            pressure: self.pressure[channel],
            bend,
            mix: Mix {
                saw: if p.saw.value() { 1.0 } else { 0.0 },
                pulse: if p.pulse.value() { 1.0 } else { 0.0 },
                sub: p.sub.smoothed.next(),
                noise: p.noise.smoothed.next(),
            },
            pulse_width: p.pulse_width.smoothed.next(),
            cutoff_hz: p.cutoff.smoothed.next(),
            resonance: p.resonance.smoothed.next(),
            // Envelope times set state-machine behaviour and are not smoothed; sustain is a level.
            attack_s: p.attack.value(),
            decay_s: p.decay.value(),
            sustain: p.sustain.smoothed.next(),
            release_s: p.release.value(),
            vca_gate: p.vca_mode.value() == VcaMode::Gate,
        };

        Patch {
            voice,
            // Synced, the LFO runs at its division (`plans/plan-tempo-sync-controls.md`).
            lfo_rate_hz: self.synced_lfo_hz.unwrap_or_else(|| p.lfo_rate.value()),
            lfo_delay_s: p.lfo_delay.value(),
            hpf: p.hpf.value().into(),
            level: p.level.smoothed.next(),
            chorus: p.chorus.value().into(),
            volume: p.volume.smoothed.next(),
            assign: p.key_assign.value().into(),
        }
    }

    fn handle_event(&mut self, event: NoteEvent<()>) {
        match event {
            NoteEvent::NoteOn {
                voice_id,
                channel,
                note,
                velocity,
                ..
            } => {
                // Velocity zero is a note-off by convention. Otherwise velocity reaches the voices it
                // lands on as a **routing source**: the machine's keyboard sent none, and nothing
                // reads it unless a player routes it — decision 1.7's *expand the original*.
                if velocity <= 0.0 {
                    self.synth.note_off(Key { channel, note }, voice_id);
                } else {
                    self.active_channel = channel;
                    let assign = self.params.key_assign.value().into();
                    self.synth
                        .note_on(Key { channel, note }, voice_id, assign, velocity);
                }
            }

            NoteEvent::NoteOff {
                voice_id,
                channel,
                note,
                ..
            } => self.synth.note_off(Key { channel, note }, voice_id),

            // Immediate, no release — for the one note named.
            NoteEvent::Choke {
                voice_id,
                channel,
                note,
                ..
            } => self.synth.choke(Key { channel, note }, voice_id),

            // **Per-note pitch, from the host's piano roll.** CLAP's tuning expression, in semitones,
            // routed by the ledger to the presses it names. A non-finite one is dropped here, and
            // the DSP drops it again.
            NoteEvent::PolyTuning {
                voice_id,
                channel,
                note,
                tuning,
                ..
            } if tuning.is_finite() => {
                self.synth
                    .expression(Key { channel, note }, voice_id, tuning)
            }

            NoteEvent::MidiPitchBend { channel, value, .. } => {
                self.bend[channel as usize % NUM_CHANNELS] = 2.0 * (value - 0.5);
            }

            NoteEvent::MidiCC {
                channel, cc, value, ..
            } => match cc {
                // The collection's developer channel, only when this instance was started with it.
                DEV_VIEW_CC if self.dev_cc => self
                    .telemetry
                    .request_view((value.clamp(0.0, 1.0) * 127.0).round() as u8),
                DEV_DISCLOSURE_CC if self.dev_cc => {
                    self.telemetry.request_disclosure(value >= 0.5);
                }
                DEV_BROWSER_CC if self.dev_cc => {
                    self.telemetry.request_browser(value >= 0.5);
                }
                // A theme by index, 0 light / 1 dark / 2 system, as the app bar's control lists
                // them. Applied to the editor and never saved: a capture run must not rewrite the
                // choice the person at the machine made.
                DEV_THEME_CC if self.dev_cc => {
                    self.telemetry
                        .request_theme((value.clamp(0.0, 1.0) * 127.0).round() as u8);
                }
                // All sound off: immediate, no release.
                control_change::ALL_SOUND_OFF => self.synth.all_sound_off(),
                // All notes off: deliberately different, every note releases normally.
                control_change::ALL_NOTES_OFF => self.synth.all_notes_off(),
                // The mod wheel is the bender's forward push: LFO to pitch, live.
                control_change::MODULATION_MSB => {
                    self.wheel[channel as usize % NUM_CHANNELS] = value;
                }
                _ => {}
            },

            // **Channel pressure is a routing source**, per channel like the wheel and reduced the
            // same way. Nothing reads it unless a player routes it.
            NoteEvent::MidiChannelPressure {
                channel, pressure, ..
            } => {
                self.pressure[channel as usize % NUM_CHANNELS] = pressure;
            }

            // The other per-note expressions are dropped: per-note pressure, vibrato and
            // brightness would be MPE, which this one-channel instrument is not.
            _ => {}
        }
    }
}

impl Plugin for MxmPoly06 {
    const NAME: &'static str = crate::NAME;
    const VENDOR: &'static str = "mxm";
    const URL: &'static str = "https://mxm.dk";
    const EMAIL: &'static str = "plugins@mxm.dk";
    const VERSION: &'static str = env!("CARGO_PKG_VERSION");

    /// An instrument: no main input. **Stereo**, because the chorus makes the stereo; a mono layout
    /// is offered for hosts that insist, and sums the two channels.
    const AUDIO_IO_LAYOUTS: &'static [AudioIOLayout] = &[
        AudioIOLayout {
            main_input_channels: None,
            main_output_channels: NonZeroU32::new(2),
            ..AudioIOLayout::const_default()
        },
        AudioIOLayout {
            main_input_channels: None,
            main_output_channels: NonZeroU32::new(1),
            ..AudioIOLayout::const_default()
        },
    ];

    /// `MidiCCs` rather than `Basic`: pitch bend, CC 1, CC 120 and CC 123 are all needed. Declaring
    /// MIDI input is also what makes a host's panic actually clear a stuck note.
    const MIDI_INPUT: MidiConfig = MidiConfig::MidiCCs;

    /// Smoothers advance per sample inside each event-delimited block, which already removes zipper
    /// noise; splitting a second time buys little here.
    const SAMPLE_ACCURATE_AUTOMATION: bool = false;

    type Editor = editor::MxmPoly06Editor;
    type SysExMessage = ();
    type BackgroundTask = ();

    fn params(&self) -> Arc<dyn Params> {
        self.params.clone()
    }

    fn editor(&mut self, _async_executor: AsyncExecutor<Self>) -> Option<Self::Editor> {
        editor::create(self.params.clone(), self.telemetry.clone())
    }

    fn activate(
        &mut self,
        _audio_io_layout: &AudioIOLayout,
        buffer_config: &BufferConfig,
        _context: &mut impl ActivateContext<Self>,
    ) -> bool {
        // A new activation starts with no tempo and nothing resolved: the first callback reports
        // the tempo, so neither the audio nor an editor frame before it shows the last session's
        // divisions (`plans/plan-tempo-sync-controls.md`).
        self.telemetry.tempo.publish(None);
        self.synced_lfo_hz = None;
        // A rate the DSP's clamps cannot hold is refused before anything changes: a NaN, or one
        // below `MIN_SAMPLE_RATE`, crosses a `clamp` bound and panics on the audio thread.
        if !buffer_config.sample_rate.is_finite()
            || buffer_config.sample_rate < mxm_poly_06_dsp::MIN_SAMPLE_RATE
        {
            return false;
        }
        self.sample_rate = buffer_config.sample_rate;
        // `set_sample_rate` allocates the chorus's delay lines. Here, never in `process`.
        self.synth.set_sample_rate(self.sample_rate);
        self.synth.reset();
        self.telemetry.publish_sample_rate(self.sample_rate);
        true
    }

    fn reset(&mut self) {
        self.synth.reset();
        self.bend = [0.0; NUM_CHANNELS];
        self.wheel = [0.0; NUM_CHANNELS];
        self.pressure = [0.0; NUM_CHANNELS];
        self.active_channel = 0;
    }

    /// **A project saved before the tempo syncs** restores each Off rather than keeping this
    /// instance's, and a loaded preset's baseline gains it, so the preset stays clean
    /// (`mxm_preset::add_switches_off`).
    fn filter_state(state: &mut PluginState) {
        mxm_preset::add_switches_off(state, crate::preset::TEMPO_SYNC_IDS);
    }

    fn process(
        &mut self,
        buffer: &mut Buffer,
        _aux: &mut AuxiliaryBuffers,
        context: &mut impl ProcessContext<Self>,
    ) -> ProcessStatus {
        let num_samples = buffer.samples();
        let mut next_event = context.next_event();
        let mut block_start = 0usize;

        // Control-rate settings once per call: the HPF position and the chorus mode are switches,
        // read as values. **Not through `next_patch`**: that advances every smoother, and calling
        // it here advanced each one an extra sample per host buffer — which made smoothing depend
        // on the buffer size. Found in review.
        self.synth.set_switches(
            self.params.hpf.value().into(),
            self.params.chorus.value().into(),
        );
        // **Topology, once per buffer.** Which routes are live changes only on a parameter event —
        // or on a host state restore, which sends none, which is why it is re-read every buffer.
        let routed = self.resolve_topology();
        // The LFO's tempo sync, once per buffer, and the tempo in force for the editor's reading.
        let tempo = context.transport().tempo;
        self.synced_lfo_hz = self.params.synced_lfo_rate(tempo);
        self.telemetry.tempo.publish(tempo);

        while block_start < num_samples {
            let mut block_end = (block_start + MAX_BLOCK_SIZE).min(num_samples);

            // Apply everything scheduled at or before this point, then shorten the block so the
            // next event lands exactly where it should.
            loop {
                match next_event {
                    Some(event) if (event.timing() as usize) <= block_start => {
                        self.handle_event(event);
                        next_event = context.next_event();
                    }
                    Some(event) if (event.timing() as usize) < block_end => {
                        block_end = event.timing() as usize;
                        break;
                    }
                    _ => break,
                }
            }

            {
                let output = buffer.as_slice();
                let mut block_peak = 0.0f32;
                for i in block_start..block_end {
                    let (l, r) = self.render_sample(routed);
                    block_peak = block_peak.max(l.abs()).max(r.abs());
                    match output.len() {
                        0 => {}
                        1 => output[0][i] = 0.5 * (l + r),
                        _ => {
                            output[0][i] = l;
                            output[1][i] = r;
                        }
                    }
                }
                self.telemetry.publish_peak(block_peak);
            }

            block_start = block_end;
        }

        self.telemetry
            .publish_voices(&self.synth.voice_levels(), &self.synth.voice_notes());
        self.telemetry.publish_chorus_lfo(self.synth.chorus_lfo());

        if self.synth.is_active() {
            ProcessStatus::Tail(self.synth.tail_samples(self.params.release.value()))
        } else {
            ProcessStatus::Normal
        }
    }
}

impl ClapPlugin for MxmPoly06 {
    /// Permanent. Reverse DNS of a domain the project owns. Changing it breaks every saved project
    /// using the plugin.
    const CLAP_ID: &'static str = CLAP_ID;
    const CLAP_DESCRIPTION: Option<&'static str> = Some(
        "A six-voice polyphonic synthesizer with one envelope per voice and a built-in bucket-brigade (BBD) chorus",
    );
    const CLAP_MANUAL_URL: Option<&'static str> = None;
    const CLAP_SUPPORT_URL: Option<&'static str> = None;
    const CLAP_FEATURES: &'static [ClapFeature] = &[
        ClapFeature::Instrument,
        ClapFeature::Synthesizer,
        ClapFeature::Stereo,
    ];

    // **Polyphonic modulation is deliberately not declared.** nice-plug couples the voice-info
    // extension to `CLAP_POLY_MODULATION_CONFIG`, which advertises a host offsetting a *parameter*
    // for one voice. The routing added per-voice *sources*, not per-voice parameter destinations,
    // so declaring it would promise a separate feature. Honest is better than advertised-and-dropped.
}

nice_export_clap!(MxmPoly06);

/// The plugin's name, checked where it escapes this crate.
#[cfg(test)]
mod identity {
    use super::{CLAP_ID, NAME};

    /// The id is built from the name, so it cannot drift from it.
    #[test]
    fn the_id_is_the_name_under_the_project_domain() {
        assert_eq!(CLAP_ID, format!("dk.mxm.{NAME}"));
    }

    /// `bundler.toml` names the same instrument this crate does.
    ///
    /// **The one place the plugin's name is duplicated outside this crate**, and nothing else would
    /// catch a disagreement: `bundler.toml` is read by `xtask` at bundle time, never by the plugin,
    /// so a stale display name there produces a correctly-built bundle under the wrong filename.
    #[test]
    fn the_bundle_is_named_after_this_plugin() {
        mxm_plugin_test::bundle::is_named(env!("CARGO_MANIFEST_DIR"), env!("CARGO_PKG_NAME"), NAME);
    }
}

#[cfg(test)]
mod init_patch {
    use super::params::{ChorusMode, HpfPosition, KeyAssign, MxmPoly06Params, VcaMode};
    use nice_plug::prelude::*;

    /// **Pins the rule, not the taste.** A retune of any control survives this; making a depth
    /// non-zero because it sounded nice does not.
    #[test]
    fn every_amount_starts_at_zero() {
        let p = MxmPoly06Params::default();
        assert_eq!(p.resonance.value(), 0.0, "resonance is an amount");
        assert_eq!(
            p.portamento.value(),
            0.0,
            "portamento is an amount here: it is always engaged"
        );
        assert_eq!(p.lfo_delay.value(), 0.0, "the LFO delay holds nothing back");
        assert_eq!(
            p.chorus.value(),
            ChorusMode::Off,
            "the chorus is the amount of an effect"
        );
        // **Every modulation depth is a route amount now**, so the rule is checked where the depths
        // live: all forty-four pairs, not the six that had sliders. A signed amount is stored
        // normalised, so zero is one half.
        for (t, group) in p.routes.each().into_iter().enumerate() {
            for route in group.routes(t) {
                assert_eq!(
                    route.amount.default_normalised(),
                    0.5,
                    "`{}` is a modulation depth and must start at zero",
                    route.amount_id
                );
            }
        }
    }

    /// One plain source sounds; the rest are silent.
    #[test]
    fn one_plain_source_sounds() {
        let p = MxmPoly06Params::default();
        assert!(p.saw.value());
        assert!(!p.pulse.value());
        assert_eq!(p.sub.value(), 0.0);
        assert_eq!(p.noise.value(), 0.0);
    }

    /// Configurations start somewhere musically useful.
    #[test]
    fn configurations_start_somewhere_useful() {
        let p = MxmPoly06Params::default();
        assert!(
            p.cutoff.value() >= 12_000.0,
            "the filter starts open: {}",
            p.cutoff.value()
        );
        assert!(p.cutoff.value() < 20_000.0, "and short of the range's end");
        assert!(
            (p.pulse_width.value() - 0.5).abs() < 1e-6,
            "a square, until PWM is asked for"
        );
        assert!(
            p.lfo_rate.value() > 3.0 && p.lfo_rate.value() < 8.0,
            "a vibrato rate"
        );
        assert_eq!(p.hpf.value(), HpfPosition::Flat, "the neutral HPF position");
        assert_eq!(p.vca_mode.value(), VcaMode::Envelope);
        assert_eq!(p.key_assign.value(), KeyAssign::Poly1);
        assert!(p.sustain.value() > 0.5, "a sustained shape, for chords");
    }

    /// **The wheel's reach is a configuration, not an amount** — recorded in the plugin's AGENTS.md.
    /// At zero the mod wheel would do nothing, which reads as a broken wheel, not a neutral patch.
    #[test]
    fn the_wheel_reaches_something_at_init() {
        let p = MxmPoly06Params::default();
        assert!(p.lfo_mod.value() > 0.0);
    }

    /// **The init patch wires the machine's own six routes and nothing else**, at zero depth: the
    /// parameter defaults and the DSP's own init wiring are one set.
    #[test]
    fn the_init_patch_wires_exactly_the_machines_own_routes() {
        let p = MxmPoly06Params::default();
        assert_eq!(
            p.routes.topology().present,
            mxm_poly_06_dsp::routing::Routing::init().present,
            "the parameter defaults and the DSP's own init wiring have drifted apart"
        );
    }

    /// **The retired ids stay retired.** A name whose meaning changed must not be quietly re-used,
    /// and the count says where every parameter went.
    #[test]
    fn no_retired_id_reappears() {
        let ids: Vec<String> = MxmPoly06Params::default()
            .param_map()
            .into_iter()
            .map(|(id, _, _)| id)
            .collect();
        for retired in [
            "dcolfo",
            "pwmdepth",
            "pwmmode",
            "envamount",
            "envpolarity",
            "vcflfo",
            "keytrack",
            "bendfilter",
        ] {
            assert!(
                !ids.iter().any(|i| i == retired),
                "`{retired}` is a retired id and has come back"
            );
        }
        assert_eq!(
            ids.len(),
            24 + 4 * 11 * 2,
            "twenty-four of its own, and a presence and an amount per routing pair"
        );
    }
}

/// The path from a host's note events to the ledger.
#[cfg(test)]
mod events {
    use super::MxmPoly06;
    use nice_plug::prelude::*;

    fn note_on(plugin: &mut MxmPoly06, note: u8, voice_id: Option<i32>) {
        plugin.handle_event(NoteEvent::NoteOn {
            timing: 0,
            voice_id,
            channel: 0,
            note,
            velocity: 0.8,
        });
    }

    fn note_off(plugin: &mut MxmPoly06, note: u8, voice_id: Option<i32>) {
        plugin.handle_event(NoteEvent::NoteOff {
            timing: 0,
            voice_id,
            channel: 0,
            note,
            velocity: 0.0,
        });
    }

    #[test]
    fn six_notes_hold_six_voices_and_release_frees_them() {
        let mut plugin = MxmPoly06::default();
        for n in 0..6 {
            note_on(&mut plugin, 60 + n, None);
        }
        assert_eq!(plugin.synth.held_voices(), 0b11_1111);
        for n in 0..6 {
            note_off(&mut plugin, 60 + n, None);
        }
        assert_eq!(plugin.synth.held_voices(), 0);
    }

    #[test]
    fn a_velocity_zero_note_on_is_a_note_off() {
        let mut plugin = MxmPoly06::default();
        note_on(&mut plugin, 60, None);
        plugin.handle_event(NoteEvent::NoteOn {
            timing: 0,
            voice_id: None,
            channel: 0,
            note: 60,
            velocity: 0.0,
        });
        assert_eq!(plugin.synth.held_voices(), 0);
    }

    /// What the wrapper does at activation, so the smoothers hold their parameters' values rather
    /// than the zero they are constructed with.
    fn activate_smoothers(plugin: &MxmPoly06) {
        for (_, ptr, _) in plugin.params.param_map() {
            // SAFETY: the same call `nice_plug`'s CLAP wrapper makes in `activate`, on pointers
            // that came from this plugin's own `Params` and outlive the call.
            unsafe { ptr._internal_update_smoother(48_000.0, true) };
        }
    }

    /// **A route that arrives after an idle span renders the same whatever the block size** — its
    /// depth mid-ramp as it arrives, and a standing route's depth moving too. The topology resolves
    /// once per block and the amounts advance once per sample; reading either per block would make the
    /// host's buffer size part of the sound (`plans/plan-modulation-routing.md` §10).
    #[test]
    fn a_route_arriving_after_an_idle_span_is_block_partition_invariant() {
        use nice_plug::params::InternalParamMut;
        let render = |block: usize| -> (Vec<f32>, Vec<f32>) {
            let mut plugin = MxmPoly06::default();
            activate_smoothers(&plugin);
            let (mut left, mut right) = (vec![0.0f32; 96_000], vec![0.0f32; 96_000]);
            for (l, r) in left[..48_000]
                .chunks_mut(block)
                .zip(right[..48_000].chunks_mut(block))
            {
                plugin.render_block_for_test(l, r);
            }
            unsafe {
                let routes = &plugin.params.routes;
                routes.cutoff.saw_on._internal_set_plain_value(true);
                routes.cutoff.saw._internal_set_plain_value(0.3);
                routes.cutoff.saw._internal_update_smoother(48_000.0, false);
                routes.pitch.lfo._internal_set_plain_value(0.2);
                routes.pitch.lfo._internal_update_smoother(48_000.0, false);
            }
            note_on(&mut plugin, 48, None);
            for (l, r) in left[48_000..]
                .chunks_mut(block)
                .zip(right[48_000..].chunks_mut(block))
            {
                plugin.render_block_for_test(l, r);
            }
            (left, right)
        };
        let whole = render(64);
        assert!(
            whole.0[48_000..].iter().any(|s| s.abs() > 1e-3),
            "the premise: it plays"
        );
        assert!(whole == render(37), "64 against 37 samples a block");
        assert!(whole == render(1024), "64 against 1024 samples a block");
    }

    #[test]
    fn the_mod_wheel_adds_lfo_to_the_pitch_by_the_wheel_amount() {
        use mxm_poly_06_dsp::routing::{source, target};
        let mut plugin = MxmPoly06::default();
        activate_smoothers(&plugin);
        let (mut left, mut right) = ([0.0f32; 1], [0.0f32; 1]);
        plugin.render_block_for_test(&mut left, &mut right);
        assert_eq!(
            plugin.synth.routing().amounts[target::PITCH][source::LFO],
            0.0,
            "the premise: no depth at Init"
        );
        plugin.handle_event(NoteEvent::MidiCC {
            timing: 0,
            channel: 0,
            cc: control_change::MODULATION_MSB,
            value: 1.0,
        });
        plugin.render_block_for_test(&mut left, &mut right);
        let reach = plugin.params.lfo_mod.value();
        assert!(
            (plugin.synth.routing().amounts[target::PITCH][source::LFO] - reach).abs() < 1e-3,
            "the wheel fully forward reaches the configured amount on the vibrato route"
        );
    }

    #[test]
    fn all_sound_off_silences_everything() {
        let mut plugin = MxmPoly06::default();
        for n in 0..4 {
            note_on(&mut plugin, 60 + n, Some(n as i32));
        }
        plugin.handle_event(NoteEvent::MidiCC {
            timing: 0,
            channel: 0,
            cc: control_change::ALL_SOUND_OFF,
            value: 0.0,
        });
        assert_eq!(plugin.synth.held_voices(), 0);
        assert!(!plugin.synth.is_active());
    }

    /// Renders a short stretch through the plugin's own synth, with the smoothers activated.
    fn render(plugin: &mut MxmPoly06, samples: usize) -> Vec<(f32, f32)> {
        activate_smoothers(plugin);
        let patch = plugin.next_patch();
        plugin.synth.prepare(&patch);
        (0..samples)
            .map(|_| {
                let patch = plugin.next_patch();
                plugin.synth.process(&patch)
            })
            .collect()
    }

    /// **An oracle that can fail.** The first version asserted only that the held voices were
    /// unchanged, which held with the `PolyTuning` arm deleted. This compares renders: an
    /// expression naming a sounding note changes the sound, and one naming a note nobody plays
    /// does not.
    #[test]
    fn a_tuning_expression_reaches_the_note_it_names_and_no_other() {
        let tuning = |voice_id: Option<i32>, note: u8| NoteEvent::PolyTuning {
            timing: 0,
            voice_id,
            channel: 0,
            note,
            tuning: 7.0,
        };
        let setup = || {
            let mut plugin = MxmPoly06::default();
            note_on(&mut plugin, 60, Some(1));
            note_on(&mut plugin, 64, Some(2));
            plugin
        };

        let mut plain = setup();
        let reference = render(&mut plain, 4_000);

        let mut bent = setup();
        bent.handle_event(tuning(Some(2), 64));
        assert_ne!(
            render(&mut bent, 4_000),
            reference,
            "an expression for a sounding note must change the render"
        );

        let mut other = setup();
        other.handle_event(tuning(Some(99), 72));
        assert_eq!(
            render(&mut other, 4_000),
            reference,
            "an expression for a note nobody is playing must change nothing"
        );
    }

    /// **A non-finite tuning is dropped at the event**, and the press keeps the offset it had. A
    /// NaN in a voice's pitch sum reaches its DCO, whose phase never recovers. The reference is the
    /// same instance never sent it.
    #[test]
    fn a_non_finite_tuning_expression_is_dropped_and_the_pitch_stays_finite() {
        let tuning = |tuning: f32| NoteEvent::PolyTuning {
            timing: 0,
            voice_id: Some(2),
            channel: 0,
            note: 64,
            tuning,
        };
        let setup = || {
            let mut plugin = MxmPoly06::default();
            note_on(&mut plugin, 60, Some(1));
            note_on(&mut plugin, 64, Some(2));
            plugin.handle_event(tuning(3.0));
            plugin
        };
        for bad in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            let (mut actual, mut reference) = (setup(), setup());
            actual.handle_event(tuning(bad));
            let heard = render(&mut actual, 4_000);
            assert!(
                heard.iter().all(|(l, r)| l.is_finite() && r.is_finite()),
                "{bad}"
            );
            assert_eq!(heard, render(&mut reference, 4_000), "{bad}");
            assert!(heard.iter().any(|(l, _)| *l != 0.0), "the chord sounds");
        }
    }
}

#[cfg(test)]
mod developer_channel_tests {
    use super::*;

    fn cc(plugin: &mut MxmPoly06, cc: u8, raw: u8) {
        plugin.handle_event(NoteEvent::MidiCC {
            timing: 0,
            channel: 0,
            cc,
            value: f32::from(raw) / 127.0,
        });
    }

    /// The developer channel reaches the editor only when the instance was started with it; a
    /// host sending the same control change to an ordinary instance changes nothing.
    #[test]
    fn the_developer_channel_is_off_unless_the_environment_asked_for_it() {
        let mut plugin = MxmPoly06 {
            dev_cc: false,
            ..Default::default()
        };
        cc(&mut plugin, DEV_VIEW_CC, 1);
        cc(&mut plugin, DEV_DISCLOSURE_CC, 127);
        cc(&mut plugin, DEV_BROWSER_CC, 127);
        cc(&mut plugin, DEV_THEME_CC, 1);
        assert_eq!(plugin.telemetry.take_view_request(), None);
        assert_eq!(plugin.telemetry.take_disclosure_request(), None);
        assert_eq!(plugin.telemetry.take_browser_request(), None);
        assert_eq!(plugin.telemetry.take_theme_request(), None);

        plugin.dev_cc = true;
        cc(&mut plugin, DEV_VIEW_CC, 1);
        cc(&mut plugin, DEV_DISCLOSURE_CC, 127);
        cc(&mut plugin, DEV_BROWSER_CC, 127);
        cc(&mut plugin, DEV_THEME_CC, 1);
        assert_eq!(plugin.telemetry.take_view_request(), Some(1));
        assert_eq!(plugin.telemetry.take_disclosure_request(), Some(true));
        assert_eq!(plugin.telemetry.take_browser_request(), Some(true));
        assert_eq!(
            plugin.telemetry.take_theme_request(),
            Some(1),
            "1 is dark, as mxm_ui::theme::from_index reads it"
        );
        // Light is index 0 — a request like any other, not the absence of one.
        cc(&mut plugin, DEV_THEME_CC, 0);
        assert_eq!(plugin.telemetry.take_theme_request(), Some(0));
        // View zero is a request too, not the absence of one.
        cc(&mut plugin, DEV_VIEW_CC, 0);
        assert_eq!(plugin.telemetry.take_view_request(), Some(0));
    }
}

/// **The pre-conversion reference, captured at M0** (`plans/plan-mxm-poly-06-modulation.md` §9).
///
/// What the routing conversion is held to, and what stops existing the moment it starts: throughput
/// through the plugin's own per-sample path, and a digest of every factory sound. `BASELINE-M0.md`
/// records the figures.
///
/// ```text
/// cargo test -p mxm-poly-06 --release baseline -- --ignored --nocapture
/// ```
///
/// **Release, and a quiet machine, or the throughput means nothing** (mxm-kit's
/// `docs/code-review-notes.md` §3). The digests are deterministic and care about neither.
#[cfg(test)]
mod baseline {
    use super::*;
    use std::time::Instant;

    const FS: f32 = 48_000.0;
    const BLOCK: usize = 64;
    /// A triad, so every factory sound is heard as the polysynth it is.
    const CHORD: [u8; 3] = [48, 52, 55];

    /// The machine's own modulation paths all in use, as the routed comparison patch the conversion
    /// is measured against: the routes that replaced M0's `dcolfo` 0.3, `pwmdepth` 0.5 in LFO
    /// mode, `envamount` 0.6, `vcflfo` 0.4 and `keytrack` 0.5, at the same depths. Normalised values
    /// by permanent id, so a depth `d` is `(d + 1) / 2`.
    const ROUTED: &[(&str, f32)] = &[
        ("mod_pitch_lfo", 0.65),
        ("mod_width_lfo", 0.75),
        ("mod_cutoff_env", 0.8),
        ("mod_cutoff_lfo", 0.7),
        ("mod_cutoff_key", 0.75),
    ];

    /// What `activate` does, minus the host. Gotcha 13 in mxm-kit's `docs/adding-an-instrument.md`:
    /// a smoother reads zero until it is updated.
    fn plugin() -> MxmPoly06 {
        let mut plugin = MxmPoly06::default();
        activate_smoothers(&plugin);
        plugin.sample_rate = FS;
        plugin.synth.set_sample_rate(FS);
        plugin.synth.reset();
        plugin
    }

    fn activate_smoothers(plugin: &MxmPoly06) {
        for (_, ptr, _) in plugin.params.param_map() {
            // SAFETY: the call nice-plug's wrapper makes in `activate`, on this plugin's own params.
            unsafe { ptr._internal_update_smoother(FS, true) };
        }
    }

    fn note(plugin: &mut MxmPoly06, note: u8, on: bool) {
        plugin.handle_event(if on {
            NoteEvent::NoteOn {
                timing: 0,
                voice_id: None,
                channel: 0,
                note,
                velocity: 0.8,
            }
        } else {
            NoteEvent::NoteOff {
                timing: 0,
                voice_id: None,
                channel: 0,
                note,
                velocity: 0.0,
            }
        });
    }

    /// Sets parameters by permanent id and normalised value, then re-activates the smoothers.
    fn set(plugin: &MxmPoly06, values: &[(&str, f32)]) {
        let map = plugin.params.param_map();
        for (id, v) in values {
            let (_, ptr, _) = map
                .iter()
                .find(|(i, _, _)| i.as_str() == *id)
                .unwrap_or_else(|| panic!("no parameter `{id}`"));
            // SAFETY: a pointer from this plugin's own params, written as the preset system does.
            let _ = unsafe { ptr._internal_set_normalized_value(*v) };
        }
        activate_smoothers(plugin);
    }

    fn measure(label: &str, plugin: &mut MxmPoly06) {
        let mut left = [0.0f32; BLOCK];
        let mut right = [0.0f32; BLOCK];
        // Warm the caches first: the first blocks pay for page faults, which is not the question.
        for _ in 0..64 {
            plugin.render_block_for_test(&mut left, &mut right);
        }
        let blocks = 20_000;
        let start = Instant::now();
        for _ in 0..blocks {
            plugin.render_block_for_test(&mut left, &mut right);
        }
        let taken = start.elapsed().as_secs_f64();
        let samples = (blocks * BLOCK) as f64;
        println!(
            "  {label:<36} {:>9.3} ns/sample  {:>7.1}x realtime",
            taken * 1e9 / samples,
            samples / taken / f64::from(FS)
        );
    }

    /// Nanoseconds per sample through the plugin's own path: idle, a six-note chord on the init
    /// patch — the case routing multiplies by six — and the same chord on [`ROUTED`].
    #[test]
    #[ignore = "a measurement, not an assertion; release only, on a quiet machine"]
    fn throughput() {
        println!("\n  mxm-poly-06, pre-conversion baseline, {FS} Hz, block {BLOCK}");

        let mut idle = plugin();
        measure("Init, idle", &mut idle);

        let mut chord = plugin();
        for n in 60..66 {
            note(&mut chord, n, true);
        }
        measure("Init, six-note chord held", &mut chord);

        let mut routed = plugin();
        set(&routed, ROUTED);
        for n in 60..66 {
            note(&mut routed, n, true);
        }
        measure("Routed patch, six-note chord held", &mut routed);
        println!();
    }

    /// FNV-1a over the raw bits, the digest `plugins/mxm-poly-06/host-tests/tests/golden_audio.rs`
    /// already uses.
    fn digest(samples: &[f32]) -> String {
        let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
        for sample in samples {
            for byte in sample.to_bits().to_le_bytes() {
                hash ^= u64::from(byte);
                hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
            }
        }
        format!("{hash:016x}")
    }

    /// Renders `samples` frames, appending left and right interleaved.
    fn render(plugin: &mut MxmPoly06, samples: usize, out: &mut Vec<f32>) {
        let mut left = [0.0f32; BLOCK];
        let mut right = [0.0f32; BLOCK];
        let mut done = 0;
        while done < samples {
            let n = BLOCK.min(samples - done);
            plugin.render_block_for_test(&mut left[..n], &mut right[..n]);
            for (l, r) in left[..n].iter().zip(&right[..n]) {
                out.push(*l);
                out.push(*r);
            }
            done += n;
        }
    }

    /// The triad held 1.5 s then released with 2.5 s of tail — the same gesture for every preset,
    /// so a digest change is attributable to the patch and not to the playing.
    fn render_one(plugin: &mut MxmPoly06) -> Vec<f32> {
        let held = (FS * 1.5) as usize;
        let tail = (FS * 2.5) as usize;
        let mut out = Vec::with_capacity(2 * (held + tail));
        for n in CHORD {
            note(plugin, n, true);
        }
        render(plugin, held, &mut out);
        for n in CHORD {
            note(plugin, n, false);
        }
        render(plugin, tail, &mut out);
        out
    }

    /// Applies a factory preset's stored values by id, the way the preset system does: **only `v`
    /// is read**. Positional rather than a parser, which is all a measurement harness needs.
    fn apply(plugin: &MxmPoly06, json: &str) -> usize {
        let map = plugin.params.param_map();
        let mut applied = 0;
        for (id, ptr, _) in &map {
            let key = format!("\"{id}\"");
            let Some(at) = json.find(&key) else { continue };
            let rest = &json[at + key.len()..];
            let Some(vpos) = rest.find("\"v\"") else {
                continue;
            };
            let number: String = rest[vpos + 3..]
                .chars()
                .skip_while(|c| *c == ':' || c.is_whitespace())
                .take_while(|c| c.is_ascii_digit() || matches!(c, '.' | '-' | 'e'))
                .collect();
            if let Ok(v) = number.parse::<f32>() {
                // SAFETY: a pointer from this plugin's own params, written as the preset system does.
                let _ = unsafe { ptr._internal_set_normalized_value(v) };
                applied += 1;
            }
        }
        activate_smoothers(plugin);
        applied
    }

    /// Prints a digest and a peak for Init and for every factory sound: the reference the
    /// conversion is held to. It prints rather than asserts at M0, because after the conversion the
    /// old renders cannot be produced.
    #[test]
    #[ignore = "a capture, not an assertion; release only"]
    fn factory_bank_reference_digests() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("presets");
        let mut names: Vec<_> = std::fs::read_dir(&dir)
            .expect("presets directory")
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().to_string())
            .filter(|n| n.ends_with(".json"))
            .collect();
        names.sort();

        println!("\n  mxm-poly-06 — pre-conversion reference digests");
        println!("  {FS} Hz, triad {CHORD:?} held 1.5 s then 2.5 s release, stereo interleaved\n");

        let mut init = plugin();
        let audio = render_one(&mut init);
        let peak = audio.iter().fold(0.0f32, |m, s| m.max(s.abs()));
        println!("  {:<28} {}  peak {peak:>6.4}", "(init)", digest(&audio));

        for name in &names {
            let json = std::fs::read_to_string(dir.join(name)).expect("preset");
            let mut plugin = plugin();
            let applied = apply(&plugin, &json);
            let audio = render_one(&mut plugin);
            let peak = audio.iter().fold(0.0f32, |m, s| m.max(s.abs()));
            println!(
                "  {:<28} {}  peak {peak:>6.4}  ({applied} params)",
                name.trim_end_matches(".json"),
                digest(&audio)
            );
        }
        println!("\n  {} sounds\n", names.len());
    }
}

/// The host's sample rate at activation: the floor the DSP's clamps are safe above.
#[cfg(test)]
mod sample_rate_floor {
    use super::*;
    use mxm_poly_06_dsp::MIN_SAMPLE_RATE;

    struct Activation;

    impl ActivateContext<MxmPoly06> for Activation {
        fn plugin_api(&self) -> PluginApi {
            PluginApi::Clap
        }
        fn execute(&self, _task: ()) {}
        fn set_latency_samples(&self, _samples: u32) {}
        fn set_current_voice_capacity(&self, _capacity: u32) {}
    }

    fn activate_at(plugin: &mut MxmPoly06, sample_rate: f32) -> bool {
        plugin.activate(
            &MxmPoly06::AUDIO_IO_LAYOUTS[0],
            &BufferConfig {
                sample_rate,
                min_buffer_size: Some(1),
                max_buffer_size: 4096,
                process_mode: ProcessMode::Realtime,
            },
            &mut Activation,
        )
    }

    fn render(plugin: &mut MxmPoly06, frames: usize) -> Vec<f32> {
        let (mut left, mut right) = (vec![0.0f32; frames], vec![0.0f32; frames]);
        plugin.render_block_for_test(&mut left, &mut right);
        left.extend(right);
        left
    }

    /// **The floor activates and plays, whatever the parameters say.** Every parameter at its
    /// default, then all at the bottom of their ranges, then all at the top — every route present
    /// at full — with a note held for four seconds at 1 kHz.
    #[test]
    fn the_rate_floor_activates_and_plays_at_every_parameter_extreme() {
        for extreme in [None, Some(0.0), Some(1.0)] {
            let mut plugin = MxmPoly06::default();
            for (_, ptr, _) in plugin.params.param_map() {
                if let Some(value) = extreme {
                    let _ = unsafe { ptr._internal_set_normalized_value(value) };
                }
                unsafe { ptr._internal_update_smoother(MIN_SAMPLE_RATE, true) };
            }
            assert!(activate_at(&mut plugin, MIN_SAMPLE_RATE), "{extreme:?}");
            assert_eq!(plugin.sample_rate, MIN_SAMPLE_RATE);
            plugin.handle_event(NoteEvent::NoteOn {
                timing: 0,
                voice_id: None,
                channel: 0,
                note: 48,
                velocity: 0.8,
            });
            let out = render(&mut plugin, 4_000);
            assert!(out.iter().all(|s| s.is_finite()), "{extreme:?}");
        }
    }

    /// **A rate the DSP's clamps cannot hold is refused at activation.** `f32::clamp` panics on
    /// a NaN or inverted bound, so a NaN rate or one low enough to cross a corner's floor over its
    /// Nyquist fraction panicked on the audio thread. A refusal leaves the plugin as it was.
    #[test]
    fn activation_refuses_a_non_finite_rate_and_any_below_the_floor() {
        for unsupported in [
            MIN_SAMPLE_RATE.next_down(),
            100.0,
            20.0,
            1.0,
            0.0,
            -48_000.0,
            f32::NAN,
            f32::INFINITY,
            f32::NEG_INFINITY,
        ] {
            let mut refused = MxmPoly06::default();
            assert!(
                !activate_at(&mut refused, unsupported),
                "accepted {unsupported} Hz"
            );
            assert_eq!(refused.sample_rate, 48_000.0, "{unsupported} Hz");
        }
    }
}

/// **A synced value reaches the patch** (`plans/plan-tempo-sync-controls.md`): what a sync resolved
/// for this callback is what the DSP is given, and with none the free value is.
#[cfg(test)]
mod tempo_sync_path {
    use super::*;

    #[test]
    fn the_synced_lfo_rate_is_the_patchs() {
        let mut plugin = MxmPoly06::default();
        let free = plugin.next_patch().lfo_rate_hz;
        plugin.synced_lfo_hz = Some(free + 1.0);
        assert_eq!(plugin.next_patch().lfo_rate_hz, free + 1.0);
        plugin.synced_lfo_hz = None;
        assert_eq!(plugin.next_patch().lfo_rate_hz, free);
    }
}

/// **Activation forgets the last session's tempo and resolved syncs**: the first callback reports the
/// tempo, so nothing — the audio, or an editor frame before it — starts from the previous session's
/// divisions.
#[cfg(test)]
mod activation_forgets_the_tempo {
    use super::*;

    #[test]
    fn activation_forgets_the_last_tempo_and_resolved_syncs() {
        use nice_plug::prelude::Plugin as _;
        let mut plugin = MxmPoly06::default();
        plugin.telemetry.tempo.publish(Some(120.0));
        plugin.synced_lfo_hz = Some(1.0);
        let layout = MxmPoly06::AUDIO_IO_LAYOUTS[0];
        let config = BufferConfig {
            sample_rate: 48_000.0,
            min_buffer_size: None,
            max_buffer_size: 512,
            process_mode: ProcessMode::Realtime,
        };
        let _ = plugin.activate(&layout, &config, &mut NoInit);
        assert_eq!(plugin.telemetry.tempo.get(), None);
        assert_eq!(plugin.synced_lfo_hz, None);
    }

    /// An activation context that asks nothing of a host.
    struct NoInit;

    impl ActivateContext<MxmPoly06> for NoInit {
        fn plugin_api(&self) -> PluginApi {
            PluginApi::Clap
        }
        fn execute(&self, _task: <MxmPoly06 as Plugin>::BackgroundTask) {}
        fn set_latency_samples(&self, _samples: u32) {}
        fn set_current_voice_capacity(&self, _capacity: u32) {}
    }
}

/// What a player reads — on hover in the editor, and in a host's plugin browser — speaks to the
/// player about the sound, never about the machine or the code (`mxm_plugin_test::hover_text`).
#[cfg(test)]
mod speaks_to_the_player {
    #[test]
    fn hover_text() {
        mxm_plugin_test::hover_text::speaks_to_the_player(env!("CARGO_MANIFEST_DIR"));
    }

    #[test]
    fn host_description() {
        mxm_plugin_test::hover_text::host_description_speaks_to_the_player(env!(
            "CARGO_MANIFEST_DIR"
        ));
    }
}
