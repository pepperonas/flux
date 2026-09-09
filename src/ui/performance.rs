use crate::ui::theme;

const NOTE_NAMES: [&str; 12] = [
    "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
];

pub fn show(ui: &mut egui::Ui, active: u16, octave: i8, velocity: f32, peak: f32, voices: u32) {
    ui.heading("FLUX");
    ui.add_space(12.0);

    ui.horizontal(|ui| {
        for (i, name) in NOTE_NAMES.iter().enumerate() {
            let lit = active & (1 << i) != 0;
            let (rect, _) = ui.allocate_exact_size(egui::vec2(46.0, 96.0), egui::Sense::hover());
            let fill = if lit {
                theme::ACCENT
            } else {
                theme::SURFACE_HI
            };
            ui.painter().rect_filled(rect, theme::R_SM, fill);
            ui.painter().text(
                rect.center_bottom() - egui::vec2(0.0, 14.0),
                egui::Align2::CENTER_CENTER,
                name,
                egui::FontId::monospace(12.0),
                if lit { theme::BG } else { theme::TEXT_DIM },
            );
        }
    });

    ui.add_space(16.0);
    ui.horizontal(|ui| {
        ui.label(format!("OCTAVE {octave}"));
        ui.separator();
        ui.label(format!("VELOCITY {:.0}%", velocity * 100.0));
        ui.separator();
        ui.label(format!("VOICES {voices}"));
    });

    ui.add_space(8.0);
    let (rect, _) = ui.allocate_exact_size(egui::vec2(240.0, 8.0), egui::Sense::hover());
    ui.painter()
        .rect_filled(rect, theme::R_SM, theme::SURFACE_HI);
    let filled = egui::Rect::from_min_size(
        rect.min,
        egui::vec2(rect.width() * peak.clamp(0.0, 1.0), rect.height()),
    );
    let colour = if peak > 0.95 {
        theme::DANGER
    } else {
        theme::ACCENT
    };
    ui.painter().rect_filled(filled, theme::R_SM, colour);

    ui.add_space(20.0);
    ui.small("A W S E D F T G Y H U J K play.  Z X change octave.  C V change velocity.");
}
