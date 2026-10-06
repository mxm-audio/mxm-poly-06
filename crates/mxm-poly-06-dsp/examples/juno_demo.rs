//! Does it sound like a polysynth? Renders a short chord progression to a stereo WAV.
//!
//! Named for the machine, which is the collection's rule for examples: cargo writes every example in
//! the workspace to one flat `target/*/examples/` directory, so two crates sharing a name share an
//! output file — and cargo then runs whichever won the race, without a word. See
//! `docs/known-issues.md`.
//!
//! ```text
//! cargo run -p mxm-poly-06-dsp --release --example juno_demo
//! ```
//!
//! Four passes over the same four chords, each adding the thing the previous one lacked, so what
//! each part of the machine contributes can be heard by comparison:
//!
//! 1. saw, chorus off, HPF flat — the six DCOs, perfectly in tune with each other, which is thin
//! 2. the same with the chorus on I — the width the machine gets from decorrelating them
//! 3. chorus II, HPF at position 0 — the bass boost
//! 4. pulse with LFO width, chorus I+II, the envelope on the filter — the whole thing

/// Writes a listening demo, applying this collection's demo headroom law **at the call site**.
///
/// `mxm_audio_file` encodes what it is given and applies no gain — normalisation is a judgement
/// about the material and the file crate carries no policy. The law here is the one the
/// six hand-written writers all applied internally: leave 2 % of headroom, and scale down further if
/// the material is over full scale.
fn write_demo(path: &str, interleaved: &[f32], channels: u16, rate: u32) {
    let peak = mxm_measure::level::peak(interleaved)
        .expect("a rendered demo is finite; a NaN here is a DSP defect, not a level");
    let gain = if peak > 1.0 { 0.98 / peak } else { 0.98 };
    let scaled: Vec<f32> = interleaved.iter().map(|s| s * gain).collect();
    mxm_audio_file::write(
        path,
        &scaled,
        channels,
        rate,
        mxm_audio_file::Target::Wav(mxm_audio_file::Bits::Sixteen),
    )
    .expect("the demo is written");
}

/// **Where this demo's channel count and sample rate are decided — once, for `main` and for the
/// test below.** Both call this, so a change to either constant changes both paths and the test's
/// literal expectations catch it. With the two supplied separately at each site, a `main` passing
/// the wrong channel count left the test perfectly green.
const DEMO_CHANNELS: u16 = 2;

fn write_demo_file(path: &str, interleaved: &[f32]) {
    write_demo(path, interleaved, DEMO_CHANNELS, FS as u32);
}

use mxm_poly_06_dsp::chorus::Mode;
use mxm_poly_06_dsp::hpf::Position;
use mxm_poly_06_dsp::poly::{Assign, Key, Patch, Synth};
use mxm_poly_06_dsp::routing::{Routing, source, target};

const FS: f32 = 48_000.0;

fn main() {
    let mut synth = Synth::new();
    synth.set_sample_rate(FS);

    let chords: [&[u8]; 4] = [
        &[48, 55, 60, 64],     // C
        &[45, 52, 57, 60, 64], // Am, five notes
        &[41, 48, 53, 57, 60], // F
        &[43, 50, 55, 59, 62], // G
    ];

    let mut passes: Vec<Patch> = Vec::new();
    let base = Patch::default();
    passes.push(Patch {
        chorus: Mode::Off,
        hpf: Position::Flat,
        ..base
    });
    passes.push(Patch {
        chorus: Mode::I,
        ..base
    });
    passes.push(Patch {
        chorus: Mode::II,
        hpf: Position::Boost,
        ..base
    });
    let mut full = Patch {
        chorus: Mode::Both,
        hpf: Position::Boost,
        ..base
    };
    full.voice.mix.saw = 0.0;
    full.voice.mix.pulse = 1.0;
    full.voice.mix.sub = 0.5;
    full.voice.cutoff_hz = 900.0;
    full.voice.resonance = 0.35;
    full.voice.attack_s = 0.15;
    full.voice.decay_s = 0.8;
    full.voice.sustain = 0.4;
    full.voice.release_s = 0.6;
    full.lfo_rate_hz = 0.7;
    passes.push(full);
    // The fourth pass's modulation, as routes: PWM from the LFO, and the envelope into cutoff.
    let routings = [
        Routing::new(),
        Routing::new(),
        Routing::new(),
        Routing::from_pairs(&[
            (target::PULSE_WIDTH, source::LFO, 0.6),
            (target::CUTOFF, source::ENVELOPE, 0.45),
        ]),
    ];

    let mut out: Vec<f32> = Vec::new();
    let mut peak = 0.0f32;
    let hold = (FS * 1.6) as usize;
    let gap = (FS * 0.4) as usize;

    for (i, patch) in passes.iter().enumerate() {
        eprintln!(
            "pass {}: chorus {:?}, hpf {:?}",
            i + 1,
            patch.chorus,
            patch.hpf
        );
        synth.prepare(patch);
        synth.set_topology(&routings[i]);
        for chord in chords {
            for &note in chord {
                synth.note_on(Key { channel: 0, note }, None, Assign::Poly1, 0.8);
            }
            for _ in 0..hold {
                let (l, r) = synth.process(patch);
                peak = peak.max(l.abs()).max(r.abs());
                out.push(l);
                out.push(r);
            }
            for &note in chord {
                synth.note_off(Key { channel: 0, note }, None);
            }
            for _ in 0..gap {
                let (l, r) = synth.process(patch);
                peak = peak.max(l.abs()).max(r.abs());
                out.push(l);
                out.push(r);
            }
        }
    }
    // Let the last release and the chorus drain.
    for _ in 0..(FS * 1.5) as usize {
        let (l, r) = synth.process(&passes[3]);
        out.push(l);
        out.push(r);
    }

    let path = "mxm-poly-06-demo.wav";
    write_demo_file(path, &out);
    eprintln!(
        "wrote {path}: {:.1} s, peak {peak:.3}{}",
        out.len() as f32 / 2.0 / FS,
        if peak > 1.0 { " (normalised down)" } else { "" }
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The demo's own write path, exercised through the same wrapper `main` uses.
    ///
    /// The shared encoder is proved in `mxm-measure` against fixed header and payload bytes. What
    /// that cannot see is *this* file later writing the wrong channel count or rate, so the
    /// expectations here are **literals** — the facts about this instrument — rather than the
    /// constants under test.
    #[test]
    fn the_demo_write_path_produces_a_playable_file() {
        let frames = 256;
        let samples: Vec<f32> = (0..frames * DEMO_CHANNELS as usize)
            .map(|i| {
                let t = i as f32 / 48_000 as f32;
                // Past full scale, so the headroom branch is taken rather than skipped.
                1.6 * (std::f32::consts::TAU * 220.0 * t).sin()
            })
            .collect();

        let mut path = std::env::temp_dir();
        path.push(format!("juno-demo-demo-{}.wav", std::process::id()));
        write_demo_file(path.to_str().expect("a utf-8 path"), &samples);

        let read = mxm_audio_file_decode::decode_file(
            &path,
            &mxm_audio_file_decode::Limits::new(
                usize::MAX,
                mxm_audio_file_decode::AtLimit::Refuse,
                mxm_audio_file_decode::Keep::AllUpTo(2),
            ),
        )
        .expect("the demo file parses");
        assert_eq!(read.channels, 2, "the demo wrote the wrong channel count");
        assert_eq!(
            read.sample_rate, 48_000,
            "the demo wrote the wrong sample rate"
        );
        assert_eq!(read.frames(), frames, "the demo dropped or invented frames");

        // The headroom law, asserted rather than assumed: a source at 1.6 comes back just under
        // full scale, not clipped to it and not left loud.
        let peak = mxm_measure::level::peak(&read.interleaved).expect("a finite file");
        assert!(
            (0.97..=0.985).contains(&peak),
            "the 0.98 headroom law did not run: peak {peak}"
        );
        std::fs::remove_file(&path).ok();
    }

    /// **The whole production path, `main` included.** This is what a writer test cannot otherwise
    /// reach: the render itself, the buffer `main` chooses, and the channel count and rate it hands
    /// over. An empty or truncated render fails here and nowhere else.
    ///
    /// `#[ignore]`d because it renders the demo in full, which is tens of seconds of audio; run it
    /// with `cargo test --all-targets -- --ignored` when the demo or its write path changes.
    #[test]
    #[ignore = "renders the whole demo; run with --ignored"]
    fn the_whole_demo_renders_and_writes_a_playable_file() {
        main();
        let read = mxm_audio_file_decode::decode_file(
            "mxm-poly-06-demo.wav",
            &mxm_audio_file_decode::Limits::new(
                usize::MAX,
                mxm_audio_file_decode::AtLimit::Refuse,
                mxm_audio_file_decode::Keep::AllUpTo(2),
            ),
        )
        .expect("the demo file parses");
        assert_eq!(read.channels, 2, "the demo wrote the wrong channel count");
        assert_eq!(
            read.sample_rate, 48000,
            "the demo wrote the wrong sample rate"
        );
        assert!(
            read.frames() > 48000,
            "the demo rendered under a second of audio"
        );
        let peak = mxm_measure::level::peak(&read.interleaved).expect("a finite render");
        assert!(peak > 0.1, "the demo rendered near-silence: peak {peak}");

        // `main` writes into the working directory, which under `cargo test` is the crate root.
        // Leaving it there drops an untracked WAV into the tree every time this runs.
        std::fs::remove_file("mxm-poly-06-demo.wav").ok();
    }
}
