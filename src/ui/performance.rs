use crate::core::event::{Action, LoopCmd, TransportCmd};
use crate::engine::telemetry::{AudioCommand, Telemetry};
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

#[allow(clippy::too_many_arguments)]
pub fn show(
    ui: &mut egui::Ui,
    active: u16,
    octave: i8,
    velocity: f32,
    peak: f32,
    voices: u32,
    bpm: f32,
    playing: bool,
    loop_pos: u64,
    loop_len: u64,
    track_states: u32,
    active_track: usize,
    telemetry: Option<&Telemetry>,
) {
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
        ui.separator();
        ui.label(format!(
            "{} {:.1} BPM",
            if playing { "PLAY" } else { "STOP" },
            bpm
        ));
        if let Some(telemetry) = telemetry {
            if ui.button(if playing { "STOP" } else { "PLAY" }).clicked() {
                telemetry.push_command(AudioCommand::Act(Action::Transport(TransportCmd::Toggle)));
            }
        }
    });

    ui.add_space(18.0);
    ui.horizontal(|ui| {
        ui.heading("LOOP LANES");
        if loop_len > 0 {
            ui.add_space(12.0);
            ui.small(format!("{} / {} samples", loop_pos, loop_len));
        }
    });
    ui.horizontal(|ui| {
        let send = |cmd: LoopCmd| {
            if let Some(telemetry) = telemetry {
                telemetry.push_command(AudioCommand::Act(Action::LoopControl(cmd)));
            }
        };
        if ui.button("● REC").clicked() {
            send(LoopCmd::ToggleRecord);
        }
        if ui.button("CLEAR").clicked() {
            send(LoopCmd::Clear);
        }
        if ui.button("UNDO").clicked() {
            send(LoopCmd::Undo);
        }
        if ui.button("MUTE").clicked() {
            send(LoopCmd::Mute);
        }
    });
    ui.horizontal(|ui| {
        for i in 0..4 {
            let code = (track_states >> (i * 2)) & 0b11;
            let (label, colour) = match code {
                0 => ("EMPTY", theme::TEXT_DIM),
                1 => ("REC", theme::DANGER),
                2 => ("PLAY", theme::ACCENT),
                _ => ("MUTE", theme::ACCENT_WARM),
            };
            let active = i == active_track;
            egui::Frame::group(ui.style())
                .fill(if active {
                    theme::SURFACE_HI
                } else {
                    theme::SURFACE
                })
                .stroke(egui::Stroke::new(
                    1.0_f32,
                    if active { colour } else { theme::SURFACE_HI },
                ))
                .rounding(egui::Rounding::same(8.0))
                .inner_margin(egui::Margin::symmetric(12.0, 8.0))
                .show(ui, |ui| {
                    ui.vertical(|ui| {
                        ui.small(format!("TRACK {}", i + 1));
                        ui.colored_label(colour, label);
                        if ui.small_button("select").clicked() {
                            if let Some(telemetry) = telemetry {
                                telemetry.push_command(AudioCommand::Act(Action::LoopControl(
                                    LoopCmd::Select(i as u8),
                                )));
                            }
                        }
                    });
                });
        }
    });
    let progress = if loop_len == 0 {
        0.0
    } else {
        loop_pos as f32 / loop_len as f32
    };
    ui.add(
        egui::ProgressBar::new(progress.clamp(0.0, 1.0))
            .desired_height(6.0)
            .fill(theme::ACCENT)
            .text(""),
    );

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
    ui.small("R record  , clear  . undo  - mute  1–4 select track  SPACE play/stop");
    ui.small("A W S E D F T G Y H U J K play  ·  Z/X octave  ·  C/V velocity");
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
        // which the master stage has already squashed through `tanh`, so the
        // published peak could never exceed tanh(1.5) = 0.905 whatever was
        // played, and a warning at 0.95 was unreachable by construction.
        //
        // Both figures are from the scenario this test performs - sixteen
        // voices at full level with WET and CHAOS wide open, which are
        // ordinary macro positions rather than a contrived state:
        //
        //   metering the block that leaves the graph:   0.885   (ceiling 0.905)
        //   metering the master stage before its clip:  1.398
        //
        // An earlier version of this comment paired that 1.398 with 0.720.
        // 0.720 is the same measurement taken with the macros left at their
        // defaults - a different scenario, and so not a comparison.
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
