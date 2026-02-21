use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use chrono::Utc;
use uuid::Uuid;

use crate::models::{JwtHeader, ENTITLEMENTS};
use crate::crypto;

pub struct TokenConfig {
    pub scope: String,
    pub has_profile: bool, // For IdentityTokenPayload
    pub audience: Option<String>,
    pub fingerprint: Option<String>,
    pub skin: Option<serde_json::Value>,
    pub duration_seconds: i64,
    pub is_advanced: bool, // Switch between Identity/Session layout and Auth layout
}

pub fn create_auth_token(username: &str, uuid: &str, port: u16, config: TokenConfig) -> String {
    crypto::ensure_local_signing_capability();

    let header = JwtHeader {
        alg: "EdDSA".to_string(),
        kid: crypto::KEY_ID.to_string(),
        typ: "JWT".to_string(),
        jwk: Some(crypto::get_server_public_jwk_as_value()),
    };

    let now = Utc::now().timestamp();
    let exp = (Utc::now() + chrono::Duration::seconds(config.duration_seconds)).timestamp();
    let issuer_url = format!("http://127.0.0.000001:{}", port);

    // Build payload dynamically
    let mut payload = serde_json::Map::new();
    payload.insert("exp".into(), serde_json::json!(exp));
    payload.insert("iat".into(), serde_json::json!(now));
    payload.insert("iss".into(), serde_json::json!(issuer_url));
    payload.insert("jti".into(), serde_json::json!(Uuid::new_v4().to_string()));
    payload.insert("scope".into(), serde_json::json!(config.scope));
    payload.insert("sub".into(), serde_json::json!(uuid));

    if config.is_advanced {
        // Advanced JWT (Exchange Response)
        if let Some(aud) = config.audience {
            payload.insert("aud".into(), serde_json::json!(aud));
        }
        payload.insert("name".into(), serde_json::json!(username));
        payload.insert("username".into(), serde_json::json!(username));
        // Hardcoded entitlement for advanced token as per original code
        payload.insert("entitlements".into(), serde_json::json!(vec!["game.base"]));

        if let Some(fp) = config.fingerprint {
            payload.insert("cnf".into(), serde_json::json!({ "x5t#S256": fp }));
        }
        if let Some(s) = config.skin {
            payload.insert("skin".into(), s);
        }
    } else {
        // Standard JWT (Identity or Session)
        if config.has_profile {
            payload.insert("name".into(), serde_json::json!(username));
            payload.insert("username".into(), serde_json::json!(username));

            let entitlements: Vec<String> = ENTITLEMENTS.iter().map(|&s| s.to_string()).collect();
            payload.insert("entitlements".into(), serde_json::json!(entitlements));

            let skin_val = config.skin.unwrap_or_else(|| {
                serde_json::from_str(crate::models::DEFAULT_SKIN)
                    .unwrap_or_else(|_| serde_json::json!({}))
            });

            let profile = serde_json::json!({
                "username": username,
                "entitlements": entitlements,
                "skin": skin_val
            });
            payload.insert("profile".into(), profile);
        }
    }

    let payload_str = serde_json::to_string(&payload).unwrap();
    let header_b64 = URL_SAFE_NO_PAD.encode(serde_json::to_string(&header).unwrap());
    let payload_b64 = URL_SAFE_NO_PAD.encode(payload_str);

    let to_sign = format!("{}.{}", header_b64, payload_b64);
    let signature = crypto::sign_message_with_server_keys(&to_sign);

    format!("{}.{}", to_sign, signature)
}

pub fn extract_claim_from_jwt(token: &str, claim: &str) -> Option<String> {
    let parts: Vec<&str> = token.split('.').collect();
    if parts.len() < 2 {
        return None;
    }

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

pub fn extract_uuid_from_auth(auth_header: Option<axum::http::HeaderValue>, default_uuid: &str) -> String {
    let header = match auth_header {
        Some(h) => h.to_str().unwrap_or("").to_string(),
        None => return default_uuid.to_string(),
    };

    if header.is_empty() {
        return default_uuid.to_string();
    }

    let token = header.trim_start_matches("Bearer ").trim();
    extract_claim_from_jwt(token, "sub").unwrap_or_else(|| default_uuid.to_string())
}

pub fn extract_port(host: Option<String>) -> Option<u16> {
    host.and_then(|h| h.split(':').nth(1).and_then(|p| p.parse::<u16>().ok()))
}

pub fn extract_port_from_headers(headers: &axum::http::HeaderMap) -> Option<u16> {
    headers
        .get("host")
        .and_then(|h| h.to_str().ok())
        .and_then(|h| extract_port(Some(h.to_string())))
}

use sha2::{Digest, Sha256};

/// Generate a deterministic server UUID based on server_id (SHA-256 hash)
pub fn generate_server_uuid(server_id: &str) -> String {
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
