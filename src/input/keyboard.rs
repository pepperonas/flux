use crate::core::event::{Action, ControlEvent, ControlId, ControlValue, KeyCode, TransportCmd};
use crate::engine::telemetry::{AudioCommand, Telemetry};
use crate::input::mapping::{resolve, Binding, HeldSet, Mapping, PlayState};

/// Translate a windowing key into a FLUX key.
///
/// This is the only place `egui::Key` appears outside the interface, which is
/// what keeps `core` free of interface crates.
pub fn from_egui(key: egui::Key) -> Option<KeyCode> {
    use egui::Key as E;
    Some(match key {
        E::A => KeyCode::A, E::B => KeyCode::B, E::C => KeyCode::C, E::D => KeyCode::D,
        E::E => KeyCode::E, E::F => KeyCode::F, E::G => KeyCode::G, E::H => KeyCode::H,
        E::I => KeyCode::I, E::J => KeyCode::J, E::K => KeyCode::K, E::L => KeyCode::L,
        E::M => KeyCode::M, E::N => KeyCode::N, E::O => KeyCode::O, E::P => KeyCode::P,
        E::Q => KeyCode::Q, E::R => KeyCode::R, E::S => KeyCode::S, E::T => KeyCode::T,
        E::U => KeyCode::U, E::V => KeyCode::V, E::W => KeyCode::W, E::X => KeyCode::X,
        E::Y => KeyCode::Y, E::Z => KeyCode::Z,
        E::Num1 => KeyCode::Num1, E::Num2 => KeyCode::Num2, E::Num3 => KeyCode::Num3,
        E::Num4 => KeyCode::Num4, E::Num5 => KeyCode::Num5, E::Num6 => KeyCode::Num6,
        E::Num7 => KeyCode::Num7, E::Num8 => KeyCode::Num8, E::Num9 => KeyCode::Num9,
        E::Num0 => KeyCode::Num0,
        E::Space => KeyCode::Space,
        E::Comma => KeyCode::Comma, E::Period => KeyCode::Period,
        E::Minus => KeyCode::Minus, E::Plus => KeyCode::Plus,
        E::Slash => KeyCode::Slash, E::Backslash => KeyCode::Backslash,
        _ => return None,
    })
}

/// The layout is chosen by physical position, so it behaves the same on QWERTZ
/// and QWERTY: the lower row is the white keys, the upper row the black ones,
/// arranged as they sit on a piano.
const NOTE_KEYS: [(KeyCode, i8); 13] = [
    (KeyCode::A, 0),  (KeyCode::W, 1),  (KeyCode::S, 2),  (KeyCode::E, 3),
    (KeyCode::D, 4),  (KeyCode::F, 5),  (KeyCode::T, 6),  (KeyCode::G, 7),
    (KeyCode::Y, 8),  (KeyCode::H, 9),  (KeyCode::U, 10), (KeyCode::J, 11),
    (KeyCode::K, 12),
];

pub fn default_mapping() -> Mapping {
    let mut m = Mapping::default();
    for (key, semitone) in NOTE_KEYS {
        m.insert(ControlId::Keyboard(key), Binding::Note { semitone });
    }
    m.insert(ControlId::Keyboard(KeyCode::Z), Binding::Act(Action::OctaveShift(-1)));
    m.insert(ControlId::Keyboard(KeyCode::X), Binding::Act(Action::OctaveShift(1)));
    m.insert(ControlId::Keyboard(KeyCode::C), Binding::Act(Action::VelocityShift(-0.1)));
    m.insert(ControlId::Keyboard(KeyCode::V), Binding::Act(Action::VelocityShift(0.1)));
    m.insert(ControlId::Keyboard(KeyCode::Space), Binding::Act(Action::Transport(TransportCmd::Toggle)));
    m
}

/// Live keyboard state: which controls are down, and the octave/velocity
/// those presses are read against. Lives on the same thread that calls
/// `pump` (egui's UI thread in this app - there is no separate "input
/// thread" here, only a separate real-time audio thread that this struct
/// never touches directly).
#[derive(Default)]
pub struct KeyboardSource {
    pub held: HeldSet,
    pub play: PlayState,

    /// How many currently-sounding keyboard notes fall in each pitch class,
    /// indexed 0..12. This is what the performance view's note blocks read.
    ///
    /// It is *not* `AudioEngine::active_notes()` - that bitmask lives inside
    /// the audio callback closure (see `engine::host::AudioHost::start`), and
    /// there is no path from there to this struct without adding a field to
    /// `Telemetry` and touching `src/audio/engine.rs`, both out of bounds for
    /// this task. This is therefore "what the keyboard has told the engine to
    /// play" rather than "what the engine is still sounding": it goes dark the
    /// instant a `NoteOff` is sent, not when that voice's release tail
    /// actually finishes, and it knows nothing of voice stealing. Counted
    /// rather than a plain bitmask because two different keys - e.g. `A` and
    /// `K`, an octave apart - can share a pitch class, and releasing one must
    /// not blank a class the other key is still holding.
    sounding_counts: [u8; 12],
}

impl KeyboardSource {
    /// Whether an action belongs to the audio thread. Octave and velocity are
    /// input-thread state; forwarding them would be a command the engine has to
    /// ignore, and a reader would rightly wonder why.
    pub fn goes_to_audio(action: &Action) -> bool {
        !matches!(action, Action::OctaveShift(_) | Action::VelocityShift(_))
    }

    pub fn apply_local(&mut self, action: Action) {
        match action {
            // Clamped: an octave beyond hearing and a velocity of zero both
            // present as "the keyboard stopped working".
            Action::OctaveShift(d) => self.play.octave = (self.play.octave + d).clamp(-1, 8),
            Action::VelocityShift(d) => self.play.velocity = (self.play.velocity + d).clamp(0.1, 1.0),
            _ => {}
        }
    }

    /// Pitch classes the keyboard currently believes are sounding, one bit
    /// per semitone - see the `sounding_counts` field doc for exactly what
    /// this does and does not reflect.
    pub fn active_notes(&self) -> u16 {
        self.sounding_counts
            .iter()
            .enumerate()
            .filter(|&(_, &count)| count > 0)
            .fold(0u16, |mask, (i, _)| mask | (1 << i))
    }

    fn track_note(&mut self, action: &Action) {
        match *action {
            Action::NoteOn { note, .. } => {
                self.sounding_counts[(note % 12) as usize] =
                    self.sounding_counts[(note % 12) as usize].saturating_add(1);
            }
            Action::NoteOff { note } => {
                self.sounding_counts[(note % 12) as usize] =
                    self.sounding_counts[(note % 12) as usize].saturating_sub(1);
            }
            _ => {}
        }
    }

    /// Process one physical key transition, already translated to a FLUX
    /// `KeyCode`: update `held`, resolve it against `mapping`, and route each
    /// resulting action to the audio thread or to local state. Factored out
    /// of `pump` so the keyboard-to-action wiring - the same wiring the
    /// octave/held-note interaction depends on - is exercisable without a
    /// live `egui::Context`.
    fn on_key(&mut self, code: KeyCode, pressed: bool, mapping: &Mapping, telemetry: &Telemetry) {
        let id = ControlId::Keyboard(code);

        if pressed {
            self.held.press(id);
        } else {
            self.held.release(id);
        }

        let event = ControlEvent {
            id,
            value: ControlValue::Gate(pressed),
            host_time_ns: 0,
        };
        for action in resolve(mapping, &mut self.held, &self.play, &event) {
            self.track_note(&action);
            if Self::goes_to_audio(&action) {
                telemetry.push_command(AudioCommand::Act(action));
            } else {
                self.apply_local(action);
            }
        }
    }

    /// Read this frame's key events and turn them into engine commands.
    pub fn pump(&mut self, ctx: &egui::Context, mapping: &Mapping, telemetry: &Telemetry) {
        let events: Vec<(egui::Key, bool, bool)> = ctx.input(|i| {
            i.events
                .iter()
                .filter_map(|e| match e {
                    egui::Event::Key { key, pressed, repeat, .. } => Some((*key, *pressed, *repeat)),
                    _ => None,
                })
                .collect()
        });

        for (key, pressed, repeat) in events {
            // The operating system repeats a held key. A synth must not restart
            // the note; the key is still down and the note is still sounding.
            if repeat {
                continue;
            }
            let Some(code) = from_egui(key) else { continue };
            self.on_key(code, pressed, mapping, telemetry);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::event::{Action, ControlId, ControlValue, ControlEvent, KeyCode};
    use crate::input::mapping::{resolve, HeldSet, PlayState};

    fn press(key: KeyCode) -> ControlEvent {
        ControlEvent { id: ControlId::Keyboard(key), value: ControlValue::Gate(true), host_time_ns: 0 }
    }

    #[test]
    fn the_home_row_plays_a_chromatic_scale_from_c() {
        let m = default_mapping();
        let play = PlayState { octave: 4, velocity: 1.0 };
        let expected = [
            (KeyCode::A, 60), (KeyCode::W, 61), (KeyCode::S, 62), (KeyCode::E, 63),
            (KeyCode::D, 64), (KeyCode::F, 65), (KeyCode::T, 66), (KeyCode::G, 67),
            (KeyCode::Y, 68), (KeyCode::H, 69), (KeyCode::U, 70), (KeyCode::J, 71),
            (KeyCode::K, 72),
        ];
        for (key, note) in expected {
            let out = resolve(&m, &mut HeldSet::default(), &play, &press(key));
            assert_eq!(out, vec![Action::NoteOn { note, velocity: 1.0 }], "{key:?} played the wrong note");
        }
    }

    #[test]
    fn the_black_keys_sit_where_a_piano_would_put_them() {
        // W, E, T, Y, U are the sharps. If this drifts, the layout stops being
        // learnable by anyone who has seen a keyboard.
        let m = default_mapping();
        let play = PlayState { octave: 4, velocity: 1.0 };
        for key in [KeyCode::W, KeyCode::E, KeyCode::T, KeyCode::Y, KeyCode::U] {
            let out = resolve(&m, &mut HeldSet::default(), &play, &press(key));
            let Action::NoteOn { note, .. } = out[0] else { panic!("expected a note") };
            let pitch_class = note % 12;
            assert!([1, 3, 6, 8, 10].contains(&pitch_class),
                    "{key:?} produced pitch class {pitch_class}, which is a white key");
        }
    }

    #[test]
    fn the_transport_and_octave_keys_are_bound() {
        let m = default_mapping();
        let play = PlayState::default();
        let space = resolve(&m, &mut HeldSet::default(), &play, &press(KeyCode::Space));
        assert_eq!(space, vec![Action::Transport(crate::core::event::TransportCmd::Toggle)]);
        let down = resolve(&m, &mut HeldSet::default(), &play, &press(KeyCode::Z));
        assert_eq!(down, vec![Action::OctaveShift(-1)]);
        let up = resolve(&m, &mut HeldSet::default(), &play, &press(KeyCode::X));
        assert_eq!(up, vec![Action::OctaveShift(1)]);
    }

    #[test]
    fn no_key_is_bound_twice() {
        // Two meanings on one key is a mapping bug that presents as a mysterious
        // extra note rather than as an error.
        let m = default_mapping();
        for (id, bindings) in &m.bindings {
            assert_eq!(bindings.len(), 1, "{id:?} has {} bindings", bindings.len());
        }
    }

    #[test]
    fn octave_changes_are_clamped_to_a_playable_range() {
        let mut s = KeyboardSource::default();
        for _ in 0..50 { s.apply_local(Action::OctaveShift(1)); }
        assert!(s.play.octave <= 8, "octave ran away to {}", s.play.octave);
        for _ in 0..50 { s.apply_local(Action::OctaveShift(-1)); }
        assert!(s.play.octave >= -1, "octave ran away to {}", s.play.octave);
    }

    #[test]
    fn velocity_changes_stay_within_the_usable_range() {
        let mut s = KeyboardSource::default();
        for _ in 0..50 { s.apply_local(Action::VelocityShift(0.1)); }
        assert!(s.play.velocity <= 1.0);
        for _ in 0..50 { s.apply_local(Action::VelocityShift(-0.1)); }
        assert!(s.play.velocity >= 0.1, "velocity reached {} - silent keys look broken", s.play.velocity);
    }

    #[test]
    fn actions_handled_locally_are_not_forwarded_to_audio() {
        // Octave and velocity live on the input thread. Sending them to the
        // engine would be a command it must ignore, and a reader would wonder why.
        assert!(!KeyboardSource::goes_to_audio(&Action::OctaveShift(1)));
        assert!(!KeyboardSource::goes_to_audio(&Action::VelocityShift(0.1)));
        assert!(KeyboardSource::goes_to_audio(&Action::NoteOn { note: 60, velocity: 1.0 }));
        assert!(KeyboardSource::goes_to_audio(&Action::SetMacro {
            macro_id: crate::core::ids::MacroId(0), value: 0.5 }));
    }

    // --- KeyboardSource::pump / HeldSet wiring -----------------------------
    //
    // `resolve` takes `&mut HeldSet` so a `Binding::Note` release can read
    // back the note its press actually started, rather than recomputing one
    // from whatever `PlayState` holds by the time the release arrives (see
    // `mapping::resolve`'s doc comment and
    // `mapping::a_held_note_survives_an_octave_shift_and_releases_the_note_it_started`,
    // which pins this at the `resolve` level directly). The tests below pin
    // the same guarantee one layer up, at `KeyboardSource`, which is the
    // level that would actually break if `pump` ever passed a fresh
    // `HeldSet::default()` instead of `&mut self.held` - the default
    // mapping binds Z/X (octave) and A-K (notes) on the very same keyboard,
    // so this is one keystroke away in ordinary play, not a contrived case.

    fn key_event(key: egui::Key, pressed: bool) -> egui::Event {
        // `repeat` is deliberately always `false` here: egui's own
        // `InputState::begin_pass` recomputes it from whether the key is
        // already tracked as down, overwriting whatever an integration (or a
        // test) supplies. Setting it truthfully is therefore not this test's
        // job to get right.
        egui::Event::Key {
            key,
            physical_key: None,
            pressed,
            repeat: false,
            modifiers: egui::Modifiers::default(),
        }
    }

    /// Feed one simulated frame's worth of key events through `pump`. Each
    /// call is a distinct frame on the same `egui::Context`, so the
    /// context's own key-repeat bookkeeping (see `key_event`) carries over
    /// exactly as it would across real frames.
    fn run_frame(
        ctx: &egui::Context,
        source: &mut KeyboardSource,
        mapping: &Mapping,
        telemetry: &Telemetry,
        events: Vec<egui::Event>,
    ) {
        let raw = egui::RawInput { events, ..Default::default() };
        let _ = ctx.run(raw, |ctx| source.pump(ctx, mapping, telemetry));
    }

    #[test]
    fn releasing_a_held_note_after_an_octave_shift_stops_the_note_that_was_started() {
        let m = default_mapping();
        let telemetry = Telemetry::new(8);
        let mut source = KeyboardSource::default();
        let ctx = egui::Context::default();

        // Press A: starts middle C (octave 4, the default) and forwards it.
        run_frame(&ctx, &mut source, &m, &telemetry, vec![key_event(egui::Key::A, true)]);
        assert_eq!(
            telemetry.commands.pop(),
            Some(AudioCommand::Act(Action::NoteOn { note: 60, velocity: source.play.velocity })),
            "A should have started note 60"
        );

        // Press X while A is still held: shifts the octave. This must be
        // handled locally, not forwarded, and A's note must keep sounding.
        run_frame(&ctx, &mut source, &m, &telemetry, vec![key_event(egui::Key::X, true)]);
        assert_eq!(source.play.octave, 5, "X should have shifted the octave up");
        assert!(
            telemetry.commands.pop().is_none(),
            "an octave shift must never reach the audio thread"
        );

        // Release A. It must turn off note 60 - the one actually sounding -
        // not note 72, which is what a fresh note_for(5, 0) would produce and
        // which was never turned on. A `pump` that passed a fresh `HeldSet`
        // instead of `&mut self.held` would strand note 60 forever and send
        // this wrong note-off instead.
        run_frame(&ctx, &mut source, &m, &telemetry, vec![key_event(egui::Key::A, false)]);
        assert_eq!(
            telemetry.commands.pop(),
            Some(AudioCommand::Act(Action::NoteOff { note: 60 })),
            "releasing A must turn off the note it actually started"
        );
    }

    // --- active_notes --------------------------------------------------

    #[test]
    fn active_notes_lights_up_a_played_pitch_class_and_clears_on_release() {
        let m = default_mapping();
        let telemetry = Telemetry::new(8);
        let mut s = KeyboardSource::default();

        s.on_key(KeyCode::A, true, &m, &telemetry); // C
        assert_ne!(s.active_notes() & 1, 0, "C should be lit while A is held");

        s.on_key(KeyCode::A, false, &m, &telemetry);
        assert_eq!(s.active_notes(), 0, "C should go dark once A is released");
    }

    #[test]
    fn active_notes_stays_lit_while_another_key_of_the_same_pitch_class_is_still_held() {
        // A (semitone 0) and K (semitone 12) are both C, an octave apart.
        // Releasing one must not blank a pitch class the other is still
        // holding.
        let m = default_mapping();
        let telemetry = Telemetry::new(8);
        let mut s = KeyboardSource::default();

        s.on_key(KeyCode::A, true, &m, &telemetry);
        s.on_key(KeyCode::K, true, &m, &telemetry);
        s.on_key(KeyCode::A, false, &m, &telemetry);
        assert_ne!(s.active_notes() & 1, 0, "C should still be lit - K is still down");

        s.on_key(KeyCode::K, false, &m, &telemetry);
        assert_eq!(s.active_notes(), 0);
    }
}
