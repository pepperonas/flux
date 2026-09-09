use std::collections::{HashMap, HashSet};

use crate::core::event::{Action, ControlEvent, ControlId, ControlValue};
use crate::core::ids::{MacroId, ParamId};
use crate::core::music::note_for;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Curve {
    #[default]
    Linear,
    Exponential,
    Smoothstep,
}

impl Curve {
    /// Maps 0..=1 to 0..=1. Endpoints are always preserved, so a curve can
    /// never make a control unable to reach its extremes.
    pub fn apply(self, x: f32) -> f32 {
        let x = x.clamp(0.0, 1.0);
        match self {
            Curve::Linear => x,
            Curve::Exponential => x * x,
            Curve::Smoothstep => x * x * (3.0 - 2.0 * x),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Binding {
    /// A note relative to the current octave. The octave is applied at resolve
    /// time, not stored, so changing octave affects the next press and never
    /// strands a held note.
    Note {
        semitone: i8,
    },
    Act(Action),
    Param {
        target: ParamId,
        depth: f32,
        curve: Curve,
    },
    Macro {
        macro_id: MacroId,
        curve: Curve,
    },
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct InputChord {
    pub held: Vec<ControlId>,
    pub trigger: ControlId,
}

#[derive(Clone, Default, Debug)]
pub struct Mapping {
    pub bindings: HashMap<ControlId, Vec<Binding>>,
    pub chords: Vec<(InputChord, Binding)>,
}

impl Mapping {
    pub fn insert(&mut self, id: ControlId, binding: Binding) {
        self.bindings.entry(id).or_default().push(binding);
    }

    pub fn add_chord(&mut self, chord: InputChord, binding: Binding) {
        self.chords.push((chord, binding));
    }
}

/// Which controls are currently held down. Lives on the input thread.
#[derive(Clone, Default, Debug)]
pub struct HeldSet {
    held: HashSet<ControlId>,
}

impl HeldSet {
    pub fn press(&mut self, id: ControlId) {
        self.held.insert(id);
    }

    pub fn release(&mut self, id: ControlId) {
        self.held.remove(&id);
    }

    pub fn contains(&self, id: &ControlId) -> bool {
        self.held.contains(id)
    }

    pub fn len(&self) -> usize {
        self.held.len()
    }

    pub fn is_empty(&self) -> bool {
        self.held.is_empty()
    }
}

/// Performance state that turns a relative binding into a concrete note.
#[derive(Clone, Copy, Debug)]
pub struct PlayState {
    pub octave: i8,
    pub velocity: f32,
}

impl Default for PlayState {
    fn default() -> Self {
        PlayState {
            octave: 4,
            velocity: 0.8,
        }
    }
}

/// Turn one control event into the actions it means.
///
/// Pure, and never called from the audio callback — which is why returning a
/// `Vec` is fine here. Every interaction rule in FLUX is a property of this
/// function, which is what makes them all assertable.
pub fn resolve(
    mapping: &Mapping,
    held: &HeldSet,
    play: &PlayState,
    event: &ControlEvent,
) -> Vec<Action> {
    // A chord takes precedence over the plain binding of the same control.
    // Without this, pressing a modified combination would fire both meanings.
    if event.value.is_press() {
        for (chord, binding) in &mapping.chords {
            if chord.trigger == event.id && chord.held.iter().all(|id| held.contains(id)) {
                let mut out = Vec::new();
                emit(binding, play, event, &mut out);
                return out;
            }
        }
    }

    let Some(bindings) = mapping.bindings.get(&event.id) else {
        return Vec::new();
    };

    let mut out = Vec::new();
    for binding in bindings {
        emit(binding, play, event, &mut out);
    }
    out
}

fn emit(binding: &Binding, play: &PlayState, event: &ControlEvent, out: &mut Vec<Action>) {
    match *binding {
        Binding::Note { semitone } => {
            let note = note_for(play.octave, semitone);
            match event.value {
                ControlValue::Gate(true) | ControlValue::Trigger => out.push(Action::NoteOn {
                    note,
                    velocity: play.velocity,
                }),
                ControlValue::Gate(false) => out.push(Action::NoteOff { note }),
                // A knob bound to a note has no sensible meaning; producing
                // nothing is better than inventing one.
                _ => {}
            }
        }
        Binding::Act(action) => {
            if event.value.is_press() {
                out.push(action);
            }
        }
        Binding::Param {
            target,
            depth,
            curve,
        } => {
            if let Some(v) = event.value.as_continuous() {
                out.push(Action::SetParam {
                    target,
                    value: curve.apply(v) * depth,
                });
            }
        }
        Binding::Macro { macro_id, curve } => {
            if let Some(v) = event.value.as_continuous() {
                out.push(Action::SetMacro {
                    macro_id,
                    value: curve.apply(v),
                });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::event::{ControlEvent, ControlId, ControlValue, KeyCode, MidiControl};
    use crate::core::ids::{MacroId, MidiPortId, ParamId};

    fn ev(id: ControlId, value: ControlValue) -> ControlEvent {
        ControlEvent {
            id,
            value,
            host_time_ns: 0,
        }
    }

    fn play() -> PlayState {
        PlayState {
            octave: 4,
            velocity: 0.8,
        }
    }

    #[test]
    fn a_bound_key_press_produces_a_note_on_at_the_current_octave() {
        let mut m = Mapping::default();
        m.insert(
            ControlId::Keyboard(KeyCode::A),
            Binding::Note { semitone: 0 },
        );
        let out = resolve(
            &m,
            &HeldSet::default(),
            &play(),
            &ev(ControlId::Keyboard(KeyCode::A), ControlValue::Gate(true)),
        );
        assert_eq!(
            out,
            vec![Action::NoteOn {
                note: 60,
                velocity: 0.8
            }]
        );
    }

    #[test]
    fn releasing_the_same_key_produces_the_matching_note_off() {
        let mut m = Mapping::default();
        m.insert(
            ControlId::Keyboard(KeyCode::A),
            Binding::Note { semitone: 0 },
        );
        let out = resolve(
            &m,
            &HeldSet::default(),
            &play(),
            &ev(ControlId::Keyboard(KeyCode::A), ControlValue::Gate(false)),
        );
        assert_eq!(out, vec![Action::NoteOff { note: 60 }]);
    }

    #[test]
    fn the_octave_at_press_time_decides_the_note() {
        let mut m = Mapping::default();
        m.insert(
            ControlId::Keyboard(KeyCode::A),
            Binding::Note { semitone: 0 },
        );
        let state = PlayState {
            octave: 2,
            velocity: 1.0,
        };
        let out = resolve(
            &m,
            &HeldSet::default(),
            &state,
            &ev(ControlId::Keyboard(KeyCode::A), ControlValue::Gate(true)),
        );
        assert_eq!(
            out,
            vec![Action::NoteOn {
                note: 36,
                velocity: 1.0
            }]
        );
    }

    #[test]
    fn an_unbound_control_produces_nothing() {
        let m = Mapping::default();
        let out = resolve(
            &m,
            &HeldSet::default(),
            &play(),
            &ev(ControlId::Keyboard(KeyCode::Q), ControlValue::Gate(true)),
        );
        assert!(out.is_empty());
    }

    #[test]
    fn a_continuous_control_bound_to_a_macro_sets_it() {
        let mut m = Mapping::default();
        let knob = ControlId::Midi {
            port: MidiPortId(0),
            channel: 16,
            control: MidiControl::Cc(21),
        };
        m.insert(
            knob,
            Binding::Macro {
                macro_id: MacroId(3),
                curve: Curve::Linear,
            },
        );
        let out = resolve(
            &m,
            &HeldSet::default(),
            &play(),
            &ev(knob, ControlValue::Continuous(0.25)),
        );
        assert_eq!(
            out,
            vec![Action::SetMacro {
                macro_id: MacroId(3),
                value: 0.25
            }]
        );
    }

    #[test]
    fn a_gate_bound_to_a_macro_produces_nothing() {
        // A discrete control cannot drive a continuous target. Silently sending
        // 0.0 or 1.0 would look like a working mapping and behave like a switch.
        let mut m = Mapping::default();
        let key = ControlId::Keyboard(KeyCode::Z);
        m.insert(
            key,
            Binding::Macro {
                macro_id: MacroId(0),
                curve: Curve::Linear,
            },
        );
        let out = resolve(
            &m,
            &HeldSet::default(),
            &play(),
            &ev(key, ControlValue::Gate(true)),
        );
        assert!(out.is_empty());
    }

    #[test]
    fn a_param_binding_scales_by_depth() {
        let mut m = Mapping::default();
        let knob = ControlId::Midi {
            port: MidiPortId(0),
            channel: 16,
            control: MidiControl::Cc(22),
        };
        m.insert(
            knob,
            Binding::Param {
                target: ParamId(7),
                depth: 0.5,
                curve: Curve::Linear,
            },
        );
        let out = resolve(
            &m,
            &HeldSet::default(),
            &play(),
            &ev(knob, ControlValue::Continuous(1.0)),
        );
        assert_eq!(
            out,
            vec![Action::SetParam {
                target: ParamId(7),
                value: 0.5
            }]
        );
    }

    #[test]
    fn a_chord_fires_only_while_its_modifier_is_held() {
        let mut m = Mapping::default();
        let green = ControlId::Keyboard(KeyCode::F);
        let red = ControlId::Keyboard(KeyCode::G);
        m.insert(red, Binding::Note { semitone: 0 });
        m.add_chord(
            InputChord {
                held: vec![green],
                trigger: red,
            },
            Binding::Act(Action::Transport(crate::core::event::TransportCmd::Toggle)),
        );

        // Without the modifier, the plain binding applies.
        let plain = resolve(
            &m,
            &HeldSet::default(),
            &play(),
            &ev(red, ControlValue::Gate(true)),
        );
        assert_eq!(
            plain,
            vec![Action::NoteOn {
                note: 60,
                velocity: 0.8
            }]
        );

        // With it held, the chord wins and the plain binding is suppressed.
        let mut held = HeldSet::default();
        held.press(green);
        let chorded = resolve(&m, &held, &play(), &ev(red, ControlValue::Gate(true)));
        assert_eq!(
            chorded,
            vec![Action::Transport(crate::core::event::TransportCmd::Toggle)]
        );
    }

    #[test]
    fn a_chord_needs_every_one_of_its_modifiers() {
        let mut m = Mapping::default();
        let a = ControlId::Keyboard(KeyCode::A);
        let b = ControlId::Keyboard(KeyCode::B);
        let trigger = ControlId::Keyboard(KeyCode::C);
        m.add_chord(
            InputChord {
                held: vec![a, b],
                trigger,
            },
            Binding::Act(Action::OctaveShift(1)),
        );

        let mut only_one = HeldSet::default();
        only_one.press(a);
        assert!(resolve(
            &m,
            &only_one,
            &play(),
            &ev(trigger, ControlValue::Gate(true))
        )
        .is_empty());

        let mut both = HeldSet::default();
        both.press(a);
        both.press(b);
        assert_eq!(
            resolve(&m, &both, &play(), &ev(trigger, ControlValue::Gate(true))),
            vec![Action::OctaveShift(1)]
        );
    }

    #[test]
    fn a_chord_does_not_fire_on_release() {
        let mut m = Mapping::default();
        let modifier = ControlId::Keyboard(KeyCode::A);
        let trigger = ControlId::Keyboard(KeyCode::C);
        m.add_chord(
            InputChord {
                held: vec![modifier],
                trigger,
            },
            Binding::Act(Action::OctaveShift(1)),
        );
        let mut held = HeldSet::default();
        held.press(modifier);
        assert!(resolve(&m, &held, &play(), &ev(trigger, ControlValue::Gate(false))).is_empty());
    }

    #[test]
    fn one_control_may_carry_several_bindings() {
        let mut m = Mapping::default();
        let key = ControlId::Keyboard(KeyCode::A);
        m.insert(key, Binding::Note { semitone: 0 });
        m.insert(key, Binding::Note { semitone: 7 });
        let out = resolve(
            &m,
            &HeldSet::default(),
            &play(),
            &ev(key, ControlValue::Gate(true)),
        );
        assert_eq!(out.len(), 2);
        assert!(out.contains(&Action::NoteOn {
            note: 60,
            velocity: 0.8
        }));
        assert!(out.contains(&Action::NoteOn {
            note: 67,
            velocity: 0.8
        }));
    }

    #[test]
    fn releasing_a_control_that_was_never_held_is_harmless() {
        let mut held = HeldSet::default();
        held.release(ControlId::Keyboard(KeyCode::A));
        assert_eq!(held.len(), 0);
    }

    #[test]
    fn the_exponential_curve_keeps_the_endpoints_and_bends_the_middle() {
        assert!((Curve::Exponential.apply(0.0) - 0.0).abs() < 1e-6);
        assert!((Curve::Exponential.apply(1.0) - 1.0).abs() < 1e-6);
        assert!(Curve::Exponential.apply(0.5) < 0.5);
    }
}
