use anyhow::{Context, Result};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};
use uuid::Uuid;

use crate::game;

#[derive(Debug, Serialize, Deserialize)]
pub struct AuthTokens {
    #[serde(alias = "IdentityToken", alias = "identityToken")]
    pub identity_token: String,

    #[serde(alias = "SessionToken", alias = "sessionToken")]
    pub session_token: String,

    #[serde(alias = "ExpiresAt", alias = "expiresAt", default)]
    pub expires_at_str: String,
}

#[derive(Debug, Serialize)]
struct AuthRequest {
    uuid: String,
    name: String,
    scopes: Vec<String>,
}

pub async fn fetch_remote_tokens(
    client: &reqwest::Client,
    auth_server_url: &str,
    player_name: &str,
    player_uuid: &str,
) -> Result<AuthTokens> {
    // Usamos el endpoint child que es el estándar para launchers
    let url = format!("{}/game-session/child", auth_server_url);

    println!("[Auth] Fetching tokens from: {}", url);

    let body = AuthRequest {
        uuid: player_uuid.to_string(),
        name: player_name.to_string(),
        scopes: vec!["hytale:server".to_string(), "hytale:client".to_string()],
    };

    let response = client
        .post(&url)
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .await
        .context("Failed to connect to auth server")?;

    if !response.status().is_success() {
        let status = response.status();
        let text = response.text().await.unwrap_or_default();
        anyhow::bail!("Auth server returned error {}: {}", status, text);
    }

    let tokens: AuthTokens = response
        .json()
        .await
        .context("Failed to parse auth tokens")?;

    println!("[Auth] Tokens received successfully");
    Ok(tokens)
}

pub fn generate_fake_tokens(player_name: &str, player_uuid: &str, issuer_url: &str) -> AuthTokens {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();

    let exp = now + 36000;

    let expires_at_iso = chrono::DateTime::from_timestamp(exp as i64, 0)
        .unwrap_or_default()
        .to_rfc3339_opts(chrono::SecondsFormat::Millis, true);

    let header = serde_json::json!({
        "alg": "EdDSA",
        "typ": "JWT",
        "kid": crate::game::crypto::KEY_ID
    });

    let id_payload = serde_json::json!({
        "sub": player_uuid,
        "name": player_name,
        "username": player_name,
        "entitlements": ["game.base"],
        "scope": "hytale:server hytale:client",
        "iat": now,
        "exp": exp,
        "iss": issuer_url,
        "jti": Uuid::new_v4().to_string(),
        "profile": game::server::get_profile(player_name, player_uuid)
    });

    let session_payload = serde_json::json!({
        "sub": player_uuid,
        "scope": "hytale:server",
        "iat": now,
        "exp": exp,
        "iss": issuer_url,
        "jti": Uuid::new_v4().to_string()
    });

    let header_b64 = to_b64(&header);

    let signature_b64_id =
        crate::game::crypto::sign_message(&format!("{}.{}", header_b64, to_b64(&id_payload)));

    let signature_b64_session =
        crate::game::crypto::sign_message(&format!("{}.{}", header_b64, to_b64(&session_payload)));

    AuthTokens {
        identity_token: format!(
            "{}.{}.{}",
            header_b64,
            to_b64(&id_payload),
            signature_b64_id
        ),
        session_token: format!(
            "{}.{}.{}",
            header_b64,
            to_b64(&session_payload),
            signature_b64_session
        ),
        expires_at_str: expires_at_iso,
    }
}

fn to_b64<T: Serialize>(data: &T) -> String {
    let json = serde_json::to_string(data).unwrap();
    URL_SAFE_NO_PAD.encode(json.as_bytes())
}
