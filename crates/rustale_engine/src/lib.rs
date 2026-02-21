pub mod core;
pub mod frontend;
pub mod game;
pub mod java;
pub mod util;

#[cfg(feature = "news")]
pub mod news;
pub mod system;
pub mod profiles; // Existing one
pub mod lang;
pub mod cli;

// Re-export specific items for convenience
pub use rustale_shared::{LauncherStatus, GameSettings, Profile};
