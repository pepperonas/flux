use crate::core::event::Action;

/// What the audio thread is asked to do.
///
/// Every variant is `Copy`. If one ever needs owned data, that is a design
/// question to raise rather than a constraint to work around: the queue is
/// allocation-free and freeing on the audio thread is forbidden.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum AudioCommand {
    Act(Action),
    /// A continuous 440 Hz tone. Kept permanently as a diagnostic: it proves
    /// the whole path from device selection to speaker independently of whether
    /// any input is working.
    SetTestTone(bool),
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum EngineEvent {
    NoteStarted { note: u8 },
    NoteEnded { note: u8 },
    VoiceStolen { note: u8 },
}
