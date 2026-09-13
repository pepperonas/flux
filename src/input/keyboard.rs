use crate::core::event::{
    Action, ControlEvent, ControlId, ControlValue, KeyCode, LoopCmd, TransportCmd,
};
use crate::engine::telemetry::{AudioCommand, Telemetry};
use crate::input::mapping::{resolve, Binding, HeldSet, Mapping, PlayState};

/// Translate a windowing key into a FLUX key.
///
/// This is the only place `egui::Key` appears outside the interface, which is
/// what keeps `core` free of interface crates.
pub fn from_egui(key: egui::Key) -> Option<KeyCode> {
    use egui::Key as E;
    Some(match key {
        E::A => KeyCode::A,
        E::B => KeyCode::B,
        E::C => KeyCode::C,
        E::D => KeyCode::D,
        E::E => KeyCode::E,
        E::F => KeyCode::F,
        E::G => KeyCode::G,
        E::H => KeyCode::H,
        E::I => KeyCode::I,
        E::J => KeyCode::J,
        E::K => KeyCode::K,
        E::L => KeyCode::L,
        E::M => KeyCode::M,
        E::N => KeyCode::N,
        E::O => KeyCode::O,
        E::P => KeyCode::P,
        E::Q => KeyCode::Q,
        E::R => KeyCode::R,
        E::S => KeyCode::S,
        E::T => KeyCode::T,
        E::U => KeyCode::U,
        E::V => KeyCode::V,
        E::W => KeyCode::W,
        E::X => KeyCode::X,
        E::Y => KeyCode::Y,
        E::Z => KeyCode::Z,
        E::Num1 => KeyCode::Num1,
        E::Num2 => KeyCode::Num2,
        E::Num3 => KeyCode::Num3,
        E::Num4 => KeyCode::Num4,
        E::Num5 => KeyCode::Num5,
        E::Num6 => KeyCode::Num6,
        E::Num7 => KeyCode::Num7,
        E::Num8 => KeyCode::Num8,
        E::Num9 => KeyCode::Num9,
        E::Num0 => KeyCode::Num0,
        E::Space => KeyCode::Space,
        E::Comma => KeyCode::Comma,
        E::Period => KeyCode::Period,
        E::Minus => KeyCode::Minus,
        E::Plus => KeyCode::Plus,
        E::Slash => KeyCode::Slash,
        E::Backslash => KeyCode::Backslash,
        E::Escape => KeyCode::Escape,
        _ => return None,
    })
}

/// The layout is chosen by physical position, so it behaves the same on QWERTZ
/// and QWERTY: the lower row is the white keys, the upper row the black ones,
/// arranged as they sit on a piano.
const NOTE_KEYS: [(KeyCode, i8); 13] = [
    (KeyCode::A, 0),
    (KeyCode::W, 1),
    (KeyCode::S, 2),
    (KeyCode::E, 3),
    (KeyCode::D, 4),
    (KeyCode::F, 5),
    (KeyCode::T, 6),
    (KeyCode::G, 7),
    (KeyCode::Y, 8),
    (KeyCode::H, 9),
    (KeyCode::U, 10),
    (KeyCode::J, 11),
    (KeyCode::K, 12),
];

pub fn default_mapping() -> Mapping {
    let mut m = Mapping::default();
    for (key, semitone) in NOTE_KEYS {
        m.insert(ControlId::Keyboard(key), Binding::Note { semitone });
    }
    m.insert(
        ControlId::Keyboard(KeyCode::Z),
        Binding::Act(Action::OctaveShift(-1)),
    );
    m.insert(
        ControlId::Keyboard(KeyCode::X),
        Binding::Act(Action::OctaveShift(1)),
    );
    m.insert(
        ControlId::Keyboard(KeyCode::C),
        Binding::Act(Action::VelocityShift(-0.1)),
    );
    m.insert(
        ControlId::Keyboard(KeyCode::V),
        Binding::Act(Action::VelocityShift(0.1)),
    );
    m.insert(
        ControlId::Keyboard(KeyCode::Space),
        Binding::Act(Action::Transport(TransportCmd::Toggle)),
    );
    m.insert(
        ControlId::Keyboard(KeyCode::Escape),
        Binding::Act(Action::Panic),
    );
    m.insert(
        ControlId::Keyboard(KeyCode::R),
        Binding::Act(Action::LoopControl(LoopCmd::ToggleRecord)),
    );
    m.insert(
        ControlId::Keyboard(KeyCode::Comma),
        Binding::Act(Action::LoopControl(LoopCmd::Clear)),
    );
    m.insert(
        ControlId::Keyboard(KeyCode::Period),
        Binding::Act(Action::LoopControl(LoopCmd::Undo)),
    );
    m.insert(
        ControlId::Keyboard(KeyCode::Minus),
        Binding::Act(Action::LoopControl(LoopCmd::Mute)),
    );
    for (key, track) in [
        (KeyCode::Num1, 0),
        (KeyCode::Num2, 1),
        (KeyCode::Num3, 2),
        (KeyCode::Num4, 3),
    ] {
        m.insert(
            ControlId::Keyboard(key),
            Binding::Act(Action::LoopControl(LoopCmd::Select(track))),
        );
    }
    m
}

/// Live keyboard state: which controls are down, and the octave/velocity
/// those presses are read against. Lives on the same thread that calls
/// `pump` (egui's UI thread in this app - there is no separate "input
/// thread" here, only a separate real-time audio thread that this struct
/// never touches directly).
///
/// This does *not* track which notes are sounding. `AudioEngine::
/// active_notes()` is published to `Telemetry` from `render()` and read
/// from there instead (`engine::telemetry::Telemetry::active_notes`) - that
/// reflects what the audio thread is actually still ringing (including a
/// voice's release tail and any voice-stealing), which a count kept here
/// from `NoteOn`/`NoteOff` alone could not.
#[derive(Default)]
pub struct KeyboardSource {
    pub held: HeldSet,
    pub play: PlayState,
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
            Action::VelocityShift(d) => {
                self.play.velocity = (self.play.velocity + d).clamp(0.1, 1.0)
            }
            _ => {}
        }
    }

    /// Process one control transition: update `held`, resolve it against
    /// `mapping`, and route each resulting action to the audio thread or to
    /// local state. Factored out of `pump` so the keyboard-to-action wiring -
    /// the same wiring the octave/held-note interaction depends on - is
    /// exercisable without a live `egui::Context`.
    fn on_control(
        &mut self,
        id: ControlId,
        pressed: bool,
        mapping: &Mapping,
        telemetry: &Telemetry,
    ) {
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
            if Self::goes_to_audio(&action) {
                telemetry.push_command(AudioCommand::Act(action));
            } else {
                self.apply_local(action);
            }
        }
    }

    /// Let go of everything currently held, as though every one of those
    /// controls had been released.
    ///
    /// The operating system stops delivering key events to a window that has
    /// lost focus, and neither egui nor winit synthesises the releases that
    /// never arrive - egui's own `keys_down` simply keeps them. Without this,
    /// Cmd-Tab while holding a chord leaves it sounding with no note-off ever
    /// sent, and the instrument has no panic action to recover with
    /// (ARCHITECTURE SS3, which is why `resolve` records the started note in
    /// the first place).
    ///
    /// Every control is let go through the ordinary release path rather than
    /// by inventing note-offs here, so the notes turned off are exactly the
    /// ones the presses started - including after an octave change, which is
    /// the entire reason `HeldSet` remembers them.
    pub fn release_all(&mut self, mapping: &Mapping, telemetry: &Telemetry) {
        let held: Vec<ControlId> = self.held.iter().collect();
        for id in held {
            self.on_control(id, false, mapping, telemetry);
        }
    }

    /// Read this frame's input and turn it into engine commands.
    pub fn pump(&mut self, ctx: &egui::Context, mapping: &Mapping, telemetry: &Telemetry) {
        let (keys, focused) = ctx.input(|i| {
            let keys: Vec<(KeyCode, bool)> = i
                .events
                .iter()
                .filter_map(|e| match e {
                    egui::Event::Key {
                        key,
                        pressed,
                        repeat,
                        ..
                    } => {
                        // The operating system repeats a held key. A synth
                        // must not restart the note; the key is still down
                        // and the note is still sounding.
                        if *repeat {
                            return None;
                        }
                        from_egui(*key).map(|code| (code, *pressed))
                    }
                    _ => None,
                })
                .collect();
            (keys, i.focused)
        });

        for (code, pressed) in keys {
            self.on_control(ControlId::Keyboard(code), pressed, mapping, telemetry);
        }

        // Anything still held when a frame ends without window focus is let
        // go of. The end-of-frame state is used rather than the
        // `WindowFocused(false)` event so that the order events arrived in
        // cannot matter: a key press that lands in the same frame as the
        // focus loss is caught whichever side of it the press fell on, and a
        // window that loses and regains focus inside one frame keeps what it
        // was holding. It is also self-healing - any unfocused frame at all
        // clears a release that went missing for some other reason.
        if !focused {
            self.release_all(mapping, telemetry);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::event::{Action, ControlEvent, ControlId, ControlValue, KeyCode};
    use crate::input::mapping::{resolve, HeldSet, PlayState};

    fn press(key: KeyCode) -> ControlEvent {
        ControlEvent {
            id: ControlId::Keyboard(key),
            value: ControlValue::Gate(true),
            host_time_ns: 0,
        }
    }

    #[test]
    fn the_home_row_plays_a_chromatic_scale_from_c() {
        let m = default_mapping();
        let play = PlayState {
            octave: 4,
            velocity: 1.0,
        };
        let expected = [
            (KeyCode::A, 60),
            (KeyCode::W, 61),
            (KeyCode::S, 62),
            (KeyCode::E, 63),
            (KeyCode::D, 64),
            (KeyCode::F, 65),
            (KeyCode::T, 66),
            (KeyCode::G, 67),
            (KeyCode::Y, 68),
            (KeyCode::H, 69),
            (KeyCode::U, 70),
            (KeyCode::J, 71),
            (KeyCode::K, 72),
        ];
        for (key, note) in expected {
            let out = resolve(&m, &mut HeldSet::default(), &play, &press(key));
            assert_eq!(
                out,
                vec![Action::NoteOn {
                    note,
                    velocity: 1.0
                }],
                "{key:?} played the wrong note"
            );
        }
    }

    #[test]
    fn the_black_keys_sit_where_a_piano_would_put_them() {
        // W, E, T, Y, U are the sharps. If this drifts, the layout stops being
        // learnable by anyone who has seen a keyboard.
        let m = default_mapping();
        let play = PlayState {
            octave: 4,
            velocity: 1.0,
        };
        for key in [KeyCode::W, KeyCode::E, KeyCode::T, KeyCode::Y, KeyCode::U] {
            let out = resolve(&m, &mut HeldSet::default(), &play, &press(key));
            let Action::NoteOn { note, .. } = out[0] else {
                panic!("expected a note")
            };
            let pitch_class = note % 12;
            assert!(
                [1, 3, 6, 8, 10].contains(&pitch_class),
                "{key:?} produced pitch class {pitch_class}, which is a white key"
            );
        }
    }

    #[test]
    fn the_transport_and_octave_keys_are_bound() {
        let m = default_mapping();
        let play = PlayState::default();
        let space = resolve(&m, &mut HeldSet::default(), &play, &press(KeyCode::Space));
        assert_eq!(
            space,
            vec![Action::Transport(crate::core::event::TransportCmd::Toggle)]
        );
        let down = resolve(&m, &mut HeldSet::default(), &play, &press(KeyCode::Z));
        assert_eq!(down, vec![Action::OctaveShift(-1)]);
        let up = resolve(&m, &mut HeldSet::default(), &play, &press(KeyCode::X));
        assert_eq!(up, vec![Action::OctaveShift(1)]);
    }

    #[test]
    fn loop_controls_are_bound_to_visible_keyboard_keys() {
        let m = default_mapping();
        let play = PlayState::default();
        for (key, cmd) in [
            (KeyCode::R, LoopCmd::ToggleRecord),
            (KeyCode::Comma, LoopCmd::Clear),
            (KeyCode::Period, LoopCmd::Undo),
            (KeyCode::Minus, LoopCmd::Mute),
        ] {
            assert_eq!(
                resolve(&m, &mut HeldSet::default(), &play, &press(key)),
                vec![Action::LoopControl(cmd)]
            );
        }
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
        for _ in 0..50 {
            s.apply_local(Action::OctaveShift(1));
        }
        assert!(s.play.octave <= 8, "octave ran away to {}", s.play.octave);
        for _ in 0..50 {
            s.apply_local(Action::OctaveShift(-1));
        }
        assert!(s.play.octave >= -1, "octave ran away to {}", s.play.octave);
    }

    #[test]
    fn velocity_changes_stay_within_the_usable_range() {
        let mut s = KeyboardSource::default();
        for _ in 0..50 {
            s.apply_local(Action::VelocityShift(0.1));
        }
        assert!(s.play.velocity <= 1.0);
        for _ in 0..50 {
            s.apply_local(Action::VelocityShift(-0.1));
        }
        assert!(
            s.play.velocity >= 0.1,
            "velocity reached {} - silent keys look broken",
            s.play.velocity
        );
    }

    #[test]
    fn actions_handled_locally_are_not_forwarded_to_audio() {
        // Octave and velocity live on the input thread. Sending them to the
        // engine would be a command it must ignore, and a reader would wonder why.
        assert!(!KeyboardSource::goes_to_audio(&Action::OctaveShift(1)));
        assert!(!KeyboardSource::goes_to_audio(&Action::VelocityShift(0.1)));
        assert!(KeyboardSource::goes_to_audio(&Action::NoteOn {
            note: 60,
            velocity: 1.0
        }));
        assert!(KeyboardSource::goes_to_audio(&Action::SetMacro {
            macro_id: crate::core::ids::MacroId(0),
            value: 0.5
        }));
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
        // job to get right - and it's exactly what makes
        // `holding_a_key_down_does_not_retrigger_it_on_the_operating_systems_key_repeat`
        // below possible: `begin_pass` marks a second `pressed: true` for a
        // key that is still tracked as down as `repeat: true`, whatever this
        // helper passes in, which models real OS auto-repeat without a real
        // window.
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
        let raw = egui::RawInput {
            events,
            ..Default::default()
        };
        let _ = ctx.run(raw, |ctx| source.pump(ctx, mapping, telemetry));
    }

    #[test]
    fn releasing_a_held_note_after_an_octave_shift_stops_the_note_that_was_started() {
        let m = default_mapping();
        let telemetry = Telemetry::new(8);
        let mut source = KeyboardSource::default();
        let ctx = egui::Context::default();

        // Press A: starts middle C (octave 4, the default) and forwards it.
        run_frame(
            &ctx,
            &mut source,
            &m,
            &telemetry,
            vec![key_event(egui::Key::A, true)],
        );
        assert_eq!(
            telemetry.commands.pop(),
            Some(AudioCommand::Act(Action::NoteOn {
                note: 60,
                velocity: source.play.velocity
            })),
            "A should have started note 60"
        );

        // Press X while A is still held: shifts the octave. This must be
        // handled locally, not forwarded, and A's note must keep sounding.
        run_frame(
            &ctx,
            &mut source,
            &m,
            &telemetry,
            vec![key_event(egui::Key::X, true)],
        );
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
        run_frame(
            &ctx,
            &mut source,
            &m,
            &telemetry,
            vec![key_event(egui::Key::A, false)],
        );
        assert_eq!(
            telemetry.commands.pop(),
            Some(AudioCommand::Act(Action::NoteOff { note: 60 })),
            "releasing A must turn off the note it actually started"
        );
    }

    #[test]
    fn holding_a_key_down_does_not_retrigger_it_on_the_operating_systems_key_repeat() {
        // Two `pressed: true` events for the same key with no release between
        // them, across two separate frames on one `egui::Context`, is exactly
        // what a real window delivers while a key is held: `begin_pass` sees
        // the key is still in its own `keys_down` set from the first frame
        // and marks the second `repeat: true` on its own (see `key_event`).
        // The guard in `pump` must swallow that second press - a retriggered
        // note would restart the envelope's attack while the key is still
        // physically down, which is the stutter this exists to prevent.
        let m = default_mapping();
        let telemetry = Telemetry::new(8);
        let mut source = KeyboardSource::default();
        let ctx = egui::Context::default();

        run_frame(
            &ctx,
            &mut source,
            &m,
            &telemetry,
            vec![key_event(egui::Key::A, true)],
        );
        assert_eq!(
            telemetry.commands.pop(),
            Some(AudioCommand::Act(Action::NoteOn {
                note: 60,
                velocity: source.play.velocity
            })),
            "the genuine first press should have started note 60"
        );

        run_frame(
            &ctx,
            &mut source,
            &m,
            &telemetry,
            vec![key_event(egui::Key::A, true)],
        );
        assert!(
            telemetry.commands.pop().is_none(),
            "an OS key-repeat must not produce a second NoteOn"
        );
    }
    /// One frame in which the window is not focused. egui-winit sets both
    /// `RawInput::focused` and pushes `Event::WindowFocused`, so both are
    /// modelled here.
    fn run_unfocused_frame(
        ctx: &egui::Context,
        source: &mut KeyboardSource,
        mapping: &Mapping,
        telemetry: &Telemetry,
        mut events: Vec<egui::Event>,
    ) {
        events.push(egui::Event::WindowFocused(false));
        let raw = egui::RawInput {
            events,
            focused: false,
            ..Default::default()
        };
        let _ = ctx.run(raw, |ctx| source.pump(ctx, mapping, telemetry));
    }

    /// Every command the telemetry queue is holding, in arrival order.
    fn drain(telemetry: &Telemetry) -> Vec<AudioCommand> {
        let mut out = Vec::new();
        while let Some(c) = telemetry.commands.pop() {
            out.push(c);
        }
        out
    }

    #[test]
    fn losing_window_focus_releases_every_held_note() {
        // Cmd-Tab away from a held chord and the operating system simply
        // stops delivering key events; the releases never arrive, and egui
        // does not invent them - its own `keys_down` keeps the keys down
        // forever. Before this was handled, the chord droned on with no
        // note-off ever sent and no panic action to recover with.
        let m = default_mapping();
        let telemetry = Telemetry::new(32);
        let mut source = KeyboardSource::default();
        let ctx = egui::Context::default();

        run_frame(
            &ctx,
            &mut source,
            &m,
            &telemetry,
            vec![
                key_event(egui::Key::A, true),
                key_event(egui::Key::D, true),
                key_event(egui::Key::G, true),
            ],
        );
        let started = drain(&telemetry);
        assert_eq!(started.len(), 3, "the chord did not start: {started:?}");

        run_unfocused_frame(&ctx, &mut source, &m, &telemetry, vec![]);
        let stopped = drain(&telemetry);
        // The order is whatever the held set iterates in, which is arbitrary
        // and harmless - note-offs for distinct notes commute.
        let mut notes: Vec<u8> = stopped
            .iter()
            .map(|c| match c {
                AudioCommand::Act(Action::NoteOff { note }) => *note,
                other => panic!("focus loss produced {other:?}, not a note-off"),
            })
            .collect();
        notes.sort_unstable();
        assert_eq!(
            notes,
            vec![60, 64, 67],
            "focus loss left notes sounding with no note-off"
        );
        assert!(
            source.held.is_empty(),
            "the keys are still recorded as held after focus was lost"
        );
    }

    #[test]
    fn focus_loss_releases_the_note_that_was_started_not_the_one_the_octave_now_implies() {
        // The same guarantee `releasing_a_held_note_after_an_octave_shift_
        // stops_the_note_that_was_started` pins for a real release. It holds
        // here for free only because focus loss goes through the ordinary
        // release path; a hand-written note-off loop that recomputed the note
        // from the current octave would strand note 60 and turn off note 72,
        // which was never on.
        let m = default_mapping();
        let telemetry = Telemetry::new(32);
        let mut source = KeyboardSource::default();
        let ctx = egui::Context::default();

        run_frame(
            &ctx,
            &mut source,
            &m,
            &telemetry,
            vec![key_event(egui::Key::A, true)],
        );
        run_frame(
            &ctx,
            &mut source,
            &m,
            &telemetry,
            vec![key_event(egui::Key::X, true)],
        );
        assert_eq!(source.play.octave, 5);
        drain(&telemetry);

        run_unfocused_frame(&ctx, &mut source, &m, &telemetry, vec![]);
        assert_eq!(
            drain(&telemetry),
            vec![AudioCommand::Act(Action::NoteOff { note: 60 })],
            "focus loss must stop the note the press actually started"
        );
    }

    #[test]
    fn a_key_pressed_in_the_same_frame_as_the_focus_loss_does_not_survive_it() {
        // The window can lose focus in the same frame a key goes down, and
        // the two can arrive in either order. Reading the frame's focus state
        // after the keys have been processed, rather than acting on the
        // event where it happens to sit in the queue, means neither ordering
        // can leave a note that nothing will ever turn off.
        for press_first in [true, false] {
            let m = default_mapping();
            let telemetry = Telemetry::new(32);
            let mut source = KeyboardSource::default();
            let ctx = egui::Context::default();

            let mut events = vec![key_event(egui::Key::A, true)];
            if press_first {
                run_unfocused_frame(&ctx, &mut source, &m, &telemetry, events);
            } else {
                events.insert(0, egui::Event::WindowFocused(false));
                let raw = egui::RawInput {
                    events,
                    focused: false,
                    ..Default::default()
                };
                let _ = ctx.run(raw, |ctx| source.pump(ctx, &m, &telemetry));
            }

            assert_eq!(
                drain(&telemetry),
                vec![
                    AudioCommand::Act(Action::NoteOn {
                        note: 60,
                        velocity: source.play.velocity
                    }),
                    AudioCommand::Act(Action::NoteOff { note: 60 }),
                ],
                "press_first={press_first}: a key pressed as focus was lost \
                 was left sounding"
            );
            assert!(source.held.is_empty(), "press_first={press_first}");
        }
    }

    #[test]
    fn focus_lost_and_regained_inside_one_frame_keeps_what_was_held() {
        // The reason the end-of-frame state decides this rather than the
        // `WindowFocused(false)` event: acting on the event would drop a
        // chord the player never actually lost, because the window was
        // focused again before the frame was even drawn.
        let m = default_mapping();
        let telemetry = Telemetry::new(32);
        let mut source = KeyboardSource::default();
        let ctx = egui::Context::default();

        run_frame(
            &ctx,
            &mut source,
            &m,
            &telemetry,
            vec![key_event(egui::Key::A, true)],
        );
        drain(&telemetry);

        let raw = egui::RawInput {
            events: vec![
                egui::Event::WindowFocused(false),
                egui::Event::WindowFocused(true),
            ],
            focused: true,
            ..Default::default()
        };
        let _ = ctx.run(raw, |ctx| source.pump(ctx, &m, &telemetry));

        assert!(
            drain(&telemetry).is_empty(),
            "a chord was dropped over a focus flicker inside a single frame"
        );
        assert!(source.held.contains(&ControlId::Keyboard(KeyCode::A)));
    }

    #[test]
    fn regaining_focus_on_its_own_starts_nothing() {
        // The counterpart: `WindowFocused(true)` must not be mistaken for
        // input. The keys may still be physically down, but FLUX has already
        // let go of them, and resurrecting notes the player cannot see would
        // be worse than making them press again.
        let m = default_mapping();
        let telemetry = Telemetry::new(32);
        let mut source = KeyboardSource::default();
        let ctx = egui::Context::default();

        run_unfocused_frame(
            &ctx,
            &mut source,
            &m,
            &telemetry,
            vec![key_event(egui::Key::A, true)],
        );
        drain(&telemetry);
        run_frame(
            &ctx,
            &mut source,
            &m,
            &telemetry,
            vec![egui::Event::WindowFocused(true)],
        );
        assert!(drain(&telemetry).is_empty());
    }
}
