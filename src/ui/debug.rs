use crate::engine::host::{describe_buffer_frames, describe_latency_ms};
use crate::engine::telemetry::{AudioCommand, Telemetry};
use crate::input::midi::MidiSource;
use crate::input::xplorer::XplorerSource;

pub fn show(
    ui: &mut egui::Ui,
    host: Option<&crate::engine::host::AudioHost>,
    error: Option<&str>,
    test_tone: &mut bool,
    midi: &MidiSource,
    xplorer: &XplorerSource,
) {
    ui.heading("DIAGNOSTICS");
    if let Some(err) = error {
        ui.colored_label(crate::ui::theme::DANGER, err);
        ui.label("FLUX is running without audio. Choose another device in Settings.");
        return;
    }
    let Some(host) = host else { return };
    let t: &Telemetry = &host.telemetry;

    egui::Grid::new("diag")
        .num_columns(2)
        .spacing([24.0, 6.0])
        .show(ui, |ui| {
            ui.label("Device");
            ui.label(&host.device_name);
            ui.end_row();
            ui.label("Sample rate");
            ui.label(format!("{:.0} Hz", host.sample_rate));
            ui.end_row();
            // "measuring…" until the device's first callback has actually
            // happened - never a guessed number formatted as though it were
            // one. See `engine::host::ObservedBufferSize`.
            ui.label("Buffer");
            ui.label(describe_buffer_frames(host.buffer_frames()));
            ui.end_row();
            ui.label("Latency");
            ui.label(describe_latency_ms(host.latency_ms()));
            ui.end_row();
            ui.label("DSP load");
            ui.label(format!("{:.1} %", t.dsp_load_percent()));
            ui.end_row();
            ui.label("Active voices");
            ui.label(t.active_voices().to_string());
            ui.end_row();
            ui.label("Peak");
            ui.label(format!("{:.3}", t.peak()));
            ui.end_row();
            ui.label("Dropped commands");
            ui.label(t.dropped_commands().to_string());
            ui.end_row();
            ui.label("Dropped events");
            ui.label(t.dropped_events().to_string());
            ui.end_row();
            ui.label("Underruns");
            ui.label(t.underruns().to_string());
            ui.end_row();
        });

    ui.separator();
    ui.label("Guitar Hero X-plorer");
    ui.small(format!(
        "{}; {} raw reports",
        xplorer.status(),
        xplorer.reports()
    ));
    if let Some(fingerprint) = xplorer.last_report_fingerprint() {
        ui.small(format!("last report fingerprint: {fingerprint:016x}"));
    }

    ui.separator();
    ui.label("MIDI inputs");
    if midi.ports.is_empty() {
        ui.small("None connected at application start.");
    } else {
        for port in &midi.ports {
            ui.small(port);
        }
    }

    ui.separator();
    if ui.checkbox(test_tone, "Test tone (440 Hz)").changed() {
        t.push_command(AudioCommand::SetTestTone(*test_tone));
    }
    ui.small("Proves the path from device to speaker without needing any input to work.");
}
