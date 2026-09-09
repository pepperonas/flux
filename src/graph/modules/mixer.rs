use crate::graph::module::{Module, ModuleSpec, ProcessCtx};
use crate::graph::patch::{spec_for, ModuleKind};

/// Sums its inputs. Summing is explicit and visible in the patch, which is why
/// a plain input port refuses a second cable - two sources landing on one
/// input would sum invisibly, and the patch view would show two cables into a
/// socket that looks like it holds one.
///
/// The scratch buffers exist so `process` never allocates: an unconnected
/// input still needs somewhere to hold "silence" for the length of the block
/// while the connected one is summed against it.
#[derive(Default)]
pub struct Mixer {
    scratch_a: Vec<f32>,
    scratch_b: Vec<f32>,
}

impl Module for Mixer {
    fn spec(&self) -> &'static ModuleSpec {
        spec_for(ModuleKind::Mixer)
    }

    fn prepare(&mut self, _sample_rate: f32, max_block: usize) {
        self.scratch_a = vec![0.0; max_block];
        self.scratch_b = vec![0.0; max_block];
    }

    fn reset(&mut self) {
        // No recursive state: both scratch buffers are fully overwritten
        // before every use, so there is nothing here to zero between notes.
    }

    fn process(&mut self, ctx: &mut ProcessCtx) {
        let frames = ctx.frames;
        match ctx.input(0) {
            Some(s) => self.scratch_a[..frames].copy_from_slice(&s[..frames]),
            None => self.scratch_a[..frames].fill(0.0),
        }
        match ctx.input(1) {
            Some(s) => self.scratch_b[..frames].copy_from_slice(&s[..frames]),
            None => self.scratch_b[..frames].fill(0.0),
        }

        let a = &self.scratch_a[..frames];
        let b = &self.scratch_b[..frames];
        if let Some(out) = ctx.output(0) {
            for ((o, x), y) in out.iter_mut().zip(a).zip(b) {
                *o = x + y;
            }
        }
    }
}
