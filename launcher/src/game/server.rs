use crate::game::crypto;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::io::Read;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::oneshot;
use uuid::Uuid;
use warp::Filter;

#[derive(Deserialize)]
struct UpdatePathRequest {
    game_dir: String,
}

#[derive(Deserialize)]
struct UpdateIdentityRequest {
    username: String,
    uuid: String,
}

// ==================== DATA STRUCTURES (hytFormats.go ported) ====================

#[derive(Deserialize, Debug)]
struct SessionRequest {
    pub uuid: Option<String>,
    pub name: Option<String>,
    #[serde(alias = "scope")]
    scopes: Option<Vec<String>>,
    #[serde(flatten)]
    extra: HashMap<String, serde_json::Value>,
}

#[derive(Deserialize, Debug)]
struct RefreshRequest {
    #[serde(rename = "sessionToken")]
    session_token: String,
    #[serde(flatten)]
    extra: HashMap<String, serde_json::Value>,
}

#[derive(Deserialize, Debug)]
struct AuthorizeRequest {
    #[serde(rename = "identityToken")]
    identity_token: String,
    // El servidor al que te quieres unir
    #[serde(alias = "server_id")]
    audience: Option<String>,
    #[serde(alias = "scope")]
    scopes: Option<Vec<String>>,
    #[serde(flatten)]
    extra: HashMap<String, serde_json::Value>,
}

#[derive(Serialize)]
struct AuthorizeResponse {
    #[serde(rename = "authorizationGrant")]
    authorization_grant: String,
    #[serde(rename = "expiresAt")]
    expires_at: DateTime<Utc>,
}

#[derive(Deserialize, Debug)]
struct ExchangeRequest {
    #[serde(rename = "authorizationGrant")]
    authorization_grant: String,
    #[serde(alias = "scope")]
    scopes: Option<Vec<String>>,
    #[serde(flatten)]
    extra: HashMap<String, serde_json::Value>,
}

#[derive(Deserialize, Debug)]
struct ServerAutoAuthRequest {
    #[serde(alias = "server_id")]
    server_id: Option<String>,
    #[serde(alias = "serverId")]
    server_id_alt: Option<String>,
    #[serde(alias = "server_name")]
    server_name: Option<String>,
    #[serde(alias = "serverName")]
    server_name_alt: Option<String>,
    #[serde(flatten)]
    extra: HashMap<String, serde_json::Value>,
}

#[derive(Serialize)]
struct ExchangeResponse {
    #[serde(rename = "accessToken")]
    access_token: String,
    #[serde(rename = "tokenType")]
    token_type: String,
    #[serde(rename = "expiresIn")]
    expires_in: i64,
    #[serde(rename = "refreshToken")]
    refresh_token: String,
    #[serde(rename = "expiresAt")]
    expires_at: DateTime<Utc>,
    scope: String,
}

#[derive(Serialize)]
struct ServerAutoAuthResponse {
    #[serde(rename = "identityToken")]
    identity_token: String,
    #[serde(rename = "sessionToken")]
    session_token: String,
    #[serde(rename = "expiresIn")]
    expires_in: i64,
    #[serde(rename = "expiresAt")]
    expires_at: DateTime<Utc>,
    #[serde(rename = "tokenType")]
    token_type: String,
    #[serde(rename = "serverId")]
    server_id: String,
    #[serde(rename = "serverUuid")]
    server_uuid: String,
    #[serde(rename = "serverName")]
    server_name: String,
}

#[derive(Serialize)]
struct ProfileLookupResponse {
    uuid: String,
    username: String,
}

#[derive(Serialize)]
struct HealthResponse {
    status: String,
    server: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct Confirmation {
    #[serde(rename = "x5t#S256")]
    pub x5t_s256: String,
}

// Payload especifico para JWT de Auth Grant y Access Token
#[derive(Debug, Serialize, Deserialize)]
struct AuthTokenPayload {
    exp: i64,
    iat: i64,
    iss: String,
    jti: String,
    scope: String,
    sub: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    aud: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    username: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    entitlements: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    cnf: Option<Confirmation>, // <--- Certificate Binding
}

#[derive(Serialize, Deserialize, Clone)]
struct LauncherData {
    #[serde(rename = "EulaAcceptedAt")] 
    eula_accepted_at: DateTime<Utc>,
    
    #[serde(rename = "Owner")] 
    owner: String,
    
    #[serde(rename = "Patchlines")]
    patchlines: Patchlines,
    
    #[serde(rename = "Profiles")]
    profiles: Vec<LauncherProfileInfo>,
}

#[derive(Serialize, Deserialize, Clone)]
struct Patchlines {
    #[serde(rename = "PreRelease")]
    pre_release: GameVersionInfo,
    
    #[serde(rename = "Release")]
    release: GameVersionInfo,
}

#[derive(Serialize, Deserialize, Clone)]
struct GameVersionInfo {
    #[serde(rename = "BuildVersion")]
    build_version: String,
    
    #[serde(rename = "Newest")]
    newest: i32,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct AccountInfo {
    #[serde(rename = "createdAt", alias = "CreatedAt")]
    created_at: DateTime<Utc>,

    #[serde(alias = "Entitlements")]
    entitlements: Vec<String>,
    
    #[serde(rename = "nextNameChangeAt")]
    next_name_change_at: DateTime<Utc>,
    
    #[serde(skip_serializing_if = "Option::is_none")]
    skin: Option<String>,
    
    #[serde(alias = "Username")]
    username: String,
    
    #[serde(alias = "UUID")]
    uuid: String,
}

#[derive(Deserialize)]
struct CosmeticDefinition {
    #[serde(rename = "Id")]
    id: String,
}

#[derive(Serialize, Deserialize, Default)]
struct SessionNewResponse {
    #[serde(rename = "expiresAt")]
    expires_at: DateTime<Utc>,
    #[serde(rename = "identityToken")]
    identity_token: String,
    #[serde(rename = "sessionToken")]
    session_token: String,
}

#[derive(Serialize, Deserialize)]
struct JwtHeader {
    alg: String,
    kid: String,
    typ: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    jwk: Option<serde_json::Value>,
}

#[derive(Serialize, Deserialize)]
struct IdentityTokenPayload {
    exp: i64,
    iat: i64,
    iss: String,
    jti: String,
    scope: String,
    sub: String,
    name: String,
    username: String,
    entitlements: Vec<String>,
    profile: ProfileInfo,
}

#[derive(Serialize, Deserialize)]
struct SessionTokenPayload {
    exp: i64,
    iat: i64,
    iss: String,
    jti: String,
    scope: String,
    sub: String,
}

#[derive(Serialize, Deserialize)]
struct ProfileInfo {
    username: String,
    //entitlements: Vec<String>,
    //skin: serde_json::Value, // Changed from String to Value to ensure JSON object
}

// ==================== SERVER LOGIC ====================

const ENTITLEMENTS: &[&str] = &["game.base", "game.deluxe", "game.founder", "game.server"];

// Default skin JSON (Fallback)
const DEFAULT_SKIN: &str = r#"{"bodyCharacteristic":"Muscular.09","underwear":"Boxer.Purple","face":"Face_Neutral","ears":"Default","mouth":"Mouth_Long","haircut":"SuperSlickback.PitchBlack","facialHair":null,"eyebrows":"Thin.PitchBlack","eyes":"Large_Eyes.GreenLight","pants":"Bermuda_Rolled.GreyBlue","overpants":null,"undertop":null,"overtop":"Winter_Jacket.Red","shoes":"BasicShoes_Sandals.Black","headAccessory":"StrawHat.Red","faceAccessory":"Plaster.Brown","earAccessory":null,"skinFeature":null,"gloves":null,"cape":null}"#;

/// Generate a deterministic server UUID based on server_id (SHA-256 hash)
fn generate_server_uuid(server_id: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(server_id.as_bytes());
    let hash = hasher.finalize();

    // Take first 16 bytes and format as UUID
    let bytes = &hash[..16];
    format!(
        "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        bytes[0],
        bytes[1],
        bytes[2],
        bytes[3],
        bytes[4],
        bytes[5],
        bytes[6],
        bytes[7],
        bytes[8],
        bytes[9],
        bytes[10],
        bytes[11],
        bytes[12],
        bytes[13],
        bytes[14],
        bytes[15]
    )
}

struct ServerState {
    username: String,
    uuid: String,
    skins: HashMap<String, serde_json::Value>, // Changed to store JSON values
    game_dir: PathBuf,
    last_server_uuid: Option<String>, // <--- NUEVO
}

pub async fn is_server_alive(port: u16) -> bool {
    // 1. First check if port is actually bound by any process
    if std::net::TcpListener::bind(("127.0.0.000001", port)).is_ok() {
        // Port is free, no server running
        return false;
    }
    
    // 2. Port is bound, now verify it's actually our server
    let client = reqwest::Client::new();
    let url = format!("http://127.0.0.000001:{}/health", port);

    match client
        .get(&url)
        .timeout(std::time::Duration::from_millis(2000)) // Increased timeout for Linux
        .send()
        .await
    {
        Ok(resp) => {
            // Additional verification: check if it's actually our server
            if resp.status().is_success() {
                // Try to parse response to ensure it's our health endpoint
                match resp.text().await {
                    Ok(text) => text.contains("hytale-rust-emulator"),
                    Err(_) => false,
                }
            } else {
                false
            }
        },
        Err(_) => false,
    }
}

pub async fn start_server(
    username: String,
    uuid: String,
    game_folder_path: PathBuf,
    shutdown_rx: oneshot::Receiver<()>,
    port: u16,
) -> anyhow::Result<()> {
    // 0. Inicializar claves
    println!("Initializing constant JWKS (RAM-Only)...");
    crypto::initialize_constant_keys();

    // 1. CARGA DE SKINS (Persistencia)
    let identity_dir = crate::config::get_identity_dir();
    let skin_file = identity_dir.join("skins.json");

    let skins: HashMap<String, serde_json::Value> = if skin_file.exists() {
        // Leemos el archivo
        match tokio::fs::read_to_string(&skin_file).await {
            Ok(content) => {
                // Intentamos parsear. Si falla (archivo corrupto), iniciamos vacio.
                serde_json::from_str(&content).unwrap_or_default()
            },
            Err(e) => {
                eprintln!("[Server] Error reading skins.json: {}", e);
                HashMap::new()
            }
        }
    } else {
        HashMap::new()
    };

    // Shared state inicializado con las skins cargadas
    let state = Arc::new(tokio::sync::Mutex::new(ServerState {
        username,
        uuid,
        skins, // <--- Aqui pasamos el mapa cargado
        game_dir: game_folder_path,
        last_server_uuid: None,
    }));

    let auth_header = warp::header::optional::<String>("authorization");
    let host_header = warp::header::optional::<String>("host");

    // --- WARP FILTERS ---

    let state_filter = warp::any().map(move || state.clone());

    let internal_update_path = warp::path!("internal" / "update-path")
        .and(warp::post())
        .and(warp::body::json())
        .and(state_filter.clone())
        .then(handle_update_path);

    let internal_update_identity = warp::path!("internal" / "update-identity")
        .and(warp::post())
        .and(warp::body::json())
        .and(state_filter.clone())
        .then(handle_update_identity);

    // 1. GET /my-account/game-profile
    let game_profile = warp::path!("my-account" / "game-profile")
        .and(warp::get())
        .and(auth_header.clone())
        .and(state_filter.clone())
        .then(handle_game_profile);

    // 2. PUT /my-account/skin (Save skin)
    let skin_put = warp::path!("my-account" / "skin")
        .and(warp::put())
        .and(auth_header.clone())
        .and(warp::body::bytes())
        .and(state_filter.clone())
        .then(handle_skin_put);

    // 3. GET /my-account/cosmetics (Read Assets.zip)
    let cosmetics_get = warp::path!("my-account" / "cosmetics")
        .and(warp::get())
        .and(state_filter.clone())
        .then(handle_cosmetics_get);

    // 4. GET /my-account/get-launcher-data
    let launcher_data = warp::path!("my-account" / "get-launcher-data")
        .and(warp::get())
        .and(state_filter.clone())
        .then(handle_launcher_data);

    // 5. POST /game-session/child (Auth)
    let session_child = warp::path!("game-session" / "child")
        .and(warp::post())
        .and(warp::body::json())
        .and(host_header.clone())
        .and(state_filter.clone())
        .then(move |body, host, state| handle_session_child(port, body, host, state));

    // 6. Stubs (Bugs, Feedback)
    let stubs = warp::path!("bugs" / "create")
        .map(|| warp::reply::with_status("", warp::http::StatusCode::NO_CONTENT))
        .or(warp::path!("feedback" / "create")
            .map(|| warp::reply::with_status("", warp::http::StatusCode::NO_CONTENT)));

    let jwks_route = warp::path!("jwks.json").and(warp::get()).map(|| {
        let jwks = crypto::get_host_jwks();
        warp::reply::json(&jwks)
    });

    let permissive_jwks = warp::get()
        .and(warp::path::tail())
        .and_then(|tail: warp::path::Tail| async move {
            if tail.as_str().ends_with("jwks.json") {
                Ok(())
            } else {
                Err(warp::reject::not_found())
            }
        })
        .map(|_| {
            let jwks = crypto::get_host_jwks();
            warp::reply::json(&jwks)
        });

    let health = warp::path!("health").and(warp::get()).map(|| {
        warp::reply::json(&HealthResponse {
            status: "ok".into(),
            server: "hytale-rust-emulator".into(),
        })
    });

    // 8. POST /game-session/new (Login inicial)
    let session_new = warp::path!("game-session" / "new")
        .and(warp::post())
        .and(warp::body::json())
        .and(host_header.clone())
        .and(state_filter.clone())
        .then(move |body: SessionRequest, host, state| handle_session_new(port, body, host, state));

    // 9. POST /game-session/refresh (Mantener sesion viva)
    let session_refresh = warp::path!("game-session" / "refresh")
        .and(warp::post())
        .and(warp::body::json())
        .and(host_header.clone())
        .and(state_filter.clone())
        .then(move |body: RefreshRequest, host, state| {
            handle_session_refresh(port, body, host, state)
        });

    // 10. DELETE /game-session (Logout)
    let session_delete = warp::path!("game-session")
        .and(warp::delete())
        .map(|| warp::reply::with_status("", warp::http::StatusCode::NO_CONTENT));

    // 11. POST /game-session/authorize (Paso 1 del Join Server: Cliente pide permiso)
    // Tambien mapea /server-join/auth-grant que a veces usa el cliente antiguo
    let session_authorize = warp::path!("game-session" / "authorize")
        .or(warp::path!("server-join" / "auth-grant"))
        .unify()
        .and(warp::post())
        .and(warp::body::json())
        .and(host_header.clone())
        .and(state_filter.clone())
        .then(move |body: AuthorizeRequest, host, state| {
            handle_session_authorize(port, body, host, state)
        });

    // 12. POST /game-session/exchange (Paso 2 del Join Server: Servidor valida permiso)
    // Tambien mapea /server-join/auth-token
    let session_exchange = warp::path!("game-session" / "exchange")
        .or(warp::path!("server-join" / "auth-token"))
        .unify()
        .and(warp::post())
        .and(warp::body::json())
        .and(host_header.clone())
        .and(state_filter.clone())
        .then(move |body: ExchangeRequest, host, state| {
            handle_session_exchange(port, body, host, state)
        });

    // 13. GET /profile/uuid/{uuid} (Lookup usado por servidores)
    let profile_by_uuid = warp::path!("profile" / "uuid" / String)
        .and(warp::get())
        .and(state_filter.clone())
        .then(handle_profile_lookup_uuid);

    // 14. GET /profile/username/{username}
    let profile_by_username = warp::path!("profile" / "username" / String)
        .and(warp::get())
        .and(state_filter.clone())
        .then(handle_profile_lookup_username);

    // 15. POST /server/auto-auth (Server auto-auth for F2P servers)
    let server_auto_auth = warp::path!("server" / "auto-auth")
        .and(warp::post())
        .and(warp::body::json())
        .and(host_header.clone())
        .and(state_filter.clone())
        .then(
            move |body: ServerAutoAuthRequest, host: Option<String>, state| {
                handle_server_auto_auth(port, body, host, state)
            },
        );

    // 16. ANY /telemetry/{path}
    // 17. ANY /analytics/{path}
    // 18. ANY /event/{path}

    fn ok_response() -> impl warp::Reply {
        warp::reply::json(&json!({ "success": true, "received": true }))
    }

    let telemetry = warp::path!("telemetry" / ..)
        .and(warp::any())
        .map(ok_response);

    let analytics = warp::path!("analytics" / ..)
        .and(warp::any())
        .map(ok_response);

    let event = warp::path!("event" / ..).and(warp::any()).map(ok_response);

    // CORS (Permissive for the local client to not have problems)
    let cors = warp::cors()
        .allow_any_origin()
        .allow_headers(vec!["Authorization", "Content-Type"])
        .allow_methods(vec!["GET", "POST", "PUT", "DELETE"]);

    let log = warp::log::custom(|info| {
        println!(
            "Request: {} {}\n    Headers: {:?}",
            info.method(),
            info.path(),
            info.request_headers()
        );
    });

    let catch_unknown = warp::any()
        .and(warp::path::tail())
        .map(|tail: warp::path::Tail| {
            println!("Unknown endpoint requested: {}", tail.as_str());
            warp::reply::with_status("", warp::http::StatusCode::NOT_FOUND)
        });

    let account_routes = game_profile
        .or(skin_put)
        .or(cosmetics_get)
        .or(launcher_data)
        .boxed();

    let session_routes = session_child
        .or(session_new)
        .or(session_refresh)
        .or(session_delete)
        .or(session_authorize)
        .or(session_exchange)
        .boxed();

    let profile_routes = profile_by_uuid.or(profile_by_username).boxed();

    let system_routes = health.or(jwks_route).or(permissive_jwks).or(stubs).boxed();

    let misc_routes = telemetry
        .or(analytics)
        .or(event)
        .or(server_auto_auth)
        .or(internal_update_path)
        .or(internal_update_identity)
        .or(catch_unknown)
        .boxed();

    // Combinar los grupos ya "cajitas" (boxed)
    let routes = account_routes
        .or(session_routes)
        .or(profile_routes)
        .or(system_routes)
        .or(misc_routes)
        .with(cors)
        .with(log);

    // Start server on 127.0.0.1:{port} - BLINDAJE ESTRICTO A LOOPBACK
    // Esto es CLAVE en Linux para pasar desapercibido por el firewall
    let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), port);

    {
        match std::net::TcpListener::bind(addr) {
            Ok(_) => {
                // El puerto esta libre. El listener se dropea aqui, liberando el puerto.
            }
            Err(_) => {
                eprintln!("Port {} is already in use.", port);
                return Err(anyhow::anyhow!("Port in use"));
            }
        }
    }

    println!("Seamless Server listening on loopback: http://{}", addr);
    let server = warp::serve(routes).bind(addr).await;

    let graceful_server = server.graceful(async {
        shutdown_rx.await.ok();
    });

    tokio::spawn(graceful_server.run());
    Ok(())
}

// --- HANDLERS ---

async fn handle_game_profile(
    auth: Option<String>,
    state: Arc<tokio::sync::Mutex<ServerState>>,
) -> impl warp::Reply {
    let state = state.lock().await;
    let target_uuid = extract_uuid_from_auth(auth, &state.uuid);

    // 1. Obtener la skin (Value/Objeto) del mapa en memoria
    let skin_obj = state.skins.get(&target_uuid)
        .cloned()
        .unwrap_or_else(|| serde_json::from_str(DEFAULT_SKIN).unwrap_or_default());

    // 2. CONVERTIR A STRING (Crucial para cliente de Hytale)
    // El cliente espera: "skin": "{\"hair\":...}"
    // NO: "skin": { "hair": ... }
    let skin_string = serde_json::to_string(&skin_obj).ok();

    let info = AccountInfo {
        uuid: target_uuid.clone(),
        username: state.username.clone(),
        entitlements: vec!["game.base".to_string()],
        created_at: Utc::now(),
        next_name_change_at: Utc::now() + chrono::Duration::days(30),
        skin: skin_string, // String o None
    };

    // 3. Crear respuesta JSON
    let json_resp = warp::reply::json(&info);

    // 4. Agregar headers anti-cache para que los cambios se vean instantaneamente
    warp::reply::with_header(
        warp::reply::with_header(
            warp::reply::with_header(json_resp, "Cache-Control", "no-store, no-cache, must-revalidate"),
            "Pragma", "no-cache"
        ),
        "Expires", "0"
    )
}


async fn handle_skin_put(
    auth: Option<String>,
    body: bytes::Bytes,
    state: Arc<tokio::sync::Mutex<ServerState>>,
) -> impl warp::Reply {
    let mut state = state.lock().await;
    
    // Identificar usuario desde el token (o usar el default del estado)
    let target_uuid = extract_uuid_from_auth(auth, &state.uuid);

    if let Ok(json_str) = String::from_utf8(body.to_vec()) {
        // Intentar parsear el body como Objeto JSON
        if let Ok(serde_json::Value::Object(new_parts)) = serde_json::from_str::<serde_json::Value>(&json_str) {
            
            println!(">>> [SKIN UPDATE] Received update for {}", target_uuid);

            // 1. Obtener la skin actual de memoria, o la default si no existe
            let mut current_skin = state.skins.get(&target_uuid)
                .cloned()
                .unwrap_or_else(|| serde_json::from_str(DEFAULT_SKIN).unwrap_or_default());

            // 2. MERGE INTELIGENTE:
            // Si la actual es un objeto, iteramos sobre los campos nuevos y actualizamos/insertamos.
            if let serde_json::Value::Object(ref mut current_map) = current_skin {
                for (k, v) in new_parts {
                    // Sobrescribe el campo existente o agrega el nuevo
                    current_map.insert(k, v);
                }
            } else {
                // Caso raro: si lo guardado antes no era objeto valido, reemplazamos todo.
                current_skin = serde_json::Value::Object(new_parts);
            }

            // 3. Actualizar memoria RAM
            state.skins.insert(target_uuid.clone(), current_skin);

            // 4. PERSISTENCIA EN DISCO (Async)
            let identity_dir = crate::config::get_identity_dir();
            let save_path = identity_dir.join("skins.json");
            
            // Creamos carpeta si no existe
            if let Some(parent) = save_path.parent() {
                let _ = tokio::fs::create_dir_all(parent).await;
            }

            // Serializar el mapa completo de todas las skins
            if let Ok(serialized) = serde_json::to_string_pretty(&state.skins) {
                if let Err(e) = tokio::fs::write(&save_path, serialized).await {
                    eprintln!("[Server] Error saving skins.json: {}", e);
                } else {
                    println!("[Server] Skin saved to disk successfully.");
                }
            }

            return warp::reply::with_status("Skin saved", warp::http::StatusCode::NO_CONTENT);
        }
    }
    
    println!("[Server] Invalid skin payload received");
    warp::reply::with_status("Invalid Data", warp::http::StatusCode::BAD_REQUEST)
}

async fn handle_cosmetics_get(state: Arc<tokio::sync::Mutex<ServerState>>) -> impl warp::Reply {
    let state = state.lock().await;
    println!("Cosmetics get endpoint requested.");
    // Read Assets.zip from the game folder
    let assets_zip_path = state.game_dir.join("Assets.zip");

    // Execute ZIP reading in a blocking thread because 'zip' is synchronous
    let cosmetics_json =
        tokio::task::spawn_blocking(move || read_cosmetics_from_zip(&assets_zip_path))
            .await
            .unwrap_or_else(|_| "{}".to_string());

    // Return raw JSON (it's already a JSON string)
    warp::reply::with_header(cosmetics_json, "Content-Type", "application/json")
}

// Struct especifico para la respuesta de LauncherData (Campos en mayuscula)
#[derive(Serialize, Deserialize, Clone)]
struct LauncherProfileInfo {
    #[serde(rename = "CreatedAt")]
    created_at: DateTime<Utc>,
    #[serde(rename = "Entitlements")]
    entitlements: Vec<String>,
    #[serde(rename = "NextNameChangeAt")]
    next_name_change_at: DateTime<Utc>,
    #[serde(rename = "Skin", skip_serializing_if = "Option::is_none")]
    skin: Option<String>,
    #[serde(rename = "Username")]
    username: String,
    #[serde(rename = "UUID")]
    uuid: String,
}
async fn handle_launcher_data(state: Arc<tokio::sync::Mutex<ServerState>>) -> impl warp::Reply {
    let state = state.lock().await;
    
    // Recuperar
    let skin_obj = state.skins.get(&state.uuid)
        .cloned()
        .unwrap_or_else(|| serde_json::from_str(DEFAULT_SKIN).unwrap_or_default());

    // Convertir a String
    let skin_string = serde_json::to_string(&skin_obj).ok();

    let data = LauncherData {
        eula_accepted_at: Utc::now(),
        owner: state.uuid.clone(),
        patchlines: Patchlines {
            pre_release: GameVersionInfo {
                build_version: "2026.01.14".into(), // Ejemplo
                newest: 99,
            },
            release: GameVersionInfo {
                build_version: "2026.01.13".into(),
                newest: 99,
            },
        },
        profiles: vec![
            LauncherProfileInfo {
                uuid: state.uuid.clone(),
                username: state.username.clone(),
                entitlements: vec!["game.base".to_string()],
                created_at: Utc::now(),
                next_name_change_at: Utc::now() + chrono::Duration::days(30),
                // Aqui va la String serializada
                skin: skin_string 
            }
        ],
    };

    warp::reply::json(&data)
}

async fn handle_session_child(
    port: u16,
    body: SessionRequest, // Ahora recibe el body
    host: Option<String>,
    state: Arc<tokio::sync::Mutex<ServerState>>,
) -> impl warp::Reply {
    let state = state.lock().await;
    let actual_port = extract_port(host).unwrap_or(port);

    println!("Session child endpoint requested.");
    println!("Body: {:?}", body);
    let requested_uuid = body.uuid.clone().unwrap_or_else(|| state.uuid.clone());
    let requested_name = body.name.clone().unwrap_or_else(|| state.username.clone());

    let identity_token = generate_jwt(
        &requested_name,
        &requested_uuid,
        "hytale:server hytale:client",
        true,
        actual_port,
    );
    let session_token = generate_jwt(
        &requested_name,
        &requested_uuid,
        "hytale:server",
        false,
        actual_port,
    );

    let resp = SessionNewResponse {
        expires_at: Utc::now() + chrono::Duration::hours(10),
        identity_token,
        session_token,
    };

    warp::reply::json(&resp)
}

// --- UTILITIES ---

fn read_cosmetics_from_zip(zip_path: &PathBuf) -> String {
    println!("Assets.zip requested.");
    if !zip_path.exists() {
        println!("Assets.zip not found at: {:?}", zip_path);
        return "{}".to_string();
    }

    let file = match std::fs::File::open(zip_path) {
        Ok(f) => f,
        Err(_) => return "{}".to_string(),
    };

    let mut archive = match zip::ZipArchive::new(file) {
        Ok(a) => a,
        Err(_) => return "{}".to_string(),
    };

    let mut inventory: HashMap<String, Vec<String>> = HashMap::new();

    // List of JSON files inside Assets.zip
    let categories = vec![
        "BodyCharacteristics",
        "Capes",
        "EarAccessory",
        "Ears",
        "Eyebrows",
        "Eyes",
        "Faces",
        "FaceAccessory",
        "FacialHair",
        "Gloves",
        "Haircuts",
        "HeadAccessory",
        "Mouths",
        "Overpants",
        "Overtops",
        "Pants",
        "Shoes",
        "SkinFeatures",
        "Undertops",
        "Underwear",
    ];

    for category in categories {
        let inner_path = format!("Cosmetics/CharacterCreator/{}.json", category);

        if let Ok(mut file) = archive.by_name(&inner_path) {
            let mut content = String::new();
            if file.read_to_string(&mut content).is_ok() {
                if let Ok(defs) = serde_json::from_str::<Vec<CosmeticDefinition>>(&content) {
                    let ids: Vec<String> = defs.into_iter().map(|d| d.id).collect();

                    // Here we use the strict mapping
                    let field_name = get_exact_field_name(category);
                    inventory.insert(field_name, ids);
                }
            }
        }
    }

    serde_json::to_string(&inventory).unwrap_or("{}".to_string())
}

fn get_exact_field_name(cat: &str) -> String {
    println!("Get exact field name requested.");
    match cat {
        "BodyCharacteristics" => "bodyCharacteristic".to_string(),
        "Capes" => "cape".to_string(),
        "Faces" => "face".to_string(),
        "Haircuts" => "haircut".to_string(),
        "Mouths" => "mouth".to_string(),
        "Overtops" => "overtop".to_string(),
        "Undertops" => "undertop".to_string(),
        "SkinFeatures" => "skinFeature".to_string(),

        "EarAccessory" => "earAccessory".to_string(),
        "Ears" => "ears".to_string(),
        "Eyebrows" => "eyebrows".to_string(),
        "Eyes" => "eyes".to_string(),
        "FaceAccessory" => "faceAccessory".to_string(),
        "FacialHair" => "facialHair".to_string(),
        "Gloves" => "gloves".to_string(),
        "HeadAccessory" => "headAccessory".to_string(),
        "Overpants" => "overpants".to_string(),
        "Pants" => "pants".to_string(),
        "Shoes" => "shoes".to_string(),
        "Underwear" => "underwear".to_string(),

        // Dynamic fallback
        _ => {
            let mut c = cat.chars();
            match c.next() {
                None => String::new(),
                Some(f) => f.to_lowercase().collect::<String>() + c.as_str(),
            }
        }
    }
}

async fn handle_session_new(
    port: u16,
    body: SessionRequest,
    host: Option<String>,
    state: Arc<tokio::sync::Mutex<ServerState>>,
) -> impl warp::Reply {
    let state = state.lock().await;
    let actual_port = extract_port(host).unwrap_or(port);

    // USAMOS la variable 'body' imprimiendola. Rust ya no se quejara.
    println!(">>> [SESSION NEW] Solicitud recibida");
    println!("    Scopes solicitados: {:?}", body.scopes);
    println!("    Extra fields: {:?}", body.extra);

    let skin = state
        .skins
        .get(&state.uuid)
        .cloned()
        .unwrap_or_else(|| serde_json::from_str(DEFAULT_SKIN).unwrap_or_default());

    // Si el cliente pide scopes especificos, los usamos, si no, ponemos los default
    let scope_str = body
        .scopes
        .map(|s| s.join(" "))
        .unwrap_or_else(|| "hytale:server hytale:client".to_string());

    let identity_token = generate_jwt(
        &state.username,
        &state.uuid,
        &format!("{} hytale:server", scope_str),
        true,
        actual_port,
    );
    let session_token = generate_jwt(
        &state.username,
        &state.uuid,
        "hytale:server",
        false,
        actual_port,
    );

    let resp = SessionNewResponse {
        expires_at: Utc::now() + chrono::Duration::hours(10),
        identity_token,
        session_token,
    };
    warp::reply::json(&resp)
}

// POST /game-session/refresh
async fn handle_session_refresh(
    port: u16,
    body: RefreshRequest,
    host: Option<String>,
    state: Arc<tokio::sync::Mutex<ServerState>>,
) -> impl warp::Reply {
    let state = state.lock().await;
    let actual_port = extract_port(host).unwrap_or(port);

    // Logueamos el token viejo para "usar" la variable
    println!(">>> [SESSION REFRESH] Solicitud recibida");
    println!("    Token anterior (prefix): {:.15}...", body.session_token);
    println!("    Extra fields: {:?}", body.extra);

    let skin = state
        .skins
        .get(&state.uuid)
        .cloned()
        .unwrap_or_else(|| serde_json::from_str(DEFAULT_SKIN).unwrap_or_default());

    // En un refresh, generalmente se renuevan los mismos permisos
    let identity_token = generate_jwt(
        &state.username,
        &state.uuid,
        "hytale:server hytale:client",
        true,
        actual_port,
    );
    let session_token = generate_jwt(
        &state.username,
        &state.uuid,
        "hytale:server",
        false,
        actual_port,
    );

    let resp = SessionNewResponse {
        expires_at: Utc::now() + chrono::Duration::hours(10),
        identity_token,
        session_token,
    };
    warp::reply::json(&resp)
}

// POST /game-session/authorize
async fn handle_session_authorize(
    port: u16,
    body: AuthorizeRequest,
    host: Option<String>,
    state: Arc<tokio::sync::Mutex<ServerState>>,
) -> impl warp::Reply {
    let state = state.lock().await;
    println!(">>> [SESSION AUTHORIZE] Solicitud recibida");
    println!("    Body: {:?}", body);
    let actual_port = extract_port(host).unwrap_or(port);

    println!(">>> [SESSION AUTHORIZE] El cliente solicita unirse a un servidor");
    println!("    Extra fields recibidos: {:?}", body.extra);

    // Intentar encontrar el audience en varios lugares
    let detected_audience = body
        .audience
        .clone()
        .or_else(|| {
            body.extra
                .get("aud")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
        })
        .or_else(|| {
            body.extra
                .get("server_id")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
        })
        .or_else(|| {
            body.extra
                .get("serverId")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
        });

    let audience = match detected_audience {
        Some(a) if a != "hytale-client" => a, // Evitar usar el client ID como server ID
        _ => {
            // FALLBACK CRITICO: Si no hay audience o es el del cliente, usar el ultimo servidor registrado
            if let Some(last_uuid) = &state.last_server_uuid {
                println!(
                    "    ! Usando fallback al ultimo servidor registrado: {}",
                    last_uuid
                );
                last_uuid.clone()
            } else {
                println!(
                    "    ! Advertencia: No se detecto audience y no hay servidor registrado, usando fallback generico"
                );
                "hytale-server".to_string()
            }
        }
    };

    println!("    Audience final (Server UUID): {}", audience);

    let scope_str = body
        .scopes
        .map(|s| s.join(" "))
        .unwrap_or_else(|| "hytale:server hytale:client".to_string());

    // Generamos un "Authorization Grant" que contiene el AUD del servidor de destino
    let auth_grant = generate_advanced_jwt(
        &state.username,
        &state.uuid,
        Some(audience),
        &scope_str,
        actual_port,
        None,
    );

    let resp = AuthorizeResponse {
        authorization_grant: auth_grant,
        expires_at: Utc::now() + chrono::Duration::minutes(5),
    };
    warp::reply::json(&resp)
}

// POST /game-session/exchange
async fn handle_session_exchange(
    port: u16,
    body: ExchangeRequest,
    host: Option<String>,
    state: Arc<tokio::sync::Mutex<ServerState>>,
) -> impl warp::Reply {
    let state = state.lock().await;
    let actual_port = extract_port(host).unwrap_or(port);

    println!(">>> [SESSION EXCHANGE] Generating final Access Token");
    println!("    Body received: {:?}", body);

    // Extraer el audience del authorizationGrant
    let granted_audience = extract_claim_from_jwt(&body.authorization_grant, "aud");
    println!("    Audience recovered from Grant: {:?}", granted_audience);

    // CRITICO: Extraer el fingerprint del certificado x509 para el mTLS binding (cnf claim)
    let fingerprint = body
        .extra
        .get("x509Fingerprint")
        .or_else(|| body.extra.get("certFingerprint"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    if fingerprint.is_some() {
        println!(
            "    Detected fingerprint for mTLS binding: {:?}",
            fingerprint
        );
    }

    let scope_str = body
        .scopes
        .map(|s| s.join(" "))
        .unwrap_or_else(|| "hytale:server hytale:client".to_string());

    // Generamos el Access Token final incluyendo el fingerprint si existe
    let access_token = generate_advanced_jwt(
        &state.username,
        &state.uuid,
        granted_audience,
        &scope_str,
        actual_port,
        fingerprint, // <--- Pasamos el fingerprint
    );

    let refresh_token = generate_jwt(
        &state.username,
        &state.uuid,
        "hytale:server",
        false,
        actual_port,
    );

    let resp = ExchangeResponse {
        access_token,
        token_type: "Bearer".to_string(),
        expires_in: 3600,
        refresh_token,
        expires_at: Utc::now() + chrono::Duration::hours(1),
        scope: scope_str,
    };
    warp::reply::json(&resp)
}

async fn handle_profile_lookup_username(
    username_query: String,
    state: Arc<tokio::sync::Mutex<ServerState>>,
) -> impl warp::Reply {
    let state = state.lock().await;
    println!(">>> [PROFILE LOOKUP] Buscando usuario: {}", username_query);

    // En este emulador simple local, solo "existimos" nosotros.
    // Si buscan nuestro nombre (ignorando mayusculas/minusculas), devolvemos nuestro UUID.
    if username_query.to_lowercase() == state.username.to_lowercase() {
        let resp = ProfileLookupResponse {
            uuid: state.uuid.clone(),
            username: state.username.clone(),
        };
        return warp::reply::with_status(warp::reply::json(&resp), warp::http::StatusCode::OK);
    }

    // Si buscan a otro, 404 Not Found
    warp::reply::with_status(
        warp::reply::json(&json!({ "error": "Profile not found" })),
        warp::http::StatusCode::NOT_FOUND,
    )
}

// GET /profile/uuid/:uuid
async fn handle_profile_lookup_uuid(
    uuid_query: String,
    state: Arc<tokio::sync::Mutex<ServerState>>,
) -> impl warp::Reply {
    let state = state.lock().await;
    println!(">>> [PROFILE LOOKUP] Buscando UUID: {}", uuid_query);

    if uuid_query == state.uuid {
        let resp = ProfileLookupResponse {
            uuid: state.uuid.clone(),
            username: state.username.clone(),
        };
        return warp::reply::with_status(warp::reply::json(&resp), warp::http::StatusCode::OK);
    }

    warp::reply::with_status(
        warp::reply::json(&json!({ "error": "Profile not found" })),
        warp::http::StatusCode::NOT_FOUND,
    )
}

// POST /server/auto-auth (Server auto-auth for F2P servers)
async fn handle_server_auto_auth(
    port: u16,
    body: ServerAutoAuthRequest,
    host: Option<String>,
    state: Arc<tokio::sync::Mutex<ServerState>>,
) -> impl warp::Reply {
    let actual_port = extract_port(host).unwrap_or(port);
    println!(">>> [SERVER AUTO-AUTH] El servidor de juego se esta identificando");
    println!("    Body: {:?}", body);

    // Server can provide its own ID or we generate one
    let server_id = body
        .server_id
        .or(body.server_id_alt)
        .unwrap_or_else(|| Uuid::new_v4().to_string());

    // Generate a server-specific UUID
    let server_uuid = generate_server_uuid(&server_id);

    // GUARDAR EL UUID PARA FUTUROS JOINS
    {
        let mut state_lock = state.lock().await;
        state_lock.last_server_uuid = Some(server_uuid.clone());
        println!("    ! Guardado server_uuid fallback: {}", server_uuid);
    }

    // Server name for logging/identification (optional)
    let server_name = body
        .server_name
        .or(body.server_name_alt)
        .unwrap_or_else(|| format!("Server-{}", &server_id[..8]));

    // Generate tokens with server scope
    let identity_token = generate_jwt(
        &server_name,
        &server_uuid,
        "hytale:server", // Server scope only
        true,            // Include profile info
        actual_port,
    );

    let session_token = generate_jwt(
        &server_name,
        &server_uuid,
        "hytale:server",
        false, // No profile needed for session token
        actual_port,
    );

    let expires_at = Utc::now() + chrono::Duration::hours(10); // 10 hours TTL

    println!(
        ">>> [SERVER AUTO-AUTH] Success: {} ({})",
        server_uuid, server_name
    );

    let resp = ServerAutoAuthResponse {
        identity_token,
        session_token,
        expires_in: 36000, // 10 hours in seconds
        expires_at,
        token_type: "Bearer".to_string(),
        server_id: server_id.clone(),
        server_uuid,
        server_name,
    };

    warp::reply::json(&resp)
}

fn generate_jwt(
    username: &str,
    uuid: &str,
    scope: &str,
    include_profile: bool,
    port: u16,
) -> String {
    println!("JWT generation requested with fresh server keys.");
    
    // Asegurar que tenemos capacidad de firmar localmente
    crypto::ensure_local_signing_capability();
    
    use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};

    let header = JwtHeader {
        alg: "EdDSA".to_string(),
        kid: crypto::KEY_ID.to_string(),
        typ: "JWT".to_string(),
        jwk: Some(crypto::get_server_public_jwk_as_value()),
    };

    let now = Utc::now().timestamp();
    let exp = (Utc::now() + chrono::Duration::hours(10)).timestamp();
    let sub = uuid.to_string();

    let issuer_url = format!("http://127.0.0.000001:{}", port);

    let payload_str = if include_profile {
        let p = IdentityTokenPayload {
            exp,
            iat: now,
            iss: issuer_url,
            jti: Uuid::new_v4().to_string(),
            scope: scope.to_string(),
            sub,
            name: username.to_string(),
            username: username.to_string(),
            entitlements: ENTITLEMENTS.iter().map(|&s| s.to_string()).collect(),
            profile: ProfileInfo {
                username: username.to_string(),
                //entitlements: ENTITLEMENTS.iter().map(|&s| s.to_string()).collect(),
                //skin: serde_json::from_str(skin).unwrap_or_else(|_| serde_json::json!({})),
            },
        };
        serde_json::to_string(&p).unwrap()
    } else {
        let p = SessionTokenPayload {
            exp,
            iat: now,
            iss: issuer_url,
            jti: Uuid::new_v4().to_string(),
            scope: scope.to_string(),
            sub,
        };
        serde_json::to_string(&p).unwrap()
    };

    let header_b64 = URL_SAFE_NO_PAD.encode(serde_json::to_string(&header).unwrap());
    let payload_b64 = URL_SAFE_NO_PAD.encode(payload_str);

    let to_sign = format!("{}.{}", header_b64, payload_b64);
    let signature = crypto::sign_message_with_server_keys(&to_sign);

    format!("{}.{}", to_sign, signature)
}

fn generate_advanced_jwt(
    username: &str,
    uuid: &str,
    audience: Option<String>,
    scope: &str,
    port: u16,
    fingerprint: Option<String>, // <--- NUEVO
) -> String {
    // Asegurar que tenemos capacidad de firmar localmente
    crypto::ensure_local_signing_capability();
    
    use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};

    let header = JwtHeader {
        alg: "EdDSA".to_string(),
        kid: crypto::KEY_ID.to_string(),
        typ: "JWT".to_string(),
        jwk: Some(crypto::get_server_public_jwk_as_value()),
    };

    let now = Utc::now().timestamp();
    let exp = (Utc::now() + chrono::Duration::hours(1)).timestamp();

    let issuer_url = format!("http://127.0.0.000001:{}", port);

    let payload = AuthTokenPayload {
        exp,
        iat: now,
        iss: issuer_url,
        jti: Uuid::new_v4().to_string(),
        scope: scope.to_string(),
        sub: uuid.to_string(),
        aud: audience,
        name: Some(username.to_string()),
        username: Some(username.to_string()), // <--- Replicar para compatibilidad
        entitlements: Some(vec!["game.base".to_string()]),
        cnf: fingerprint.map(|f| Confirmation { x5t_s256: f }), // <--- NUEVO
    };

    let payload_json = serde_json::to_string(&payload).unwrap();

    let header_b64 = URL_SAFE_NO_PAD.encode(serde_json::to_string(&header).unwrap());
    let payload_b64 = URL_SAFE_NO_PAD.encode(payload_json);

    let to_sign = format!("{}.{}", header_b64, payload_b64);
    let signature = crypto::sign_message_with_server_keys(&to_sign);

    format!("{}.{}", to_sign, signature)
}

fn extract_claim_from_jwt(token: &str, claim: &str) -> Option<String> {
    let parts: Vec<&str> = token.split('.').collect();
    if parts.len() < 2 {
        return None;
    }

    use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
    if let Ok(payload_bytes) = URL_SAFE_NO_PAD.decode(parts[1]) {
        if let Ok(payload_json) = serde_json::from_slice::<serde_json::Value>(&payload_bytes) {
            return payload_json
                .get(claim)
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
        }
    }
    None
}

fn extract_uuid_from_auth(auth_header: Option<String>, default_uuid: &str) -> String {
    let header = match auth_header {
        Some(h) => h,
        None => return default_uuid.to_string(),
    };

    let token = header.trim_start_matches("Bearer ").trim();
    extract_claim_from_jwt(token, "sub").unwrap_or_else(|| default_uuid.to_string())
}

async fn handle_update_path(
    body: UpdatePathRequest,
    state: Arc<tokio::sync::Mutex<ServerState>>,
) -> impl warp::Reply {
    let mut state = state.lock().await;
    println!(">>> [INTERNAL] Updating Game Dir to: {}", body.game_dir);

    let new_path = PathBuf::from(&body.game_dir);
    if new_path.exists() {
        state.game_dir = new_path;
        warp::reply::with_status("Path updated", warp::http::StatusCode::OK)
    } else {
        warp::reply::with_status("Path does not exist", warp::http::StatusCode::BAD_REQUEST)
    }
}

async fn handle_update_identity(
    body: UpdateIdentityRequest,
    state: Arc<tokio::sync::Mutex<ServerState>>,
) -> impl warp::Reply {
    let mut state = state.lock().await;
    println!(
        ">>> [INTERNAL] Updating Identity to: {} ({})",
        body.username, body.uuid
    );

    state.username = body.username;
    state.uuid = body.uuid;

    warp::reply::with_status("Identity updated", warp::http::StatusCode::OK)
}

fn extract_port(host: Option<String>) -> Option<u16> {
    host.and_then(|h| h.split(':').nth(1).and_then(|p| p.parse::<u16>().ok()))
}
