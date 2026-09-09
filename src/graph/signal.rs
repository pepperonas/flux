#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SignalType {
    Audio,
    Control,
    Gate,
    Trigger,
    Clock,
}

impl SignalType {
    pub fn name(self) -> &'static str {
        match self {
            SignalType::Audio => "AUDIO",
            SignalType::Control => "CONTROL",
            SignalType::Gate => "GATE",
            SignalType::Trigger => "TRIGGER",
            SignalType::Clock => "CLOCK",
        }
    }
}

/// Which connections are meaningful.
///
/// The refusals carry more design than the permissions. `Control -> Gate` is
/// invalid because turning a continuous value into a gate needs a threshold,
/// and choosing that threshold belongs to the player, not to a guess. `Trigger
/// -> Gate` is invalid because a trigger has no duration, so nothing could
/// decide when the gate closes.
pub fn can_connect(from: SignalType, to: SignalType) -> bool {
    use SignalType::*;
    matches!(
        (from, to),
        (Audio, Audio)
            | (Audio, Control)
            | (Control, Audio)
            | (Control, Control)
            | (Gate, Gate)
            | (Gate, Control)
            | (Gate, Trigger)
            | (Trigger, Trigger)
            | (Clock, Clock)
            | (Clock, Trigger)
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use SignalType::*;

    #[test]
    fn audio_flows_into_audio_and_can_modulate() {
        assert!(can_connect(Audio, Audio));
        assert!(can_connect(Audio, Control)); // audio-rate modulation is legitimate
    }

    #[test]
    fn control_is_a_signal_too() {
        assert!(can_connect(Control, Control));
        assert!(can_connect(Control, Audio));
    }

    #[test]
    fn a_gate_can_become_a_trigger_but_not_the_other_way() {
        // A gate has a rising edge, so it can produce a trigger. A trigger has
        // no duration, so it cannot produce a gate - there would be nothing to
        // decide when the gate closes.
        assert!(can_connect(Gate, Trigger));
        assert!(!can_connect(Trigger, Gate));
    }

    #[test]
    fn a_continuous_value_cannot_become_a_gate() {
        // Turning a continuous value into a gate needs a threshold, and that is
        // a decision the player must make explicitly with a comparator module -
        // not one FLUX guesses on their behalf.
        assert!(!can_connect(Control, Gate));
        assert!(!can_connect(Audio, Gate));
    }

    #[test]
    fn clock_drives_triggers_and_clocks_only() {
        assert!(can_connect(Clock, Trigger));
        assert!(can_connect(Clock, Clock));
        assert!(!can_connect(Clock, Audio));
        assert!(!can_connect(Clock, Control));
    }

    #[test]
    fn audio_is_never_a_clock() {
        assert!(!can_connect(Audio, Clock));
        assert!(!can_connect(Control, Clock));
        assert!(!can_connect(Gate, Clock));
    }
}
