use anyhow::Result;
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use rand::RngCore;
use sha2::{Digest, Sha256};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OAuthTokens {
    pub access_token: String,
    pub id_token: Option<String>, // Often returned in OIDC, but Device Code flow might just return access_token
    pub refresh_token: Option<String>,
    pub expires_in: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HytaleProfile {
    pub uuid: String,
    pub username: String,
}

#[derive(Debug, Clone)]
pub struct OAuthSuccess {
    pub tokens: OAuthTokens,
    pub profile: Option<HytaleProfile>,
}

pub fn generate_pkce() -> (rustale_security::memory::SafeString, String) {
    let mut verifier_bytes = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut verifier_bytes);
    let verifier = URL_SAFE_NO_PAD.encode(verifier_bytes);

    let mut hasher = Sha256::new();
    hasher.update(verifier.as_bytes());
    let challenge_bytes = hasher.finalize();
    let challenge = URL_SAFE_NO_PAD.encode(challenge_bytes);

    (rustale_security::memory::SafeString::new(verifier), challenge)
}

pub async fn run_client_oauth_flow(
    issuer: &str,
    client_id: &str,
) -> Result<OAuthSuccess> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;
    
    let (verifier, challenge) = generate_pkce();
    let state = uuid::Uuid::new_v4().to_string();

    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let port = listener.local_addr()?.port();
    let redirect_uri = format!("http://127.0.0.1:{}/callback", port);

    let auth_url = format!(
        "{}/oauth2/auth?response_type=code&client_id={}&redirect_uri={}&scope=openid%20hytale:profile&state={}&code_challenge={}&code_challenge_method=S256",
        issuer,
        client_id,
        urlencoding::encode(&redirect_uri),
        state,
        challenge
    );

    // Open browser (handled gracefully if it fails)
    let _ = open::that(&auth_url);

    // Wait for callback
    let (mut stream, _) = listener.accept().await?;
    let mut reader = tokio::io::BufReader::new(&mut stream);
    let mut first_line = String::new();
    let _ = tokio::io::AsyncBufReadExt::read_line(&mut reader, &mut first_line).await?;

    let mut code = None;
    let mut received_state = None;
    let mut error_msg = None;

    if first_line.starts_with("GET ") {
        let parts: Vec<&str> = first_line.split_whitespace().collect();
        if parts.len() > 1 {
            let url = parts[1];
            if let Some(query) = url.split('?').nth(1) {
                for pair in query.split('&') {
                    let mut kv = pair.split('=');
                    if let (Some(k), Some(v)) = (kv.next(), kv.next()) {
                        if k == "code" {
                            code = Some(v.to_string());
                        } else if k == "state" {
                            received_state = Some(v.to_string());
                        } else if k == "error" {
                            error_msg = Some(v.to_string());
                        } else if k == "error_description" {
                            if let Some(err) = &mut error_msg {
                                *err = format!("{}: {}", err, urlencoding::decode(v).unwrap_or_default());
                            }
                        }
                    }
                }
            }
        }
    }

    let response_body = if error_msg.is_none() && code.is_some() && received_state.as_deref() == Some(state.as_str()) {
        "<html><body><h1>Success!</h1><p>You can close this window and return to the application.</p><script>window.close();</script></body></html>"
    } else {
        "<html><body><h1>Authentication Failed</h1><p>Please close this window and try again.</p></body></html>"
    };

    let response = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        response_body.len(),
        response_body
    );
    let _ = stream.write_all(response.as_bytes()).await;
    let _ = stream.flush().await;

    if let Some(err) = error_msg {
        return Err(anyhow::anyhow!("OAuth Error: {}", err));
    }

    if received_state.as_deref() != Some(state.as_str()) {
        return Err(anyhow::anyhow!("State mismatch"));
    }

    let code = code.ok_or_else(|| anyhow::anyhow!("No code received"))?;

    // Exchange token
    let client = crate::network::HTTP_CLIENT.clone();
    let params = [
        ("grant_type", "authorization_code"),
        ("code", &code),
        ("redirect_uri", &redirect_uri),
        ("code_verifier", verifier.as_str()),
        ("client_id", client_id),
    ];

    let token_url = format!("{}/oauth2/token", issuer);
    let token_res = client
        .post(&token_url)
        .form(&params)
        .send()
        .await?;

    if !token_res.status().is_success() {
        let err_text = token_res.text().await.unwrap_or_default();
        return Err(anyhow::anyhow!("Token exchange failed: {}", err_text));
    }

    let tokens: OAuthTokens = token_res.json().await?;

    // Get UserInfo
    let user_url = format!("{}/userinfo", issuer);
    let user_res = client
        .get(&user_url)
        .header("Authorization", format!("Bearer {}", tokens.access_token))
        .send()
        .await?;

    let mut profile = None;
    if user_res.status().is_success() {
        if let Ok(data) = user_res.json::<serde_json::Value>().await {
            if let Some(prof) = data.get("profile") {
                if let (Some(uuid), Some(username)) = (prof.get("uuid").and_then(|v| v.as_str()), prof.get("username").and_then(|v| v.as_str())) {
                    profile = Some(HytaleProfile {
                        uuid: uuid.to_string(),
                        username: username.to_string(),
                    });
                }
            }
        }
    }

    Ok(OAuthSuccess { tokens, profile })
}

#[derive(Deserialize)]
struct DeviceAuthResponse {
    device_code: String,
    user_code: String,
    verification_uri: Option<String>,
    verification_uri_complete: Option<String>,
    expires_in: u64,
    interval: Option<u64>,
}

#[derive(Deserialize)]
struct TokenErrorResponse {
    error: String,
}

pub async fn run_server_device_code_flow(
    issuer: &str,
    client_id: &str,
) -> Result<OAuthSuccess> {
    let client = crate::network::HTTP_CLIENT.clone();
    
    // Step 1: Request Device Code
    let device_auth_url = format!("{}/oauth2/device/auth", issuer);
    let params = [
        ("client_id", client_id),
        ("scope", "openid offline auth:server"),
    ];
    
    let res = client.post(&device_auth_url).form(&params).send().await?;
    if !res.status().is_success() {
        return Err(anyhow::anyhow!("Failed to initiate device auth: {}", res.text().await.unwrap_or_default()));
    }
    
    let device_res: DeviceAuthResponse = res.json().await?;
    
    let url = device_res.verification_uri_complete
        .or(device_res.verification_uri)
        .unwrap_or_else(|| "https://accounts.hytale.com/device".to_string());
        
    println!("===================================================================");
    println!("DEVICE AUTHORIZATION");
    println!("===================================================================");
    println!("Visit: {}", url);
    println!("Enter code: {}", device_res.user_code);
    println!("===================================================================");
    println!("Waiting for authorization (expires in {} seconds)...", device_res.expires_in);
    
    let interval = device_res.interval.unwrap_or(5).max(1);
    let token_url = format!("{}/oauth2/token", issuer);
    
    let mut tokens = None;
    
    let max_attempts = (device_res.expires_in / interval).max(1);
    for _ in 0..max_attempts {
        tokio::time::sleep(tokio::time::Duration::from_secs(interval)).await;
        
        let token_params = [
            ("client_id", client_id),
            ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
            ("device_code", &device_res.device_code),
        ];
        
        let token_res = client.post(&token_url).form(&token_params).send().await?;
        
        if token_res.status().is_success() {
            tokens = Some(token_res.json::<OAuthTokens>().await?);
            break;
        } else {
            let err_res: TokenErrorResponse = token_res.json().await.unwrap_or(TokenErrorResponse { error: "unknown".to_string() });
            if err_res.error != "authorization_pending" {
                return Err(anyhow::anyhow!("Device auth failed: {}", err_res.error));
            }
        }
    }
    
    let tokens = tokens.ok_or_else(|| anyhow::anyhow!("Authorization timed out"))?;
    println!("> Authentication successful! Mode: OAUTH_DEVICE");
    
    Ok(OAuthSuccess { tokens, profile: None })
}

pub async fn refresh_oauth_tokens(
    issuer: &str,
    client_id: &str,
    refresh_token: &str,
) -> Result<OAuthTokens> {
    let client = crate::network::HTTP_CLIENT.clone();
    let token_url = format!("{}/oauth2/token", issuer);
    
    let params = [
        ("grant_type", "refresh_token"),
        ("client_id", client_id),
        ("refresh_token", refresh_token),
    ];
    
    let res = client.post(&token_url).form(&params).send().await?;
    
    if !res.status().is_success() {
        let err_text = res.text().await.unwrap_or_default();
        return Err(anyhow::anyhow!("OAuth refresh failed: {}", err_text));
    }
    
    let new_tokens: OAuthTokens = res.json().await?;
    Ok(new_tokens)
}
