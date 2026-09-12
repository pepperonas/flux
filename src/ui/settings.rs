pub enum SettingsAction {
    Refresh,
    ReconnectMidi,
    Select(String),
}

pub fn show(ui: &mut egui::Ui, devices: &[String], active: &str) -> Option<SettingsAction> {
    ui.heading("SETTINGS");
    ui.add_space(8.0);
    ui.label("Audio output");
    let mut selected = None;
    if ui.button("Refresh devices").clicked() {
        selected = Some(SettingsAction::Refresh);
    }
    if ui.button("Reconnect MIDI").clicked() {
        selected = Some(SettingsAction::ReconnectMidi);
    }
    if devices.is_empty() {
        ui.small("No output devices reported by the host.");
    } else {
        for device in devices {
            if ui.selectable_label(device == active, device).clicked() && device != active {
                selected = Some(SettingsAction::Select(device.clone()));
            }
        }
    }
    ui.add_space(12.0);
    ui.small("Changing the device restarts the audio stream.");
    selected
}
