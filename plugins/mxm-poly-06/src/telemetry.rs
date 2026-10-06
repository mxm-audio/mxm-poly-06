//! The **only** channel from the audio thread to the editor.
//!
//! Atomics, written once per block, read whenever the editor happens to look. No locks, no
//! allocation, and the UI may drop as many frames as it likes — a display that made the audio
//! thread wait would be a display that could cause a dropout.
//!
//! Two rules carried from the other two instruments' `telemetry.rs`, both of which exist because
//! the obvious implementation loses information:
//!
//! - **A peak is max-combined and reset when the UI reads it.** Overwriting each block means a
//!   transient that landed between two frames is simply gone; combining means the value is always
//!   *loudest since you last looked*.
//! - **A clip latches until acknowledged.** A meter that quietly forgets it clipped is worse than
//!   no meter, and design system §5.4 requires the indication to persist.
//!
//! # The voices are this instrument's own
//!
//! The brief's §8 asks for the one display the mono instruments could not have: **which of the six
//! voices is sounding, and how loud**. It is what makes POLY 1 and POLY 2 legible — the two modes
//! distribute notes across the cards differently, and nothing on the panel shows that — and it is
//! how a seventh key stealing a voice reads as the machine's behaviour rather than a dropout. Six
//! envelope levels and six held flags, **exact**: the values the DSP used, read once per block.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU32, Ordering};

use mxm_poly_06_dsp::voice::VOICES;

#[derive(Debug)]
pub struct Telemetry {
    /// Peak of the samples produced, max-combined, reset on read.
    peak: AtomicU32,
    /// Sticky: set when a sample reaches full scale, cleared only by the user.
    clipped: AtomicBool,
    /// Each voice's envelope level, once per block.
    voice_level: [AtomicU32; VOICES],
    /// Each voice's key, with bit 8 set while it is held. One word so a frame reads it whole.
    voice_note: [AtomicU32; VOICES],
    /// The chorus modulator, `-1..=1`, once per block.
    chorus_lfo: AtomicU32,
    /// Published once in `activate`, because the filter curve is plotted against it and it changes
    /// only when the host reconfigures.
    sample_rate: AtomicU32,
    /// The developer channel's requests of the editor: a view to show, and whether the expander
    /// is open. `NO_REQUEST` when nothing is asked. See `plugins/AGENTS.md`.
    dev_view: AtomicU8,
    dev_disclosure: AtomicU8,
    /// The developer channel's request to open or close the preset browser, or `NO_REQUEST`.
    dev_browser: AtomicU8,
    /// The developer channel's request to show a theme, by index, or `NO_REQUEST`. Theme is
    /// interface state, so this reaches the editor and nothing else; the DSP never sees it.
    dev_theme: AtomicU8,
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
            peak: AtomicU32::new(0),
            clipped: AtomicBool::new(false),
            voice_level: std::array::from_fn(|_| AtomicU32::new(0)),
            voice_note: std::array::from_fn(|_| AtomicU32::new(0)),
            chorus_lfo: AtomicU32::new(0),
            sample_rate: AtomicU32::new(48_000f32.to_bits()),
            dev_view: AtomicU8::new(u8::MAX),
            dev_disclosure: AtomicU8::new(u8::MAX),
            dev_browser: AtomicU8::new(u8::MAX),
            dev_theme: AtomicU8::new(u8::MAX),
            tempo: mxm_tempo::TempoCell::new(),
        }
    }

    pub fn shared() -> Arc<Self> {
        Arc::new(Self::new())
    }

    // ---- audio thread ----

    /// Publish a block's peak. **Combined, not overwritten**: see the module doc.
    pub fn publish_peak(&self, peak: f32) {
        let mut current = self.peak.load(Ordering::Relaxed);
        loop {
            let combined = f32::from_bits(current).max(peak);
            match self.peak.compare_exchange_weak(
                current,
                combined.to_bits(),
                Ordering::Relaxed,
                Ordering::Relaxed,
            ) {
                Ok(_) => break,
                Err(seen) => current = seen,
            }
        }
        if peak >= 1.0 {
            self.clipped.store(true, Ordering::Relaxed);
        }
    }

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

    /// The loudest sample since this was last called, **and resets**.
    pub fn take_peak(&self) -> f32 {
        f32::from_bits(self.peak.swap(0, Ordering::Relaxed))
    }

    pub fn clipped(&self) -> bool {
        self.clipped.load(Ordering::Relaxed)
    }

    /// Acknowledge the clip indication. The user's act, never a timeout.
    pub fn clear_clip(&self) {
        self.clipped.store(false, Ordering::Relaxed);
    }

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
    fn a_peak_is_combined_and_reset_on_read() {
        let t = Telemetry::new();
        t.publish_peak(0.4);
        t.publish_peak(0.9);
        t.publish_peak(0.2);
        assert_eq!(t.take_peak(), 0.9, "the loudest of the three, not the last");
        assert_eq!(t.take_peak(), 0.0, "and reading resets it");
    }

    #[test]
    fn a_clip_latches_until_acknowledged() {
        let t = Telemetry::new();
        t.publish_peak(1.0);
        for _ in 0..100 {
            t.publish_peak(0.1);
        }
        assert!(t.clipped(), "a meter that forgets is worse than no meter");
        t.clear_clip();
        assert!(!t.clipped());
    }

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

/// Nothing requested on a developer-channel slot.
const NO_REQUEST: u8 = u8::MAX;

/// The developer channel's requests of the editor — mxm-kit's `docs/plugin-conventions.md`, *A
/// developer channel in every editor*. Each is taken once; the DSP reads nothing.
impl Telemetry {
    /// Developer category address (0–5), or Parameters (127); never a derived tab index.
    pub fn request_view(&self, view: u8) {
        self.dev_view
            .store(view.min(NO_REQUEST - 1), Ordering::Relaxed);
    }

    /// The developer channel asks the editor to open or close the preset browser.
    pub fn request_browser(&self, open: bool) {
        self.dev_browser.store(u8::from(open), Ordering::Relaxed);
    }

    /// Whether the developer channel asked the browser open or closed since the editor last
    /// looked, if it did.
    pub fn take_browser_request(&self) -> Option<bool> {
        match self.dev_browser.swap(NO_REQUEST, Ordering::Relaxed) {
            NO_REQUEST => None,
            open => Some(open != 0),
        }
    }

    /// The developer channel asks the editor for a theme, by index — 0 light, 1 dark, 2 system,
    /// as `mxm_ui::theme::from_index` reads it.
    pub fn request_theme(&self, theme: u8) {
        self.dev_theme
            .store(theme.min(NO_REQUEST - 1), Ordering::Relaxed);
    }

    /// The theme the developer channel asked for since the editor last looked, if any.
    pub fn take_theme_request(&self) -> Option<u8> {
        match self.dev_theme.swap(NO_REQUEST, Ordering::Relaxed) {
            NO_REQUEST => None,
            theme => Some(theme),
        }
    }

    /// The developer channel asks the editor to open or close its expander.
    pub fn request_disclosure(&self, open: bool) {
        self.dev_disclosure.store(u8::from(open), Ordering::Relaxed);
    }

    /// The view the developer channel asked for since the editor last looked, if any.
    pub fn take_view_request(&self) -> Option<usize> {
        match self.dev_view.swap(NO_REQUEST, Ordering::Relaxed) {
            NO_REQUEST => None,
            view => Some(usize::from(view)),
        }
    }

    /// Whether the developer channel asked the expander open or closed since the editor last
    /// looked, if it did.
    pub fn take_disclosure_request(&self) -> Option<bool> {
        match self.dev_disclosure.swap(NO_REQUEST, Ordering::Relaxed) {
            NO_REQUEST => None,
            open => Some(open != 0),
        }
    }
}

#[cfg(test)]
mod developer_channel_tests {
    use super::*;

    #[test]
    fn a_developer_request_is_taken_once() {
        let t = Telemetry::new();
        assert_eq!(
            t.take_view_request(),
            None,
            "nothing asked on a fresh instance"
        );
        t.request_view(1);
        assert_eq!(t.take_view_request(), Some(1));
        assert_eq!(t.take_view_request(), None, "and taking it clears it");
        t.request_disclosure(true);
        assert_eq!(t.take_disclosure_request(), Some(true));
        t.request_browser(true);
        assert_eq!(t.take_browser_request(), Some(true));
        assert_eq!(t.take_browser_request(), None, "taken once");
        assert_eq!(t.take_disclosure_request(), None);
    }
}
