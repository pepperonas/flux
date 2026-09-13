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
    /// A note relative to the current octave. The octave is read at press
    /// time, and the concrete note that results is remembered in `HeldSet`
    /// (keyed by control and semitone) rather than only by the fact that the
    /// control is down. A later release reads that note back instead of
    /// recomputing it from whatever `PlayState` happens to hold by then --
    /// which is what lets the octave change while a note is held without
    /// stranding it: the release always turns off the note the press
    /// actually started, never the note the current octave would produce.
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

/// Which controls are currently held down, and which note (if any) each
/// control's press actually started. Lives on the input thread.
///
/// The two are tracked separately on purpose: a chord modifier is "held" but
/// never starts a note, and a single control can carry more than one
/// `Binding::Note` (see `one_control_may_carry_several_bindings`), so the
/// note memory is keyed by `(ControlId, semitone)` rather than by control
/// alone -- otherwise a second binding on the same key would overwrite the
/// first binding's remembered note.
#[derive(Clone, Default, Debug)]
pub struct HeldSet {
    held: HashSet<ControlId>,
    notes: HashMap<(ControlId, i8), u8>,
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

    /// The controls currently held, in no particular order.
    ///
    /// Order is genuinely arbitrary and safe to be: the only caller releases
    /// every one of them, and note-offs for distinct notes commute.
    pub fn iter(&self) -> impl Iterator<Item = ControlId> + '_ {
        self.held.iter().copied()
    }

    pub fn len(&self) -> usize {
        self.held.len()
    }

    pub fn is_empty(&self) -> bool {
        self.held.is_empty()
    }

    /// Record which concrete note a `(control, semitone)` press actually
    /// started, so a later release of the same control can turn off exactly
    /// that note regardless of what `PlayState` does in between.
    fn remember_note(&mut self, id: ControlId, semitone: i8, note: u8) {
        self.notes.insert((id, semitone), note);
    }

    /// Read back and forget the note a `(control, semitone)` press started,
    /// if one is on record.
    fn take_note(&mut self, id: ControlId, semitone: i8) -> Option<u8> {
        self.notes.remove(&(id, semitone))
    }

    fn has_note(&self, id: ControlId, semitone: i8) -> bool {
        self.notes.contains_key(&(id, semitone))
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
/// A deterministic function of its arguments: the same `mapping`, `held`,
/// `play` and `event` always produce the same actions and leave `held` in
/// the same resulting state. It never touches the audio thread, global
/// state or I/O -- `held` is the only thing it mutates, and only to record
/// which note a press actually started (see `Binding::Note`) so that a
/// later release can read it back rather than recompute a possibly-wrong
/// one. Every interaction rule in FLUX is a property of this function,
/// which is what makes them all assertable.
pub fn resolve(
    mapping: &Mapping,
    held: &mut HeldSet,
    play: &PlayState,
    event: &ControlEvent,
) -> Vec<Action> {
    // A chord takes precedence over the plain binding of the same control.
    // Without this, pressing a modified combination would fire both meanings.
    //
    if event.value.is_press() {
        for (chord, binding) in &mapping.chords {
            if chord.trigger == event.id && chord.held.iter().all(|id| held.contains(id)) {
                let mut out = Vec::new();
                emit(binding, play, event, &mut out, held);
                return out;
            }
        }
    }

    // A chord-bound note remembers the concrete note under its trigger and
    // semitone. That memory, rather than the current modifier state, identifies
    // the matching release: the modifier may already have been released.
    if matches!(event.value, ControlValue::Gate(false)) {
        for (chord, binding) in &mapping.chords {
            let Binding::Note { semitone } = *binding else {
                continue;
            };
            if chord.trigger == event.id && held.has_note(event.id, semitone) {
                let mut out = Vec::new();
                emit(binding, play, event, &mut out, held);
                return out;
            }
        }
    }

    let Some(bindings) = mapping.bindings.get(&event.id) else {
        return Vec::new();
    };

    let mut out = Vec::new();
    for binding in bindings {
        emit(binding, play, event, &mut out, held);
    }
    out
}

fn emit(
    binding: &Binding,
    play: &PlayState,
    event: &ControlEvent,
    out: &mut Vec<Action>,
    held: &mut HeldSet,
) {
    match *binding {
        Binding::Note { semitone } => match event.value {
            ControlValue::Gate(true) => {
                let note = note_for(play.octave, semitone);
                held.remember_note(event.id, semitone, note);
                out.push(Action::NoteOn {
                    note,
                    velocity: play.velocity,
                });
            }
            // Momentary: no matching release will ever follow, so there is
            // nothing worth remembering.
            ControlValue::Trigger => {
                out.push(Action::NoteOn {
                    note: note_for(play.octave, semitone),
                    velocity: play.velocity,
                });
            }
            ControlValue::Gate(false) => {
                // Release exactly the note the press started, not whatever
                // the current PlayState would produce -- the octave may have
                // changed while the control was held. A release with no
                // matching press on record falls back to a fresh
                // computation, which keeps it well-defined rather than
                // silently doing nothing.
                let note = held
                    .take_note(event.id, semitone)
                    .unwrap_or_else(|| note_for(play.octave, semitone));
                out.push(Action::NoteOff { note });
            }
            // A knob bound to a note has no sensible meaning; producing
            // nothing is better than inventing one.
            _ => {}
        },
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
            &mut HeldSet::default(),
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
            &mut HeldSet::default(),
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
            &mut HeldSet::default(),
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
            &mut HeldSet::default(),
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
            &mut HeldSet::default(),
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
            &mut HeldSet::default(),
            &play(),
            &ev(key, ControlValue::Gate(true)),
        );
        assert!(out.is_empty());
    }

    #[test]
    fn a_gate_bound_to_a_param_produces_nothing() {
        // Same rule as the macro case above, pinned separately: `Param` and
        // `Macro` are structurally identical continuous targets, and a fix
        // (or regression) in one arm says nothing about the other.
        let mut m = Mapping::default();
        let key = ControlId::Keyboard(KeyCode::Y);
        m.insert(
            key,
            Binding::Param {
                target: ParamId(9),
                depth: 1.0,
                curve: Curve::Linear,
            },
        );
        let out = resolve(
            &m,
            &mut HeldSet::default(),
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
            &mut HeldSet::default(),
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
            &mut HeldSet::default(),
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
        let chorded = resolve(&m, &mut held, &play(), &ev(red, ControlValue::Gate(true)));
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
            &mut only_one,
            &play(),
            &ev(trigger, ControlValue::Gate(true))
        )
        .is_empty());

        let mut both = HeldSet::default();
        both.press(a);
        both.press(b);
        assert_eq!(
            resolve(
                &m,
                &mut both,
                &play(),
                &ev(trigger, ControlValue::Gate(true))
            ),
            vec![Action::OctaveShift(1)]
        );
    }

    #[test]
    fn a_chord_does_not_fire_on_release() {
        let mut m = Mapping::default();
        let modifier = ControlId::Keyboard(KeyCode::A);
        let trigger = ControlId::Keyboard(KeyCode::C);
        // Bound to `Binding::Note`, not `Binding::Act`: `Act`'s own arm
        // re-checks `is_press()` independently, so a chord bound to it stays
        // silent on release even if the outer press guard below is removed,
        // and the test would not be exercising that guard at all. `Note`'s
        // Gate(false) arm produces a real NoteOff unconditionally, so this
        // only stays empty because the outer guard keeps the chord loop from
        // running on a release in the first place.
        m.add_chord(
            InputChord {
                held: vec![modifier],
                trigger,
            },
            Binding::Note { semitone: 0 },
        );
        let mut held = HeldSet::default();
        held.press(modifier);
        assert!(resolve(
            &m,
            &mut held,
            &play(),
            &ev(trigger, ControlValue::Gate(false))
        )
        .is_empty());
    }

    #[test]
    fn a_chord_note_releases_even_after_its_modifier() {
        let mut m = Mapping::default();
        let modifier = ControlId::Keyboard(KeyCode::A);
        let trigger = ControlId::Keyboard(KeyCode::C);
        m.add_chord(
            InputChord {
                held: vec![modifier],
                trigger,
            },
            Binding::Note { semitone: 7 },
        );
        let mut held = HeldSet::default();
        held.press(modifier);
        assert_eq!(
            resolve(
                &m,
                &mut held,
                &play(),
                &ev(trigger, ControlValue::Gate(true))
            ),
            vec![Action::NoteOn {
                note: 67,
                velocity: 0.8
            }]
        );
        held.release(modifier);
        assert_eq!(
            resolve(
                &m,
                &mut held,
                &play(),
                &ev(trigger, ControlValue::Gate(false))
            ),
            vec![Action::NoteOff { note: 67 }]
        );
    }

    #[test]
    fn one_control_may_carry_several_bindings() {
        let mut m = Mapping::default();
        let key = ControlId::Keyboard(KeyCode::A);
        m.insert(key, Binding::Note { semitone: 0 });
        m.insert(key, Binding::Note { semitone: 7 });
        let out = resolve(
            &m,
            &mut HeldSet::default(),
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

    #[test]
    fn a_held_note_survives_an_octave_shift_and_releases_the_note_it_started() {
        // The critical bug this pins: Binding::Note recomputed its note from
        // the *current* PlayState on release as well as on press, using
        // HeldSet only to know a control was down, never which note it had
        // actually started. Task 14 binds octave shift and notes on the same
        // keyboard sharing one mutable PlayState, so a keystroke between
        // press and release stranded the originally-sounding note forever.
        let mut m = Mapping::default();
        let key = ControlId::Keyboard(KeyCode::A);
        m.insert(key, Binding::Note { semitone: 0 });
        let mut held = HeldSet::default();

        // Press at octave 4 starts note 60.
        let press = resolve(
            &m,
            &mut held,
            &PlayState {
                octave: 4,
                velocity: 0.8,
            },
            &ev(key, ControlValue::Gate(true)),
        );
        assert_eq!(
            press,
            vec![Action::NoteOn {
                note: 60,
                velocity: 0.8
            }]
        );

        // The octave changes while the key is still physically held -- e.g.
        // the player pressed an octave-up key without releasing this one.
        let shifted = PlayState {
            octave: 6,
            velocity: 0.8,
        };

        // Releasing now must turn off note 60, the one that is actually
        // sounding -- not note 84, which is what note_for(6, 0) would give a
        // fresh computation, and which was never turned on.
        let release = resolve(&m, &mut held, &shifted, &ev(key, ControlValue::Gate(false)));
        assert_eq!(release, vec![Action::NoteOff { note: 60 }]);
    }
}
