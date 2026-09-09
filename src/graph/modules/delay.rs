use crate::audio::dsp::flush_denormal;
use crate::core::ids::ParamId;
use crate::graph::module::{Module, ModuleSpec, ProcessCtx};
use crate::graph::patch::{spec_for, ModuleKind};
use crate::params::registry::{ParamRegistry, DELAY_FEEDBACK, DELAY_MIX, DELAY_TIME};

/// The longest delay the buffer can hold, in seconds. Allocated once in
/// `prepare`; changing the time only moves a read pointer.
const MAX_DELAY_S: f32 = 2.0;

#[derive(Default)]
pub struct Delay {
    buffer: Vec<f32>,
    write: usize,
    /// Holds the block's dry input so it can be read alongside `buffer` while
    /// `ctx.output(0)` is borrowed - `ctx` cannot lend out its input and its
    /// output at once, so the input is copied out here first.
    scratch: Vec<f32>,
    registry: ParamRegistry,
}

impl Module for Delay {
    fn spec(&self) -> &'static ModuleSpec {
        spec_for(ModuleKind::Delay)
    }

    fn prepare(&mut self, sample_rate: f32, max_block: usize) {
        self.buffer = vec![0.0; (sample_rate * MAX_DELAY_S) as usize + 1];
        self.scratch = vec![0.0; max_block];
        self.write = 0;
    }

    fn process(&mut self, ctx: &mut ProcessCtx) {
        let frames = ctx.frames;
        match ctx.input(0) {
            Some(s) => self.scratch[..frames].copy_from_slice(&s[..frames]),
            None => self.scratch[..frames].fill(0.0),
        }
        let read = |id: ParamId| ctx.params.get(id.0 as usize).copied().unwrap_or(0.0);
        let time = self.registry.denormalize(DELAY_TIME, read(DELAY_TIME));
        let feedback = self
            .registry
            .denormalize(DELAY_FEEDBACK, read(DELAY_FEEDBACK));
        let mix = self.registry.denormalize(DELAY_MIX, read(DELAY_MIX));

        let len = self.buffer.len();
        let offset = ((time * ctx.sample_rate) as usize).clamp(1, len - 1);

        if let Some(out) = ctx.output(0) {
            for (dry, o) in self.scratch[..frames].iter().zip(out.iter_mut()) {
                let read_at = (self.write + len - offset) % len;
                let wet = self.buffer[read_at];
                // Overwrites the slot the read pointer will reach `offset`
                // samples from now; it never accumulates into it.
                self.buffer[self.write] = flush_denormal(dry + wet * feedback);
                self.write = (self.write + 1) % len;
                *o = dry * (1.0 - mix) + wet * mix;
            }
        }
    }
}
