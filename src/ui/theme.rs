use egui::{Color32, Rounding, Stroke, Visuals};

pub const BG: Color32 = Color32::from_rgb(0x0B, 0x0C, 0x10);
pub const SURFACE: Color32 = Color32::from_rgb(0x14, 0x16, 0x1C);
pub const SURFACE_HI: Color32 = Color32::from_rgb(0x1E, 0x21, 0x2A);
pub const TEXT: Color32 = Color32::from_rgb(0xE6, 0xE8, 0xEF);
pub const TEXT_DIM: Color32 = Color32::from_rgb(0x9A, 0x9F, 0xAF);
pub const ACCENT: Color32 = Color32::from_rgb(0x7C, 0xE0, 0xD6);
pub const ACCENT_WARM: Color32 = Color32::from_rgb(0xFF, 0xB3, 0x6B);
pub const DANGER: Color32 = Color32::from_rgb(0xFF, 0x6B, 0x7A);

pub const R_SM: f32 = 6.0;
pub const R_MD: f32 = 12.0;

pub fn apply(ctx: &egui::Context) {
    let mut v = Visuals::dark();
    v.panel_fill = BG;
    v.window_fill = SURFACE;
    v.extreme_bg_color = BG;
    v.override_text_color = Some(TEXT);
    v.widgets.noninteractive.bg_fill = SURFACE;
    v.widgets.inactive.bg_fill = SURFACE_HI;
    v.widgets.hovered.bg_fill = SURFACE_HI;
    v.widgets.active.bg_fill = ACCENT;
    v.widgets.noninteractive.rounding = Rounding::same(R_MD);
    v.widgets.inactive.rounding = Rounding::same(R_MD);
    v.widgets.hovered.rounding = Rounding::same(R_MD);
    v.widgets.active.rounding = Rounding::same(R_MD);
    v.widgets.noninteractive.bg_stroke = Stroke::new(1.0_f32, SURFACE_HI);
    ctx.set_visuals(v);
}
