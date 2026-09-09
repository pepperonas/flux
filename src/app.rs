use crate::engine::host::AudioHost;
use crate::input::keyboard::{self, KeyboardSource};
use crate::input::mapping::Mapping;
use crate::ui::{self, View};

pub struct FluxApp {
    view: View,
    audio: Option<AudioHost>,
    audio_error: Option<String>,
    test_tone: bool,
    keyboard: KeyboardSource,
    mapping: Mapping,
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
        FluxApp {
            view: View::default(),
            audio,
            audio_error,
            test_tone: false,
            keyboard: KeyboardSource::default(),
            mapping: keyboard::default_mapping(),
        }
    }
}

impl Default for FluxApp {
    fn default() -> FluxApp {
        FluxApp::new()
    }
}

impl eframe::App for FluxApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // An instrument must redraw continuously: meters and note feedback are live.
        ctx.request_repaint();

        // Read this frame's key events before anything is drawn, so the
        // panels below always reflect this frame's state. Without a device
        // there is nowhere for the resulting commands to go, so keyboard
        // input is only pumped once audio is up.
        if let Some(host) = &self.audio {
            self.keyboard.pump(ctx, &self.mapping, &host.telemetry);
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
                self.keyboard.active_notes(),
                self.keyboard.play.octave,
                self.keyboard.play.velocity,
                self.audio.as_ref().map_or(0.0, |h| h.telemetry.peak()),
                self.audio.as_ref().map_or(0, |h| h.telemetry.active_voices()),
            ),
            View::Debug => ui::debug::show(
                ui,
                self.audio.as_ref(),
                self.audio_error.as_deref(),
                &mut self.test_tone,
            ),
        });
    }
}
