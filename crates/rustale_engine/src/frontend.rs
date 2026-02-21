/// Defines the contract for any frontend that can run on top of the engine.
///
/// This trait allows the launcher binary to be agnostic about the actual
/// frontend being used (Iced GUI, egui, TUI, headless CLI, etc.).
/// Each frontend crate implements this trait and the launcher dispatches to it.
pub trait FrontendRunner {
    /// Run the frontend to completion, returning an exit code.
    fn run(self) -> std::process::ExitCode;
}

/// Configuration passed from the launcher to any frontend.
/// Frontends should not parse CLI args themselves.
#[derive(Debug, Clone, Default)]
pub struct FrontendConfig {
    /// Start in quickplay mode (launch game immediately, no UI shown).
    pub quickplay: bool,
    /// Preferred window width (0 = use frontend default).
    pub width: f32,
    /// Preferred window height (0 = use frontend default).
    pub height: f32,
}
