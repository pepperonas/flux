use crate::core::ids::ParamId;

pub const OSC_WAVE: ParamId = ParamId(0);
pub const OSC_DETUNE: ParamId = ParamId(1);
pub const OSC_LEVEL: ParamId = ParamId(2);
pub const FILTER_CUTOFF: ParamId = ParamId(3);
pub const FILTER_RESONANCE: ParamId = ParamId(4);
pub const ENV_ATTACK: ParamId = ParamId(5);
pub const ENV_DECAY: ParamId = ParamId(6);
pub const ENV_SUSTAIN: ParamId = ParamId(7);
pub const ENV_RELEASE: ParamId = ParamId(8);
pub const LFO_RATE: ParamId = ParamId(9);
pub const LFO_AMOUNT: ParamId = ParamId(10);
pub const DELAY_TIME: ParamId = ParamId(11);
pub const DELAY_FEEDBACK: ParamId = ParamId(12);
pub const DELAY_MIX: ParamId = ParamId(13);
pub const REVERB_SIZE: ParamId = ParamId(14);
pub const REVERB_MIX: ParamId = ParamId(15);
pub const MASTER_GAIN: ParamId = ParamId(16);
pub const PARAM_COUNT: usize = 17;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Taper {
    Linear,
    /// Constant ratio per unit of travel. Required for anything measured in
    /// hertz or seconds, where hearing is logarithmic.
    Exponential,
}

#[derive(Clone, Copy, Debug)]
pub struct ParamDesc {
    pub name: &'static str,
    pub min: f32,
    pub max: f32,
    pub default: f32,
    pub taper: Taper,
    pub unit: &'static str,
    /// Plain language, shown as a tooltip. It lives here rather than in the
    /// interface so that the explanation and the value cannot drift apart.
    pub help: &'static str,
}

pub struct ParamRegistry {
    descs: [ParamDesc; PARAM_COUNT],
}

impl Default for ParamRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl ParamRegistry {
    pub fn new() -> Self {
        use Taper::{Exponential as Exp, Linear as Lin};
        let d = |name, min, max, default, taper, unit, help| ParamDesc {
            name,
            min,
            max,
            default,
            taper,
            unit,
            help,
        };
        ParamRegistry {
            descs: [
                d("Waveform", 0.0, 3.0, 2.0, Lin, "",
                  "The basic shape of the sound. Sine is pure and soft, saw is bright and buzzy."),
                d("Detune", 0.0, 50.0, 8.0, Lin, "cents",
                  "Pulls the two oscillators slightly apart. A little makes the sound fatter; a lot makes it seasick."),
                d("Level", 0.0, 1.0, 0.8, Lin, "",
                  "How loud each played note is before the effects."),
                d("Cutoff", 20.0, 20_000.0, 2_000.0, Exp, "Hz",
                  "Frequencies above this are removed. Lower values make the sound darker and rounder."),
                d("Resonance", 0.5, 20.0, 1.0, Lin, "",
                  "Emphasises the frequencies right at the cutoff. Raise it for a sharper, more vocal sound."),
                d("Attack", 0.001, 4.0, 0.005, Exp, "s",
                  "How long the note takes to reach full volume. Short is percussive, long is a swell."),
                d("Decay", 0.001, 4.0, 0.25, Exp, "s",
                  "How long it takes to fall from full volume to the sustain level."),
                d("Sustain", 0.0, 1.0, 0.7, Lin, "",
                  "The level a note holds at while a key stays down."),
                d("Release", 0.001, 8.0, 0.4, Exp, "s",
                  "How long the note takes to fade after the key is let go."),
                d("LFO Rate", 0.01, 20.0, 1.2, Exp, "Hz",
                  "How fast the modulation wobbles."),
                d("LFO Amount", 0.0, 1.0, 0.0, Lin, "",
                  "How far the wobble moves whatever it is connected to."),
                d("Delay Time", 0.01, 2.0, 0.375, Exp, "s",
                  "The gap before each echo repeats."),
                d("Delay Feedback", 0.0, 0.95, 0.35, Lin, "",
                  "How much of each echo is fed back in. High values repeat for a long time."),
                d("Delay Mix", 0.0, 1.0, 0.2, Lin, "",
                  "How much echo is blended into the sound."),
                d("Reverb Size", 0.0, 1.0, 0.5, Lin, "",
                  "How large the imagined room is. Larger sounds more distant."),
                d("Reverb Mix", 0.0, 1.0, 0.18, Lin, "",
                  "How much room is blended into the sound."),
                d("Master", 0.0, 1.0, 0.8, Lin, "",
                  "The overall output level."),
            ],
        }
    }

    pub fn desc(&self, id: ParamId) -> &ParamDesc {
        &self.descs[id.0 as usize]
    }

    /// Normalised 0..1 to the parameter's real units.
    pub fn denormalize(&self, id: ParamId, norm: f32) -> f32 {
        let d = self.desc(id);
        let n = norm.clamp(0.0, 1.0);
        match d.taper {
            Taper::Linear => d.min + n * (d.max - d.min),
            Taper::Exponential => d.min * (d.max / d.min).powf(n),
        }
    }

    /// Real units back to normalised 0..1.
    pub fn normalize(&self, id: ParamId, value: f32) -> f32 {
        let d = self.desc(id);
        let v = value.clamp(d.min, d.max);
        match d.taper {
            Taper::Linear => (v - d.min) / (d.max - d.min),
            Taper::Exponential => (v / d.min).ln() / (d.max / d.min).ln(),
        }
    }

    pub fn format(&self, id: ParamId, norm: f32) -> String {
        let d = self.desc(id);
        let v = self.denormalize(id, norm);
        if d.unit.is_empty() {
            format!("{v:.2}")
        } else if v >= 1000.0 {
            format!("{:.1}k {}", v / 1000.0, d.unit)
        } else {
            format!("{v:.2} {}", d.unit)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_param_id_has_a_description() {
        let reg = ParamRegistry::new();
        for i in 0..PARAM_COUNT {
            let d = reg.desc(ParamId(i as u16));
            assert!(!d.name.is_empty(), "param {i} has no name");
            assert!(!d.help.is_empty(), "param {i} has no help text");
            assert!(d.min < d.max, "param {i} has an empty range");
            assert!(
                d.default >= d.min && d.default <= d.max,
                "param {i} default is out of range"
            );
        }
    }

    #[test]
    fn a_linear_param_maps_the_ends_and_the_middle() {
        let reg = ParamRegistry::new();
        assert!((reg.denormalize(FILTER_RESONANCE, 0.0) - 0.5).abs() < 1e-4);
        assert!((reg.denormalize(FILTER_RESONANCE, 1.0) - 20.0).abs() < 1e-4);
        assert!((reg.denormalize(FILTER_RESONANCE, 0.5) - 10.25).abs() < 1e-4);
    }

    #[test]
    fn cutoff_is_exponential_so_the_knob_feels_musical() {
        // A linear cutoff spends most of its travel above 10 kHz, where nothing
        // musically interesting happens. Half-way must land near the geometric
        // mean, not the arithmetic one.
        let reg = ParamRegistry::new();
        assert!((reg.denormalize(FILTER_CUTOFF, 0.0) - 20.0).abs() < 1e-3);
        assert!((reg.denormalize(FILTER_CUTOFF, 1.0) - 20_000.0).abs() < 1.0);
        let mid = reg.denormalize(FILTER_CUTOFF, 0.5);
        assert!(
            (mid - 632.45).abs() < 1.0,
            "expected the geometric mean, got {mid}"
        );
    }

    #[test]
    fn normalize_and_denormalize_are_inverses() {
        let reg = ParamRegistry::new();
        for id in [FILTER_CUTOFF, FILTER_RESONANCE, ENV_ATTACK, MASTER_GAIN] {
            for n in [0.0f32, 0.13, 0.5, 0.87, 1.0] {
                let round_trip = reg.normalize(id, reg.denormalize(id, n));
                assert!(
                    (round_trip - n).abs() < 1e-4,
                    "{id:?} at {n} round-tripped to {round_trip}"
                );
            }
        }
    }
}
