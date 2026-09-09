#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Grid {
    Off,
    Quarter,
    Eighth,
    #[default]
    Sixteenth,
    ThirtySecond,
}

impl Grid {
    pub fn samples(self, samples_per_beat: f64) -> Option<f64> {
        let divisor = match self {
            Grid::Off => return None,
            Grid::Quarter => 1.0,
            Grid::Eighth => 2.0,
            Grid::Sixteenth => 4.0,
            Grid::ThirtySecond => 8.0,
        };
        Some(samples_per_beat / divisor)
    }

    pub fn label(self) -> &'static str {
        match self {
            Grid::Off => "OFF",
            Grid::Quarter => "1/4",
            Grid::Eighth => "1/8",
            Grid::Sixteenth => "1/16",
            Grid::ThirtySecond => "1/32",
        }
    }
}

pub fn quantize(pos: u64, grid: f64) -> u64 {
    if grid <= 0.0 {
        return pos;
    }
    ((pos as f64 / grid).round() * grid).round() as u64
}

/// Quantize a position inside a loop.
///
/// A note played just before the loop end snaps *forward* past the end. Wrapping
/// it to the start is what makes it audible on the next pass; without the wrap it
/// would sit beyond the loop and never play again. The note has already been
/// heard live at the moment it was struck, so nothing is lost.
pub fn quantize_in_loop(pos: u64, grid: f64, loop_len: u64) -> u64 {
    let q = quantize(pos, grid);
    if loop_len == 0 {
        q
    } else {
        q % loop_len
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SPB: f64 = 24_000.0; // 120 BPM at 48 kHz

    #[test]
    fn grid_off_yields_no_grid() {
        assert!(Grid::Off.samples(SPB).is_none());
    }

    #[test]
    fn a_sixteenth_is_a_quarter_of_a_beat() {
        assert_eq!(Grid::Sixteenth.samples(SPB), Some(6_000.0));
    }

    #[test]
    fn quantize_snaps_to_the_nearest_line() {
        assert_eq!(quantize(5_000, 6_000.0), 6_000);
        assert_eq!(quantize(2_000, 6_000.0), 0);
        assert_eq!(quantize(9_000, 6_000.0), 12_000); // exactly halfway rounds up
    }

    #[test]
    fn quantize_with_a_zero_grid_is_the_identity() {
        assert_eq!(quantize(1_234, 0.0), 1_234);
    }

    #[test]
    fn a_note_played_just_before_the_loop_end_wraps_to_the_start() {
        // The defining edge case: quantizing forward past the loop end must wrap
        // to zero, not land beyond the loop and never play.
        let loop_len = 48_000;
        assert_eq!(quantize_in_loop(47_500, 6_000.0, loop_len), 0);
    }

    #[test]
    fn quantize_in_loop_leaves_interior_positions_alone() {
        assert_eq!(quantize_in_loop(5_000, 6_000.0, 48_000), 6_000);
    }
}
