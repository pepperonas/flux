use crate::ui::theme;

/// Where the output meter turns red.
///
/// The number it is compared against is the master stage's peak *before* its
/// soft clip (see `graph::modules::Output` and `Module::peak`), so a value
/// above 1.0 is both meaningful and reachable: it says the clipper is doing
/// real work. Measured against the post-clip block instead, nothing above
/// `tanh(1.5) = 0.905` exists and this warning could never have fired at all.
const CLIP_WARNING_PEAK: f32 = 0.95;

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
    let colour = if peak > CLIP_WARNING_PEAK {
        theme::DANGER
    } else {
        theme::ACCENT
    };
    ui.painter().rect_filled(filled, theme::R_SM, colour);

    ui.add_space(20.0);
    ui.small("A W S E D F T G Y H U J K play.  Z X change octave.  C V change velocity.");
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::engine::AudioEngine;
    use crate::core::event::Action;
    use crate::core::ids::MacroId;
    use crate::engine::telemetry::{AudioCommand, Telemetry};
    use crate::params::registry::{MASTER_GAIN, OSC_LEVEL};
    use std::sync::Arc;

    #[test]
    fn the_clip_warning_can_actually_fire() {
        // It could not before. The meter read the block leaving the graph,
        // which the master stage has already squashed through `tanh` - so the
        // published peak could never exceed tanh(1.5) = 0.905, and a warning
        // at 0.95 was unreachable by construction. Driven as hard as the
        // instrument goes it measured 0.720; the ceiling was 0.905.
        //
        // The peak published now is the master stage's own, taken before the
        // clipper. Sixteen voices at full level with WET and CHAOS wide open -
        // ordinary macro positions, not a contrived state - reaches 1.398.
        let telemetry = Telemetry::new(64);
        let mut engine = AudioEngine::new(48_000.0, 512, Arc::clone(&telemetry));
        engine.params.set_base(MASTER_GAIN, 1.0);
        engine.params.set_base(OSC_LEVEL, 1.0);
        for macro_id in [MacroId(1), MacroId(3)] {
            engine
                .telemetry
                .push_command(AudioCommand::Act(Action::SetMacro {
                    macro_id,
                    value: 1.0,
                }));
        }
        for note in 40..56u8 {
            engine
                .telemetry
                .push_command(AudioCommand::Act(Action::NoteOn {
                    note,
                    velocity: 1.0,
                }));
        }

        let mut highest = 0.0f32;
        for _ in 0..400 {
            engine.render(256);
            highest = highest.max(telemetry.peak());
        }

        assert!(
            highest > 1.5f32.tanh(),
            "the published peak never rose above the post-clip ceiling \
             {:.3}, so it is still the clipped block being metered",
            1.5f32.tanh()
        );
        assert!(
            highest > CLIP_WARNING_PEAK,
            "the loudest the instrument gets is {highest:.3}, below the clip \
             warning at {CLIP_WARNING_PEAK} - the indicator can never fire"
        );
    }
}
