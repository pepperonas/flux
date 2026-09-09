use crate::core::ids::ParamId;
use crate::graph::module::{Module, ModuleSpec, ProcessCtx};
use crate::graph::patch::{spec_for, ModuleKind};
use crate::params::registry::{ParamRegistry, MASTER_GAIN};

/// The terminal node.
///
/// It carries an audio output port (see `spec_for`'s note on `ModuleKind::
/// Output`), so the engine reads the finished block out of the pool like any
/// other module's output.
pub struct Output {
    /// Holds the block's dry input, for the same reason `Delay` and `Reverb`
    /// need one: `ctx.input` and `ctx.output` cannot be borrowed from `ctx`
    /// at once.
    scratch: Vec<f32>,
    peak: f32,
    registry: ParamRegistry,
}

impl Default for Output {
    fn default() -> Self {
        Output {
            scratch: Vec::new(),
            peak: 0.0,
            registry: ParamRegistry::new(),
        }
    }
}

impl Output {
    /// The loudest sample of the most recent block, for the meter.
    ///
    /// Measured on the gained signal *before* the soft clip, not after: a
    /// meter that read the post-`tanh` value would compress towards 1.0 and
    /// could never show how hard the clipper is actually working.
    pub fn master_peak(&self) -> f32 {
        self.peak
    }
}

impl Module for Output {
    fn spec(&self) -> &'static ModuleSpec {
        spec_for(ModuleKind::Output)
    }

    fn prepare(&mut self, _sample_rate: f32, max_block: usize) {
        self.scratch = vec![0.0; max_block];
    }

    fn reset(&mut self) {
        self.peak = 0.0;
    }

    fn peak(&self) -> Option<f32> {
        Some(self.master_peak())
    }

    fn process(&mut self, ctx: &mut ProcessCtx) {
        let frames = ctx.frames;
        match ctx.input(0) {
            Some(s) => self.scratch[..frames].copy_from_slice(&s[..frames]),
            None => self.scratch[..frames].fill(0.0),
        }
        let read = |id: ParamId| ctx.params.get(id.0 as usize).copied().unwrap_or(0.0);
        let gain = self.registry.denormalize(MASTER_GAIN, read(MASTER_GAIN));

        let mut peak = 0.0f32;
        match ctx.output(0) {
            Some(out) => {
                for (o, dry) in out.iter_mut().zip(&self.scratch[..frames]) {
                    let gained = (dry * gain).clamp(-1.5, 1.5);
                    peak = peak.max(gained.abs());
                    // A soft clip rather than a hard one: a runaway patch
                    // should sound wrong, not damage anything or produce a
                    // digital spike that lands like a click. The clamp before
                    // it bounds what the clipper is asked to do, so an
                    // arbitrarily large input cannot arrive at `tanh` as an
                    // infinity.
                    *o = gained.tanh();
                }
            }
            // Nobody is listening this block, but the meter must not freeze
            // on a stale reading, so the block is still measured.
            None => {
                for dry in &self.scratch[..frames] {
                    peak = peak.max((dry * gain).clamp(-1.5, 1.5).abs());
                }
            }
        }
        self.peak = peak;
    }
}
