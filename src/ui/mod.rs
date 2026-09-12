pub mod debug;
pub mod patch;
pub mod performance;
pub mod settings;
pub mod theme;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum View {
    #[default]
    Performance,
    Patch,
    Settings,
    Debug,
}

impl View {
    pub fn label(self) -> &'static str {
        match self {
            View::Performance => "PERFORMANCE",
            View::Patch => "PATCH",
            View::Settings => "SETTINGS",
            View::Debug => "DIAGNOSTICS",
        }
    }
}
