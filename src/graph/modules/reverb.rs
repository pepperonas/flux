use crate::audio::dsp::flush_denormal;
use crate::core::ids::ParamId;
use crate::graph::module::{Module, ModuleSpec, ProcessCtx};
use crate::graph::patch::{spec_for, ModuleKind};
use crate::params::registry::{ParamRegistry, REVERB_MIX, REVERB_SIZE};

/// Four comb filters into two all-passes: a Schroeder reverb.
///
/// Chosen because it is small, well understood, and sounds like a room. Delay
/// lengths are mutually prime so the combs cannot reinforce each other into a
/// ringing pitch, and are quoted for 44.1 kHz then scaled by the real sample
/// rate in `prepare`, so the room does not change size with the audio device.
const COMB_LENS: [usize; 4] = [1_557, 1_617, 1_491, 1_422];
const ALLPASS_LENS: [usize; 2] = [225, 556];

#[derive(Default)]
pub struct Reverb {
    combs: [Vec<f32>; 4],
    comb_pos: [usize; 4],
    allpasses: [Vec<f32>; 2],
    allpass_pos: [usize; 2],
    /// Holds the block's dry input, for the same reason `Delay` needs one:
    /// `ctx.input` and `ctx.output` cannot be borrowed from `ctx` at once.
    scratch: Vec<f32>,
    registry: ParamRegistry,
}

impl Module for Reverb {
    fn spec(&self) -> &'static ModuleSpec {
        spec_for(ModuleKind::Reverb)
    }

    fn prepare(&mut self, sample_rate: f32, max_block: usize) {
        let scale = sample_rate / 44_100.0;
        for (comb, len) in self.combs.iter_mut().zip(COMB_LENS) {
            *comb = vec![0.0; (len as f32 * scale) as usize + 1];
        }
        for (allpass, len) in self.allpasses.iter_mut().zip(ALLPASS_LENS) {
            *allpass = vec![0.0; (len as f32 * scale) as usize + 1];
        }
        self.scratch = vec![0.0; max_block];
        self.comb_pos = [0; 4];
        self.allpass_pos = [0; 2];
    }

    fn reset(&mut self) {
        for c in &mut self.combs {
            c.fill(0.0);
        }
        for a in &mut self.allpasses {
            a.fill(0.0);
        }
    }

    fn process(&mut self, ctx: &mut ProcessCtx) {
        let frames = ctx.frames;
        match ctx.input(0) {
            Some(s) => self.scratch[..frames].copy_from_slice(&s[..frames]),
            None => self.scratch[..frames].fill(0.0),
        }
        let read = |id: ParamId| ctx.params.get(id.0 as usize).copied().unwrap_or(0.0);
        let size = self.registry.denormalize(REVERB_SIZE, read(REVERB_SIZE));
        let mix = self.registry.denormalize(REVERB_MIX, read(REVERB_MIX));
        // Kept below 1.0 with headroom: at 1.0 the combs never decay.
        let feedback = 0.7 + size * 0.28;

        if let Some(out) = ctx.output(0) {
            for (dry, o) in self.scratch[..frames].iter().zip(out.iter_mut()) {
                let mut wet = 0.0;
                for (comb, pos) in self.combs.iter_mut().zip(self.comb_pos.iter_mut()) {
                    let delayed = comb[*pos];
                    wet += delayed;
                    // Overwrites the slot the comb will read back on its next
                    // pass; it never accumulates into it.
                    comb[*pos] = flush_denormal(dry + delayed * feedback);
                    *pos = (*pos + 1) % comb.len();
                }
                wet *= 0.25;
                for (allpass, pos) in self.allpasses.iter_mut().zip(self.allpass_pos.iter_mut()) {
                    let delayed = allpass[*pos];
                    let output = delayed - wet;
                    allpass[*pos] = flush_denormal(wet + delayed * 0.5);
                    *pos = (*pos + 1) % allpass.len();
                    wet = output;
                }
                *o = dry * (1.0 - mix) + wet * mix;
            }
        }
    }
}
