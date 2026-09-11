use crossbeam_queue::ArrayQueue;
use std::sync::atomic::{AtomicBool, AtomicU16, AtomicU32, AtomicU64, Ordering};
use std::sync::Arc;

pub use crate::engine::command::{AudioCommand, EngineEvent};

/// Shared between every thread. Continuous values live in atomics rather than
/// in the event queue: pushing a level meter at two hundred hertz would flood
/// the queue and starve real events.
pub struct Telemetry {
    pub commands: ArrayQueue<AudioCommand>,
    pub events: ArrayQueue<EngineEvent>,
    peak_milli: AtomicU32,
    active_voices: AtomicU32,
    active_notes: AtomicU16,
    sample_pos: AtomicU64,
    bpm_milli: AtomicU32,
    playing: AtomicBool,
    loop_pos: AtomicU64,
    loop_len: AtomicU64,
    track_states: AtomicU32,
    active_track: AtomicU32,
    dsp_load_permille: AtomicU32,
    dropped_commands: AtomicU64,
    dropped_events: AtomicU64,
    underruns: AtomicU64,
}

impl Telemetry {
    pub fn new(capacity: usize) -> Arc<Telemetry> {
        Arc::new(Telemetry {
            commands: ArrayQueue::new(capacity),
            events: ArrayQueue::new(capacity),
            peak_milli: AtomicU32::new(0),
            active_voices: AtomicU32::new(0),
            active_notes: AtomicU16::new(0),
            sample_pos: AtomicU64::new(0),
            bpm_milli: AtomicU32::new(120_000),
            playing: AtomicBool::new(true),
            loop_pos: AtomicU64::new(0),
            loop_len: AtomicU64::new(0),
            track_states: AtomicU32::new(0),
            active_track: AtomicU32::new(0),
            dsp_load_permille: AtomicU32::new(0),
            dropped_commands: AtomicU64::new(0),
            dropped_events: AtomicU64::new(0),
            underruns: AtomicU64::new(0),
        })
    }

    /// Returns false when the queue was full. Never blocks: blocking here would
    /// stall the input thread that pushed.
    pub fn push_command(&self, cmd: AudioCommand) -> bool {
        match self.commands.push(cmd) {
            Ok(()) => true,
            Err(_) => {
                self.dropped_commands.fetch_add(1, Ordering::Relaxed);
                false
            }
        }
    }

    pub fn push_event(&self, ev: EngineEvent) {
        if self.events.push(ev).is_err() {
            self.dropped_events.fetch_add(1, Ordering::Relaxed);
        }
    }

    pub fn set_peak(&self, v: f32) {
        self.peak_milli
            .store((v.clamp(0.0, 4.0) * 1000.0) as u32, Ordering::Relaxed);
    }

    pub fn peak(&self) -> f32 {
        self.peak_milli.load(Ordering::Relaxed) as f32 / 1000.0
    }

    pub fn set_active_voices(&self, n: u32) {
        self.active_voices.store(n, Ordering::Relaxed);
    }
    pub fn active_voices(&self) -> u32 {
        self.active_voices.load(Ordering::Relaxed)
    }

    /// Pitch classes actually sounding on the audio thread, one bit per
    /// semitone - see `audio::voice::VoicePool::active_pitch_classes`. This
    /// is what the performance view's note blocks read: it holds through a
    /// voice's release tail exactly as `active_voices` does, so the two
    /// numbers on screen never contradict each other.
    pub fn set_active_notes(&self, mask: u16) {
        self.active_notes.store(mask, Ordering::Relaxed);
    }
    pub fn active_notes(&self) -> u16 {
        self.active_notes.load(Ordering::Relaxed)
    }

    pub fn set_transport(&self, sample_pos: u64, bpm: f32, playing: bool) {
        self.sample_pos.store(sample_pos, Ordering::Relaxed);
        self.bpm_milli
            .store((bpm.max(0.0) * 1000.0) as u32, Ordering::Relaxed);
        self.playing.store(playing, Ordering::Relaxed);
    }

    pub fn sample_pos(&self) -> u64 {
        self.sample_pos.load(Ordering::Relaxed)
    }

    pub fn bpm(&self) -> f32 {
        self.bpm_milli.load(Ordering::Relaxed) as f32 / 1000.0
    }

    pub fn playing(&self) -> bool {
        self.playing.load(Ordering::Relaxed)
    }

    pub fn set_loop_state(&self, pos: u64, len: u64, states: u32, active: usize) {
        self.loop_pos.store(pos, Ordering::Relaxed);
        self.loop_len.store(len, Ordering::Relaxed);
        self.track_states.store(states, Ordering::Relaxed);
        self.active_track.store(active as u32, Ordering::Relaxed);
    }
    pub fn loop_pos(&self) -> u64 {
        self.loop_pos.load(Ordering::Relaxed)
    }
    pub fn loop_len(&self) -> u64 {
        self.loop_len.load(Ordering::Relaxed)
    }
    pub fn track_states(&self) -> u32 {
        self.track_states.load(Ordering::Relaxed)
    }
    pub fn active_track(&self) -> usize {
        self.active_track.load(Ordering::Relaxed) as usize
    }

    pub fn set_dsp_load(&self, permille: u32) {
        self.dsp_load_permille.store(permille, Ordering::Relaxed);
    }
    pub fn dsp_load_percent(&self) -> f32 {
        self.dsp_load_permille.load(Ordering::Relaxed) as f32 / 10.0
    }

    /// One block that was not delivered in time.
    ///
    /// Counted by the audio host when a callback takes longer than the block
    /// it was asked to fill - see `engine::host::callback_cost` for why this
    /// is measured rather than asked of the backend, and for what it does not
    /// include.
    pub fn note_underrun(&self) {
        self.underruns.fetch_add(1, Ordering::Relaxed);
    }
    pub fn underruns(&self) -> u64 {
        self.underruns.load(Ordering::Relaxed)
    }
    pub fn dropped_commands(&self) -> u64 {
        self.dropped_commands.load(Ordering::Relaxed)
    }
    pub fn dropped_events(&self) -> u64 {
        self.dropped_events.load(Ordering::Relaxed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::event::{Action, TransportCmd};

    #[test]
    fn a_full_queue_drops_and_counts_rather_than_blocking() {
        // Blocking here would stall whichever input thread pushed, and a
        // silently dropped command is a note that never sounds with nothing to
        // show for it. The counter is the only way to learn it happened.
        let t = Telemetry::new(4);
        for _ in 0..4 {
            assert!(t.push_command(AudioCommand::Act(Action::Transport(TransportCmd::Play))));
        }
        assert!(!t.push_command(AudioCommand::Act(Action::Transport(TransportCmd::Play))));
        assert_eq!(t.dropped_commands(), 1);
    }

    #[test]
    fn draining_makes_room_again() {
        let t = Telemetry::new(2);
        t.push_command(AudioCommand::SetTestTone(true));
        assert!(t.commands.pop().is_some());
        assert!(t.push_command(AudioCommand::SetTestTone(false)));
    }

    #[test]
    fn peak_survives_the_trip_through_an_atomic() {
        let t = Telemetry::new(4);
        t.set_peak(0.625);
        assert!((t.peak() - 0.625).abs() < 1e-3);
    }

    #[test]
    fn active_notes_survive_the_trip_through_an_atomic() {
        let t = Telemetry::new(4);
        let mask: u16 = (1 << 0) | (1 << 7); // C and G
        t.set_active_notes(mask);
        assert_eq!(t.active_notes(), mask);
    }

    #[test]
    fn every_command_is_copy() {
        fn assert_copy<T: Copy>() {}
        assert_copy::<AudioCommand>();
        assert_copy::<EngineEvent>();
    }
}
