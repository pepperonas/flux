use crate::core::ids::{DeviceId, MacroId, MidiPortId, ParamId};

/// FLUX's own key identity.
///
/// Deliberately not `egui::Key`: `core` must stay free of interface crates, and
/// the mapping layer is where a windowing key becomes a FLUX control. Values are
/// physical positions, so the layout behaves the same on QWERTZ and QWERTY.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum KeyCode {
    A,
    B,
    C,
    D,
    E,
    F,
    G,
    H,
    I,
    J,
    K,
    L,
    M,
    N,
    O,
    P,
    Q,
    R,
    S,
    T,
    U,
    V,
    W,
    X,
    Y,
    Z,
    Num1,
    Num2,
    Num3,
    Num4,
    Num5,
    Num6,
    Num7,
    Num8,
    Num9,
    Num0,
    Space,
    Comma,
    Period,
    Minus,
    Plus,
    Slash,
    Backslash,
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum ControlValue {
    /// Held: a key, a fret, a pad.
    Gate(bool),
    /// Momentary impulse with no duration: a strum, a transport button.
    Trigger,
    /// Normalised 0.0..=1.0: a knob, a fader, whammy, tilt, a CC, pad pressure.
    Continuous(f32),
    /// Relative movement from an endless encoder.
    Delta(i32),
}

impl ControlValue {
    pub fn as_continuous(self) -> Option<f32> {
        match self {
            ControlValue::Continuous(v) => Some(v),
            _ => None,
        }
    }

    pub fn is_press(self) -> bool {
        matches!(self, ControlValue::Gate(true) | ControlValue::Trigger)
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum GuitarControl {
    Fret(u8),
    StrumUp,
    StrumDown,
    Whammy,
    Tilt,
    Start,
    Select,
    Dpad(u8),
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum MidiControl {
    Note(u8),
    Cc(u8),
    PolyAftertouch(u8),
    ChannelPressure,
    PitchBend,
}

/// The identity of one physical control, whatever produced it.
///
/// The audio engine never sees this type. That is the whole point: it cannot
/// tell a fret from a knob from a key, which is what keeps FLUX open to
/// hardware nobody has thought of yet.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum ControlId {
    Keyboard(KeyCode),
    Guitar {
        device: DeviceId,
        control: GuitarControl,
    },
    Midi {
        port: MidiPortId,
        channel: u8,
        control: MidiControl,
    },
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct ControlEvent {
    pub id: ControlId,
    pub value: ControlValue,
    /// Monotonic host time. Milestone M1a stamps commands at drain time; the
    /// field exists now so that turning on sub-block placement later is not a
    /// signature change through five modules.
    pub host_time_ns: u64,
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum TransportCmd {
    Play,
    Stop,
    Toggle,
    SetBpm(f32),
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LoopCmd {
    ToggleRecord,
    Clear,
    Undo,
    Mute,
    Select(u8),
}

/// What the musical engine is asked to do. Every variant is `Copy`, because
/// these travel through an allocation-free queue into the audio thread.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Action {
    NoteOn { note: u8, velocity: f32 },
    NoteOff { note: u8 },
    SetMacro { macro_id: MacroId, value: f32 },
    SetParam { target: ParamId, value: f32 },
    Transport(TransportCmd),
    LoopControl(LoopCmd),
    OctaveShift(i8),
    VelocityShift(f32),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::ids::MidiPortId;
    use std::collections::HashSet;

    #[test]
    fn control_ids_are_usable_as_map_keys() {
        // Learn mode depends on this: "remember the next ControlId" is only
        // three lines because every source produces the same hashable identity.
        let mut set = HashSet::new();
        set.insert(ControlId::Keyboard(KeyCode::A));
        set.insert(ControlId::Keyboard(KeyCode::A));
        assert_eq!(set.len(), 1);
    }

    #[test]
    fn the_midi_channel_is_part_of_the_identity() {
        // The Launchkey uses the channel as a namespace: pads on 10, encoders
        // on 16, encoder touch on 15. Folding it away would merge controls that
        // are physically different things.
        let pad = ControlId::Midi {
            port: MidiPortId(0),
            channel: 10,
            control: MidiControl::Note(36),
        };
        let key = ControlId::Midi {
            port: MidiPortId(0),
            channel: 1,
            control: MidiControl::Note(36),
        };
        assert_ne!(pad, key);
    }

    #[test]
    fn the_same_control_on_two_ports_is_two_controls() {
        let a = ControlId::Midi {
            port: MidiPortId(0),
            channel: 1,
            control: MidiControl::Cc(17),
        };
        let b = ControlId::Midi {
            port: MidiPortId(1),
            channel: 1,
            control: MidiControl::Cc(17),
        };
        assert_ne!(a, b);
    }

    #[test]
    fn a_gate_going_down_is_a_press_and_going_up_is_not() {
        assert!(ControlValue::Gate(true).is_press());
        assert!(!ControlValue::Gate(false).is_press());
        assert!(ControlValue::Trigger.is_press());
    }

    #[test]
    fn only_continuous_values_read_as_continuous() {
        assert_eq!(ControlValue::Continuous(0.5).as_continuous(), Some(0.5));
        assert_eq!(ControlValue::Gate(true).as_continuous(), None);
        assert_eq!(ControlValue::Trigger.as_continuous(), None);
        assert_eq!(ControlValue::Delta(3).as_continuous(), None);
    }

    #[test]
    fn every_action_is_copy() {
        // The audio command queue is allocation-free, so nothing reachable from
        // an Action may own heap memory. This test fails to compile if that breaks.
        fn assert_copy<T: Copy>() {}
        assert_copy::<Action>();
        assert_copy::<ControlEvent>();
        assert_copy::<ControlId>();
    }
}
