use axum::{extract::State, response::{IntoResponse, Json}, http::HeaderMap};
use std::sync::Arc;
use tokio::sync::Mutex;
use uuid::Uuid;

use crate::{state::ServerState, models::*, utils::*};

// POST /game-session/child
pub async fn handle_session_child(
    State(state): State<Arc<Mutex<ServerState>>>,
    headers: HeaderMap,
    Json(body): Json<SessionRequest>,
) -> impl IntoResponse {
    let state = state.lock().await;
    let actual_port = extract_port_from_headers(&headers).unwrap_or(8080);

    println!("Session child endpoint requested.");
    println!("Body: {:?}", body);
    let requested_uuid = body.uuid.clone().unwrap_or_else(|| state.uuid.clone());
    let requested_name = body.name.clone().unwrap_or_else(|| state.username.clone());

    let user_data = get_user_skins_data(&requested_uuid, &state.skins);
    let skin_val = Some(user_data.skin);

    let identity_token = create_auth_token(
        &requested_name,
        &requested_uuid,
        actual_port,
        TokenConfig {
            scope: "hytale:server hytale:client".to_string(),
            has_profile: true,
            audience: None,
            fingerprint: None,
            skin: skin_val,
            duration_seconds: 36000,
            is_advanced: false,
        },
    );
    let session_token = create_auth_token(
        &requested_name,
        &requested_uuid,
        actual_port,
        TokenConfig {
            scope: "hytale:server".to_string(),
            has_profile: false,
            audience: None,
            fingerprint: None,
            skin: None,
            duration_seconds: 36000,
            is_advanced: false,
        },
    );

    let resp = SessionNewResponse {
        expires_at: chrono::Utc::now() + chrono::Duration::hours(10),
        identity_token,
        session_token,
    };

    Json(resp)
}

// POST /game-session/new
pub async fn handle_session_new(
    State(state): State<Arc<Mutex<ServerState>>>,
    headers: HeaderMap,
    Json(body): Json<SessionRequest>,
) -> impl IntoResponse {
    let state = state.lock().await;
    let actual_port = extract_port_from_headers(&headers).unwrap_or(8080);

    println!(">>> [SESSION NEW] Solicitud recibida");
    println!("    Scopes solicitados: {:?}", body.scopes);
    println!("    Extra fields: {:?}", body.extra);

    let user_data = get_user_skins_data(&state.uuid, &state.skins);
    let skin_val = Some(user_data.skin);

    let scope_str = body
        .scopes
        .map(|s| s.join(" "))
        .unwrap_or_else(|| "hytale:server hytale:client".to_string());

    let identity_token = create_auth_token(
        &state.username,
        &state.uuid,
        actual_port,
        TokenConfig {
            scope: format!("{} hytale:server", scope_str),
            has_profile: true,
            audience: None,
            fingerprint: None,
            skin: skin_val,
            duration_seconds: 36000,
            is_advanced: false,
        },
    );
    let session_token = create_auth_token(
        &state.username,
        &state.uuid,
        actual_port,
        TokenConfig {
            scope: "hytale:server".to_string(),
            has_profile: false,
            audience: None,
            fingerprint: None,
            skin: None,
            duration_seconds: 36000,
            is_advanced: false,
        },
    );

    let resp = SessionNewResponse {
        expires_at: chrono::Utc::now() + chrono::Duration::hours(10),
        identity_token,
        session_token,
    };
    Json(resp)
}

// POST /game-session/refresh
pub async fn handle_session_refresh(
    State(state): State<Arc<Mutex<ServerState>>>,
    headers: HeaderMap,
    Json(body): Json<RefreshRequest>,
) -> impl IntoResponse {
    let state = state.lock().await;
    let actual_port = extract_port_from_headers(&headers).unwrap_or(8080);

    println!(">>> [SESSION REFRESH] Solicitud recibida");
    println!("    Token anterior (prefix): {:.15}...", body.session_token);
    println!("    Extra fields: {:?}", body.extra);

    let user_data = get_user_skins_data(&state.uuid, &state.skins);
    let skin_val = Some(user_data.skin);

    let identity_token = create_auth_token(
        &state.username,
        &state.uuid,
        actual_port,
        TokenConfig {
            scope: "hytale:server hytale:client".to_string(),
            has_profile: true,
            audience: None,
            fingerprint: None,
            skin: skin_val,
            duration_seconds: 36000,
            is_advanced: false,
        },
    );
    let session_token = create_auth_token(
        &state.username,
        &state.uuid,
        actual_port,
        TokenConfig {
            scope: "hytale:server".to_string(),
            has_profile: false,
            audience: None,
            fingerprint: None,
            skin: None,
            duration_seconds: 36000,
            is_advanced: false,
        },
    );

    let resp = SessionNewResponse {
        expires_at: chrono::Utc::now() + chrono::Duration::hours(10),
        identity_token,
        session_token,
    };
    Json(resp)
}

// POST /game-session/authorize
pub async fn handle_session_authorize(
    State(state): State<Arc<Mutex<ServerState>>>,
    headers: HeaderMap,
    Json(body): Json<AuthorizeRequest>,
) -> impl IntoResponse {
    let state = state.lock().await;
    let actual_port = extract_port_from_headers(&headers).unwrap_or(8080);

    println!(">>> [SESSION AUTHORIZE] Solicitud recibida");
    println!("    Body: {:?}", body);

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
        Some(a) if a != "hytale-client" => a,
        _ => {
            if let Some(last_uuid) = &state.last_server_uuid {
                println!("    ! Usando fallback al ultimo servidor registrado: {}", last_uuid);
                last_uuid.clone()
            } else {
                println!("    ! Advertencia: No se detecto audience y no hay servidor registrado, usando fallback generico");
                "hytale-server".to_string()
            }
        }
    };

    println!("    Audience final (Server UUID): {}", audience);

    let scope_str = body
        .scopes
        .map(|s| s.join(" "))
        .unwrap_or_else(|| "hytale:server hytale:client".to_string());

    let user_data = get_user_skins_data(&state.uuid, &state.skins);
    let skin_val = Some(user_data.skin);

    let auth_grant = create_auth_token(
        &state.username,
        &state.uuid,
        actual_port,
        TokenConfig {
            scope: scope_str,
            has_profile: true,
            audience: Some(audience),
            fingerprint: None,
            skin: skin_val,
            duration_seconds: 3600,
            is_advanced: true,
        },
    );

    let resp = AuthorizeResponse {
        authorization_grant: auth_grant,
        expires_at: chrono::Utc::now() + chrono::Duration::minutes(5),
    };
    Json(resp)
}

// POST /game-session/exchange
pub async fn handle_session_exchange(
    State(state): State<Arc<Mutex<ServerState>>>,
    headers: HeaderMap,
    Json(body): Json<ExchangeRequest>,
) -> impl IntoResponse {
    let state = state.lock().await;
    let actual_port = extract_port_from_headers(&headers).unwrap_or(8080);

    println!(">>> [SESSION EXCHANGE] Generating final Access Token");
    println!("    Body received: {:?}", body);

    let granted_audience = extract_claim_from_jwt(&body.authorization_grant, "aud");
    println!("    Audience recovered from Grant: {:?}", granted_audience);

    let fingerprint = body
        .extra
        .get("x509Fingerprint")
        .or_else(|| body.extra.get("certFingerprint"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    if fingerprint.is_some() {
        println!("    Detected fingerprint for mTLS binding: {:?}", fingerprint);
    }

    let scope_str = body
        .scopes
        .map(|s| s.join(" "))
        .unwrap_or_else(|| "hytale:server hytale:client".to_string());

    let user_data = get_user_skins_data(&state.uuid, &state.skins);
    let skin_val = Some(user_data.skin);

    let access_token = create_auth_token(
        &state.username,
        &state.uuid,
        actual_port,
        TokenConfig {
            scope: scope_str.clone(),
            has_profile: true,
            audience: granted_audience,
            fingerprint,
            skin: skin_val,
            duration_seconds: 3600,
            is_advanced: true,
        },
    );

    let refresh_token = create_auth_token(
        &state.username,
        &state.uuid,
        actual_port,
        TokenConfig {
            scope: "hytale:server".to_string(),
            has_profile: false,
            audience: None,
            fingerprint: None,
            skin: None,
            duration_seconds: 36000,
            is_advanced: false,
        },
    );

    let resp = ExchangeResponse {
        access_token,
        token_type: "Bearer".to_string(),
        expires_in: 3600,
        refresh_token,
        expires_at: chrono::Utc::now() + chrono::Duration::hours(1),
        scope: scope_str,
    };
    Json(resp)
}

// POST /server/auto-auth
pub async fn handle_server_auto_auth(
    State(state): State<Arc<Mutex<ServerState>>>,
    headers: HeaderMap,
    Json(body): Json<ServerAutoAuthRequest>,
) -> impl IntoResponse {
    let mut state = state.lock().await;
    let actual_port = extract_port_from_headers(&headers).unwrap_or(8080);

    println!(">>> [SERVER AUTO-AUTH] El servidor de juego se esta identificando");
    println!("    Body: {:?}", body);

    let server_id = body
        .server_id
        .or(body.server_id_alt)
        .unwrap_or_else(|| Uuid::new_v4().to_string());

    let server_uuid = generate_server_uuid(&server_id);

    // GUARDAR EL UUID PARA FUTUROS JOINS
    state.last_server_uuid = Some(server_uuid.clone());
    println!("    ! Guardado server_uuid fallback: {}", server_uuid);

    let server_name = body
        .server_name
        .or(body.server_name_alt)
        .unwrap_or_else(|| format!("Server-{}", &server_id[..8]));

    let identity_token = create_auth_token(
        &server_name,
        &server_uuid,
        actual_port,
        TokenConfig {
            scope: "hytale:server".to_string(),
            has_profile: true,
            audience: None,
            fingerprint: None,
            skin: None,
            duration_seconds: 36000,
            is_advanced: false,
        },
    );

    let session_token = create_auth_token(
        &server_name,
        &server_uuid,
        actual_port,
        TokenConfig {
            scope: "hytale:server".to_string(),
            has_profile: false,
            audience: None,
            fingerprint: None,
            skin: None,
            duration_seconds: 36000,
            is_advanced: false,
        },
    );

    let expires_at = chrono::Utc::now() + chrono::Duration::hours(10);

    println!(">>> [SERVER AUTO-AUTH] Success: {} ({})", server_uuid, server_name);

    let resp = ServerAutoAuthResponse {
        identity_token,
        session_token,
        expires_in: 36000,
        expires_at,
        token_type: "Bearer".to_string(),
        server_id: server_id.clone(),
        server_uuid,
        server_name,
    };

    Json(resp)
}
