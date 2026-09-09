pub mod debug;
pub mod performance;
pub mod theme;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum View {
    #[default]
    Performance,
    Debug,
}

impl View {
    pub fn label(self) -> &'static str {
        match self {
            View::Performance => "PERFORMANCE",
            View::Debug => "DIAGNOSTICS",
        }
    }
}
