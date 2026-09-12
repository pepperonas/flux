use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use midir::{MidiInput, MidiInputConnection};

use crate::core::event::Action;
use crate::core::ids::MacroId;
use crate::engine::telemetry::{AudioCommand, Telemetry};

/// Turns ordinary channel-voice MIDI notes into the same actions the computer
/// keyboard emits. The audio engine therefore still has no MIDI type in scope.
fn note_action(message: &[u8]) -> Option<Action> {
    let [status, note, value, ..] = message else {
        return None;
    };
    match status & 0xF0 {
        0x90 if *value != 0 => Some(Action::NoteOn {
            note: *note,
            velocity: *value as f32 / 127.0,
        }),
        0x80 | 0x90 => Some(Action::NoteOff { note: *note }),
        _ => None,
    }
}

fn control_action(message: &[u8]) -> Option<Action> {
    let [status, controller, value, ..] = message else {
        return None;
    };
    if status & 0xF0 != 0xB0 || !(21..=28).contains(controller) {
        return None;
    }
    Some(Action::SetMacro {
        macro_id: MacroId(*controller - 21),
        value: *value as f32 / 127.0,
    })
}

/// Owns open CoreMIDI/ALSA/WinMM connections. Keeping this value alive keeps
/// each callback alive; disconnecting it on application shutdown releases the
/// ports cleanly.
#[derive(Default)]
pub struct MidiSource {
    _connections: Vec<MidiInputConnection<Arc<MidiRuntime>>>,
    pub ports: Vec<String>,
    messages: Arc<AtomicU64>,
    last_message: Arc<AtomicU64>,
}

struct MidiRuntime {
    telemetry: Arc<Telemetry>,
    messages: Arc<AtomicU64>,
    last_message: Arc<AtomicU64>,
}

impl MidiSource {
    /// Connect every currently visible MIDI input. A missing or unavailable
    /// port is logged and does not stop the instrument from opening.
    pub fn connect_all(telemetry: Arc<Telemetry>) -> MidiSource {
        let messages = Arc::new(AtomicU64::new(0));
        let last_message = Arc::new(AtomicU64::new(0));
        let runtime = Arc::new(MidiRuntime {
            telemetry,
            messages: Arc::clone(&messages),
            last_message: Arc::clone(&last_message),
        });
        let names: Vec<String> = match MidiInput::new("FLUX MIDI discovery") {
            Ok(input) => input
                .ports()
                .iter()
                .filter_map(|port| input.port_name(port).ok())
                .collect(),
            Err(err) => {
                log::warn!("MIDI input unavailable: {err}");
                return MidiSource::default();
            }
        };

        let mut connections = Vec::with_capacity(names.len());
        let mut connected = Vec::with_capacity(names.len());
        for name in names {
            let input = match MidiInput::new("FLUX MIDI input") {
                Ok(input) => input,
                Err(err) => {
                    log::warn!("could not create MIDI input for {name}: {err}");
                    continue;
                }
            };
            let Some(port) = input
                .ports()
                .into_iter()
                .find(|port| input.port_name(port).ok().as_deref() == Some(name.as_str()))
            else {
                continue;
            };
            let callback_name = format!("FLUX: {name}");
            match input.connect(
                &port,
                &callback_name,
                |_timestamp, message, runtime| {
                    runtime.messages.fetch_add(1, Ordering::Relaxed);
                    let packed = message
                        .iter()
                        .take(8)
                        .enumerate()
                        .fold(0u64, |value, (i, byte)| {
                            value | (u64::from(*byte) << (i * 8))
                        });
                    runtime.last_message.store(packed, Ordering::Relaxed);
                    if let Some(action) = note_action(message) {
                        runtime.telemetry.push_command(AudioCommand::Act(action));
                    } else if let Some(action) = control_action(message) {
                        runtime.telemetry.push_command(AudioCommand::Act(action));
                    }
                },
                Arc::clone(&runtime),
            ) {
                Ok(connection) => {
                    log::info!("MIDI input connected: {name}");
                    connected.push(name);
                    connections.push(connection);
                }
                Err(err) => log::warn!("could not connect MIDI input {name}: {err}"),
            }
        }
        MidiSource {
            _connections: connections,
            ports: connected,
            messages,
            last_message,
        }
    }

    pub fn messages(&self) -> u64 {
        self.messages.load(Ordering::Relaxed)
    }

    pub fn last_message(&self) -> Option<u64> {
        (self.messages() > 0).then(|| self.last_message.load(Ordering::Relaxed))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn note_on_keeps_the_midi_velocity() {
        assert_eq!(
            note_action(&[0x91, 60, 64]),
            Some(Action::NoteOn {
                note: 60,
                velocity: 64.0 / 127.0
            })
        );
    }

    #[test]
    fn note_on_with_zero_velocity_is_a_note_off() {
        assert_eq!(
            note_action(&[0x90, 60, 0]),
            Some(Action::NoteOff { note: 60 })
        );
    }

    #[test]
    fn unrelated_messages_do_not_reach_the_audio_engine() {
        assert_eq!(note_action(&[0xB0, 74, 127]), None);
        assert_eq!(note_action(&[0xF8]), None);
    }

    #[test]
    fn encoder_ccs_target_the_eight_macros() {
        assert_eq!(
            control_action(&[0xB0, 21, 127]),
            Some(Action::SetMacro {
                macro_id: MacroId(0),
                value: 1.0
            })
        );
        assert_eq!(
            control_action(&[0xB7, 28, 64]),
            Some(Action::SetMacro {
                macro_id: MacroId(7),
                value: 64.0 / 127.0
            })
        );
    }
}
