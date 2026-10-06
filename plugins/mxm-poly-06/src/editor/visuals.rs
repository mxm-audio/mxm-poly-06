//! The brief's §8 displays.
//!
//! Two here; the third — output level with clip indication — is the app bar's, per §3.1.
//!
//! Both draw with theme tokens and never with a literal colour: `crates/ui/AGENTS.md` is explicit
//! that a consumer needing a value the theme does not expose adds the token there rather than the
//! literal here.

use egui::{Color32, Pos2, Rect, Sense, Stroke, Ui, Vec2, pos2};
use mxm_poly_06_dsp::filter::K_MAX;
use mxm_poly_06_dsp::hpf::{self, Position};
use mxm_poly_06_dsp::voice::VOICES;
use mxm_ui::space::{HAIRLINE, RADIUS, SPACE_2};
use mxm_ui::theme::Tokens;

/// The height a display gets when nothing else decides.
pub const HEIGHT: f32 = 72.0;

/// The response curve's narrowest: nothing of its own. It is a picture of the controls around it,
/// drawn across whatever width they give the card, and it holds no text a width could break.
pub const RESPONSE_MIN_WIDTH: f32 = 0.0;

/// The voice display's height: two lines of text in a cell, and a bar behind them.
pub const VOICES_HEIGHT: f32 = 56.0;

/// The voice display's narrowest: six cells, each as wide as the widest key [`voices`] can name in
/// the caption style with `SPACE_2` either side, `SPACE_2` between them and at both ends — the
/// arithmetic [`voices`] lays its cells out by, run backwards.
#[must_use]
pub fn voices_min_width(ui: &Ui) -> f32 {
    let font = mxm_ui::typography::caption_style(ui.style()).resolve(ui.style());
    let widest = (0..=127u8)
        .map(|note| {
            ui.painter()
                .layout_no_wrap(note_name(note), font.clone(), Color32::PLACEHOLDER)
                .size()
                .x
        })
        .fold(0.0, f32::max);
    let cell = widest + 2.0 * SPACE_2;
    cell * VOICES as f32 + SPACE_2 * (VOICES as f32 + 1.0)
}

/// The plotted frequency span.
const PLOT_LOW_HZ: f32 = 20.0;
const PLOT_HIGH_HZ: f32 = 20_000.0;

/// The bottom of the plotted magnitude span, in dB.
const PLOT_BOTTOM_DB: f32 = -48.0;

/// The **least** headroom above the passband, in dB. The top is scaled to what the curves reach,
/// because a resonant peak grows without bound toward threshold and any fixed ceiling is one a high
/// setting walks through — `mxm-mono-03` records having shipped that.
const PLOT_MIN_TOP_DB: f32 = 12.0;

/// Breathing room above the tallest peak.
const PLOT_HEADROOM_DB: f32 = 3.0;

/// How finely a curve is sampled.
const POINTS: usize = 160;

/// The whole filter section's response: the global HPF **and** the per-voice lowpass, as one curve.
///
/// # Why one curve
///
/// The HPF's bottom position is a bass *boost*, and the two most common mistakes about this machine
/// are to think of position 0 as "off" and to think of the HPF as part of the voice. Drawing the
/// two filters as one response shows the boost lifting the low end under the lowpass's corner,
/// which is what a chord actually goes through.
///
/// # Declared approximation
///
/// The **linear analytic response** at the current settings: four identical poles with global
/// feedback for the lowpass, normalised so its passband sits at 0 dB, plus the HPF's one-pole
/// prototype. It ignores the compensation's saturation and the per-voice capacitor spread, so a
/// measured sweep at high resonance will not match it exactly. Stated here rather than left to be
/// discovered.
pub fn filter_response(
    ui: &mut Ui,
    tokens: &Tokens,
    hpf: Position,
    cutoff_hz: f32,
    resonance: f32,
    env_reach_hz: f32,
    height: f32,
) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), height), Sense::hover());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, RADIUS as f32, tokens.surface_2);

    let mut curves: Vec<(f32, Color32, f32)> = Vec::new();
    if (env_reach_hz / cutoff_hz).log2().abs() > 0.02 {
        curves.push((env_reach_hz, faded(tokens.mod_envelope), HAIRLINE));
    }
    curves.push((cutoff_hz, tokens.accent, HAIRLINE * 1.5));

    let samples: Vec<Vec<f32>> = curves
        .iter()
        .map(|(hz, _, _)| response_db(hpf, *hz, resonance))
        .collect();
    let peak = samples
        .iter()
        .flat_map(|curve| curve.iter().copied())
        .fold(f32::NEG_INFINITY, f32::max);
    let top_db = (peak + PLOT_HEADROOM_DB).max(PLOT_MIN_TOP_DB);

    for (curve, (_, colour, width)) in samples.iter().zip(&curves) {
        plot(&painter, rect, curve, top_db, *colour, *width);
    }

    painter.rect_stroke(
        rect,
        RADIUS as f32,
        Stroke::new(HAIRLINE, tokens.border),
        egui::StrokeKind::Inside,
    );
}

/// One curve's magnitude, in dB, sampled evenly in log frequency.
fn response_db(hpf: Position, cutoff_hz: f32, resonance: f32) -> Vec<f32> {
    (0..POINTS)
        .map(|i| {
            let t = i as f32 / (POINTS - 1) as f32;
            let hz = PLOT_LOW_HZ * (PLOT_HIGH_HZ / PLOT_LOW_HZ).powf(t);
            lowpass_db(cutoff_hz, resonance, hz) + hpf::magnitude_db(hpf, hz)
        })
        .collect()
}

/// The four-pole ladder's magnitude at one frequency, in dB, passband at 0.
///
/// `H(s) = 1 / ((1 + s/wc)^4 + k)`, evaluated on the imaginary axis and normalised by its DC gain
/// `1/(1+k)` — the droop the JUNO's compensation puts back is not drawn, because what the plot is
/// for is the corner's shape, not the level.
fn lowpass_db(cutoff_hz: f32, resonance: f32, hz: f32) -> f32 {
    let k = K_MAX * resonance.clamp(0.0, 1.0);
    let ratio = hz / cutoff_hz.max(1.0);
    // (1 + j r)^4 by repeated complex multiplication.
    let (mut re, mut im) = (1.0f32, 0.0f32);
    for _ in 0..4 {
        let (nr, ni) = (re - im * ratio, re * ratio + im);
        re = nr;
        im = ni;
    }
    re += k;
    let magnitude = 1.0 / (re * re + im * im).sqrt().max(1e-12);
    let dc = 1.0 / (1.0 + k);
    20.0 * (magnitude / dc).max(1e-6).log10()
}

fn plot(
    painter: &egui::Painter,
    rect: Rect,
    curve: &[f32],
    top_db: f32,
    colour: Color32,
    width: f32,
) {
    let span = top_db - PLOT_BOTTOM_DB;
    let points: Vec<Pos2> = curve
        .iter()
        .enumerate()
        .map(|(i, db)| {
            let t = i as f32 / (curve.len() - 1) as f32;
            let y = (top_db - db) / span;
            pos2(
                rect.left() + t * rect.width(),
                rect.top() + y.clamp(0.0, 1.0) * rect.height(),
            )
        })
        .collect();
    painter.add(egui::Shape::line(points, Stroke::new(width, colour)));
}

/// The six voices: which is sounding what, and how loud.
///
/// # The display this instrument exists to have
///
/// POLY 1 reuses low-numbered cards and POLY 2 rotates, and because the cards differ slightly the
/// two modes sound different — but nothing on the panel shows *which* card a note landed on. Six
/// cells, one per voice, each with its envelope level as a bar and its key as text while it sounds.
/// A seventh key stealing a voice reads as the machine's behaviour rather than a dropout, because
/// the cell it took is visibly the one that changed.
///
/// **Exact**, not approximated: the values the DSP used, published once per block.
pub fn voices(ui: &mut Ui, tokens: &Tokens, levels: &[f32; VOICES], notes: &[(u8, bool); VOICES]) {
    let (rect, _) = ui.allocate_exact_size(
        Vec2::new(ui.available_width(), VOICES_HEIGHT),
        Sense::hover(),
    );
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, RADIUS as f32, tokens.surface_2);
    // The caption style's font, resolved once: the collection's typography, not a literal size.
    let font = mxm_ui::typography::caption_style(ui.style()).resolve(ui.style());

    let gap = SPACE_2;
    let cell_w = (rect.width() - gap * (VOICES as f32 + 1.0)) / VOICES as f32;
    for (i, (level, (note, held))) in levels.iter().zip(notes).enumerate() {
        let x = rect.left() + gap + i as f32 * (cell_w + gap);
        let cell = Rect::from_min_size(
            pos2(x, rect.top() + gap),
            Vec2::new(cell_w, rect.height() - 2.0 * gap),
        );
        painter.rect_filled(cell, RADIUS as f32, tokens.surface_1);

        // The bar: envelope level, from the bottom. Accent while the key is held, quieter while
        // it releases — a change of hue *and* of a label, never hue alone (§7.2).
        let h = cell.height() * level.clamp(0.0, 1.0);
        if h > 0.5 {
            let bar = Rect::from_min_max(pos2(cell.left(), cell.bottom() - h), cell.max);
            painter.rect_filled(
                bar,
                RADIUS as f32,
                if *held {
                    tokens.accent
                } else {
                    faded(tokens.accent)
                },
            );
        }

        // The voice's number always, and the key beneath it while anything is sounding on it.
        // **Each on a line of its own that never moves** (design system §7.5): the number was
        // centred alone and pushed up half a line when the key appeared beneath it.
        let colour = if *held {
            tokens.text_primary
        } else {
            tokens.text_secondary
        };
        let line = font.size * 1.2;
        painter.text(
            cell.center() - Vec2::new(0.0, line / 2.0),
            egui::Align2::CENTER_CENTER,
            format!("{}", i + 1),
            font.clone(),
            colour,
        );
        if *level > 1e-3 {
            painter.text(
                cell.center() + Vec2::new(0.0, line / 2.0),
                egui::Align2::CENTER_CENTER,
                note_name(*note),
                font.clone(),
                colour,
            );
        }
        painter.rect_stroke(
            cell,
            RADIUS as f32,
            Stroke::new(HAIRLINE, tokens.border),
            egui::StrokeKind::Inside,
        );
    }
}

/// A MIDI note as a name, `C4` for 60.
pub fn note_name(note: u8) -> String {
    const NAMES: [&str; 12] = [
        "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
    ];
    format!(
        "{}{}",
        NAMES[usize::from(note % 12)],
        i32::from(note / 12) - 1
    )
}

/// A secondary trace's colour: the same hue, quieter, so the base curve reads as the base.
fn faded(colour: Color32) -> Color32 {
    Color32::from_rgba_unmultiplied(colour.r(), colour.g(), colour.b(), 110)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Where the voice display paints the text `wanted`, with voice 1 silent or sounding.
    fn painted_at(sounding: bool, wanted: &str) -> Option<Pos2> {
        fn find(shape: &egui::Shape, wanted: &str) -> Option<Pos2> {
            match shape {
                egui::Shape::Text(text) if text.galley.text() == wanted => Some(text.pos),
                egui::Shape::Vec(shapes) => shapes.iter().find_map(|s| find(s, wanted)),
                _ => None,
            }
        }
        let ctx = egui::Context::default();
        mxm_ui::typography::apply(&ctx);
        mxm_ui::theme::apply(&ctx);
        let mut levels = [0.0; VOICES];
        let mut notes = [(60, false); VOICES];
        if sounding {
            levels[0] = 1.0;
            notes[0] = (60, true);
        }
        let mut found = None;
        for _ in 0..3 {
            let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
                ui.set_width(400.0);
                voices(ui, &mxm_ui::LIGHT, &levels, &notes);
            });
            output.textures_delta.clear();
            found = output
                .shapes
                .iter()
                .find_map(|clipped| find(&clipped.shape, wanted));
        }
        found
    }

    /// **A voice's number never moves** (design system §7.5): it keeps its line whether or not a key
    /// sounds on it, and the key takes a line of its own beneath. The number was centred alone, and
    /// jumped up half a line when the key appeared under it.
    #[test]
    fn a_voices_number_holds_still_when_a_note_sounds() {
        let silent = painted_at(false, "1").expect("the silent voice paints its number");
        let sounding = painted_at(true, "1").expect("the sounding voice paints its number");
        assert_eq!(silent, sounding, "the number moved when a note sounded");
        let key = painted_at(true, "C4").expect("the sounding voice names its key");
        assert!(key.y > sounding.y, "the key is not beneath the number");
    }

    #[test]
    fn the_passband_sits_at_zero_whatever_the_resonance() {
        for step in 0..=10 {
            let resonance = step as f32 / 10.0;
            let low = lowpass_db(800.0, resonance, PLOT_LOW_HZ);
            assert!(
                low.abs() < 1.0,
                "at resonance {resonance} the passband sits at {low} dB"
            );
        }
    }

    #[test]
    fn the_curve_stays_inside_the_plot_at_every_resonance() {
        for position in [
            Position::Boost,
            Position::Flat,
            Position::Cut1,
            Position::Cut2,
        ] {
            for step in 0..=20 {
                let resonance = step as f32 / 20.0;
                let curve = response_db(position, 800.0, resonance);
                let peak = curve.iter().copied().fold(f32::NEG_INFINITY, f32::max);
                let top_db = (peak + PLOT_HEADROOM_DB).max(PLOT_MIN_TOP_DB);
                let span = top_db - PLOT_BOTTOM_DB;
                let highest = (top_db - peak) / span;
                assert!(
                    (0.0..=1.0).contains(&highest) && highest > 0.0,
                    "{position:?} at {resonance}"
                );
            }
        }
    }

    #[test]
    fn the_boost_lifts_the_low_end_of_the_combined_curve() {
        let flat = response_db(Position::Flat, 8_000.0, 0.0);
        let boost = response_db(Position::Boost, 8_000.0, 0.0);
        assert!(
            boost[0] - flat[0] > 8.0,
            "at 20 Hz the boost adds {} dB",
            boost[0] - flat[0]
        );
        let last = POINTS - 1;
        assert!(
            (boost[last] - flat[last]).abs() < 0.5,
            "and nothing at the top"
        );
    }

    #[test]
    fn note_names_are_the_usual_ones() {
        assert_eq!(note_name(60), "C4");
        assert_eq!(note_name(69), "A4");
        assert_eq!(note_name(0), "C-1");
        assert_eq!(note_name(127), "G9");
    }
}
