pub mod delay;
pub mod lfo;
pub mod mixer;
pub mod output;
pub mod reverb;

pub use delay::Delay;
pub use lfo::Lfo;
pub use mixer::Mixer;
pub use output::Output;
pub use reverb::Reverb;

use crate::graph::module::Module;
use crate::graph::patch::ModuleKind;

/// Build a module instance. Each module owns its own `ParamRegistry`, built
/// here, so no registry needs to be threaded through the graph.
///
/// Voice-zone kinds are not executed through the graph in M1a - `audio::voice`
/// runs that chain directly - so asking for one here is a programming error
/// rather than a runtime condition.
pub fn make(kind: ModuleKind) -> Box<dyn Module> {
    match kind {
        ModuleKind::Lfo => Box::new(Lfo::default()),
        ModuleKind::Mixer => Box::new(Mixer::default()),
        ModuleKind::Delay => Box::new(Delay::default()),
        ModuleKind::Reverb => Box::new(Reverb::default()),
        ModuleKind::Output => Box::new(Output::default()),
        ModuleKind::Oscillator | ModuleKind::Filter | ModuleKind::Envelope | ModuleKind::Vca => {
            panic!("{kind:?} is a voice-zone module and is not executed through the graph in M1a")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::module::{Module, ProcessCtx, MAX_INPUTS, MAX_OUTPUTS};
    use crate::params::registry::{ParamRegistry, PARAM_COUNT};

    const FRAMES: usize = 128;

    /// Run one module in isolation with a given input and normalised params.
    fn render(m: &mut dyn Module, input: Option<&[f32]>, params: &[f32]) -> Vec<f32> {
        let mut out_buf = vec![0.0f32; FRAMES];
        let mut inputs: [Option<&[f32]>; MAX_INPUTS] = [None; MAX_INPUTS];
        inputs[0] = input;
        let mut outputs: [Option<&mut [f32]>; MAX_OUTPUTS] = [const { None }; MAX_OUTPUTS];
        outputs[0] = Some(&mut out_buf[..]);
        let mut ctx = ProcessCtx {
            frames: FRAMES,
            sample_rate: 48_000.0,
            inputs,
            outputs,
            params,
        };
        m.process(&mut ctx);
        out_buf
    }

    fn defaults() -> Vec<f32> {
        let reg = ParamRegistry::new();
        (0..PARAM_COUNT)
            .map(|i| {
                let id = crate::core::ids::ParamId(i as u16);
                reg.normalize(id, reg.desc(id).default)
            })
            .collect()
    }

    #[test]
    fn every_module_kind_can_be_built() {
        use crate::graph::patch::ModuleKind::*;
        for kind in [Lfo, Mixer, Delay, Reverb, Output] {
            let m = make(kind);
            assert_eq!(m.spec().name, crate::graph::patch::spec_for(kind).name);
        }
    }

    #[test]
    fn the_mixer_sums_its_inputs() {
        let mut m = make(crate::graph::patch::ModuleKind::Mixer);
        m.prepare(48_000.0, FRAMES);
        let a = vec![0.25f32; FRAMES];
        let out = render(m.as_mut(), Some(&a), &defaults());
        assert!((out[0] - 0.25).abs() < 1e-6);
    }

    #[test]
    fn a_module_overwrites_its_output_rather_than_accumulating() {
        // The contract every module obeys. If one accumulated, its output would
        // grow without bound across blocks and nothing would say why.
        let mut m = make(crate::graph::patch::ModuleKind::Mixer);
        m.prepare(48_000.0, FRAMES);
        let a = vec![0.25f32; FRAMES];
        let first = render(m.as_mut(), Some(&a), &defaults());
        let second = render(m.as_mut(), Some(&a), &defaults());
        assert!((first[0] - second[0]).abs() < 1e-6);
    }

    #[test]
    fn the_delay_repeats_an_impulse_later_not_immediately() {
        // Deviation from the brief: its version set DELAY_TIME by normalising
        // 0.001 s, with a comment expecting that to land on 48 samples, and
        // then looked for the echo inside a single 128-frame render() call
        // (samples 40..60). But DELAY_TIME's registered range (Task 7,
        // src/params/registry.rs) is [0.01, 2.0] s - `normalize` clamps
        // 0.001 down to that 0.01 s floor, which is 480 samples at 48 kHz.
        // No legal DELAY_TIME value can produce an echo inside one 128-frame
        // block; the registry's own minimum already exceeds it. This keeps
        // the property under test (an echo appears later, not immediately)
        // but drives the delay at its real minimum and scans across enough
        // blocks - the module's state persists across `render()` calls,
        // since it lives in `self`, not in the per-call `ProcessCtx` - for
        // the echo to actually arrive.
        let mut m = make(crate::graph::patch::ModuleKind::Delay);
        m.prepare(48_000.0, FRAMES);
        let mut params = defaults();
        params[crate::params::registry::DELAY_TIME.0 as usize] = 0.0; // registry minimum: 0.01 s = 480 samples
        params[crate::params::registry::DELAY_MIX.0 as usize] = 1.0;

        let mut input = vec![0.0f32; FRAMES];
        input[0] = 1.0;
        let first = render(m.as_mut(), Some(&input), &params);
        assert!(
            first[0].abs() < 0.9,
            "the echo arrived instantly: {}",
            first[0]
        );

        let silence = vec![0.0f32; FRAMES];
        let mut echo = 0.0f32;
        for _ in 0..4 {
            let out = render(m.as_mut(), Some(&silence), &params);
            echo = echo.max(out.iter().fold(0.0f32, |a, b| a.max(b.abs())));
        }
        assert!(
            echo > 0.1,
            "no echo appeared around the expected delay, peak was {echo}"
        );
    }

    #[test]
    fn no_module_produces_a_nan_or_runs_away() {
        // A single NaN poisons everything downstream and the speakers go silent
        // with no error anywhere. Cheap to check, catastrophic to miss.
        use crate::graph::patch::ModuleKind::*;
        for kind in [Lfo, Mixer, Delay, Reverb, Output] {
            let mut m = make(kind);
            m.prepare(48_000.0, FRAMES);
            let mut noisy = vec![0.0f32; FRAMES];
            for (i, v) in noisy.iter_mut().enumerate() {
                *v = if i % 2 == 0 { 0.9 } else { -0.9 };
            }
            for _ in 0..200 {
                let out = render(m.as_mut(), Some(&noisy), &defaults());
                for v in &out {
                    assert!(v.is_finite(), "{:?} produced {v}", kind);
                    assert!(v.abs() < 20.0, "{:?} ran away to {v}", kind);
                }
            }
        }
    }

    #[test]
    fn the_output_module_reports_the_peak_it_saw() {
        let mut m = Output::default();
        m.prepare(48_000.0, FRAMES);
        let mut input = vec![0.0f32; FRAMES];
        input[10] = 0.5;
        let mut params = defaults();
        params[crate::params::registry::MASTER_GAIN.0 as usize] = 1.0;
        render(&mut m, Some(&input), &params);
        assert!((m.master_peak() - 0.5).abs() < 1e-3);
    }
}
