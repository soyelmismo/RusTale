use base64::{
    Engine as _,
    engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD},
};
use ed25519_dalek::{Signer, SigningKey};
use once_cell::sync::Lazy;
use rand::Rng;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;

// Identificador de clave usado por el protocolo Hytale
pub const KEY_ID: &str = "2025-10-01";

#[derive(Serialize)]
pub struct JwkSet {
    keys: Vec<Jwk>,
}

#[derive(Serialize)]
pub struct Jwk {
    kty: String, // Key Type (OKP para Ed25519)
    crv: String, // Curve (Ed25519)
    x: String,   // Public Key (Base64URL)
    kid: String, // Key ID
    #[serde(rename = "use")]
    use_key: String, // "sig"
}
pub fn get_jwks() -> JwkSet {
    let lock = KEY_PAIR.lock().unwrap();
    let verifying_key = lock.verifying_key();
    let public_bytes = verifying_key.to_bytes();

    // Codificación Base64 URL-Safe sin Padding (Requisito RFC 8037)
    let x_b64 = URL_SAFE_NO_PAD.encode(public_bytes);

    JwkSet {
        keys: vec![Jwk {
            kty: "OKP".to_string(),
            crv: "Ed25519".to_string(),
            x: x_b64,
            kid: KEY_ID.to_string(),
            use_key: "sig".to_string(),
        }],
    }
}

#[derive(Serialize, Deserialize)]
struct KeyPairData {
    #[serde(rename = "privateKey")]
    pub private_key: String,
    #[serde(rename = "publicKey")]
    pub public_key: String,
    #[serde(rename = "createdAt")]
    pub created_at: String,
}

pub static KEY_PAIR: Lazy<Mutex<SigningKey>> = Lazy::new(|| {
    let path = crate::config::get_identity_dir().join("jwt_keys.json");
    Mutex::new(load_or_generate_keys(&path))
});

fn load_or_generate_keys(path: &PathBuf) -> SigningKey {
    if path.exists() {
        if let Ok(content) = fs::read_to_string(path) {
            // Intentar formato Node.js (camelCase)
            if let Ok(data) = serde_json::from_str::<KeyPairData>(&content) {
                if let Ok(key) = parse_private_key(&data.private_key) {
                    println!("[Crypto] Claves cargadas (Formato Node.js)");
                    return key;
                }
            }
        }
        println!("[Crypto] Claves corruptas o ilegibles. Generando nuevas.");
    }

    // SOLUCION AL ERROR: Generar bytes manualmente para evitar conflicto de versiones de rand
    let mut bytes = [0u8; 32];
    rand::rng().fill(&mut bytes);
    let signing_key = SigningKey::from_bytes(&bytes);

    save_keys(path, &signing_key);
    signing_key
}

/// Parsea una clave privada Ed25519 desde Base64.
/// Soporta tanto formato RAW (32 bytes) como PKCS8 DER (generado por Node crypto).
fn parse_private_key(base64_str: &str) -> Result<SigningKey, ()> {
    let bytes = STANDARD.decode(base64_str).map_err(|_| ())?;

    // Caso 1: Raw Bytes (32 bytes)
    if bytes.len() == 32 {
        let array: [u8; 32] = bytes.try_into().map_err(|_| ())?;
        return Ok(SigningKey::from_bytes(&array));
    }

    // Caso 2: PKCS8 DER (~48 bytes).
    // Node.js crypto.generateKeyPairSync('ed25519') envuelve la clave en una estructura ASN.1.
    // Para Ed25519, la clave privada son los ultimos 32 bytes de la estructura Octet String.
    if bytes.len() > 32 {
        if let Some(raw_key) = bytes.get(bytes.len() - 32..) {
            let array: [u8; 32] = raw_key.try_into().map_err(|_| ())?;
            return Ok(SigningKey::from_bytes(&array));
        }
    }

    Err(())
}

fn save_keys(path: &PathBuf, key: &SigningKey) {
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }

    let verifying_key = key.verifying_key();

    // Guardamos en formato compatible con Node.js (JSON + Base64 Standard)
    let data = KeyPairData {
        private_key: STANDARD.encode(key.to_bytes()), // Guardamos RAW para facilitar lectura futura
        public_key: STANDARD.encode(verifying_key.to_bytes()),
        created_at: chrono::Utc::now().to_rfc3339(),
    };

    if let Ok(json) = serde_json::to_string_pretty(&data) {
        let _ = fs::write(path, json);
        println!("[Crypto] Nuevas claves guardadas en {:?}", path);
    }
}

pub fn sign_message(message: &str) -> String {
    let lock = KEY_PAIR.lock().unwrap();
    let signature = lock.sign(message.as_bytes());
    URL_SAFE_NO_PAD.encode(signature.to_bytes())
}
