// The architecture is built ahead of its consumers: transport and quantization
// (M1b), control-surface and MIDI types (M2), guitar types (M3). Their absence
// of callers is intentional, not neglect. Remove this once M2 lands and the
// remaining gaps are real.
#![allow(dead_code)]

mod app;
mod core;
mod input;
mod params;
mod ui;

fn main() -> eframe::Result<()> {
    env_logger::init();
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1180.0, 760.0])
            .with_min_inner_size([900.0, 600.0])
            .with_title("FLUX"),
        ..Default::default()
    };
    eframe::run_native(
        "FLUX",
        options,
        Box::new(|cc| {
            ui::theme::apply(&cc.egui_ctx);
            Ok(Box::new(app::FluxApp::default()))
        }),
    )
}
