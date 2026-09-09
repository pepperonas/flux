/// The musical clock. The audio thread owns the only instance; the interface
/// reads a copy of its position from an atomic. Position is in samples and is
/// never derived from wall time, so it cannot drift.
#[derive(Clone, Copy, Debug)]
pub struct Transport {
    pub sample_rate: f32,
    pub bpm: f32,
    pub sample_pos: u64,
    pub playing: bool,
    /// Master loop length in samples. Zero means no loop has been recorded yet.
    pub loop_len: u64,
}

impl Default for Transport {
    fn default() -> Self {
        Transport {
            sample_rate: 48_000.0,
            bpm: 120.0,
            sample_pos: 0,
            playing: true,
            loop_len: 0,
        }
    }
}

/// FLUX is 4/4 throughout milestone M1. Other metres are a later design.
pub const BEATS_PER_BAR: u32 = 4;

impl Transport {
    pub fn samples_per_beat(&self) -> f64 {
        60.0 / self.bpm as f64 * self.sample_rate as f64
    }

    pub fn samples_per_bar(&self) -> f64 {
        self.samples_per_beat() * BEATS_PER_BAR as f64
    }

    pub fn pos_in_loop(&self) -> u64 {
        if self.loop_len == 0 {
            self.sample_pos
        } else {
            self.sample_pos % self.loop_len
        }
    }

    /// The first bar line at or after `from`. "At or after" rather than "after"
    /// so that arming exactly on the line starts immediately.
    pub fn boundary_at_or_after(&self, from: u64) -> u64 {
        let spb = self.samples_per_bar();
        let bars = (from as f64 / spb).ceil();
        (bars * spb).round() as u64
    }

    pub fn advance(&mut self, frames: u64) {
        if self.playing {
            self.sample_pos += frames;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t() -> Transport {
        Transport {
            sample_rate: 48_000.0,
            bpm: 120.0,
            ..Default::default()
        }
    }

    #[test]
    fn at_120_bpm_a_beat_is_half_a_second() {
        assert!((t().samples_per_beat() - 24_000.0).abs() < 1e-6);
    }

    #[test]
    fn a_bar_is_four_beats() {
        assert!((t().samples_per_bar() - 96_000.0).abs() < 1e-6);
    }

    #[test]
    fn a_position_exactly_on_a_bar_line_is_already_a_boundary() {
        // Arming exactly on the line must start now, not wait a whole bar.
        assert_eq!(t().boundary_at_or_after(96_000), 96_000);
        assert_eq!(t().boundary_at_or_after(0), 0);
    }

    #[test]
    fn a_position_inside_a_bar_advances_to_the_next_line() {
        assert_eq!(t().boundary_at_or_after(1), 96_000);
        assert_eq!(t().boundary_at_or_after(95_999), 96_000);
    }

    #[test]
    fn position_in_loop_wraps() {
        let mut tr = t();
        tr.loop_len = 96_000;
        tr.sample_pos = 96_010;
        assert_eq!(tr.pos_in_loop(), 10);
    }

    #[test]
    fn position_in_loop_without_a_loop_is_the_raw_position() {
        let mut tr = t();
        tr.loop_len = 0;
        tr.sample_pos = 12_345;
        assert_eq!(tr.pos_in_loop(), 12_345);
    }

    #[test]
    fn a_tempo_that_is_not_a_whole_number_of_samples_still_advances_exactly() {
        // 140 BPM at 44100 is 18900 samples per beat, but 137 is not integral.
        // Position must stay in samples, never accumulate float drift.
        let mut tr = Transport {
            sample_rate: 44_100.0,
            bpm: 137.0,
            ..Default::default()
        };
        for _ in 0..1000 {
            tr.advance(512);
        }
        assert_eq!(tr.sample_pos, 512_000);
    }
}
