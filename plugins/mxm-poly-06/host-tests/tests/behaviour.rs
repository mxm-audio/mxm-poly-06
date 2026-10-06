//! Does mxm-poly-06 do what a six-voice polysynth with a chorus says it does?
//!
//! Measured through the real player, on rendered audio — not by reading the DSP and agreeing with
//! it. Every assertion here is a property of the sound: a set of pitches, a stereo difference, a
//! level, a spectrum.
//!
//! The plan's §10 names what only this instrument claims; these are the ones a host can see.

use mxm_player_harness::app_harness;

use mxm_player::events::input::Payload;
use mxm_player::session::{FRAMES_PER_BLOCK, Session};
use std::path::PathBuf;

const PLUGIN: &str = "dk.mxm.mxm-poly-06";
const SAMPLE_RATE: f64 = 48_000.0;

fn bundle() -> Option<(PathBuf, PathBuf)> {
    let dir = app_harness::bundled_dir_with("mxm-poly-06")?;
    let file = dir.join("mxm-poly-06.clap");
    file.exists().then_some((dir, file))
}

fn session(name: &str) -> Option<Session> {
    let (dir, file) = bundle()?;
    let mut s = Session::scratch(name, vec![dir]);
    s.load(&file, PLUGIN);
    Some(s)
}

const SKIP: &str = "skipping: run `cargo xtask bundle mxm-poly-06 --release`";

// --- measurement --------------------------------------------------------------------------------

use mxm_measure::channels::left;

use mxm_measure::channels::right;

/// Peak magnitude of a capture.
///
/// **A shim over `mxm-measure`, and the `expect` is the point.** The shared ruler reports absence for
/// a **non-finite** buffer rather than the largest number in it, because `f32::max` would otherwise
/// let a render that is half NaN measure as perfectly healthy — and then pass every "is it quiet?"
/// assertion below. Panicking here is the loud failure that behaviour deserves.
fn peak(samples: &[f32]) -> f32 {
    mxm_measure::level::peak(samples).expect("the capture is finite")
}

/// RMS via `mxm-measure`, narrowed to `f32` for these call sites.
///
/// **Absence panics rather than reading as zero.** The shared ruler declines for an empty buffer and
/// for a **non-finite** one; turning the second into `0.0` would let a broken render pass a silence
/// assertion, which is exactly what the result-form contract is for.
fn rms(samples: &[f32]) -> f32 {
    mxm_measure::level::rms(samples).expect("the capture is non-empty and finite") as f32
}

/// How much of one frequency is in a captured window — **a relative figure, not an amplitude.**
///
/// Two reasons it is relative, and both matter to anyone quoting a number from these tests:
///
/// - **The capture length is the session's, not ours.** `component_amplitude` reads a component's
///   true amplitude only over a whole number of cycles; these windows are whole blocks, so the
///   reading carries spectral leakage. Comparing one pitch against another in the same window is
///   sound — the leakage is common to both — and calling the result an absolute amplitude is not.
/// - **The absolute value moved by 6 dB with the migration to the shared probe**, which is a
///   correction rather than a regression: every local copy of this helper computed `|X|/N`, half a
///   component's amplitude, and the shared probe reports the amplitude. A figure quoted from an
///   older run of these tests is 6 dB low.
///
/// **Absence panics rather than reading as zero.** The probe declines for two reasons — an empty
/// window, which cannot happen here, and a **non-finite render**, which can. Folding that into `0.0`
/// would let a NaN-producing plugin sail through every "quieter than" and "silent" assertion below,
/// which is the precise failure the shared crate's result-form contract exists to prevent.
fn magnitude_at(samples: &[f32], hz: f64) -> f64 {
    mxm_measure::spectrum::component_amplitude(samples, hz, SAMPLE_RATE)
        .expect("the capture is non-empty and finite")
}

use mxm_measure::convert::note_hz;

/// A crude high-frequency measure: mean absolute sample-to-sample difference. Relative only.
fn brightness(samples: &[f32]) -> f32 {
    if samples.len() < 2 {
        return 0.0;
    }
    samples.windows(2).map(|w| (w[1] - w[0]).abs()).sum::<f32>() / (samples.len() - 1) as f32
}

// --- driving ------------------------------------------------------------------------------------

/// Sets a parameter by name, as a position in its normalised range, and lets it settle.
fn set_param(session: &mut Session, name: &str, fraction: f64) -> String {
    let param = session
        .state()
        .param(name)
        .unwrap_or_else(|| panic!("`{name}` is not a parameter"))
        .clone();
    let value = param.min + fraction * (param.max - param.min);
    session
        .app()
        .engine_mut()
        .push_gui_event(Payload::ParamValue {
            param_id: param.id,
            value,
        });
    session.advance_blocks(4).expect("the session advances");
    session
        .state()
        .param(name)
        .map(|p| p.text.clone())
        .unwrap_or_default()
}

/// Holds `notes`, renders `blocks`, releases them, and returns the interleaved sustained portion.
fn chord(session: &mut Session, notes: &[u8], blocks: u64) -> Vec<f32> {
    session.clear_capture();
    for &n in notes {
        session.app().note_on(n, 100.0 / 127.0);
    }
    session.advance_blocks(blocks).expect("advances");
    let audio = session.captured();
    for &n in notes {
        session.app().note_off(n);
    }
    session.advance_blocks(60).expect("advances");
    let skip = (FRAMES_PER_BLOCK * 2 * 3).min(audio.len());
    audio[skip..].to_vec()
}

// --- the tests ----------------------------------------------------------------------------------

#[test]
fn at_rest_it_is_exactly_silent() {
    let Some(mut s) = session("poly-rest") else {
        eprintln!("{SKIP}");
        return;
    };
    s.advance_blocks(20).expect("advances");
    assert_eq!(
        peak(&s.captured()),
        0.0,
        "an idle synth must render exact zeros, not merely something quiet"
    );
}

#[test]
fn six_notes_sound_six_pitches() {
    let Some(mut s) = session("poly-six") else {
        eprintln!("{SKIP}");
        return;
    };
    set_param(&mut s, "Cutoff", 1.0);
    let notes = [48u8, 52, 55, 59, 62, 65];
    let audio = left(&chord(&mut s, &notes, 12));
    let floor = magnitude_at(&audio, note_hz(70.0)); // a pitch nobody played
    for &n in &notes {
        let m = magnitude_at(&audio, note_hz(f64::from(n)));
        assert!(
            m > floor * 4.0,
            "note {n} at {:.1} Hz is not in the chord: {m:e} against a floor of {floor:e}",
            note_hz(f64::from(n))
        );
    }
}

#[test]
fn a_seventh_note_steals_a_voice_and_the_chord_stays_at_six() {
    let Some(mut s) = session("poly-steal") else {
        eprintln!("{SKIP}");
        return;
    };
    set_param(&mut s, "Cutoff", 1.0);
    // Six held, then a seventh: POLY 1 steals the oldest press's voice, which held note 48.
    let notes = [48u8, 52, 55, 59, 62, 65, 69];
    let audio = left(&chord(&mut s, &notes, 12));
    let floor = magnitude_at(&audio, note_hz(72.0));
    assert!(
        magnitude_at(&audio, note_hz(69.0)) > floor * 4.0,
        "the seventh note must sound"
    );
    assert!(
        magnitude_at(&audio, note_hz(48.0)) < magnitude_at(&audio, note_hz(52.0)) * 0.5,
        "the oldest note should have been stolen"
    );
}

#[test]
fn the_chorus_makes_the_stereo_and_off_does_not() {
    let Some(mut s) = session("poly-chorus") else {
        eprintln!("{SKIP}");
        return;
    };
    let off = chord(&mut s, &[57], 12);
    assert_eq!(
        left(&off),
        right(&off),
        "with the chorus off the two channels must be identical"
    );

    set_param(&mut s, "Chorus mode", 1.0 / 3.0); // I
    let on = chord(&mut s, &[57], 12);
    let (l, r) = (left(&on), right(&on));
    let differ = l
        .iter()
        .zip(&r)
        .filter(|(a, b)| (*a - *b).abs() > 1e-3)
        .count();
    assert!(
        differ > l.len() / 2,
        "with the chorus on the channels differed on only {differ} of {} samples",
        l.len()
    );
}

#[test]
fn the_hpfs_bottom_position_boosts_the_bass() {
    let Some(mut s) = session("poly-hpf") else {
        eprintln!("{SKIP}");
        return;
    };
    let low = 36u8; // C2, 65 Hz: inside the boost
    let flat = rms(&left(&chord(&mut s, &[low], 12)));
    set_param(&mut s, "HPF", 0.0);
    let boosted = rms(&left(&chord(&mut s, &[low], 12)));
    set_param(&mut s, "HPF", 1.0);
    let cut = rms(&left(&chord(&mut s, &[low], 12)));
    assert!(boosted > flat * 1.5, "boost {boosted} against flat {flat}");
    assert!(cut < flat * 0.5, "cut 2 {cut} against flat {flat}");
}

#[test]
fn closing_the_filter_darkens_the_sound() {
    let Some(mut s) = session("poly-cutoff") else {
        eprintln!("{SKIP}");
        return;
    };
    let open = brightness(&left(&chord(&mut s, &[57], 12)));
    set_param(&mut s, "Cutoff", 0.25);
    let closed = brightness(&left(&chord(&mut s, &[57], 12)));
    assert!(closed < open * 0.5, "closed {closed} against open {open}");
}

#[test]
fn unison_is_louder_than_one_voice() {
    let Some(mut s) = session("poly-unison") else {
        eprintln!("{SKIP}");
        return;
    };
    let one = rms(&left(&chord(&mut s, &[57], 12)));
    set_param(&mut s, "Key assign", 1.0);
    let six = rms(&left(&chord(&mut s, &[57], 12)));
    assert!(six > one * 3.0, "unison {six} against a single voice {one}");
}

#[test]
fn a_release_ends_in_exact_silence() {
    let Some(mut s) = session("poly-tail") else {
        eprintln!("{SKIP}");
        return;
    };
    set_param(&mut s, "Chorus mode", 1.0); // I + II: the noise floor has to fade too
    s.clear_capture();
    s.app().note_on(60, 100.0 / 127.0);
    s.advance_blocks(8).expect("advances");
    s.app().note_off(60);
    // Release 0.3 s plus the post-mix settle of 0.6 s: two seconds is plenty.
    s.advance_blocks(200).expect("advances");
    s.clear_capture();
    s.advance_blocks(10).expect("advances");
    assert_eq!(peak(&s.captured()), 0.0, "the tail must reach exact zero");
}

/// **The headline gesture, through the real callback**: the envelope's sweep of the filter — the
/// VCF's ENV slider on the machine, the route `mod_cutoff_env` now — raised by the host over a
/// six-note chord with the filter closed. A route wired to the wrong target, or a parameter that
/// reaches nothing, leaves the chord as dark as it was.
#[test]
fn raising_the_envelope_route_opens_a_closed_filter_over_a_chord() {
    let Some(mut s) = session("poly-env-route") else {
        eprintln!("{SKIP}");
        return;
    };
    const CHORD: [u8; 6] = [48, 52, 55, 60, 64, 67];
    set_param(&mut s, "Cutoff", 0.25);
    let closed = brightness(&left(&chord(&mut s, &CHORD, 12)));
    let text = set_param(&mut s, "Cutoff from Envelope", 1.0);
    assert!(
        text.contains("+7.00 oct"),
        "the route reads the ENV slider's full reach: {text}"
    );
    let swept = brightness(&left(&chord(&mut s, &CHORD, 12)));
    assert!(
        swept > closed * 2.0,
        "swept {swept} against closed {closed}"
    );
}

/// **A route the machine never had**, added and raised through the real callback: the LFO into the
/// amplitude, a tremolo the JUNO could not make. The level holds steady window to window before the
/// route exists and swings with the LFO once it does.
#[test]
fn routing_the_lfo_to_amplitude_makes_a_tremolo() {
    let Some(mut s) = session("poly-amp-route") else {
        eprintln!("{SKIP}");
        return;
    };
    // The relative swing of each block's level, after a second for the envelope to settle.
    let swing = |audio: &[f32]| -> f32 {
        let levels: Vec<f32> = left(audio)[FRAMES_PER_BLOCK * 45..]
            .as_chunks::<FRAMES_PER_BLOCK>()
            .0
            .iter()
            .map(|block| rms(block.as_slice()))
            .collect();
        let lo = levels.iter().copied().fold(f32::INFINITY, f32::min);
        let hi = levels.iter().copied().fold(0.0f32, f32::max);
        (hi - lo) / hi
    };
    let steady = swing(&chord(&mut s, &[57], 100));
    set_param(&mut s, "Amplitude from LFO on", 1.0);
    let text = set_param(&mut s, "Amplitude from LFO", 1.0);
    assert!(
        text.contains("+100 %"),
        "the route reads its full reach: {text}"
    );
    let tremolo = swing(&chord(&mut s, &[57], 100));
    assert!(steady < 0.2, "without the route the level holds: {steady}");
    assert!(tremolo > 0.6, "with it the level swings: {tremolo}");
}
