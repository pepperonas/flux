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

    /// Like `render`, but writes into a caller-supplied buffer instead of a
    /// fresh one, so the buffer can be called into repeatedly without being
    /// cleared in between - the way a real pool buffer behaves (see
    /// `BufferPool` / `schedule::run`), and unlike `render` above.
    fn render_into(m: &mut dyn Module, input: Option<&[f32]>, params: &[f32], out_buf: &mut [f32]) {
        let mut inputs: [Option<&[f32]>; MAX_INPUTS] = [None; MAX_INPUTS];
        inputs[0] = input;
        let mut outputs: [Option<&mut [f32]>; MAX_OUTPUTS] = [const { None }; MAX_OUTPUTS];
        outputs[0] = Some(out_buf);
        let mut ctx = ProcessCtx {
            frames: FRAMES,
            sample_rate: 48_000.0,
            inputs,
            outputs,
            params,
        };
        m.process(&mut ctx);
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

    // The test above is weaker than its name and comment claim: `render()`
    // allocates a fresh, zeroed `out_buf` on every call, so it cannot tell
    // `=` from `+=` for a module with no carried state - both read 0.0 as
    // their starting point regardless of which one `Mixer::process` uses.
    // Confirmed directly: with `*o += x + y` swapped into `Mixer::process`,
    // every test in this file that goes through `render()` still passes,
    // `a_module_overwrites_its_output_rather_than_accumulating` above
    // included. In production the pool buffer a module writes into is not
    // cleared between blocks (see `BufferPool::write` / `schedule::run`),
    // so the two tests below reuse ONE output buffer across two
    // `process()` calls, uncleared in between, to match that - one for a
    // stateless module, one for a module with real internal recursive
    // state, since an accumulation bug in the output write is exactly as
    // dangerous and exactly as invisible in the second case as the first.

    #[test]
    fn the_mixer_does_not_accumulate_into_a_reused_output_buffer() {
        let mut m = make(crate::graph::patch::ModuleKind::Mixer);
        m.prepare(48_000.0, FRAMES);
        let a = vec![0.25f32; FRAMES];
        let params = defaults();

        let mut out_buf = vec![0.0f32; FRAMES];
        render_into(m.as_mut(), Some(&a), &params, &mut out_buf);
        render_into(m.as_mut(), Some(&a), &params, &mut out_buf);

        assert!(
            (out_buf[0] - 0.25).abs() < 1e-6,
            "output accumulated across calls into a reused buffer: {}",
            out_buf[0]
        );
    }

    #[test]
    fn the_delay_does_not_accumulate_into_a_reused_output_buffer() {
        // DELAY_MIX is set to 0.0 so the output is pure dry passthrough
        // (out[i] = dry[i]), which stays predictable regardless of what the
        // delay line's internal buffer/feedback state is doing underneath -
        // that recursive computation still runs on every sample (`self.
        // buffer[self.write] = ...` is unaffected by `mix`); only its
        // contribution to *this* output is zeroed by the mix blend. The
        // line under test, `*o = dry * (1.0 - mix) + wet * mix`, does not
        // care what `mix` is - it is the same overwrite-vs-accumulate
        // question as the mixer's, just reached through a recursive module.
        let mut m = make(crate::graph::patch::ModuleKind::Delay);
        m.prepare(48_000.0, FRAMES);
        let mut params = defaults();
        params[crate::params::registry::DELAY_MIX.0 as usize] = 0.0;
        let input = vec![0.3f32; FRAMES];

        let mut out_buf = vec![0.0f32; FRAMES];
        render_into(m.as_mut(), Some(&input), &params, &mut out_buf);
        render_into(m.as_mut(), Some(&input), &params, &mut out_buf);

        assert!(
            (out_buf[0] - 0.3).abs() < 1e-6,
            "output accumulated across calls into a reused buffer: {}",
            out_buf[0]
        );
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
    /// Like `render`, but feeds both of the mixer's inputs.
    fn render_two(
        m: &mut dyn Module,
        a: Option<&[f32]>,
        b: Option<&[f32]>,
        params: &[f32],
    ) -> Vec<f32> {
        let mut out_buf = vec![0.0f32; FRAMES];
        let mut inputs: [Option<&[f32]>; MAX_INPUTS] = [None; MAX_INPUTS];
        inputs[0] = a;
        inputs[1] = b;
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

    #[test]
    fn the_mixer_adds_its_second_input_to_its_first() {
        // `the_mixer_sums_its_inputs` above supplies input 0 only, so it
        // never touches the aux port and never sees an addition happen:
        // mutating `*o = x + y` to `*o = x + y * 0.0` leaves it green. This
        // one feeds both, with different values, so the result can only be
        // right if both terms are really there and neither is scaled.
        let mut m = make(crate::graph::patch::ModuleKind::Mixer);
        m.prepare(48_000.0, FRAMES);
        let a = vec![0.25f32; FRAMES];
        let b = vec![-0.1f32; FRAMES];
        let out = render_two(m.as_mut(), Some(&a), Some(&b), &defaults());
        assert!(
            (out[0] - 0.15).abs() < 1e-6,
            "0.25 and -0.1 summed to {}",
            out[0]
        );

        // The aux port on its own must arrive too - a mixer that only ever
        // copies input 0 would pass the assertion above if `y` were dropped
        // and `x` happened to be the whole answer.
        let only_b = render_two(m.as_mut(), None, Some(&b), &defaults());
        assert!(
            (only_b[0] + 0.1).abs() < 1e-6,
            "the aux input alone produced {}",
            only_b[0]
        );
    }

    #[test]
    fn the_master_stage_clamps_a_runaway_before_the_clipper_sees_it() {
        // This is the stage between a patch that has run away and a
        // full-scale spike into somebody's headphones, and nothing tested it.
        // 18.0 is not a hypothetical: it is the delay's own measured peak at
        // maximum feedback and minimum time (see the extremes test below).
        let mut m = Output::default();
        m.prepare(48_000.0, FRAMES);
        let mut params = defaults();
        params[crate::params::registry::MASTER_GAIN.0 as usize] = 1.0;
        let runaway = vec![18.0f32; FRAMES];
        let out = render(&mut m, Some(&runaway), &params);

        assert!(
            (m.master_peak() - 1.5).abs() < 1e-6,
            "the gained signal reached the clipper at {}, not the 1.5 the \
             clamp is there to impose",
            m.master_peak()
        );
        let expected = 1.5f32.tanh();
        assert!(
            (out[0] - expected).abs() < 1e-4,
            "18.0 came out at {} instead of tanh(1.5) = {expected}",
            out[0]
        );
    }

    #[test]
    fn the_master_stage_soft_clips_instead_of_passing_a_spike_through() {
        // 1.2 is under the 1.5 clamp, so only the soft clip stands between it
        // and a sample above full scale. Removing `.tanh()` sends 1.2 straight
        // out; a hard clip would send exactly 1.0, which lands as a click.
        let mut m = Output::default();
        m.prepare(48_000.0, FRAMES);
        let mut params = defaults();
        params[crate::params::registry::MASTER_GAIN.0 as usize] = 1.0;
        let hot = vec![1.2f32; FRAMES];
        let out = render(&mut m, Some(&hot), &params);

        let expected = 1.2f32.tanh();
        assert!(
            (out[0] - expected).abs() < 1e-4,
            "1.2 came out at {} instead of tanh(1.2) = {expected}",
            out[0]
        );
        assert!(
            out.iter().all(|v| v.abs() < 1.0),
            "a sample at or past full scale left the master stage"
        );
    }

    /// Every parameter corner worth checking: both ends of everything at
    /// once, each parameter alone at each end, and the two combinations that
    /// are actually dangerous.
    fn extreme_configs() -> Vec<(String, Vec<f32>)> {
        use crate::params::registry as r;
        let mut configs: Vec<(String, Vec<f32>)> = vec![
            ("all-min".into(), vec![0.0; PARAM_COUNT]),
            ("all-max".into(), vec![1.0; PARAM_COUNT]),
        ];
        let mut c = defaults();
        c[r::DELAY_FEEDBACK.0 as usize] = 1.0;
        c[r::DELAY_TIME.0 as usize] = 0.0;
        c[r::DELAY_MIX.0 as usize] = 1.0;
        configs.push(("delay feedback max, time min, mix max".into(), c));
        let mut c = defaults();
        c[r::REVERB_SIZE.0 as usize] = 1.0;
        c[r::REVERB_MIX.0 as usize] = 1.0;
        configs.push(("reverb size max, mix max".into(), c));
        for i in 0..PARAM_COUNT {
            for (end, v) in [("min", 0.0f32), ("max", 1.0f32)] {
                let mut c = defaults();
                c[i] = v;
                configs.push((format!("parameter {i} at {end}"), c));
            }
        }
        configs
    }

    #[test]
    fn no_module_produces_a_nan_or_runs_away_at_the_extremes_of_its_parameters() {
        // `no_module_produces_a_nan_or_runs_away` only ever runs at the
        // registry's defaults, which is the one setting nobody needs
        // reassuring about. At the extremes the recursive modules genuinely
        // do get loud: measured, the delay reaches exactly 18.0 (0.9 input
        // divided by 1 - 0.95 feedback) and the reverb 17.53. Neither is a
        // fault - the master stage's clamp is what makes them safe, and that
        // is pinned above - but a NaN or an unbounded climb here would be,
        // and the difference is invisible without looking.
        use crate::graph::patch::ModuleKind::*;
        let configs = extreme_configs();
        for kind in [Lfo, Mixer, Delay, Reverb, Output] {
            for (label, params) in &configs {
                let mut m = make(kind);
                m.prepare(48_000.0, FRAMES);
                let mut noisy = vec![0.0f32; FRAMES];
                for (i, v) in noisy.iter_mut().enumerate() {
                    *v = if i % 2 == 0 { 0.9 } else { -0.9 };
                }
                // Long enough for the delay's geometric series to converge on
                // its 18.0 asymptote: 0.01 s of delay is 480 samples, and
                // 0.95^135 is a thousandth.
                for _ in 0..600 {
                    let out = render(m.as_mut(), Some(&noisy), params);
                    for v in &out {
                        assert!(v.is_finite(), "{kind:?} [{label}] produced {v}");
                        assert!(v.abs() < 20.0, "{kind:?} [{label}] ran away to {v}");
                    }
                }
            }
        }
    }

    #[test]
    fn a_bigger_reverb_rings_for_longer() {
        // The comb feedback is `0.7 + size * 0.28`. Replacing that whole
        // expression with the constant 0.7 - which is to say making REVERB_SIZE
        // do nothing at all - left every test in this crate green.
        //
        // Measured on the tail: excite the reverb, then feed it silence and
        // see how much is left. At size 0 the feedback is 0.7 and a comb
        // pass is ~35 ms, so a second of silence is 28 passes and 0.7^28 is
        // about 1e-5. At size 1 the feedback is 0.98 and the same 28 passes
        // leave 0.57 of it.
        fn tail_energy(size: f32) -> f32 {
            let mut m = make(crate::graph::patch::ModuleKind::Reverb);
            m.prepare(48_000.0, FRAMES);
            let mut params = defaults();
            params[crate::params::registry::REVERB_SIZE.0 as usize] = size;
            params[crate::params::registry::REVERB_MIX.0 as usize] = 1.0;
            let excite = vec![0.5f32; FRAMES];
            for _ in 0..8 {
                render(m.as_mut(), Some(&excite), &params);
            }
            let silence = vec![0.0f32; FRAMES];
            // Roughly one second of decay at 48 kHz.
            for _ in 0..375 {
                render(m.as_mut(), Some(&silence), &params);
            }
            let out = render(m.as_mut(), Some(&silence), &params);
            out.iter().fold(0.0f32, |a, b| a.max(b.abs()))
        }

        let small = tail_energy(0.0);
        let large = tail_energy(1.0);
        assert!(
            large > small * 100.0,
            "a full-size room decayed to {large:.6} where the smallest \
             decayed to {small:.6}; REVERB_SIZE is not reaching the combs"
        );
    }
}
