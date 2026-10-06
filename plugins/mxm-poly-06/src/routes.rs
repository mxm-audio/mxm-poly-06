//! mxm-poly-06's routing parameters: one presence and one amount per *(target, source)* pair.
//!
//! `plans/plan-mxm-poly-06-modulation.md` §4, under `plans/plan-modulation-routing.md` §4.3. The
//! derive needs concrete fields and this instrument's source list is its own, so the struct is
//! declared here rather than generated — the shape `mxm-mono-01` and `mxm-mono-08` use. What is
//! shared is everything around these fields: [`mxm_modulation_params`] reads them, and
//! [`mxm_modulation`](mxm_poly_06_dsp::routing) evaluates them.
//!
//! # Permanent ids
//!
//! One `#[nested(id_prefix = …)]` per target, so a pair's ids are `<target>_<source>` and
//! `<target>_<source>on`. **Permanent from here on**, like every id in this collection.

use mxm_modulation_params::Route;
use mxm_modulation_params::reading::{self, Fader, Reach};
use mxm_poly_06_dsp::routing::{
    FULL_SCALE, INIT_PRESENT, KEY_UNIT_SEMITONES, Routing, SOURCE_NAMES, SOURCES, TARGET_NAMES,
    TARGETS, offer, source, target,
};
use nice_plug::prelude::*;

/// Every routing pair's two permanent ids, `(amount, presence)`, in `[target][source]` order.
///
/// **Written out rather than derived at runtime**, because a preset's parameter list is
/// `&'static str` and because these are permanent ids: they belong in the source where they can be
/// read, grepped and diffed. The `#[nested(id_prefix = …)]` groups in [`Routes`] still generate
/// them; `tests::the_id_table_is_what_the_derive_actually_produces` holds the two together.
pub const ROUTE_IDS: [[(&str, &str); SOURCES]; TARGETS] = [
    [
        ("mod_pitch_key", "mod_pitch_keyon"),
        ("mod_pitch_env", "mod_pitch_envon"),
        ("mod_pitch_lfo", "mod_pitch_lfoon"),
        ("mod_pitch_vel", "mod_pitch_velon"),
        ("mod_pitch_wheel", "mod_pitch_wheelon"),
        ("mod_pitch_press", "mod_pitch_presson"),
        ("mod_pitch_bend", "mod_pitch_bendon"),
        ("mod_pitch_saw", "mod_pitch_sawon"),
        ("mod_pitch_pulse", "mod_pitch_pulseon"),
        ("mod_pitch_sub", "mod_pitch_subon"),
        ("mod_pitch_noise", "mod_pitch_noiseon"),
    ],
    [
        ("mod_width_key", "mod_width_keyon"),
        ("mod_width_env", "mod_width_envon"),
        ("mod_width_lfo", "mod_width_lfoon"),
        ("mod_width_vel", "mod_width_velon"),
        ("mod_width_wheel", "mod_width_wheelon"),
        ("mod_width_press", "mod_width_presson"),
        ("mod_width_bend", "mod_width_bendon"),
        ("mod_width_saw", "mod_width_sawon"),
        ("mod_width_pulse", "mod_width_pulseon"),
        ("mod_width_sub", "mod_width_subon"),
        ("mod_width_noise", "mod_width_noiseon"),
    ],
    [
        ("mod_cutoff_key", "mod_cutoff_keyon"),
        ("mod_cutoff_env", "mod_cutoff_envon"),
        ("mod_cutoff_lfo", "mod_cutoff_lfoon"),
        ("mod_cutoff_vel", "mod_cutoff_velon"),
        ("mod_cutoff_wheel", "mod_cutoff_wheelon"),
        ("mod_cutoff_press", "mod_cutoff_presson"),
        ("mod_cutoff_bend", "mod_cutoff_bendon"),
        ("mod_cutoff_saw", "mod_cutoff_sawon"),
        ("mod_cutoff_pulse", "mod_cutoff_pulseon"),
        ("mod_cutoff_sub", "mod_cutoff_subon"),
        ("mod_cutoff_noise", "mod_cutoff_noiseon"),
    ],
    [
        ("mod_amp_key", "mod_amp_keyon"),
        ("mod_amp_env", "mod_amp_envon"),
        ("mod_amp_lfo", "mod_amp_lfoon"),
        ("mod_amp_vel", "mod_amp_velon"),
        ("mod_amp_wheel", "mod_amp_wheelon"),
        ("mod_amp_press", "mod_amp_presson"),
        ("mod_amp_bend", "mod_amp_bendon"),
        ("mod_amp_saw", "mod_amp_sawon"),
        ("mod_amp_pulse", "mod_amp_pulseon"),
        ("mod_amp_sub", "mod_amp_subon"),
        ("mod_amp_noise", "mod_amp_noiseon"),
    ],
];

/// One target's routes: a presence and a signed amount for every source the instrument declares.
///
/// **Presence is the enable and the amount is the depth**, and nothing else: no selector, because a
/// pair *is* its source; no polarity switch, because the amount is signed. Declared in source order.
#[derive(Params)]
pub struct TargetRoutes {
    #[id = "keyon"]
    pub key_on: BoolParam,
    #[id = "key"]
    pub key: FloatParam,
    #[id = "envon"]
    pub env_on: BoolParam,
    #[id = "env"]
    pub env: FloatParam,
    #[id = "lfoon"]
    pub lfo_on: BoolParam,
    #[id = "lfo"]
    pub lfo: FloatParam,
    #[id = "velon"]
    pub vel_on: BoolParam,
    #[id = "vel"]
    pub vel: FloatParam,
    #[id = "wheelon"]
    pub wheel_on: BoolParam,
    #[id = "wheel"]
    pub wheel: FloatParam,
    #[id = "presson"]
    pub press_on: BoolParam,
    #[id = "press"]
    pub press: FloatParam,
    #[id = "bendon"]
    pub bend_on: BoolParam,
    #[id = "bend"]
    pub bend: FloatParam,
    #[id = "sawon"]
    pub saw_on: BoolParam,
    #[id = "saw"]
    pub saw: FloatParam,
    #[id = "pulseon"]
    pub pulse_on: BoolParam,
    #[id = "pulse"]
    pub pulse: FloatParam,
    #[id = "subon"]
    pub sub_on: BoolParam,
    #[id = "sub"]
    pub sub: FloatParam,
    #[id = "noiseon"]
    pub noise_on: BoolParam,
    #[id = "noise"]
    pub noise: FloatParam,
}

/// A route amount: signed, centred on zero, and **an amount, so it starts there** — the
/// collection's one route parameter (`mxm_modulation_params::reading`), on the travel the pair's
/// offer allows, which on this instrument is always both halves.
///
/// Smoothed at this instrument's own 10 ms, the smoothing every depth slider it replaces had —
/// `plugins/AGENTS.md`'s *smooth signals, not coefficients*. It reads as [`reach`] says.
fn amount(target: usize, source: usize) -> FloatParam {
    reading::amount_param(
        format!("{} from {}", TARGET_NAMES[target], SOURCE_NAMES[source]),
        reach(target, source),
        Fader::for_offer(offer(target, source), false),
        10.0,
    )
}

/// What a route reads: **what its pair delivers at this amount, in the target's own unit** —
/// semitones, a percentage of width, octaves, a percentage of level — and per octave of keyboard for
/// a Key route. So the machine's own routes read, at full, the numbers their retired sliders meant:
/// +7.00 st, +45 %, +7.00 and +3.00 oct, +1.00 oct/oct and +4.00 oct; and an added route reads the
/// standard's reach, +12.00 st or +4.00 oct. A percentage of the amount would say nothing false and
/// nothing useful (`docs/code-review-notes.md` §7, *what a route's amount reads*).
fn reach(target: usize, source: usize) -> Reach {
    let unit = match target {
        target::PITCH => reading::SEMITONES,
        target::CUTOFF => reading::OCTAVES,
        _ => reading::PERCENT,
    };
    let full = FULL_SCALE[target][source];
    if source == source::KEY {
        Reach::per_octave(full * 12.0 / KEY_UNIT_SEMITONES, unit)
    } else {
        Reach::new(full, unit)
    }
}

/// Whether a route exists. **Configuration, not an amount**, so its default is the machine's own
/// wiring: the six paths the JUNO itself has are present in the init patch, everything else absent.
fn present(target: &str, source: &str, wired: bool) -> BoolParam {
    BoolParam::new(format!("{target} from {source} on"), wired)
}

impl TargetRoutes {
    /// Every pair for one target, at the init patch: the machine's own wiring present, the rest
    /// absent, and **every one of them at zero depth**.
    pub fn new(target: usize) -> Self {
        let n = SOURCE_NAMES;
        let wired = |s: usize| INIT_PRESENT.contains(&(target, s));
        let name = TARGET_NAMES[target];
        Self {
            key_on: present(name, n[0], wired(0)),
            key: amount(target, 0),
            env_on: present(name, n[1], wired(1)),
            env: amount(target, 1),
            lfo_on: present(name, n[2], wired(2)),
            lfo: amount(target, 2),
            vel_on: present(name, n[3], wired(3)),
            vel: amount(target, 3),
            wheel_on: present(name, n[4], wired(4)),
            wheel: amount(target, 4),
            press_on: present(name, n[5], wired(5)),
            press: amount(target, 5),
            bend_on: present(name, n[6], wired(6)),
            bend: amount(target, 6),
            saw_on: present(name, n[7], wired(7)),
            saw: amount(target, 7),
            pulse_on: present(name, n[8], wired(8)),
            pulse: amount(target, 8),
            sub_on: present(name, n[9], wired(9)),
            sub: amount(target, 9),
            noise_on: present(name, n[10], wired(10)),
            noise: amount(target, 10),
        }
    }

    /// This target's routes in **declared source order**. `target` is its index, because each row's
    /// keyboard scope is its parameter's permanent id and those live in [`ROUTE_IDS`], keyed by
    /// target.
    pub fn routes(&self, target: usize) -> [Route<'_>; SOURCES] {
        let ids = ROUTE_IDS[target];
        let pairs: [(&BoolParam, &FloatParam); SOURCES] = [
            (&self.key_on, &self.key),
            (&self.env_on, &self.env),
            (&self.lfo_on, &self.lfo),
            (&self.vel_on, &self.vel),
            (&self.wheel_on, &self.wheel),
            (&self.press_on, &self.press),
            (&self.bend_on, &self.bend),
            (&self.saw_on, &self.saw),
            (&self.pulse_on, &self.pulse),
            (&self.sub_on, &self.sub),
            (&self.noise_on, &self.noise),
        ];
        std::array::from_fn(|s| Route {
            source: SOURCE_NAMES[s],
            present: pairs[s].0,
            amount: pairs[s].1,
            present_id: ids[s].1,
            amount_id: ids[s].0,
        })
    }

    /// Whether each of this target's routes exists. Read **once per interval**, never per sample.
    pub fn presences(&self, target: usize) -> [bool; SOURCES] {
        mxm_modulation_params::presences(&self.routes(target))
    }

    /// One source's amount parameter, by index, in declared source order — a `match` rather than
    /// an array of references, so a source nothing reads costs a branch and no pointer stores.
    #[inline]
    fn amount_param(&self, source: usize) -> &FloatParam {
        match source {
            0 => &self.key,
            1 => &self.env,
            2 => &self.lfo,
            3 => &self.vel,
            4 => &self.wheel,
            5 => &self.press,
            6 => &self.bend,
            7 => &self.saw,
            8 => &self.pulse,
            9 => &self.sub,
            _ => &self.noise,
        }
    }

    /// Snaps each newly present route's smoother to its stored value.
    ///
    /// **An absent route's smoother is not advanced, so it must not be resumed either.** While the
    /// pair was absent the parameter stayed editable — a host automating it, a preset load — which
    /// moves the *target* and leaves the smoother wherever the last live sample left it. Resuming
    /// would ramp the route in from a stale number over a span set by how long it was absent, which
    /// is the host's buffer sizes deciding a sound (`docs/code-review-notes.md` §7).
    pub fn arm(&self, newly_present: &[bool; SOURCES]) {
        for (source, &now) in newly_present.iter().enumerate() {
            if now {
                let param = self.amount_param(source);
                param.smoothed.reset(param.value());
            }
        }
    }
}

/// All four targets' routes.
#[derive(Params)]
pub struct Routes {
    #[nested(id_prefix = "mod_pitch", group = "Modulation - Pitch")]
    pub pitch: TargetRoutes,
    #[nested(id_prefix = "mod_width", group = "Modulation - Pulse width")]
    pub width: TargetRoutes,
    #[nested(id_prefix = "mod_cutoff", group = "Modulation - Cutoff")]
    pub cutoff: TargetRoutes,
    #[nested(id_prefix = "mod_amp", group = "Modulation - Amplitude")]
    pub amp: TargetRoutes,
}

impl Default for Routes {
    fn default() -> Self {
        Self::new()
    }
}

impl Routes {
    /// The init patch: **the machine's own six routes present, every depth at zero.** They were
    /// five sliders, a PWM mode switch, an envelope polarity switch and a bend sensitivity; a
    /// fresh instance shows the JUNO's own signal flow, one row per path, each at zero.
    pub fn new() -> Self {
        Self {
            pitch: TargetRoutes::new(target::PITCH),
            width: TargetRoutes::new(target::PULSE_WIDTH),
            cutoff: TargetRoutes::new(target::CUTOFF),
            amp: TargetRoutes::new(target::AMPLITUDE),
        }
    }

    /// The four targets, in declared target order.
    pub fn each(&self) -> [&TargetRoutes; TARGETS] {
        [&self.pitch, &self.width, &self.cutoff, &self.amp]
    }

    /// Which routes are live, for the whole instrument. Once per interval.
    pub fn topology(&self) -> Routing {
        let mut routing = Routing::new();
        for (index, (slot, group)) in routing.present.iter_mut().zip(self.each()).enumerate() {
            *slot = group.presences(index);
        }
        routing.compact();
        routing
    }

    /// The topology for this interval, with every **newly present** route's smoother snapped to its
    /// stored value. `previous` is the topology the last interval ran; see [`TargetRoutes::arm`].
    pub fn topology_from(&self, previous: &Routing) -> Routing {
        let routing = self.topology();
        for (index, group) in self.each().into_iter().enumerate() {
            let mut newly = [false; SOURCES];
            for (slot, (&now, &before)) in newly.iter_mut().zip(
                routing.present[index]
                    .iter()
                    .zip(previous.present[index].iter()),
            ) {
                *slot = now && !before;
            }
            group.arm(&newly);
        }
        routing
    }

    /// Fills this sample's amounts into an already-topologised [`Routing`], as **fractions of each
    /// route's full scale** — the scale is applied by the sum.
    ///
    /// **Only a live route's smoother is advanced**: an absent pair costs a branch, and its stored
    /// depth is left where the player put it.
    ///
    /// `wheel_push` is the mod wheel times `lfomod`, and it is here rather than in a route because of
    /// what it does: **it deepens the vibrato the DCO's LFO route already carries** — the forward
    /// push's own path, which the plan keeps as named legacy (`plan-modulation-routing.md` §5.2).
    /// It used to be added to `dcolfo` and clamped; it is added to the pair that replaced `dcolfo`,
    /// clamped to the amount's own range, which is the same number for every legacy setting.
    /// **It adds, whichever way the route points**: the lever's vibrato is its own depth
    /// summed with the route's, as the machine's slider and lever depths summed, so a negative route is
    /// partly cancelled rather than deepened. Taking the route's sign instead would step the depth by
    /// twice the push as a swept amount crossed zero.
    pub fn advance(&self, routing: &mut Routing, wheel_push: f32) {
        let targets = self.each();
        for i in 0..routing.live().len() {
            let (t, s) = routing.live()[i];
            routing.amounts[t as usize][s as usize] =
                targets[t as usize].amount_param(s as usize).smoothed.next();
        }
        let vibrato = &mut routing.amounts[target::PITCH][source::LFO];
        *vibrato = (*vibrato + wheel_push).clamp(-1.0, 1.0);
    }

    /// Every routing parameter, named by its permanent id, for the preset layer: **presets carry
    /// routing**, or a sound would load with somebody else's routes still in it.
    pub fn parameters(&self) -> Vec<(&'static str, &dyn mxm_preset::ErasedParam)> {
        let mut out = Vec::with_capacity(TARGETS * SOURCES * 2);
        for (index, (group, ids)) in self.each().into_iter().zip(ROUTE_IDS).enumerate() {
            for (route, (amount, presence)) in group.routes(index).into_iter().zip(ids) {
                out.push((amount, route.amount));
                out.push((presence, route.present));
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nice_plug::params::Params;

    /// [`ROUTE_IDS`] names exactly what the derive produces, and nothing else.
    #[test]
    fn the_id_table_is_what_the_derive_actually_produces() {
        let params = crate::params::MxmPoly06Params::default();
        let real: std::collections::BTreeSet<String> = params
            .param_map()
            .into_iter()
            .map(|(id, _, _)| id)
            .filter(|id| id.starts_with("mod_"))
            .collect();
        let named: std::collections::BTreeSet<String> = ROUTE_IDS
            .iter()
            .flatten()
            .flat_map(|(amount, presence)| [(*amount).to_owned(), (*presence).to_owned()])
            .collect();
        assert_eq!(named, real, "ROUTE_IDS has drifted from the derived ids");
        assert_eq!(named.len(), TARGETS * SOURCES * 2);
    }

    /// **Remove, edit while absent, re-add: the route arrives at the depth the player set.**
    ///
    /// Falsified before trusted: with `arm`'s body removed, the first sample after re-adding reads a
    /// point on the ramp down from `0.9` instead of the stored `-0.4`.
    #[test]
    fn a_re_added_route_arrives_at_its_stored_depth_rather_than_ramping_from_a_stale_one() {
        use nice_plug::params::InternalParamMut;

        let routes = Routes::new();
        assert!(
            !routes.cutoff.saw_on.value(),
            "this pair starts absent, which is what the test needs"
        );

        const RATE: f32 = 48_000.0;
        unsafe {
            routes.cutoff.saw_on._internal_set_plain_value(true);
            routes.cutoff.saw._internal_set_plain_value(0.9);
            routes.cutoff.saw._internal_update_smoother(RATE, true);
        }
        let routing = routes.topology_from(&Routing::new());
        let mut amounts = routing;
        routes.advance(&mut amounts, 0.0);
        assert_eq!(amounts.amounts[target::CUTOFF][source::SAW], 0.9);

        unsafe {
            routes.cutoff.saw_on._internal_set_plain_value(false);
        }
        let absent = routes.topology_from(&routing);
        unsafe {
            routes.cutoff.saw._internal_set_plain_value(-0.4);
            routes.cutoff.saw._internal_update_smoother(RATE, false);
        }
        assert!(
            routes.cutoff.saw.smoothed.is_smoothing(),
            "the edit must leave the smoother mid-ramp, or this proves nothing"
        );

        unsafe {
            routes.cutoff.saw_on._internal_set_plain_value(true);
        }
        let mut back = routes.topology_from(&absent);
        routes.advance(&mut back, 0.0);
        assert_eq!(
            back.amounts[target::CUTOFF][source::SAW],
            -0.4,
            "a re-added route must arrive at its stored depth"
        );
    }

    /// **The wheel deepens the vibrato route, and only that one**, clamped to the amount's range —
    /// the same number the old `dcolfo + wheel × lfomod` reached for every legacy setting.
    #[test]
    fn the_wheel_push_deepens_the_vibrato_route_and_nothing_else() {
        use nice_plug::params::InternalParamMut;

        let routes = Routes::new();
        unsafe {
            routes.pitch.lfo._internal_set_plain_value(0.3);
            routes.pitch.lfo._internal_update_smoother(48_000.0, true);
            routes.cutoff.lfo._internal_set_plain_value(0.3);
            routes.cutoff.lfo._internal_update_smoother(48_000.0, true);
        }
        let mut routing = routes.topology_from(&Routing::new());
        routes.advance(&mut routing, 0.5);
        assert_eq!(routing.amounts[target::PITCH][source::LFO], 0.8);
        assert_eq!(routing.amounts[target::CUTOFF][source::LFO], 0.3);
        routes.advance(&mut routing, 1.0);
        assert_eq!(
            routing.amounts[target::PITCH][source::LFO],
            1.0,
            "clamped at full, as the slider was"
        );
    }

    /// **The wheel's push adds, whichever way the route points**, so the depth is continuous as a swept
    /// amount crosses zero. A push that took the route's sign — tried on 2026-09-15, after a review read
    /// *deepens* as *further from zero* — stepped the depth by twice the push at the crossing, and the
    /// next review caught it.
    #[test]
    fn the_wheel_push_adds_the_same_whichever_way_the_route_points() {
        use nice_plug::params::InternalParamMut;

        let routes = Routes::new();
        let depth_at = |amount: f32| {
            unsafe {
                routes.pitch.lfo._internal_set_plain_value(amount);
                routes.pitch.lfo._internal_update_smoother(48_000.0, true);
            }
            let stored = routes.pitch.lfo.value();
            let mut routing = routes.topology_from(&Routing::new());
            routes.advance(&mut routing, 0.5);
            (stored, routing.amounts[target::PITCH][source::LFO])
        };
        for amount in [-0.3, -0.001, 0.0, 0.001, 0.3] {
            let (stored, depth) = depth_at(amount);
            assert_eq!(
                depth,
                stored + 0.5,
                "the push at a stored depth of {stored}"
            );
        }
    }

    /// **A route reads what its pair delivers**, in the target's unit — the machine's own routes at
    /// full read the numbers their retired sliders meant — and a reading typed back in lands on the
    /// amount it came from. The one defect a player meets on the first knob they turn, and no audio
    /// assertion can see it.
    #[test]
    fn a_route_reads_what_its_pair_delivers_and_reads_back() {
        use mxm_preset::ErasedParam;
        let r = Routes::new();
        let cases: [(&FloatParam, f32, &str); 15] = [
            // The machine's own, at their sliders' reach.
            (&r.pitch.lfo, 1.0, "+7.00 st"),
            (&r.width.lfo, 1.0, "+45 %"),
            (&r.cutoff.env, 1.0, "+7.00 oct"),
            (&r.cutoff.env, 0.0, "-7.00 oct"),
            (&r.cutoff.env, 0.5, "+0.00 oct"),
            (&r.cutoff.lfo, 1.0, "+3.00 oct"),
            (&r.cutoff.key, 1.0, "+1.00 oct/oct"),
            (&r.cutoff.bend, 1.0, "+4.00 oct"),
            (&r.amp.lfo, 1.0, "+100 %"),
            // Added ones, at the collection's standard reach.
            (&r.pitch.key, 1.0, "+12.00 st/oct"),
            (&r.pitch.wheel, 1.0, "+12.00 st"),
            (&r.pitch.env, 0.0, "-12.00 st"),
            (&r.cutoff.vel, 1.0, "+4.00 oct"),
            (&r.amp.vel, 1.0, "+100 %"),
            (&r.amp.key, 1.0, "+20 %/oct"),
        ];
        for (param, normalised, expected) in cases {
            let text = ErasedParam::format(param, normalised);
            assert_eq!(
                text,
                expected,
                "{} at {normalised}",
                ErasedParam::name(param)
            );
            let back = param
                .string_to_normalized_value(&text)
                .expect("the reading parses");
            assert!(
                (back - normalised).abs() < 1e-3,
                "{}: {text} read back as {back}",
                ErasedParam::name(param)
            );
        }
    }

    /// **Every reading survives the host's round trip, a rounded zero included**: printed, parsed and
    /// printed again, it is the same text — at amounts either side of zero, not only the round numbers.
    /// A plain signed format printed `-0 %` there, which parses to zero and prints `+0 %`, and
    /// `clap-validator`'s `param-conversions` fails on that whenever its random values land in the
    /// sliver (`mxm_modulation_params::signed`).
    #[test]
    fn every_reading_survives_the_hosts_round_trip_a_rounded_zero_included() {
        let routes = Routes::new();
        let check = |param: &FloatParam| {
            for normalised in [
                0.0f32, 0.25, 0.4999, 0.49999, 0.5, 0.50001, 0.5001, 0.75, 1.0,
            ] {
                let text = param.normalized_value_to_string(normalised, true);
                let back = param
                    .string_to_normalized_value(&text)
                    .unwrap_or_else(|| panic!("{}: {text} does not parse", param.name()));
                assert_eq!(
                    text,
                    param.normalized_value_to_string(back, true),
                    "{} at {normalised}",
                    param.name()
                );
            }
        };
        for group in routes.each() {
            for source in 0..SOURCES {
                check(group.amount_param(source));
            }
        }
    }

    use mxm_plugin_test::routing_checks;

    /// **Every route parameter says what the DSP does** — the modulation standard's plugin half:
    /// each pair's travel is its offer's, its reading carries its target's unit and states what
    /// `mxm_poly_06_dsp::conformance` measures the voice's own graph delivering, and every reading
    /// survives the host's round trip.
    ///
    /// Falsified before trusted: with the pitch reading's reach left at the LFO slider's seven
    /// semitones, it names every added pitch pair from a performance source.
    #[test]
    fn every_route_parameter_says_what_the_dsp_does() {
        let routes = Routes::new();
        let groups = routes.each();
        if let Err(failures) =
            routing_checks::amounts(&mxm_poly_06_dsp::conformance::Declared, |target, source| {
                Some(groups[target].amount_param(source))
            })
        {
            panic!("{} failure(s):\n{}", failures.len(), failures.join("\n"));
        }
    }

    /// **A control-map role may take a route's amount only where Init wires that route**, or its
    /// knob is dead on a fresh instance. A role bound to a presence is never dead.
    ///
    /// Falsified before trusted: with `osc1.pwm_depth` pointed at `mod_width_env`, a route the init
    /// patch leaves absent, this fails naming that id.
    #[test]
    fn a_control_map_role_never_points_at_a_dead_route() {
        let text = include_str!("../control-map.json");
        let params = crate::params::MxmPoly06Params::default();

        let mut named = 0;
        for (t, group) in params.routes.each().into_iter().enumerate() {
            for (route, (amount, presence)) in group.routes(t).into_iter().zip(ROUTE_IDS[t]) {
                if text.contains(&format!("\"{presence}\"")) {
                    named += 1;
                }
                if text.contains(&format!("\"{amount}\"")) {
                    named += 1;
                    assert!(
                        route.is_present(),
                        "the control map binds a role to {amount}, which Init does not wire: a \
                         knob on it would do nothing"
                    );
                }
            }
        }
        // Five ids, six roles: `filter.lfo_amount` and `lfo1.to_filter` both name the filter's LFO.
        assert_eq!(
            named, 5,
            "the map names {named} routing ids; it named five when this was written"
        );
    }
}
