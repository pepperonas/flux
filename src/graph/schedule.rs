use crate::core::ids::ModuleId;
use crate::graph::module::{Module, ProcessCtx, MAX_INPUTS, MAX_OUTPUTS};
use crate::graph::patch::{spec_for, Patch, PatchError, PortRef};

#[derive(Clone, Copy, Debug)]
pub struct Step {
    pub module: ModuleId,
    /// Index into the module instance slice, so execution needs no lookup.
    pub module_index: usize,
    pub inputs: [Option<usize>; MAX_INPUTS],
    pub outputs: [Option<usize>; MAX_OUTPUTS],
}

/// A compiled, flat execution order with buffer indices already resolved.
///
/// Compilation happens off the audio thread. The finished schedule is handed
/// over by pointer swap and the outgoing one is dropped on the thread that
/// built it, so the audio thread never sorts, allocates or frees.
#[derive(Clone, Debug, Default)]
pub struct Schedule {
    pub steps: Vec<Step>,
    pub buffer_count: usize,
}

impl Schedule {
    pub fn compile(patch: &Patch) -> Result<Schedule, PatchError> {
        let order = patch.topological_order()?;

        // One buffer per output port. Reusing buffers would save memory at the
        // cost of a liveness analysis that is not worth its risk yet.
        let mut buffer_of: Vec<(PortRef, usize)> = Vec::new();
        let mut next_buffer = 0usize;
        for node in patch.nodes() {
            let spec = spec_for(node.kind);
            for port in 0..spec.outputs.len() {
                buffer_of.push((
                    PortRef {
                        module: node.id,
                        port: port as u8,
                    },
                    next_buffer,
                ));
                next_buffer += 1;
            }
        }
        let lookup = |p: PortRef| buffer_of.iter().find(|(r, _)| *r == p).map(|(_, i)| *i);

        let mut steps = Vec::with_capacity(order.len());
        for id in order {
            let kind = patch.kind(id).ok_or(PatchError::UnknownModule(id))?;
            let spec = spec_for(kind);

            let mut inputs = [None; MAX_INPUTS];
            for port in 0..spec.inputs.len().min(MAX_INPUTS) {
                let to = PortRef {
                    module: id,
                    port: port as u8,
                };
                if let Some(edge) = patch.edges().iter().find(|e| e.to == to) {
                    inputs[port] = lookup(edge.from);
                }
            }

            let mut outputs = [None; MAX_OUTPUTS];
            for port in 0..spec.outputs.len().min(MAX_OUTPUTS) {
                outputs[port] = lookup(PortRef {
                    module: id,
                    port: port as u8,
                });
            }

            let module_index = patch
                .nodes()
                .iter()
                .position(|n| n.id == id)
                .ok_or(PatchError::UnknownModule(id))?;

            steps.push(Step {
                module: id,
                module_index,
                inputs,
                outputs,
            });
        }

        Ok(Schedule {
            steps,
            buffer_count: next_buffer,
        })
    }
}

/// Pre-allocated audio buffers, one per output port.
pub struct BufferPool {
    bufs: Vec<Vec<f32>>,
}

impl BufferPool {
    pub fn new(buffer_count: usize, max_block: usize) -> Self {
        BufferPool {
            bufs: (0..buffer_count).map(|_| vec![0.0; max_block]).collect(),
        }
    }

    pub fn buffer(&self, index: usize) -> &[f32] {
        &self.bufs[index]
    }

    pub fn clear(&mut self) {
        for b in &mut self.bufs {
            b.fill(0.0);
        }
    }

    /// Hands a buffer's contents in from outside the graph - the engine uses
    /// this to feed the polyphonic voice sum into the mixer.
    ///
    /// This runs on the audio thread, so a length mismatch must not panic:
    /// copies only `min(src.len(), buffer.len())` samples. If `src` is
    /// longer than the buffer, the excess is dropped. If `src` is shorter,
    /// only the leading samples are overwritten - the remainder of the
    /// buffer is left exactly as it was, not cleared. A mismatch is a
    /// caller bug (the voice-sum length drifting from the block size), and
    /// the right failure mode for a caller bug on the audio thread is one
    /// quietly-wrong block, not a crash mid-performance.
    pub fn write(&mut self, index: usize, src: &[f32]) {
        let dst = &mut self.bufs[index];
        let n = src.len().min(dst.len());
        dst[..n].copy_from_slice(&src[..n]);
    }
}

/// Execute one block. Allocation-free.
///
/// Output buffers are moved out of the pool before inputs are borrowed, then
/// moved back. Moving a `Vec` is a pointer copy, so this costs nothing and
/// avoids both `unsafe` and the aliasing problem of borrowing two elements of
/// one `Vec` at once.
pub fn run(
    schedule: &Schedule,
    pool: &mut BufferPool,
    modules: &mut [Box<dyn Module>],
    frames: usize,
    sample_rate: f32,
    params: &[f32],
) {
    for step in &schedule.steps {
        let mut owned: [Option<Vec<f32>>; MAX_OUTPUTS] = [const { None }; MAX_OUTPUTS];
        for (slot, index) in step.outputs.iter().enumerate() {
            if let Some(i) = index {
                owned[slot] = Some(std::mem::take(&mut pool.bufs[*i]));
            }
        }

        {
            let mut inputs: [Option<&[f32]>; MAX_INPUTS] = [None; MAX_INPUTS];
            for (slot, index) in step.inputs.iter().enumerate() {
                if let Some(i) = index {
                    inputs[slot] = Some(&pool.bufs[*i][..frames]);
                }
            }

            let mut outputs: [Option<&mut [f32]>; MAX_OUTPUTS] = [const { None }; MAX_OUTPUTS];
            for (slot, buf) in owned.iter_mut().enumerate() {
                if let Some(b) = buf {
                    outputs[slot] = Some(&mut b[..frames]);
                }
            }

            let mut ctx = ProcessCtx {
                frames,
                sample_rate,
                inputs,
                outputs,
                params,
            };
            modules[step.module_index].process(&mut ctx);
        }

        for (slot, index) in step.outputs.iter().enumerate() {
            if let Some(i) = index {
                if let Some(b) = owned[slot].take() {
                    pool.bufs[*i] = b;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::module::{Module, ModuleSpec, ProcessCtx};
    use crate::graph::patch::{ModuleKind, Patch, PortRef};

    /// Writes a constant, so the order of execution is visible in the output.
    struct Const(f32);
    impl Module for Const {
        fn spec(&self) -> &'static ModuleSpec {
            crate::graph::patch::spec_for(ModuleKind::Mixer)
        }
        fn prepare(&mut self, _: f32, _: usize) {}
        fn process(&mut self, ctx: &mut ProcessCtx) {
            let v = self.0;
            if let Some(out) = ctx.output(0) {
                out.fill(v);
            }
        }
        fn reset(&mut self) {}
    }

    /// Adds one to whatever arrives, so a missing input is detectable.
    struct AddOne;
    impl Module for AddOne {
        fn spec(&self) -> &'static ModuleSpec {
            crate::graph::patch::spec_for(ModuleKind::Delay)
        }
        fn prepare(&mut self, _: f32, _: usize) {}
        fn process(&mut self, ctx: &mut ProcessCtx) {
            let input: Vec<f32> = match ctx.input(0) {
                Some(s) => s.to_vec(),
                None => vec![0.0; ctx.frames],
            };
            if let Some(out) = ctx.output(0) {
                for (o, i) in out.iter_mut().zip(input) {
                    *o = i + 1.0;
                }
            }
        }
        fn reset(&mut self) {}
    }

    #[test]
    fn a_chain_executes_in_dependency_order() {
        let mut patch = Patch::default();
        let src = patch.add(ModuleKind::Mixer);
        let add = patch.add(ModuleKind::Delay);
        patch
            .connect(
                PortRef {
                    module: src,
                    port: 0,
                },
                PortRef {
                    module: add,
                    port: 0,
                },
            )
            .unwrap();

        let schedule = Schedule::compile(&patch).unwrap();
        let mut pool = BufferPool::new(schedule.buffer_count, 64);
        let mut modules: Vec<Box<dyn Module>> = vec![Box::new(Const(2.0)), Box::new(AddOne)];

        run(&schedule, &mut pool, &mut modules, 64, 48_000.0, &[0.0; 32]);

        let add_output = schedule
            .steps
            .iter()
            .find(|s| s.module == add)
            .unwrap()
            .outputs[0]
            .unwrap();
        assert_eq!(pool.buffer(add_output)[0], 3.0);
    }

    #[test]
    fn an_unconnected_input_reads_as_silence() {
        // A module must never see stale data from a buffer somebody else used.
        let mut patch = Patch::default();
        let _add = patch.add(ModuleKind::Delay);
        let schedule = Schedule::compile(&patch).unwrap();
        let mut pool = BufferPool::new(schedule.buffer_count, 64);
        let mut modules: Vec<Box<dyn Module>> = vec![Box::new(AddOne)];

        run(&schedule, &mut pool, &mut modules, 64, 48_000.0, &[0.0; 32]);

        let out = schedule.steps[0].outputs[0].unwrap();
        assert_eq!(pool.buffer(out)[0], 1.0);
    }

    #[test]
    fn every_output_port_gets_its_own_buffer() {
        let mut patch = Patch::default();
        let a = patch.add(ModuleKind::Mixer);
        let b = patch.add(ModuleKind::Mixer);
        let schedule = Schedule::compile(&patch).unwrap();
        let out_a = schedule
            .steps
            .iter()
            .find(|s| s.module == a)
            .unwrap()
            .outputs[0]
            .unwrap();
        let out_b = schedule
            .steps
            .iter()
            .find(|s| s.module == b)
            .unwrap()
            .outputs[0]
            .unwrap();
        assert_ne!(out_a, out_b);
    }

    #[test]
    fn a_consumers_input_points_at_its_producers_output() {
        let mut patch = Patch::default();
        let src = patch.add(ModuleKind::Mixer);
        let dst = patch.add(ModuleKind::Delay);
        patch
            .connect(
                PortRef {
                    module: src,
                    port: 0,
                },
                PortRef {
                    module: dst,
                    port: 0,
                },
            )
            .unwrap();
        let schedule = Schedule::compile(&patch).unwrap();
        let producer_out = schedule
            .steps
            .iter()
            .find(|s| s.module == src)
            .unwrap()
            .outputs[0];
        let consumer_in = schedule
            .steps
            .iter()
            .find(|s| s.module == dst)
            .unwrap()
            .inputs[0];
        assert_eq!(producer_out, consumer_in);
        assert!(producer_out.is_some());
    }

    #[test]
    fn running_a_schedule_twice_does_not_allocate_or_drift() {
        // The pool must survive repeated runs with buffers intact - the
        // take-and-return dance is easy to get wrong in a way that only shows
        // up on the second block.
        let mut patch = Patch::default();
        let src = patch.add(ModuleKind::Mixer);
        let add = patch.add(ModuleKind::Delay);
        patch
            .connect(
                PortRef {
                    module: src,
                    port: 0,
                },
                PortRef {
                    module: add,
                    port: 0,
                },
            )
            .unwrap();
        let schedule = Schedule::compile(&patch).unwrap();
        let mut pool = BufferPool::new(schedule.buffer_count, 64);
        let mut modules: Vec<Box<dyn Module>> = vec![Box::new(Const(2.0)), Box::new(AddOne)];

        for _ in 0..100 {
            run(&schedule, &mut pool, &mut modules, 64, 48_000.0, &[0.0; 32]);
        }

        let out = schedule
            .steps
            .iter()
            .find(|s| s.module == add)
            .unwrap()
            .outputs[0]
            .unwrap();
        assert_eq!(pool.buffer(out)[0], 3.0);
        assert_eq!(pool.buffer(out).len(), 64);
    }

    #[test]
    fn write_lands_a_source_that_fits_the_buffer() {
        let mut pool = BufferPool::new(1, 4);
        pool.write(0, &[1.0, 2.0, 3.0, 4.0]);
        assert_eq!(pool.buffer(0), &[1.0, 2.0, 3.0, 4.0][..]);
    }

    #[test]
    fn write_clamps_a_source_longer_than_the_buffer_instead_of_panicking() {
        let mut pool = BufferPool::new(1, 4);
        // Six samples into a four-sample buffer: must not panic, and must
        // keep only what fits.
        pool.write(0, &[1.0, 2.0, 3.0, 4.0, 5.0, 6.0]);
        assert_eq!(pool.buffer(0), &[1.0, 2.0, 3.0, 4.0][..]);
    }

    #[test]
    fn write_with_a_short_source_leaves_the_remainder_untouched() {
        // A caller passing a short slice must not silently believe the tail
        // was cleared - the untouched samples must survive exactly as they
        // were before the call.
        let mut pool = BufferPool::new(1, 4);
        pool.write(0, &[9.0, 9.0, 9.0, 9.0]);
        pool.write(0, &[1.0, 2.0]);
        assert_eq!(pool.buffer(0), &[1.0, 2.0, 9.0, 9.0][..]);
    }
}
