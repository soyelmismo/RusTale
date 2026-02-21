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

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::Verifier;
    use tempfile::TempDir;

    /// Helper to setup a fresh identity directory for each test
    fn setup_test_identity() -> TempDir {
        // Clear static state between tests
        // Note: OnceLock can only be set once, so we use different directories
        
        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        let identity_path = temp_dir.path().to_path_buf();
        
        // This will fail if already set, but that's OK for tests
        let _ = set_identity_dir(identity_path);
        
        temp_dir
    }

    // === JWK Structure Tests ===

    #[test]
    fn test_jwk_serialization() {
        let jwk = Jwk {
            kty: "OKP".to_string(),
            crv: "Ed25519".to_string(),
            x: "dGVzdC1wdWJsaWMta2V5".to_string(), // base64 encoded
            kid: "test-key-id".to_string(),
            use_key: "sig".to_string(),
        };

        let json = serde_json::to_string(&jwk).expect("Failed to serialize JWK");
        assert!(json.contains("OKP"));
        assert!(json.contains("Ed25519"));
        assert!(json.contains("test-key-id"));

        let deserialized: Jwk = serde_json::from_str(&json).expect("Failed to deserialize JWK");
        assert_eq!(deserialized.kty, jwk.kty);
        assert_eq!(deserialized.crv, jwk.crv);
        assert_eq!(deserialized.kid, jwk.kid);
    }

    #[test]
    fn test_jwks_serialization() {
        let jwks = JwkSet {
            keys: vec![
                Jwk {
                    kty: "OKP".to_string(),
                    crv: "Ed25519".to_string(),
                    x: "a2V5MQ".to_string(),
                    kid: "key-1".to_string(),
                    use_key: "sig".to_string(),
                },
                Jwk {
                    kty: "OKP".to_string(),
                    crv: "Ed25519".to_string(),
                    x: "a2V5Mg".to_string(),
                    kid: "key-2".to_string(),
                    use_key: "sig".to_string(),
                },
            ],
        };

        let json = serde_json::to_string(&jwks).expect("Failed to serialize JWKS");
        assert!(json.contains("\"keys\""));
        assert!(json.contains("key-1"));
        assert!(json.contains("key-2"));

        // Verify it matches expected JWKS format
        let value: serde_json::Value = serde_json::from_str(&json).expect("Invalid JSON");
        assert!(value["keys"].is_array());
        assert_eq!(value["keys"].as_array().unwrap().len(), 2);
    }

    // === Key ID Tests ===

    #[test]
    fn test_key_id_constant() {
        assert_eq!(KEY_ID, "rustale-host-v1");
        assert!(!KEY_ID.is_empty());
    }

    // === Base64 Encoding Tests ===

    #[test]
    fn test_base64_url_safe_encoding() {
        // Test that we use URL_SAFE_NO_PAD encoding
        let bytes: [u8; 32] = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15,
                               16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27, 28, 29, 30, 31];
        
        let encoded = URL_SAFE_NO_PAD.encode(bytes);
        
        // Should not contain padding characters
        assert!(!encoded.contains('='));
        // Should not contain '+' or '/' (URL unsafe chars)
        assert!(!encoded.contains('+'));
        assert!(!encoded.contains('/'));
        
        // Should be reversible
        let decoded = URL_SAFE_NO_PAD.decode(&encoded).expect("Failed to decode");
        assert_eq!(decoded.as_slice(), bytes);
    }

    // === Signature Verification Tests ===

    #[test]
    fn test_ed25519_sign_verify_roundtrip() {
        // Generate a new key pair
        let mut bytes = [0u8; 32];
        rand::rng().fill(&mut bytes);
        let signing_key = SigningKey::from_bytes(&bytes);
        let verifying_key = signing_key.verifying_key();

        // Sign a message
        let message = b"Test message for signing";
        let signature = signing_key.sign(message);
        
        // Verify with public key
        assert!(verifying_key.verify(message, &signature).is_ok());
        
        // Verify with wrong message fails
        let wrong_message = b"Wrong message";
        assert!(verifying_key.verify(wrong_message, &signature).is_err());
    }

    #[test]
    fn test_sign_message_produces_valid_base64() {
        let _temp = setup_test_identity();
        
        initialize_constant_keys();
        
        let message = "Test message";
        let signature_b64 = sign_message(message);
        
        // Should be valid base64
        let decoded = URL_SAFE_NO_PAD.decode(&signature_b64);
        assert!(decoded.is_ok(), "Signature should be valid base64");
        
        // Ed25519 signatures are 64 bytes
        assert_eq!(decoded.unwrap().len(), 64);
    }

    // === JWK Creation Tests ===

    #[test]
    fn test_create_jwk_from_public() {
        // Create a signing key
        let mut bytes = [0u8; 32];
        rand::rng().fill(&mut bytes);
        let signing_key = SigningKey::from_bytes(&bytes);
        let verifying_key = signing_key.verifying_key();

        // Create JWKS
        let jwks = create_jwk_from_public(&verifying_key);

        // Validate structure
        assert_eq!(jwks.keys.len(), 1);
        
        let jwk = &jwks.keys[0];
        assert_eq!(jwk.kty, "OKP");
        assert_eq!(jwk.crv, "Ed25519");
        assert_eq!(jwk.use_key, "sig");
        assert_eq!(jwk.kid, KEY_ID);
        
        // Verify the public key is correctly encoded
        let expected_x = URL_SAFE_NO_PAD.encode(verifying_key.to_bytes());
        assert_eq!(jwk.x, expected_x);
    }

    // === Private JWK Tests ===

    #[test]
    fn test_get_private_jwk_contains_d() {
        let _temp = setup_test_identity();
        
        initialize_constant_keys();
        
        let private_jwk = get_private_jwk_as_value();
        
        // Should contain private key component "d"
        assert!(private_jwk.get("d").is_some(), "Private JWK should contain 'd' component");
        assert!(private_jwk.get("x").is_some(), "Private JWK should contain 'x' component");
        assert_eq!(private_jwk["kty"], "OKP");
        assert_eq!(private_jwk["crv"], "Ed25519");
    }

    // === Remote JWKS Cache Tests ===

    #[test]
    fn test_update_jwks_from_remote() {
        let _temp = setup_test_identity();
        
        let remote_jwks = JwkSet {
            keys: vec![Jwk {
                kty: "OKP".to_string(),
                crv: "Ed25519".to_string(),
                x: "cmVtb3RlLXB1YmxpYy1rZXk".to_string(),
                kid: "remote-key".to_string(),
                use_key: "sig".to_string(),
            }],
        };

        update_jwks_from_remote(remote_jwks.clone());

        // get_global_jwks should now return the remote JWKS
        let retrieved = get_global_jwks();
        assert_eq!(retrieved.keys.len(), 1);
        assert_eq!(retrieved.keys[0].kid, "remote-key");

        // Clean up
        clear_remote_jwks();
    }

    #[test]
    fn test_clear_remote_jwks() {
        let _temp = setup_test_identity();
        
        // First set a remote JWKS
        let remote_jwks = JwkSet {
            keys: vec![Jwk {
                kty: "OKP".to_string(),
                crv: "Ed25519".to_string(),
                x: "dGVzdA".to_string(),
                kid: "to-clear".to_string(),
                use_key: "sig".to_string(),
            }],
        };
        
        update_jwks_from_remote(remote_jwks);
        
        // Clear it
        clear_remote_jwks();
        
        // Verify it's cleared (should fall back to host keys)
        // This test verifies no panic occurs
        let _jwks = get_global_jwks();
    }

    // === Key Persistence Tests ===

    #[test]
    fn test_key_persistence_roundtrip() {
        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        let identity_path = temp_dir.path().to_path_buf();
        
        // Reset identity dir (this test expects fresh state)
        // Since OnceLock can't be reset, we verify file I/O logic directly
        
        // Create a test key
        let mut bytes = [0u8; 32];
        rand::rng().fill(&mut bytes);
        let signing_key = SigningKey::from_bytes(&bytes);
        
        // Write to file
        let key_path = identity_path.join("host.key");
        std::fs::write(&key_path, bytes).expect("Failed to write key");
        
        // Read back
        let read_bytes = std::fs::read(&key_path).expect("Failed to read key");
        assert_eq!(read_bytes.len(), 32);
        assert_eq!(read_bytes.as_slice(), bytes);
        
        // Verify key can be reconstructed
        let mut arr = [0u8; 32];
        arr.copy_from_slice(&read_bytes);
        let reconstructed = SigningKey::from_bytes(&arr);
        assert_eq!(reconstructed.verifying_key(), signing_key.verifying_key());
    }

    // === Signature Integration Tests ===

    #[test]
    fn test_sign_and_verify_integration() {
        let _temp = setup_test_identity();
        
        initialize_constant_keys();
        
        // Sign a message
        let message = "Integration test message";
        let signature_b64 = sign_message(message);
        
        // Decode signature
        let sig_bytes = URL_SAFE_NO_PAD
            .decode(&signature_b64)
            .expect("Failed to decode signature");
        
        // Convert to signature type
        let sig_arr: [u8; 64] = sig_bytes.try_into().expect("Invalid signature length");
        let signature = ed25519_dalek::Signature::from_bytes(&sig_arr);
        
        // Get public key for verification
        let jwks = get_host_jwks();
        let jwk = &jwks.keys[0];
        
        // Decode public key
        let pk_bytes = URL_SAFE_NO_PAD
            .decode(&jwk.x)
            .expect("Failed to decode public key");
        let pk_arr: [u8; 32] = pk_bytes.try_into().expect("Invalid public key length");
        let verifying_key = VerifyingKey::from_bytes(&pk_arr).expect("Invalid public key");
        
        // Verify signature
        let result = verifying_key.verify(message.as_bytes(), &signature);
        assert!(result.is_ok(), "Signature verification should succeed");
    }

    // === Malformed Input Tests ===

    #[test]
    fn test_malformed_base64_rejected() {
        let invalid_b64 = "not-valid-base64!!!";
        let result = URL_SAFE_NO_PAD.decode(invalid_b64);
        assert!(result.is_err());
    }

    #[test]
    fn test_wrong_length_signature_rejected() {
        let wrong_len_bytes = vec![0u8; 32]; // 32 bytes instead of 64
        let result: Result<ed25519_dalek::Signature, _> = wrong_len_bytes.as_slice().try_into();
        assert!(result.is_err());
    }

    #[test]
    fn test_wrong_length_public_key_rejected() {
        let wrong_len_bytes = vec![0u8; 16]; // 16 bytes instead of 32
        // try_into should fail because 16 bytes cannot convert to [u8; 32]
        let result: Result<[u8; 32], _> = wrong_len_bytes.as_slice().try_into();
        assert!(result.is_err());
    }
}
