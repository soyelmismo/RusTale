pub mod models;
pub mod state;
pub mod utils;
pub mod middleware;
pub mod handlers;
pub mod server;
pub mod crypto;

// Re-exports principales
pub use server::{start_server, is_server_alive};
pub use state::ServerState;
pub use crypto::{set_identity_dir, initialize_constant_keys, update_jwks_from_remote};
