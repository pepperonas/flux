use crate::core::ids::ParamId;
use crate::graph::signal::SignalType;

pub const MAX_INPUTS: usize = 8;
pub const MAX_OUTPUTS: usize = 4;

/// Which zone a module lives in.
///
/// Voice-zone modules are instantiated per sounding note; global-zone modules
/// run once per block. The boundary is one way: voice output sums into global.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Zone {
    Voice,
    Global,
}

#[derive(Clone, Copy, Debug)]
pub struct PortSpec {
    pub name: &'static str,
    pub signal: SignalType,
}

#[derive(Clone, Copy, Debug)]
pub struct ModuleSpec {
    pub name: &'static str,
    pub zone: Zone,
    pub inputs: &'static [PortSpec],
    pub outputs: &'static [PortSpec],
    pub params: &'static [ParamId],
}

/// What a module sees while processing. It never learns about the graph, the
/// patch, or which buffers it was given.
pub struct ProcessCtx<'a> {
    pub frames: usize,
    pub sample_rate: f32,
    pub inputs: [Option<&'a [f32]>; MAX_INPUTS],
    pub outputs: [Option<&'a mut [f32]>; MAX_OUTPUTS],
    /// Resolved, normalised 0..1, indexed by the module spec's `params` order.
    pub params: &'a [f32],
}

impl<'a> ProcessCtx<'a> {
    /// An unconnected input reads as silence. Modules must handle `None` rather
    /// than assume a cable is present.
    pub fn input(&self, index: usize) -> Option<&[f32]> {
        self.inputs.get(index).copied().flatten()
    }

    pub fn output(&mut self, index: usize) -> Option<&mut [f32]> {
        self.outputs.get_mut(index)?.as_deref_mut()
    }

    pub fn param(&self, index: usize) -> f32 {
        self.params.get(index).copied().unwrap_or(0.0)
    }
}

/// A processing unit in the graph.
///
/// `prepare` is the only place a module may allocate. `process` runs in the
/// audio callback and must not allocate, lock, or block.
pub trait Module: Send {
    fn spec(&self) -> &'static ModuleSpec;
    fn prepare(&mut self, sample_rate: f32, max_block: usize);
    /// Modules **overwrite** their outputs; they never accumulate into them.
    /// Summing is a mixer's job, stated explicitly in the patch.
    fn process(&mut self, ctx: &mut ProcessCtx);
    fn reset(&mut self);
}
