use anyhow::Result;
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use rand::RngCore;
use sha2::{Digest, Sha256};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
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
    rand::rng().fill_bytes(&mut verifier_bytes);
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
    let (verifier, challenge) = generate_pkce();
    let state = uuid::Uuid::new_v4().to_string();

    let redirect_uri = "http://127.0.0.1:41234/callback".to_string();

    let auth_url = format!(
        "{}/oauth2/auth?response_type=code&client_id={}&redirect_uri={}&scope=openid%20hytale:profile&state={}&code_challenge={}&code_challenge_method=S256",
        issuer,
        client_id,
        urlencoding::encode(&redirect_uri),
        state,
        challenge
    );

    let (tx, rx) = tokio::sync::oneshot::channel::<Result<String>>();

    // Mover a un thread dedicado para la interfaz de wry/tao
    std::thread::spawn(move || {
        use tao::event_loop::{ControlFlow, EventLoopBuilder};
        use tao::window::WindowBuilder;
        use tao::event::{Event, WindowEvent};
        use wry::WebViewBuilder;
        use directories::ProjectDirs;

        enum WebViewEvent {
            ShowWindow,
            CloseWindow,
        }

        let event_loop = EventLoopBuilder::<WebViewEvent>::with_user_event().build();
        let proxy = event_loop.create_proxy();
        
        let window = WindowBuilder::new()
            .with_title("RusTale Authentication")
            .with_inner_size(tao::dpi::LogicalSize::new(500.0, 600.0))
            .with_visible(false) // Inicialmente oculta
            .build(&event_loop)
            .unwrap();

        let data_dir = ProjectDirs::from("com", "rustale", "RustaleLauncher")
            .map(|p| p.data_local_dir().join("webview"))
            .unwrap_or_else(|| std::path::PathBuf::from(".webview_data"));

        let script = r#"
            window.addEventListener('DOMContentLoaded', () => {
                let url = window.location.href.toLowerCase();
                let html = document.body.innerHTML.toLowerCase();
                
                // Mostrar ventana si estamos en una pantalla que requiere interacción manual
                if (url.includes('/login') || html.includes('sign in with') || html.includes('password')) {
                    window.ipc.postMessage('SHOW_WINDOW');
                }
                
                // Si estamos en la pantalla de "Authorize" (consentimiento), damos click automático
                if (url.includes('/oauth') || url.includes('consent') || html.includes('authorize')) {
                    let btn = document.querySelector('button[type="submit"], button.primary, .btn-primary, [name="accept"]');
                    if (btn && !url.includes('/login')) {
                        btn.click();
                    } else if (!url.includes('/login')) {
                        // Failsafe por si cambia el DOM del consent screen
                        window.ipc.postMessage('SHOW_WINDOW');
                    }
                }
                
                // Failsafe global: si el proceso se estanca en cualquier pantalla por 4 segundos, mostrarla
                setTimeout(() => window.ipc.postMessage('SHOW_WINDOW'), 4000);
            });
        "#;

        let mut tx_opt = Some(tx);
        let proxy_nav = proxy.clone();
        
        let nav_handler = move |url: String| -> bool {
            if url.starts_with("http://127.0.0.1:41234/callback") {
                if let Some(query) = url.split('?').nth(1) {
                    let mut code = None;
                    let mut err = None;
                    for pair in query.split('&') {
                        let mut kv = pair.split('=');
                        if let (Some(k), Some(v)) = (kv.next(), kv.next()) {
                            if k == "code" { code = Some(v.to_string()); }
                            if k == "error" { err = Some(v.to_string()); }
                        }
                    }
                    if let Some(tx) = tx_opt.take() {
                        if let Some(e) = err {
                            let _ = tx.send(Err(anyhow::anyhow!("OAuth error: {}", e)));
                        } else if let Some(c) = code {
                            let _ = tx.send(Ok(c));
                        } else {
                            let _ = tx.send(Err(anyhow::anyhow!("No code or error in redirect")));
                        }
                    }
                }
                let _ = proxy_nav.send_event(WebViewEvent::CloseWindow);
                false
            } else {
                true
            }
        };

        let proxy_ipc = proxy.clone();
        let ipc_handler = move |req: wry::http::Request<String>| {
            let msg = req.into_body();
            if msg == "SHOW_WINDOW" {
                let _ = proxy_ipc.send_event(WebViewEvent::ShowWindow);
            }
        };

        let mut web_context = wry::WebContext::new(Some(data_dir));

        let _webview = WebViewBuilder::new()
            .with_url(&auth_url)
            .with_web_context(&mut web_context)
            .with_initialization_script(script)
            .with_ipc_handler(ipc_handler)
            .with_navigation_handler(nav_handler)
            .build(&window)
            .unwrap();

        event_loop.run(move |event, _, control_flow| {
            *control_flow = ControlFlow::Wait;

            match event {
                Event::UserEvent(WebViewEvent::CloseWindow) => {
                    *control_flow = ControlFlow::Exit;
                }
                Event::UserEvent(WebViewEvent::ShowWindow) => {
                    window.set_visible(true);
                    window.set_focus();
                }
                Event::WindowEvent { event: WindowEvent::CloseRequested, .. } => {
                    *control_flow = ControlFlow::Exit;
                }
                _ => {}
            }
        });
    });

    let code = match rx.await {
        Ok(Ok(c)) => c,
        Ok(Err(e)) => return Err(e),
        Err(_) => return Err(anyhow::anyhow!("WebView thread panicked or closed")),
    };

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
