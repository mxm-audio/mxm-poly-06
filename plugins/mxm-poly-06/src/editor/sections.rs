//! Every card as a tree, and the advanced zone.
//!
//! The brief owns what goes where; this file implements it. §10's section order is the contract, and
//! [`super::SECTIONS`] is written in that order so a reordering is a visible diff rather than a
//! drift.

use std::collections::HashMap;

use egui::Ui;
use mxm_poly_06_dsp::voice::VOICES;
use mxm_ui::control::{Size, Wave};
use mxm_ui::space::{SPACE_2, SPACE_3};
use mxm_ui::theme::Tokens;
use mxm_ui::tree::{
    self, Flow, Font, Height, Kind, Node, leaf, pad, pad_all, row_gap, stack, stack_gap,
};
use nice_plug::prelude::ParamSetter;

use super::binding::{Bound, segmented, toggle_picture};
use super::{Section, visuals};
use crate::params::MxmPoly06Params;
use crate::telemetry::Telemetry;
use mxm_poly_06_dsp::routing::{TARGET_NAMES, target};

/// §7.1's sizing, as used here.
///
/// Brief §2 puts Cutoff and Resonance at **Primary**. Everything else on the instrument is Standard,
/// and the disclosed zone is Compact — a size difference is the cheapest way to say *these are not
/// the ones you reach for*, and it carries alongside the surface change rather than relying on it.
const PRIMARY: Size = Size::Primary;
const STANDARD: Size = Size::Standard;

// --------------------------------------------------------------------------------------------
// The cards, as trees (plans/plan-layout-tree.md)
//
// Each card is described once — `card` — and that one description is both measured (its floor and
// its height) and drawn, leaf by leaf, through the bindings below (`paint`). Nothing is typed and
// nothing is drawn to learn a size. The geometry is the hand layout's: knob rows in the
// collection's `tree::knob_row` at 76-point columns, a switch on the grid of the knob beside it, the
// displays across the card at their stated heights, and every `add_space` of the old drawing code
// as a pad over the body's `SPACE_3` rhythm.
// --------------------------------------------------------------------------------------------

/// What a leaf of this editor's cards draws. Hashed by what it names — a parameter id, a routing
/// target, a caption's text — which is also what keeps its widget ids stable when a route appears
/// above it.
#[derive(Clone, Debug, Hash)]
pub enum Leaf {
    Knob(&'static str, Size),
    /// A fader in the collection's fader row.
    Fader(&'static str),
    /// A segmented switch, on the grid of the knob beside it when it has one.
    Switch(&'static str, Option<Size>),
    /// A waveform's on/off, drawn as the wave.
    Wave(&'static str),
    /// A control's tempo sync, the quarter note beside it.
    Picture(&'static str),
    /// The line over the Oscillator's two wave switches.
    Label(&'static str),
    /// The filter section's response.
    Response,
    /// The six voices.
    Voices,
    /// One routing target's stack.
    Routes(usize),
}

/// A stepped parameter's cells, **labelled by the parameter itself**: each its option's own
/// formatted value, so a cell reads what the host's automation list reads.
fn options_of(params: &MxmPoly06Params, id: &'static str) -> Vec<String> {
    let param = binding_for(id, params).param;
    let last = param
        .steps()
        .unwrap_or_else(|| unreachable!("{id} is not a switch"));
    (0..=last)
        .map(|option| param.format(option as f32 / last as f32))
        .collect()
}

/// What a control **paints**, where its card already says the rest (design system §7.1): *Rate* and
/// *Delay* on the LFO. `None` paints the parameter's own name, which a host, a tooltip and a
/// screen reader always read. *Amplifier and chorus* holds two modules, so *VCA mode* and *Chorus
/// mode* keep theirs.
fn panel_label(id: &str) -> Option<&'static str> {
    match id {
        "lforate" => Some("Rate"),
        "lfodelay" => Some("Delay"),
        // The envelope's faders, by the convention (the owner, 2026-09-25).
        "attack" => Some("A"),
        "decay" => Some("D"),
        "sustain" => Some("S"),
        "release" => Some("R"),
        _ => None,
    }
}

/// The picture a waveform's on/off switch is drawn as.
fn wave_of(id: &str) -> Wave {
    match id {
        "saw" => Wave::RampUp,
        "pulse" => Wave::Pulse,
        other => unreachable!("{other} is not a waveform switch"),
    }
}

/// Levels as the collection's fader row (`tree::fader_row`): the envelope's A, D, S and R, and the
/// oscillator's pulse width, sub and noise — the source's own sliders.
fn faders(ui: &Ui, params: &MxmPoly06Params, ids: &[&'static str]) -> Node<Leaf> {
    mxm_ui::tree::fader_row(
        ui,
        ids.iter()
            .map(|&id| {
                let bound = binding_for(id, params);
                mxm_ui::tree::fader(
                    Leaf::Fader(id),
                    bound.painted(),
                    mxm_ui::control::widest_value(|n| bound.param.format(n as f32)),
                )
            })
            .collect(),
    )
}

/// A row of knobs in **columns of their own width**, left to right — `mxm-mono-03`'s solution to
/// two knobs sitting 190 points apart in a card that was mostly gap. `ui.columns` inside a width
/// capped at `Σ max(diameter + KNOB_GUTTER, KNOB_COLUMN_MIN)`, so each column is that sum's share,
/// gaps included, and shrinks below it only as far as its widest knob allows.
fn knobs(ui: &Ui, params: &MxmPoly06Params, knobs: &[(&'static str, Size)]) -> Node<Leaf> {
    mxm_ui::tree::knob_row(
        ui,
        knobs
            .iter()
            .map(|(id, size)| {
                (*size, {
                    let bound = binding_for(id, params);
                    let param = bound.param;
                    // A syncable control's column holds its free readings and its divisions.
                    let widest = if *id == "lforate" {
                        super::binding::synced_widest(param, crate::params::LFO_SYNC.span)
                    } else {
                        mxm_ui::control::widest_value(|n| param.format(n as f32))
                    };
                    leaf(
                        Leaf::Knob(id, *size),
                        Kind::Knob {
                            name: bound.painted().to_owned(),
                            widest,
                            size: *size,
                            column: 0.0,
                        },
                    )
                })
            })
            .collect(),
    )
}

/// A stepped parameter as a segmented control, each cell as wide as the longest option and no
/// wider (design system §7.3). `beside` is the knob it shares a top-aligned row with, if any: the
/// shared control then sits on that knob's grid — label on the knob's name line, cells centred on
/// the circle — and the rule and its measurement live in `mxm-ui`; this only says which knob.
fn switch(params: &MxmPoly06Params, id: &'static str, beside: Option<Size>) -> Node<Leaf> {
    leaf(
        Leaf::Switch(id, beside),
        Kind::Segmented {
            label: binding_for(id, params).painted().to_owned(),
            options: options_of(params, id),
            beside,
        },
    )
}

/// A display across the card at its stated height, never narrower than its stated minimum.
fn display(key: Leaf, min_width: f32, height: f32) -> Node<Leaf> {
    leaf(
        key,
        Kind::Custom {
            min_width,
            height: Height::Fixed(height),
            fills: true,
        },
    )
}

/// One target's routes, `SPACE_3` beneath the controls they move, as the shared stack states its
/// size: its narrowest is every route revealed at its widest reading, its height the patch as it
/// stands.
///
/// **Routing belongs under the thing it affects**, never in a detached footer — design system §7.4
/// and `plugins/mxm-mono-00/AGENTS.md`. The rows and the `‹ modulate ›` menu come from
/// `mxm_modulation_params`, so every editor in the collection draws this the same way.
fn routes(ui: &Ui, params: &MxmPoly06Params, target: usize) -> Node<Leaf> {
    let size = mxm_modulation_params::ui::stack_size(
        ui,
        TARGET_NAMES[target],
        &params.routes.each()[target].routes(target),
    );
    pad(SPACE_3, display(Leaf::Routes(target), size.x, size.y))
}

/// Card `section`'s body, as a tree.
#[must_use]
pub fn card(ui: &Ui, section: Section, params: &MxmPoly06Params) -> Node<Leaf> {
    body(ui, section, params, 0.0)
}

/// A card's body with `spare` points more in the filter curve: the layout lab's levelling, which
/// the paging renderer never hands out.
fn body(ui: &Ui, section: Section, params: &MxmPoly06Params, spare: f32) -> Node<Leaf> {
    let gap = ui.spacing().item_spacing.x;
    match section {
        // The DCO: one waveform with harmonics added. The octave and the two waveform switches,
        // tops aligned so the labels share a line and the cells another. On or off, as the
        // hardware has them: both may be on, and together they beat against nothing, because they
        // are one waveform. Labelled as a group, so the two picture switches (the owner,
        // 2026-09-23) have the same shape as the segmented switch beside them — a label, then the
        // control — `SPACE_2` apart. The PWM slider's LFO mode is the pulse-width route below, and
        // the DCO's LFO slider the pitch route: routing belongs beneath the control it moves.
        Section::Oscillator => stack(vec![
            row_gap(
                gap,
                vec![
                    switch(params, "range", None),
                    pad_all(
                        0.0,
                        SPACE_3,
                        0.0,
                        stack_gap(
                            SPACE_2,
                            vec![
                                leaf(
                                    Leaf::Label(WAVEFORM),
                                    Kind::Text {
                                        text: WAVEFORM.to_owned(),
                                        font: Font::Body,
                                        flow: Flow::Line,
                                    },
                                ),
                                row_gap(
                                    gap + SPACE_2,
                                    vec![
                                        leaf(Leaf::Wave("saw"), Kind::PictureToggle),
                                        leaf(Leaf::Wave("pulse"), Kind::PictureToggle),
                                    ],
                                ),
                            ],
                        ),
                    ),
                ],
            ),
            // The source's DCO sliders (R2's call on the owner's *"poly-06 is an obvious choice"*,
            // 2026-09-25, the owner's to overrule): pulse width, sub and noise as one fader row.
            pad(SPACE_3, faders(ui, params, &["pulsewidth", "sub", "noise"])),
            routes(ui, params, target::PITCH),
            routes(ui, params, target::PULSE_WIDTH),
        ]),
        // The global HPF and the per-voice lowpass, drawn as one response. The HPF first, because
        // it is first in the chain a chord goes through — and because its bottom position is a
        // boost, which the curve below is there to show. The envelope's polarity is its route's
        // sign now.
        Section::Filter => stack(vec![
            switch(params, "hpf", None),
            pad(
                SPACE_3,
                display(
                    Leaf::Response,
                    visuals::RESPONSE_MIN_WIDTH,
                    visuals::HEIGHT + spare,
                ),
            ),
            pad(
                SPACE_3,
                knobs(ui, params, &[("cutoff", PRIMARY), ("resonance", PRIMARY)]),
            ),
            routes(ui, params, target::CUTOFF),
        ]),
        // The one envelope, shared by the filter and the amplifier.
        Section::Envelope => stack(vec![faders(
            ui,
            params,
            &["attack", "decay", "sustain", "release"],
        )]),
        // The patch's level, before the chorus; the master after it is in the app bar. **The
        // chorus is on this card** (R2's call, the owner's to overrule): hugged, it was one switch
        // alone, and the level here is what drives it. No caption of its own: the one fact worth
        // saying — that the rate is all a mode changes — is the control's own tooltip.
        Section::Amplifier => stack(vec![
            row_gap(
                gap,
                vec![
                    knobs(ui, params, &[("level", STANDARD)]),
                    pad_all(0.0, SPACE_3, 0.0, switch(params, "vcamode", Some(STANDARD))),
                ],
            ),
            routes(ui, params, target::AMPLITUDE),
            pad(SPACE_3, switch(params, "chorus", None)),
        ]),
        // One LFO, for every voice at once. The rate's tempo sync is the quarter note beside it
        // (`plans/plan-tempo-sync-controls.md`).
        Section::Lfo => stack(vec![row_gap(
            gap,
            vec![
                knobs(ui, params, &[("lforate", STANDARD)]),
                tree::switch_beside_knob(
                    STANDARD,
                    leaf(Leaf::Picture("lfosync"), Kind::SyncToggle),
                ),
                knobs(ui, params, &[("lfodelay", STANDARD)]),
            ],
        )]),
        // How six cards are handed out, and brief §8.2's display this instrument exists to have:
        // which card a note landed on.
        Section::Voice => stack(vec![
            row_gap(
                gap,
                vec![
                    switch(params, "keyassign", Some(STANDARD)),
                    pad_all(
                        0.0,
                        SPACE_3,
                        0.0,
                        knobs(ui, params, &[("portamento", STANDARD)]),
                    ),
                ],
            ),
            // The bend range and the wheel's vibrato, **on the card, not behind a disclosure** (the
            // owner, 2026-09-28: *no reason to put them behind a bender knob*): the playing
            // controls, beside the others a player reaches for, each showing its value.
            pad(SPACE_3, knobs(ui, params, &BEND.map(|id| (id, STANDARD)))),
            pad(
                SPACE_3,
                display(
                    Leaf::Voices,
                    visuals::voices_min_width(ui),
                    visuals::VOICES_HEIGHT,
                ),
            ),
        ]),
    }
}

/// The line over the Oscillator's two picture switches, which names them as one control.
const WAVEFORM: &str = "Waveform";

/// The bender's range and the wheel's reach, in the order the brief lists them (brief §5), drawn
/// on the Voice card since 2026-09-28 rather than behind a disclosure.
pub const BEND: [&str; 2] = ["bendrange", "lfomod"];

/// Everything a leaf draws with: the parameters, their host, the text-entry buffers, and the voice
/// display's telemetry, read once before the cards are drawn.
pub struct Live<'a, 'b> {
    pub params: &'a MxmPoly06Params,
    pub setter: &'a ParamSetter<'b>,
    pub text_entry: &'a mut HashMap<&'static str, Option<String>>,
    pub levels: [f32; VOICES],
    pub notes: [(u8, bool); VOICES],
    /// The host tempo in force: a synced LFO rate reads its division with one.
    pub tempo: Option<f64>,
}

impl<'a, 'b> Live<'a, 'b> {
    /// Reads the voice display's telemetry once, for the whole frame.
    pub fn new(
        params: &'a MxmPoly06Params,
        telemetry: &Telemetry,
        setter: &'a ParamSetter<'b>,
        text_entry: &'a mut HashMap<&'static str, Option<String>>,
    ) -> Self {
        Self {
            params,
            setter,
            text_entry,
            levels: telemetry.voice_levels(),
            notes: telemetry.voice_notes(),
            tempo: telemetry.tempo.get(),
        }
    }
}

/// Draws one leaf, in the `Ui` the tree bounded to `rect`, through the bindings below — so the
/// controls, their gestures and their names are exactly what they were.
pub fn paint(ui: &mut Ui, tokens: &Tokens, leaf: &Leaf, rect: egui::Rect, live: &mut Live<'_, '_>) {
    let params = live.params;
    let setter = live.setter;
    match *leaf {
        // Synced to a tempo, the LFO rate reads its division; the host still reads its hertz.
        Leaf::Knob(id, size) => {
            let bound = binding_for(id, params);
            let division = {
                use nice_plug::prelude::Param as _;
                let synced: Option<(bool, &nice_plug::prelude::FloatParam, mxm_tempo::Ladder)> =
                    match id {
                        "lforate" => Some((
                            params.lfo_sync.value(),
                            &params.lfo_rate,
                            crate::params::LFO_SYNC,
                        )),
                        _ => None,
                    };
                synced
                    .filter(|(on, _, _)| *on)
                    .and_then(|(_, param, ladder)| {
                        ladder.shown(
                            param.unmodulated_normalized_value(),
                            live.tempo,
                            f64::from(param.preview_plain(0.0)),
                            f64::from(param.preview_plain(1.0)),
                        )
                    })
            };
            match division {
                Some(division) => bound.knob_with_reading(
                    ui,
                    tokens,
                    setter,
                    size,
                    rect.width(),
                    live.text_entry,
                    division.label(),
                ),
                None => bound.knob(ui, tokens, setter, size, rect.width(), live.text_entry),
            }
        }
        Leaf::Picture(id) => {
            super::binding::sync_picture(ui, tokens, id, binding_for(id, params).param, setter);
        }
        Leaf::Fader(id) => {
            let bound = binding_for(id, params);
            bound.slider_vertical(
                ui,
                tokens,
                setter,
                live.text_entry,
                bound.painted(),
                rect.width(),
                mxm_ui::control::FADER_HEIGHT,
            );
        }
        Leaf::Switch(id, beside) => {
            let bound = binding_for(id, params);
            let labels = options_of(params, id);
            let options: Vec<&str> = labels.iter().map(String::as_str).collect();
            segmented(
                ui,
                tokens,
                bound.id,
                bound.param,
                &options,
                beside,
                bound.details,
                setter,
            );
        }
        Leaf::Wave(id) => {
            let bound = binding_for(id, params);
            toggle_picture(
                ui,
                tokens,
                bound.id,
                bound.param,
                wave_of(id),
                bound.description,
                setter,
            );
        }
        Leaf::Label(text) => {
            ui.label(text);
        }
        Leaf::Response => {
            let (cutoff_hz, env_hz) = reach(params);
            visuals::filter_response(
                ui,
                tokens,
                params.hpf.value().into(),
                cutoff_hz,
                params.resonance.value(),
                env_hz,
                rect.height(),
            );
        }
        Leaf::Voices => visuals::voices(ui, tokens, &live.levels, &live.notes),
        Leaf::Routes(target) => {
            let group = params.routes.each()[target];
            let entry = live.text_entry.entry("routes").or_default();
            mxm_modulation_params::ui::stack(
                ui,
                tokens,
                TARGET_NAMES[target],
                // Nothing to drop: no target's name repeats the card it is drawn on.
                TARGET_NAMES[target],
                &group.routes(target),
                entry,
                setter,
            );
        }
    }
}

/// Draws one section's body from its tree — the layout lab's entry point, which draws these real
/// cards on its bench. `spare` goes into the filter curve.
#[allow(clippy::too_many_arguments)]
pub fn draw(
    ui: &mut Ui,
    tokens: &Tokens,
    section: Section,
    params: &MxmPoly06Params,
    telemetry: &Telemetry,
    setter: &ParamSetter<'_>,
    text_entry: &mut HashMap<&'static str, Option<String>>,
    spare: f32,
) {
    let tree = body(ui, section, params, spare);
    let mut live = Live::new(params, telemetry, setter, text_entry);
    tree::show(ui, tokens, &tree, |ui, leaf, rect| {
        paint(ui, tokens, leaf, rect, &mut live);
    });
}

// --------------------------------------------------------------------------------------------
// Helpers
// --------------------------------------------------------------------------------------------

/// Where the cutoff sits, and how far the envelope can push it.
///
/// Mirrors `Voice::process`'s arithmetic. Duplicated rather than shared because the DSP takes plain
/// values and returns samples; being a **declared approximation** is what makes that acceptable,
/// and the brief §8 declares it.
fn reach(params: &MxmPoly06Params) -> (f32, f32) {
    use mxm_poly_06_dsp::routing::{FULL_SCALE, source};
    let base = params.cutoff.value();
    // The envelope's reach is its route's signed amount at that route's own scale — and none while
    // the route is absent, where the curve draws the cutoff alone.
    let route = &params.routes.cutoff;
    let amount = if route.env_on.value() {
        route.env.value()
    } else {
        0.0
    };
    let env = base * 2f32.powf(FULL_SCALE[target::CUTOFF][source::ENVELOPE] * amount);
    (base, env.clamp(10.0, 40_000.0))
}

/// One parameter's binding, with the sentence §7.1 requires in its tooltip.
///
/// **The descriptions live here because only the plugin has them.** CLAP carries no such field, so
/// a host cannot supply one — which is the concrete reason this editor belongs to the plugin.
/// How the keyboard steps a parameter: a semitone and an octave for the bend reach
/// and for the cutoff in hertz,
/// the owner's ruling of 2026-09-23. Everything not named keeps its own step. See
/// [`crate::editor::binding::StepLaw`].
fn step_law(id: &str) -> super::binding::StepLaw {
    use super::binding::StepLaw;
    match id {
        "bendrange" => StepLaw::Semitones,
        "cutoff" => StepLaw::Hertz,
        _ => StepLaw::Own,
    }
}

pub fn binding_for<'a>(id: &'static str, p: &'a MxmPoly06Params) -> Bound<'a> {
    let (param, description, bipolar): (&'a dyn super::binding::ErasedParam, &'static str, bool) =
        match id {
            // ---- LFO ----
            "lforate" => (
                &p.lfo_rate,
                "How fast the LFO runs; it moves every voice together.",
                false,
            ),
            "lfosync" => (&p.lfo_sync, super::binding::SYNC_DESCRIPTION, false),
            "lfodelay" => (
                &p.lfo_delay,
                "How long after the first key the LFO fades in. A fade, not a wait.",
                false,
            ),
            // ---- DCO ----
            "range" => (&p.range, "The octave: 16', 8' or 4'.", false),
            "pulsewidth" => (&p.pulse_width, "How narrow the pulse wave is.", false),
            "pulse" => (&p.pulse, "The pulse wave, on or off.", false),
            "saw" => (&p.saw, "The sawtooth, on or off.", false),
            "sub" => (&p.sub, "A square one octave down, for weight.", false),
            "noise" => (&p.noise, "White noise.", false),
            // ---- HPF ----
            "hpf" => (
                &p.hpf,
                "Shapes the low end of all the voices together.",
                false,
            ),
            // ---- VCF ----
            "cutoff" => (&p.cutoff, "The filter's cutoff: lower is darker.", false),
            "resonance" => (
                &p.resonance,
                "Emphasis at the cutoff; at the top the filter whistles on its own.",
                false,
            ),
            // ---- VCA ----
            "level" => (
                &p.level,
                "How loud the voices are before the chorus; turned up, they drive it harder.",
                false,
            ),
            "vcamode" => (&p.vca_mode, "How the volume follows the keys.", false),
            // ---- ENV ----
            "attack" => (
                &p.attack,
                "How long a note takes to reach full level.",
                false,
            ),
            "decay" => (
                &p.decay,
                "How long it takes to fall to the sustain level.",
                false,
            ),
            "sustain" => (&p.sustain, "The level a held note settles at.", false),
            "release" => (
                &p.release,
                "How long a note takes to fade after the key lifts.",
                false,
            ),
            // ---- CHORUS ----
            "chorus" => (&p.chorus, "The chorus.", false),
            // ---- Voice ----
            "portamento" => (
                &p.portamento,
                "How long each voice takes to slide to its next note, from wherever it was.",
                false,
            ),
            "keyassign" => (
                &p.key_assign,
                "How keys are given voices: the first free one, the next in rotation, or all six.",
                false,
            ),
            "volume" => (&p.volume, "Output level, after the chorus.", false),
            // ---- Disclosed ----
            "bendrange" => (
                &p.bend_range,
                "Pitch-bend range into the oscillators, in semitones.",
                false,
            ),
            "lfomod" => (
                &p.lfo_mod,
                "How much vibrato the mod wheel adds when pushed all the way.",
                false,
            ),

            other => unreachable!("no binding for {other}"),
        };
    Bound {
        id,
        param,
        description,
        panel: panel_label(id).map(std::borrow::Cow::Borrowed),
        bipolar,
        law: step_law(id),
        stepped: None,
        details: details_of(id),
    }
}

/// What each option of a stepped control does, one sentence per cell in the parameter's own order
/// (design system §7.3; the owner, 2026-09-27: the cells of a row do not share one sentence).
/// Empty for everything drawn as a knob, slider or toggle.
fn details_of(id: &str) -> &'static [&'static str] {
    match id {
        "range" => &[
            "One octave below the note played.",
            "The note as played.",
            "One octave above the note played.",
        ],
        "hpf" => &[
            "Lifts the bass.",
            "Leaves the low end as it is.",
            "Thins out the lowest bass.",
            "Cuts the low end further, for a lighter sound.",
        ],
        "vcamode" => &[
            "The envelope shapes the volume of each note.",
            "Full volume while a key is held, silent once it is released.",
        ],
        "chorus" => &[
            "No chorus: the voices as they are.",
            "A slow, gentle chorus.",
            "A faster, deeper swirl.",
            "Both at once: the thickest setting.",
        ],
        "keyassign" => &[
            "Each new key takes the first free voice.",
            "Each new key takes the next voice in turn, so releases can ring on.",
            "All six voices play every note: one huge, detuned sound.",
        ],
        _ => &[],
    }
}

/// Every parameter, bound, in the instrument's own order.
///
/// What `preset.rs` iterates: capture, Init, resolve and the dirty baseline all walk this list, so a
/// parameter missing from [`ALL_IDS`] would silently fall out of every preset — which is why
/// `every_parameter_is_drawn_exactly_once` checks the list against the `Params` derive.
pub fn all_parameters(params: &MxmPoly06Params) -> Vec<Bound<'_>> {
    ALL_IDS.iter().map(|id| binding_for(id, params)).collect()
}

/// Every id this editor draws. One list, so the coverage test and the lookup cannot disagree.
pub const ALL_IDS: &[&str] = &[
    "range",
    "saw",
    "pulse",
    "pulsewidth",
    "sub",
    "noise",
    "hpf",
    "cutoff",
    "resonance",
    "attack",
    "decay",
    "sustain",
    "release",
    "level",
    "volume",
    "vcamode",
    "lforate",
    "lfosync",
    "lfodelay",
    "keyassign",
    "portamento",
    "chorus",
    "bendrange",
    "lfomod",
];

#[cfg(test)]
mod tests {
    use super::*;

    /// Every parameter appears exactly once in the panel: on a card, in the Amplifier's disclosure,
    /// or in the app bar.
    ///
    /// **The check the brief's sign-off asks for**, mechanised: a parameter with no control is a
    /// parameter nobody can reach, and one drawn twice is two controls disagreeing about a value.
    #[test]
    fn every_parameter_is_drawn_exactly_once() {
        use nice_plug::prelude::Params;
        let params = MxmPoly06Params::default();
        let declared: Vec<String> = params
            .param_map()
            .into_iter()
            .map(|(id, _, _)| id)
            // A routing parameter is drawn by its target's stack, not by this list; the pairs are
            // counted by `every_route_belongs_to_exactly_one_target` instead.
            .filter(|id| !id.starts_with("mod_"))
            .collect();

        let mut drawn: Vec<&str> = super::super::SECTIONS
            .iter()
            .flat_map(|s| s.parameters().iter().copied())
            .collect();
        drawn.extend_from_slice(super::super::BAR_PARAMETERS);

        for id in &declared {
            let count = drawn.iter().filter(|d| *d == id).count();
            assert_eq!(count, 1, "{id} is drawn {count} times, expected once");
        }
        assert_eq!(
            drawn.len(),
            declared.len(),
            "drawn {drawn:?} against declared {declared:?}"
        );
        assert_eq!(
            ALL_IDS.len(),
            declared.len(),
            "ALL_IDS has fallen behind the parameters"
        );
    }

    /// **Every routing pair belongs to exactly one target**, whose card draws its stack: pitch and
    /// pulse width on the Oscillator, cutoff on the Filter, amplitude on the Amplifier.
    #[test]
    fn every_route_belongs_to_exactly_one_target() {
        use nice_plug::prelude::Params;
        let routing: Vec<String> = MxmPoly06Params::default()
            .param_map()
            .into_iter()
            .map(|(id, _, _)| id)
            .filter(|id| id.starts_with("mod_"))
            .collect();
        assert_eq!(
            routing.len(),
            4 * 11 * 2,
            "a presence and an amount per pair"
        );
        for id in &routing {
            let matched = ["mod_pitch_", "mod_width_", "mod_cutoff_", "mod_amp_"]
                .iter()
                .filter(|p| id.starts_with(**p))
                .count();
            assert_eq!(matched, 1, "{id} matches {matched} targets");
        }
    }

    /// Every binding resolves, which `binding_for`'s `unreachable!` would otherwise turn into a
    /// panic inside a paint call — and a panic there takes the host down with it.
    #[test]
    fn every_drawn_parameter_has_a_binding() {
        let params = MxmPoly06Params::default();
        for id in ALL_IDS {
            let bound = binding_for(id, &params);
            assert!(!bound.description.is_empty(), "{id} has no description");
            assert!(
                bound.description.ends_with('.'),
                "{id}'s description is not a sentence: {:?}",
                bound.description
            );
        }
    }
}
