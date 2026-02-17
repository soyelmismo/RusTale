// ID Constante para permitir Key Rotation en el futuro si fuese necesario
pub const KEY_ID: &str = "rustale-host-v1";

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use ed25519_dalek::{Signer, SigningKey, VerifyingKey};
use rand::Rng;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use std::sync::{OnceLock, RwLock};

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct JwkSet {
    pub keys: Vec<Jwk>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Jwk {
    pub kty: String,   // Key Type (OKP para Ed25519)
    pub crv: String,   // Curve (Ed25519)
    pub x: String,     // Public Key (Base64URL)
    pub kid: String,   // Key ID
    #[serde(rename = "use")]
    pub use_key: String, // "sig"
}

// === MEMORIA ESTÁTICA SEGURA ===

// 1. Host Identity: Generada UNA vez al inicio. Inmutable.
// Contiene la clave Privada para firmar. Nunca puede ser None una vez inicializada.
static HOST_IDENTITY: OnceLock<SigningKey> = OnceLock::new();

// 2. Cache JWK Publico del Host: Para servir /jwks.json rapidamente.
static HOST_JWKS_CACHE: OnceLock<JwkSet> = OnceLock::new();

// 3. Claves Remotas (Opcional): Si actuamos como cliente validando otros servidores.
// Usamos RwLock porque estas SI pueden cambiar si nos conectamos a otro server.
static REMOTE_JWKS_CACHE: RwLock<Option<JwkSet>> = RwLock::new(None);

// 4. Path to identity directory (set once at initialization)
static IDENTITY_DIR: OnceLock<PathBuf> = OnceLock::new();

/// Initialize the identity directory path (must be called before using crypto functions)
pub fn set_identity_dir(path: PathBuf) -> Result<(), String> {
    if let Some(existing) = IDENTITY_DIR.get() {
        // If it's already set to the same path, it's fine (idempotent)
        if existing == &path {
            return Ok(());
        }
        return Err("Identity directory already set to a different path".to_string());
    }
    
    // Create the directory if it doesn't exist
    if let Err(e) = std::fs::create_dir_all(&path) {
        return Err(format!("Failed to create identity directory: {}", e));
    }
    
    IDENTITY_DIR.set(path).map_err(|_| "Failed to set identity directory".to_string())
}

pub fn get_identity_dir() -> PathBuf {
    IDENTITY_DIR
        .get()
        .cloned()
        .expect("Identity directory not set. Call set_identity_dir() first.")
}

/// Obtiene el JWKS publico actual (Host o Remoto).
/// Prioridad: Si hay remoto, devuelve remoto (modo cliente).
/// Si no, devuelve local (modo servidor).
pub fn get_global_jwks() -> JwkSet {
    // 1. Si tenemos llaves remotas inyectadas (modo cliente), las preferimos para validacion
    if let Ok(guard) = REMOTE_JWKS_CACHE.read() {
        if let Some(remote) = guard.as_ref() {
            return remote.clone();
        }
    }

    // 2. Si no, devolvemos las nuestras (modo servidor)
    initialize_constant_keys(); // Garantizar inicializacion
    HOST_JWKS_CACHE
        .get()
        .expect("Keys should be initialized")
        .clone()
}

pub fn get_jwks() -> JwkSet {
    get_global_jwks()
}

/// Firma un mensaje SIEMPRE con las llaves del Host.
/// Si el Host no esta inicializado, lo inicializa ahora mismo.
/// Esta funcion es ROBUSTA: No puede fallar ni regenerar claves aleatoriamente.
pub fn sign_message(message: &str) -> String {
    // Garantizar que existen claves
    initialize_constant_keys();

    // Obtener referencia segura sin bloqueos
    let key = HOST_IDENTITY
        .get()
        .expect("Host Identity failed to initialize");

    let signature = key.sign(message.as_bytes());
    URL_SAFE_NO_PAD.encode(signature.to_bytes())
}

/// Alias para claridad semantica
pub fn sign_message_with_server_keys(message: &str) -> String {
    sign_message(message)
}

pub fn get_server_public_jwk_as_value() -> serde_json::Value {
    let jwks = get_jwks();
    if let Some(key) = jwks.keys.first() {
        serde_json::json!({
            "kty": key.kty, "crv": key.crv, "x": key.x, "kid": key.kid, "use": key.use_key
        })
    } else {
        serde_json::json!({})
    }
}

pub fn get_private_jwk_as_value() -> serde_json::Value {
    // Para arquitectura descentralizada donde el cliente es su propio emisor
    // Incluye la clave privada ("d") para que el servidor pueda firmar en nombre del cliente
    initialize_constant_keys();

    let key = HOST_IDENTITY
        .get()
        .expect("Host Identity failed to initialize");
    let public_key = key.verifying_key();

    // Extraer bytes de la clave privada
    let private_bytes = key.as_bytes();
    let public_bytes = public_key.to_bytes();

    // Codificar en base64url sin padding
    let d_b64 = URL_SAFE_NO_PAD.encode(private_bytes);
    let x_b64 = URL_SAFE_NO_PAD.encode(public_bytes);

    serde_json::json!({
        "kty": "OKP",
        "crv": "Ed25519",
        "x": x_b64,
        "d": d_b64,
        "kid": KEY_ID,
        "use": "sig"
    })
}

/// Retorna la ruta al archivo de llave privada
fn get_key_file_path() -> PathBuf {
    let path = get_identity_dir().join("host.key");
    println!("[Crypto] Key file path: {:?}", path);
    path
}

/// Inicializa la identidad criptografica del servidor.
///
/// 1. Intenta leer `identity/host.key`.
/// 2. Si existe y es valida, la carga.
/// 3. Si no, genera una nueva y la guarda.
/// 4. PANIC si no tiene permisos de escritura (La seguridad es critica).
pub fn initialize_constant_keys() {
    // Asegurar que el directorio de identidad está establecido
    let identity_dir = IDENTITY_DIR.get().expect("Identity directory not set. Call set_identity_dir() first.");
    
    HOST_IDENTITY.get_or_init(|| {
        let key_path = identity_dir.join("host.key");
        println!("[Crypto] Key file path: {:?}", key_path);

        // A. INTENTO DE CARGA (Persistencia)
        if key_path.exists() {
            println!("[Crypto] Loading Host Identity from disk: {:?}", key_path);
            match fs::read(&key_path) {
                Ok(bytes) => {
                    if bytes.len() == 32 {
                        let mut arr = [0u8; 32];
                        arr.copy_from_slice(&bytes);
                        let key = SigningKey::from_bytes(&arr);

                        // Generar el cache publico inmediatamente
                        let jwk_set = create_jwk_from_public(&key.verifying_key());
                        let _ = HOST_JWKS_CACHE.set(jwk_set);

                        return key;
                    } else {
                        eprintln!(
                            "[Crypto] CRITICAL: Corrupt key file (len != 32). Backing up and regenerating."
                        );
                        let _ = fs::rename(&key_path, key_path.with_extension("key.corrupt"));
                    }
                }
                Err(e) => {
                    // Si no podemos leer la llave, es un error fatal de permisos o hardware.
                    panic!("[Crypto] FATAL: Cannot read host identity file: {}", e);
                }
            }
        }

        // B. GENERACIÓN (Primer arranque o regeneracion)
        println!("[Crypto] Generating NEW Persistent Host Identity...");

        // Crear directorios si no existen
        if !identity_dir.exists() {
            fs::create_dir_all(identity_dir).expect("Failed to create identity directory");
        }

        let mut bytes = [0u8; 32];
        rand::rng().fill(&mut bytes);
        let signing_key = SigningKey::from_bytes(&bytes);

        // Guardar estrictamente
        match fs::write(&key_path, bytes) {
            Ok(_) => println!("[Crypto] Host Identity saved to {:?}", key_path),
            Err(e) => panic!("[Crypto] FATAL: Failed to persist host identity! {}", e),
        }

        // Cachear publica
        let jwk_set = create_jwk_from_public(&signing_key.verifying_key());
        let _ = HOST_JWKS_CACHE.set(jwk_set);

        signing_key
    });
}

/// Helper para crear estructura JWKS
fn create_jwk_from_public(vk: &VerifyingKey) -> JwkSet {
    let public_bytes = vk.to_bytes();
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

pub fn update_jwks_from_remote(jwks: JwkSet) {
    if let Ok(mut guard) = REMOTE_JWKS_CACHE.write() {
        *guard = Some(jwks);
        println!("[Crypto] Synchronized with remote JWKS. Host keys preserved.");
    }
}

// Deprecated logic kept for compatibility
pub fn ensure_local_signing_capability() {
    initialize_constant_keys();
}

/// Obtiene EXCLUSIVAMENTE las llaves públicas locales del servidor emulado.
/// Ignora el caché remoto. Usado por el servidor interno para anunciar su propia identidad.
pub fn get_host_jwks() -> JwkSet {
    initialize_constant_keys();
    HOST_JWKS_CACHE
        .get()
        .expect("Host keys should be initialized")
        .clone()
}

/// Limpia el caché de llaves remotas.
/// Debe llamarse al cambiar de modo Online/Offline.
pub fn clear_remote_jwks() {
    if let Ok(mut guard) = REMOTE_JWKS_CACHE.write() {
        *guard = None;
        println!("[Crypto] Remote JWKS cache cleared.");
    }
}
