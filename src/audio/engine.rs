use crate::engine::telemetry::{AudioCommand, Telemetry};
use std::sync::Arc;

/// Minimal engine for M1a: proves the path from device to speaker with a test
/// tone. Task 13 grows this into the real two-zone engine (voices feeding the
/// scheduled global graph); the signature here - `new`, `render`, `output` -
/// is what that task keeps.
pub struct AudioEngine {
    sample_rate: f32,
    pub telemetry: Arc<Telemetry>,
    out: Vec<f32>,
    test_tone: bool,
    test_phase: f32,
}

impl AudioEngine {
    pub fn new(sample_rate: f32, max_block: usize, telemetry: Arc<Telemetry>) -> AudioEngine {
        AudioEngine {
            sample_rate,
            telemetry,
            out: vec![0.0; max_block],
            test_tone: false,
            test_phase: 0.0,
        }
    }

    pub fn render(&mut self, frames: usize) {
        let frames = frames.min(self.out.len());
        while let Some(cmd) = self.telemetry.commands.pop() {
            if let AudioCommand::SetTestTone(on) = cmd {
                self.test_tone = on;
            }
        }
        self.out[..frames].fill(0.0);
        if self.test_tone {
            let inc = 440.0 / self.sample_rate;
            let mut peak = 0.0f32;
            for s in self.out[..frames].iter_mut() {
                *s = (self.test_phase * std::f32::consts::TAU).sin() * 0.2;
                peak = peak.max(s.abs());
                self.test_phase += inc;
                if self.test_phase >= 1.0 {
                    self.test_phase -= 1.0;
                }
            }
            self.telemetry.set_peak(peak);
        } else {
            self.telemetry.set_peak(0.0);
        }
    }

    pub fn output(&self) -> &[f32] {
        &self.out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_test_tone_produces_finite_in_range_audio() {
        // This is the diagnostic the test tone exists to be: proof that the
        // path from a command through the engine to an output buffer works,
        // independent of whether any input is wired up yet.
        let telemetry = Telemetry::new(4);
        let mut engine = AudioEngine::new(48_000.0, 512, Arc::clone(&telemetry));
        assert!(telemetry.push_command(AudioCommand::SetTestTone(true)));

        engine.render(512);
        let out = engine.output();

        assert!(out.iter().any(|&s| s != 0.0), "test tone produced silence");
        assert!(
            out.iter().all(|s| s.is_finite()),
            "test tone produced a non-finite sample"
        );
        assert!(
            out.iter().all(|s| s.abs() <= 1.0),
            "test tone exceeded full scale"
        );
    }

    #[test]
    fn without_the_test_tone_the_engine_stays_silent() {
        let telemetry = Telemetry::new(4);
        let mut engine = AudioEngine::new(48_000.0, 512, Arc::clone(&telemetry));

        engine.render(512);

        assert!(engine.output().iter().all(|&s| s == 0.0));
    }
}
