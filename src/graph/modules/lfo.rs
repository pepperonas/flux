use crate::core::ids::ParamId;
use crate::graph::module::{Module, ModuleSpec, ProcessCtx};
use crate::graph::patch::{spec_for, ModuleKind};
use crate::params::registry::{ParamRegistry, LFO_AMOUNT, LFO_RATE};
use std::f32::consts::TAU;

/// A free-running sine oscillator in the control domain.
///
/// It has no audio input, so `process` reads nothing from `ctx` but its
/// params - there is nothing to protect against overwriting because there is
/// no other signal path through it.
pub struct Lfo {
    phase: f32,
    registry: ParamRegistry,
}

impl Default for Lfo {
    fn default() -> Self {
        Lfo {
            phase: 0.0,
            registry: ParamRegistry::new(),
        }
    }
}

impl Module for Lfo {
    fn spec(&self) -> &'static ModuleSpec {
        spec_for(ModuleKind::Lfo)
    }

    fn prepare(&mut self, _sample_rate: f32, _max_block: usize) {}

    fn process(&mut self, ctx: &mut ProcessCtx) {
        let read = |id: ParamId| ctx.params.get(id.0 as usize).copied().unwrap_or(0.0);
        let rate = self.registry.denormalize(LFO_RATE, read(LFO_RATE));
        let amount = self.registry.denormalize(LFO_AMOUNT, read(LFO_AMOUNT));
        let inc = rate / ctx.sample_rate;
        let frames = ctx.frames;
        let mut phase = self.phase;

        match ctx.output(0) {
            Some(out) => {
                for sample in out.iter_mut() {
                    *sample = (phase * TAU).sin() * amount;
                    phase += inc;
                    if phase >= 1.0 {
                        phase -= 1.0;
                    }
                }
            }
            // Nobody is listening this block, but the phase still has to
            // advance as if they were, so re-patching mid-performance does
            // not jump the waveform.
            None => phase = (phase + inc * frames as f32).fract(),
        }
        self.phase = phase;
    }
}
