use std::sync::Arc;

use crate::engine::host::AudioHost;
use crate::engine::telemetry::Telemetry;
use crate::input::keyboard::{self, KeyboardSource};
use crate::input::mapping::Mapping;
use crate::input::midi::MidiSource;
use crate::input::xplorer::XplorerSource;
use crate::ui::{self, View};

/// How many engine events one interface frame may take off the queue.
///
/// The queue is bounded, so emptying it is bounded work - but the audio thread
/// pushes while the interface pops, so `while let Some(_) = pop()` can be kept
/// running indefinitely by a producer that keeps up, and a frame that never
/// ends is worse than a dropped event. At 60 Hz this clears 15 360 events a
/// second against the two or three an ordinary note produces, so the cap is
/// never the thing that limits the drain in practice.
const EVENTS_PER_FRAME: usize = 256;

/// Take this frame's share of engine events off the queue, and report how many
/// were taken.
///
/// Nothing in M1a's interface acts on them. The note blocks and the voice
/// count read the atomics instead, which is deliberate: those hold through a
/// voice's release tail and its stealing, which a tally kept from NoteStarted
/// and NoteEnded could not. The queue is emptied anyway because
/// ARCHITECTURE SS2 makes `dropped_events` the only evidence that anything was
/// ever lost - "you cannot retrofit a counter into a path that already
/// silently discarded" - and a queue with no reader turns that counter into a
/// measure of the interface's own neglect rather than of a real fault.
/// Measured without this drain: 700 notes of ordinary playing over 30 seconds
/// dropped 1060 events out of 2084 and the counter was pure noise.
///
/// SS2 names "voice stolen" as an event-queue payload, so the pushes stay; the
/// looper's loop lanes in M1b are the first consumer that reads them.
fn drain_engine_events(telemetry: &Telemetry) -> usize {
    let mut taken = 0;
    while taken < EVENTS_PER_FRAME && telemetry.events.pop().is_some() {
        taken += 1;
    }
    taken
}

pub struct FluxApp {
    view: View,
    audio: Option<AudioHost>,
    audio_error: Option<String>,
    test_tone: bool,
    keyboard: KeyboardSource,
    mapping: Mapping,
    midi: MidiSource,
    xplorer: XplorerSource,
}

impl FluxApp {
    pub fn new() -> FluxApp {
        // A device error must not take the process down: it is shown in the
        // diagnostics view, and the rest of the interface keeps working.
        let (audio, audio_error) = match AudioHost::start(None) {
            Ok(host) => (Some(host), None),
            Err(err) => {
                log::error!("audio host failed to start: {err}");
                (None, Some(err.to_string()))
            }
        };
        let midi = audio
            .as_ref()
            .map(|host| MidiSource::connect_all(Arc::clone(&host.telemetry)))
            .unwrap_or_default();
        FluxApp {
            view: View::default(),
            audio,
            audio_error,
            test_tone: false,
            keyboard: KeyboardSource::default(),
            mapping: keyboard::default_mapping(),
            midi,
            xplorer: XplorerSource::start(),
        }
    }
}

impl Default for FluxApp {
    fn default() -> FluxApp {
        FluxApp::new()
    }
}

impl FluxApp {
    /// Everything one interface frame owes the audio thread: hand it this
    /// frame's key presses, and take back the events it published.
    ///
    /// Split out of `update` because `update` cannot be called from a test -
    /// `eframe::Frame` has no public constructor - and leaving the drain as a
    /// bare line in there would leave the fix it implements unpinned. What
    /// remains untestable is the single delegation below.
    fn service_audio(&mut self, ctx: &egui::Context, telemetry: &Telemetry) {
        self.keyboard.pump(ctx, &self.mapping, telemetry);
        drain_engine_events(telemetry);
    }
}

impl eframe::App for FluxApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // An instrument must redraw continuously: meters and note feedback are live.
        ctx.request_repaint();

        // Read this frame's key events before anything is drawn, so the
        // panels below always reflect this frame's state. Without a device
        // there is nowhere for the resulting commands to go, and nothing
        // producing events to drain, so this only runs once audio is up.
        // The handle is cloned rather than borrowed so that `service_audio`
        // can take `&mut self`; an `Arc` clone once a frame costs nothing.
        let telemetry = self.audio.as_ref().map(|h| Arc::clone(&h.telemetry));
        if let Some(telemetry) = telemetry {
            self.service_audio(ctx, &telemetry);
        }

        egui::TopBottomPanel::top("nav").show(ctx, |ui| {
            ui.horizontal(|ui| {
                for view in [View::Performance, View::Debug] {
                    if ui
                        .selectable_label(self.view == view, view.label())
                        .clicked()
                    {
                        self.view = view;
                    }
                }
            });
        });

        egui::CentralPanel::default().show(ctx, |ui| match self.view {
            View::Performance => ui::performance::show(
                ui,
                self.audio
                    .as_ref()
                    .map_or(0, |h| h.telemetry.active_notes()),
                self.keyboard.play.octave,
                self.keyboard.play.velocity,
                self.audio.as_ref().map_or(0.0, |h| h.telemetry.peak()),
                self.audio
                    .as_ref()
                    .map_or(0, |h| h.telemetry.active_voices()),
                self.audio.as_ref().map_or(120.0, |h| h.telemetry.bpm()),
                self.audio.as_ref().is_some_and(|h| h.telemetry.playing()),
            ),
            View::Debug => ui::debug::show(
                ui,
                self.audio.as_ref(),
                self.audio_error.as_deref(),
                &mut self.test_tone,
                &self.midi,
                &self.xplorer,
            ),
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::engine::AudioEngine;
    use crate::core::event::Action;
    use crate::engine::telemetry::{AudioCommand, EngineEvent};
    use std::sync::Arc;

    #[test]
    fn one_frame_never_takes_more_than_its_share() {
        // The audio thread pushes while the interface pops. An unbounded
        // `while let Some(_) = pop()` can therefore be kept alive by a
        // producer that keeps up, and a frame that never returns is a worse
        // failure than a lost event.
        let t = Telemetry::new(EVENTS_PER_FRAME * 4);
        for i in 0..(EVENTS_PER_FRAME + 50) {
            t.push_event(EngineEvent::NoteStarted {
                note: (i % 128) as u8,
            });
        }
        assert_eq!(drain_engine_events(&t), EVENTS_PER_FRAME);
        assert_eq!(t.events.len(), 50);
    }

    #[test]
    fn successive_frames_clear_a_backlog() {
        // Bounded per frame, but not bounded overall: whatever a burst left
        // behind is gone within a few more frames, so the cap cannot turn
        // into a permanent backlog that drops events anyway.
        let t = Telemetry::new(EVENTS_PER_FRAME * 4);
        for i in 0..(EVENTS_PER_FRAME + 50) {
            t.push_event(EngineEvent::NoteStarted {
                note: (i % 128) as u8,
            });
        }
        drain_engine_events(&t);
        assert_eq!(drain_engine_events(&t), 50);
        assert_eq!(drain_engine_events(&t), 0);
        assert!(t.events.is_empty());
    }

    #[test]
    fn a_frame_hands_over_key_presses_and_takes_back_engine_events() {
        // The wiring, not just the pieces. Deleting either half of
        // `service_audio` leaves the instrument looking fine and quietly
        // broken - no sound at all in one direction, a `dropped_events`
        // counter that measures the interface's neglect in the other.
        let t = Telemetry::new(16);
        for i in 0..8u8 {
            t.push_event(EngineEvent::NoteStarted { note: i });
        }
        let mut app = FluxApp {
            view: View::default(),
            audio: None,
            audio_error: None,
            test_tone: false,
            keyboard: KeyboardSource::default(),
            mapping: keyboard::default_mapping(),
            midi: MidiSource::default(),
            xplorer: XplorerSource::start(),
        };
        let ctx = egui::Context::default();
        let raw = egui::RawInput {
            events: vec![egui::Event::Key {
                key: egui::Key::A,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::default(),
            }],
            ..Default::default()
        };
        let _ = ctx.run(raw, |ctx| app.service_audio(ctx, &t));

        assert_eq!(
            t.commands.pop(),
            Some(AudioCommand::Act(Action::NoteOn {
                note: 60,
                velocity: app.keyboard.play.velocity
            })),
            "the frame did not hand the key press to the audio thread"
        );
        assert!(
            t.events.is_empty(),
            "the frame did not take the engine's events back off the queue"
        );
    }

    #[test]
    fn ordinary_playing_drops_no_events_once_the_interface_drains_them() {
        // The measurement this drain exists for. Before it, 700 notes over
        // thirty seconds - unremarkable playing - saturated the 1024-slot
        // queue and dropped 1060 of the 2084 events produced, while the
        // diagnostics view showed that number to the player as though it
        // meant something had gone wrong.
        //
        // The frame cadence here is deliberately conservative: a drain every
        // three 256-frame blocks is 16 ms, one 60 Hz frame.
        const BLOCK: usize = 256;
        let telemetry = Telemetry::new(1024);
        let mut engine = AudioEngine::new(48_000.0, 512, Arc::clone(&telemetry));
        let mut since_frame = 0;
        let block = |engine: &mut AudioEngine, since_frame: &mut usize| {
            engine.render(BLOCK);
            *since_frame += 1;
            if *since_frame == 3 {
                *since_frame = 0;
                drain_engine_events(&telemetry);
            }
        };
        for i in 0..700u32 {
            let note = 48 + (i % 24) as u8;
            engine
                .telemetry
                .push_command(AudioCommand::Act(Action::NoteOn {
                    note,
                    velocity: 0.8,
                }));
            for _ in 0..4 {
                block(&mut engine, &mut since_frame);
            }
            engine
                .telemetry
                .push_command(AudioCommand::Act(Action::NoteOff { note }));
            for _ in 0..4 {
                block(&mut engine, &mut since_frame);
            }
        }
        assert_eq!(
            telemetry.dropped_events(),
            0,
            "ordinary playing lost engine events, so the counter no longer \
             means what the diagnostics view claims it means"
        );
    }
}
