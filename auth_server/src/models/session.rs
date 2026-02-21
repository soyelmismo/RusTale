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

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration;

    // === SessionRequest Tests ===

    #[test]
    fn test_session_request_deserialization() {
        let json = r#"{"uuid": "test-uuid", "name": "TestPlayer"}"#;
        let req: SessionRequest = serde_json::from_str(json).expect("Failed to deserialize");
        
        assert_eq!(req.uuid, Some("test-uuid".to_string()));
        assert_eq!(req.name, Some("TestPlayer".to_string()));
        assert!(req.scopes.is_none());
    }

    #[test]
    fn test_session_request_with_scopes() {
        let json = r#"{"uuid": "test", "scope": ["minecraft:servers", "minecraft:profile"]}"#;
        let req: SessionRequest = serde_json::from_str(json).expect("Failed to deserialize");
        
        assert_eq!(req.scopes, Some(vec!["minecraft:servers".to_string(), "minecraft:profile".to_string()]));
    }

    #[test]
    fn test_session_request_extra_fields_preserved() {
        let json = r#"{"uuid": "test", "custom_field": "value", "number": 42}"#;
        let req: SessionRequest = serde_json::from_str(json).expect("Failed to deserialize");
        
        assert_eq!(req.extra.get("custom_field").unwrap().as_str().unwrap(), "value");
        assert_eq!(req.extra.get("number").unwrap().as_i64().unwrap(), 42);
    }

    // === AuthorizeRequest Tests ===

    #[test]
    fn test_authorize_request_server_id_alias() {
        // Test that server_id maps to audience
        let json = r#"{"server_id": "my-server"}"#;
        let req: AuthorizeRequest = serde_json::from_str(json).expect("Failed to deserialize");
        
        assert_eq!(req.audience, Some("my-server".to_string()));
    }

    #[test]
    fn test_authorize_request_scopes_alias() {
        let json = r#"{"scope": ["read", "write"]}"#;
        let req: AuthorizeRequest = serde_json::from_str(json).expect("Failed to deserialize");
        
        assert_eq!(req.scopes, Some(vec!["read".to_string(), "write".to_string()]));
    }

    // === AuthorizeResponse Tests ===

    #[test]
    fn test_authorize_response_serialization() {
        let response = AuthorizeResponse {
            authorization_grant: "grant-123".to_string(),
            expires_at: Utc::now() + Duration::hours(1),
        };
        
        let json = serde_json::to_string(&response).expect("Failed to serialize");
        
        assert!(json.contains("authorizationGrant"));
        assert!(json.contains("expiresAt"));
        assert!(json.contains("grant-123"));
    }

    // === ExchangeRequest Tests ===

    #[test]
    fn test_exchange_request_deserialization() {
        let json = r#"{"authorizationGrant": "grant-token", "scope": ["profile"]}"#;
        let req: ExchangeRequest = serde_json::from_str(json).expect("Failed to deserialize");
        
        assert_eq!(req.authorization_grant, "grant-token");
        assert_eq!(req.scopes, Some(vec!["profile".to_string()]));
    }

    // === ExchangeResponse Tests ===

    #[test]
    fn test_exchange_response_serialization() {
        let response = ExchangeResponse {
            access_token: "access-token-123".to_string(),
            token_type: "Bearer".to_string(),
            expires_in: 3600,
            refresh_token: "refresh-token-123".to_string(),
            expires_at: Utc::now() + Duration::hours(1),
            scope: "minecraft:servers".to_string(),
        };
        
        let json = serde_json::to_string(&response).expect("Failed to serialize");
        
        assert!(json.contains("accessToken"));
        assert!(json.contains("refreshToken"));
        assert!(json.contains("Bearer"));
        assert!(json.contains("3600"));
    }

    // === SessionNewResponse Tests ===

    #[test]
    fn test_session_new_response_serialization() {
        let response = SessionNewResponse {
            expires_at: Utc::now() + Duration::hours(24),
            identity_token: "identity-token".to_string(),
            session_token: "session-token".to_string(),
        };
        
        let json = serde_json::to_string(&response).expect("Failed to serialize");
        
        assert!(json.contains("identityToken"));
        assert!(json.contains("sessionToken"));
        assert!(json.contains("expiresAt"));
    }

    #[test]
    fn test_session_new_response_deserialization() {
        let json = r#"{"expiresAt": "2024-12-31T23:59:59Z", "identityToken": "id-token", "sessionToken": "sess-token"}"#;
        let response: SessionNewResponse = serde_json::from_str(json).expect("Failed to deserialize");
        
        assert_eq!(response.identity_token, "id-token");
        assert_eq!(response.session_token, "sess-token");
    }

    #[test]
    fn test_session_new_response_default() {
        let response = SessionNewResponse::default();
        
        // Default should have empty tokens
        assert!(response.identity_token.is_empty());
        assert!(response.session_token.is_empty());
    }

    // === ServerAutoAuthRequest Tests ===

    #[test]
    fn test_server_auto_auth_request_server_id_aliases() {
        // Test snake_case version
        let json1 = r#"{"server_id": "srv-1"}"#;
        let req1: ServerAutoAuthRequest = serde_json::from_str(json1).expect("Failed to deserialize");
        assert_eq!(req1.server_id, Some("srv-1".to_string()));
        
        // Test camelCase version
        let json2 = r#"{"serverId": "srv-2"}"#;
        let req2: ServerAutoAuthRequest = serde_json::from_str(json2).expect("Failed to deserialize");
        assert_eq!(req2.server_id_alt, Some("srv-2".to_string()));
    }

    #[test]
    fn test_server_auto_auth_request_server_name_aliases() {
        let json = r#"{"server_name": "My Server", "serverName": "My Server Alt"}"#;
        let req: ServerAutoAuthRequest = serde_json::from_str(json).expect("Failed to deserialize");
        
        assert_eq!(req.server_name, Some("My Server".to_string()));
        assert_eq!(req.server_name_alt, Some("My Server Alt".to_string()));
    }

    // === ServerAutoAuthResponse Tests ===

    #[test]
    fn test_server_auto_auth_response_serialization() {
        let response = ServerAutoAuthResponse {
            identity_token: "id-token".to_string(),
            session_token: "session-token".to_string(),
            expires_in: 86400,
            expires_at: Utc::now() + Duration::days(1),
            token_type: "Bearer".to_string(),
            server_id: "server-123".to_string(),
            server_uuid: "uuid-456".to_string(),
            server_name: "Test Server".to_string(),
        };
        
        let json = serde_json::to_string(&response).expect("Failed to serialize");
        
        assert!(json.contains("identityToken"));
        assert!(json.contains("serverId"));
        assert!(json.contains("serverUuid"));
        assert!(json.contains("serverName"));
        assert!(json.contains("Test Server"));
    }

    // === JwtHeader Tests ===

    #[test]
    fn test_jwt_header_serialization() {
        let header = JwtHeader {
            alg: "EdDSA".to_string(),
            kid: "rustale-host-v1".to_string(),
            typ: "JWT".to_string(),
            jwk: None,
        };
        
        let json = serde_json::to_string(&header).expect("Failed to serialize");
        
        assert!(json.contains("EdDSA"));
        assert!(json.contains("JWT"));
        assert!(!json.contains("jwk")); // Should be omitted when None
    }

    #[test]
    fn test_jwt_header_with_jwk() {
        let header = JwtHeader {
            alg: "EdDSA".to_string(),
            kid: "key-1".to_string(),
            typ: "JWT".to_string(),
            jwk: Some(serde_json::json!({"kty": "OKP", "crv": "Ed25519"})),
        };
        
        let json = serde_json::to_string(&header).expect("Failed to serialize");
        
        assert!(json.contains("jwk"));
        assert!(json.contains("OKP"));
    }

    #[test]
    fn test_jwt_header_deserialization() {
        let json = r#"{"alg": "EdDSA", "kid": "test-key", "typ": "JWT"}"#;
        let header: JwtHeader = serde_json::from_str(json).expect("Failed to deserialize");
        
        assert_eq!(header.alg, "EdDSA");
        assert_eq!(header.kid, "test-key");
        assert_eq!(header.typ, "JWT");
        assert!(header.jwk.is_none());
    }

    // === Serialization Format Tests ===

    #[test]
    fn test_authorize_response_json_structure() {
        let response = AuthorizeResponse {
            authorization_grant: "grant-xyz".to_string(),
            expires_at: Utc::now(),
        };
        
        let json = serde_json::to_string(&response).expect("Failed to serialize");
        let value: serde_json::Value = serde_json::from_str(&json).expect("Invalid JSON");
        
        // Verify camelCase field names
        assert!(value.get("authorizationGrant").is_some());
        assert!(value.get("expiresAt").is_some());
        assert_eq!(value["authorizationGrant"], "grant-xyz");
    }

    #[test]
    fn test_exchange_response_json_structure() {
        let response = ExchangeResponse {
            access_token: "access".to_string(),
            token_type: "Bearer".to_string(),
            expires_in: 3600,
            refresh_token: "refresh".to_string(),
            expires_at: Utc::now(),
            scope: "scope".to_string(),
        };
        
        let json = serde_json::to_string(&response).expect("Failed to serialize");
        let value: serde_json::Value = serde_json::from_str(&json).expect("Invalid JSON");
        
        // Verify camelCase field names
        assert_eq!(value["accessToken"], "access");
        assert_eq!(value["tokenType"], "Bearer");
        assert_eq!(value["expiresIn"], 3600);
        assert_eq!(value["refreshToken"], "refresh");
    }
}
