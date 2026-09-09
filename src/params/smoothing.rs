/// One-pole smoothing so that a jumping MIDI value or a yanked whammy bar does
/// not produce zipper noise.
#[derive(Clone, Copy, Debug)]
pub struct Smoother {
    current: f32,
    target: f32,
    coeff: f32,
}

impl Smoother {
    pub fn new(sample_rate: f32, time_ms: f32) -> Self {
        let samples = (time_ms / 1000.0 * sample_rate).max(1.0);
        Smoother {
            current: 0.0,
            target: 0.0,
            coeff: 1.0 - (-1.0 / samples).exp(),
        }
    }

    pub fn set_target(&mut self, v: f32) {
        self.target = v;
    }

    /// Jump instantly. Used when a voice starts, where smoothing would be a
    /// slide up from the previous note's value.
    pub fn snap(&mut self, v: f32) {
        self.current = v;
        self.target = v;
    }

    pub fn next(&mut self) -> f32 {
        self.current += (self.target - self.current) * self.coeff;
        self.current
    }

    pub fn current(&self) -> f32 {
        self.current
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snap_moves_immediately() {
        let mut s = Smoother::new(48_000.0, 5.0);
        s.snap(0.75);
        assert!((s.next() - 0.75).abs() < 1e-6);
    }

    #[test]
    fn it_approaches_the_target_without_overshooting() {
        let mut s = Smoother::new(48_000.0, 5.0);
        s.snap(0.0);
        s.set_target(1.0);
        let mut previous = 0.0;
        for _ in 0..48_000 {
            let v = s.next();
            assert!(v >= previous - 1e-9, "went backwards: {previous} then {v}");
            assert!(v <= 1.0 + 1e-6, "overshot: {v}");
            previous = v;
        }
        assert!(previous > 0.99, "did not converge, reached only {previous}");
    }

    #[test]
    fn one_time_constant_covers_most_of_the_distance() {
        let mut s = Smoother::new(48_000.0, 5.0);
        s.snap(0.0);
        s.set_target(1.0);
        for _ in 0..240 {
            s.next(); // 5 ms at 48 kHz
        }
        let v = s.current();
        assert!(
            v > 0.5 && v < 0.75,
            "expected roughly one time constant, got {v}"
        );
    }
}
