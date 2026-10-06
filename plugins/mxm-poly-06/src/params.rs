//! Parameter definitions.
//!
//! Every `#[id]` here is **permanent**. Changing one breaks every saved project that used the
//! plugin, so ids are part of the public interface.
//!
//! # The panel is the hardware's controls, and nothing else
//!
//! LFO (rate, delay), DCO (range, pulse width, pulse, saw, sub, noise), HPF (position), VCF (cutoff,
//! resonance), VCA (level, ENV/GATE), ENV (A D S R), CHORUS (mode) — plus portamento, key assign and
//! the master volume from the left of the panel. The bender's range and the wheel's reach are
//! **disclosed** below the divider. Nothing the plug-out added is here;
//! `research:instruments/juno-106.md` §9 lists what that rules out and why.
//!
//! # The machine's modulation is routing
//!
//! The DCO's LFO slider, the PWM slider in LFO mode, the VCF's ENV, LFO and KYBD sliders with the
//! envelope's polarity, and the bender's reach into the VCF are **routes** ([`crate::routes`]),
//! present in the init patch at zero depth. `dcolfo`, `pwmdepth`, `pwmmode`, `envamount`,
//! `envpolarity`, `vcflfo`, `keytrack` and `bendfilter` are **retired permanent ids** and may not
//! come back (`plans/plan-mxm-poly-06-modulation.md` §4.1).
//!
//! # Pulse width is one parameter, and its modulation a route
//!
//! The hardware has one slider that is a width in MAN and a depth in LFO — *"the one genuinely
//! confusing control on the machine"*. Here the width is `pulsewidth` and the LFO's sweep is the
//! `(Pulse width ← LFO)` route summed onto it: LFO mode is that route at the slider's depth over a
//! width of one half, MAN mode the route at zero. Each has a stable name that tells the truth in
//! host automation.
//!
//! # Two collection contracts, and where this instrument sits against them
//!
//! **Every amount starts at zero.** Every route amount, resonance, portamento, and the sub and noise
//! levels. **The one classification worth stating**: `lfomod` — how
//! far the mod wheel reaches — is a **configuration**, not an amount. The wheel itself is the amount
//! and rests at zero; this is the range it opens, and at zero the wheel would do nothing, which reads
//! as a broken wheel rather than a neutral patch. Recorded in the plugin's AGENTS.md.
//!
//! **Smooth signals, not coefficients.** Everything added to or multiplied into the audio is
//! smoothed. Envelope times, portamento time, the LFO's rate and delay are not: they set
//! state-machine behaviour, and smoothing them makes timing impossible to reason about.

use mxm_poly_06_dsp::chorus::Mode as ChorusModeDsp;
use mxm_poly_06_dsp::dco::Range as RangeDsp;
use mxm_poly_06_dsp::hpf::Position as HpfDsp;
use mxm_poly_06_dsp::poly::Assign as AssignDsp;
use nice_plug::prelude::*;
use std::sync::{Arc, RwLock};

/// The DCO's range switch, in organ footage.
#[derive(Enum, Debug, Clone, Copy, PartialEq, Eq)]
pub enum Range {
    #[id = "16"]
    #[name = "16'"]
    Sixteen,
    #[id = "8"]
    #[name = "8'"]
    Eight,
    #[id = "4"]
    #[name = "4'"]
    Four,
}

impl From<Range> for RangeDsp {
    fn from(r: Range) -> Self {
        match r {
            Range::Sixteen => RangeDsp::Sixteen,
            Range::Eight => RangeDsp::Eight,
            Range::Four => RangeDsp::Four,
        }
    }
}

/// The HPF's four positions. Position 0 is a bass boost, not "off".
#[derive(Enum, Debug, Clone, Copy, PartialEq, Eq)]
pub enum HpfPosition {
    #[id = "boost"]
    #[name = "Boost"]
    Boost,
    #[id = "flat"]
    #[name = "Flat"]
    Flat,
    #[id = "cut1"]
    #[name = "Cut 1"]
    Cut1,
    #[id = "cut2"]
    #[name = "Cut 2"]
    Cut2,
}

impl From<HpfPosition> for HpfDsp {
    fn from(p: HpfPosition) -> Self {
        match p {
            HpfPosition::Boost => HpfDsp::Boost,
            HpfPosition::Flat => HpfDsp::Flat,
            HpfPosition::Cut1 => HpfDsp::Cut1,
            HpfPosition::Cut2 => HpfDsp::Cut2,
        }
    }
}

/// The VCA's ENV / GATE switch.
#[derive(Enum, Debug, Clone, Copy, PartialEq, Eq)]
pub enum VcaMode {
    #[id = "envelope"]
    #[name = "Envelope"]
    Envelope,
    #[id = "gate"]
    #[name = "Gate"]
    Gate,
}

/// The chorus's two buttons and both together.
#[derive(Enum, Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChorusMode {
    #[id = "off"]
    #[name = "Off"]
    Off,
    #[id = "i"]
    #[name = "I"]
    I,
    #[id = "ii"]
    #[name = "II"]
    II,
    #[id = "both"]
    #[name = "I + II"]
    Both,
}

impl From<ChorusMode> for ChorusModeDsp {
    fn from(m: ChorusMode) -> Self {
        match m {
            ChorusMode::Off => ChorusModeDsp::Off,
            ChorusMode::I => ChorusModeDsp::I,
            ChorusMode::II => ChorusModeDsp::II,
            ChorusMode::Both => ChorusModeDsp::Both,
        }
    }
}

/// KEY ASSIGN.
#[derive(Enum, Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyAssign {
    #[id = "poly1"]
    #[name = "Poly 1"]
    Poly1,
    #[id = "poly2"]
    #[name = "Poly 2"]
    Poly2,
    #[id = "unison"]
    #[name = "Unison"]
    Unison,
}

impl From<KeyAssign> for AssignDsp {
    fn from(a: KeyAssign) -> Self {
        match a {
            KeyAssign::Poly1 => AssignDsp::Poly1,
            KeyAssign::Poly2 => AssignDsp::Poly2,
            KeyAssign::Unison => AssignDsp::Unison,
        }
    }
}

/// Formats a parameter value for display.
type ValueToString = Arc<dyn Fn(f32) -> String + Send + Sync>;
/// Parses a typed-in value, returning `None` if it cannot be understood.
type StringToValue = Arc<dyn Fn(&str) -> Option<f32> + Send + Sync>;

/// Format seconds as whole milliseconds below a second, hundredths of a second from it — **the unit
/// chosen from the rounded milliseconds, not the raw value.** Chosen from the raw value, 0.9995 s
/// printed `1000 ms` and read back `1.00 s`, and where a range's inverse of one second lands just
/// below it, `1.00 s` read back `1000 ms`.
fn v2s_time() -> ValueToString {
    Arc::new(|s| {
        let ms = s * 1000.0;
        if ms.round() >= 1000.0 {
            format!("{s:.2} s")
        } else {
            format!("{ms:.0} ms")
        }
    })
}

/// Cutoff in tenths of a hertz, then kHz — `mxm-mono-pr1`'s reading, in place of
/// `formatters::v2s_f32_hz_then_khz`, which chooses its unit from the raw value: 999.95 Hz printed
/// `1000.0 Hz` and read back `1.0 kHz`. The `1.0 kHz` bucket reads in whole hertz, because 1000 Hz
/// can preview just below the unit boundary after the normalised inverse.
fn v2s_cutoff_hz_then_khz() -> ValueToString {
    Arc::new(|value| {
        let rounded_hz = value.round();
        if (1_000.0..=1_050.0).contains(&rounded_hz) {
            format!("{rounded_hz:.0} Hz")
        } else if value < 1_000.0 {
            format!("{value:.1} Hz")
        } else {
            format!("{:.1} kHz", value / 1_000.0)
        }
    })
}

fn s2v_time() -> StringToValue {
    Arc::new(|text| {
        let t = text.trim().to_lowercase();
        let (number, scale) = if let Some(rest) = t.strip_suffix("ms") {
            (rest, 0.001)
        } else if let Some(rest) = t.strip_suffix('s') {
            (rest, 1.0)
        } else {
            (t.as_str(), 0.001)
        };
        number.trim().parse::<f32>().ok().map(|v| v * scale)
    })
}

/// Show a `0..=1` control as a percentage.
fn v2s_percent() -> ValueToString {
    Arc::new(|v| format!("{:.0} %", v * 100.0))
}

fn s2v_percent() -> StringToValue {
    Arc::new(|text| {
        text.trim()
            .trim_end_matches('%')
            .trim()
            .parse::<f32>()
            .ok()
            .map(|v| v / 100.0)
    })
}

/// A `0..=1` amount, smoothed, shown as a percentage.
fn amount(name: &str, default: f32) -> FloatParam {
    FloatParam::new(name, default, FloatRange::Linear { min: 0.0, max: 1.0 })
        .with_smoother(SmoothingStyle::Linear(10.0))
        .with_value_to_string(v2s_percent())
        .with_string_to_value(s2v_percent())
}

/// An envelope time: unsmoothed, formatted in ms or s. The hardware's ranges.
fn envelope_time(name: &str, default: f32, max: f32) -> FloatParam {
    FloatParam::new(
        name,
        default,
        FloatRange::Skewed {
            min: 0.0015,
            max,
            factor: FloatRange::skew_factor(-2.0),
        },
    )
    .with_value_to_string(v2s_time())
    .with_string_to_value(s2v_time())
}

/// **The LFO rate's tempo sync** (`plans/plan-tempo-sync-controls.md`): every LFO's ladder, 1/32 to
/// four bars, the top the fastest.
pub const LFO_SYNC: mxm_tempo::Ladder =
    mxm_tempo::Ladder::new(mxm_tempo::Span::LFO, mxm_tempo::Direction::Rate);

#[derive(Params)]
pub struct MxmPoly06Params {
    // ---- LFO ----
    #[id = "lforate"]
    pub lfo_rate: FloatParam,
    /// The LFO rate's tempo sync: its position picks a division of the host's tempo.
    #[id = "lfosync"]
    pub lfo_sync: BoolParam,
    #[id = "lfodelay"]
    pub lfo_delay: FloatParam,

    // ---- DCO ----
    #[id = "range"]
    pub range: EnumParam<Range>,
    /// The pulse width: the base the pulse-width routes sum onto.
    #[id = "pulsewidth"]
    pub pulse_width: FloatParam,
    #[id = "pulse"]
    pub pulse: BoolParam,
    #[id = "saw"]
    pub saw: BoolParam,
    #[id = "sub"]
    pub sub: FloatParam,
    #[id = "noise"]
    pub noise: FloatParam,

    // ---- HPF ----
    #[id = "hpf"]
    pub hpf: EnumParam<HpfPosition>,

    // ---- VCF ----
    #[id = "cutoff"]
    pub cutoff: FloatParam,
    #[id = "resonance"]
    pub resonance: FloatParam,

    // ---- VCA ----
    /// The patch's level. **Before the chorus**, so it sets how hard the BBD is driven.
    #[id = "level"]
    pub level: FloatParam,
    #[id = "vcamode"]
    pub vca_mode: EnumParam<VcaMode>,

    // ---- ENV ----
    #[id = "attack"]
    pub attack: FloatParam,
    #[id = "decay"]
    pub decay: FloatParam,
    #[id = "sustain"]
    pub sustain: FloatParam,
    #[id = "release"]
    pub release: FloatParam,

    // ---- CHORUS ----
    #[id = "chorus"]
    pub chorus: EnumParam<ChorusMode>,

    // ---- Voice ----
    /// Portamento time. An **amount**: with it at zero there is no portamento, so it starts there.
    #[id = "portamento"]
    pub portamento: FloatParam,
    #[id = "keyassign"]
    pub key_assign: EnumParam<KeyAssign>,
    /// The master volume. **After the chorus.**
    #[id = "volume"]
    pub volume: FloatParam,

    // ================= Disclosed =================
    //
    // The bender's range and the wheel's reach, from the left cheek of the panel: easy to miss there,
    // so disclosed here rather than hidden. The bender's reach into the VCF is a route now.
    /// Bend sensitivity into the DCO, in semitones.
    #[id = "bendrange"]
    pub bend_range: FloatParam,
    /// How much LFO the forward push — the mod wheel — adds to the pitch.
    #[id = "lfomod"]
    pub lfo_mod: FloatParam,

    /// Every modulation route: a presence and a signed amount per *(target, source)* pair.
    #[nested(group = "Modulation")]
    pub routes: crate::routes::Routes,

    /// Which preset is loaded, and what it looked like when it was.
    ///
    /// **Persisted with the patch, not beside it.** nice-plug carries non-parameter state through
    /// the `Params` derive's `#[persist]`, so it belongs here rather than as a field on the plugin
    /// struct — and it has its own version number, because nice-plug's state version is
    /// `Plugin::VERSION`, which moves for unrelated reasons.
    #[persist = "preset"]
    pub preset: RwLock<mxm_preset::PresetIdentity>,
}

impl Default for MxmPoly06Params {
    /// The init patch, which is also the set of CLAP `default_value`s.
    ///
    /// They are allowed to be equal and must not be two concepts: a host shows `default_value` as a
    /// control's detent and uses it for "reset this parameter", so a divergence means the Init
    /// button and the host disagree about the same sound.
    fn default() -> Self {
        Self {
            // ---- LFO: a vibrato rate, so it is vibrato the moment a depth is raised ----
            lfo_rate: FloatParam::new(
                "LFO rate",
                5.0,
                FloatRange::Skewed {
                    min: 0.1,
                    max: mxm_poly_06_dsp::lfo::RATE_MAX_HZ,
                    factor: FloatRange::skew_factor(-1.5),
                },
            )
            .with_unit(" Hz")
            .with_value_to_string(formatters::v2s_f32_rounded(2)),
            lfo_sync: BoolParam::new("LFO sync", false),
            lfo_delay: FloatParam::new("LFO delay", 0.0, FloatRange::Linear { min: 0.0, max: 3.0 })
                .with_value_to_string(v2s_time())
                .with_string_to_value(s2v_time()),

            // ---- DCO: one plain source sounds ----
            range: EnumParam::new("Range", Range::Eight),
            pulse_width: FloatParam::new(
                "Pulse width",
                0.5,
                FloatRange::Linear {
                    min: 0.05,
                    max: 0.95,
                },
            )
            .with_smoother(SmoothingStyle::Linear(10.0))
            .with_value_to_string(v2s_percent())
            .with_string_to_value(s2v_percent()),
            pulse: BoolParam::new("Pulse", false),
            saw: BoolParam::new("Saw", true),
            sub: amount("Sub", 0.0),
            noise: amount("Noise", 0.0),

            // ---- HPF: the neutral position ----
            hpf: EnumParam::new("HPF", HpfPosition::Flat),

            // ---- VCF: open, and short of the range's end ----
            cutoff: FloatParam::new(
                "Cutoff",
                16_000.0,
                FloatRange::Skewed {
                    min: 20.0,
                    max: 20_000.0,
                    factor: FloatRange::skew_factor(-2.0),
                },
            )
            .with_smoother(SmoothingStyle::Logarithmic(10.0))
            // No `with_unit`: this formatter carries its own, switching between Hz and kHz, and a
            // second one made the round trip read "1.0 kHz Hz" — which clap-validator caught.
            .with_value_to_string(v2s_cutoff_hz_then_khz())
            .with_string_to_value(formatters::s2v_f32_hz_then_khz()),
            resonance: amount("Resonance", 0.0),

            // ---- VCA ----
            level: FloatParam::new(
                "Level",
                util::db_to_gain(-2.0),
                FloatRange::Skewed {
                    min: util::db_to_gain(-60.0),
                    max: util::db_to_gain(0.0),
                    factor: FloatRange::gain_skew_factor(-60.0, 0.0),
                },
            )
            .with_smoother(SmoothingStyle::Logarithmic(20.0))
            .with_unit(" dB")
            .with_value_to_string(formatters::v2s_f32_gain_to_db(1))
            .with_string_to_value(formatters::s2v_f32_gain_to_db()),
            vca_mode: EnumParam::new("VCA mode", VcaMode::Envelope),

            // ---- ENV: the hardware's ranges, a sustained shape to start ----
            attack: envelope_time("Attack", 0.002, 3.0),
            decay: envelope_time("Decay", 0.4, 12.0),
            sustain: FloatParam::new("Sustain", 0.8, FloatRange::Linear { min: 0.0, max: 1.0 })
                .with_smoother(SmoothingStyle::Linear(20.0))
                .with_value_to_string(v2s_percent())
                .with_string_to_value(s2v_percent()),
            release: envelope_time("Release", 0.3, 12.0),

            // ---- CHORUS: off. It is the amount of an effect, and Init is the plain machine ----
            // "Chorus mode", not "Chorus": the control sits in a card called Chorus, and a label that
            // repeats the card's title says nothing; in a host's automation list it says which thing.
            chorus: EnumParam::new("Chorus mode", ChorusMode::Off),

            // ---- Voice ----
            portamento: FloatParam::new(
                "Portamento",
                0.0,
                FloatRange::Skewed {
                    min: 0.0,
                    max: 2.0,
                    factor: FloatRange::skew_factor(-1.5),
                },
            )
            .with_value_to_string(v2s_time())
            .with_string_to_value(s2v_time()),
            key_assign: EnumParam::new("Key assign", KeyAssign::Poly1),
            volume: FloatParam::new(
                "Volume",
                util::db_to_gain(-6.0),
                FloatRange::Skewed {
                    min: util::db_to_gain(-60.0),
                    max: util::db_to_gain(0.0),
                    factor: FloatRange::gain_skew_factor(-60.0, 0.0),
                },
            )
            // Stored as linear gain, formatted as dB: `SmoothingStyle::Logarithmic` over a dB range
            // spanning zero is mathematically invalid and trips a debug assertion.
            .with_smoother(SmoothingStyle::Logarithmic(20.0))
            .with_unit(" dB")
            .with_value_to_string(formatters::v2s_f32_gain_to_db(1))
            .with_string_to_value(formatters::s2v_f32_gain_to_db()),

            // ---- Disclosed ----
            bend_range: FloatParam::new(
                "Bend range",
                2.0,
                FloatRange::Linear {
                    min: 0.0,
                    max: 24.0,
                },
            )
            .with_smoother(SmoothingStyle::Linear(20.0))
            .with_unit(" st")
            .with_value_to_string(formatters::v2s_f32_rounded(0)),
            lfo_mod: amount("Wheel to LFO", 0.5),

            routes: crate::routes::Routes::new(),

            preset: RwLock::new(mxm_preset::PresetIdentity::none()),
        }
    }
}

impl MxmPoly06Params {
    /// The LFO rate while its sync follows the host, or `None` for its free value: the modulated
    /// position picks a division on [`LFO_SYNC`]. Resolved once a buffer by the plugin.
    pub fn synced_lfo_rate(&self, tempo: Option<f64>) -> Option<f32> {
        let param = &self.lfo_rate;
        LFO_SYNC
            .resolve(
                self.lfo_sync.value(),
                tempo,
                param.modulated_normalized_value(),
                f64::from(param.preview_plain(0.0)),
                f64::from(param.preview_plain(1.0)),
            )
            .map(|hz| hz as f32)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **The LFO sync picks a division and is inert without a tempo**
    /// (`plans/plan-tempo-sync-controls.md`): off, or with no tempo, the knob's own hertz stand; on
    /// at 120 bpm the ends are the ladder's ends that the range can hold, the top the fastest.
    #[test]
    fn lfo_sync_picks_a_division_and_is_inert_without_a_tempo() {
        use nice_plug::params::InternalParamMut;
        fn set<P: InternalParamMut>(param: &P, normalized: f32) {
            unsafe {
                let _ = param._internal_set_normalized_value(normalized);
            }
        }
        let p = MxmPoly06Params::default();
        set(&p.lfo_rate, 1.0);
        assert_eq!(p.synced_lfo_rate(Some(120.0)), None, "off is the free rate");
        set(&p.lfo_sync, 1.0);
        assert_eq!(p.synced_lfo_rate(None), None, "no tempo is the free rate");

        let top = p.synced_lfo_rate(Some(120.0)).expect("synced at a tempo");
        set(&p.lfo_rate, 0.0);
        let bottom = p.synced_lfo_rate(Some(120.0)).expect("synced at a tempo");
        let (lo, hi) = (
            f64::from(p.lfo_rate.preview_plain(0.0)),
            f64::from(p.lfo_rate.preview_plain(1.0)),
        );
        assert!(
            top > bottom,
            "the top of a rate is the fastest: {bottom} to {top}"
        );
        let reach = LFO_SYNC.reachable(120.0, lo, hi).divisions();
        let fastest = reach[0].hz(120.0) as f32;
        let slowest = reach[reach.len() - 1].hz(120.0) as f32;
        assert!((top - fastest).abs() < 1e-4, "{top} against {fastest}");
        assert!(
            (bottom - slowest).abs() < 1e-4,
            "{bottom} against {slowest}"
        );
    }

    /// Plain values either side of every point where a formatter here changes unit, precision or
    /// sign. Each parameter clamps what lies outside its own range, so one list serves them all.
    const BOUNDARIES: [f32; 34] = [
        // Every route amount crosses zero: a ten-thousandth and a thousandth of the unit, and the
        // half-hundredth where two decimals tie.
        -0.005, -1.0e-3, -1.0e-4, 0.0, 1.0e-4, 1.0e-3, 0.005,
        // Envelope times, LFO delay and portamento read whole milliseconds below a second and
        // hundredths of a second above: both rounding edges below one second and the `1.00 s`
        // bucket above it.
        0.9994, 0.9995, 0.9996, 0.99995, 1.0, 1.004, 1.005, 1.006,
        // Cutoff reads tenths of a hertz, then whole hertz across the `1.0 kHz` bucket, then kHz:
        // either side of the tenth that rounds to 999.5 Hz, of 1000 Hz, and of 1050.5 Hz.
        999.4, 999.44, 999.45, 999.46, 999.49, 999.5, 999.9, 999.95, 1_000.0, 1_000.1, 1_049.9,
        1_050.4, 1_050.45, 1_050.5, 1_050.6,
        // Level and Volume read tenths of a decibel of a linear gain, and -0.05 dB is where the
        // reading rounds to zero at the top of their range.
        0.99425, 0.99426, 0.99427, 0.9999,
    ];

    /// **Every parameter's text survives the host's own conversion.** The CLAP wrapper formats a
    /// normalised value, parses the text back to a normalised value and formats that again, so a
    /// reading that chooses its unit or its sign from the raw value can print one text, parse to the
    /// other side of its own switch and print another — which `clap-validator`'s
    /// `param-conversions` fails only when its values land in that sliver, so a clean run proves
    /// nothing (`docs/code-review-notes.md` §6). This walks every parameter, with the unit on as the
    /// host sees it, across clap-validator 0.4.1's own grid, the collection's `i / 19` grid, and
    /// the normalised neighbours of every value in [`BOUNDARIES`].
    #[test]
    fn every_parameter_text_is_idempotent_through_the_hosts_conversion() {
        let params = MxmPoly06Params::default();
        let map = params.param_map();
        let validator_values = 4_000_usize.div_ceil(map.len()).clamp(5, 100);
        let mut failures: Vec<String> = Vec::new();
        for (id, ptr, _group) in &map {
            // SAFETY: `params` owns every parameter these pointers refer to and outlives the loop;
            // this is the same access `param-conversions` makes through CLAP.
            unsafe {
                // The wrapper hands CLAP `normalised × step count` and divides by it on the way in.
                let steps = ptr.step_count().unwrap_or(1) as f64;
                let from_clap = |value: f64| value as f32 / steps as f32;
                let grid = (0..=19).map(|i| (i as f32 / 19.0, "grid"));
                let validator = (0..validator_values).map(|i| {
                    let value = steps * (i as f64 / (validator_values - 1) as f64);
                    (from_clap(value), "validator grid")
                });
                let boundary = BOUNDARIES.into_iter().flat_map(|plain| {
                    let at = ptr.preview_normalized(plain).clamp(0.0, 1.0);
                    [at.next_down().max(0.0), at, at.next_up().min(1.0)].map(|n| (n, "boundary"))
                });
                for (value, from) in grid.chain(validator).chain(boundary) {
                    let first = ptr.normalized_value_to_string(value, true);
                    let second = ptr.string_to_normalized_value(&first).map(|parsed| {
                        ptr.normalized_value_to_string(from_clap(parsed as f64 * steps), true)
                    });
                    if second.as_deref() != Some(first.as_str()) {
                        let failure = format!("{id}: {first:?} reads back as {second:?}");
                        if failures
                            .last()
                            .is_none_or(|last| !last.starts_with(&failure))
                        {
                            let plain = ptr.preview_plain(value);
                            failures.push(format!("{failure} ({from}, plain {plain})"));
                        }
                    }
                }
            }
        }
        assert!(
            failures.is_empty(),
            "{} parameter texts changed through the host's conversion:\n{}",
            failures.len(),
            failures.join("\n")
        );
    }
}
