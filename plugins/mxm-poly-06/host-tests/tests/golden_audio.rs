//! T4 for mxm-poly-06 — golden audio: a fixed score through the real application path.
//!
//! `plugins/mxm-mono-01/host-tests/tests/golden_audio.rs` covers mxm-mono-01 and records why a hash is the reference. This is the
//! same test for the polysynth, added with the instrument rather than after it: `crates/
//! mxm-mono-03-dsp/AGENTS.md` records that mono-03 shipped without one and that a DSP change there
//! could move the sound while the whole suite stayed green.
//!
//! # When this fails
//!
//! A failure is not automatically a bug — a deliberate DSP change should fail it. Listen to the
//! artifact it writes, decide whether the change was intended, and update the reference in the same
//! commit as the change that caused it. A silent update is the one thing that makes this worthless.

use mxm_player_harness::app_harness;

use mxm_player::session::Session;
use std::path::PathBuf;

const PLUGIN: &str = "dk.mxm.mxm-poly-06";

/// The committed reference for [`score`], at mxm-poly-06's default parameters.
///
/// Update **only** together with the change that moved it, and record *why* here.
///
/// **First pinned 2026-09-02**, at the instrument's first build: a four-note chord, a fifth note
/// arriving under it, one released, then all released into a tail. **Measured, not listened to**:
/// the render's properties were checked by `plugins/mxm-poly-06/host-tests/tests/behaviour.rs` on the same build — six
/// pitches present, exact silence at rest and after the tail — and nobody has yet played the WAV.
/// The first person who does should say so here.
///
/// **Moved by the first code review (2026-09-02)**, three fixes at once: `process()` no longer
/// advances every smoother an extra sample per host buffer, the saw and pulse switches ramp over the
/// gate time instead of stepping, and a reset filter reseeds with its own card's seed. Rendered and
/// measured by the behaviour suite on the same build; still not listened to.
///
/// **Moved again by the same review's later rounds, back to the first value.** The saw switch's
/// smoothing had been ramping the waveform in over the first two milliseconds of *every* note, which
/// was never the intent: the ramp exists for a switch thrown mid-note. Idle control state is now
/// taken outright, so a note begins at full level, and the two other round-1 fixes turn out to be
/// bit-invisible at the score's default patch. Still not listened to.
const GOLDEN_DIGEST: &str = "ac2b14b5d74e399d";

/// Whether this platform's render can match the pinned digests. They are Windows': each platform's
/// maths library rounds in its own way, so the same score renders different bits on Linux and macOS.
/// The owner pinned them on Windows only, where the sound was recorded and approved (2026-10-06);
/// elsewhere every other check in these tests still runs.
const DIGESTS_PINNED_HERE: bool = cfg!(target_os = "windows");

/// The same score with the chorus **on**, one digest per mode.
///
/// **Pinned 2026-09-04, before `mxm-chorus-06` was built.** The default-patch digest above cannot
/// see the chorus at all — the default is Off — so a change to `chorus.rs` could move the
/// instrument's sound while every test stayed green. The standalone chorus depends on this crate's
/// module and opens its fixed quantities as inputs the synth never touches; these three are what
/// prove the synth heard nothing. Measured, not listened to, like the one above.
const CHORUS_DIGESTS: &[(&str, f64, &str)] = &[
    ("I", 1.0, "5ab3e4eff368ff0d"),
    ("II", 2.0, "43e601b341d0e02a"),
    ("I + II", 3.0, "45e7d8e98bab9c91"),
];

/// How many samples the score renders.
const GOLDEN_SAMPLES: usize = 60 * mxm_player::session::FRAMES_PER_BLOCK * 2;

fn bundle() -> Option<(PathBuf, PathBuf)> {
    let dir = app_harness::bundled_dir_with("mxm-poly-06")?;
    let file = dir.join("mxm-poly-06.clap");
    file.exists().then_some((dir, file))
}

/// The score: a chord, a note joining it, a note leaving it, then all released. Fixed forever.
///
/// It exercises what is easy to break silently in a polysynth — the ledger's assignment order, a
/// press joining a held chord, a release under others still held, and the tail through the post-mix
/// chain — rather than a single note that would pass if five of the six voices stopped working.
fn score(session: &mut Session) -> Result<(), String> {
    session.advance_blocks(2)?;

    for note in [48u8, 55, 60, 64] {
        session.app().note_on(note, 100.0 / 127.0);
    }
    session.advance_blocks(12)?;

    session.app().note_on(67, 90.0 / 127.0);
    session.advance_blocks(8)?;

    session.app().note_off(55);
    session.advance_blocks(8)?;

    for note in [48u8, 60, 64, 67] {
        session.app().note_off(note);
    }
    session.advance_blocks(30)
}

fn render_score(name: &str) -> Option<(Vec<f32>, PathBuf)> {
    render_score_with_chorus(name, None)
}

/// Renders the score, optionally with the chorus set to a mode first (its plain value: 1, 2, 3).
fn render_score_with_chorus(name: &str, chorus: Option<f64>) -> Option<(Vec<f32>, PathBuf)> {
    let (dir, file) = bundle()?;
    let mut session = Session::scratch(name, vec![dir]);
    session.load(&file, PLUGIN);
    if let Some(mode) = chorus {
        let param = session
            .state()
            .param("Chorus mode")
            .expect("Chorus mode is a parameter")
            .clone();
        assert_eq!(
            (param.min, param.max),
            (0.0, 3.0),
            "the chorus is a four-position switch reported as 0..=3"
        );
        session
            .app()
            .engine_mut()
            .push_gui_event(mxm_player::events::input::Payload::ParamValue {
                param_id: param.id,
                value: mode,
            });
    }
    score(&mut session).expect("the session advances");
    let samples = session.captured();
    let (wav, _json) = session
        .write_artifacts(name)
        .expect("the artifacts are written");
    Some((samples, wav))
}

#[test]
fn the_score_still_sounds_the_same() {
    let Some((samples, wav)) = render_score("golden-poly-06") else {
        eprintln!("skipping: run `cargo xtask bundle mxm-poly-06 --release`");
        return;
    };

    assert_eq!(samples.len(), GOLDEN_SAMPLES, "the score's length changed");
    assert!(
        samples.iter().any(|s| s.abs() > 1e-4),
        "the score rendered silence, which no reference should ever match"
    );

    let actual = digest(&samples);
    if DIGESTS_PINNED_HERE {
        assert_eq!(
            actual,
            GOLDEN_DIGEST,
            "mxm-poly-06 renders differently through the player than the committed reference.\n\
             If the change was deliberate, listen to {} and update GOLDEN_DIGEST to {actual} in the \
             same commit as the change that caused it.",
            wav.display()
        );
    }
}

#[test]
fn the_score_with_the_chorus_on_still_sounds_the_same() {
    // Every mode is rendered before anything is asserted, so one run reports every digest that
    // moved rather than the first — three modes pinned from one message, not three runs.
    let mut moved = Vec::new();
    for (mode, value, expected) in CHORUS_DIGESTS {
        let name = format!("golden-poly-06-chorus-{}", value);
        let Some((samples, wav)) = render_score_with_chorus(&name, Some(*value)) else {
            eprintln!("skipping: run `cargo xtask bundle mxm-poly-06 --release`");
            return;
        };
        assert_eq!(samples.len(), GOLDEN_SAMPLES, "the score's length changed");
        let actual = digest(&samples);
        if DIGESTS_PINNED_HERE {
            assert_ne!(
                actual, GOLDEN_DIGEST,
                "chorus {mode} rendered the dry digest: the mode was not applied"
            );
        }
        if DIGESTS_PINNED_HERE && &actual != expected {
            moved.push(format!("{mode}: {actual} (listen to {})", wav.display()));
        }
    }
    assert!(
        moved.is_empty(),
        "mxm-poly-06 with the chorus on renders differently than the committed references.
         If the change was deliberate, listen, and update CHORUS_DIGESTS in the same commit as          the change that caused it:
  {}",
        moved.join("
  ")
    );
}

#[test]
fn the_golden_test_would_catch_a_change_in_the_sound() {
    let Some((dir, file)) = bundle() else {
        eprintln!("skipping: run `cargo xtask bundle mxm-poly-06 --release`");
        return;
    };
    let mut session = Session::scratch("golden-poly-06-sensitivity", vec![dir]);
    session.load(&file, PLUGIN);

    let cutoff = session
        .state()
        .param("Cutoff")
        .expect("Cutoff is a parameter")
        .clone();
    session
        .app()
        .engine_mut()
        .push_gui_event(mxm_player::events::input::Payload::ParamValue {
            param_id: cutoff.id,
            value: cutoff.min + 0.05 * (cutoff.max - cutoff.min),
        });
    score(&mut session).expect("the session advances");

    if DIGESTS_PINNED_HERE {
        assert_ne!(
            digest(&session.captured()),
            GOLDEN_DIGEST,
            "closing the filter must change the render; if it does not, the digest is not measuring \
             the audio"
        );
    }
}

/// A stable digest of the rendered samples, on their exact bit patterns. FNV-1a, as mxm-mono-01's `golden_audio`.
fn digest(samples: &[f32]) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for s in samples {
        for byte in s.to_bits().to_le_bytes() {
            hash ^= u64::from(byte);
            hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
        }
    }
    format!("{hash:016x}")
}
