use crate::ui::theme;

/// Read-only view of the built-in signal path. The graph is intentionally
/// described here as presentation data; execution remains owned by the audio
/// engine's compiled schedule.
pub fn show(ui: &mut egui::Ui) {
    ui.heading("PATCH");
    ui.add_space(8.0);
    ui.label("Built-in signal path");
    ui.add_space(12.0);
    ui.horizontal_wrapped(|ui| {
        for (index, name) in ["16× VOICE", "MIXER", "DELAY", "REVERB", "OUTPUT"]
            .into_iter()
            .enumerate()
        {
            egui::Frame::group(ui.style())
                .fill(theme::SURFACE_HI)
                .rounding(egui::Rounding::same(8.0))
                .inner_margin(egui::Margin::symmetric(16.0, 14.0))
                .show(ui, |ui| {
                    ui.strong(name);
                    ui.small(if index == 0 {
                        "OSC → FILTER → VCA"
                    } else {
                        "audio"
                    });
                });
            if index < 4 {
                ui.label("→");
            }
        }
    });
    ui.add_space(16.0);
    ui.small("Read-only in M1. Module editing and cable changes are planned for M4.");
}
