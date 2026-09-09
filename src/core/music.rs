/// Semitone offsets from the root for each supported scale.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Scale {
    Chromatic,
    Major,
    NaturalMinor,
    HarmonicMinor,
    Dorian,
    Phrygian,
    Mixolydian,
    PentatonicMinor,
    Blues,
}

impl Scale {
    pub fn degrees(self) -> &'static [u8] {
        match self {
            Scale::Chromatic => &[0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11],
            Scale::Major => &[0, 2, 4, 5, 7, 9, 11],
            Scale::NaturalMinor => &[0, 2, 3, 5, 7, 8, 10],
            Scale::HarmonicMinor => &[0, 2, 3, 5, 7, 8, 11],
            Scale::Dorian => &[0, 2, 3, 5, 7, 9, 10],
            Scale::Phrygian => &[0, 1, 3, 5, 7, 8, 10],
            Scale::Mixolydian => &[0, 2, 4, 5, 7, 9, 10],
            Scale::PentatonicMinor => &[0, 3, 5, 7, 10],
            Scale::Blues => &[0, 3, 5, 6, 7, 10],
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Scale::Chromatic => "Chromatic",
            Scale::Major => "Major",
            Scale::NaturalMinor => "Minor",
            Scale::HarmonicMinor => "Harmonic Minor",
            Scale::Dorian => "Dorian",
            Scale::Phrygian => "Phrygian",
            Scale::Mixolydian => "Mixolydian",
            Scale::PentatonicMinor => "Pentatonic Minor",
            Scale::Blues => "Blues",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Key {
    /// Root pitch class, 0 = C.
    pub root: u8,
    pub scale: Scale,
}

impl Default for Key {
    fn default() -> Self {
        Key {
            root: 9,
            scale: Scale::NaturalMinor,
        } // A minor
    }
}

impl Key {
    /// Move a note to the nearest scale degree at or below it.
    ///
    /// Snapping *down* rather than to-nearest is deliberate: a chromatic run
    /// played upwards then stays monotonic, where nearest-neighbour snapping
    /// makes it stutter back and forth.
    pub fn snap(&self, note: u8) -> u8 {
        let degrees = self.scale.degrees();
        let pitch_class = (note as i16 - self.root as i16).rem_euclid(12) as u8;
        let mut lowered = 0u8;
        for offset in 0..12u8 {
            let candidate = pitch_class.wrapping_sub(offset);
            if candidate <= pitch_class && degrees.contains(&candidate) {
                lowered = offset;
                break;
            }
        }
        note.saturating_sub(lowered)
    }
}

/// Equal temperament, A4 = 440 Hz = note 69. Takes `f32` so pitch bend and the
/// whammy bar can express positions between semitones.
pub fn note_to_freq(note: f32) -> f32 {
    440.0 * ((note - 69.0) / 12.0).exp2()
}

/// Middle C is note 60 and is called C4, so octave 4 semitone 0 is 60.
/// Clamps rather than wrapping: holding octave-up must never produce a bass note.
pub fn note_for(octave: i8, semitone: i8) -> u8 {
    let n = 12 * (octave as i32 + 1) + semitone as i32;
    n.clamp(0, 127) as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a4_is_440_hz() {
        assert!((note_to_freq(69.0) - 440.0).abs() < 1e-3);
    }

    #[test]
    fn an_octave_down_halves_the_frequency() {
        assert!((note_to_freq(57.0) - 220.0).abs() < 1e-3);
    }

    #[test]
    fn fractional_notes_bend_between_semitones() {
        // Used by pitch bend and the whammy bar later; must not round to an integer note.
        let quarter_tone = note_to_freq(69.5);
        assert!(quarter_tone > 440.0 && quarter_tone < note_to_freq(70.0));
    }

    #[test]
    fn middle_c_is_note_60() {
        assert_eq!(note_for(4, 0), 60);
    }

    #[test]
    fn note_for_clamps_instead_of_wrapping() {
        // A user holding octave-up must never make the synth play a wrapped low note.
        assert_eq!(note_for(20, 11), 127);
        assert_eq!(note_for(-20, 0), 0);
    }

    #[test]
    fn snapping_leaves_in_key_notes_alone() {
        let key = Key {
            root: 0,
            scale: Scale::Major,
        }; // C major
        assert_eq!(key.snap(60), 60); // C
        assert_eq!(key.snap(62), 62); // D
        assert_eq!(key.snap(64), 64); // E
    }

    #[test]
    fn snapping_pulls_out_of_key_notes_down_to_the_nearest_degree() {
        let key = Key {
            root: 0,
            scale: Scale::Major,
        };
        assert_eq!(key.snap(61), 60); // C# -> C
        assert_eq!(key.snap(66), 65); // F# -> F
    }

    #[test]
    fn snapping_respects_the_root() {
        let key = Key {
            root: 2,
            scale: Scale::Major,
        }; // D major has F#
        assert_eq!(key.snap(66), 66); // F# is in key, must be left alone
        assert_eq!(key.snap(65), 64); // F natural is not; snaps down to E
    }

    #[test]
    fn chromatic_snapping_is_the_identity() {
        let key = Key {
            root: 7,
            scale: Scale::Chromatic,
        };
        for n in 0..=127u8 {
            assert_eq!(key.snap(n), n);
        }
    }
}
