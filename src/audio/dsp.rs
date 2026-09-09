use std::f32::consts::PI;

/// Filters and reverb tails decay towards zero and end up in denormal floats,
/// which are catastrophically slow on some processors. Every recursive piece of
/// state passes through here. It is cheap and easy to forget, so it is a rule
/// rather than a habit.
#[inline]
pub fn flush_denormal(x: f32) -> f32 {
    if x.abs() < 1e-30 {
        0.0
    } else {
        x
    }
}

/// Polynomial band-limited step.
///
/// A naive saw or square jumps between samples, and that jump contains
/// frequencies above half the sample rate which fold back down as aliasing. This
/// correction rounds the jump over one sample. It is about twenty lines of
/// arithmetic and it is the difference between the first keypress being
/// convincing or sounding cheap.
#[inline]
pub fn poly_blep(t: f32, dt: f32) -> f32 {
    if t < dt {
        let t = t / dt;
        2.0 * t - t * t - 1.0
    } else if t > 1.0 - dt {
        let t = (t - 1.0) / dt;
        t * t + 2.0 * t + 1.0
    } else {
        0.0
    }
}

/// Same correction as `poly_blep`, expressed as the signed distance to an
/// edge rather than a phase wrapped into [0, 1).
///
/// Used for the square wave's second discontinuity (at phase 0.5) so the
/// calculation never has to add 0.5 to a phase close to 0.5, which can round
/// to exactly 1.0 in f32 and land on the wrong side of the edge.
#[inline]
fn edge_blep(delta: f32, dt: f32) -> f32 {
    if (0.0..dt).contains(&delta) {
        let u = delta / dt;
        2.0 * u - u * u - 1.0
    } else if (-dt..0.0).contains(&delta) {
        let u = delta / dt;
        u * u + 2.0 * u + 1.0
    } else {
        0.0
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Waveform {
    Sine,
    Triangle,
    #[default]
    Saw,
    Square,
}

impl Waveform {
    pub fn from_index(v: f32) -> Waveform {
        match v.round() as i32 {
            0 => Waveform::Sine,
            1 => Waveform::Triangle,
            3 => Waveform::Square,
            _ => Waveform::Saw,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Waveform::Sine => "SINE",
            Waveform::Triangle => "TRI",
            Waveform::Saw => "SAW",
            Waveform::Square => "SQUARE",
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Osc {
    phase: f32,
}

impl Osc {
    pub fn reset(&mut self) {
        self.phase = 0.0;
    }

    pub fn tick(&mut self, freq: f32, sample_rate: f32, wave: Waveform) -> f32 {
        let dt = (freq / sample_rate).clamp(0.0, 0.49);
        let t = self.phase;
        let out = match wave {
            Waveform::Sine => (t * 2.0 * PI).sin(),
            Waveform::Triangle => 4.0 * (t - 0.5).abs() - 1.0,
            Waveform::Saw => 2.0 * t - 1.0 - poly_blep(t, dt),
            Waveform::Square => {
                let raw = if t < 0.5 { 1.0 } else { -1.0 };
                // The second edge sits at t = 0.5. Wrapping `t + 0.5` into
                // [0, 1) and reusing `poly_blep` there looks natural, but for
                // `t` a few ULPs below 0.5 the sum rounds to exactly 1.0 in
                // f32 (a round-to-even tie at the 1.0 boundary), which then
                // wraps to 0.0 and applies the wrong side of the correction -
                // doubling the discontinuity instead of cancelling it.
                // Measured: this pushes the output to +-2.0 at several
                // frequencies (e.g. 672 Hz and 4 kHz at 48 kHz), well outside
                // the waveform's valid range. `edge_blep` takes the signed
                // distance to the edge instead, so every intermediate value
                // stays near zero, where f32 addition is well behaved, and
                // that sum is never formed.
                let delta = t - 0.5;
                raw + poly_blep(t, dt) - edge_blep(delta, dt)
            }
        };
        self.phase += dt;
        if self.phase >= 1.0 {
            self.phase -= 1.0;
        }
        out
    }
}

/// Topology-preserving-transform state variable filter.
///
/// Chosen over a naive digital ladder because it stays stable at high
/// resonance, where the ladder blows up. That stability is asserted in a test.
#[derive(Clone, Copy, Debug, Default)]
pub struct Svf {
    ic1: f32,
    ic2: f32,
}

impl Svf {
    pub fn reset(&mut self) {
        self.ic1 = 0.0;
        self.ic2 = 0.0;
    }

    pub fn lowpass(&mut self, input: f32, cutoff_hz: f32, q: f32, sample_rate: f32) -> f32 {
        let cutoff = cutoff_hz.clamp(20.0, sample_rate * 0.45);
        let g = (PI * cutoff / sample_rate).tan();
        let k = 1.0 / q.max(0.5);
        let a1 = 1.0 / (1.0 + g * (g + k));
        let a2 = g * a1;
        let a3 = g * a2;

        let v3 = input - self.ic2;
        let v1 = a1 * self.ic1 + a2 * v3;
        let v2 = self.ic2 + a2 * self.ic1 + a3 * v3;
        self.ic1 = flush_denormal(2.0 * v1 - self.ic1);
        self.ic2 = flush_denormal(2.0 * v2 - self.ic2);
        v2
    }
}

#[derive(Clone, Copy, Debug)]
pub struct AdsrParams {
    pub attack: f32,
    pub decay: f32,
    pub sustain: f32,
    pub release: f32,
}

impl Default for AdsrParams {
    fn default() -> Self {
        AdsrParams {
            attack: 0.005,
            decay: 0.25,
            sustain: 0.7,
            release: 0.4,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
enum Stage {
    #[default]
    Idle,
    Attack,
    Decay,
    Sustain,
    Release,
}

/// Linear attack, exponential decay and release.
///
/// The attack is linear because a predictable, fast rise is what makes a note
/// feel immediate. Decay and release are exponential because that is how sound
/// actually dies away.
#[derive(Clone, Copy, Debug, Default)]
pub struct Adsr {
    stage: Stage,
    level: f32,
}

/// Below this the release is over. An exponential curve never reaches zero, and
/// a voice that never finishes releasing is a voice the pool can never reuse.
const IDLE_THRESHOLD: f32 = 1e-4;

impl Adsr {
    pub fn gate_on(&mut self) {
        self.stage = Stage::Attack;
    }

    pub fn gate_off(&mut self) {
        if self.stage != Stage::Idle {
            self.stage = Stage::Release;
        }
    }

    pub fn is_idle(&self) -> bool {
        self.stage == Stage::Idle
    }

    pub fn reset(&mut self) {
        self.stage = Stage::Idle;
        self.level = 0.0;
    }

    pub fn tick(&mut self, p: &AdsrParams, sample_rate: f32) -> f32 {
        let coeff = |seconds: f32| 1.0 - (-1.0 / (seconds.max(0.0005) * sample_rate)).exp();
        match self.stage {
            Stage::Idle => {
                self.level = 0.0;
            }
            Stage::Attack => {
                self.level += 1.0 / (p.attack.max(0.0005) * sample_rate);
                if self.level >= 1.0 {
                    self.level = 1.0;
                    self.stage = Stage::Decay;
                }
            }
            Stage::Decay => {
                self.level += (p.sustain - self.level) * coeff(p.decay);
                if (self.level - p.sustain).abs() < 1e-3 {
                    self.level = p.sustain;
                    self.stage = Stage::Sustain;
                }
            }
            Stage::Sustain => {
                self.level = p.sustain;
            }
            Stage::Release => {
                self.level -= self.level * coeff(p.release);
                if self.level < IDLE_THRESHOLD {
                    self.level = 0.0;
                    self.stage = Stage::Idle;
                }
            }
        }
        self.level
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SR: f32 = 48_000.0;

    fn rms(samples: &[f32]) -> f32 {
        (samples.iter().map(|x| x * x).sum::<f32>() / samples.len() as f32).sqrt()
    }

    #[test]
    fn denormals_are_flushed_but_real_values_survive() {
        assert_eq!(flush_denormal(1e-40), 0.0);
        assert_eq!(flush_denormal(-1e-40), 0.0);
        assert_eq!(flush_denormal(0.5), 0.5);
        assert_eq!(flush_denormal(-1e-6), -1e-6);
    }

    #[test]
    fn poly_blep_is_zero_away_from_the_discontinuity() {
        let dt = 0.01;
        assert_eq!(poly_blep(0.5, dt), 0.0);
        assert_eq!(poly_blep(0.3, dt), 0.0);
    }

    #[test]
    fn poly_blep_is_nonzero_at_the_edges() {
        // This correction is the difference between a synth that sounds cheap
        // and one that does not, so its presence is asserted rather than assumed.
        let dt = 0.01;
        assert!(poly_blep(0.001, dt) != 0.0);
        assert!(poly_blep(0.999, dt) != 0.0);
    }

    #[test]
    fn every_waveform_stays_in_range_across_the_spectrum() {
        for wave in [
            Waveform::Sine,
            Waveform::Triangle,
            Waveform::Saw,
            Waveform::Square,
        ] {
            for freq in [20.0, 440.0, 4_000.0, 12_000.0] {
                let mut osc = Osc::default();
                for _ in 0..4_800 {
                    let v = osc.tick(freq, SR, wave);
                    assert!(v.is_finite(), "{wave:?} at {freq} Hz produced {v}");
                    assert!(v.abs() <= 1.5, "{wave:?} at {freq} Hz produced {v}");
                }
            }
        }
    }

    #[test]
    fn a_sine_has_the_expected_rms() {
        let mut osc = Osc::default();
        let out: Vec<f32> = (0..48_000)
            .map(|_| osc.tick(100.0, SR, Waveform::Sine))
            .collect();
        assert!((rms(&out) - std::f32::consts::FRAC_1_SQRT_2).abs() < 0.01);
    }

    #[test]
    fn band_limiting_reduces_high_frequency_saw_energy() {
        // A naive saw aliases: energy that should not exist folds back down. The
        // band-limited version must have measurably less total energy at a
        // frequency where aliasing is severe.
        let mut naive = 0.0f32;
        let mut naive_out = Vec::new();
        let inc = 6_000.0 / SR;
        for _ in 0..48_000 {
            naive += inc;
            if naive >= 1.0 {
                naive -= 1.0;
            }
            naive_out.push(2.0 * naive - 1.0);
        }
        let mut osc = Osc::default();
        let blep_out: Vec<f32> = (0..48_000)
            .map(|_| osc.tick(6_000.0, SR, Waveform::Saw))
            .collect();
        assert!(
            rms(&blep_out) < rms(&naive_out),
            "band-limited {} should be below naive {}",
            rms(&blep_out),
            rms(&naive_out)
        );
    }

    #[test]
    fn the_filter_passes_a_low_tone_and_removes_a_high_one() {
        let mut low_osc = Osc::default();
        let mut low_filter = Svf::default();
        let low: Vec<f32> = (0..24_000)
            .map(|_| {
                low_filter.lowpass(low_osc.tick(100.0, SR, Waveform::Sine), 2_000.0, 0.707, SR)
            })
            .collect();

        let mut high_osc = Osc::default();
        let mut high_filter = Svf::default();
        let high: Vec<f32> = (0..24_000)
            .map(|_| {
                high_filter.lowpass(
                    high_osc.tick(12_000.0, SR, Waveform::Sine),
                    2_000.0,
                    0.707,
                    SR,
                )
            })
            .collect();

        assert!(
            rms(&low) > 0.6,
            "the passband was attenuated: {}",
            rms(&low)
        );
        assert!(rms(&high) < 0.1, "the stopband leaked: {}", rms(&high));
    }

    #[test]
    fn the_filter_stays_stable_at_maximum_resonance() {
        // A naive digital ladder blows up here. This is why the filter is TPT.
        let mut osc = Osc::default();
        let mut filter = Svf::default();
        for _ in 0..48_000 {
            let v = filter.lowpass(osc.tick(220.0, SR, Waveform::Saw), 800.0, 20.0, SR);
            assert!(v.is_finite(), "filter produced {v}");
            assert!(v.abs() < 100.0, "filter ran away to {v}");
        }
    }

    #[test]
    fn the_envelope_rises_holds_and_falls() {
        let p = AdsrParams {
            attack: 0.01,
            decay: 0.05,
            sustain: 0.5,
            release: 0.05,
        };
        let mut env = Adsr::default();
        assert!(env.is_idle());

        env.gate_on();
        let mut peak: f32 = 0.0;
        for _ in 0..(0.01 * SR) as usize {
            peak = peak.max(env.tick(&p, SR));
        }
        assert!(peak > 0.95, "attack only reached {peak}");

        for _ in 0..(0.5 * SR) as usize {
            env.tick(&p, SR);
        }
        let held = env.tick(&p, SR);
        assert!((held - 0.5).abs() < 0.02, "sustain settled at {held}");

        env.gate_off();
        for _ in 0..(2.0 * SR) as usize {
            env.tick(&p, SR);
        }
        assert!(env.is_idle(), "envelope never became idle");
        assert_eq!(env.tick(&p, SR), 0.0);
    }

    #[test]
    fn an_idle_envelope_stays_silent_forever() {
        // A voice pool decides a voice is free by asking this. If an idle
        // envelope ever produced signal, freed voices would leak sound.
        let p = AdsrParams {
            attack: 0.01,
            decay: 0.05,
            sustain: 0.5,
            release: 0.05,
        };
        let mut env = Adsr::default();
        for _ in 0..1_000 {
            assert_eq!(env.tick(&p, SR), 0.0);
        }
    }

    #[test]
    fn releasing_a_note_that_never_sounded_does_nothing() {
        let p = AdsrParams {
            attack: 0.01,
            decay: 0.05,
            sustain: 0.5,
            release: 0.05,
        };
        let mut env = Adsr::default();
        env.gate_off();
        assert!(env.is_idle());
        assert_eq!(env.tick(&p, SR), 0.0);
    }
    #[test]
    fn an_oscillator_asked_for_an_impossible_frequency_stays_in_range() {
        // `every_waveform_stays_in_range_across_the_spectrum` only asks for
        // frequencies a keyboard can produce at 48 kHz, so the clamp on `dt`
        // never does anything there: removing `.clamp(0.0, 0.49)` leaves every
        // test in this crate green.
        //
        // It is not decoration. The phase wrap subtracts 1.0 exactly once per
        // sample, so a `dt` outside [0, 1) walks the phase away without bound
        // and the waveform goes with it - measured with the clamp removed, a
        // saw at -440 Hz reaches 995 994 and one at five times the sample rate
        // reaches 632 009. With it, nothing exceeds 1.0.
        //
        // Reachable, not hypothetical: `Action::NoteOn` carries a `u8`, so a
        // source that is not the computer keyboard can ask for note 200, and
        // at any sample rate below about 25 kHz even the top of the ordinary
        // MIDI range is already past Nyquist. A DSP primitive must not depend
        // on its caller having checked.
        for wave in [
            Waveform::Sine,
            Waveform::Triangle,
            Waveform::Saw,
            Waveform::Square,
        ] {
            for freq in [-44_100.0, -440.0, 0.0, SR, 5.0 * SR, 1e9] {
                let mut osc = Osc::default();
                for _ in 0..1_000 {
                    let v = osc.tick(freq, SR, wave);
                    assert!(v.is_finite(), "{wave:?} at {freq} Hz produced {v}");
                    assert!(
                        v.abs() <= 1.5,
                        "{wave:?} at {freq} Hz produced {v}, outside the \
                         waveform's range"
                    );
                }
            }
        }
    }
}
