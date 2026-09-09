use std::sync::Arc;

use crate::audio::voice::{ParamValues, VoicePool};
use crate::core::event::Action;
use crate::core::ids::ParamId;
use crate::engine::telemetry::{AudioCommand, EngineEvent, Telemetry};
use crate::graph::module::Module;
use crate::graph::modules;
use crate::graph::patch::{ModuleKind, Patch, PortRef};
use crate::graph::schedule::{run, BufferPool, Schedule};
use crate::params::macros::{macro_source, MACROS, MOD_SOURCE_COUNT};
use crate::params::modmatrix::{ModMatrix, ModRoute};
use crate::params::registry::{self as p, ParamRegistry, PARAM_COUNT};

/// Owns every piece of DSP state. Lives entirely on the audio thread.
///
/// Grown from the Task 12 minimal engine into the real two-zone engine:
/// sixteen voices feed the scheduled global graph (mixer -> delay -> reverb ->
/// output). `new`, `render` and `output` keep the signature Task 12 fixed, so
/// `engine::host` calls this exactly the same way it called the stub.
pub struct AudioEngine {
    sample_rate: f32,
    pub telemetry: Arc<Telemetry>,
    pub params: ModMatrix,
    registry: ParamRegistry,

    voices: VoicePool,
    voice_bus: Vec<f32>,

    schedule: Schedule,
    pool: BufferPool,
    modules: Vec<Box<dyn Module>>,
    voice_buffer: usize,
    output_buffer: usize,
    out: Vec<f32>,

    test_tone: bool,
    test_phase: f32,
}

impl AudioEngine {
    pub fn new(sample_rate: f32, max_block: usize, telemetry: Arc<Telemetry>) -> AudioEngine {
        let registry = ParamRegistry::new();

        let mut params = ModMatrix::new(PARAM_COUNT, MOD_SOURCE_COUNT);
        for i in 0..PARAM_COUNT {
            let id = ParamId(i as u16);
            params.set_base(id, registry.normalize(id, registry.desc(id).default));
        }
        // The M1a macro routes. The remaining macros are defined but unrouted
        // until patterns exist in M5 - a knob that moves nothing is honest here,
        // and the interface marks them as inactive rather than pretending.
        let bright = macro_source(MACROS[0].id);
        let wet = macro_source(MACROS[1].id);
        let chaos = macro_source(MACROS[3].id);
        params.add_route(ModRoute {
            source: bright,
            target: p::FILTER_CUTOFF,
            depth: 0.45,
        });
        params.add_route(ModRoute {
            source: bright,
            target: p::FILTER_RESONANCE,
            depth: 0.15,
        });
        params.add_route(ModRoute {
            source: wet,
            target: p::REVERB_MIX,
            depth: 0.5,
        });
        params.add_route(ModRoute {
            source: wet,
            target: p::DELAY_MIX,
            depth: 0.35,
        });
        params.add_route(ModRoute {
            source: chaos,
            target: p::LFO_AMOUNT,
            depth: 0.7,
        });
        params.add_route(ModRoute {
            source: chaos,
            target: p::DELAY_FEEDBACK,
            depth: 0.3,
        });

        // The fixed M1a global patch. The voice chain feeds the mixer; the
        // module specs for that chain live in graph::patch so the patch view can
        // draw it even though it is executed directly in audio::voice.
        let mut patch = Patch::default();
        let mixer = patch.add(ModuleKind::Mixer);
        let delay = patch.add(ModuleKind::Delay);
        let reverb = patch.add(ModuleKind::Reverb);
        let output = patch.add(ModuleKind::Output);
        patch
            .connect(
                PortRef {
                    module: mixer,
                    port: 0,
                },
                PortRef {
                    module: delay,
                    port: 0,
                },
            )
            .expect("the built-in patch is valid");
        patch
            .connect(
                PortRef {
                    module: delay,
                    port: 0,
                },
                PortRef {
                    module: reverb,
                    port: 0,
                },
            )
            .expect("the built-in patch is valid");
        patch
            .connect(
                PortRef {
                    module: reverb,
                    port: 0,
                },
                PortRef {
                    module: output,
                    port: 0,
                },
            )
            .expect("the built-in patch is valid");

        let mut schedule = Schedule::compile(&patch).expect("the built-in patch compiles");
        let mut module_list: Vec<Box<dyn Module>> = patch
            .nodes()
            .iter()
            .map(|n| modules::make(n.kind))
            .collect();
        for m in module_list.iter_mut() {
            m.prepare(sample_rate, max_block);
        }
        // One buffer past the compiled ones carries the voice sum into the
        // mixer, whose first input the fixed patch leaves unconnected.
        let voice_buffer = schedule.buffer_count;
        let pool = BufferPool::new(schedule.buffer_count + 1, max_block);
        let mixer_step = schedule
            .steps
            .iter_mut()
            .find(|s| s.module == mixer)
            .expect("the mixer is in the schedule");
        mixer_step.inputs[0] = Some(voice_buffer);
        let output_buffer = schedule
            .steps
            .iter()
            .find(|s| s.module == output)
            .and_then(|s| s.outputs[0])
            .expect("the output module has an audio output port");

        AudioEngine {
            sample_rate,
            telemetry,
            params,
            registry,
            voices: VoicePool::default(),
            voice_bus: vec![0.0; max_block],
            schedule,
            pool,
            modules: module_list,
            voice_buffer,
            output_buffer,
            out: vec![0.0; max_block],
            test_tone: false,
            test_phase: 0.0,
        }
    }

    fn drain_commands(&mut self) {
        // Drain everything waiting, not one per block: sixteen notes pushed
        // together must all sound in the same block, or a chord spreads out.
        while let Some(cmd) = self.telemetry.commands.pop() {
            match cmd {
                AudioCommand::SetTestTone(on) => self.test_tone = on,
                AudioCommand::Act(action) => match action {
                    Action::NoteOn { note, velocity } => {
                        if let Some(stolen) = self.voices.note_on(note, velocity, self.sample_rate)
                        {
                            self.telemetry
                                .push_event(EngineEvent::VoiceStolen { note: stolen });
                        }
                        self.telemetry.push_event(EngineEvent::NoteStarted { note });
                    }
                    Action::NoteOff { note } => {
                        self.voices.note_off(note);
                        self.telemetry.push_event(EngineEvent::NoteEnded { note });
                    }
                    Action::SetMacro { macro_id, value } => {
                        self.params.set_source(macro_source(macro_id), value);
                    }
                    Action::SetParam { target, value } => {
                        self.params.set_base(target, value);
                    }
                    // Transport, octave and velocity are handled on the input
                    // thread; they never reach the audio thread in M1a.
                    Action::Transport(_) | Action::OctaveShift(_) | Action::VelocityShift(_) => {}
                },
            }
        }
    }

    pub fn render(&mut self, frames: usize) {
        let frames = frames.min(self.out.len());
        self.drain_commands();
        self.params.recompute();

        let mut values = ParamValues::default();
        values.0.copy_from_slice(self.params.values());

        self.voice_bus[..frames].fill(0.0);
        self.voices
            .render(&mut self.voice_bus[..frames], &values, self.sample_rate);

        if self.test_tone {
            let inc = 440.0 / self.sample_rate;
            for s in self.voice_bus[..frames].iter_mut() {
                *s += (self.test_phase * std::f32::consts::TAU).sin() * 0.2;
                self.test_phase += inc;
                if self.test_phase >= 1.0 {
                    self.test_phase -= 1.0;
                }
            }
        }

        // Hand the voice sum to the mixer through the reserved buffer, run the
        // global graph, then take the terminal node's audio back out.
        self.pool
            .write(self.voice_buffer, &self.voice_bus[..frames]);

        run(
            &self.schedule,
            &mut self.pool,
            &mut self.modules,
            frames,
            self.sample_rate,
            self.params.values(),
        );

        self.out[..frames].copy_from_slice(&self.pool.buffer(self.output_buffer)[..frames]);
        self.telemetry.set_peak(
            self.out[..frames]
                .iter()
                .fold(0.0f32, |a, b| a.max(b.abs())),
        );

        self.telemetry
            .set_active_voices(self.voices.active_count() as u32);
        self.telemetry
            .set_active_notes(self.voices.active_pitch_classes());
    }

    pub fn output(&self) -> &[f32] {
        &self.out
    }

    pub fn active_notes(&self) -> u16 {
        self.voices.active_pitch_classes()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::event::Action;
    use crate::engine::telemetry::{AudioCommand, Telemetry};

    fn engine() -> AudioEngine {
        AudioEngine::new(48_000.0, 512, Telemetry::new(64))
    }

    #[test]
    fn a_fresh_engine_renders_silence() {
        let mut e = engine();
        e.render(256);
        assert!(e.output()[..256].iter().all(|v| *v == 0.0));
    }

    #[test]
    fn a_note_on_command_produces_sound() {
        let mut e = engine();
        e.telemetry.push_command(AudioCommand::Act(Action::NoteOn {
            note: 60,
            velocity: 1.0,
        }));
        e.render(256);
        let peak = e.output()[..256].iter().fold(0.0f32, |a, b| a.max(b.abs()));
        assert!(peak > 0.001, "the engine stayed silent, peak {peak}");
    }

    #[test]
    fn commands_are_drained_every_block_not_one_per_block() {
        // Sixteen notes pushed together must all sound in the same block. A
        // one-per-block drain would spread a chord over a quarter of a second.
        let mut e = engine();
        for note in 48..64u8 {
            e.telemetry.push_command(AudioCommand::Act(Action::NoteOn {
                note,
                velocity: 0.8,
            }));
        }
        e.render(64);
        assert_eq!(e.telemetry.active_voices(), 16);
    }

    #[test]
    fn a_macro_reaches_the_parameter_it_is_routed_to() {
        let mut e = engine();
        let before = e.params.value(crate::params::registry::FILTER_CUTOFF);
        e.telemetry
            .push_command(AudioCommand::Act(Action::SetMacro {
                macro_id: crate::core::ids::MacroId(0),
                value: 1.0,
            }));
        e.render(64);
        let after = e.params.value(crate::params::registry::FILTER_CUTOFF);
        assert!(
            after > before,
            "BRIGHT did not open the filter: {before} then {after}"
        );
    }

    #[test]
    fn a_negative_bipolar_macro_moves_the_other_way() {
        let mut e = engine();
        e.render(64);
        let neutral = e.params.value(crate::params::registry::FILTER_CUTOFF);
        e.telemetry
            .push_command(AudioCommand::Act(Action::SetMacro {
                macro_id: crate::core::ids::MacroId(0),
                value: -1.0,
            }));
        e.render(64);
        assert!(
            e.params.value(crate::params::registry::FILTER_CUTOFF) < neutral,
            "DARK did not close the filter"
        );
    }

    #[test]
    fn the_test_tone_is_independent_of_any_voice() {
        let mut e = engine();
        e.telemetry.push_command(AudioCommand::SetTestTone(true));
        e.render(256);
        let peak = e.output()[..256].iter().fold(0.0f32, |a, b| a.max(b.abs()));
        assert!(peak > 0.01);
        assert_eq!(e.telemetry.active_voices(), 0);
    }

    #[test]
    fn active_notes_reports_the_pitch_classes_being_played() {
        let mut e = engine();
        e.telemetry.push_command(AudioCommand::Act(Action::NoteOn {
            note: 60,
            velocity: 1.0,
        }));
        e.telemetry.push_command(AudioCommand::Act(Action::NoteOn {
            note: 67,
            velocity: 1.0,
        }));
        e.render(64);
        let mask = e.active_notes();
        assert_ne!(mask & (1 << 0), 0, "C is not reported");
        assert_ne!(mask & (1 << 7), 0, "G is not reported");
        assert_eq!(mask & (1 << 1), 0, "C sharp should not be reported");
    }

    #[test]
    fn active_notes_are_published_to_telemetry() {
        // `active_notes()` above is only reachable from inside the audio
        // callback closure that owns this engine - the UI thread reads this
        // exclusively through `Telemetry`, published from `render()` right
        // next to `set_active_voices`. Pinned separately from the test above:
        // a bug in one path (e.g. `render` never calling `set_active_notes`,
        // or publishing a stale/empty mask) says nothing about the other, and
        // this is the one the performance view's note blocks actually read.
        let mut e = engine();
        e.telemetry.push_command(AudioCommand::Act(Action::NoteOn {
            note: 60,
            velocity: 1.0,
        }));
        e.telemetry.push_command(AudioCommand::Act(Action::NoteOn {
            note: 67,
            velocity: 1.0,
        }));
        e.render(64);
        let mask = e.telemetry.active_notes();
        assert_ne!(mask & (1 << 0), 0, "C is not published to telemetry");
        assert_ne!(mask & (1 << 7), 0, "G is not published to telemetry");
    }

    #[test]
    fn rendering_never_produces_a_nan() {
        // One NaN poisons everything downstream and the speakers go silent with
        // nothing logged anywhere.
        let mut e = engine();
        for note in 40..56u8 {
            e.telemetry.push_command(AudioCommand::Act(Action::NoteOn {
                note,
                velocity: 1.0,
            }));
        }
        e.telemetry
            .push_command(AudioCommand::Act(Action::SetMacro {
                macro_id: crate::core::ids::MacroId(1),
                value: 1.0,
            }));
        for _ in 0..500 {
            e.render(256);
            assert!(e.output()[..256].iter().all(|v| v.is_finite()));
        }
    }
}
