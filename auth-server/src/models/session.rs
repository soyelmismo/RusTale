use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Deserialize, Debug)]
pub struct SessionRequest {
    pub uuid: Option<String>,
    pub name: Option<String>,
    #[serde(alias = "scope")]
    pub scopes: Option<Vec<String>>,
    #[serde(flatten)]
    pub extra: std::collections::HashMap<String, serde_json::Value>,
}

#[derive(Deserialize, Debug)]
pub struct RefreshRequest {
    #[serde(rename = "sessionToken")]
    pub session_token: String,
    #[serde(flatten)]
    pub extra: std::collections::HashMap<String, serde_json::Value>,
}

#[derive(Deserialize, Debug)]
pub struct AuthorizeRequest {
    #[serde(alias = "server_id")]
    pub audience: Option<String>,
    #[serde(alias = "scope")]
    pub scopes: Option<Vec<String>>,
    #[serde(flatten)]
    pub extra: std::collections::HashMap<String, serde_json::Value>,
}

#[derive(Serialize)]
pub struct AuthorizeResponse {
    #[serde(rename = "authorizationGrant")]
    pub authorization_grant: String,
    #[serde(rename = "expiresAt")]
    pub expires_at: DateTime<Utc>,
}

#[derive(Deserialize, Debug)]
pub struct ExchangeRequest {
    #[serde(rename = "authorizationGrant")]
    pub authorization_grant: String,
    #[serde(alias = "scope")]
    pub scopes: Option<Vec<String>>,
    #[serde(flatten)]
    pub extra: std::collections::HashMap<String, serde_json::Value>,
}

#[derive(Serialize)]
pub struct ExchangeResponse {
    #[serde(rename = "accessToken")]
    pub access_token: String,
    #[serde(rename = "tokenType")]
    pub token_type: String,
    #[serde(rename = "expiresIn")]
    pub expires_in: i64,
    #[serde(rename = "refreshToken")]
    pub refresh_token: String,
    #[serde(rename = "expiresAt")]
    pub expires_at: DateTime<Utc>,
    pub scope: String,
}

#[derive(Serialize, Deserialize, Default)]
pub struct SessionNewResponse {
    #[serde(rename = "expiresAt")]
    pub expires_at: DateTime<Utc>,
    #[serde(rename = "identityToken")]
    pub identity_token: String,
    #[serde(rename = "sessionToken")]
    pub session_token: String,
}

#[derive(Deserialize, Debug)]
pub struct ServerAutoAuthRequest {
    #[serde(alias = "server_id")]
    pub server_id: Option<String>,
    #[serde(alias = "serverId")]
    pub server_id_alt: Option<String>,
    #[serde(alias = "server_name")]
    pub server_name: Option<String>,
    #[serde(alias = "serverName")]
    pub server_name_alt: Option<String>,
}

#[derive(Serialize)]
pub struct ServerAutoAuthResponse {
    #[serde(rename = "identityToken")]
    pub identity_token: String,
    #[serde(rename = "sessionToken")]
    pub session_token: String,
    #[serde(rename = "expiresIn")]
    pub expires_in: i64,
    #[serde(rename = "expiresAt")]
    pub expires_at: DateTime<Utc>,
    #[serde(rename = "tokenType")]
    pub token_type: String,
    #[serde(rename = "serverId")]
    pub server_id: String,
    #[serde(rename = "serverUuid")]
    pub server_uuid: String,
    #[serde(rename = "serverName")]
    pub server_name: String,
}

#[derive(Serialize, Deserialize)]
pub struct JwtHeader {
    pub alg: String,
    pub kid: String,
    pub typ: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub jwk: Option<serde_json::Value>,
}
