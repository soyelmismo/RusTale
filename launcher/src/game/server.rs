use crate::game::crypto;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::json;
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
struct ProfileLookupResponse {
    uuid: String,
    username: String,
}

#[derive(Serialize)]
struct HealthResponse {
    status: String,
    server: String,
}

// Payload específico para JWT de Auth Grant y Access Token
#[derive(Serialize)]
struct AuthTokenPayload {
    exp: i64,
    iat: i64,
    iss: String,
    jti: String,
    scope: String,
    sub: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    aud: Option<String>, // Audience es vital para Auth Grants
    #[serde(skip_serializing_if = "Option::is_none")]
    name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    entitlements: Option<Vec<String>>,
}

#[derive(Serialize, Deserialize, Clone)]
struct LauncherData {
    #[serde(rename = "eula_accepted_at")]
    eula_accepted_at: DateTime<Utc>,
    owner: String,
    patchlines: Patchlines,
    profiles: Vec<AccountInfo>,
}

#[derive(Serialize, Deserialize, Clone)]
struct Patchlines {
    #[serde(rename = "pre-release")]
    pre_release: GameVersionInfo,
    release: GameVersionInfo,
}

#[derive(Serialize, Deserialize, Clone)]
struct GameVersionInfo {
    #[serde(rename = "buildVersion")]
    build_version: String,
    newest: i32,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct AccountInfo {
    #[serde(rename = "createdAt")]
    created_at: DateTime<Utc>,
    entitlements: Vec<String>,
    #[serde(rename = "nextNameChangeAt")]
    next_name_change_at: DateTime<Utc>,
    skin: String,
    username: String,
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
}

#[derive(Serialize, Deserialize)]
struct IdentityTokenPayload {
    exp: i64,
    iat: i64,
    iss: String,
    jti: String,
    scope: String,
    sub: String,
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
    entitlements: Vec<String>,
    skin: String,
}

// ==================== SERVER LOGIC ====================

const ENTITLEMENTS: &[&str] = &["game.base", "game.deluxe", "game.founder", "game.server"];

// Default skin JSON (Fallback)
const DEFAULT_SKIN: &str = r#"{"bodyCharacteristic":"Muscular.09","underwear":"Boxer.Purple","face":"Face_Neutral","ears":"Default","mouth":"Mouth_Long","haircut":"SuperSlickback.PitchBlack","facialHair":null,"eyebrows":"Thin.PitchBlack","eyes":"Large_Eyes.GreenLight","pants":"Bermuda_Rolled.GreyBlue","overpants":null,"undertop":null,"overtop":"Winter_Jacket.Red","shoes":"BasicShoes_Sandals.Black","headAccessory":"StrawHat.Red","faceAccessory":"Plaster.Brown","earAccessory":null,"skinFeature":null,"gloves":null,"cape":null}"#;

struct ServerState {
    username: String,
    uuid: String,
    skins: HashMap<String, String>,
    game_dir: PathBuf,
}

// AÑADIR ESTA FUNCIÓN AL FINAL O DONDE PREFIERAS
pub async fn is_server_alive(port: u16) -> bool {
    let client = reqwest::Client::new();
    let url = format!("http://127.0.0.1:{}/health", port);

    match client
        .get(&url)
        .timeout(std::time::Duration::from_millis(500))
        .send()
        .await
    {
        Ok(resp) => resp.status().is_success(),
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
    // 1. Load saved skin (Multi-user support)
    // 1. Load saved skin (Multi-user support)
    // Usar RusTale/server/identity/skins.json
    let identity_dir = crate::config::get_identity_dir();
    let skin_file = identity_dir.join("skins.json");

    let auth_header = warp::header::optional::<String>("authorization");

    let skins: HashMap<String, String> = if skin_file.exists() {
        let content = tokio::fs::read_to_string(&skin_file)
            .await
            .unwrap_or_default();
        match serde_json::from_str::<HashMap<String, String>>(&content) {
            Ok(map) => map,
            Err(_) => {
                let mut map = HashMap::new();
                if content.contains("bodyCharacteristic") {
                    map.insert(username.clone(), content);
                }
                map
            }
        }
    } else {
        HashMap::new()
    };

    // Shared state
    let state = Arc::new(tokio::sync::Mutex::new(ServerState {
        username,
        uuid,
        skins,
        game_dir: game_folder_path,
    }));

    // --- WARP FILTERS ---

    let state_filter = warp::any().map(move || state.clone());

    let internal_update_path = warp::path!("internal" / "update-path")
        .and(warp::post())
        .and(warp::body::json())
        .and(state_filter.clone())
        .then(handle_update_path);

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
        .and(warp::body::json()) // Extraer el JSON
        .and(state_filter.clone())
        .then(move |body, state| handle_session_child(port, body, state));

    // 6. Stubs (Bugs, Feedback)
    let stubs = warp::path!("bugs" / "create")
        .map(|| warp::reply::with_status("", warp::http::StatusCode::NO_CONTENT))
        .or(warp::path!("feedback" / "create")
            .map(|| warp::reply::with_status("", warp::http::StatusCode::NO_CONTENT)));

    let jwks_route = warp::path!("jwks.json").and(warp::get()).map(|| {
        let jwks = crypto::get_jwks();
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
            let jwks = crypto::get_jwks();
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
        .and(state_filter.clone())
        .then(move |body: SessionRequest, state| handle_session_new(port, body, state));

    // 9. POST /game-session/refresh (Mantener sesión viva)
    let session_refresh = warp::path!("game-session" / "refresh")
        .and(warp::post())
        .and(warp::body::json())
        .and(state_filter.clone())
        .then(move |body: RefreshRequest, state| handle_session_refresh(port, body, state));

    // 10. DELETE /game-session (Logout)
    let session_delete = warp::path!("game-session")
        .and(warp::delete())
        .map(|| warp::reply::with_status("", warp::http::StatusCode::NO_CONTENT));

    // 11. POST /game-session/authorize (Paso 1 del Join Server: Cliente pide permiso)
    // También mapea /server-join/auth-grant que a veces usa el cliente antiguo
    let session_authorize = warp::path!("game-session" / "authorize")
        .or(warp::path!("server-join" / "auth-grant"))
        .unify()
        .and(warp::post())
        .and(warp::body::json())
        .and(state_filter.clone())
        .then(move |body: AuthorizeRequest, state| handle_session_authorize(port, body, state));

    // 12. POST /game-session/exchange (Paso 2 del Join Server: Servidor valida permiso)
    // También mapea /server-join/auth-token
    let session_exchange = warp::path!("game-session" / "exchange")
        .or(warp::path!("server-join" / "auth-token"))
        .unify()
        .and(warp::post())
        .and(warp::body::json())
        .and(state_filter.clone())
        .then(move |body: ExchangeRequest, state| handle_session_exchange(port, body, state));

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

    // 15. ANY /telemetry/{path}
    // 16. ANY /analytics/{path}
    // 17. ANY /event/{path}

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
        .or(internal_update_path)
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

    // Start server on 127.0.0.1:{port}
    let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)), port);

    {
        match std::net::TcpListener::bind(addr) {
            Ok(_) => {
                // El puerto está libre. El listener se dropea aquí, liberando el puerto.
            }
            Err(_) => {
                eprintln!("Port {} is already in use.", port);
                return Err(anyhow::anyhow!("Port in use"));
            }
        }
    }

    println!("Emulated server started successfully on http://{}", addr);
    let server = warp::serve(routes).bind(addr).await;

    let graceful_server = server.graceful(async {
        shutdown_rx.await.ok();
    });

    tokio::spawn(graceful_server.run());
    Ok(())
}

// --- HANDLERS ---

async fn handle_game_profile(
    auth: Option<String>, // Nueva entrada
    state: Arc<tokio::sync::Mutex<ServerState>>,
) -> impl warp::Reply {
    let state = state.lock().await;

    // Identificar al usuario por su token, o fallback al uuid del estado
    let target_uuid = extract_uuid_from_auth(auth, &state.uuid);

    let skin = state
        .skins
        .get(&target_uuid)
        .map(|s| s.as_str())
        .unwrap_or(DEFAULT_SKIN);

    let info = gen_account_info(&state.username, &target_uuid, skin);
    warp::reply::json(&info)
}

pub fn get_profile(username: &str, uuid: &str) -> AccountInfo {
    let skin = DEFAULT_SKIN;
    println!("Game profile endpoint requested.");
    let info = gen_account_info(username, uuid, skin);
    info
}

async fn handle_skin_put(
    auth: Option<String>, // Nueva entrada
    body: bytes::Bytes,
    state: Arc<tokio::sync::Mutex<ServerState>>,
) -> impl warp::Reply {
    let mut state = state.lock().await;
    let target_uuid = extract_uuid_from_auth(auth, &state.uuid);

    if let Ok(json_str) = String::from_utf8(body.to_vec()) {
        state.skins.insert(target_uuid.clone(), json_str);

        // Persist to disk
        let identity_dir = crate::config::get_identity_dir();
        let save_path = identity_dir.join("skins.json");
        if let Some(parent) = save_path.parent() {
            let _ = tokio::fs::create_dir_all(parent).await;
        }

        if let Ok(serialized) = serde_json::to_string_pretty(&state.skins) {
            let _ = tokio::fs::write(save_path, serialized).await;
        }

        return warp::reply::with_status("Skin saved", warp::http::StatusCode::NO_CONTENT);
    }
    warp::reply::with_status("Invalid UTF-8", warp::http::StatusCode::BAD_REQUEST)
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

async fn handle_launcher_data(state: Arc<tokio::sync::Mutex<ServerState>>) -> impl warp::Reply {
    let state = state.lock().await;
    println!("Launcher data endpoint requested.");
    let skin = state
        .skins
        .get(&state.uuid)
        .map(|s| s.as_str())
        .unwrap_or(DEFAULT_SKIN);

    let data = LauncherData {
        eula_accepted_at: Utc::now(),
        owner: Uuid::new_v4().to_string(),
        patchlines: Patchlines {
            pre_release: GameVersionInfo {
                build_version: "2026.01.14".into(),
                newest: 99,
            },
            release: GameVersionInfo {
                build_version: "2026.01.13".into(),
                newest: 99,
            },
        },
        profiles: vec![gen_account_info(&state.username, &state.uuid, skin)],
    };

    warp::reply::json(&data)
}

async fn handle_session_child(
    port: u16,
    body: SessionRequest, // Ahora recibe el body

    state: Arc<tokio::sync::Mutex<ServerState>>,
) -> impl warp::Reply {
    let state = state.lock().await;
    println!("Session child endpoint requested.");
    println!("Body: {:?}", body);
    let requested_uuid = body.uuid.clone().unwrap_or_else(|| state.uuid.clone());
    let requested_name = body.name.clone().unwrap_or_else(|| state.username.clone());

    let skin = state
        .skins
        .get(&requested_uuid)
        .map(|s| s.as_str())
        .unwrap_or(DEFAULT_SKIN)
        .to_string();

    let identity_token = generate_jwt(
        &requested_name,
        &requested_uuid,
        &skin,
        "hytale:server hytale:client",
        true,
        port,
    );
    let session_token = generate_jwt(
        &requested_name,
        &requested_uuid,
        &skin,
        "hytale:server",
        false,
        port,
    );

    let resp = SessionNewResponse {
        expires_at: Utc::now() + chrono::Duration::hours(10),
        identity_token,
        session_token,
    };

    warp::reply::json(&resp)
}

// --- UTILITIES ---

fn gen_account_info(username: &str, uuid: &str, skin: &str) -> AccountInfo {
    println!("Account info endpoint requested.");
    AccountInfo {
        created_at: Utc::now(),
        entitlements: ENTITLEMENTS.iter().map(|&s| s.to_string()).collect(),
        next_name_change_at: Utc::now(),
        skin: skin.to_string(),
        username: username.to_string(),
        uuid: uuid.to_string(),
    }
}

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
    state: Arc<tokio::sync::Mutex<ServerState>>,
) -> impl warp::Reply {
    let state = state.lock().await;

    // USAMOS la variable 'body' imprimiéndola. Rust ya no se quejará.
    println!(">>> [SESSION NEW] Solicitud recibida");
    println!("    Scopes solicitados: {:?}", body.scopes);
    println!("    Extra fields: {:?}", body.extra);

    let skin = state
        .skins
        .get(&state.uuid)
        .map(|s| s.as_str())
        .unwrap_or(DEFAULT_SKIN);

    // Si el cliente pide scopes específicos, los usamos, si no, ponemos los default
    let scope_str = body
        .scopes
        .map(|s| s.join(" "))
        .unwrap_or_else(|| "hytale:server hytale:client".to_string());

    let identity_token = generate_jwt(
        &state.username,
        &state.uuid,
        skin,
        &format!("{} hytale:server", scope_str),
        true,
        port,
    );
    let session_token = generate_jwt(
        &state.username,
        &state.uuid,
        skin,
        "hytale:server",
        false,
        port,
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
    state: Arc<tokio::sync::Mutex<ServerState>>,
) -> impl warp::Reply {
    let state = state.lock().await;

    // Logueamos el token viejo para "usar" la variable
    println!(">>> [SESSION REFRESH] Solicitud recibida");
    println!("    Token anterior (prefix): {:.15}...", body.session_token);
    println!("    Extra fields: {:?}", body.extra);

    let skin = state
        .skins
        .get(&state.uuid)
        .map(|s| s.as_str())
        .unwrap_or(DEFAULT_SKIN);

    // En un refresh, generalmente se renuevan los mismos permisos
    let identity_token = generate_jwt(
        &state.username,
        &state.uuid,
        skin,
        "hytale:server hytale:client",
        true,
        port,
    );
    let session_token = generate_jwt(
        &state.username,
        &state.uuid,
        skin,
        "hytale:server",
        false,
        port,
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
    state: Arc<tokio::sync::Mutex<ServerState>>,
) -> impl warp::Reply {
    let state = state.lock().await;

    // Aquí vemos qué servidor está intentando conectarse
    println!(">>> [SESSION AUTHORIZE] Un servidor pide permiso");
    println!("    Server Audience ID: {:?}", body.audience);
    println!("    Scopes: {:?}", body.scopes);
    println!("    Extra fields: {:?}", body.extra);

    // Simulamos validación del token de identidad (en prod habría que verificar firma)
    if body.identity_token.is_empty() {
        println!("    ! Advertencia: Identity Token vacío");
    }

    let audience = body
        .audience
        .unwrap_or_else(|| "unknown-server".to_string());
    let scope_str = body
        .scopes
        .map(|s| s.join(" "))
        .unwrap_or_else(|| "hytale:server hytale:client".to_string());

    // Generamos un "Authorization Grant"
    let auth_grant = generate_advanced_jwt(
        &state.username,
        &state.uuid,
        Some(audience),
        &scope_str,
        port,
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
    state: Arc<tokio::sync::Mutex<ServerState>>,
) -> impl warp::Reply {
    let state = state.lock().await;

    println!(">>> [SESSION EXCHANGE] Intercambio de token final");
    println!(
        "    Grant recibido (longitud): {}",
        body.authorization_grant.len()
    );
    println!("    Extra fields: {:?}", body.extra);

    let scope_str = body
        .scopes
        .map(|s| s.join(" "))
        .unwrap_or_else(|| "hytale:server hytale:client".to_string());

    // Generamos el Access Token final que permite jugar
    let access_token = generate_advanced_jwt(
        &state.username,
        &state.uuid,
        Some("hytale-server".to_string()),
        &scope_str,
        port,
    );

    let refresh_token = generate_jwt(
        &state.username,
        &state.uuid,
        "",
        "hytale:server",
        false,
        port,
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
    // Si buscan nuestro nombre (ignorando mayúsculas/minúsculas), devolvemos nuestro UUID.
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

fn generate_jwt(
    username: &str,
    uuid: &str,
    skin: &str,
    scope: &str,
    include_profile: bool,
    port: u16,
) -> String {
    println!("JWT generation requested.");
    use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};

    let header = JwtHeader {
        alg: "EdDSA".to_string(),
        kid: crypto::KEY_ID.to_string(),
        typ: "JWT".to_string(),
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
            profile: ProfileInfo {
                username: username.to_string(),
                entitlements: ENTITLEMENTS.iter().map(|&s| s.to_string()).collect(),
                skin: skin.to_string(),
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
    let signature = crypto::sign_message(&to_sign);

    format!("{}.{}", to_sign, signature)
}

fn generate_advanced_jwt(
    username: &str,
    uuid: &str,
    audience: Option<String>,
    scope: &str,
    port: u16,
) -> String {
    use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};

    let header = JwtHeader {
        alg: "EdDSA".to_string(),
        kid: crypto::KEY_ID.to_string(),
        typ: "JWT".to_string(),
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
        entitlements: Some(vec!["game.base".to_string()]),
    };

    let payload_json = serde_json::to_string(&payload).unwrap();

    let header_b64 = URL_SAFE_NO_PAD.encode(serde_json::to_string(&header).unwrap());
    let payload_b64 = URL_SAFE_NO_PAD.encode(payload_json);

    let to_sign = format!("{}.{}", header_b64, payload_b64);
    let signature = crypto::sign_message(&to_sign);

    format!("{}.{}", to_sign, signature)
}

fn extract_uuid_from_auth(auth_header: Option<String>, default_uuid: &str) -> String {
    let header = match auth_header {
        Some(h) => h,
        None => return default_uuid.to_string(),
    };

    // Formato: "Bearer header.payload.signature"
    let token = header.trim_start_matches("Bearer ").trim();
    let parts: Vec<&str> = token.split('.').collect();

    if parts.len() < 2 {
        return default_uuid.to_string();
    }

    // El payload es la segunda parte
    use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
    if let Ok(payload_bytes) = URL_SAFE_NO_PAD.decode(parts[1]) {
        if let Ok(payload_json) = serde_json::from_slice::<serde_json::Value>(&payload_bytes) {
            if let Some(sub) = payload_json.get("sub").and_then(|s| s.as_str()) {
                return sub.to_string();
            }
        }
    }

    default_uuid.to_string()
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
