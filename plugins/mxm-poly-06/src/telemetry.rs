//! The **only** channel from the audio thread to the editor.
//!
//! Atomics, written once per block, read whenever the editor happens to look. No locks, no
//! allocation, and the UI may drop as many frames as it likes — a display that made the audio
//! thread wait would be a display that could cause a dropout.
//!
//! **The meter and the developer channel's requests are `mxm_telemetry`'s**, held as `meter` and
//! `dev`, with one policy for every plugin: a peak is max-combined and reset when the UI reads it,
//! a clip latches until acknowledged (design system §5.4), and a non-finite peak latches the clip
//! and is never the level. What follows here is this plugin's own.
//!
//! # The voices are this instrument's own
//!
//! The brief's §8 asks for the one display the mono instruments could not have: **which of the six
//! voices is sounding, and how loud**. It is what makes POLY 1 and POLY 2 legible — the two modes
//! distribute notes across the cards differently, and nothing on the panel shows that — and it is
//! how a seventh key stealing a voice reads as the machine's behaviour rather than a dropout. Six
//! envelope levels and six held flags, **exact**: the values the DSP used, read once per block.

use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use mxm_poly_06_dsp::voice::VOICES;

#[derive(Debug)]
pub struct Telemetry {
    /// The output level and its clip latch: the collection's one meter policy
    /// (`mxm_telemetry::PeakMeter`).
    pub meter: mxm_telemetry::PeakMeter,
    /// The developer channel's requests of the editor, each taken once
    /// (`mxm_telemetry::DevRequests`).
    pub dev: mxm_telemetry::DevRequests,
    /// Each voice's envelope level, once per block.
    voice_level: [AtomicU32; VOICES],
    /// Each voice's key, with bit 8 set while it is held. One word so a frame reads it whole.
    voice_note: [AtomicU32; VOICES],
    /// The chorus modulator, `-1..=1`, once per block.
    chorus_lfo: AtomicU32,
    /// Published once in `activate`, because the filter curve is plotted against it and it changes
    /// only when the host reconfigures.
    sample_rate: AtomicU32,
    /// The host tempo in force, so a synced LFO rate reads its division.
    pub tempo: mxm_tempo::TempoCell,
}

impl Default for Telemetry {
    fn default() -> Self {
        Self::new()
    }
}

impl Telemetry {
    pub fn new() -> Self {
        Self {
            meter: mxm_telemetry::PeakMeter::new(),
            dev: mxm_telemetry::DevRequests::new(),
            voice_level: std::array::from_fn(|_| AtomicU32::new(0)),
            voice_note: std::array::from_fn(|_| AtomicU32::new(0)),
            chorus_lfo: AtomicU32::new(0),
            sample_rate: AtomicU32::new(48_000f32.to_bits()),
            tempo: mxm_tempo::TempoCell::new(),
        }
    }

    pub fn shared() -> Arc<Self> {
        Arc::new(Self::new())
    }

    // ---- audio thread ----

    /// Publish the six voices' levels and keys.
    pub fn publish_voices(&self, levels: &[f32; VOICES], notes: &[(u8, bool); VOICES]) {
        for i in 0..VOICES {
            self.voice_level[i].store(levels[i].to_bits(), Ordering::Relaxed);
            let word = u32::from(notes[i].0) | if notes[i].1 { 0x100 } else { 0 };
            self.voice_note[i].store(word, Ordering::Relaxed);
        }
    }

    pub fn publish_chorus_lfo(&self, value: f32) {
        self.chorus_lfo.store(value.to_bits(), Ordering::Relaxed);
    }

    pub fn publish_sample_rate(&self, rate: f32) {
        self.sample_rate.store(rate.to_bits(), Ordering::Relaxed);
    }

    // ---- editor thread ----

    /// Each voice's envelope level.
    pub fn voice_levels(&self) -> [f32; VOICES] {
        std::array::from_fn(|i| f32::from_bits(self.voice_level[i].load(Ordering::Relaxed)))
    }

    /// Each voice's key and whether it is held.
    pub fn voice_notes(&self) -> [(u8, bool); VOICES] {
        std::array::from_fn(|i| {
            let word = self.voice_note[i].load(Ordering::Relaxed);
            ((word & 0xFF) as u8, word & 0x100 != 0)
        })
    }

    pub fn chorus_lfo(&self) -> f32 {
        f32::from_bits(self.chorus_lfo.load(Ordering::Relaxed))
    }

    pub fn sample_rate(&self) -> f32 {
        f32::from_bits(self.sample_rate.load(Ordering::Relaxed))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_voices_round_trip_whole() {
        let t = Telemetry::new();
        let levels = [0.1, 0.2, 0.3, 0.4, 0.5, 0.6];
        let notes = [
            (60, true),
            (64, false),
            (67, true),
            (0, false),
            (127, true),
            (72, false),
        ];
        t.publish_voices(&levels, &notes);
        assert_eq!(t.voice_levels(), levels);
        assert_eq!(t.voice_notes(), notes);
    }
}
