use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use midir::{MidiInput, MidiInputConnection, MidiOutput, MidiOutputConnection};

use crate::core::event::{Action, LoopCmd};
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

fn pad_action(message: &[u8]) -> Option<Action> {
    let [status, note, value, ..] = message else {
        return None;
    };
    if status & 0xF0 != 0x90 || status & 0x0F != 9 || *value == 0 {
        return None;
    }
    let command = match *note {
        36..=39 => LoopCmd::Select(*note - 36),
        40 => LoopCmd::ToggleRecord,
        41 => LoopCmd::Clear,
        42 => LoopCmd::Undo,
        43 => LoopCmd::Mute,
        44 => return Some(Action::Transport(crate::core::event::TransportCmd::Toggle)),
        45 => return Some(Action::Transport(crate::core::event::TransportCmd::Stop)),
        46 => return Some(Action::Transport(crate::core::event::TransportCmd::Play)),
        _ => return None,
    };
    Some(Action::LoopControl(command))
}

pub fn pad_rgb_sysex(pad: u8, red: u8, green: u8, blue: u8) -> Vec<u8> {
    vec![
        0xF0,
        0x00,
        0x20,
        0x29,
        0x02,
        0x13,
        0x01,
        0x43,
        pad.min(127),
        red.min(127),
        green.min(127),
        blue.min(127),
        0xF7,
    ]
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
    outputs: Vec<MidiOutputConnection>,
    last_feedback: Option<(u32, usize, bool)>,
    next_clock_sample: u64,
}

struct MidiRuntime {
    telemetry: Arc<Telemetry>,
    messages: Arc<AtomicU64>,
    last_message: Arc<AtomicU64>,
}

impl MidiSource {
    pub fn visible_ports() -> Option<Vec<String>> {
        let input = MidiInput::new("FLUX MIDI discovery").ok()?;
        Some(
            input
                .ports()
                .iter()
                .filter_map(|port| input.port_name(port).ok())
                .collect(),
        )
    }

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
        let Some(names) = Self::visible_ports() else {
            log::warn!("MIDI input unavailable during port discovery");
            return MidiSource::default();
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
                    if let Some(action) = pad_action(message) {
                        runtime.telemetry.push_command(AudioCommand::Act(action));
                    } else if let Some(action) = note_action(message) {
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
        let mut outputs = Vec::new();
        if let Ok(discovery) = MidiOutput::new("FLUX Launchkey feedback") {
            for port in discovery.ports() {
                let Ok(name) = discovery.port_name(&port) else {
                    continue;
                };
                if !name.to_ascii_lowercase().contains("daw") {
                    continue;
                }
                let Ok(output) = MidiOutput::new("FLUX Launchkey feedback") else {
                    continue;
                };
                match output.connect(&port, "FLUX Launchkey DAW") {
                    Ok(mut connection) => {
                        let _ = connection.send(&[0x9F, 0x0C, 0x7F]);
                        let _ = connection.send(&[0xB6, 0x54, 0x01]);
                        // Stationary display, arrangement 3 (title + 2×4
                        // labels), followed by a short title. The remaining
                        // per-control fields are left untouched until their
                        // exact MK4 indices are verified on hardware.
                        let _ = connection
                            .send(&[0xF0, 0x00, 0x20, 0x29, 0x02, 0x13, 0x04, 0x20, 0x03, 0xF7]);
                        let mut title = vec![0xF0, 0x00, 0x20, 0x29, 0x02, 0x13, 0x06, 0x20, 0x00];
                        title.extend_from_slice(b"FLUX");
                        title.push(0xF7);
                        let _ = connection.send(&title);
                        for (field, label) in [
                            "BRIGHT", "DARK", "WET", "DRY", "ENERGY", "CHAOS", "DENSITY", "SPACE",
                        ]
                        .into_iter()
                        .enumerate()
                        {
                            let mut text = vec![
                                0xF0,
                                0x00,
                                0x20,
                                0x29,
                                0x02,
                                0x13,
                                0x06,
                                0x20,
                                (field + 1) as u8,
                            ];
                            text.extend_from_slice(label.as_bytes());
                            text.push(0xF7);
                            let _ = connection.send(&text);
                        }
                        log::info!("MIDI feedback connected: {name}");
                        outputs.push(connection);
                    }
                    Err(err) => log::warn!("could not connect MIDI output {name}: {err}"),
                }
            }
        }
        MidiSource {
            _connections: connections,
            ports: connected,
            messages,
            last_message,
            outputs,
            last_feedback: None,
            next_clock_sample: 0,
        }
    }

    pub fn messages(&self) -> u64 {
        self.messages.load(Ordering::Relaxed)
    }

    pub fn feedback_outputs(&self) -> usize {
        self.outputs.len()
    }

    pub fn last_message(&self) -> Option<u64> {
        (self.messages() > 0).then(|| self.last_message.load(Ordering::Relaxed))
    }

    pub fn ports_changed(&self) -> bool {
        Self::visible_ports().is_some_and(|ports| ports != self.ports)
    }

    pub fn render_feedback(
        &mut self,
        track_states: u32,
        active_track: usize,
        playing: bool,
        sample_pos: u64,
        bpm: f32,
        sample_rate: f32,
    ) {
        let state = (track_states, active_track, playing);
        if self.last_feedback != Some(state) {
            self.last_feedback = Some(state);
            for connection in &mut self.outputs {
                for track in 0..4u8 {
                    let code = ((track_states >> (track * 2)) & 0b11) as u8;
                    let velocity = if code == 0 {
                        0
                    } else if track as usize == active_track {
                        127
                    } else {
                        64
                    };
                    let _ = connection.send(&[0x9A, 36 + track, velocity]);
                    let (mut red, mut green, mut blue) = match code {
                        1 => (127, 12, 12),
                        2 => (12, 110, 48),
                        3 => (127, 80, 12),
                        _ => (0, 0, 0),
                    };
                    if track as usize != active_track && code != 0 {
                        red /= 2;
                        green /= 2;
                        blue /= 2;
                    }
                    let _ = connection.send(&pad_rgb_sysex(track, red, green, blue));
                }
                let transport_note = if playing { 44 } else { 45 };
                let _ = connection.send(&[0x9A, transport_note, 127]);
            }
        }
        if playing && bpm > 0.0 && sample_rate > 0.0 {
            let samples_per_clock = (sample_rate as f64 * 60.0 / (bpm as f64 * 24.0))
                .round()
                .max(1.0) as u64;
            if self.next_clock_sample == 0 {
                self.next_clock_sample = sample_pos;
            }
            let mut sent = 0;
            while sample_pos >= self.next_clock_sample && sent < 8 {
                for connection in &mut self.outputs {
                    let _ = connection.send(&[0xF8]);
                }
                self.next_clock_sample = self.next_clock_sample.saturating_add(samples_per_clock);
                sent += 1;
            }
        } else {
            self.next_clock_sample = 0;
        }
    }
}

impl Drop for MidiSource {
    fn drop(&mut self) {
        for connection in &mut self.outputs {
            let _ = connection.send(&[0xB6, 0x54, 0x00]);
            let _ = connection.send(&[0x9F, 0x0C, 0x00]);
        }
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

    #[test]
    fn channel_ten_pads_control_the_looper() {
        assert_eq!(
            pad_action(&[0x99, 36, 127]),
            Some(Action::LoopControl(LoopCmd::Select(0)))
        );
        assert_eq!(
            pad_action(&[0x99, 40, 127]),
            Some(Action::LoopControl(LoopCmd::ToggleRecord))
        );
        assert_eq!(
            pad_action(&[0x99, 44, 127]),
            Some(Action::Transport(crate::core::event::TransportCmd::Toggle))
        );
        assert_eq!(pad_action(&[0x99, 36, 0]), None);
    }

    #[test]
    fn pad_rgb_sysex_uses_the_mini_mk4_product_id() {
        assert_eq!(
            pad_rgb_sysex(3, 255, 32, 7),
            vec![0xF0, 0x00, 0x20, 0x29, 0x02, 0x13, 0x01, 0x43, 3, 127, 32, 7, 0xF7]
        );
    }
}
