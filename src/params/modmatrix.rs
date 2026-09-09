use crate::core::ids::{ModSourceId, ParamId};

#[derive(Clone, Copy, Debug)]
pub struct ModRoute {
    pub source: ModSourceId,
    pub target: ParamId,
    pub depth: f32,
}

/// One mechanism for every kind of modulation.
///
/// Macros, LFOs and mapped continuous controls are all just sources with
/// routes. "Whammy to cutoff" is not a feature here, it is a row - which is the
/// whole reason this type exists rather than three separate systems.
pub struct ModMatrix {
    base: Vec<f32>,
    sources: Vec<f32>,
    routes: Vec<ModRoute>,
    out: Vec<f32>,
}

impl ModMatrix {
    pub fn new(param_count: usize, source_count: usize) -> Self {
        ModMatrix {
            base: vec![0.0; param_count],
            sources: vec![0.0; source_count],
            routes: Vec::with_capacity(64),
            out: vec![0.0; param_count],
        }
    }

    pub fn set_base(&mut self, p: ParamId, v: f32) {
        self.base[p.0 as usize] = v.clamp(0.0, 1.0);
    }

    pub fn base(&self, p: ParamId) -> f32 {
        self.base[p.0 as usize]
    }

    pub fn set_source(&mut self, s: ModSourceId, v: f32) {
        self.sources[s.0 as usize] = v.clamp(-1.0, 1.0);
    }

    pub fn source(&self, s: ModSourceId) -> f32 {
        self.sources[s.0 as usize]
    }

    /// Routes are added while the patch is being built, never from the audio
    /// thread - `Vec::push` may allocate.
    pub fn add_route(&mut self, route: ModRoute) {
        self.routes.push(route);
    }

    /// Allocation-free. Runs once per audio block.
    pub fn recompute(&mut self) {
        self.out.copy_from_slice(&self.base);
        for r in &self.routes {
            self.out[r.target.0 as usize] += self.sources[r.source.0 as usize] * r.depth;
        }
        // Clamp only once, after every contribution has been summed. Clamping
        // per route would make the result depend on route order.
        for v in &mut self.out {
            *v = v.clamp(0.0, 1.0);
        }
    }

    pub fn value(&self, p: ParamId) -> f32 {
        self.out[p.0 as usize]
    }

    pub fn values(&self) -> &[f32] {
        &self.out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::ids::{ModSourceId, ParamId};

    fn matrix() -> ModMatrix {
        ModMatrix::new(4, 4)
    }

    #[test]
    fn without_routes_the_value_is_the_base() {
        let mut m = matrix();
        m.set_base(ParamId(0), 0.3);
        m.recompute();
        assert!((m.value(ParamId(0)) - 0.3).abs() < 1e-6);
    }

    #[test]
    fn a_route_adds_source_times_depth() {
        let mut m = matrix();
        m.set_base(ParamId(0), 0.2);
        m.add_route(ModRoute {
            source: ModSourceId(0),
            target: ParamId(0),
            depth: 0.5,
        });
        m.set_source(ModSourceId(0), 0.6);
        m.recompute();
        assert!((m.value(ParamId(0)) - 0.5).abs() < 1e-6);
    }

    #[test]
    fn a_negative_source_subtracts() {
        // Bipolar macros depend on this: DARK is BRIGHT with a negative source.
        let mut m = matrix();
        m.set_base(ParamId(0), 0.7);
        m.add_route(ModRoute {
            source: ModSourceId(0),
            target: ParamId(0),
            depth: 0.5,
        });
        m.set_source(ModSourceId(0), -1.0);
        m.recompute();
        assert!((m.value(ParamId(0)) - 0.2).abs() < 1e-6);
    }

    #[test]
    fn clamping_happens_after_summing_not_per_route() {
        // This is the one that silently ruins modulation. Base 0.5, one route
        // pushing +0.8 and another -0.6 must land on 0.7. Clamping each route as
        // it is applied would give clamp(1.3)=1.0 then 1.0-0.6=0.4 - a different
        // sound, and one that changes depending on route order.
        let mut m = matrix();
        m.set_base(ParamId(0), 0.5);
        m.add_route(ModRoute {
            source: ModSourceId(0),
            target: ParamId(0),
            depth: 1.0,
        });
        m.add_route(ModRoute {
            source: ModSourceId(1),
            target: ParamId(0),
            depth: 1.0,
        });
        m.set_source(ModSourceId(0), 0.8);
        m.set_source(ModSourceId(1), -0.6);
        m.recompute();
        assert!((m.value(ParamId(0)) - 0.7).abs() < 1e-6);
    }

    #[test]
    fn the_result_is_clamped_to_the_normalized_range() {
        let mut m = matrix();
        m.set_base(ParamId(0), 0.9);
        m.add_route(ModRoute {
            source: ModSourceId(0),
            target: ParamId(0),
            depth: 1.0,
        });
        m.set_source(ModSourceId(0), 1.0);
        m.recompute();
        assert!((m.value(ParamId(0)) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn one_source_can_drive_several_targets() {
        // This is what a macro is. Nothing special is needed for it to work.
        let mut m = matrix();
        m.add_route(ModRoute {
            source: ModSourceId(0),
            target: ParamId(0),
            depth: 1.0,
        });
        m.add_route(ModRoute {
            source: ModSourceId(0),
            target: ParamId(1),
            depth: 0.5,
        });
        m.set_source(ModSourceId(0), 0.4);
        m.recompute();
        assert!((m.value(ParamId(0)) - 0.4).abs() < 1e-6);
        assert!((m.value(ParamId(1)) - 0.2).abs() < 1e-6);
    }

    #[test]
    fn the_base_survives_recomputation() {
        // recompute() must not write its result back into base, or modulation
        // would ratchet upward on every block.
        let mut m = matrix();
        m.set_base(ParamId(0), 0.5);
        m.add_route(ModRoute {
            source: ModSourceId(0),
            target: ParamId(0),
            depth: 0.3,
        });
        m.set_source(ModSourceId(0), 1.0);
        for _ in 0..100 {
            m.recompute();
        }
        assert!((m.base(ParamId(0)) - 0.5).abs() < 1e-6);
        assert!((m.value(ParamId(0)) - 0.8).abs() < 1e-6);
    }
}
