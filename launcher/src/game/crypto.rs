use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use ed25519_dalek::{Signer, SigningKey};
use rand::Rng;
use serde::{Deserialize, Serialize};
use std::sync::Mutex;

// Identificador de clave usado por el protocolo Hytale
pub const KEY_ID: &str = "2025-10-01";

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct JwkSet {
    pub keys: Vec<Jwk>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Jwk {
    pub kty: String, // Key Type (OKP para Ed25519)
    pub crv: String, // Curve (Ed25519)
    pub x: String,   // Public Key (Base64URL)
    pub kid: String, // Key ID
    #[serde(rename = "use")]
    pub use_key: String, // "sig"
}

// Estado global en RAM (ya no se guarda en HDD)
static SESSION_KEYS: Mutex<Option<SigningKey>> = Mutex::new(None);
static SESSION_JWKS: Mutex<Option<JwkSet>> = Mutex::new(None);

/// Obtiene el JWKS global actual. Si no existen, las inicializa una única vez.
pub fn get_global_jwks() -> JwkSet {
    let mut jwks_lock = SESSION_JWKS.lock().unwrap();
    if let Some(jwks) = &*jwks_lock {
        return jwks.clone();
    }

    // Si no existen, generarlas por primera vez (Lazy Init)
    drop(jwks_lock);
    initialize_constant_keys();
    SESSION_JWKS.lock().unwrap().as_ref().unwrap().clone()
}

pub fn get_jwks() -> JwkSet {
    get_global_jwks()
}

/// Firma un mensaje usando las claves de sesión actuales
pub fn sign_message(message: &str) -> String {
    let mut key_lock = SESSION_KEYS.lock().unwrap();

    // Asegurar que las claves existan
    if key_lock.is_none() {
        // If we have JWKS but no keys, it means we are a client of a remote emulator.
        // We CANNOT sign messages because we don't have the private key.
        let jwks_lock = SESSION_JWKS.lock().unwrap();
        if jwks_lock.is_some() {
            eprintln!(
                "[Crypto] Error: Attempted to sign message but we only have remote public keys. This token will be invalid!"
            );
            return "INVALID_SIGNATURE_REMOTE_ONLY".to_string();
        }
        drop(jwks_lock);

        drop(key_lock);
        force_regenerate_keys();
        key_lock = SESSION_KEYS.lock().unwrap();
    }

    if let Some(key) = key_lock.as_ref() {
        let signature = key.sign(message.as_bytes());
        URL_SAFE_NO_PAD.encode(signature.to_bytes())
    } else {
        "ERROR_NO_KEY".to_string()
    }
}

/// Firma un mensaje con las claves del servidor (unificado con sesión en RAM)
pub fn sign_message_with_server_keys(message: &str) -> String {
    sign_message(message)
}

/// Retorna el JWK público actual como un Value de serde_json
pub fn get_public_jwk_as_value() -> serde_json::Value {
    let jwks = get_jwks();
    if let Some(key) = jwks.keys.first() {
        serde_json::json!({
            "kty": key.kty,
            "crv": key.crv,
            "x": key.x,
            "kid": key.kid,
            "use": key.use_key
        })
    } else {
        // Fallback (no debería ocurrir)
        serde_json::json!({})
    }
}

pub fn get_server_public_jwk_as_value() -> serde_json::Value {
    get_public_jwk_as_value()
}

/// Fuerza la regeneración de claves JWT (Solo en RAM)
/// Se llama en cada inicio de juego o servidor
pub fn force_regenerate_keys() {
    let mut key_lock = SESSION_KEYS.lock().unwrap();
    let mut jwks_lock = SESSION_JWKS.lock().unwrap();

    // WARNING: If we already have keys or JWKS from a remote, do NOT regenerate
    // unless explicitly asked (which we don't have an API for yet).
    if key_lock.is_some() || jwks_lock.is_some() {
        return;
    }

    // Generar bytes aleatorios para la clave privada
    let mut bytes = [0u8; 32];
    rand::rng().fill(&mut bytes);
    let signing_key = SigningKey::from_bytes(&bytes);

    // Derivar clave pública y formatear como JWK
    let verifying_key = signing_key.verifying_key();
    let public_bytes = verifying_key.to_bytes();
    let x_b64 = URL_SAFE_NO_PAD.encode(public_bytes);

    let jwks = JwkSet {
        keys: vec![Jwk {
            kty: "OKP".to_string(),
            crv: "Ed25519".to_string(),
            x: x_b64,
            kid: KEY_ID.to_string(),
            use_key: "sig".to_string(),
        }],
    };

    *key_lock = Some(signing_key);
    *jwks_lock = Some(jwks);

    println!("[Crypto] Fresh JWT keys generated and active in RAM.");
}

/// Actualiza el JWKS desde una fuente remota (usado al unirse a servidores dedicados)
pub fn update_jwks_from_remote(jwks: JwkSet) {
    let mut jwks_lock = SESSION_JWKS.lock().unwrap();
    let mut key_lock = SESSION_KEYS.lock().unwrap();

    *jwks_lock = Some(jwks);
    *key_lock = None; // Importante: invalidar llaves privadas locales si somos clientes

    println!("[Crypto] JWKS updated from remote server (Cloned)");
}

/// Inicializa claves si no existen
pub fn initialize_constant_keys() {
    let mut key_lock = SESSION_KEYS.lock().unwrap();
    if key_lock.is_none() {
        drop(key_lock);
        force_regenerate_keys();
    }
}

/// Mantenido por compatibilidad
pub fn initialize_server_keys_if_local() {
    initialize_constant_keys();
}
