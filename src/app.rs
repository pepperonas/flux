use crate::ui::{self, View};

#[derive(Default)]
pub struct FluxApp {
    view: View,
}

impl eframe::App for FluxApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // An instrument must redraw continuously: meters and note feedback are live.
        ctx.request_repaint();

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
            View::Performance => ui::performance::show(ui),
            View::Debug => ui::debug::show(ui),
        });
    }
}
