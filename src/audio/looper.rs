use crate::core::event::{Action, LoopCmd};
use crate::core::quantize::{quantize_in_loop, Grid};

pub const TRACK_COUNT: usize = 4;
const MAX_EVENTS: usize = 2048;

#[derive(Clone, Copy, Debug, PartialEq)]
struct LoopEvent {
    pos: u64,
    action: Action,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TrackState {
    Empty,
    Recording,
    Playing,
    Muted,
}

struct Track {
    events: [Option<LoopEvent>; MAX_EVENTS],
    len: usize,
    before_record: usize,
    state: TrackState,
}

impl Default for Track {
    fn default() -> Self {
        Self {
            events: [None; MAX_EVENTS],
            len: 0,
            before_record: 0,
            state: TrackState::Empty,
        }
    }
}

impl Track {
    fn clear(&mut self) {
        self.len = 0;
        self.state = TrackState::Empty;
    }

    fn insert(&mut self, event: LoopEvent) {
        if self.len == MAX_EVENTS {
            return;
        }
        let mut at = self.len;
        while at > 0 && self.events[at - 1].expect("occupied").pos > event.pos {
            self.events[at] = self.events[at - 1];
            at -= 1;
        }
        self.events[at] = Some(event);
        self.len += 1;
    }
}

pub struct EventLooper {
    tracks: [Track; TRACK_COUNT],
    pub active_track: usize,
    pub loop_len: u64,
    pub grid: Grid,
    recording_start: Option<u64>,
}

impl Default for EventLooper {
    fn default() -> Self {
        Self {
            tracks: std::array::from_fn(|_| Track::default()),
            active_track: 0,
            loop_len: 0,
            grid: Grid::Sixteenth,
            recording_start: None,
        }
    }
}

impl EventLooper {
    pub fn state_codes(&self) -> u32 {
        self.tracks.iter().enumerate().fold(0, |bits, (i, track)| {
            let code = match track.state {
                TrackState::Empty => 0,
                TrackState::Recording => 1,
                TrackState::Playing => 2,
                TrackState::Muted => 3,
            };
            bits | (code << (i * 2))
        })
    }

    pub fn handle(&mut self, cmd: LoopCmd, pos: u64, samples_per_beat: f64, samples_per_bar: f64) {
        match cmd {
            LoopCmd::ToggleRecord => {
                let track = &mut self.tracks[self.active_track];
                match track.state {
                    TrackState::Recording => {
                        track.state = TrackState::Playing;
                        if self.loop_len == 0 {
                            self.loop_len =
                                ((pos.max(1) as f64 / 96_000.0).ceil() as u64).max(1) * 96_000;
                        }
                        self.recording_start = None;
                        if let Some(grid) = self.grid.samples(samples_per_beat) {
                            for event in track.events[..track.len].iter_mut().flatten() {
                                event.pos = quantize_in_loop(event.pos, grid, self.loop_len);
                            }
                            track.events[..track.len]
                                .sort_by_key(|event| event.expect("occupied").pos);
                        }
                    }
                    TrackState::Empty | TrackState::Playing | TrackState::Muted => {
                        track.before_record = track.len;
                        track.state = TrackState::Recording;
                        let bar = samples_per_bar.max(1.0).round() as u64;
                        self.recording_start =
                            Some(if pos == 0 { 0 } else { pos.div_ceil(bar) * bar });
                    }
                }
            }
            LoopCmd::Clear => self.tracks[self.active_track].clear(),
            LoopCmd::Undo => {
                let track = &mut self.tracks[self.active_track];
                track.len = track.before_record.min(track.len);
                track.state = if track.len == 0 {
                    TrackState::Empty
                } else {
                    TrackState::Playing
                };
            }
            LoopCmd::Mute => {
                let track = &mut self.tracks[self.active_track];
                track.state = match track.state {
                    TrackState::Muted => TrackState::Playing,
                    _ => TrackState::Muted,
                };
            }
            LoopCmd::Select(n) if (n as usize) < TRACK_COUNT => self.active_track = n as usize,
            LoopCmd::Select(_) => {}
        }
    }

    pub fn record(&mut self, action: Action, pos: u64, samples_per_beat: f64) {
        let track = &mut self.tracks[self.active_track];
        if track.state != TrackState::Recording {
            return;
        }
        if self.recording_start.is_some_and(|start| pos < start) {
            return;
        }
        let p = if self.loop_len == 0 {
            pos
        } else {
            quantize_in_loop(
                pos,
                self.grid.samples(samples_per_beat).unwrap_or(0.0),
                self.loop_len,
            )
        };
        track.insert(LoopEvent { pos: p, action });
    }

    pub fn events_in_block(&self, pos: u64, frames: usize) -> [Option<Action>; 64] {
        let mut out = [None; 64];
        if self.loop_len == 0 {
            return out;
        }
        let p = pos % self.loop_len;
        let end = p.saturating_add(frames as u64);
        let wrapped_end = end % self.loop_len;
        let mut out_len = 0;
        for track in &self.tracks {
            if matches!(track.state, TrackState::Playing) {
                for event in track.events[..track.len].iter().flatten() {
                    let in_range = if end < self.loop_len {
                        event.pos >= p && event.pos < end
                    } else {
                        event.pos >= p || event.pos < wrapped_end
                    };
                    if in_range && out_len < out.len() {
                        out[out_len] = Some(event.action);
                        out_len += 1;
                    }
                }
            }
        }
        out
    }

    pub fn track_state(&self, n: usize) -> &'static str {
        match self.tracks[n].state {
            TrackState::Empty => "EMPTY",
            TrackState::Recording => "REC",
            TrackState::Playing => "PLAY",
            TrackState::Muted => "MUTE",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recording_toggle_captures_and_replays_an_event() {
        let mut l = EventLooper::default();
        l.handle(LoopCmd::ToggleRecord, 0, 24_000.0, 96_000.0);
        l.record(
            Action::NoteOn {
                note: 60,
                velocity: 1.0,
            },
            100,
            24_000.0,
        );
        l.handle(LoopCmd::ToggleRecord, 100_000, 24_000.0, 96_000.0);
        l.loop_len = 96_000;
        assert_eq!(
            l.events_in_block(0, 256)[0],
            Some(Action::NoteOn {
                note: 60,
                velocity: 1.0
            })
        );
    }

    #[test]
    fn undo_restores_the_event_count_before_the_overdub() {
        let mut l = EventLooper::default();
        l.handle(LoopCmd::ToggleRecord, 0, 24_000.0, 96_000.0);
        l.record(
            Action::NoteOn {
                note: 60,
                velocity: 1.0,
            },
            100,
            24_000.0,
        );
        l.handle(LoopCmd::ToggleRecord, 100_000, 24_000.0, 96_000.0);
        l.handle(LoopCmd::ToggleRecord, 100_000, 24_000.0, 96_000.0);
        l.record(
            Action::NoteOn {
                note: 64,
                velocity: 1.0,
            },
            200,
            24_000.0,
        );
        l.handle(LoopCmd::Undo, 200, 24_000.0, 96_000.0);
        l.loop_len = 96_000;
        assert_eq!(l.events_in_block(200, 256)[0], None);
    }

    #[test]
    fn selecting_a_track_does_not_change_the_others() {
        let mut l = EventLooper::default();
        l.handle(LoopCmd::Select(2), 0, 24_000.0, 96_000.0);
        assert_eq!(l.active_track, 2);
        assert_eq!(l.track_state(0), "EMPTY");
    }

    #[test]
    fn a_recording_armed_inside_a_bar_waits_for_the_next_bar() {
        let mut l = EventLooper::default();
        l.handle(LoopCmd::ToggleRecord, 100, 24_000.0, 96_000.0);
        l.record(
            Action::NoteOn {
                note: 60,
                velocity: 1.0,
            },
            200,
            24_000.0,
        );
        l.record(
            Action::NoteOn {
                note: 64,
                velocity: 1.0,
            },
            96_100,
            24_000.0,
        );
        l.handle(LoopCmd::ToggleRecord, 192_000, 24_000.0, 96_000.0);
        l.loop_len = 192_000;
        assert_eq!(l.events_in_block(0, 256)[0], None);
        assert_eq!(
            l.events_in_block(96_000, 256)[0],
            Some(Action::NoteOn {
                note: 64,
                velocity: 1.0
            })
        );
    }
}
