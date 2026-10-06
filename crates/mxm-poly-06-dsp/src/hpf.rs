//! The global HPF: four stepped positions, and the lowest one boosts.
//!
//! `research:instruments/juno-106.md` §3.3 and `research:filters/machines/ir3109-roland.md`, *Juno-106 —
//! the bass boost*. One op-amp (IC4) with its network selected by a 4052 analogue multiplexer (IC3):
//! two control bits, four states. **After the voice sum and before the patch's VCA**, so the low
//! end of a chord is shaped once rather than six times.
//!
//! **Stepped, not continuous, and the steps are the point.** Position 0 is a low-shelf *boost*, not
//! "high-pass off"; 1 is flat; only 2 and 3 cut. A 106 sounds bigger than a 60 on the same patch
//! largely because of position 0.
//!
//! # What is derived, and what is chosen
//!
//! The Jack Board schematic gives the network's values without a traced topology for each leg, so
//! the four responses rest on an assumption and say so:
//!
//! - **The two cuts are derived** from the RC products in hand — `47 kΩ × 0.015 µF` and
//!   `47 kΩ × 0.0047 µF` — under the assumption that each cut leg is a series RC into a unity-gain
//!   inverting stage, which gives one-pole highpass corners at [`CUT_1_HZ`] and [`CUT_2_HZ`].
//!   Computed, not measured; the assumption is the weakest part.
//! - **The boost is chosen.** The published figures disagree by roughly 7 dB — "+3 dB at 70 Hz" by
//!   one account, "approximately +10 dB below 250 Hz" in Electric Druid's circuit analysis — and
//!   the 1 MΩ / 220 kΩ legs that would make a shelf cannot be placed without the topology. The
//!   research's standing instruction is *model it as a low shelf and tune by ear*; the circuit
//!   analysis is the higher tier of the two sources, so [`BOOST_DB`] and [`BOOST_CORNER_HZ`] start
//!   from it. Recorded as chosen in the crate's AGENTS.md; it is the third thing worth measuring if
//!   a unit appears.

use crate::onepole::OnePole;

/// The four slider positions, bottom to top.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Position {
    /// Position 0: a low-shelf boost.
    Boost,
    /// Position 1: flat. The neutral one, and the init patch's.
    #[default]
    Flat,
    /// Position 2: a gentle cut.
    Cut1,
    /// Position 3: a firmer cut.
    Cut2,
}

/// The low shelf's gain at DC. Electric Druid's figure; the other published account says +3 dB.
pub const BOOST_DB: f32 = 10.0;

/// Where the shelf is halfway. Chosen so the boost is fully present below about 100 Hz and gone by
/// about 1 kHz, which is what "below 250 Hz" describes.
pub const BOOST_CORNER_HZ: f32 = 250.0;

/// Position 2's corner: `1 / (2π · 47 kΩ · 0.015 µF)`.
pub const CUT_1_HZ: f32 = 225.8;

/// Position 3's corner: `1 / (2π · 47 kΩ · 0.0047 µF)`.
pub const CUT_2_HZ: f32 = 720.5;

#[derive(Debug, Clone)]
pub struct Hpf {
    position: Position,
    /// The shelf's lowpass leg. Runs in every position so switching into the boost is click-free.
    shelf: OnePole,
    /// The cut's highpass. Likewise always running, at whichever cut corner was last selected.
    cut: OnePole,
    /// `10^(BOOST_DB/20) - 1`: how much lowpassed signal the shelf adds.
    shelf_gain: f32,
    sample_rate: f32,
}

impl Default for Hpf {
    fn default() -> Self {
        Self::new()
    }
}

impl Hpf {
    pub fn new() -> Self {
        let mut hpf = Self {
            position: Position::Flat,
            shelf: OnePole::new(),
            cut: OnePole::new(),
            shelf_gain: 10f32.powf(BOOST_DB / 20.0) - 1.0,
            sample_rate: 48_000.0,
        };
        hpf.set_sample_rate(48_000.0);
        hpf
    }

    pub fn set_sample_rate(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate;
        self.shelf.set_cutoff(BOOST_CORNER_HZ, sample_rate);
        self.cut.set_cutoff(CUT_1_HZ, sample_rate);
    }

    pub fn reset(&mut self) {
        self.shelf.reset();
        self.cut.reset();
    }

    /// Control-rate: the position is a switch, recomputed at a block boundary.
    pub fn set_position(&mut self, position: Position) {
        self.position = position;
        match position {
            Position::Cut1 => self.cut.set_cutoff(CUT_1_HZ, self.sample_rate),
            Position::Cut2 => self.cut.set_cutoff(CUT_2_HZ, self.sample_rate),
            Position::Boost | Position::Flat => {}
        }
    }

    pub fn position(&self) -> Position {
        self.position
    }

    #[inline]
    pub fn process(&mut self, x: f32) -> f32 {
        // Both legs run whatever the position, so the states are warm when the switch moves.
        let low = self.shelf.lowpass(x);
        let high = self.cut.highpass(x);
        match self.position {
            Position::Boost => x + self.shelf_gain * low,
            Position::Flat => x,
            Position::Cut1 | Position::Cut2 => high,
        }
    }
}

/// The analytic magnitude of each position at `hz`, in dB, for a display.
///
/// The analogue prototype rather than the running filter: one-pole shelf and one-pole highpass.
/// Differs from the digital filter only by prewarping near Nyquist.
pub fn magnitude_db(position: Position, hz: f32) -> f32 {
    let one_pole_high = |corner: f32| (hz / corner) / (1.0 + (hz / corner).powi(2)).sqrt();
    let magnitude = match position {
        Position::Boost => {
            // x + g·low(x): the shelf's magnitude, from its complex response.
            let g = 10f32.powf(BOOST_DB / 20.0) - 1.0;
            let w = hz / BOOST_CORNER_HZ;
            let (re, im) = (1.0 + g / (1.0 + w * w), -g * w / (1.0 + w * w));
            (re * re + im * im).sqrt()
        }
        Position::Flat => 1.0,
        Position::Cut1 => one_pole_high(CUT_1_HZ),
        Position::Cut2 => one_pole_high(CUT_2_HZ),
    };
    20.0 * magnitude.max(1e-6).log10()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn level_db(position: Position, hz: f32, fs: f32) -> f32 {
        let mut hpf = Hpf::new();
        hpf.set_sample_rate(fs);
        hpf.set_position(position);
        let n = (fs * 0.5) as usize;
        let mut peak = 0.0f32;
        for i in 0..n * 2 {
            let x = (std::f32::consts::TAU * hz * i as f32 / fs).sin();
            let y = hpf.process(x);
            if i >= n {
                peak = peak.max(y.abs());
            }
        }
        20.0 * peak.log10()
    }

    #[test]
    fn position_zero_boosts_the_bass_and_leaves_the_top_alone() {
        for fs in [44_100.0f32, 48_000.0, 96_000.0] {
            let low = level_db(Position::Boost, 40.0, fs);
            // 6 kHz rather than 8: at 48 kHz an 8 kHz sine lands on six fixed phases per cycle
            // and its sampled peak is sin(60°), which reads as −1.25 dB of attenuation that is
            // the measurement's and not the filter's.
            let high = level_db(Position::Boost, 6_000.0, fs);
            assert!(
                (low - BOOST_DB).abs() < 0.5,
                "at {fs}: {low:.2} dB at 40 Hz"
            );
            assert!(high.abs() < 0.2, "at {fs}: {high:.2} dB at 6 kHz");
        }
    }

    #[test]
    fn position_one_is_flat() {
        for hz in [30.0f32, 300.0, 3_000.0] {
            let db = level_db(Position::Flat, hz, 48_000.0);
            assert!(db.abs() < 0.01, "{hz} Hz: {db} dB");
        }
    }

    #[test]
    fn only_the_top_two_positions_cut_and_the_top_cuts_more() {
        let fs = 48_000.0;
        let at_60 = |p| level_db(p, 60.0, fs);
        let (boost, flat, cut1, cut2) = (
            at_60(Position::Boost),
            at_60(Position::Flat),
            at_60(Position::Cut1),
            at_60(Position::Cut2),
        );
        assert!(
            boost > flat && flat > cut1 && cut1 > cut2,
            "{boost} {flat} {cut1} {cut2}"
        );
        assert!(cut1 < -8.0, "position 2 at 60 Hz: {cut1:.1} dB");
        assert!(cut2 < -18.0, "position 3 at 60 Hz: {cut2:.1} dB");
    }

    #[test]
    fn the_cuts_are_three_db_down_at_their_derived_corners() {
        let fs = 96_000.0;
        let c1 = level_db(Position::Cut1, CUT_1_HZ, fs);
        let c2 = level_db(Position::Cut2, CUT_2_HZ, fs);
        assert!((c1 + 3.01).abs() < 0.3, "cut 1 at its corner: {c1:.2} dB");
        assert!((c2 + 3.01).abs() < 0.3, "cut 2 at its corner: {c2:.2} dB");
    }

    #[test]
    fn the_display_curve_agrees_with_the_running_filter() {
        for position in [
            Position::Boost,
            Position::Flat,
            Position::Cut1,
            Position::Cut2,
        ] {
            for hz in [40.0f32, 200.0, 1_000.0, 5_000.0] {
                let running = level_db(position, hz, 96_000.0);
                let drawn = magnitude_db(position, hz);
                assert!(
                    (running - drawn).abs() < 0.5,
                    "{position:?} at {hz} Hz: running {running:.2}, drawn {drawn:.2}"
                );
            }
        }
    }

    #[test]
    fn silence_stays_exactly_zero_in_every_position() {
        for position in [
            Position::Boost,
            Position::Flat,
            Position::Cut1,
            Position::Cut2,
        ] {
            let mut hpf = Hpf::new();
            hpf.set_position(position);
            hpf.process(1.0);
            for _ in 0..200_000 {
                hpf.process(0.0);
            }
            assert_eq!(hpf.process(0.0), 0.0, "{position:?}");
        }
    }
}
