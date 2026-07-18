use axum::{
    middleware,
    routing::{delete, get, post, put},
    Router,
};
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::{oneshot, Mutex};

use crate::{
    crypto,
    handlers,
    middleware::{cors_middleware, cors_options_handler, log_request},
    state::ServerState,
    utils::{load_skins_from_disk, migrate_skins},
};

/// Check if the server is alive on the given port
pub async fn is_server_alive(port: u16) -> bool {
    // 1. First check if port is actually bound by any process
    if std::net::TcpListener::bind(("127.0.0.1", port)).is_ok() {
        // Port is free, no server running
        return false;
    }

    // 2. Port is bound, now verify it's actually our server
    let url = format!("http://127.0.0.1:{}/health", port);

    match rustale_shared::HTTP_CLIENT
        .get(&url)
        .timeout(std::time::Duration::from_millis(2000))
        .send()
        .await
    {
        Ok(resp) => {
            if resp.status().is_success() {
                match resp.text().await {
                    Ok(text) => text.contains("hytale-rust-emulator"),
                    Err(_) => false,
                }
            } else {
                false
            }
        }
        Err(_) => false,
    }
}

/// Start the authentication server
pub async fn start_server(
    username: String,
    uuid: String,
    game_folder_path: PathBuf,
    identity_dir: PathBuf,
    shutdown_rx: oneshot::Receiver<()>,
    port: u16,
) -> anyhow::Result<()> {
    // 0. Initialize crypto keys
    println!("Initializing constant JWKS...");
    println!("[Server] Identity directory: {:?}", identity_dir);
    
    if let Err(_) = crypto::set_identity_dir(identity_dir.clone()) {
        // Only log warning, don't return Err if it was already set
        println!("[Server] Identity directory was already initialized by caller.");
    }
    
    crypto::initialize_constant_keys();


    // 1. Load skins from disk
    let mut skins = load_skins_from_disk(&identity_dir).await;

    // 2. Migrate legacy skin formats
    if migrate_skins(&mut skins).await {
        crate::utils::save_skins_to_disk(&skins, &identity_dir).await;
        println!("[Server] Global migration completed and saved to disk.");
    }

    // 3. Create shared state
    let state = Arc::new(Mutex::new(ServerState {
        username,
        uuid,
        skins,
        game_dir: game_folder_path,
        last_server_uuid: None,
    }));

    // 4. Build router
    let app = create_router(state, port);

    // 5. Bind to loopback address
    let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), port);

    // Check if port is available
    {
        match std::net::TcpListener::bind(addr) {
            Ok(_) => {
                // Port is free
            }
            Err(_) => {
                eprintln!("Port {} is already in use.", port);
                return Err(anyhow::anyhow!("Port in use"));
            }
        }
    }

    println!("Seamless Server listening on loopback: http://{}", addr);

    // 6. Create TCP listener
    let listener = tokio::net::TcpListener::bind(addr).await?;

    // 7. Serve with graceful shutdown
    axum::serve(listener, app)
        .with_graceful_shutdown(async {
            shutdown_rx.await.ok();
        })
        .await?;

    Ok(())
}

fn create_router(state: Arc<Mutex<ServerState>>, _port: u16) -> Router {
    // System routes
    let system_routes = Router::new()
        .route("/health", get(handlers::health))
        .route("/jwks.json", get(handlers::jwks))
        .route("/.well-known/jwks.json", get(handlers::jwks))
        .route("/bugs/create", post(handlers::no_content_stub))
        .route("/feedback/create", post(handlers::no_content_stub));

    // Telemetry stubs (catch-all)
    let telemetry_routes = Router::new()
        .route("/telemetry/{*path}", get(handlers::ok_stub).post(handlers::ok_stub))
        .route("/analytics/{*path}", get(handlers::ok_stub).post(handlers::ok_stub))
        .route("/event/{*path}", get(handlers::ok_stub).post(handlers::ok_stub));

    // Account management routes
    let account_routes = Router::new()
        .route("/my-account/game-profile", get(handlers::handle_game_profile))
        .route("/my-account/skin", put(handlers::handle_skin_put))
        .route("/my-account/cosmetics", get(handlers::handle_cosmetics_inventory_get))
        .route("/my-account/get-launcher-data", get(handlers::handle_launcher_data))
        .route("/my-account/get-profiles", get(handlers::handle_get_profiles))
        .route("/account-data/skin/{uuid}", get(handlers::handle_account_data_skin_get));

    // Player skins routes
    let player_skins_routes = Router::new()
        .route("/player-skins", get(handlers::handle_player_skins_get))
        .route("/player-skins", post(handlers::handle_player_skins_post))
        .route("/player-skins/active", put(handlers::handle_player_skins_set_active))
        .route("/player-skins/{skin_id}", put(handlers::handle_player_skins_update))
        .route("/player-skins/{skin_id}", delete(handlers::handle_player_skins_delete));

    // Game session routes
    let session_routes = Router::new()
        .route("/game-session/child", post(handlers::handle_session_child))
        .route("/game-session/new", post(handlers::handle_session_new))
        .route("/game-session/refresh", post(handlers::handle_session_refresh))
        .route("/game-session", delete(handlers::no_content_stub))
        .route("/game-session/authorize", post(handlers::handle_session_authorize))
        .route("/game-session/exchange", post(handlers::handle_session_exchange))
        // Legacy aliases for older clients
        .route("/server-join/auth-grant", post(handlers::handle_session_authorize))
        .route("/server-join/auth-token", post(handlers::handle_session_exchange));

    // Profile lookup routes
    let profile_routes = Router::new()
        .route("/profile/uuid/{uuid}", get(handlers::handle_profile_lookup_uuid))
        .route("/profile/username/{username}", get(handlers::handle_profile_lookup_username));

    // Server routes
    let server_routes = Router::new()
        .route("/server/auto-auth", post(handlers::handle_server_auto_auth));

    // Internal API routes
    let internal_routes = Router::new()
        .route("/internal/update-path", post(handlers::handle_update_path))
        .route("/internal/update-identity", post(handlers::handle_update_identity));

    // Cosmetics routes
    let cosmetics_routes = Router::new()
        .route("/cosmetics/list", get(handlers::handle_cosmetics_list_get));

    // Discovery routes
    let discovery_routes = Router::new()
        .route("/servers/listings", get(handlers::handle_listings_get))
        .route("/servers/{uuid}/interaction/{action}", post(handlers::handle_interaction_post));

    // Combine all routes
    Router::new()
        .merge(system_routes)
        .merge(telemetry_routes)
        .merge(account_routes)
        .merge(player_skins_routes)
        .merge(session_routes)
        .merge(profile_routes)
        .merge(server_routes)
        .merge(internal_routes)
        .merge(cosmetics_routes)
        .merge(discovery_routes)
        .route("/{*path}", axum::routing::options(cors_options_handler))
        .fallback(handlers::not_found)
        .layer(middleware::from_fn(cors_middleware))
        .layer(middleware::from_fn(log_request))
        .with_state(state)
}
